//! TAPE's parameters (spec § 3), block `BlockRef::TapeEngine`: ids frozen
//! once written.

use super::oneshot::SliceCode;
use crate::block::{
    Block, Change, DiskCode, ParamId, ParamSpec, Smoothing, ValFmt, apply_code, identity_code,
};
use crate::dsp::algo::math::exp2;

/// How a note plays its source: the machines (spec § 2 Machines). Each
/// discriminant is its disk code; 1 (REVERSE) is retired, and loads as
/// LOOP with END before START (`translate_v1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapeModel {
    Loop = 0,
    PingPong = 2,
    Transwave = 3,
    Scrub = 4,
    Stretch = 5,
    Tron = 6,
    Oneshot = 7,
}

impl TapeModel {
    /// MODEL's screen order: its value is the index here.
    pub const ALL: [TapeModel; 7] = [
        TapeModel::Oneshot,
        TapeModel::Loop,
        TapeModel::Tron,
        TapeModel::Stretch,
        TapeModel::Transwave,
        TapeModel::Scrub,
        TapeModel::PingPong,
    ];

    /// MODEL's value: the machine's place on screen.
    pub fn index(self) -> u8 {
        match self {
            TapeModel::Oneshot => 0,
            TapeModel::Loop => 1,
            TapeModel::Tron => 2,
            TapeModel::Stretch => 3,
            TapeModel::Transwave => 4,
            TapeModel::Scrub => 5,
            TapeModel::PingPong => 6,
        }
    }

    /// By MODEL's value; past the end, the last.
    pub fn from_index(v: u8) -> Self {
        Self::ALL[usize::from(v).min(Self::ALL.len() - 1)]
    }

    /// Order is direction: END before START plays reversed. The others
    /// take the sorted window.
    pub fn ordered(self) -> bool {
        matches!(
            self,
            TapeModel::Oneshot | TapeModel::Loop | TapeModel::Tron | TapeModel::Stretch
        )
    }
}

impl DiskCode for TapeModel {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            TapeModel::Loop => "LOOP",
            TapeModel::PingPong => "PING_PONG",
            TapeModel::Transwave => "TRANSWAVE",
            TapeModel::Scrub => "SCRUB",
            TapeModel::Stretch => "STRETCH",
            TapeModel::Tron => "TRON",
            TapeModel::Oneshot => "ONESHOT",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.disk_code() == c)
    }
}

/// MODEL's retired code: REVERSE, now LOOP with END before START.
pub const REVERSE_CODE: u8 = 1;

/// The speed's deviation under TAPE: the capstan's transport (ADR 0053),
/// or CHOMPI's Warble (`warble`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Wow {
    Capstan = 0,
    Warble = 1,
}

impl DiskCode for Wow {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Wow::Capstan => "CAPSTAN",
            Wow::Warble => "WARBLE",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Wow::Capstan),
            1 => Some(Wow::Warble),
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

/// MODEL's names, by `TapeModel::index`: each machine's page name, as
/// STRETCH and PING-PONG overrun a cell's bar.
pub const MODEL_NAMES: [&str; 7] = ["ONE", "LOOP", "TRON", "STRCH", "WAVE", "SCRUB", "PING"];

/// The machines' names in full, by `TapeModel::index`: the model page's
/// header.
pub const MACHINE_NAMES: [&str; 7] = [
    "ONESHOT",
    "LOOP",
    "TRON",
    "STRETCH",
    "TRANSWAVE",
    "SCRUB",
    "PING-PONG",
];

/// SLICE's names, by its value.
pub const SLICE_NAMES: [&str; 18] = [
    "OFF", "NOTE", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
    "16",
];

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
    /// ONESHOT's SLICE.
    pub slice: SliceCode,
    pub wow: Wow,
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
            slice: SliceCode::OFF,
            wow: Wow::Capstan,
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
    pub const SLICE: ParamId = ParamId(13);
    pub const WOW: ParamId = ParamId(14);
}

/// Whether `model` reads `id`: the one table for cells, dimming and the
/// audio test, from the machines' (`machine::MACHINES`, whose every page
/// ends in WOW). START, END, TAPE and AGE read on every machine; ROOT and
/// PERIOD have no cell.
pub fn reads(model: TapeModel, id: ParamId) -> bool {
    type P = TapeEngineParams;
    match id {
        P::MODEL | P::START | P::END | P::TAPE | P::AGE | P::ROOT | P::PERIOD => true,
        _ => super::machine::cell_of(model, id).is_some(),
    }
}

/// DRAG in seconds: 10 to 500 ms, exponential.
pub fn drag_s(drag: f32) -> f32 {
    // log2(50)
    0.01 * exp2(5.643_856 * drag)
}

const fn unit(
    id: u8,
    label: &'static str,
    default: f32,
    modulatable: bool,
    smoothing: Smoothing,
) -> ParamSpec {
    ParamSpec::continuous(
        id,
        label,
        ValFmt::Uni,
        0.0,
        1.0,
        default,
        1.0 / 128.0,
        modulatable,
        smoothing,
    )
}

/// The five macros are modulatable (read every block, eased); the rest is
/// read at note-on or at a wrap. ROOT and PERIOD have no cell: a pick
/// copies them from the source.
pub static TAPE_ENGINE_SPECS: [ParamSpec; 15] = [
    // The neutral is LOOP: index 1.
    ParamSpec::choice(
        0,
        "MODEL",
        ValFmt::Names(&MODEL_NAMES),
        6.0,
        1.0,
        Change::Crossfade,
    )
    .ident("MODEL")
    .glyph(crate::glyph::FocusGlyph::None),
    unit(1, "START", 0.0, false, Smoothing::NoteOn).ident("START"),
    unit(2, "END", 1.0, false, Smoothing::NoteOn).ident("END"),
    unit(3, "POSITION", 0.0, true, Smoothing::KNOB)
        .short("POS")
        .ident("POSITION"),
    ParamSpec::continuous(
        4,
        "MOTION",
        ValFmt::Bi,
        -1.0,
        1.0,
        0.0,
        1.0 / 64.0,
        true,
        Smoothing::KNOB,
    )
    .short("MOT")
    .ident("MOTION"),
    unit(5, "TAPE", 0.0, true, Smoothing::KNOB).ident("TAPE"),
    unit(6, "AGE", 0.0, true, Smoothing::KNOB).ident("AGE"),
    unit(7, "SIZE", 1.0, true, Smoothing::KNOB).ident("SIZE"),
    ParamSpec::continuous(
        8,
        "XFADE",
        ValFmt::Uni,
        0.0,
        0.5,
        0.1,
        1.0 / 256.0,
        false,
        Smoothing::NoteOn,
    )
    .short("XFD")
    .ident("XFADE"),
    unit(9, "DRAG", 0.3, false, Smoothing::NoteOn).ident("DRAG"),
    ParamSpec::choice(
        10,
        "TYPE",
        ValFmt::Names(&["MONO", "POLY"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("TYPE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::continuous(
        11,
        "ROOT",
        ValFmt::Uni,
        0.0,
        12_700.0,
        6_000.0,
        1.0,
        false,
        Smoothing::NoteOn,
    )
    .ident("ROOT"),
    ParamSpec::continuous(
        12,
        "PERIOD",
        ValFmt::Uni,
        0.0,
        65_536.0,
        0.0,
        1.0,
        false,
        Smoothing::NoteOn,
    )
    .ident("PERIOD"),
    ParamSpec::choice(
        13,
        "SLICE",
        ValFmt::Names(&SLICE_NAMES),
        17.0,
        0.0,
        Change::NoteOn,
    )
    .short("SLC")
    .ident("SLICE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        14,
        "WOW",
        ValFmt::Names(&["CAPSTAN", "WARBLE"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("WOW")
    .glyph(crate::glyph::FocusGlyph::None),
];

impl Block for TapeEngineParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &TAPE_ENGINE_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODEL => self.model.index() as f32,
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
            Self::SLICE => self.slice.code() as f32,
            Self::WOW => self.wow as u8 as f32,
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
            Self::SLICE => self.slice = SliceCode::new(v as u8),
            Self::WOW => {
                self.wow = if v as u8 == 0 {
                    Wow::Capstan
                } else {
                    Wow::Warble
                }
            }
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::MODEL => Some(self.model.disk_code()),
            Self::TYPE => Some(self.kind.disk_code()),
            Self::SLICE => Some(self.slice.code()),
            Self::WOW => Some(self.wow.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::MODEL => Some(self.model.disk_ident()),
            Self::TYPE => Some(self.kind.disk_ident()),
            Self::SLICE => Some(SLICE_IDENTS[usize::from(self.slice.code())]),
            Self::WOW => Some(self.wow.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::MODEL => apply_code(TapeModel::from_disk_code(code), |m| self.model = m),
            Self::TYPE => apply_code(StretchType::from_disk_code(code), |k| self.kind = k),
            Self::SLICE => apply_code(identity_code(&TAPE_ENGINE_SPECS, id, code), |c| {
                self.slice = SliceCode::new(c)
            }),
            Self::WOW => apply_code(Wow::from_disk_code(code), |w| self.wow = w),
            _ => false,
        }
    }
}

/// SLICE's frozen idents, by code.
const SLICE_IDENTS: [&str; 18] = [
    "OFF", "NOTE", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S12",
    "S13", "S14", "S15", "S16",
];

/// A file from before the machines (TAPE Task 12b, #369): MODEL's retired
/// REVERSE (code 1) is LOOP with START and END swapped, which plays it bit
/// for bit. Run after the file's writes, so a sparse Sound's base START 0,
/// END 1 becomes START 1, END 0.
pub fn translate_v1(old: &crate::storage::Retired, b: &mut dyn Block) {
    type P = TapeEngineParams;
    if old.code(P::MODEL) != Some(REVERSE_CODE) {
        return;
    }
    let _ = b.set_enum_code(P::MODEL, TapeModel::Loop.disk_code());
    let (start, end) = (b.get(P::START), b.get(P::END));
    b.set(P::START, end);
    b.set(P::END, start);
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
