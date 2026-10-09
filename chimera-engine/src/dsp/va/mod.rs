//! VA: two variable-wave oscillators and a combine stage (spec § Pages,
//! § Params and Motion). This is the params block; the engine is added
//! on top of it.

use crate::block::{
    Block, Change, DiskCode, Motion, ParamId, ParamSpec, Smoothing, ValFmt, apply_code,
};
use crate::dsp::clampf;
use crate::glyph::FocusGlyph;

/// One of the two oscillators: the index into `VaParams`' pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OscId {
    One = 0,
    Two = 1,
}

impl OscId {
    pub const ALL: [OscId; 2] = [OscId::One, OscId::Two];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How the two oscillators combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum VaMode {
    #[default]
    Mix = 0,
    Sync = 1,
    Ring = 2,
    Xor = 3,
    Bits = 4,
}

impl VaMode {
    pub const ALL: [VaMode; 5] = [
        VaMode::Mix,
        VaMode::Sync,
        VaMode::Ring,
        VaMode::Xor,
        VaMode::Bits,
    ];
}

/// MODE's names, in `VaMode`'s order (display only).
pub const MODE_NAMES: [&str; 5] = ["MIX", "SYNC", "RING", "XOR", "BITS"];

impl DiskCode for VaMode {
    fn disk_code(self) -> u8 {
        match self {
            VaMode::Mix => 0,
            VaMode::Sync => 1,
            VaMode::Ring => 2,
            VaMode::Xor => 3,
            VaMode::Bits => 4,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            VaMode::Mix => "MIX",
            VaMode::Sync => "SYNC",
            VaMode::Ring => "RING",
            VaMode::Xor => "XOR",
            VaMode::Bits => "BITS",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(VaMode::Mix),
            1 => Some(VaMode::Sync),
            2 => Some(VaMode::Ring),
            3 => Some(VaMode::Xor),
            4 => Some(VaMode::Bits),
            _ => None,
        }
    }
}

/// The VA Sound's parameters; pairs are indexed by `OscId::index`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VaParams {
    pub wave: [f32; 2],
    pub shape: [f32; 2],
    pub coarse: [i8; 2],
    /// Cents.
    pub fine: [f32; 2],
    pub mode: VaMode,
    pub amount: f32,
    pub sub: f32,
    pub stability: f32,
}

/// The format's neutral (ADR 0087): a saved Sound omits what is neutral.
impl Default for VaParams {
    fn default() -> Self {
        Self {
            wave: [2.0 / 3.0; 2],
            shape: [0.0; 2],
            coarse: [0; 2],
            fine: [0.0; 2],
            mode: VaMode::Mix,
            amount: 0.5,
            sub: 0.0,
            stability: 1.0,
        }
    }
}

impl VaParams {
    pub const WAVE1: ParamId = ParamId(0);
    pub const WAVE2: ParamId = ParamId(1);
    pub const MODE: ParamId = ParamId(2);
    pub const AMOUNT: ParamId = ParamId(3);
    pub const SUB: ParamId = ParamId(4);
    pub const STAB: ParamId = ParamId(5);
    pub const CRSE1: ParamId = ParamId(6);
    pub const FINE1: ParamId = ParamId(7);
    pub const SHAPE1: ParamId = ParamId(8);
    pub const CRSE2: ParamId = ParamId(9);
    pub const FINE2: ParamId = ParamId(10);
    pub const SHAPE2: ParamId = ParamId(11);
}

const fn uni(id: u8, label: &'static str, ident: &'static str, default: f32) -> ParamSpec {
    ParamSpec::continuous(
        id,
        label,
        ValFmt::Uni,
        0.0,
        1.0,
        default,
        1.0 / 128.0,
        true,
        Smoothing::KNOB,
    )
    .ident(ident)
}

const fn coarse(id: u8, label: &'static str, short: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Signed(24),
        -24.0,
        24.0,
        0.0,
        true,
        Motion::Stepped(Change::Snap),
    )
    .ident(label)
    .short(short)
    .semitones(24.0)
}

const fn fine(id: u8, label: &'static str, short: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Signed(100),
        -100.0,
        100.0,
        0.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident(label)
    .short(short)
    .cents(100.0)
}

pub static VA_SPECS: [ParamSpec; 12] = [
    uni(0, "WAVE1", "WAVE1", 2.0 / 3.0).short("WV1"),
    uni(1, "WAVE2", "WAVE2", 2.0 / 3.0).short("WV2"),
    ParamSpec::choice(
        2,
        "MODE",
        ValFmt::Names(&MODE_NAMES),
        4.0,
        0.0,
        Change::Crossfade,
    )
    .ident("MODE")
    .glyph(FocusGlyph::None),
    uni(3, "AMNT", "AMOUNT", 0.5).glyph(FocusGlyph::Crossfader),
    uni(4, "SUB", "SUB", 0.0).glyph(FocusGlyph::LevelBar),
    uni(5, "STAB", "STAB", 1.0),
    coarse(6, "CRSE1", "CR1"),
    fine(7, "FINE1", "FN1"),
    uni(8, "SHAPE1", "SHAPE1", 0.0).short("SH1"),
    coarse(9, "CRSE2", "CR2"),
    fine(10, "FINE2", "FN2"),
    uni(11, "SHAPE2", "SHAPE2", 0.0).short("SH2"),
];

impl Block for VaParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &VA_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVE1 => self.wave[0],
            Self::WAVE2 => self.wave[1],
            Self::MODE => self.mode.disk_code() as f32,
            Self::AMOUNT => self.amount,
            Self::SUB => self.sub,
            Self::STAB => self.stability,
            Self::CRSE1 => self.coarse[0] as f32,
            Self::FINE1 => self.fine[0],
            Self::SHAPE1 => self.shape[0],
            Self::CRSE2 => self.coarse[1] as f32,
            Self::FINE2 => self.fine[1],
            Self::SHAPE2 => self.shape[1],
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        // A modulated CRSE is stepped; NaN rounds to 0 (`as` saturates).
        let semis = || clampf(libm::roundf(v), -24.0, 24.0) as i8;
        match id {
            Self::WAVE1 => self.wave[0] = v,
            Self::WAVE2 => self.wave[1] = v,
            Self::MODE => self.mode = VaMode::from_disk_code(v as u8).unwrap_or_default(),
            Self::AMOUNT => self.amount = v,
            Self::SUB => self.sub = v,
            Self::STAB => self.stability = v,
            Self::CRSE1 => self.coarse[0] = semis(),
            Self::FINE1 => self.fine[0] = v,
            Self::SHAPE1 => self.shape[0] = v,
            Self::CRSE2 => self.coarse[1] = semis(),
            Self::FINE2 => self.fine[1] = v,
            Self::SHAPE2 => self.shape[1] = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::MODE).then(|| self.mode.disk_code())
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        (id == Self::MODE).then(|| self.mode.disk_ident())
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::MODE && apply_code(VaMode::from_disk_code(code), |m| self.mode = m)
    }
}
