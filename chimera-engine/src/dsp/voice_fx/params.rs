//! VOICEFX (block 36): the slot's TYPE and PLACE, its three pots and MIX
//! (ADR 0155). What each pot does is the type's; the block only holds them.

use crate::block::{Block, Change, DiskCode, ParamId, ParamSpec, Smoothing, ValFmt, apply_code};
use crate::glyph::FocusGlyph;

/// What the slot runs. Codes are forever: 2 is FUZZ's and 4 DELAY's, neither
/// built yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum FxType {
    #[default]
    Off = 0,
    Soft = 1,
    Fold = 3,
}

impl FxType {
    /// TYPE's order on the knob.
    pub const LIST: [FxType; 3] = [FxType::Off, FxType::Soft, FxType::Fold];
    /// TYPE's screen names, in `LIST`'s order.
    pub const NAMES: [&'static str; 3] = ["OFF", "SOFT", "FOLD"];

    /// Where the type sits when picked from OFF: a drive before the
    /// filter, a fold after it.
    pub const fn default_place(self) -> Place {
        match self {
            FxType::Off | FxType::Soft => Place::Pre,
            FxType::Fold => Place::Post,
        }
    }

    /// `v` as an index into `LIST`, held in range.
    pub fn from_index(v: f32) -> FxType {
        Self::LIST[(v.max(0.0) as usize).min(Self::LIST.len() - 1)]
    }

    fn index(self) -> usize {
        Self::LIST.iter().position(|&t| t == self).unwrap_or(0)
    }
}

impl DiskCode for FxType {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            FxType::Off => "OFF",
            FxType::Soft => "SOFT",
            FxType::Fold => "FOLD",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(FxType::Off),
            1 => Some(FxType::Soft),
            3 => Some(FxType::Fold),
            _ => None,
        }
    }
}

/// Which side of the filter the slot sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Place {
    #[default]
    Pre = 0,
    Post = 1,
}

impl DiskCode for Place {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Place::Pre => "PRE",
            Place::Post => "POST",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Place::Pre),
            1 => Some(Place::Post),
            _ => None,
        }
    }
}

/// The slot's stored settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceFxParams {
    /// Private: `set_type` moves PLACE with it.
    ty: FxType,
    pub place: Place,
    /// P1–P3, the type's own pots.
    pub pots: [f32; 3],
    pub mix: f32,
}

impl Default for VoiceFxParams {
    fn default() -> Self {
        Self {
            ty: FxType::Off,
            place: Place::Pre,
            pots: [0.0, 0.5, 0.5],
            mix: 1.0,
        }
    }
}

impl VoiceFxParams {
    pub const TYPE: ParamId = ParamId(0);
    pub const PLACE: ParamId = ParamId(1);
    pub const P1: ParamId = ParamId(2);
    pub const P2: ParamId = ParamId(3);
    pub const P3: ParamId = ParamId(4);
    pub const MIX: ParamId = ParamId(5);

    pub fn ty(&self) -> FxType {
        self.ty
    }

    /// A TYPE turn: leaving OFF takes the type's place; any later pick keeps
    /// yours. A load sets both as stored (`set_enum_code`).
    pub fn set_type(&mut self, t: FxType) {
        if self.ty == FxType::Off {
            self.place = t.default_place();
        }
        self.ty = t;
    }

    /// The pot's slot in `pots`, for P1–P3.
    const fn pot(id: ParamId) -> Option<usize> {
        match id.0 {
            2..=4 => Some(id.0 as usize - 2),
            _ => None,
        }
    }
}

/// P3's gain, as the filter's LEVEL: ×¼ to ×4, exactly 1 at ½.
#[inline]
pub fn pot_level(p3: f32) -> f32 {
    crate::dsp::filter::level_gain(p3)
}

pub static VOICE_FX_SPECS: [ParamSpec; 6] = [
    ParamSpec::choice(
        0,
        "FX",
        ValFmt::Names(&FxType::NAMES),
        (FxType::LIST.len() - 1) as f32,
        0.0,
        Change::Crossfade,
    )
    .ident("TYPE")
    .glyph(FocusGlyph::None),
    ParamSpec::choice(
        1,
        "PLACE",
        ValFmt::Names(&["PRE", "POST"]),
        1.0,
        0.0,
        Change::Crossfade,
    )
    .ident("PLACE")
    .glyph(FocusGlyph::None),
    ParamSpec::continuous(
        2,
        "AMOUNT",
        ValFmt::Uni,
        0.0,
        1.0,
        0.0,
        1.0 / 128.0,
        true,
        Smoothing::STAGE,
    )
    .ident("P1"),
    ParamSpec::continuous(
        3,
        "SHAPE",
        ValFmt::Bi,
        0.0,
        1.0,
        0.5,
        1.0 / 128.0,
        true,
        Smoothing::STAGE,
    )
    .ident("P2"),
    ParamSpec::continuous(
        4,
        "LEVEL",
        ValFmt::Uni,
        0.0,
        1.0,
        0.5,
        1.0 / 128.0,
        true,
        Smoothing::STAGE,
    )
    .ident("P3"),
    ParamSpec::continuous(
        5,
        "MIX",
        ValFmt::Bi,
        0.0,
        1.0,
        1.0,
        1.0 / 128.0,
        true,
        Smoothing::STAGE,
    )
    .ident("MIX")
    .glyph(FocusGlyph::Crossfader),
];

impl Block for VoiceFxParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &VOICE_FX_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match (id, Self::pot(id)) {
            (_, Some(i)) => self.pots[i],
            (Self::TYPE, _) => self.ty.index() as f32,
            (Self::PLACE, _) => self.place as u8 as f32,
            (Self::MIX, _) => self.mix,
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match (id, Self::pot(id)) {
            (_, Some(i)) => self.pots[i] = v,
            (Self::TYPE, _) => self.set_type(FxType::from_index(v)),
            (Self::PLACE, _) => self.place = if v >= 0.5 { Place::Post } else { Place::Pre },
            (Self::MIX, _) => self.mix = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::TYPE => Some(self.ty.disk_code()),
            Self::PLACE => Some(self.place.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::TYPE => Some(self.ty.disk_ident()),
            Self::PLACE => Some(self.place.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            // Stored as it was: a sparse record leaves out a PLACE equal to
            // its base's, so following the type here would move it.
            Self::TYPE => apply_code(FxType::from_disk_code(code), |t| self.ty = t),
            Self::PLACE => apply_code(Place::from_disk_code(code), |p| self.place = p),
            _ => false,
        }
    }
}
