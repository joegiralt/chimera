//! TAPE's parameters (spec § 3), block `BlockRef::TapeEngine`: ids frozen
//! once written.

use super::oneshot::SliceCode;
use super::stretch::units::{GrainLen, Length, Spray, StretchEnd};
use crate::block::{
    Block, Change, DiskCode, Motion, ParamId, ParamSpec, Smoothing, ValFmt, apply_code,
    identity_code,
};
use crate::dsp::algo::math::exp2;
use crate::dsp::modulator::law::Law;

/// How a note plays its source: the machines (spec § 2 Machines). Each
/// discriminant is its disk code. WALK is LOOP's code 0; ONESHOT (7),
/// PING-PONG (2), REVERSE (1) and TRON (6) are retired and load as WALK
/// (`translate_v1`, ADR 0203).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapeModel {
    Walk = 0,
    Transwave = 3,
    Scrub = 4,
    Stretch = 5,
}

impl TapeModel {
    /// Every model, in MODEL's order (`TapeMachine::ALL`).
    pub const ALL: [TapeModel; 4] = [
        TapeModel::Walk,
        TapeModel::Stretch,
        TapeModel::Scrub,
        TapeModel::Transwave,
    ];

    /// The model's place in `ALL`.
    pub fn index(self) -> u8 {
        match self {
            TapeModel::Walk => 0,
            TapeModel::Stretch => 1,
            TapeModel::Scrub => 2,
            TapeModel::Transwave => 3,
        }
    }
}

impl DiskCode for TapeModel {
    fn disk_code(self) -> u8 {
        self as u8
    }

    // WALK keeps LOOP's frozen ident.
    fn disk_ident(self) -> &'static str {
        match self {
            TapeModel::Walk => "LOOP",
            TapeModel::Transwave => "TRANSWAVE",
            TapeModel::Scrub => "SCRUB",
            TapeModel::Stretch => "STRETCH",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.disk_code() == c)
    }
}

/// `RetiredModel` and its `ALL` from one list: a variant cannot miss `ALL`.
macro_rules! retired_models {
    ($($v:ident = $c:literal),+ $(,)?) => {
        /// MODEL's retired codes: they load as WALK (`translate_v1`).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum RetiredModel {
            $($v = $c),+
        }

        impl RetiredModel {
            pub const ALL: [Self; [$(Self::$v),+].len()] = [$(Self::$v),+];
        }
    };
}

retired_models!(Reverse = 1, PingPong = 2, Tron = 6, Oneshot = 7);

/// DIRECTION's retired code: MODE ONE (0), which read neither POSITION nor
/// LENGTH. It loads as FORWARD from the window's start, whole (ADR 0203).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RetiredMode {
    One = 0,
}

impl RetiredMode {
    pub const fn code(self) -> u8 {
        self as u8
    }
}

impl RetiredModel {
    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|m| *m as u8 == c)
    }

    fn translate(self, b: &mut dyn Block) {
        type P = TapeEngineParams;
        let _ = b.set_enum_code(P::MODEL, TapeModel::Walk.disk_code());
        match self {
            Self::Reverse => {
                let _ = b.set_enum_code(P::MODE, Direction::Loop.disk_code());
                let (start, end) = (b.get(P::START), b.get(P::END));
                b.set(P::START, end);
                b.set(P::END, start);
            }
            Self::Oneshot => {
                one_as_forward(b);
                b.set(P::DRAG, FADE_HOLD);
                b.set(P::SLICE, 0.0);
            }
            Self::PingPong => {
                let _ = b.set_enum_code(P::MODE, Direction::Ping.disk_code());
            }
            // TRON read LENGTH as its tape and never POSITION.
            Self::Tron => {
                let _ = b.set_enum_code(P::MODE, Direction::Forward.disk_code());
                b.set(P::POSITION, 0.0);
                let _ = b.set_enum_code(P::TRON, Tron::On.disk_code());
            }
        }
    }
}

/// MODE ONE as FORWARD: from the window's start, all of it.
fn one_as_forward(b: &mut dyn Block) {
    type P = TapeEngineParams;
    let _ = b.set_enum_code(P::MODE, Direction::Forward.disk_code());
    b.set(P::POSITION, 0.0);
    b.set(P::SIZE, 1.0);
}

/// WALK's DIRECTION (ADR 0203): once forward, once reversed, around a
/// loop, or back and forth. Discriminants are DIRECTION's values (its
/// screen order); the disk codes are `disk_code`'s, LOOP's and PING's kept
/// from MODE and ONE's (0) retired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    Forward = 0,
    Reverse = 1,
    Loop = 2,
    Ping = 3,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::Forward,
        Direction::Reverse,
        Direction::Loop,
        Direction::Ping,
    ];

    /// FORWARD and REVERSE play the window once.
    pub const fn once(self) -> bool {
        matches!(self, Direction::Forward | Direction::Reverse)
    }
}

impl DiskCode for Direction {
    fn disk_code(self) -> u8 {
        match self {
            Direction::Loop => 1,
            Direction::Ping => 2,
            Direction::Forward => 3,
            Direction::Reverse => 4,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Direction::Forward => "FORWARD",
            Direction::Reverse => "REVERSE",
            Direction::Loop => "LOOP",
            Direction::Ping => "PING",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.disk_code() == c)
    }
}

impl Choice for Direction {
    fn from_value(v: f32) -> Option<Self> {
        Self::ALL.get(usize::from(v as u8)).copied()
    }

    fn value(self) -> f32 {
        f32::from(self as u8)
    }
}

/// WALK's DRIFT on its knob's grid, 1/128 a step: a byte, so the block
/// keeps its size (the voice holds a played copy).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Drift(u8);

/// Shown as its value, as a real param is.
impl core::fmt::Debug for Drift {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.get().fmt(f)
    }
}

impl Drift {
    pub const OFF: Drift = Drift(0);

    /// `v` (0..1) to its nearest step; a NaN is OFF.
    pub fn of(v: f32) -> Drift {
        let v = if v > 0.0 { v.min(1.0) } else { 0.0 };
        Drift(libm::roundf(v * 128.0) as u8)
    }

    /// 0..1.
    pub fn get(self) -> f32 {
        f32::from(self.0) * (1.0 / 128.0)
    }
}

/// WALK's TRON (ADR 0203): the Mellotron M400's tape. ON, a note's tape
/// runs out (LENGTH's, up to 8 s, on FORWARD and REVERSE; 8 s around a
/// loop) and key up lifts the pressure pad through FADE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Tron {
    Off = 0,
    On = 1,
}

impl DiskCode for Tron {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Tron::Off => "OFF",
            Tron::On => "ON",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Tron::Off),
            1 => Some(Tron::On),
            _ => None,
        }
    }
}

impl Choice for Tron {
    fn from_value(v: f32) -> Option<Self> {
        Self::from_disk_code(v as u8)
    }

    fn value(self) -> f32 {
        f32::from(self as u8)
    }
}

/// An enum param's value as the UI and the matrix speak it: its place in
/// the param's names. Every value names one variant, or none.
pub trait Choice: Sized + Copy {
    fn from_value(v: f32) -> Option<Self>;
    fn value(self) -> f32;
}

/// MODEL's value: a machine's place in `TapeMachine::ALL`, never its disk
/// code (`TapeModel`'s, ADR 0189).
///
/// ```compile_fail,E0308
/// use chimera_engine::block::DiskCode;
/// use chimera_engine::dsp::tape_engine::{TapeMachine, TapeModel};
/// let _ = TapeMachine::from_index(TapeModel::Transwave.disk_code());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineIndex(pub u8);

/// What MODEL lists (ADR 0203): the machines are verbs. WALK holds what
/// ONE, LOOP, PING and TRON were, as DIRECTION and TRON.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapeMachine {
    Walk,
    Stretch,
    Scrub,
    Wave,
}

impl TapeMachine {
    /// MODEL's screen order.
    pub const ALL: [TapeMachine; 4] = [
        TapeMachine::Walk,
        TapeMachine::Stretch,
        TapeMachine::Scrub,
        TapeMachine::Wave,
    ];

    pub const fn of(model: TapeModel) -> Self {
        match model {
            TapeModel::Walk => TapeMachine::Walk,
            TapeModel::Scrub => TapeMachine::Scrub,
            TapeModel::Stretch => TapeMachine::Stretch,
            TapeModel::Transwave => TapeMachine::Wave,
        }
    }

    pub const fn model(self) -> TapeModel {
        match self {
            TapeMachine::Walk => TapeModel::Walk,
            TapeMachine::Scrub => TapeModel::Scrub,
            TapeMachine::Stretch => TapeModel::Stretch,
            TapeMachine::Wave => TapeModel::Transwave,
        }
    }

    pub const fn index(self) -> u8 {
        self as u8
    }

    /// By MODEL's value; past the end, the last.
    pub fn from_index(v: MachineIndex) -> Self {
        Self::ALL[usize::from(v.0).min(Self::ALL.len() - 1)]
    }
}

impl Choice for TapeMachine {
    fn from_value(v: f32) -> Option<Self> {
        Self::ALL.get(usize::from(v as u8)).copied()
    }

    fn value(self) -> f32 {
        f32::from(self.index())
    }
}

/// How a note plays, the deck's path: WALK's DIRECTION and TRON split
/// into the paths ONE, LOOP, PING and TRON were, then the other machines.
/// TRON around a loop is LOOP's or PING's path with the tape's run-out
/// (`TapeEngineParams::tron`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    One,
    Loop,
    Ping,
    Tron,
    Scrub,
    Stretch,
    Wave,
}

impl Way {
    pub const ALL: [Way; 7] = [
        Way::One,
        Way::Loop,
        Way::Ping,
        Way::Tron,
        Way::Scrub,
        Way::Stretch,
        Way::Wave,
    ];

    pub const fn of(m: TapeMachine, d: Direction, tron: Tron) -> Way {
        match (m, d, tron) {
            (TapeMachine::Walk, Direction::Forward | Direction::Reverse, Tron::Off) => Way::One,
            (TapeMachine::Walk, Direction::Forward | Direction::Reverse, Tron::On) => Way::Tron,
            (TapeMachine::Walk, Direction::Loop, _) => Way::Loop,
            (TapeMachine::Walk, Direction::Ping, _) => Way::Ping,
            (TapeMachine::Scrub, ..) => Way::Scrub,
            (TapeMachine::Stretch, ..) => Way::Stretch,
            (TapeMachine::Wave, ..) => Way::Wave,
        }
    }

    /// The machine, DIRECTION and TRON that play `self`: ONE and TRON
    /// FORWARD, off WALK LOOP and TRON off (the neutral's).
    pub const fn parts(self) -> (TapeMachine, Direction, Tron) {
        let d = match self {
            Way::One | Way::Tron => Direction::Forward,
            Way::Ping => Direction::Ping,
            _ => Direction::Loop,
        };
        let t = if matches!(self, Way::Tron) {
            Tron::On
        } else {
            Tron::Off
        };
        (self.machine(), d, t)
    }

    pub const fn machine(self) -> TapeMachine {
        match self {
            Way::One | Way::Loop | Way::Ping | Way::Tron => TapeMachine::Walk,
            Way::Scrub => TapeMachine::Scrub,
            Way::Stretch => TapeMachine::Stretch,
            Way::Wave => TapeMachine::Wave,
        }
    }

    /// Order is direction: END before START plays reversed. PING and the
    /// others take the sorted window.
    pub const fn ordered(self) -> bool {
        match self {
            Way::One | Way::Loop | Way::Tron | Way::Stretch => true,
            Way::Ping | Way::Scrub | Way::Wave => false,
        }
    }
}

/// FADE's last step: note-off is ignored and a ONE note plays to END.
pub const FADE_HOLD: f32 = 1.0;

/// FADE's default in `d`: FORWARD and REVERSE hold (a drum one-shot), the
/// loops release over 32 ms.
pub const fn fade_default(d: Direction) -> f32 {
    if d.once() { FADE_HOLD } else { 0.3 }
}

/// `p` holds: WALK once through, TRON off, FADE at HOLD. Nothing else has
/// the step: TRON's pad always lifts.
pub fn hold(p: &TapeEngineParams) -> bool {
    p.way() == Way::One && p.drag >= FADE_HOLD
}

/// SLICE NOTE: a ONE note plays at ROOT, whatever its key.
pub fn at_root(p: &TapeEngineParams) -> bool {
    p.way() == Way::One && p.slice.slice().by_note()
}

/// A note on `p` plays at ROOT whatever its key: WALK under SLICE NOTE
/// (`at_root`), or SCRUB at KEY POSITION, where the key places the finger
/// (ADR 0205).
pub fn plays_at_root(p: &TapeEngineParams) -> bool {
    match p.machine {
        TapeMachine::Walk => at_root(p),
        TapeMachine::Scrub => p.key == KeyMode::Position,
        TapeMachine::Stretch | TapeMachine::Wave => false,
    }
}

/// The machines that seek: none streams (slice 1 § 2.7). WALK reads
/// forward unless it plays reversed or bounces.
///
/// ```compile_fail,E0599
/// use chimera_engine::dsp::tape_engine::params::{NeedsRam, Seeks};
/// let _ = NeedsRam::Machine(Seeks::Play);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seeks {
    Scrub,
    Stretch,
    Wave,
}

/// Why a streamed source can't play `p` (slice 1 § 2.3): streams read
/// forward only until slice 5, and the other machines seek.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedsRam {
    Ping,
    /// REVERSE, or END before START.
    Reverse,
    Machine(Seeks),
}

/// `p` plays its window backward: REVERSE with END after START, or
/// FORWARD or LOOP with END before it (#369).
pub fn backward(p: &TapeEngineParams) -> bool {
    (p.direction == Direction::Reverse) != (p.end < p.start)
}

/// `None`: WALK read forward, TRON or not. TRON's note is ONE's path, one
/// head forward to END, so it streams as ONE does (#698).
pub fn needs_ram(p: &TapeEngineParams) -> Option<NeedsRam> {
    match p.machine {
        TapeMachine::Walk if p.direction == Direction::Ping => Some(NeedsRam::Ping),
        TapeMachine::Walk if backward(p) => Some(NeedsRam::Reverse),
        TapeMachine::Walk => None,
        TapeMachine::Scrub => Some(NeedsRam::Machine(Seeks::Scrub)),
        TapeMachine::Stretch => Some(NeedsRam::Machine(Seeks::Stretch)),
        TapeMachine::Wave => Some(NeedsRam::Machine(Seeks::Wave)),
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

impl Choice for Wow {
    fn from_value(v: f32) -> Option<Self> {
        Self::from_disk_code(v as u8)
    }

    fn value(self) -> f32 {
        f32::from(self as u8)
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

impl Choice for Retrig {
    fn from_value(v: f32) -> Option<Self> {
        Self::from_disk_code(v as u8)
    }

    fn value(self) -> f32 {
        f32::from(self as u8)
    }
}

/// A note on `p`'s Part cuts its other notes: WALK once through at
/// RETRIG CUT, TRON off.
pub fn cuts(p: &TapeEngineParams) -> bool {
    p.way() == Way::One && p.retrig == Retrig::Cut
}

/// SCRUB's KEY (ADR 0205): a key places the finger and plays at ROOT
/// (POSITION), or plays the scrub at its pitch (PITCH).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyMode {
    Position = 0,
    Pitch = 1,
}

impl DiskCode for KeyMode {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            KeyMode::Position => "POSITION",
            KeyMode::Pitch => "PITCH",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(KeyMode::Position),
            1 => Some(KeyMode::Pitch),
            _ => None,
        }
    }
}

impl Choice for KeyMode {
    fn from_value(v: f32) -> Option<Self> {
        Self::from_disk_code(v as u8)
    }

    fn value(self) -> f32 {
        f32::from(self as u8)
    }
}

/// STRETCH's TYPE (ADR 0204): CLASSIC, the S950 splice in phase with the
/// old grain (MONO) or at the time pointer (POLY), or GRAIN, four grains
/// on the shared core. Codes append: a Sound from before GRAIN is CLASSIC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StretchType {
    Mono = 0,
    Poly = 1,
    Grain = 2,
}

impl DiskCode for StretchType {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        match self {
            StretchType::Mono => "MONO",
            StretchType::Poly => "POLY",
            StretchType::Grain => "GRAIN",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        STRETCH_TYPES.get(usize::from(c)).copied()
    }
}

/// By code (and so by value): the table, not a match, so TYPE's decoding is data.
const STRETCH_TYPES: [StretchType; 3] = [StretchType::Mono, StretchType::Poly, StretchType::Grain];

impl Choice for StretchType {
    fn from_value(v: f32) -> Option<Self> {
        Self::from_disk_code(v as u8)
    }

    fn value(self) -> f32 {
        f32::from(self as u8)
    }
}

/// MODEL's names, by `TapeMachine::index`: each machine's page name, as
/// STRETCH and TRANSWAVE overrun a cell's bar.
pub const MODEL_NAMES: [&str; 4] = ["WALK", "STRCH", "SCRUB", "WAVE"];

/// The machines' names in full, by `TapeMachine::index`: the model page's
/// header.
pub const MACHINE_NAMES: [&str; 4] = ["WALK", "STRETCH", "SCRUB", "WAVE"];

/// DIRECTION's names, by its value: in full where they fit a cell,
/// FORWARD and PING-PONG don't (full-word labels rule).
pub const DIRECTION_NAMES: [&str; 4] = ["FWD", "REVERSE", "LOOP", "PING"];

/// SLICE's names, by its value.
pub const SLICE_NAMES: [&str; 18] = [
    "OFF", "NOTE", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15",
    "16",
];

/// The source is not a param: it is the Sound's `SourceRef` record. START,
/// END, POSITION and SIZE are fractions; the window is `[START, END]` of the
/// source, END kept ≥ START + 2 ms where it plays (`heads::window_end`).
///
/// TRON is WALK's, not a machine (ADR 0203):
/// ```compile_fail,E0599
/// use chimera_engine::dsp::tape_engine::TapeMachine;
/// let _ = TapeMachine::Tron;
/// ```
#[derive(Clone, Copy, PartialEq)]
pub struct TapeEngineParams {
    /// MODEL.
    pub machine: TapeMachine,
    /// WALK's DIRECTION (`MODE` on disk) and TRON: kept off WALK.
    pub direction: Direction,
    pub tron: Tron,
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
    /// SCRUB's inertia, WALK's and TRON's FADE: `drag_s`.
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
    /// WALK's DRIFT: how far POSITION can step at each note, 0 to the
    /// whole window (`drift::step`).
    pub drift: Drift,
    /// SCRUB's KEY.
    pub key: KeyMode,
    /// STRETCH's LENGTH, GRAIN, SPRAY and END (ids 22–25, ADR 0204), as
    /// the knobs and the codec hold them: one array, so `get` and `write`
    /// reach them in one arm each. The DSP reads them typed (`length`,
    /// `grain`, `spray`, `end_mode`), each held in its range.
    pub stretch: [f32; 4],
}

/// As the disk has it: MODEL's model and MODE (`neutral_v1.txt` pins it).
impl core::fmt::Debug for TapeEngineParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TapeEngineParams")
            .field("model", &self.model())
            .field("mode", &self.direction)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("position", &self.position)
            .field("motion", &self.motion)
            .field("tape", &self.tape)
            .field("age", &self.age)
            .field("size", &self.size)
            .field("xfade", &self.xfade)
            .field("drag", &self.drag)
            .field("kind", &self.kind)
            .field("root", &self.root)
            .field("period", &self.period)
            .field("slice", &self.slice)
            .field("wow", &self.wow)
            .field("retrig", &self.retrig)
            .field("tron", &self.tron)
            .field("drift", &self.drift)
            .field("key", &self.key)
            .field("stretch", &self.stretch)
            .finish()
    }
}

/// Neutral (`Sound::neutral(Tape)`, a format contract): the whole source
/// looped with a folded 10 % crossfade.
impl Default for TapeEngineParams {
    fn default() -> Self {
        Self {
            machine: TapeMachine::Walk,
            direction: Direction::Loop,
            tron: Tron::Off,
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
            drift: Drift::OFF,
            key: KeyMode::Position,
            stretch: STRETCH_DEFAULTS,
        }
    }
}

impl TapeEngineParams {
    pub const fn machine(&self) -> TapeMachine {
        self.machine
    }

    pub fn set_machine(&mut self, m: TapeMachine) {
        self.machine = m;
    }

    pub const fn model(&self) -> TapeModel {
        self.machine.model()
    }

    /// `w`'s machine, DIRECTION and TRON (`Way::parts`).
    pub fn set_way(&mut self, w: Way) {
        (self.machine, self.direction, self.tron) = w.parts();
    }

    /// `self` on `w` (`set_way`).
    pub const fn with_way(self, w: Way) -> Self {
        let (machine, direction, tron) = w.parts();
        Self {
            machine,
            direction,
            tron,
            ..self
        }
    }

    /// The deck's path for this machine, DIRECTION and TRON.
    pub const fn way(&self) -> Way {
        Way::of(self.machine, self.direction, self.tron)
    }

    /// WALK with TRON on: the tape runs out and the pad lifts at key up.
    pub fn tron_on(&self) -> bool {
        self.machine == TapeMachine::Walk && self.tron == Tron::On
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
    // 17 and 18 are SLICE's (ADR 0199, `tape-slice`).
    pub const TRON: ParamId = ParamId(19);
    pub const DRIFT: ParamId = ParamId(20);
    /// SCRUB's KEY (ADR 0205).
    pub const KEY: ParamId = ParamId(21);
    // STRETCH GRAIN's (ADR 0204).
    pub const LENGTH: ParamId = ParamId(22);
    pub const GRAIN: ParamId = ParamId(23);
    pub const SPRAY: ParamId = ParamId(24);
    pub const FINISH: ParamId = ParamId(25);

    /// STRETCH's LENGTH, 0.25 to 8 s.
    #[inline(always)]
    pub fn length(&self) -> Length {
        Length::of_knob(self.stretch[0])
    }

    /// STRETCH's GRAIN, 30 to 250 ms.
    #[inline(always)]
    pub fn grain(&self) -> GrainLen {
        GrainLen::of_knob(self.stretch[1])
    }

    /// STRETCH's SPRAY.
    #[inline(always)]
    pub fn spray(&self) -> Spray {
        Spray::of_knob(self.stretch[2])
    }

    /// STRETCH's END: HOLD or STOP.
    #[inline(always)]
    pub fn end_mode(&self) -> StretchEnd {
        StretchEnd::of_knob(self.stretch[3])
    }
}

/// LENGTH 2 s, GRAIN 80 ms, SPRAY 40 %, END HOLD: each on the knob's grid.
pub const STRETCH_DEFAULTS: [f32; 4] = [77.0 / 128.0, 59.0 / 128.0, 51.0 / 128.0, 0.0];

/// Whether `m` reads `id`: the one table for cells, dimming and the audio
/// test, from the machines' (`machine::MACHINES`). START, END, TAPE, AGE
/// and WOW read on every machine; ROOT and PERIOD have no cell. DIRECTION
/// (MODE) is WALK's cell.
pub fn reads(m: TapeMachine, id: ParamId) -> bool {
    type P = TapeEngineParams;
    match id {
        P::MODEL | P::START | P::END | P::TAPE | P::AGE | P::WOW | P::ROOT | P::PERIOD => true,
        _ => super::machine::cell_of(m, id).is_some(),
    }
}

/// Whether `m` moves `id` by a motion of its own the knobs would only
/// blur: SCRUB's POSITION is its jog, which the UI steps with DRAG's
/// momentum (ADR 0112). The Part's knobs hand it on at once.
pub fn jogs(m: TapeMachine, id: ParamId) -> bool {
    m == TapeMachine::Scrub && id == TapeEngineParams::POSITION
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
pub static TAPE_ENGINE_SPECS: [ParamSpec; 24] = [
    // The neutral is WALK in DIRECTION LOOP.
    ParamSpec::choice(
        0,
        "MODEL",
        ValFmt::Names(&MODEL_NAMES),
        3.0,
        0.0,
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
        ValFmt::Names(&["CLASSIC", "CLS POLY", "GRAIN"]),
        2.0,
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
    // WALK's DIRECTION, MODE on disk (ADR 0203): LOOP by default.
    ParamSpec::choice(
        15,
        "DIRECTION",
        ValFmt::Names(&DIRECTION_NAMES),
        3.0,
        2.0,
        Change::NoteOn,
    )
    .short("DIR")
    .ident("MODE")
    .glyph(TAPE),
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
    ParamSpec::choice(
        19,
        "TRON",
        ValFmt::Names(&["OFF", "ON"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("TRON")
    .glyph(crate::glyph::FocusGlyph::Switch),
    // Read at note-on: each note steps POSITION by up to DRIFT (ADR 0203).
    unit(20, "DRIFT", 0.0, false, Smoothing::NoteOn)
        .short("DFT")
        .ident("DRIFT")
        .glyph(TAPE),
    ParamSpec::choice(
        21,
        "KEY",
        ValFmt::Names(&["POSITION", "PITCH"]),
        1.0,
        0.0,
        Change::NoteOn,
    )
    .ident("KEY")
    .glyph(TAPE),
    // STRETCH's (ADR 0204): read every block, so a turn moves the grains
    // to come; the playing ones finish where they started. Not
    // modulatable: no route reaches them (as XFADE and DRAG).
    ParamSpec::continuous(
        22,
        "LENGTH",
        ValFmt::Law(Law::StretchLength),
        0.0,
        1.0,
        STRETCH_DEFAULTS[0],
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .short("LEN")
    .ident("STRETCH_LENGTH")
    .glyph(TAPE),
    ParamSpec::continuous(
        23,
        "GRAIN",
        ValFmt::Law(Law::GrainSize),
        0.0,
        1.0,
        STRETCH_DEFAULTS[1],
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .short("GRN")
    .ident("GRAIN")
    .glyph(TAPE),
    ParamSpec::continuous(
        24,
        "SPRAY",
        ValFmt::Law(Law::Pct),
        0.0,
        1.0,
        STRETCH_DEFAULTS[2],
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .short("SPR")
    .ident("SPRAY")
    .glyph(TAPE),
    // FINISH: the cell's END, a word apart from the window's END.
    ParamSpec::stepped(
        25,
        "FINISH",
        ValFmt::Names(&["HOLD", "STOP"]),
        0.0,
        1.0,
        STRETCH_DEFAULTS[3],
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("STRETCH_END")
    .glyph(TAPE),
];

impl Block for TapeEngineParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &TAPE_ENGINE_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODEL => self.machine.value(),
            Self::START => self.start,
            Self::END => self.end,
            Self::POSITION => self.position,
            Self::MOTION => self.motion,
            Self::TAPE => self.tape,
            Self::AGE => self.age,
            Self::SIZE => self.size,
            Self::XFADE => self.xfade,
            Self::DRAG => self.drag,
            Self::TYPE => self.kind.value(),
            Self::ROOT => self.root,
            Self::PERIOD => self.period,
            Self::SLICE => self.slice.code() as f32,
            Self::WOW => self.wow.value(),
            Self::MODE => self.direction.value(),
            Self::RETRIG => self.retrig.value(),
            Self::TRON => self.tron.value(),
            Self::DRIFT => self.drift.get(),
            Self::KEY => self.key.value(),
            ParamId(id @ 22..=25) => self.stretch[usize::from(id - 22)],
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODEL => self.set_machine(TapeMachine::from_index(MachineIndex(v as u8))),
            Self::START => self.start = v,
            Self::END => self.end = v,
            Self::POSITION => self.position = v,
            Self::MOTION => self.motion = v,
            Self::TAPE => self.tape = v,
            Self::AGE => self.age = v,
            Self::SIZE => self.size = v,
            Self::XFADE => self.xfade = v,
            Self::DRAG => self.drag = v,
            Self::TYPE => set(&mut self.kind, v),
            Self::ROOT => self.root = v,
            Self::PERIOD => self.period = v,
            Self::SLICE => self.slice = SliceCode::new(v as u8),
            Self::WOW => set(&mut self.wow, v),
            Self::MODE => set(&mut self.direction, v),
            Self::RETRIG => set(&mut self.retrig, v),
            Self::TRON => set(&mut self.tron, v),
            Self::DRIFT => self.drift = Drift::of(v),
            Self::KEY => set(&mut self.key, v),
            ParamId(id @ 22..=25) => self.stretch[usize::from(id - 22)] = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::MODEL => Some(self.model().disk_code()),
            Self::TYPE => Some(self.kind.disk_code()),
            Self::SLICE => Some(self.slice.code()),
            Self::WOW => Some(self.wow.disk_code()),
            Self::MODE => Some(self.direction.disk_code()),
            Self::RETRIG => Some(self.retrig.disk_code()),
            Self::TRON => Some(self.tron.disk_code()),
            Self::KEY => Some(self.key.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::MODEL => Some(self.model().disk_ident()),
            Self::TYPE => Some(self.kind.disk_ident()),
            Self::SLICE => Some(SLICE_IDENTS[usize::from(self.slice.code())]),
            Self::WOW => Some(self.wow.disk_ident()),
            Self::MODE => Some(self.direction.disk_ident()),
            Self::RETRIG => Some(self.retrig.disk_ident()),
            Self::TRON => Some(self.tron.disk_ident()),
            Self::KEY => Some(self.key.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::MODEL => apply_code(TapeModel::from_disk_code(code), |m| {
                self.set_machine(TapeMachine::of(m))
            }),
            Self::TYPE => apply_code(StretchType::from_disk_code(code), |k| self.kind = k),
            Self::SLICE => apply_code(identity_code(&TAPE_ENGINE_SPECS, id, code), |c| {
                self.slice = SliceCode::new(c)
            }),
            Self::WOW => apply_code(Wow::from_disk_code(code), |w| self.wow = w),
            Self::MODE => apply_code(Direction::from_disk_code(code), |d| self.direction = d),
            Self::RETRIG => apply_code(Retrig::from_disk_code(code), |r| self.retrig = r),
            Self::TRON => apply_code(Tron::from_disk_code(code), |t| self.tron = t),
            Self::KEY => apply_code(KeyMode::from_disk_code(code), |k| self.key = k),
            _ => false,
        }
    }
}

/// `v` into `x` when it names a variant.
fn set<T: Choice>(x: &mut T, v: f32) {
    if let Some(c) = T::from_value(v) {
        *x = c;
    }
}

/// SLICE's frozen idents, by code.
const SLICE_IDENTS: [&str; 18] = [
    "OFF", "NOTE", "S1", "S2", "S3", "S4", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S12",
    "S13", "S14", "S15", "S16",
];

/// A file from before WALK (ADR 0170, ADR 0203): MODEL's retired codes
/// and MODE ONE load as WALK. REVERSE (1) is LOOP with START and END
/// swapped, which plays it bit for bit; ONESHOT (7) is FORWARD with FADE at
/// HOLD and no SLICE; PING-PONG (2) is PING; TRON (6) is FORWARD with TRON
/// on from the window's start. MODE ONE is FORWARD from the window's
/// start, whole, as ONE read neither POSITION nor LENGTH. Run after the
/// file's writes, so a sparse Sound's base START 0, END 1 becomes START
/// 1, END 0.
pub fn translate_v1(old: &crate::storage::Retired, b: &mut dyn Block) {
    type P = TapeEngineParams;
    if let Some(m) = old.code(P::MODEL).and_then(RetiredModel::from_code) {
        m.translate(b);
    } else if old.code(P::MODE) == Some(RetiredMode::One.code()) {
        // Off WALK the code was only kept: the machine's POSITION and
        // LENGTH are its own.
        if b.enum_code(P::MODEL) == Some(TapeModel::Walk.disk_code()) {
            one_as_forward(b);
        } else {
            let _ = b.set_enum_code(P::MODE, Direction::Forward.disk_code());
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
