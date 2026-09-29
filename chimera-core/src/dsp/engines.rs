//! A voice's engine: one slot holding the engine it plays, rebuilt in
//! place when it plays another (ADR 0051, superseding 0008). The variant
//! is the engine; dispatch is one exhaustive `match` per method. Adding an
//! engine = one variant here; the compiler lists the arms.

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};
use crate::dsp::modal::{ModalEngine, ResonatorMode};
use crate::hw::Cost;
use crate::in_place::{by_value, in_place_enum};
use crate::modulation::ModState;
use crate::params::{EngineType, ParamSnapshot, PitchParams};
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
    /// use chimera_core::dsp::modal::{ModalEngine, ResonatorMode};
    /// let _ = EngineSlot::Modal(ModalEngine::new(ResonatorMode::Modal));
    /// ```
    ///
    /// Nor can the seal be named, to wrap an engine in it:
    ///
    /// ```compile_fail,E0603
    /// use chimera_core::dsp::modal::{ModalEngine, ResonatorMode};
    /// let _ = chimera_core::in_place::Sealed(ModalEngine::new(ResonatorMode::Modal));
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
    /// By value, through the stack: for tests. A voice builds its slot in
    /// place (`init_in_place`).
    #[doc(hidden)]
    pub fn new(kind: SlotKind) -> Self {
        // SAFETY: `init_in_place` writes the whole slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, kind)) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, kind: SlotKind) -> &mut Self {
        match kind {
            // SAFETY: `AlgoEngine::init_in_place` writes every field.
            SlotKind::Algo => unsafe { Self::init_algo(slot, AlgoEngine::init_in_place) },
            // SAFETY: `ModalEngine::init_in_place` writes every field.
            SlotKind::Modal(mode) => unsafe {
                Self::init_modal(slot, |m| ModalEngine::init_in_place(m, mode))
            },
        }
    }

    pub fn kind(&self) -> SlotKind {
        match self {
            Self::Algo(_) => SlotKind::Algo,
            Self::Modal(m) => SlotKind::Modal(m.mode()),
        }
    }

    /// Back to the idle `kind` that `new` builds, in place: no stack copy
    /// of an engine.
    pub fn rebuild(&mut self, kind: SlotKind) {
        match kind {
            // SAFETY: `AlgoEngine::init_in_place` writes every field.
            SlotKind::Algo => unsafe { self.rebuild_algo(AlgoEngine::init_in_place) },
            // SAFETY: `ModalEngine::init_in_place` writes every field.
            SlotKind::Modal(mode) => unsafe {
                self.rebuild_modal(|m| ModalEngine::init_in_place(m, mode))
            },
        }
    }

    /// `p` must play this slot's engine.
    pub fn note_on(&mut self, note: MidiNote, vel: Velocity, p: &ParamSnapshot, sample_rate: u32) {
        debug_assert_eq!(p.engine(), self.kind().engine());
        self.set_pitch(p);
        match self {
            Self::Algo(a) => a.note_on(note, vel, &p.algo, sample_rate),
            Self::Modal(m) => m.note_on(note.get(), vel.get(), &p.modal, sample_rate),
        }
    }

    pub fn note_off(&mut self) {
        match self {
            Self::Algo(a) => a.note_off(),
            Self::Modal(m) => m.note_off(),
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
    ) {
        debug_assert_eq!(p.engine(), self.kind().engine());
        self.set_pitch(p);
        match self {
            Self::Algo(a) => a.render(out, &p.algo, live, sample_rate),
            Self::Modal(m) => m.render(out, &p.modal, sample_rate),
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

    /// The Modal model the sounding note plays, if Modal is sounding.
    pub fn modal_playing(&self) -> Option<ResonatorMode> {
        match self {
            Self::Algo(_) => None,
            Self::Modal(m) => m.playing(),
        }
    }

    /// Cycles/sample of one engine instance playing `p` (ADR 0013).
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        match p.engine() {
            EngineType::Algo => AlgoEngine::cost(&p.algo, &mods.algo_levels_routed()),
            EngineType::Modal if pitch_routed(mods) => {
                ModalEngine::cost(&p.modal) + ModalEngine::PITCH
            }
            EngineType::Modal => ModalEngine::cost(&p.modal),
        }
    }
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
        let kinds = [SlotKind::Algo]
            .into_iter()
            .chain(MODES.map(SlotKind::Modal));
        for k in kinds {
            slot.rebuild(k);
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
}
