//! TAPE's parameters (spec § 3), block `BlockRef::TapeEngine`: ids frozen
//! once written.

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::algo::math::exp2;

/// How a note plays its source. Disk codes 0–5; 6 is TRON's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapeModel {
    Loop = 0,
    Reverse = 1,
    PingPong = 2,
    Transwave = 3,
    Scrub = 4,
    Stretch = 5,
}

impl TapeModel {
    pub const ALL: [TapeModel; 6] = [
        TapeModel::Loop,
        TapeModel::Reverse,
        TapeModel::PingPong,
        TapeModel::Transwave,
        TapeModel::Scrub,
        TapeModel::Stretch,
    ];

    /// By the choice's value; past the end, the last.
    pub fn from_index(v: u8) -> Self {
        Self::ALL[usize::from(v).min(Self::ALL.len() - 1)]
    }
}

impl DiskCode for TapeModel {
    fn disk_code(self) -> u8 {
        match self {
            TapeModel::Loop => 0,
            TapeModel::Reverse => 1,
            TapeModel::PingPong => 2,
            TapeModel::Transwave => 3,
            TapeModel::Scrub => 4,
            TapeModel::Stretch => 5,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            TapeModel::Loop => "LOOP",
            TapeModel::Reverse => "REVERSE",
            TapeModel::PingPong => "PING_PONG",
            TapeModel::Transwave => "TRANSWAVE",
            TapeModel::Scrub => "SCRUB",
            TapeModel::Stretch => "STRETCH",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(TapeModel::Loop),
            1 => Some(TapeModel::Reverse),
            2 => Some(TapeModel::PingPong),
            3 => Some(TapeModel::Transwave),
            4 => Some(TapeModel::Scrub),
            5 => Some(TapeModel::Stretch),
            _ => None,
        }
    }
}

/// STRETCH's splice: in phase with the old grain (MONO), or at the time
/// pointer (POLY).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StretchType {
    Mono = 0,
    Poly = 1,
}

impl DiskCode for StretchType {
    fn disk_code(self) -> u8 {
        match self {
            StretchType::Mono => 0,
            StretchType::Poly => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            StretchType::Mono => "MONO",
            StretchType::Poly => "POLY",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(StretchType::Mono),
            1 => Some(StretchType::Poly),
            _ => None,
        }
    }
}

/// MODEL's names, by `TapeModel as u8`.
pub const MODEL_NAMES: [&str; 6] = ["LOOP", "REVERSE", "PINGPONG", "WAVE", "SCRUB", "STRETCH"];

/// The source is not a param: it is the Sound's `SourceRef` record. START,
/// END, POSITION and SIZE are fractions; the window is `[START, END]` of the
/// source, END kept ≥ START + 2 ms where it plays (`heads::window_end`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapeEngineParams {
    pub model: TapeModel,
    pub start: f32,
    pub end: f32,
    pub position: f32,
    /// −1 to +1, 0 still.
    pub motion: f32,
    pub tape: f32,
    pub age: f32,
    pub size: f32,
    /// 0 to ½ of the loop.
    pub xfade: f32,
    /// SCRUB's inertia: `drag_s`.
    pub drag: f32,
    pub kind: StretchType,
    /// The source's root, MIDI cents (6 000 = C4), copied at pick.
    pub root: f32,
    /// The source's period, frames, copied at pick; 0 unpitched.
    pub period: f32,
}

/// Neutral (`Sound::neutral(Tape)`, a format contract): the whole source
/// looped with a folded 10 % crossfade.
impl Default for TapeEngineParams {
    fn default() -> Self {
        Self {
            model: TapeModel::Loop,
            start: 0.0,
            end: 1.0,
            position: 0.0,
            motion: 0.0,
            tape: 0.0,
            age: 0.0,
            size: 1.0,
            xfade: 0.1,
            drag: 0.3,
            kind: StretchType::Mono,
            root: 6_000.0,
            period: 0.0,
        }
    }
}

impl TapeEngineParams {
    pub const MODEL: ParamId = ParamId(0);
    pub const START: ParamId = ParamId(1);
    pub const END: ParamId = ParamId(2);
    pub const POSITION: ParamId = ParamId(3);
    pub const MOTION: ParamId = ParamId(4);
    pub const TAPE: ParamId = ParamId(5);
    pub const AGE: ParamId = ParamId(6);
    pub const SIZE: ParamId = ParamId(7);
    pub const XFADE: ParamId = ParamId(8);
    pub const DRAG: ParamId = ParamId(9);
    pub const TYPE: ParamId = ParamId(10);
    pub const ROOT: ParamId = ParamId(11);
    pub const PERIOD: ParamId = ParamId(12);
}

/// DRAG in seconds: 10 to 500 ms, exponential.
pub fn drag_s(drag: f32) -> f32 {
    // log2(50)
    0.01 * exp2(5.643_856 * drag)
}

const fn unit(id: u8, label: &'static str, default: f32, modulatable: bool) -> ParamSpec {
    ParamSpec::continuous(
        id,
        label,
        ValFmt::Uni,
        0.0,
        1.0,
        default,
        1.0 / 128.0,
        modulatable,
    )
}

/// The five macros are modulatable (read every block, eased); the rest is
/// read at note-on or at a wrap. ROOT and PERIOD have no cell: a pick
/// copies them from the source.
pub static TAPE_ENGINE_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(0, "MODEL", ValFmt::Names(&MODEL_NAMES), 5.0, 0.0)
        .ident("MODEL")
        .glyph(crate::ui::glyph::FocusGlyph::None),
    unit(1, "START", 0.0, false).ident("START"),
    unit(2, "END", 1.0, false).ident("END"),
    unit(3, "POSITION", 0.0, true)
        .short("POS")
        .ident("POSITION"),
    ParamSpec::continuous(4, "MOTION", ValFmt::Bi, -1.0, 1.0, 0.0, 1.0 / 64.0, true)
        .short("MOT")
        .ident("MOTION"),
    unit(5, "TAPE", 0.0, true).ident("TAPE"),
    unit(6, "AGE", 0.0, true).ident("AGE"),
    unit(7, "SIZE", 1.0, true).ident("SIZE"),
    ParamSpec::continuous(8, "XFADE", ValFmt::Uni, 0.0, 0.5, 0.1, 1.0 / 256.0, false)
        .short("XFD")
        .ident("XFADE"),
    unit(9, "DRAG", 0.3, false).ident("DRAG"),
    ParamSpec::choice(10, "TYPE", ValFmt::Names(&["MONO", "POLY"]), 1.0, 0.0)
        .ident("TYPE")
        .glyph(crate::ui::glyph::FocusGlyph::None),
    ParamSpec::continuous(11, "ROOT", ValFmt::Uni, 0.0, 12_700.0, 6_000.0, 1.0, false)
        .ident("ROOT"),
    ParamSpec::continuous(12, "PERIOD", ValFmt::Uni, 0.0, 65_536.0, 0.0, 1.0, false)
        .ident("PERIOD"),
];

impl Block for TapeEngineParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &TAPE_ENGINE_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODEL => self.model as u8 as f32,
            Self::START => self.start,
            Self::END => self.end,
            Self::POSITION => self.position,
            Self::MOTION => self.motion,
            Self::TAPE => self.tape,
            Self::AGE => self.age,
            Self::SIZE => self.size,
            Self::XFADE => self.xfade,
            Self::DRAG => self.drag,
            Self::TYPE => self.kind as u8 as f32,
            Self::ROOT => self.root,
            Self::PERIOD => self.period,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODEL => self.model = TapeModel::from_index(v as u8),
            Self::START => self.start = v,
            Self::END => self.end = v,
            Self::POSITION => self.position = v,
            Self::MOTION => self.motion = v,
            Self::TAPE => self.tape = v,
            Self::AGE => self.age = v,
            Self::SIZE => self.size = v,
            Self::XFADE => self.xfade = v,
            Self::DRAG => self.drag = v,
            Self::TYPE => {
                self.kind = if v as u8 == 0 {
                    StretchType::Mono
                } else {
                    StretchType::Poly
                }
            }
            Self::ROOT => self.root = v,
            Self::PERIOD => self.period = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::MODEL => Some(self.model.disk_code()),
            Self::TYPE => Some(self.kind.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::MODEL => Some(self.model.disk_ident()),
            Self::TYPE => Some(self.kind.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::MODEL => apply_code(TapeModel::from_disk_code(code), |m| self.model = m),
            Self::TYPE => apply_code(StretchType::from_disk_code(code), |k| self.kind = k),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_runs_10_to_500_ms() {
        assert!((drag_s(0.0) - 0.01).abs() < 1e-7);
        assert!((drag_s(1.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn specs_default_to_neutral() {
        let p = TapeEngineParams::default();
        for s in &TAPE_ENGINE_SPECS {
            assert_eq!(p.get(s.id), s.default, "{}", s.label);
        }
    }
}
