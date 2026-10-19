//! Semantic parameter addresses (spec §2): an address names *what* a
//! parameter is, not where it sits on a page or in a chain, so rearranging
//! cells or reordering blocks never remaps a mod route.

use crate::block::{Block, ParamId, ParamSpec, find_spec};
use crate::dsp::algo::params::{OpView, OpViewMut};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::orbit::RingIdx;

/// An operator of the Algo engine. `TryFrom<u8>` rejects values above 5, so an out-of-range
/// operator (bad sound or SysEx data) is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    A,
    B,
    C,
    D,
    E,
    F,
}

/// Rejected operator index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpOutOfRange(pub u8);

impl Op {
    pub const ALL: [Op; 6] = [Op::A, Op::B, Op::C, Op::D, Op::E, Op::F];

    /// Index into `AlgoParams::ops`.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Step through the operators by encoder ticks, clamped at A and F.
    pub fn nudged(self, delta: i8) -> Op {
        Op::ALL[(self.index() as i16 + delta as i16).clamp(0, Op::ALL.len() as i16 - 1) as usize]
    }
}

impl TryFrom<u8> for Op {
    type Error = OpOutOfRange;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Op::ALL.get(v as usize).copied().ok_or(OpOutOfRange(v))
    }
}

/// One block instance of a Part voice. Multiple instances of one kind (two
/// filters) are sub-project 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRef {
    Modal,
    /// TAPE's engine (`TapeEngineParams`). `Tape` is the master stage.
    TapeEngine,
    /// VA's engine (`VaParams`).
    Va,
    /// The Algo engine's voice-level parameters (`AlgoParams`).
    Algo,
    /// One Algo operator (`AlgoOpParams`).
    AlgoOp(Op),
    Drive,
    Filter,
    Folder,
    /// The voice's FX slot (`VoiceFxParams`, ADR 0155).
    VoiceFx,
    /// ENV slot `n`: `envelopes[n]`.
    Env(crate::dsp::modulator::EnvSlot),
    /// LFO slot `n`: `lfos[n]`.
    Lfo(crate::dsp::modulator::LfoSlot),
    /// `OutParams { volume, pan }`
    Out,
    /// The voice's pitch (`PitchParams`), on every engine (ADR 0042).
    Pitch,
    /// Chorus, delay, reverb, tape and the master compressor: the Performance's shared FX.
    Chorus,
    Delay,
    Reverb,
    Tape,
    Comp,
    /// A Part's mix settings (`PartParams`): channel, mode, output, level,
    /// pan, SNAP, HELD.
    Part,
    /// SETTINGS › PERSONALIZE › THEME (`ThemeSettings`): held by the UI,
    /// not a Sound.
    Theme,
    /// SETTINGS › MIDI CONFIG (`MidiSettings`): SYNC, held by the UI and
    /// stored in SYSTEM (ADR 0080).
    Midi,
    /// Part n's `PartParams` from any page: the SETTINGS mirrors. UI-only,
    /// so not in `ALL` and never stored.
    PartMix(crate::project::PartId),
    /// ORBIT's chord ring (`ChordRing`): KEY, SCALE, BARS, SEGS. Project
    /// data, never a voice's.
    OrbitChords,
    /// ORBIT ring n of the live scene (`SceneRing`).
    OrbitRing(crate::orbit::RingIdx),
    /// ORBIT's BPM (`Bpm`): saved in its own record, so no block code and
    /// not in `ALL`.
    Tempo,
}

impl BlockRef {
    pub const ALL: [BlockRef; 36] = [
        BlockRef::Modal,
        BlockRef::Algo,
        BlockRef::AlgoOp(Op::A),
        BlockRef::AlgoOp(Op::B),
        BlockRef::AlgoOp(Op::C),
        BlockRef::AlgoOp(Op::D),
        BlockRef::AlgoOp(Op::E),
        BlockRef::AlgoOp(Op::F),
        BlockRef::Drive,
        BlockRef::Filter,
        BlockRef::Folder,
        BlockRef::Env(EnvSlot::Env1),
        BlockRef::Env(EnvSlot::Env2),
        BlockRef::Env(EnvSlot::Env3),
        BlockRef::Lfo(LfoSlot::Lfo1),
        BlockRef::Lfo(LfoSlot::Lfo2),
        BlockRef::Lfo(LfoSlot::Lfo3),
        BlockRef::Out,
        BlockRef::Pitch,
        BlockRef::Chorus,
        BlockRef::Delay,
        BlockRef::Reverb,
        BlockRef::Tape,
        BlockRef::Comp,
        BlockRef::Part,
        BlockRef::Theme,
        BlockRef::OrbitChords,
        BlockRef::OrbitRing(RingIdx::ALL[0]),
        BlockRef::OrbitRing(RingIdx::ALL[1]),
        BlockRef::OrbitRing(RingIdx::ALL[2]),
        BlockRef::OrbitRing(RingIdx::ALL[3]),
        BlockRef::OrbitRing(RingIdx::ALL[4]),
        BlockRef::Midi,
        BlockRef::TapeEngine,
        BlockRef::Va,
        BlockRef::VoiceFx,
    ];

    /// The block type's spec table (static; no instance needed).
    pub const fn specs(self) -> &'static [ParamSpec] {
        match self {
            BlockRef::Modal => &crate::dsp::modal::MODAL_SPECS,
            BlockRef::TapeEngine => &crate::dsp::tape_engine::TAPE_ENGINE_SPECS,
            BlockRef::Va => &crate::dsp::va::VA_SPECS,
            BlockRef::Algo => &crate::dsp::algo::params::ALGO_SPECS,
            BlockRef::AlgoOp(_) => &crate::dsp::algo::params::ALGO_OP_SPECS,
            BlockRef::Drive => &crate::params::DRIVE_SPECS,
            BlockRef::Filter => &crate::params::FILTER_SPECS,
            BlockRef::Folder => &crate::params::FOLDER_SPECS,
            BlockRef::VoiceFx => &crate::dsp::voice_fx::params::VOICE_FX_SPECS,
            BlockRef::Env(_) => &crate::params::ENV_SPECS,
            BlockRef::Lfo(_) => &crate::dsp::lfo::LFO_SPECS,
            BlockRef::Out => &crate::params::OUT_SPECS,
            BlockRef::Pitch => &crate::params::PITCH_SPECS,
            BlockRef::Chorus => &crate::dsp::chorus::CHORUS_SPECS,
            BlockRef::Delay => &crate::dsp::delay::DELAY_SPECS,
            BlockRef::Reverb => &crate::dsp::reverb::REVERB_SPECS,
            BlockRef::Tape => &crate::dsp::tape::TAPE_SPECS,
            BlockRef::Comp => &crate::dsp::comp::COMP_SPECS,
            BlockRef::Part | BlockRef::PartMix(_) => &crate::part::PART_SPECS,
            BlockRef::Theme => &crate::theme_settings::THEME_SPECS,
            BlockRef::Midi => &crate::midi_settings::MIDI_SPECS,
            BlockRef::OrbitChords => &crate::orbit::scene::CHORD_SPECS,
            BlockRef::OrbitRing(_) => &crate::orbit::scene::RING_SPECS,
            BlockRef::Tempo => &crate::orbit::scene::TEMPO_SPECS,
        }
    }

    /// A Sound holds it (`ParamSnapshot`'s blocks): what a Part's knobs
    /// glide (ADR 0112). `ParamSnapshot`'s dispatch agrees (a test).
    pub const fn in_sound(self) -> bool {
        match self {
            BlockRef::Modal
            | BlockRef::TapeEngine
            | BlockRef::Va
            | BlockRef::Algo
            | BlockRef::AlgoOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::VoiceFx
            | BlockRef::Env(_)
            | BlockRef::Lfo(_)
            | BlockRef::Out
            | BlockRef::Pitch => true,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::PartMix(_)
            | BlockRef::Theme
            | BlockRef::Midi
            | BlockRef::OrbitChords
            | BlockRef::OrbitRing(_)
            | BlockRef::Tempo => false,
        }
    }

    /// Whether `Voice::render` reads this block from its modulated copy and
    /// hears it without a route of its own. ENV slots read their LEVEL, TIME,
    /// RISE, FALL and SHAPE sums per block; the LFO is read unmodulated; FX run
    /// outside `Voice`.
    pub const fn voice_reads(self) -> bool {
        match self {
            BlockRef::Modal
            | BlockRef::Va
            | BlockRef::Algo
            | BlockRef::AlgoOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::Out
            | BlockRef::Pitch
            | BlockRef::VoiceFx
            | BlockRef::Env(_) => true,
            // Not yet: the voice plays TAPE from build step 3.
            BlockRef::TapeEngine
            | BlockRef::Lfo(_)
            | BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::PartMix(_)
            | BlockRef::Theme
            | BlockRef::Midi
            | BlockRef::OrbitChords
            | BlockRef::OrbitRing(_)
            | BlockRef::Tempo => false,
        }
    }
}

/// The Part's knobs are a Sound's (ADR 0112): no other block declares one,
/// so none is left unglided by a knob that never sees it.
const _: () = {
    const fn knobs_in(specs: &[ParamSpec]) -> bool {
        let mut i = 0;
        while i < specs.len() {
            if specs[i].motion().knob().is_some() {
                return true;
            }
            i += 1;
        }
        false
    }
    let mut b = 0;
    while b < BlockRef::ALL.len() {
        let r = BlockRef::ALL[b];
        assert!(r.in_sound() || !knobs_in(r.specs()));
        b += 1;
    }
    assert!(!knobs_in(BlockRef::Tempo.specs()));
};

/// A parameter of a block instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamAddr {
    pub block: BlockRef,
    pub param: ParamId,
}

impl ParamAddr {
    pub const fn new(block: BlockRef, param: ParamId) -> Self {
        Self { block, param }
    }

    pub const fn spec(self) -> Option<&'static ParamSpec> {
        find_spec(self.block.specs(), self.param)
    }

    /// Modulation may target this address: the spec says the value is read
    /// per block *and* `Voice` reads this block instance (plan D7).
    pub const fn modulatable(self) -> bool {
        self.block.voice_reads()
            && match self.spec() {
                Some(s) => s.modulatable,
                None => false,
            }
    }
}

/// A block as `BlockRead::block` lends it: an operator through its
/// guard (`OpView`), so WAVE reads its kind; any other block as itself.
pub enum BlockView<'a> {
    Plain(&'a dyn Block),
    Op(OpView<'a>),
}

/// `BlockView`, writable: an operator's WAVE writes go through the formant
/// budget (`OpViewMut`).
pub enum BlockViewMut<'a> {
    Plain(&'a mut dyn Block),
    Op(OpViewMut<'a>),
}

impl<'a> core::ops::Deref for BlockView<'a> {
    type Target = dyn Block + 'a;

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            BlockView::Plain(b) => *b,
            BlockView::Op(o) => o,
        }
    }
}

impl<'a> core::ops::Deref for BlockViewMut<'a> {
    type Target = dyn Block + 'a;

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            BlockViewMut::Plain(b) => &**b,
            BlockViewMut::Op(o) => o,
        }
    }
}

impl core::ops::DerefMut for BlockViewMut<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            BlockViewMut::Plain(b) => &mut **b,
            BlockViewMut::Op(o) => o,
        }
    }
}

/// The read half of `Blocks`, for code that only reads: a page's values, a
/// frame. A `BlockRead` alone can't be written through.
pub trait BlockRead {
    fn block(&self, b: BlockRef) -> Option<BlockView<'_>>;
}

/// Resolves block addresses to values (spec § Data model). A Sound's
/// `ParamSnapshot` holds the voice blocks; a Part view (`PartEdit`) adds the
/// shared FX. `None`: the address is not held here.
pub trait Blocks: BlockRead {
    fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>>;
}
