//! Semantic parameter addresses (spec §2): an address names *what* a
//! parameter is, not where it sits on a page or in a chain, so rearranging
//! cells or reordering blocks never remaps a mod route.

use crate::block::{Block, ParamId, ParamSpec, find_spec};
use crate::dsp::modulator::EnvSlot;

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
    /// The Algo engine's voice-level parameters (`AlgoParams`).
    Algo,
    /// One Algo operator (`AlgoOpParams`).
    AlgoOp(Op),
    Drive,
    Filter,
    Folder,
    /// ENV slot `n`: `envelopes[n]`.
    Env(crate::dsp::modulator::EnvSlot),
    Lfo,
    /// `OutParams { volume, pan }`
    Out,
    /// Chorus, delay, reverb, tape and the master compressor: the Performance's shared FX.
    Chorus,
    Delay,
    Reverb,
    Tape,
    Comp,
    /// A Part's mix settings (`PartParams`): channel, mode, output, level,
    /// pan, sends.
    Part,
    /// System › Theme (`ThemeSettings`): held by the UI, not a Sound.
    Theme,
}

impl BlockRef {
    pub const ALL: [BlockRef; 23] = [
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
        BlockRef::Lfo,
        BlockRef::Out,
        BlockRef::Chorus,
        BlockRef::Delay,
        BlockRef::Reverb,
        BlockRef::Tape,
        BlockRef::Comp,
        BlockRef::Part,
        BlockRef::Theme,
    ];

    /// The block type's spec table (static; no instance needed).
    pub fn specs(self) -> &'static [ParamSpec] {
        match self {
            BlockRef::Modal => &crate::dsp::modal::MODAL_SPECS,
            BlockRef::Algo => &crate::dsp::algo::params::ALGO_SPECS,
            BlockRef::AlgoOp(_) => &crate::dsp::algo::params::ALGO_OP_SPECS,
            BlockRef::Drive => &crate::params::DRIVE_SPECS,
            BlockRef::Filter => &crate::params::FILTER_SPECS,
            BlockRef::Folder => &crate::params::FOLDER_SPECS,
            BlockRef::Env(_) => &crate::params::ENV_SPECS,
            BlockRef::Lfo => &crate::dsp::lfo::LFO_SPECS,
            BlockRef::Out => &crate::params::OUT_SPECS,
            BlockRef::Chorus => &crate::dsp::chorus::CHORUS_SPECS,
            BlockRef::Delay => &crate::dsp::delay::DELAY_SPECS,
            BlockRef::Reverb => &crate::dsp::reverb::REVERB_SPECS,
            BlockRef::Tape => &crate::dsp::tape::TAPE_SPECS,
            BlockRef::Comp => &crate::dsp::comp::COMP_SPECS,
            BlockRef::Part => &crate::part::PART_SPECS,
            BlockRef::Theme => &crate::ui::theme_settings::THEME_SPECS,
        }
    }

    /// Whether `Voice::render` reads this block from its modulated copy and
    /// hears it without a route of its own. ENV slots feed the matrix and are
    /// not read as destinations until their LEVEL, TIME, RISE, FALL and SHAPE
    /// open (filter-routing Task 9); the LFO is read unmodulated; FX run outside
    /// `Voice`.
    pub const fn voice_reads(self) -> bool {
        match self {
            BlockRef::Modal
            | BlockRef::Algo
            | BlockRef::AlgoOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::Out => true,
            BlockRef::Env(_)
            | BlockRef::Lfo
            | BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::Theme => false,
        }
    }
}

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

    pub fn spec(self) -> Option<&'static ParamSpec> {
        find_spec(self.block.specs(), self.param)
    }

    /// Modulation may target this address: the spec says the value is read
    /// per block *and* `Voice` reads this block instance (plan D7).
    pub fn modulatable(self) -> bool {
        self.block.voice_reads() && self.spec().is_some_and(|s| s.modulatable)
    }
}

/// Resolves block addresses to values (spec § Data model). A Sound's
/// `ParamSnapshot` holds the voice blocks; a Part view (`PartEdit`) adds the
/// shared FX. `None`: the address is not held here.
pub trait Blocks {
    fn block(&self, b: BlockRef) -> Option<&dyn Block>;
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block>;
}
