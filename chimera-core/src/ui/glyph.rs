//! Focus glyphs: the gauge at the right of the focus band, hand-assigned
//! per parameter on its spec (`ParamSpec::glyph`), ARC by default.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::chorus::{ChorusMode, ChorusParams};
use crate::dsp::delay::DelayParams;
use crate::dsp::reverb::ReverbParams;

/// The gauge a parameter's focus band shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusGlyph {
    /// 270° arc, from 12:00 when bipolar.
    #[default]
    Arc,
    /// No gauge: the value text gets the whole band (word choices).
    None,
    /// Two-state on/off.
    Switch,
    /// Vertical slider with tick dots.
    LevelBar,
    /// Horizontal crossfader.
    Crossfader,
    /// One animated glyph for all of an effect's params (`CompositeId::
    /// params`: the braid's 4, the rings' 7, the cube's 5), drawn from
    /// their set values, never the modulated ones.
    Composite(CompositeId),
}

/// The composites, each shared by its effect's params.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeId {
    /// SIZE, TIME, AMOUNT.
    ReverbCube,
    /// TIME, FEEDBACK, TONE.
    DelayRings,
    /// RATE, DEPTH, MIX; MODE is the strand count.
    ChorusBraid,
}

impl CompositeId {
    /// The params a composite reads, in its inputs' order: every spec that
    /// carries it, and only those (`tests/focus_glyph_test.rs` pins both).
    pub const fn params(self) -> &'static [ParamAddr] {
        match self {
            CompositeId::ChorusBraid => &BRAID_PARAMS,
            CompositeId::DelayRings => &RINGS_PARAMS,
            CompositeId::ReverbCube => &CUBE_PARAMS,
        }
    }
}

/// What the focus band draws, with its inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gauge {
    /// `value` 0..1, from 12:00 when `bipolar`.
    Arc { value: f32, bipolar: bool },
    /// No gauge: the value text has the whole band.
    None,
    /// A two-state toggle: `on` 0 off, 1 on, eased between.
    Switch { on: f32 },
    /// A vertical slider, `value` 0..1 rising from the bottom, beside
    /// `ticks` dots: one per step for a few steps, else 8.
    LevelBar { value: f32, ticks: u8 },
    /// A horizontal crossfader, `value` 0 left .. 1 right, from the set
    /// value, never the modulated one.
    Crossfader { value: f32 },
    /// The chorus braid (`CompositeId::ChorusBraid`).
    Braid(Braid),
    /// The delay's rings (`CompositeId::DelayRings`).
    Rings(Rings),
    /// The reverb's cube (`CompositeId::ReverbCube`).
    Cube(Cube),
}

/// The reverb params the cube reads, in `Cube::from_set`'s order.
pub const CUBE_PARAMS: [ParamAddr; 5] = [
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SIZE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::TIME),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DAMPING),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::GRIT),
];

/// The cube's param in focus, emphasised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubePart {
    Size,
    Time,
    Damp,
    Mix,
    Grit,
}

impl CubePart {
    /// In `CUBE_PARAMS`' order.
    pub const ALL: [CubePart; 5] = [
        CubePart::Size,
        CubePart::Time,
        CubePart::Damp,
        CubePart::Mix,
        CubePart::Grit,
    ];
}

/// The reverb as a room: a wireframe cube in perspective, slowly turning,
/// from the reverb's set values (normalized) and the UI clock's `frame`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cube {
    /// How big the room is.
    pub size: f32,
    /// How long it rings: afterimages trailing the turn.
    pub time: f32,
    /// How fast the highs die: the far edges dim, then dot.
    pub damp: f32,
    /// Edge weight.
    pub mix: f32,
    /// The lo-fi dirt: edges crackle.
    pub grit: f32,
    pub focus: Option<CubePart>,
    pub frame: u32,
}

impl Cube {
    /// From `CUBE_PARAMS`' set values, normalized.
    pub fn from_set(set: [f32; 5], focus: Option<CubePart>, frame: u32) -> Self {
        let n = |i: usize| set[i].clamp(0.0, 1.0);
        Self {
            size: n(0),
            time: n(1),
            damp: n(2),
            mix: n(3),
            grit: n(4),
            focus,
            frame,
        }
    }

    /// Half the cube's side, px: 5 (SIZE 0) to 11.
    pub fn half(&self) -> f32 {
        5.0 + 6.0 * self.size
    }

    /// Afterimages trailing the turn: none (TIME 0) to 3.
    pub fn trails(&self) -> usize {
        libm::roundf(self.time * 3.0) as usize
    }

    /// How far behind each afterimage lags, radians.
    pub fn lag(&self) -> f32 {
        0.12 + 0.18 * self.time
    }

    /// The turn at `frame`, radians in 0..τ: one slow constant spin, 16 s a
    /// turn at 20 fps. In f64, so it stays smooth however long it runs.
    pub fn turn(&self) -> f32 {
        use core::f64::consts::TAU;
        libm::fmod(self.frame as f64 * TAU / 320.0, TAU) as f32
    }
}

/// The delay params the rings read, in `Rings::from_set`'s order.
pub const RINGS_PARAMS: [ParamAddr; 7] = [
    ParamAddr::new(BlockRef::Delay, DelayParams::TIME_MS),
    ParamAddr::new(BlockRef::Delay, DelayParams::FEEDBACK),
    ParamAddr::new(BlockRef::Delay, DelayParams::TONE),
    ParamAddr::new(BlockRef::Delay, DelayParams::MIX),
    ParamAddr::new(BlockRef::Delay, DelayParams::WOW_FLUTTER),
    ParamAddr::new(BlockRef::Delay, DelayParams::SATURATION),
    ParamAddr::new(BlockRef::Delay, DelayParams::REV_SEND),
];

/// The rings' param in focus, emphasised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingsPart {
    Time,
    Fdbk,
    Tone,
    Mix,
    Mech,
    Sat,
    Rev,
}

impl RingsPart {
    /// In `RINGS_PARAMS`' order.
    pub const ALL: [RingsPart; 7] = [
        RingsPart::Time,
        RingsPart::Fdbk,
        RingsPart::Tone,
        RingsPart::Mix,
        RingsPart::Mech,
        RingsPart::Sat,
        RingsPart::Rev,
    ];
}

/// The delay's echoes as rings spreading from a source dot, one per
/// repeat, from the delay's set values (normalized) and the UI clock's
/// `frame`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rings {
    /// How often a ring leaves the source, so how far apart they travel.
    pub time: f32,
    /// How many repeats survive.
    pub fdbk: f32,
    /// How many stay crisp before they blur.
    pub tone: f32,
    /// Ring weight against the source dot.
    pub mix: f32,
    /// MECHANICS: how much the rings wobble.
    pub mech: f32,
    /// How thick the newest ring is.
    pub sat: f32,
    /// The send on to the reverb: dots where the rings leave.
    pub rev: f32,
    pub focus: Option<RingsPart>,
    pub frame: u32,
}

impl Rings {
    /// How far a ring spreads each UI frame, px.
    pub const SPEED: f32 = 0.8;

    /// From `RINGS_PARAMS`' set values, normalized.
    pub fn from_set(set: [f32; 7], focus: Option<RingsPart>, frame: u32) -> Self {
        let n = |i: usize| set[i].clamp(0.0, 1.0);
        Self {
            time: n(0),
            fdbk: n(1),
            tone: n(2),
            mix: n(3),
            mech: n(4),
            sat: n(5),
            rev: n(6),
            focus,
            frame,
        }
    }

    /// UI frames between rings: 4 (TIME 0) to 16 (TIME 1), so 3 to 13 px
    /// apart: seven rings in the field to two.
    pub fn period(&self) -> f32 {
        4.0 + 12.0 * self.time
    }

    /// The distance between neighbouring rings, px.
    pub fn spacing(&self) -> f32 {
        self.period() * Self::SPEED
    }

    /// Repeats louder than an eighth of the first (FDBK^k ≥ 1/8), at most
    /// 12; with no feedback, the first alone.
    pub fn survivors(&self) -> usize {
        if self.fdbk < 0.01 {
            return 1;
        }
        let k = libm::logf(0.125) / libm::logf(self.fdbk.min(0.999));
        (1 + k as usize).min(12)
    }

    /// Rings drawn solid before they blur (dotted): 1 (TONE 0) to 5.
    pub fn crisp(&self) -> usize {
        1 + libm::roundf(self.tone * 4.0) as usize
    }

    /// The wobble's phase at `frame`, radians in 0..τ, 0.37 a frame. In
    /// f64, so it stays smooth however long the clock has run.
    pub fn wobble_phase(&self) -> f32 {
        libm::fmod(self.frame as f64 * 0.37, core::f64::consts::TAU) as f32
    }

    /// How long the newest ring has been out, UI frames (0..period). In
    /// f64, so it stays smooth however long the clock has run.
    pub fn age(&self) -> f32 {
        libm::fmod(self.frame as f64, self.period() as f64) as f32
    }
}

/// The chorus params the braid reads, in `Braid::from_set`'s order.
pub const BRAID_PARAMS: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Chorus, ChorusParams::MODE),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::RATE),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::DEPTH),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::MIX),
];

/// The braid's param in focus, emphasised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BraidPart {
    Mode,
    Rate,
    Depth,
    Mix,
}

impl BraidPart {
    /// In `BRAID_PARAMS`' order.
    pub const ALL: [BraidPart; 4] = [
        BraidPart::Mode,
        BraidPart::Rate,
        BraidPart::Depth,
        BraidPart::Mix,
    ];
}

/// The chorus's voices as strands twisting round a dry centre line, from
/// the chorus's set values (normalized) and the UI clock's `frame`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Braid {
    /// MODE: OFF, I, II or I+II.
    pub mode: ChorusMode,
    /// How fast the twist travels.
    pub rate: f32,
    /// How far the strands swing apart.
    pub depth: f32,
    /// How bright the strands are against the dry line.
    pub mix: f32,
    pub focus: Option<BraidPart>,
    pub frame: u32,
}

impl Braid {
    /// From `BRAID_PARAMS`' set values, normalized.
    pub fn from_set(set: [f32; 4], focus: Option<BraidPart>, frame: u32) -> Self {
        Self {
            mode: ChorusMode::from_u8(libm::roundf(set[0].clamp(0.0, 1.0) * 3.0) as u8),
            rate: set[1].clamp(0.0, 1.0),
            depth: set[2].clamp(0.0, 1.0),
            mix: set[3].clamp(0.0, 1.0),
            focus,
            frame,
        }
    }

    /// OFF none (the dry line alone), I and II two, I+II three.
    pub const fn strands(&self) -> usize {
        match self.mode {
            ChorusMode::Off => 0,
            ChorusMode::JunoI | ChorusMode::JunoII => 2,
            ChorusMode::JunoBoth => 3,
        }
    }

    /// Twists along the box, and how fast the twist travels: II's are
    /// tighter and faster.
    const fn turns_and_speed(&self) -> (f32, f64) {
        match self.mode {
            ChorusMode::JunoII => (2.5, 1.6),
            ChorusMode::Off | ChorusMode::JunoI | ChorusMode::JunoBoth => (1.5, 1.0),
        }
    }

    /// Twists along the box.
    pub const fn turns(&self) -> f32 {
        self.turns_and_speed().0
    }

    /// The twist's phase at `frame`, radians in 0..τ: RATE sets how fast
    /// it travels (0.16 to 1.6 Hz at 20 fps); II travels faster. In f64,
    /// so it stays smooth however long the clock has run.
    pub fn twist(&self) -> f32 {
        let per_frame = 0.05 + 0.45 * self.rate as f64;
        let speed = self.turns_and_speed().1;
        libm::fmod(
            self.frame as f64 * per_frame * speed,
            core::f64::consts::TAU,
        ) as f32
    }
}

impl FocusGlyph {
    pub const ALL: [FocusGlyph; 8] = [
        FocusGlyph::Arc,
        FocusGlyph::None,
        FocusGlyph::Switch,
        FocusGlyph::LevelBar,
        FocusGlyph::Crossfader,
        FocusGlyph::Composite(CompositeId::ReverbCube),
        FocusGlyph::Composite(CompositeId::DelayRings),
        FocusGlyph::Composite(CompositeId::ChorusBraid),
    ];

    /// Index in `ALL`. Exhaustive: a new variant fails to compile here
    /// until it is listed.
    const fn position(self) -> usize {
        match self {
            FocusGlyph::Arc => 0,
            FocusGlyph::None => 1,
            FocusGlyph::Switch => 2,
            FocusGlyph::LevelBar => 3,
            FocusGlyph::Crossfader => 4,
            FocusGlyph::Composite(CompositeId::ReverbCube) => 5,
            FocusGlyph::Composite(CompositeId::DelayRings) => 6,
            FocusGlyph::Composite(CompositeId::ChorusBraid) => 7,
        }
    }

    /// The gauge the focus band draws for this glyph at the focused slot's
    /// `value` in format `fmt`. A composite's comes from `composite`,
    /// which reads its params' set values (`Renderer::eased_set`).
    pub fn gauge(
        self,
        value: f32,
        fmt: ValFmt,
        composite: impl FnOnce(CompositeId) -> Gauge,
    ) -> Gauge {
        let bipolar = fmt.is_bipolar();
        match self {
            FocusGlyph::None => Gauge::None,
            FocusGlyph::Switch => Gauge::Switch { on: value },
            FocusGlyph::LevelBar => Gauge::LevelBar {
                value,
                ticks: level_ticks(fmt),
            },
            FocusGlyph::Crossfader => Gauge::Crossfader { value },
            FocusGlyph::Composite(CompositeId::ChorusBraid) => composite(CompositeId::ChorusBraid),
            FocusGlyph::Composite(CompositeId::DelayRings) => composite(CompositeId::DelayRings),
            FocusGlyph::Composite(CompositeId::ReverbCube) => composite(CompositeId::ReverbCube),
            FocusGlyph::Arc => Gauge::Arc { value, bipolar },
        }
    }
}

const _: () = {
    let mut i = 0;
    while i < FocusGlyph::ALL.len() {
        assert!(FocusGlyph::ALL[i].position() == i);
        i += 1;
    }
};

impl Gauge {
    /// Moves on its own: its band redraws every frame while shown.
    pub const fn animates(&self) -> bool {
        match self {
            Gauge::Arc { .. }
            | Gauge::None
            | Gauge::Switch { .. }
            | Gauge::LevelBar { .. }
            | Gauge::Crossfader { .. } => false,
            Gauge::Braid(_) | Gauge::Rings(_) | Gauge::Cube(_) => true,
        }
    }
}

/// A level bar's ticks for `fmt`: one per step when it has 2 to 9, else 8.
fn level_ticks(fmt: ValFmt) -> u8 {
    let steps = fmt.max_int().saturating_add(1);
    if fmt.is_discrete() && (2..=9).contains(&steps) {
        steps
    } else {
        8
    }
}

/// The focus band's dirty-key share of the UI clock: the frame while the
/// glyph animates, else constant.
pub const fn anim_key(animates: bool, frame: u32) -> u32 {
    if animates { frame } else { 0 }
}
