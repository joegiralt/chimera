//! VOICEFX (block 36): the slot's TYPE and PLACE, its three pots and MIX
//! (ADR 0155). What each pot does is the type's; the block only holds them.

use crate::block::{Block, Change, DiskCode, ParamId, ParamSpec, Smoothing, ValFmt, apply_code};
use crate::glyph::FocusGlyph;
use crate::hw::Cost;

/// What the slot runs. Codes are forever: 4 is DELAY's, not built yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum FxType {
    #[default]
    Off = 0,
    Soft = 1,
    Fuzz = 2,
    Fold = 3,
}

impl FxType {
    /// TYPE's order on the knob.
    pub const LIST: [FxType; 4] = [FxType::Off, FxType::Soft, FxType::Fuzz, FxType::Fold];
    /// TYPE's screen names, in `LIST`'s order.
    pub const NAMES: [&'static str; 4] = ["OFF", "SOFT", "FUZZ", "FOLD"];

    /// Where the type sits when picked from OFF: a drive or fuzz before
    /// the filter, a fold after it.
    pub const fn default_place(self) -> Place {
        match self {
            FxType::Off | FxType::Soft | FxType::Fuzz => Place::Pre,
            FxType::Fold => Place::Post,
        }
    }

    /// `v` as an index into `LIST`, held in range.
    pub fn from_index(v: f32) -> FxType {
        Self::LIST[(v.max(0.0) as usize).min(Self::LIST.len() - 1)]
    }

    /// Whether the type's kernel is bank 2's (FUZZ, ADR 0157), so it
    /// passes dry while bank 2 can't play.
    pub const fn in_bank2(self) -> bool {
        matches!(self, FxType::Fuzz)
    }

    /// Its place in `LIST`.
    const fn index(self) -> usize {
        match self {
            FxType::Off => 0,
            FxType::Soft => 1,
            FxType::Fuzz => 2,
            FxType::Fold => 3,
        }
    }

    /// Whether the slot runs: once P1 is 0.001 or more, or `moved` (a route
    /// on P1, or FOLD's tap) may raise it. OFF never; FUZZ always, as the
    /// pedal at SUSTAIN 0 still fuzzes. The bill and the gate read the same
    /// answer (`Voice::fx_runs`).
    pub fn runs(self, p1: f32, moved: bool) -> bool {
        match self {
            FxType::Off => false,
            FxType::Fuzz => true,
            FxType::Soft | FxType::Fold => p1 >= 0.001 || moved,
        }
    }

    /// The type running steady.
    /// - SOFT, today's DRV: the unit reads 58 (2026-10-19, rev V, ROUTING
    ///   bench, run 2, `/VOICE` slope); the host-era bench said 57 (#642).
    /// - FOLD: the bench's FOLD row less 1 OP (measured 2026-09-28, rev V
    ///   at 480 MHz), 43, and the offset of silence's fold (ADR 0060), 1
    ///   instruction a sample, 1.6 by ADR 0056's host method. LEVEL's
    ///   multiply adds about 1: re-measured on the unit in Task 13.
    /// - FUZZ: the host count, `tools/loop_count.py` on the ORBIT ELF's
    ///   `Fuzz::steady` (ADR 0056): 186 instructions and 4 `vdiv` a round of
    ///   two samples, 93 and 2 a sample (ADR 0156), 175. The unit's `FUZZ`
    ///   row (`bench.rs`, less 1 OP) is PENDING (unit); FUZZ plus the bare
    ///   chain stays under the ceiling, 271 - 95.
    /// - The unit, 2026-10-22 (ROUTING bench, rev V at 480 MHz, each row
    ///   less 1 OP): SOFT 84 and SOFT LO 96, FOLD 80, FUZZ 244. Billed 100,
    ///   80 and 245: FUZZ as it reads, over ADR 0156's ceiling, which ADR
    ///   0211 lifts (the owner keeps its sound and its fewer voices).
    pub const fn cost(self) -> Cost {
        match self {
            FxType::Off => Cost::ZERO,
            FxType::Soft => Cost(90), // SOFT LO reads 90 over 1 OP (ADR 0218)
            FxType::Fuzz => Cost(245),
            FxType::Fold => Cost(80),
        }
    }

    /// A route on P1–P3 or MIX, or FOLD's tap: the type ramps every block.
    /// - SOFT: 36 instructions a sample against the steady 22; the unit
    ///   reads 26 (84 − 58; 2026-10-19, rev V, ROUTING bench, run 2,
    ///   `/VOICE` slope), the host count 23 (#642).
    /// - FOLD: recounted on the bench ELF at 1879169d (2026-10-07): the
    ///   moving loop 45 instructions a sample (silence's fold per sample,
    ///   ADR 0060) + 3.48 of block, 80, against the steady 24 + 3.48, 45.
    /// - FUZZ: the moving loop, counted on the ORBIT ELF's `Fuzz::run` (the
    ///   per-sample `tick` with four settings stepped and the gains formed):
    ///   123 instructions and 2 `vdiv` a sample, 225, over the steady 175:
    ///   50. The unit's `FUZZ MOVE` row is PENDING (unit).
    ///
    /// Unrouted, only the UI moves them: brief, in the headroom (ADR 0061).
    pub const fn ramp_cost(self) -> Cost {
        match self {
            FxType::Off => Cost::ZERO,
            FxType::Soft => Cost(26),
            FxType::Fuzz => Cost(50),
            FxType::Fold => Cost(35),
        }
    }

    /// A knob glide's blocks: four steady loops' prologues, a few
    /// instructions each, no divide: under 1 a sample, billed 1 (ADR 0117).
    /// FUZZ's four prologues each take two `exp2`s (SUSTAIN and VOLUME)
    /// and the call, about 45 instructions: 3 a sample, billed 5 (a
    /// margin; the unit's reading is PENDING).
    pub const fn glide_cost(self) -> Cost {
        match self {
            FxType::Off => Cost::ZERO,
            FxType::Fuzz => Cost(5),
            FxType::Soft | FxType::Fold => Cost(1),
        }
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
            FxType::Fuzz => "FUZZ",
            FxType::Fold => "FOLD",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(FxType::Off),
            1 => Some(FxType::Soft),
            2 => Some(FxType::Fuzz),
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

    /// A TYPE turn: leaving OFF for a type takes its place; any later pick keeps
    /// yours. A load sets both as stored (`set_enum_code`).
    pub fn set_type(&mut self, t: FxType) {
        if self.ty == FxType::Off && t != FxType::Off {
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
    .ident("P1")
    // AMOUNT is wider than a matrix header.
    .short("AMT"),
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
    .ident("P2")
    .short("SHAP"),
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
