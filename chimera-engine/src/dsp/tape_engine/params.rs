//! TAPE's parameters (spec § 3), block `BlockRef::TapeEngine`: ids frozen
//! once written.

use super::oneshot::SliceCode;
use crate::block::{
    Block, Change, DiskCode, ParamId, ParamSpec, Smoothing, ValFmt, apply_code, identity_code,
};
use crate::dsp::algo::math::exp2;

/// How a note plays its source: the machines (spec § 2 Machines). Each
/// discriminant is its disk code. PLAY is LOOP's code 0; ONESHOT (7),
/// PING-PONG (2) and REVERSE (1) are retired and load as PLAY
/// (`translate_v1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapeModel {
    Play = 0,
    Transwave = 3,
    Scrub = 4,
    Stretch = 5,
    Tron = 6,
}

impl TapeModel {
    /// Every model; MODEL lists them as `TapeMachine`s.
    pub const ALL: [TapeModel; 5] = [
        TapeModel::Play,
        TapeModel::Tron,
        TapeModel::Scrub,
        TapeModel::Stretch,
        TapeModel::Transwave,
    ];

    /// The model's place in `ALL`.
    pub fn index(self) -> u8 {
        match self {
            TapeModel::Play => 0,
            TapeModel::Tron => 1,
            TapeModel::Scrub => 2,
            TapeModel::Stretch => 3,
            TapeModel::Transwave => 4,
        }
    }

    /// By `index`; past the end, the last.
    pub fn from_index(v: u8) -> Self {
        Self::ALL[usize::from(v).min(Self::ALL.len() - 1)]
    }

    /// Order is direction: END before START plays reversed. PING and the
    /// others take the sorted window.
    pub fn ordered(self, mode: PlayMode) -> bool {
        match self {
            TapeModel::Play => mode != PlayMode::Ping,
            TapeModel::Tron | TapeModel::Stretch => true,
            TapeModel::Scrub | TapeModel::Transwave => false,
        }
    }
}

impl DiskCode for TapeModel {
    fn disk_code(self) -> u8 {
        self as u8
    }

    // PLAY keeps LOOP's frozen ident.
    fn disk_ident(self) -> &'static str {
        match self {
            TapeModel::Play => "LOOP",
            TapeModel::Transwave => "TRANSWAVE",
            TapeModel::Scrub => "SCRUB",
            TapeModel::Stretch => "STRETCH",
            TapeModel::Tron => "TRON",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.disk_code() == c)
    }
}

/// MODEL's retired codes: REVERSE, PING-PONG and ONESHOT, which load as
/// PLAY (`translate_v1`).
pub const REVERSE_CODE: u8 = 1;
pub const PING_PONG_CODE: u8 = 2;
pub const ONESHOT_CODE: u8 = 7;

/// How PLAY plays: once to END, around a loop, or back and forth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayMode {
    One = 0,
    Loop = 1,
    Ping = 2,
}

impl DiskCode for PlayMode {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            PlayMode::One => "ONE",
            PlayMode::Loop => "LOOP",
            PlayMode::Ping => "PING",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(PlayMode::One),
            1 => Some(PlayMode::Loop),
            2 => Some(PlayMode::Ping),
            _ => None,
        }
    }
}

/// What MODEL lists (#651, ADR 0178): PLAY's three MODEs each a machine
/// of their own beside the others. The block keeps PLAY and its MODE, so
/// the disk codes hold; MODEL's value is the index here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapeMachine {
    One,
    Loop,
    Ping,
    Tron,
    Scrub,
    Stretch,
    Wave,
}

impl TapeMachine {
    /// MODEL's screen order.
    pub const ALL: [TapeMachine; 7] = [
        TapeMachine::One,
        TapeMachine::Loop,
        TapeMachine::Ping,
        TapeMachine::Tron,
        TapeMachine::Scrub,
        TapeMachine::Stretch,
        TapeMachine::Wave,
    ];

    pub const fn of(model: TapeModel, mode: PlayMode) -> Self {
        match (model, mode) {
            (TapeModel::Play, PlayMode::One) => TapeMachine::One,
            (TapeModel::Play, PlayMode::Loop) => TapeMachine::Loop,
            (TapeModel::Play, PlayMode::Ping) => TapeMachine::Ping,
            (TapeModel::Tron, _) => TapeMachine::Tron,
            (TapeModel::Scrub, _) => TapeMachine::Scrub,
            (TapeModel::Stretch, _) => TapeMachine::Stretch,
            (TapeModel::Transwave, _) => TapeMachine::Wave,
        }
    }

    pub const fn model(self) -> TapeModel {
        match self {
            TapeMachine::One | TapeMachine::Loop | TapeMachine::Ping => TapeModel::Play,
            TapeMachine::Tron => TapeModel::Tron,
            TapeMachine::Scrub => TapeModel::Scrub,
            TapeMachine::Stretch => TapeModel::Stretch,
            TapeMachine::Wave => TapeModel::Transwave,
        }
    }

    /// PLAY's MODE; `None` off PLAY.
    pub const fn mode(self) -> Option<PlayMode> {
        match self {
            TapeMachine::One => Some(PlayMode::One),
            TapeMachine::Loop => Some(PlayMode::Loop),
            TapeMachine::Ping => Some(PlayMode::Ping),
            _ => None,
        }
    }

    pub const fn index(self) -> u8 {
        self as u8
    }

    /// By MODEL's value; past the end, the last.
    pub fn from_index(v: u8) -> Self {
        Self::ALL[usize::from(v).min(Self::ALL.len() - 1)]
    }
}

/// FADE's last step: note-off is ignored and a ONE note plays to END.
pub const FADE_HOLD: f32 = 1.0;

/// FADE's default in `mode`: ONE holds (a drum one-shot), the others
/// release over 32 ms.
pub fn fade_default(mode: PlayMode) -> f32 {
    if mode == PlayMode::One {
        FADE_HOLD
    } else {
        0.3
    }
}

/// `p` holds: PLAY in MODE ONE with FADE at HOLD. No other MODE has the step.
pub fn hold(p: &TapeEngineParams) -> bool {
    p.model == TapeModel::Play && p.mode == PlayMode::One && p.drag >= FADE_HOLD
}

/// SLICE NOTE: a ONE note plays at ROOT, whatever its key.
pub fn at_root(p: &TapeEngineParams) -> bool {
    p.model == TapeModel::Play && p.mode == PlayMode::One && p.slice.slice().by_note()
}

/// Why a streamed source can't play `p` (slice 1 § 2.3): streams read
/// forward only until slice 5, and the other machines seek.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedsRam {
    Ping,
    /// END before START.
    Reverse,
    /// SCRUB, STRETCH and WAVE.
    Machine(TapeModel),
}

/// `None`: ONE, LOOP or TRON, END at or after START. TRON's note is ONE's
/// path, one head forward to END, so it streams as ONE does (#698).
pub fn needs_ram(p: &TapeEngineParams) -> Option<NeedsRam> {
    match (p.model, p.mode) {
        (TapeModel::Play, PlayMode::Ping) => Some(NeedsRam::Ping),
        (TapeModel::Play | TapeModel::Tron, _) if p.end < p.start => Some(NeedsRam::Reverse),
        (TapeModel::Play | TapeModel::Tron, _) => None,
        (m, _) => Some(NeedsRam::Machine(m)),
    }
}

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

/// ONE's RETRIG (#698): a note on the Part STACKs on its others, or CUTs
/// them (an open hat choked by a closed one, MPC's mute group within a
/// Part) until KIT's choke groups land. A key struck again always cuts its
/// own note (the allocator's re-strike).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Retrig {
    Stack = 0,
    Cut = 1,
}

impl DiskCode for Retrig {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Retrig::Stack => "STACK",
            Retrig::Cut => "CUT",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Retrig::Stack),
            1 => Some(Retrig::Cut),
            _ => None,
        }
    }
}

/// A note on `p`'s Part cuts its other notes: ONE at RETRIG CUT.
pub fn cuts(p: &TapeEngineParams) -> bool {
    p.machine() == TapeMachine::One && p.retrig == Retrig::Cut
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

/// MODEL's names, by `TapeMachine::index`: each machine's page name, as
/// STRETCH and TRANSWAVE overrun a cell's bar.
pub const MODEL_NAMES: [&str; 7] = ["ONE", "LOOP", "PING", "TRON", "SCRUB", "STRCH", "WAVE"];

/// The machines' names in full, by `TapeMachine::index`: the model page's
/// header.
pub const MACHINE_NAMES: [&str; 7] = [
    "ONE",
    "LOOP",
    "PING",
    "TRON",
    "SCRUB",
    "STRETCH",
    "TRANSWAVE",
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
    /// PLAY's MODE: MODEL's ONE, LOOP or PING (`machine`).
    pub mode: PlayMode,
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
    /// SCRUB's inertia, PLAY's and TRON's FADE: `drag_s`.
    pub drag: f32,
    pub kind: StretchType,
    /// The source's root, MIDI cents (6 000 = C4), copied at pick.
    pub root: f32,
    /// The source's period, frames, copied at pick; 0 unpitched.
    pub period: f32,
    /// ONESHOT's SLICE: retired, kept at OFF.
    pub slice: SliceCode,
    pub wow: Wow,
    /// ONE's RETRIG.
    pub retrig: Retrig,
}

/// Neutral (`Sound::neutral(Tape)`, a format contract): the whole source
/// looped with a folded 10 % crossfade.
impl Default for TapeEngineParams {
    fn default() -> Self {
        Self {
            model: TapeModel::Play,
            mode: PlayMode::Loop,
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
            retrig: Retrig::Stack,
        }
    }
}

impl TapeEngineParams {
    /// The machine MODEL shows: the model and, on PLAY, its MODE.
    pub const fn machine(&self) -> TapeMachine {
        TapeMachine::of(self.model, self.mode)
    }

    /// MODEL's write: the machine's model and, on PLAY, its MODE.
    pub fn set_machine(&mut self, m: TapeMachine) {
        self.model = m.model();
        if let Some(mode) = m.mode() {
            self.mode = mode;
        }
    }

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
    pub const MODE: ParamId = ParamId(15);
    pub const RETRIG: ParamId = ParamId(16);
}

/// Whether `m` reads `id`: the one table for cells, dimming and the audio
/// test, from the machines' (`machine::MACHINES`). START, END, TAPE, AGE
/// and WOW read on every machine; ROOT and PERIOD have no cell.
pub fn reads(m: TapeMachine, id: ParamId) -> bool {
    type P = TapeEngineParams;
    match id {
        P::MODEL
        | P::MODE
        | P::START
        | P::END
        | P::TAPE
        | P::AGE
        | P::WOW
        | P::ROOT
        | P::PERIOD => true,
        _ => super::machine::cell_of(m, id).is_some(),
    }
}

/// `reads` for the machine `model` plays under PLAY's `mode`: ONE plays
/// one head to END, so POSITION, WALK, LENGTH and XFADE do nothing there;
/// PING has no crossfade.
pub fn reads_in(model: TapeModel, mode: PlayMode, id: ParamId) -> bool {
    reads(TapeMachine::of(model, mode), id)
}

/// Whether `model` moves `id` by a motion of its own the knobs would only
/// blur: SCRUB's POSITION is its jog, which the UI steps with DRAG's
/// momentum (ADR 0112). The Part's knobs hand it on at once.
pub fn jogs(model: TapeModel, id: ParamId) -> bool {
    model == TapeModel::Scrub && id == TapeEngineParams::POSITION
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

/// Every machine cell shows the machine's glyph (ADR 0173).
const TAPE: crate::glyph::FocusGlyph = crate::glyph::FocusGlyph::Tape;

/// The five macros are modulatable (read every block, eased), and START,
/// latched by ONE's and TRON's first block; the rest is read at note-on or
/// at a wrap. ROOT and PERIOD have no cell: a pick
/// copies them from the source.
pub static TAPE_ENGINE_SPECS: [ParamSpec; 17] = [
    // The neutral is PLAY in MODE LOOP: LOOP, index 1.
    ParamSpec::choice(
        0,
        "MODEL",
        ValFmt::Names(&MODEL_NAMES),
        6.0,
        1.0,
        Change::Crossfade,
    )
    .ident("MODEL")
    .glyph(TAPE),
    // A matrix destination, latched at note-on: VEL → START (#698).
    unit(1, "START", 0.0, true, Smoothing::NoteOn).ident("START"),
    unit(2, "END", 1.0, false, Smoothing::NoteOn).ident("END"),
    unit(3, "POSITION", 0.0, true, Smoothing::KNOB)
        .short("POS")
        .ident("POSITION")
        .glyph(TAPE),
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
    .ident("MOTION")
    .glyph(TAPE),
    unit(5, "TAPE", 0.0, true, Smoothing::KNOB)
        .ident("TAPE")
        .glyph(TAPE),
    unit(6, "AGE", 0.0, true, Smoothing::KNOB)
        .ident("AGE")
        .glyph(TAPE),
    unit(7, "SIZE", 1.0, true, Smoothing::KNOB)
        .ident("SIZE")
        .glyph(TAPE),
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
    .ident("XFADE")
    .glyph(TAPE),
    unit(9, "DRAG", 0.3, false, Smoothing::NoteOn)
        .ident("DRAG")
        .glyph(TAPE),
    ParamSpec::choice(
        10,
        "TYPE",
        ValFmt::Names(&["MONO", "POLY"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("TYPE")
    .glyph(TAPE),
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
    .glyph(TAPE),
    // PLAY's MODE: no cell, MODEL writes it (ADR 0178).
    ParamSpec::choice(
        15,
        "MODE",
        ValFmt::Names(&["ONE", "LOOP", "PING"]),
        2.0,
        1.0,
        Change::NoteOn,
    )
    .ident("MODE")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        16,
        "RETRIG",
        ValFmt::Names(&["STACK", "CUT"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .short("RTG")
    .ident("RETRIG")
    .glyph(crate::glyph::FocusGlyph::None),
];

impl Block for TapeEngineParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &TAPE_ENGINE_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODEL => self.machine().index() as f32,
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
            Self::MODE => self.mode as u8 as f32,
            Self::RETRIG => self.retrig as u8 as f32,
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODEL => self.set_machine(TapeMachine::from_index(v as u8)),
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
            Self::MODE => self.mode = PlayMode::from_disk_code(v as u8).unwrap_or(PlayMode::Ping),
            Self::RETRIG => {
                self.retrig = if v as u8 == 0 {
                    Retrig::Stack
                } else {
                    Retrig::Cut
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
            Self::MODE => Some(self.mode.disk_code()),
            Self::RETRIG => Some(self.retrig.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::MODEL => Some(self.model.disk_ident()),
            Self::TYPE => Some(self.kind.disk_ident()),
            Self::SLICE => Some(SLICE_IDENTS[usize::from(self.slice.code())]),
            Self::WOW => Some(self.wow.disk_ident()),
            Self::MODE => Some(self.mode.disk_ident()),
            Self::RETRIG => Some(self.retrig.disk_ident()),
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
            Self::MODE => apply_code(PlayMode::from_disk_code(code), |m| self.mode = m),
            Self::RETRIG => apply_code(Retrig::from_disk_code(code), |r| self.retrig = r),
            _ => false,
        }
    }
}

/// SLICE's frozen idents, by code.
const SLICE_IDENTS: [&str; 18] = [
    "OFF", "NOTE", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S12",
    "S13", "S14", "S15", "S16",
];

/// A file from before PLAY: MODEL's retired codes load as PLAY (#369, ADR
/// 0170). REVERSE (1) is LOOP with START and END swapped, which plays it bit
/// for bit; ONESHOT (7) is MODE ONE with FADE at HOLD and no SLICE; PING-PONG
/// (2) is MODE PING. Run after the file's writes, so a sparse Sound's base
/// START 0, END 1 becomes START 1, END 0.
pub fn translate_v1(old: &crate::storage::Retired, b: &mut dyn Block) {
    type P = TapeEngineParams;
    let mode = match old.code(P::MODEL) {
        Some(REVERSE_CODE) => PlayMode::Loop,
        Some(PING_PONG_CODE) => PlayMode::Ping,
        Some(ONESHOT_CODE) => PlayMode::One,
        _ => return,
    };
    let _ = b.set_enum_code(P::MODEL, TapeModel::Play.disk_code());
    let _ = b.set_enum_code(P::MODE, mode.disk_code());
    match old.code(P::MODEL) {
        Some(REVERSE_CODE) => {
            let (start, end) = (b.get(P::START), b.get(P::END));
            b.set(P::START, end);
            b.set(P::END, start);
        }
        Some(ONESHOT_CODE) => {
            b.set(P::DRAG, FADE_HOLD);
            b.set(P::SLICE, 0.0);
        }
        _ => {}
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
