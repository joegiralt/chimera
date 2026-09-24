//! Semantic parameter addresses (spec §2): an address names *what* a
//! parameter is, not where it sits on a page or in a chain, so rearranging
//! cells or reordering blocks never remaps a mod route.

use crate::block::{find_spec, ParamId, ParamSpec};

/// An FM operator. `TryFrom<u8>` rejects values above 3, so an out-of-range
/// operator (bad sound or SysEx data) is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    A,
    B,
    C,
    D,
}

/// Rejected operator index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpOutOfRange(pub u8);

impl Op {
    pub const ALL: [Op; 4] = [Op::A, Op::B, Op::C, Op::D];

    /// Index into `FmParams::operators`.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Step through the operators by encoder ticks, clamped at A and D.
    pub fn nudged(self, delta: i8) -> Op {
        Op::ALL[(self.index() as i16 + delta as i16).clamp(0, 3) as usize]
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
    Pizza,
    Modal,
    Fm,
    FmOp(Op),
    Drive,
    Filter,
    Folder,
    /// `envelopes[0]`
    AmpEnv,
    /// `envelopes[1]`
    FilterEnv,
    /// `envelopes[2]`
    AuxEnv,
    Lfo,
    /// `OutParams { volume, pan }`
    Out,
    /// Chorus, delay and reverb run outside `Voice` (desktop only).
    Chorus,
    Delay,
    Reverb,
}

impl BlockRef {
    pub const ALL: [BlockRef; 18] = [
        BlockRef::Pizza,
        BlockRef::Modal,
        BlockRef::Fm,
        BlockRef::FmOp(Op::A),
        BlockRef::FmOp(Op::B),
        BlockRef::FmOp(Op::C),
        BlockRef::FmOp(Op::D),
        BlockRef::Drive,
        BlockRef::Filter,
        BlockRef::Folder,
        BlockRef::AmpEnv,
        BlockRef::FilterEnv,
        BlockRef::AuxEnv,
        BlockRef::Lfo,
        BlockRef::Out,
        BlockRef::Chorus,
        BlockRef::Delay,
        BlockRef::Reverb,
    ];

    /// The block type's spec table (static; no instance needed).
    pub fn specs(self) -> &'static [ParamSpec] {
        match self {
            BlockRef::Pizza => &crate::dsp::pizza::PIZZA_SPECS,
            BlockRef::Modal => &crate::dsp::modal::MODAL_SPECS,
            BlockRef::Fm => &crate::params::FM_SPECS,
            BlockRef::FmOp(_) => &crate::params::FM_OP_SPECS,
            BlockRef::Drive => &crate::params::DRIVE_SPECS,
            BlockRef::Filter => &crate::params::FILTER_SPECS,
            BlockRef::Folder => &crate::params::FOLDER_SPECS,
            BlockRef::AmpEnv | BlockRef::FilterEnv | BlockRef::AuxEnv => &crate::params::ENV_SPECS,
            BlockRef::Lfo => &crate::dsp::lfo::LFO_SPECS,
            BlockRef::Out => &crate::params::OUT_SPECS,
            BlockRef::Chorus => &crate::dsp::chorus::CHORUS_SPECS,
            BlockRef::Delay => &crate::dsp::delay::DELAY_SPECS,
            BlockRef::Reverb => &crate::dsp::reverb::REVERB_SPECS,
        }
    }

    /// Whether `Voice::render` reads this block from its modulated copy.
    /// Filter/aux envelopes are never read; the LFO is read unmodulated; FX
    /// run outside `Voice`.
    pub const fn voice_reads(self) -> bool {
        match self {
            BlockRef::Pizza
            | BlockRef::Modal
            | BlockRef::Fm
            | BlockRef::FmOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::AmpEnv
            | BlockRef::Out => true,
            BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb => false,
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
