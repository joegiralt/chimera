//! A voice's engine: one slot holding the engine it plays, rebuilt in
//! place when it plays another (ADR 0051, superseding 0008). The variant
//! is the engine; dispatch is one exhaustive `match` per method. Adding an
//! engine = one variant here; the compiler lists the arms.

use crate::taps::TapRows;
use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::addr::{BlockRef, ParamAddr};
use crate::block::OffsetLaw;
use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};
use crate::dsp::algo::plan::OPS;
use crate::dsp::modal::{Halo, ModalEngine, ModalParams, Model, ResonatorMode, SymPool};
use crate::dsp::tape_engine::{COST_WAVE_LEVEL, LEVEL_CENTS, TapeEngine, TapeModel};
use crate::dsp::va::{VA_OUT_GAIN, VaEngine};
use crate::hw::Cost;
use crate::in_place::{in_place_enum, move_out};
use crate::modulation::{ModState, amount_scale};
use crate::params::{EngineType, ParamSnapshot, PitchParams, Steal};
use crate::project::{PartId, SourceId};
use crate::source_table::GateView;
use crate::sym_alloc::{Lease, SymAlloc};
use crate::voice_alloc::VoiceIdx;
use crate::{MidiNote, Velocity};

/// What a slot holds: the engine and, for Modal, its model. TAPE and VA
/// are one kind each: every model or MODE has the same state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Algo,
    Modal(ResonatorMode),
    Tape,
    Va,
}

/// What an engine reads beside its params: the sympathetic pool (MODAL's
/// SYMP), this block's view of the pool's sources (TAPE) and the Part the
/// voice plays (VA's seed). Each engine ignores what it doesn't read.
pub struct EngineCtx<'a> {
    pub sym: &'a mut SymPool,
    pub src: &'a GateView<'a>,
    pub part: PartId,
    /// SETTINGS > TUNING in semitones, eased (`Instrument`): added to the
    /// pitch offset of every engine.
    pub tune: f32,
}

impl SlotKind {
    /// The kind `p` plays: its engine, then Modal's MODE.
    pub fn of(p: &ParamSnapshot) -> Self {
        match p.engine() {
            EngineType::Algo => Self::Algo,
            EngineType::Modal => Self::Modal(p.modal.mode),
            EngineType::Tape => Self::Tape,
            EngineType::Va => Self::Va,
        }
    }

    pub fn engine(self) -> EngineType {
        match self {
            Self::Algo => EngineType::Algo,
            Self::Modal(_) => EngineType::Modal,
            Self::Tape => EngineType::Tape,
            Self::Va => EngineType::Va,
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
    /// use chimera_engine::dsp::engines::EngineSlot;
    /// use chimera_engine::dsp::modal::{ModalEngine, ResonatorMode, SymPool};
    /// let e = ModalEngine::new_in(&mut SymPool::boxed(), ResonatorMode::Modal);
    /// let _ = EngineSlot::Modal(e);
    /// ```
    ///
    /// Nor can the seal be named, to wrap an engine in it:
    ///
    /// ```compile_fail,E0603
    /// use chimera_engine::dsp::modal::{ModalEngine, ResonatorMode, SymPool};
    /// let e = ModalEngine::new_in(&mut SymPool::boxed(), ResonatorMode::Modal);
    /// let _ = chimera_engine::in_place::Sealed(e);
    /// ```
    #[expect(
        clippy::large_enum_variant,
        reason = "one slot per voice, built in place: boxing needs a heap"
    )]
    pub enum EngineSlot {
        Algo(AlgoEngine) => rebuild_algo, init_algo;
        Modal(ModalEngine) => rebuild_modal, init_modal;
        Tape(TapeEngine) => rebuild_tape, init_tape;
        Va(VaEngine) => rebuild_va, init_va;
    }
}

/// TAPE's `out_gain` (ADR 0063, TAPE Task 19): TAPE INIT on the reference
/// source, MORPH KEYS' C4 peaked at −0.5 dBFS as the importer leaves every
/// source, sits at the factory median (+12.5 dB). A sine, its crest 3 dB,
/// plays about 7 dB over it; factory TAPE Sounds trim their OUT LEVEL.
pub const TAPE_OUT_GAIN: f32 = 4.236;

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
            // SAFETY: `AlgoEngine::init_in_place` writes every field. A lone
            // slot is voice 0's.
            SlotKind::Algo => unsafe {
                Self::init_algo(slot, |a| {
                    let a = AlgoEngine::init_in_place(a);
                    a.seed(VoiceIdx::ALL[0]);
                    a
                })
            },
            // SAFETY: `ModalEngine::init_in_place` writes every field.
            SlotKind::Modal(mode) => unsafe {
                Self::init_modal(slot, |m| {
                    ModalEngine::init_in_place(m, Model::resting(mode))
                })
            },
            // SAFETY: `TapeEngine::init_in_place` writes every field. A lone
            // slot is voice 0's.
            SlotKind::Tape => unsafe {
                Self::init_tape(slot, |t| {
                    let t = TapeEngine::init_in_place(t);
                    t.seed(0);
                    t
                })
            },
            // SAFETY: `VaEngine::init_in_place` writes every field. A lone
            // slot is voice 0's.
            SlotKind::Va => unsafe {
                Self::init_va(slot, |v| {
                    let v = VaEngine::init_in_place(v);
                    v.place(VoiceIdx::ALL[0]);
                    v
                })
            },
        }
    }

    pub fn kind(&self) -> SlotKind {
        match self {
            Self::Algo(_) => SlotKind::Algo,
            Self::Modal(m) => SlotKind::Modal(m.mode()),
            Self::Tape(_) => SlotKind::Tape,
            Self::Va(_) => SlotKind::Va,
        }
    }

    /// The engine's output gain at the VCA (ADR 0058): Algo is the
    /// reference.
    pub fn out_gain(&self) -> f32 {
        match self {
            Self::Algo(_) => 1.0,
            Self::Tape(_) => TAPE_OUT_GAIN,
            Self::Va(_) => VA_OUT_GAIN,
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
    ///
    /// A TAPE slot rebuilt is seeded by `voice` (its own strip of tape,
    /// spec § 2 TAPE); one rebuilt from TAPE keeps the strip it ran. VA
    /// alike: one rebuilt from VA runs on (`VaCarry`), any other is fresh.
    pub fn rebuild(&mut self, kind: SlotKind, pool: &mut SymAlloc, voice: VoiceIdx) {
        let sympathetic = kind == SlotKind::Modal(ResonatorMode::Sympathetic);
        let old = match self {
            // SAFETY: the payload is rebuilt below in every arm, and
            // `rebuild_*` aborts rather than unwinds.
            Self::Modal(m) => m.lease_mut().map(|l| unsafe { move_out(l) }),
            Self::Algo(_) | Self::Tape(_) | Self::Va(_) => None,
        };
        // Nothing between the move and the rebuild can panic: a lend only
        // when there was no old lease, a give-back only after.
        let (lease, back) = match old {
            Some(l) if sympathetic => (Some(l), None),
            Some(l) => (None, Some(l)),
            None if sympathetic => (pool.lend(voice), None),
            None => (None, None),
        };
        self.build(kind, lease, voice);
        if let Some(l) = back {
            pool.give_back(l);
        }
    }

    /// `kind`; Sympathetic with `lease`'s halo, or bare without one; TAPE on
    /// `voice`'s strip; VA on `voice`'s seed, where its last note left off;
    /// ALGO's noise on `voice`'s seeds.
    fn build(&mut self, kind: SlotKind, lease: Option<Lease>, voice: VoiceIdx) {
        match kind {
            SlotKind::Algo => {
                // SAFETY: `AlgoEngine::init_in_place` writes every field.
                unsafe { self.rebuild_algo(AlgoEngine::init_in_place) };
                if let Self::Algo(a) = self {
                    a.seed(voice);
                }
            }
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
            SlotKind::Tape => {
                let strip = match self {
                    Self::Tape(t) => Some(t.strip()),
                    _ => None,
                };
                // SAFETY: `TapeEngine::init_in_place` writes every field.
                unsafe { self.rebuild_tape(TapeEngine::init_in_place) };
                if let Self::Tape(t) = self {
                    match strip {
                        Some(s) => t.restore(s),
                        None => t.seed(voice.index() as u32),
                    }
                }
            }
            SlotKind::Va => {
                let carry = match self {
                    Self::Va(v) => Some(v.carry()),
                    _ => None,
                };
                // SAFETY: `VaEngine::init_in_place` writes every field.
                unsafe { self.rebuild_va(VaEngine::init_in_place) };
                if let Self::Va(v) = self {
                    v.place(voice);
                    if let Some(c) = carry {
                        v.restore(c);
                    }
                }
            }
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
            Self::Algo(_) | Self::Tape(_) | Self::Va(_) => pool.note_on_clear(None, voice),
        }
    }

    /// `p` must play this slot's engine. At STEAL GLIDE a note on a
    /// sounding engine, another note of its Part stealing it, glides it
    /// there (#254, ADR 0065). TAPE latches `source`, and with none plays
    /// nothing; the other engines ignore it. VA seeds by `ctx.part`.
    pub fn note_on(
        &mut self,
        note: MidiNote,
        vel: Velocity,
        p: &ParamSnapshot,
        sample_rate: u32,
        source: Option<SourceId>,
        ctx: &mut EngineCtx<'_>,
    ) {
        debug_assert_eq!(p.engine(), self.kind().engine());
        self.set_pitch(p, ctx.tune);
        let pool = &mut *ctx.sym;
        match (self, p.pitch.steal_glide()) {
            (Self::Algo(a), None) => a.note_on(note, vel, &p.algo, sample_rate),
            (Self::Algo(a), Some(tau)) => a.glide_on(note, vel, &p.algo, sample_rate, tau),
            (Self::Modal(m), None) => m.note_on(note.get(), vel.get(), &p.modal, sample_rate, pool),
            (Self::Modal(m), Some(tau)) => {
                m.glide_on(note.get(), vel.get(), &p.modal, (sample_rate, tau), pool)
            }
            // A glide on a sounding engine plays on its own source (ADR
            // 0087: a ringing note keeps what it latched).
            (Self::Tape(t), Some(tau)) if t.is_active() => {
                t.glide_to(note.get(), (sample_rate, tau))
            }
            (Self::Tape(t), _) => {
                match source {
                    Some(id) => t.latch(id),
                    None => t.unlatch(),
                }
                t.start(note.get(), &p.tape, ctx.src, sample_rate);
            }
            (Self::Va(v), None) => v.note_on(note.get(), &p.va, ctx.part, sample_rate),
            (Self::Va(v), Some(tau)) => v.glide_on(note.get(), &p.va, ctx.part, (sample_rate, tau)),
        }
    }

    /// The sounding note moved to `note` with no strike, gliding a one-pole
    /// of `tau` seconds in log pitch (a re-snap, spec § 5.2).
    pub fn retune(
        &mut self,
        note: MidiNote,
        (sample_rate, tau): (u32, f32),
        ctx: &mut EngineCtx<'_>,
    ) {
        match self {
            Self::Algo(a) => a.glide_to(note, tau),
            Self::Modal(m) => m.glide_to(note.get(), (sample_rate, tau), ctx.sym),
            Self::Tape(t) => t.glide_to(note.get(), (sample_rate, tau)),
            Self::Va(v) => v.glide_to(note.get(), (sample_rate, tau)),
        }
    }

    /// The bytes `retune` to `note` writes to the lines: a glide's growth
    /// (`ModalEngine::strike_clear`), none on Algo or an idle engine.
    pub fn retune_clear(&self, pool: &SymPool, note: MidiNote, sample_rate: u32) -> usize {
        match self {
            Self::Modal(m) => m
                .strike_clear(note.get(), true, sample_rate, pool)
                .unwrap_or(0),
            Self::Algo(_) | Self::Tape(_) | Self::Va(_) => 0,
        }
    }

    /// What a glide costs `p` under `mods` beyond `cost`, which bills it
    /// only at STEAL GLIDE or with a PITCH route: a Modal voice's retune
    /// each block (`ModalEngine::PITCH`, and `HALO_PITCH` on SYMP). Algo
    /// sets its frequency each block anyway.
    pub fn glide_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        match p.engine() {
            // Billed in `cost` already: the pitch wheel may move any Modal voice.
            EngineType::Tape if wave_levels(p) && !level_billed(p, mods) => COST_WAVE_LEVEL,
            _ => Cost::ZERO,
        }
    }

    /// A glide steal's ratio now, 1 at rest: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        match self {
            Self::Algo(a) => a.slide(),
            Self::Modal(m) => m.slide(),
            Self::Tape(t) => t.slide(),
            Self::Va(v) => v.slide(),
        }
    }

    pub fn note_off(&mut self, ctx: &mut EngineCtx<'_>) {
        match self {
            Self::Algo(a) => a.note_off(),
            Self::Modal(m) => m.note_off(ctx.sym),
            Self::Tape(t) => t.note_off(),
            Self::Va(v) => v.note_off(),
        }
    }

    /// TAPE's heads for the strip; none on the others.
    pub fn heads(&self) -> crate::dsp::tape_engine::Heads {
        match self {
            Self::Tape(t) => t.heads(),
            Self::Algo(_) | Self::Modal(_) | Self::Va(_) => crate::dsp::tape_engine::Heads::NONE,
        }
    }

    /// The source TAPE latched at its last note-on; none on the others.
    pub fn source(&self) -> Option<SourceId> {
        match self {
            Self::Algo(_) | Self::Modal(_) | Self::Va(_) => None,
            Self::Tape(t) => t.source(),
        }
    }

    /// The engine plays on through note-off (ONESHOT): the voice keeps its
    /// gate. ALGO, MODAL and VA never.
    pub fn sustains(&self) -> bool {
        match self {
            Self::Algo(_) | Self::Modal(_) | Self::Va(_) => false,
            Self::Tape(t) => t.sustains(),
        }
    }

    /// A fresh note is to come on this idle slot: TAPE's ramp starts from
    /// silence, not from its last note's sample (an idle voice skips
    /// render, so the engine still holds it); VA's ramps start on their
    /// values.
    pub fn hold(&mut self) {
        match self {
            Self::Algo(_) | Self::Modal(_) => {}
            Self::Tape(t) => t.hold(),
            Self::Va(v) => v.hold(),
        }
    }

    /// Render one block of raw engine output from (possibly modulated)
    /// params, which must play this slot's engine. ALGO fills the wanted
    /// tap rows; MODAL leaves them untouched.
    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &ParamSnapshot,
        live: &AlgoLive,
        sample_rate: u32,
        ctx: &mut EngineCtx<'_>,
        taps: &mut TapRows,
    ) {
        debug_assert_eq!(p.engine(), self.kind().engine());
        self.set_pitch(p, ctx.tune);
        match self {
            Self::Algo(a) => a.render(out, &p.algo, live, sample_rate, taps),
            Self::Modal(m) => m.render(out, &p.modal, sample_rate, ctx.sym),
            Self::Tape(t) => t.play(out, &p.tape, ctx.src, sample_rate),
            Self::Va(v) => v.render(out, &p.va, sample_rate),
        }
    }

    /// `p`'s pitch offset (ADR 0042) plus the unit's TUNING, `tune`
    /// semitones, to the engine about to play: the one path every engine's
    /// pitch takes. `+ 0.0` leaves the offset's bits as they were.
    fn set_pitch(&mut self, p: &ParamSnapshot, tune: f32) {
        let st = p.pitch.semitones() + tune;
        match self {
            Self::Algo(a) => a.set_pitch(st),
            Self::Modal(m) => m.set_pitch(crate::params::PitchParams::ratio_of(st)),
            Self::Tape(t) => t.set_pitch(st * 100.0),
            Self::Va(v) => v.set_pitch(st * 100.0),
        }
    }

    /// Voice lifetime: is the engine still sounding?
    pub fn is_active(&self) -> bool {
        match self {
            Self::Algo(a) => a.is_active(),
            Self::Modal(m) => m.is_active(),
            Self::Tape(t) => t.is_active(),
            Self::Va(v) => v.is_active(),
        }
    }

    /// What the sounding note costs, if its model is latched at note-on
    /// (MODAL, TAPE) or its MODE plays (VA) and it sounds.
    pub fn playing_cost(&self) -> Option<Cost> {
        match self {
            Self::Algo(_) => None,
            Self::Modal(m) => m.playing_cost(),
            Self::Tape(t) => t.playing_cost(),
            Self::Va(v) => v.playing_cost(),
        }
    }

    /// ALGO's ops the kernel runs next block; `None` on other engines.
    pub fn algo_running(&self) -> Option<[bool; OPS]> {
        match self {
            Self::Algo(a) => Some(a.running()),
            _ => None,
        }
    }

    /// What this slot's engine bills for `p`'s stored model, before its
    /// routes: what `playing_cost` is held against.
    pub fn model_cost(&self, p: &ParamSnapshot) -> Cost {
        match self {
            Self::Algo(_) => Cost::ZERO,
            Self::Modal(_) => ModalEngine::cost(&p.modal),
            Self::Tape(_) => TapeEngine::model_cost(p.tape.model),
            Self::Va(_) => VaEngine::cost(&p.va),
        }
    }

    /// Cycles/sample of one engine instance playing `p` (ADR 0013).
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        match p.engine() {
            EngineType::Algo => AlgoEngine::cost(&p.algo, &mods.algo_levels_routed()),
            EngineType::Modal => {
                let mut c = ModalEngine::cost(&p.modal);
                // A route, a steal's glide or the pitch wheel (#437) retunes
                // each block: any Modal voice may be bent, so all are billed.
                c = c + pitch_cost(p);
                if p.modal.mode == ResonatorMode::Sympathetic && chord_routed(mods) {
                    c = c + ModalEngine::CHORD;
                }
                c
            }
            EngineType::Tape if wave_levels(p) && level_billed(p, mods) => {
                TapeEngine::cost(&p.tape, mods) + COST_WAVE_LEVEL
            }
            EngineType::Tape => TapeEngine::cost(&p.tape, mods),
            EngineType::Va => VaEngine::cost(&p.va),
        }
    }
}

/// TRANSWAVE: a pitch that moves far enough changes level, a block of four
/// heads (`COST_WAVE_LEVEL`).
fn wave_levels(p: &ParamSnapshot) -> bool {
    p.tape.model == TapeModel::Transwave
}

/// Pitch can move past `LEVEL_CENTS` in a note: a glide steal, or PITCH
/// and FINE routes whose `|amount| × range` sum past it. A vibrato can't.
fn level_billed(p: &ParamSnapshot, mods: &ModState) -> bool {
    p.pitch.steal == Steal::Glide || pitch_reach(mods) > LEVEL_CENTS
}

/// The most cents PITCH's and FINE's routes can move a note, every source
/// at full swing.
fn pitch_reach(mods: &ModState) -> f32 {
    [PitchParams::PITCH, PitchParams::FINE]
        .into_iter()
        .map(|q| {
            let addr = ParamAddr::new(BlockRef::Pitch, q);
            let cents = match addr.spec().map(|s| s.law) {
                Some(OffsetLaw::Semitones(n)) => n * 100.0,
                Some(OffsetLaw::Cents(n)) => n,
                _ => 0.0,
            };
            let d = mods.find(addr);
            let swing = |s| d.map_or(0.0, |d| amount_scale(mods.amount(s, d)).abs());
            cents * (0..mods.num_sources()).map(swing).sum::<f32>()
        })
        .sum()
}

/// A route, of any amount, into STRUCTURE: SYMP's chord.
fn chord_routed(mods: &ModState) -> bool {
    mods.routes_into(ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE)) != 0
}

/// A Modal voice that retunes each block (a PITCH route, a glide):
/// `ModalEngine::PITCH`, and on SYMP `HALO_PITCH` for its halo's targets.
fn pitch_cost(p: &ParamSnapshot) -> Cost {
    if p.modal.mode == ResonatorMode::Sympathetic {
        ModalEngine::PITCH + ModalEngine::HALO_PITCH
    } else {
        ModalEngine::PITCH
    }
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
        let kinds = [SlotKind::Algo, SlotKind::Tape, SlotKind::Va]
            .into_iter()
            .chain(MODES.map(SlotKind::Modal))
            .chain([SlotKind::Tape, SlotKind::Va]);
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
                EngineSlot::Tape(t) => {
                    assert_eq!(tag, 2);
                    core::ptr::from_ref(t).cast::<u8>()
                }
                EngineSlot::Va(v) => {
                    assert_eq!(tag, 3);
                    core::ptr::from_ref(v).cast::<u8>()
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
            EngineSlot::Algo(_) | EngineSlot::Tape(_) | EngineSlot::Va(_) => None,
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
