//! A Part's mix settings (instrument-core spec § Data model): the MIDI
//! channel it listens on, Mono/Poly, the DAC pair it plays out of, level
//! and pan, and how its played keys follow ORBIT (SNAP, HELD).
//! One `Block`, so it gets pages and snap like any other.

use crate::MidiChannel;
use crate::block::{
    Block, Change, DiskCode, Motion, ParamId, ParamSpec, Smoothing, ValFmt, apply_code,
};
use crate::orbit::ring::SnapTo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PartMode {
    /// One voice, retriggered by each note; never stolen.
    Mono = 0,
    /// Voices from the shared pool.
    Poly = 1,
}

impl DiskCode for PartMode {
    fn disk_code(self) -> u8 {
        match self {
            PartMode::Mono => 0,
            PartMode::Poly => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            PartMode::Mono => "MONO",
            PartMode::Poly => "POLY",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(PartMode::Mono),
            1 => Some(PartMode::Poly),
            _ => None,
        }
    }
}

impl PartMode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => PartMode::Mono,
            _ => PartMode::Poly,
        }
    }
}

/// One of the three stereo DAC outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DacPair {
    P1 = 0,
    P2 = 1,
    P3 = 2,
}

impl DiskCode for DacPair {
    fn disk_code(self) -> u8 {
        match self {
            DacPair::P1 => 0,
            DacPair::P2 => 1,
            DacPair::P3 => 2,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            DacPair::P1 => "P1",
            DacPair::P2 => "P2",
            DacPair::P3 => "P3",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(DacPair::P1),
            1 => Some(DacPair::P2),
            2 => Some(DacPair::P3),
            _ => None,
        }
    }
}

impl DacPair {
    pub const ALL: [DacPair; crate::hw::DAC_PAIRS] = [DacPair::P1, DacPair::P2, DacPair::P3];

    pub const fn index(self) -> usize {
        self as usize
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => DacPair::P1,
            1 => DacPair::P2,
            _ => DacPair::P3,
        }
    }
}

/// What a played key snaps to while ORBIT runs (spec § 5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LiveSnap {
    /// As played: the neutral default (ADR 0045).
    Off = 0,
    Scale = 1,
    Chord = 2,
}

impl LiveSnap {
    pub const ALL: [LiveSnap; 3] = [LiveSnap::Off, LiveSnap::Scale, LiveSnap::Chord];

    /// The set a ring would snap to, as `LiveSnap` names it; `None`: OFF.
    pub const fn target(self) -> Option<SnapTo> {
        match self {
            LiveSnap::Off => None,
            LiveSnap::Scale => Some(SnapTo::Scale),
            LiveSnap::Chord => Some(SnapTo::Chord),
        }
    }
}

impl DiskCode for LiveSnap {
    fn disk_code(self) -> u8 {
        match self {
            LiveSnap::Off => 0,
            LiveSnap::Scale => 1,
            LiveSnap::Chord => 2,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            LiveSnap::Off => "OFF",
            LiveSnap::Scale => "SCALE",
            LiveSnap::Chord => "CHORD",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        LiveSnap::ALL.into_iter().find(|s| s.disk_code() == c)
    }
}

/// What a held key does at a chord change (spec § 5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Held {
    /// Keeps its pitch until the key is released: the neutral default.
    Stay = 0,
    /// Glides to its raw key snapped to the new chord (`RESNAP_GLIDE_MS`).
    Resnap = 1,
}

impl Held {
    pub const ALL: [Held; 2] = [Held::Stay, Held::Resnap];
}

impl DiskCode for Held {
    fn disk_code(self) -> u8 {
        match self {
            Held::Stay => 0,
            Held::Resnap => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Held::Stay => "STAY",
            Held::Resnap => "RE_SNAP",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Held::ALL.into_iter().find(|h| h.disk_code() == c)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartParams {
    pub channel: MidiChannel,
    pub mode: PartMode,
    pub output: DacPair,
    /// 0..1
    pub level: f32,
    /// -1 (left) .. 1 (right)
    pub pan: f32,
    pub snap: LiveSnap,
    pub held: Held,
    /// Pitch-bend range, whole semitones each way, 0..=`MAX_BEND`.
    pub bend: u8,
    /// TUNING: an index into `TUNING_NAMES` (ADR 0240); `TUNING_SCALA`
    /// plays the Part's Scala file, by name (ADR 0259).
    pub tuning: u8,
    /// MAPPING (ADR 0259): a Scala file's degrees on consecutive keys (0,
    /// CONTINUOUS) or from each C, the spare keys silent (1, KEYBOARD).
    pub mapping: u8,
}

/// LEVEL `level` under expression `e` (CC 11, ADR 0263): (e / 127)², GM's
/// 40 log10 law; `level` itself, bit for bit, at 127.
pub fn expressed(level: f32, e: u8) -> f32 {
    let e = f32::from(e) / 127.0;
    level * (e * e)
}

/// TUNING's choices: 12-TET, the named temperaments
/// (`chimera_theory::tuning::BUILT_IN`), the MTS table, then a Scala file
/// from the card, the Part's own (`Part::scala`, ADR 0259).
pub const TUNING_NAMES: [&str; 7] = [
    "12-TET", "JUST", "PYTHAG", "MEANTONE", "WERCK", "MTS", "SCALA",
];
/// TUNING's MTS choice.
pub const TUNING_MTS: u8 = 5;
/// TUNING's Scala file choice: the card's files follow MTS.
pub const TUNING_SCALA: u8 = 6;
/// SCALA's disk code: 6–13 were the retired `TUNE1`–`TUNE8` (ADR 0259),
/// and codes are never reused.
const SCALA_CODE: u8 = 14;
/// MAPPING's choices (ADR 0259): CONTINUOUS and KEYBOARD, cut to fit
/// the cell (KEYBD as the filter's).
pub const MAPPING_NAMES: [&str; 2] = ["CONTIN.", "KEYBD"];

/// The widest BEND: two octaves.
pub const MAX_BEND: u8 = 24;
/// BEND's default: the usual two semitones.
pub const INIT_BEND: u8 = 2;

impl PartParams {
    pub const CHANNEL: ParamId = ParamId(0);
    pub const MODE: ParamId = ParamId(1);
    pub const OUTPUT: ParamId = ParamId(2);
    pub const LEVEL: ParamId = ParamId(3);
    pub const PAN: ParamId = ParamId(4);
    pub const SNAP: ParamId = ParamId(8);
    pub const HELD: ParamId = ParamId(9);
    pub const BEND: ParamId = ParamId(10);
    pub const TUNING: ParamId = ParamId(11);
    pub const MAPPING: ParamId = ParamId(12);

    /// Part `index` (0-based) listens on channel `index`, Poly, output P1,
    /// level 0.8, centre pan, its keys as played.
    pub fn for_part(index: usize) -> Self {
        Self {
            channel: MidiChannel::clamped(index as u8),
            mode: PartMode::Poly,
            output: DacPair::P1,
            level: 0.8,
            pan: 0.0,
            snap: LiveSnap::Off,
            held: Held::Stay,
            bend: INIT_BEND,
            tuning: 0,
            mapping: 0,
        }
    }
}

impl Default for PartParams {
    fn default() -> Self {
        Self::for_part(0)
    }
}

/// Nothing here is modulatable: the mixer applies these, not the voice (ADR 0010).
pub static PART_SPECS: [ParamSpec; 10] = [
    ParamSpec::choice(0, "CH", ValFmt::OneBased(15), 15.0, 0.0, Change::Snap).ident("CH"),
    ParamSpec::choice(
        1,
        "MODE",
        ValFmt::Names(&["MONO", "POLY"]),
        1.0,
        1.0,
        Change::NoteOn,
    )
    .ident("MODE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        2,
        "OUT",
        ValFmt::Names(&["P1", "P2", "P3"]),
        2.0,
        0.0,
        Change::Snap,
    )
    .ident("OUT")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::continuous(
        3,
        "LEVEL",
        ValFmt::Uni,
        0.0,
        1.0,
        0.8,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("LEVEL")
    .glyph(crate::glyph::FocusGlyph::LevelBar),
    ParamSpec::continuous(
        4,
        "PAN",
        ValFmt::Pan,
        -1.0,
        1.0,
        0.0,
        2.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("PAN"),
    ParamSpec::choice(
        8,
        "SNAP",
        ValFmt::Names(&["OFF", "SCALE", "CHORD"]),
        2.0,
        0.0,
        Change::NoteOn,
    )
    .ident("SNAP")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        9,
        "HELD",
        ValFmt::Names(&["STAY", "RE-SNAP"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("HELD")
    .glyph(crate::glyph::FocusGlyph::None),
    // The audio glides the wheel's reach itself (`Instrument::ease_bend`).
    ParamSpec::stepped(
        10,
        "BEND",
        ValFmt::Int(MAX_BEND),
        0.0,
        MAX_BEND as f32,
        INIT_BEND as f32,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("BEND"),
    // A new note plays it: one sounding keeps its pitch.
    ParamSpec::choice(
        11,
        "TUNING",
        ValFmt::Names(&TUNING_NAMES),
        (TUNING_NAMES.len() - 1) as f32,
        0.0,
        Change::NoteOn,
    )
    .ident("TUNING")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        12,
        "MAPPING",
        ValFmt::Names(&MAPPING_NAMES),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("MAPPING")
    .glyph(crate::glyph::FocusGlyph::None),
];

/// 0 to 15 as text: CH's values by channel number (the byte is the
/// meaning), and the digits ORB and the algorithm's operators draw.
pub const NUMERALS: [&str; 16] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
];
const _: () = assert!(NUMERALS.len() == PART_SPECS[0].max as usize + 1);

impl Block for PartParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &PART_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::CHANNEL => self.channel.get() as f32,
            Self::MODE => self.mode as u8 as f32,
            Self::OUTPUT => self.output as u8 as f32,
            Self::LEVEL => self.level,
            Self::PAN => self.pan,
            Self::SNAP => self.snap as u8 as f32,
            Self::HELD => self.held as u8 as f32,
            Self::BEND => f32::from(self.bend),
            Self::TUNING => f32::from(self.tuning),
            Self::MAPPING => f32::from(self.mapping),
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CHANNEL => self.channel = MidiChannel::clamped(v as u8),
            Self::MODE => self.mode = PartMode::from_u8(v as u8),
            Self::OUTPUT => self.output = DacPair::from_u8(v as u8),
            Self::LEVEL => self.level = v,
            Self::PAN => self.pan = v,
            Self::SNAP => {
                let i = (v as usize).min(LiveSnap::ALL.len() - 1);
                self.snap = LiveSnap::ALL[i];
            }
            Self::HELD => self.held = Held::ALL[(v as usize).min(Held::ALL.len() - 1)],
            Self::BEND => self.bend = crate::dsp::clampf(v, 0.0, f32::from(MAX_BEND)) as u8,
            Self::TUNING => self.tuning = (v as u8).min(TUNING_NAMES.len() as u8 - 1),
            Self::MAPPING => self.mapping = (v as u8).min(1),
            _ => {}
        }
    }

    /// CH's code is the channel, 0..=15.
    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::CHANNEL => Some(self.channel.get()),
            Self::MODE => Some(self.mode.disk_code()),
            Self::OUTPUT => Some(self.output.disk_code()),
            Self::SNAP => Some(self.snap.disk_code()),
            Self::HELD => Some(self.held.disk_code()),
            // The choice's index, SCALA's its own (`SCALA_CODE`).
            Self::TUNING if self.tuning == TUNING_SCALA => Some(SCALA_CODE),
            Self::TUNING => Some(self.tuning),
            Self::MAPPING => Some(self.mapping),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::CHANNEL => NUMERALS.get(usize::from(self.channel.get())).copied(),
            Self::MODE => Some(self.mode.disk_ident()),
            Self::OUTPUT => Some(self.output.disk_ident()),
            Self::SNAP => Some(self.snap.disk_ident()),
            Self::HELD => Some(self.held.disk_ident()),
            Self::TUNING => TUNING_NAMES.get(usize::from(self.tuning)).copied(),
            // Whole words on disk, whatever the cell shows.
            Self::MAPPING => ["CONTINUOUS", "KEYBOARD"]
                .get(usize::from(self.mapping))
                .copied(),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::CHANNEL => apply_code(MidiChannel::new(code), |c| self.channel = c),
            Self::MODE => apply_code(PartMode::from_disk_code(code), |m| self.mode = m),
            Self::OUTPUT => apply_code(DacPair::from_disk_code(code), |o| self.output = o),
            Self::SNAP => apply_code(LiveSnap::from_disk_code(code), |s| self.snap = s),
            Self::HELD => apply_code(Held::from_disk_code(code), |h| self.held = h),
            Self::TUNING => apply_code(
                match code {
                    SCALA_CODE => Some(TUNING_SCALA),
                    c if c < TUNING_SCALA => Some(c),
                    _ => None,
                },
                |t| self.tuning = t,
            ),
            Self::MAPPING => apply_code((code < 2).then_some(code), |m| self.mapping = m),
            _ => false,
        }
    }
}
