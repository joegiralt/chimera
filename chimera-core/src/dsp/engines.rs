//! A voice's engine: one slot holding the engine it plays, rebuilt in
//! place when it plays another (ADR 0051, superseding 0008). The variant
//! is the engine; dispatch is one exhaustive `match` per method. Adding an
//! engine = one variant here; the compiler lists the arms.

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};
use crate::dsp::modal::{Halo, ModalEngine, ModalParams, Model, ResonatorMode, SymPool};
use crate::hw::Cost;
use crate::in_place::{in_place_enum, move_out};
use crate::modulation::ModState;
use crate::params::{EngineType, ParamSnapshot, PitchParams, Steal};
use crate::sym_alloc::{Lease, SymAlloc};
use crate::voice_alloc::VoiceIdx;
use crate::{MidiNote, Velocity};

/// What a slot holds: the engine and, for Modal, its model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Algo,
    Modal(ResonatorMode),
}

impl SlotKind {
    /// The kind `p` plays: its engine, then Modal's MODE.
    pub fn of(p: &ParamSnapshot) -> Self {
        match p.engine() {
            EngineType::Algo => Self::Algo,
            EngineType::Modal => Self::Modal(p.modal.mode),
        }
    }

    pub fn engine(self) -> EngineType {
        match self {
            Self::Algo => EngineType::Algo,
            Self::Modal(_) => EngineType::Modal,
        }
    }

    /// What an idle voice holds for this kind: never a pool slot.
    /// Sympathetic rests as String, its main string without a halo.
    pub fn resting(self) -> Self {
        match self {
            Self::Modal(ResonatorMode::Sympathetic) => Self::Modal(ResonatorMode::String),
            other => other,
        }
    }
}

in_place_enum! {
    /// The one engine a voice holds: the largest engine plus a tag, never
    /// the sum.
    ///
    /// A variant is only built in place (`init_in_place`, `rebuild`), never
    /// by value and moved in (ADR 0008): its payload is sealed.
    ///
    /// ```compile_fail,E0308
    /// use chimera_core::dsp::engines::EngineSlot;
    /// use chimera_core::dsp::modal::{ModalEngine, ResonatorMode, SymPool};
    /// let e = ModalEngine::new_in(&mut SymPool::boxed(), ResonatorMode::Modal);
    /// let _ = EngineSlot::Modal(e);
    /// ```
    ///
    /// Nor can the seal be named, to wrap an engine in it:
    ///
    /// ```compile_fail,E0603
    /// use chimera_core::dsp::modal::{ModalEngine, ResonatorMode, SymPool};
    /// let e = ModalEngine::new_in(&mut SymPool::boxed(), ResonatorMode::Modal);
    /// let _ = chimera_core::in_place::Sealed(e);
    /// ```
    #[expect(
        clippy::large_enum_variant,
        reason = "one slot per voice, built in place: boxing needs a heap"
    )]
    pub enum EngineSlot {
        Algo(AlgoEngine) => rebuild_algo, init_algo;
        Modal(ModalEngine) => rebuild_modal, init_modal;
    }
}

impl EngineSlot {
    /// By value, through the stack: tests only. A voice builds its slot in
    /// place (`init_in_place`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(kind: SlotKind) -> Self {
        // SAFETY: `init_in_place` writes the whole slot.
        unsafe { crate::in_place::by_value(|slot| Self::init_in_place(slot, kind)) }
    }

    /// `kind.resting()`, in place: a slot is built holding no lease.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, kind: SlotKind) -> &mut Self {
        match kind.resting() {
            // SAFETY: `AlgoEngine::init_in_place` writes every field.
            SlotKind::Algo => unsafe { Self::init_algo(slot, AlgoEngine::init_in_place) },
            // SAFETY: `ModalEngine::init_in_place` writes every field.
            SlotKind::Modal(mode) => unsafe {
                Self::init_modal(slot, |m| {
                    ModalEngine::init_in_place(m, Model::resting(mode))
                })
            },
        }
    }

    pub fn kind(&self) -> SlotKind {
        match self {
            Self::Algo(_) => SlotKind::Algo,
            Self::Modal(m) => SlotKind::Modal(m.mode()),
        }
    }

    /// The engine's output gain at the VCA (ADR 0058): Algo is the reference.
    pub fn out_gain(&self) -> f32 {
        match self {
            Self::Algo(_) => 1.0,
            Self::Modal(m) => m.out_gain(),
        }
    }

    /// An idle `kind`, fresh, in place: no stack copy of an engine. The
    /// one place a lease moves (exclusive-state spec § 4.4):
    ///
    /// | From | Into | Halo |
    /// |---|---|---|
    /// | Sympathetic, `Full` | Sympathetic | kept |
    /// | Sympathetic, `Full` | another kind | given back |
    /// | anything else | Sympathetic | `pool.lend(voice)`: `Full` if a slot is promised to it, else `Bare` |
    /// | anything else | another kind | none |
    pub fn rebuild(&mut self, kind: SlotKind, pool: &mut SymAlloc, voice: VoiceIdx) {
        let sympathetic = kind == SlotKind::Modal(ResonatorMode::Sympathetic);
        let old = match self {
            // SAFETY: the payload is rebuilt below in every arm, and
            // `rebuild_*` aborts rather than unwinds.
            Self::Modal(m) => m.lease_mut().map(|l| unsafe { move_out(l) }),
            Self::Algo(_) => None,
        };
        // Nothing between the move and the rebuild can panic: a lend only
        // when there was no old lease, a give-back only after.
        let (lease, back) = match old {
            Some(l) if sympathetic => (Some(l), None),
            Some(l) => (None, Some(l)),
            None if sympathetic => (pool.lend(voice), None),
            None => (None, None),
        };
        self.build(kind, lease);
        if let Some(l) = back {
            pool.give_back(l);
        }
    }

    /// `kind`; Sympathetic with `lease`'s halo, or bare without one.
    fn build(&mut self, kind: SlotKind, lease: Option<Lease>) {
        match kind {
            // SAFETY: `AlgoEngine::init_in_place` writes every field.
            SlotKind::Algo => unsafe { self.rebuild_algo(AlgoEngine::init_in_place) },
            // SAFETY: `ModalEngine::init_in_place` writes every field.
            SlotKind::Modal(ResonatorMode::Sympathetic) => unsafe {
                let halo = lease.map_or(Halo::Bare, Halo::Full);
                self.rebuild_modal(|m| ModalEngine::init_in_place(m, Model::Sympathetic(halo)))
            },
            // SAFETY: `ModalEngine::init_in_place` writes every field.
            SlotKind::Modal(mode) => unsafe {
                debug_assert!(lease.is_none());
                self.rebuild_modal(|m| ModalEngine::init_in_place(m, Model::resting(mode)))
            },
        }
    }

    /// Sympathetic with a halo: it holds a lease.
    pub fn rings(&self) -> bool {
        matches!(self, Self::Modal(m) if !m.is_bare() && m.mode() == ResonatorMode::Sympathetic)
    }

    /// Sympathetic without a halo: its note plays the main string alone.
    pub fn is_bare(&self) -> bool {
        matches!(self, Self::Modal(m) if m.is_bare())
    }

    /// The bytes a Sympathetic note-on of `note` on `voice` clears
    /// (`SymPool::note_on_clear`), or writes as it adds to what rings, a
    /// re-strike or, at `glide`, a glide (`ModalEngine::strike_clear`).
    pub fn sym_note_on_clear(
        &self,
        pool: &SymPool,
        voice: VoiceIdx,
        (note, glide, sample_rate): (MidiNote, bool, u32),
    ) -> usize {
        match self {
            Self::Modal(m) => m
                .strike_clear(note.get(), glide, sample_rate, pool)
                .unwrap_or_else(|| pool.note_on_clear(Some(m), voice)),
            Self::Algo(_) => pool.note_on_clear(None, voice),
        }
    }

    /// `p` must play this slot's engine. At STEAL GLIDE a note on a
    /// sounding engine, another note of its Part stealing it, glides it
    /// there (#254, ADR 0065).
    pub fn note_on(
        &mut self,
        note: MidiNote,
        vel: Velocity,
        p: &ParamSnapshot,
        sample_rate: u32,
        pool: &mut SymPool,
    ) {
        debug_assert_eq!(p.engine(), self.kind().engine());
        self.set_pitch(p);
        match (self, p.pitch.steal_glide()) {
            (Self::Algo(a), None) => a.note_on(note, vel, &p.algo, sample_rate),
            (Self::Algo(a), Some(tau)) => a.glide_on(note, vel, &p.algo, sample_rate, tau),
            (Self::Modal(m), None) => m.note_on(note.get(), vel.get(), &p.modal, sample_rate, pool),
            (Self::Modal(m), Some(tau)) => {
                m.glide_on(note.get(), vel.get(), &p.modal, (sample_rate, tau), pool)
            }
        }
    }

    /// A glide steal's ratio now, 1 at rest: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        match self {
            Self::Algo(a) => a.slide(),
            Self::Modal(m) => m.slide(),
        }
    }

    pub fn note_off(&mut self, pool: &mut SymPool) {
        match self {
            Self::Algo(a) => a.note_off(),
            Self::Modal(m) => m.note_off(pool),
        }
    }

    /// Render one block of raw engine output from (possibly modulated)
    /// params, which must play this slot's engine.
    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &ParamSnapshot,
        live: &AlgoLive,
        sample_rate: u32,
        pool: &mut SymPool,
    ) {
        debug_assert_eq!(p.engine(), self.kind().engine());
        self.set_pitch(p);
        match self {
            Self::Algo(a) => a.render(out, &p.algo, live, sample_rate),
            Self::Modal(m) => m.render(out, &p.modal, sample_rate, pool),
        }
    }

    /// `p`'s pitch offset (ADR 0042) to the engine about to play.
    fn set_pitch(&mut self, p: &ParamSnapshot) {
        match self {
            Self::Algo(a) => a.set_pitch(p.pitch.semitones()),
            Self::Modal(m) => m.set_pitch(p.pitch.ratio()),
        }
    }

    /// Voice lifetime: is the engine still sounding?
    pub fn is_active(&self) -> bool {
        match self {
            Self::Algo(a) => a.is_active(),
            Self::Modal(m) => m.is_active(),
        }
    }

    /// What the sounding Modal note costs, if Modal is sounding.
    pub fn modal_playing_cost(&self) -> Option<Cost> {
        match self {
            Self::Algo(_) => None,
            Self::Modal(m) => m.playing_cost(),
        }
    }

    /// Cycles/sample of one engine instance playing `p` (ADR 0013).
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        match p.engine() {
            EngineType::Algo => AlgoEngine::cost(&p.algo, &mods.algo_levels_routed()),
            EngineType::Modal => {
                let mut c = ModalEngine::cost(&p.modal);
                // A steal's glide retunes each block, as a route does.
                if pitch_routed(mods) || p.pitch.steal == Steal::Glide {
                    c = c + ModalEngine::PITCH;
                    if p.modal.mode == ResonatorMode::Sympathetic {
                        c = c + ModalEngine::HALO_PITCH;
                    }
                }
                if p.modal.mode == ResonatorMode::Sympathetic && chord_routed(mods) {
                    c = c + ModalEngine::CHORD;
                }
                c
            }
        }
    }
}

/// A route, of any amount, into STRUCTURE: SYMP's chord.
fn chord_routed(mods: &ModState) -> bool {
    mods.routes_into(ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE)) != 0
}

/// A route, of any amount, into the voice's PITCH or FINE.
fn pitch_routed(mods: &ModState) -> bool {
    [PitchParams::PITCH, PitchParams::FINE]
        .into_iter()
        .any(|q| mods.routes_into(ParamAddr::new(BlockRef::Pitch, q)) != 0)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::boxed::Box;

    use super::*;
    use crate::sym_alloc::{SYM_SLOTS, SymAlloc, SymSlot};
    use crate::voice_alloc::VoiceIdx;

    const MODES: [ResonatorMode; 4] = [
        ResonatorMode::String,
        ResonatorMode::Modal,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ];

    #[test]
    fn slot_kind_reads_engine_then_mode() {
        for mode in MODES {
            let mut algo = ParamSnapshot::for_engine(EngineType::Algo);
            algo.modal.mode = mode;
            assert_eq!(SlotKind::of(&algo), SlotKind::Algo);
            let mut modal = ParamSnapshot::for_engine(EngineType::Modal);
            modal.modal.mode = mode;
            assert_eq!(SlotKind::of(&modal), SlotKind::Modal(mode));
            assert_eq!(SlotKind::Modal(mode).engine(), EngineType::Modal);
        }
        assert_eq!(SlotKind::Algo.engine(), EngineType::Algo);
    }

    #[test]
    fn engine_slot_layout_matches_repr() {
        let mut raw = Box::<EngineSlot>::new_uninit();
        // SAFETY: the box is valid for `size_of::<EngineSlot>()` byte writes.
        unsafe {
            raw.as_mut_ptr()
                .cast::<u8>()
                .write_bytes(0xA5, size_of::<EngineSlot>())
        };
        EngineSlot::init_in_place(&mut raw, SlotKind::Algo);
        // SAFETY: `init_in_place` built a valid slot in the box.
        let mut slot = unsafe { raw.assume_init() };
        let mut pool = SymAlloc::new();
        let kinds = [SlotKind::Algo]
            .into_iter()
            .chain(MODES.map(SlotKind::Modal));
        for k in kinds {
            slot.rebuild(k, &mut pool, VoiceIdx::ALL[0]);
            assert_eq!(slot.kind(), k);
            let (tag, payload) = slot.mirror_parts();
            let addr = match &*slot {
                EngineSlot::Algo(a) => {
                    assert_eq!(tag, 0);
                    core::ptr::from_ref(a).cast::<u8>()
                }
                EngineSlot::Modal(m) => {
                    assert_eq!(tag, 1);
                    core::ptr::from_ref(m).cast::<u8>()
                }
            };
            assert_eq!(addr, payload, "{k:?}");
        }
    }

    #[test]
    fn resting_maps_only_sympathetic() {
        assert_eq!(SlotKind::Algo.resting(), SlotKind::Algo);
        for mode in MODES {
            let want = match mode {
                ResonatorMode::Sympathetic => ResonatorMode::String,
                other => other,
            };
            assert_eq!(SlotKind::Modal(mode).resting(), SlotKind::Modal(want));
        }
    }

    /// The lease `slot`'s engine holds, if it holds one.
    fn lease_slot(slot: &mut EngineSlot) -> Option<SymSlot> {
        match slot {
            EngineSlot::Algo(_) => None,
            EngineSlot::Modal(m) => m.lease_mut().map(|l| l.slot()),
        }
    }

    #[test]
    fn rebuild_moves_leases_as_the_table_says() {
        let v = |i: usize| VoiceIdx::ALL[i];
        let sym = SlotKind::Modal(ResonatorMode::Sympathetic);
        let string = SlotKind::Modal(ResonatorMode::String);
        let mut raw = Box::<EngineSlot>::new_uninit();
        EngineSlot::init_in_place(&mut raw, SlotKind::Algo);
        // SAFETY: `init_in_place` built a valid slot in the box.
        let mut slot = unsafe { raw.assume_init() };
        let mut pool = SymAlloc::new();

        assert!(pool.place(v(0)));
        slot.rebuild(sym, &mut pool, v(0));
        assert_eq!((slot.kind(), pool.lent()), (sym, 1));
        let held = lease_slot(&mut slot).expect("a lease");

        slot.rebuild(sym, &mut pool, v(0));
        assert_eq!(pool.lent(), 1);
        assert_eq!(lease_slot(&mut slot), Some(held), "kept, not lent again");

        slot.rebuild(string, &mut pool, v(0));
        assert_eq!((pool.lent(), pool.free()), (0, SYM_SLOTS));

        // Unplaced, or with every slot held: Sympathetic, bare.
        slot.rebuild(sym, &mut pool, v(0));
        assert!(slot.is_bare() && pool.lent() == 0, "no promise: bare");
        let others: std::vec::Vec<_> = (1..=SYM_SLOTS)
            .map(|i| {
                assert!(pool.place(v(i)));
                pool.lend(v(i)).expect("promised")
            })
            .collect();
        assert!(!pool.place(v(0)));
        slot.rebuild(string, &mut pool, v(0));
        slot.rebuild(sym, &mut pool, v(0));
        assert_eq!(slot.kind(), sym);
        assert!(slot.is_bare(), "no slot free: bare");
        others.into_iter().for_each(|l| pool.give_back(l));
    }
}
