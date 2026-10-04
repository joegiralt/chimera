//! Focus glyphs: the gauge at the right of the focus band, hand-assigned
//! per parameter on its spec (`ParamSpec::glyph`), ARC by default.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::algo::waves::{WAVE_LEN, WaveId};
use crate::dsp::chorus::{ChorusMode, ChorusParams};
use crate::dsp::clampf;
use crate::dsp::delay::DelayParams;
use crate::dsp::reverb::{EngineKnobs, RevSync, ReverbParams, ReverbType, knobs};

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
    /// A note on a three-line staff, by semitones (`StaffNote`).
    Staff,
    /// A small scope tracing one period of the chosen wave's table.
    Wave,
    /// One animated glyph for all of an effect's params (`CompositeId::
    /// params`: the braid's 4, the rings' 7, the cube's 5), drawn from
    /// their set values, never the modulated ones.
    Composite(CompositeId),
}

/// The composites, each shared by its effect's params.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeId {
    /// The reverb's knobs: the cube under QUAD (ADR 0067, 0085).
    Reverb,
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
            CompositeId::Reverb => &REVERB_PARAMS,
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
    /// A notehead `step` diatonic steps above the staff's middle line
    /// (fractional while it eases), with its `accidental`.
    Staff { step: f32, accidental: Accidental },
    /// The scope, tracing `wave_trace` of this wave.
    Wave(WaveId),
    /// The chorus braid (`CompositeId::ChorusBraid`).
    Braid(Braid),
    /// The delay's rings (`CompositeId::DelayRings`).
    Rings(Rings),
    /// The reverb's cube (`CompositeId::Reverb`), under QUAD.
    Cube(Cube),
    /// The reverb's TAIL (`CompositeId::Reverb`), under the other TYPEs.
    Tail(Tail),
}

/// A staff note's accidental.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accidental {
    Natural,
    Sharp,
    Flat,
}

/// A transposition on the staff: diatonic steps from the middle line
/// (C), spelled in C major, sharps going up and flats going down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaffNote {
    pub step: i8,
    pub accidental: Accidental,
}

impl StaffNote {
    /// `semis` on the staff: ±12 is an octave (±7 steps); past it the note
    /// wraps within the octave and the readout carries the rest.
    pub const fn of(semis: i8) -> Self {
        // Steps above C for each pitch class, sharp-spelled and flat-spelled.
        const UP: [i8; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
        const DOWN: [i8; 12] = [0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6];
        const BLACK: u16 = 0b0101_0100_1010;
        let mag = semis.unsigned_abs();
        let r = (if mag > 12 { (mag - 1) % 12 + 1 } else { mag }) as usize;
        let (step, pc, inbetween) = if semis >= 0 {
            let pc = r % 12;
            (UP[pc] + 7 * (r / 12) as i8, pc, Accidental::Sharp)
        } else {
            let pc = 12 - r;
            (DOWN[pc] - 7, pc, Accidental::Flat)
        };
        let accidental = if BLACK >> pc & 1 == 1 {
            inbetween
        } else {
            Accidental::Natural
        };
        Self { step, accidental }
    }
}

/// The staff gauge at `semis`, eased: the notehead slides between the
/// whole semitones either side, but snaps where they are more than a
/// step apart (the octave wrap); the accidental is the nearest one's.
fn staff(semis: f32) -> Gauge {
    let semis = clampf(semis, -127.0, 126.0);
    let lo = libm::floorf(semis);
    let (a, b) = (StaffNote::of(lo as i8), StaffNote::of(lo as i8 + 1));
    let near = StaffNote::of(libm::roundf(semis) as i8);
    let step = if (b.step - a.step).abs() > 1 {
        near.step as f32
    } else {
        a.step as f32 + (b.step - a.step) as f32 * (semis - lo)
    };
    Gauge::Staff {
        step,
        accidental: near.accidental,
    }
}

/// One period of `wave`'s mip-0 table across `N` points, the last the
/// table's guard (its first sample again): each the nearest sample, scaled
/// so full scale is `half` px (rounded, by 2^15), up positive.
pub fn wave_trace<const N: usize>(wave: WaveId, half: i32) -> [i32; N] {
    let table = wave.table(0);
    core::array::from_fn(|i| {
        let s = table[i * WAVE_LEN / (N - 1).max(1)] as i32;
        (s * half + (1 << 14)) >> 15
    })
}

/// The reverb params the cube reads, in `Cube::from_set`'s order.
pub const CUBE_PARAMS: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SIZE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::TIME),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DAMPING),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SMEAR),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::RATE),
];

/// Every reverb param that shows the composite: the cube's six, then the
/// slots only the other TYPEs read (their glyph lands with them).
pub const REVERB_PARAMS: [ParamAddr; 10] = [
    CUBE_PARAMS[0],
    CUBE_PARAMS[1],
    CUBE_PARAMS[2],
    CUBE_PARAMS[3],
    CUBE_PARAMS[4],
    CUBE_PARAMS[5],
    ParamAddr::new(BlockRef::Reverb, ReverbParams::TONE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DIFFUSE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::PREDELAY),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SHAPE),
];

/// The cube's param in focus, emphasised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubePart {
    Size,
    Time,
    Damp,
    Mix,
    Smear,
    Rate,
}

impl CubePart {
    /// In `CUBE_PARAMS`' order.
    pub const ALL: [CubePart; 6] = [
        CubePart::Size,
        CubePart::Time,
        CubePart::Damp,
        CubePart::Mix,
        CubePart::Smear,
        CubePart::Rate,
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
    /// The random wander: the far edges jitter, slowly.
    pub smear: f32,
    /// How often the jitter moves.
    pub rate: f32,
    pub focus: Option<CubePart>,
    pub frame: u32,
}

impl Cube {
    /// From `CUBE_PARAMS`' set values, normalized.
    pub fn from_set(set: [f32; 6], focus: Option<CubePart>, frame: u32) -> Self {
        let n = |i: usize| clampf(set[i], 0.0, 1.0);
        Self {
            size: n(0),
            time: n(1),
            damp: n(2),
            mix: n(3),
            smear: n(4),
            rate: n(5),
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
        ((self.frame % 320) as f64 * TAU / 320.0) as f32
    }
}

/// TAIL's columns.
pub const TAIL_COLS: usize = 60;

/// TAIL's x axis, ms: the TYPE's own range, so a turn always moves it.
pub fn tail_span_ms(t: ReverbType) -> f32 {
    match t {
        ReverbType::Quad | ReverbType::Reverse => 2_000.0,
        ReverbType::Room => 1_500.0,
        ReverbType::Hall => 10_000.0,
        ReverbType::Bloom => 12_000.0,
        ReverbType::Gate => 1_400.0,
    }
}

/// TAIL's range: 48 dB under the peak is the floor.
const TAIL_DB: f32 = 48.0;

/// `x`'s triangle, 0 at whole numbers, 1 halfway: `x` ≥ 0.
fn tri(x: f32) -> f32 {
    1.0 - libm::fabsf(2.0 * (x - x as u32 as f32) - 1.0)
}

/// The TYPE's energy envelope, from set values only (spec § Glyph): each
/// of `TAIL_COLS` columns over `tail_span_ms`, its level in 1/255ths of
/// 48 dB, 255 the peak.
/// - GATE: a plateau sloped by SHAPE, a cliff at LENGTH, then the TAIL.
/// - REVERSE: a ramp rising by CURVE that cuts dead at LENGTH, its echoes
///   under REGEN.
/// - The others: an exponential fall at their RT60.
///
/// TONE dims it (6 dB at its darkest), DENSITY notches it between taps,
/// and SMEAR ripples it at RATE.
#[inline(never)]
pub fn tail_shape(t: ReverbType, k: &EngineKnobs) -> [u8; TAIL_COLS] {
    let span = tail_span_ms(t);
    let len = k.length_ms.max(1.0);
    let fall = -60.0e-3 / k.rt60_s.max(0.01);
    let curve = libm::fabsf(k.shape_db);
    let notch = (12.0 - k.taps).max(0.0) * 0.75;
    let db = |x: f32| 6.02 * crate::dsp::rev::parts::lg2(x);
    let echo = if k.regen > 0.0 { db(k.regen) } else { -TAIL_DB };
    let tail = if k.tail > 0.0 {
        db(k.tail) - curve - 12.0
    } else {
        -2.0 * TAIL_DB
    };
    let dim = 6.0 * clampf((10_000.0 - k.tone_hz) / 7_500.0, 0.0, 1.0);
    let mut out = [0u8; TAIL_COLS];
    for (c, o) in out.iter_mut().enumerate() {
        let ms = (c as f32 + 0.5) * span / TAIL_COLS as f32;
        let x = ms / len;
        let pass = x as u32;
        let at = x - pass as f32;
        let tap = notch * tri(11.0 * at);
        let level = match t {
            ReverbType::Gate if pass == 0 => -curve * at - tap,
            ReverbType::Gate => tail + fall * (ms - len),
            ReverbType::Reverse => pass as f32 * echo - curve * (1.0 - at) - tap,
            // ROOM: a pre-delay gap, then the fall.
            ReverbType::Room if ms < k.predelay_ms => -2.0 * TAIL_DB,
            ReverbType::Room => fall * (ms - k.predelay_ms),
            ReverbType::Quad | ReverbType::Hall | ReverbType::Bloom => fall * ms,
        };
        let ripple = 3.0 * k.smear_ms * (2.0 * tri(4e-3 * k.rate_hz * ms) - 1.0);
        *o = (clampf((level - dim + ripple) / TAIL_DB + 1.0, 0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    out
}

/// The reverb's TAIL: `tail_shape` as a polyline, the focused knob's part
/// in the accent, and a playhead crossing it at the TYPE's real time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tail {
    pub shape: [u8; TAIL_COLS],
    /// The columns drawn in the accent, `from..to`.
    pub accent: (u8, u8),
    /// The playhead's column, if it is on the shape.
    pub head: Option<u8>,
}

impl Tail {
    /// From `REVERB_PARAMS`' set values (normalized) under TYPE `t`, the
    /// focused one's index in them, and the UI clock's `frame` (20 fps).
    #[inline(never)]
    pub fn from_set(t: ReverbType, set: [f32; 10], focus: Option<usize>, frame: u32) -> Self {
        let p = ReverbParams {
            kind: t,
            size: set[0],
            time: set[1],
            damping: set[2],
            mix: set[3],
            smear: set[4],
            rate: set[5],
            tone: set[6],
            diffuse: set[7],
            predelay: set[8],
            shape: set[9],
            sync: RevSync::Off,
        };
        let k = knobs(&p);
        let span = tail_span_ms(t);
        let n = TAIL_COLS as f32;
        let edge = clampf(k.length_ms / span * n, 0.0, n) as u8;
        let all = TAIL_COLS as u8;
        let accent = match focus {
            // LENGTH, DENSITY, SHAPE: the burst.
            Some(0 | 7 | 9) => (0, edge),
            // TAIL, REGEN: what follows it.
            Some(1) => (edge, all),
            Some(_) => (0, all),
            None => (0, 0),
        };
        // A pass over the span, then 0.4 s at rest, at 20 fps.
        let at = (frame % ((span as u32 + 400) / 50)) as f32 * 50.0;
        let head = (at < span).then(|| (at / span * n) as u8);
        Self {
            shape: tail_shape(t, &k),
            accent,
            head,
        }
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
        let n = |i: usize| clampf(set[i], 0.0, 1.0);
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
        wrap(self.frame as f64 * 0.37, core::f64::consts::TAU)
    }

    /// How long the newest ring has been out, UI frames (0..period). In
    /// f64, so it stays smooth however long the clock has run.
    pub fn age(&self) -> f32 {
        wrap(self.frame as f64, self.period() as f64)
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
            mode: ChorusMode::from_u8(libm::roundf(clampf(set[0], 0.0, 1.0) * 3.0) as u8),
            rate: clampf(set[1], 0.0, 1.0),
            depth: clampf(set[2], 0.0, 1.0),
            mix: clampf(set[3], 0.0, 1.0),
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
        wrap(
            self.frame as f64 * per_frame * speed,
            core::f64::consts::TAU,
        )
    }
}

impl FocusGlyph {
    pub const ALL: [FocusGlyph; 10] = [
        FocusGlyph::Arc,
        FocusGlyph::None,
        FocusGlyph::Switch,
        FocusGlyph::LevelBar,
        FocusGlyph::Crossfader,
        FocusGlyph::Staff,
        FocusGlyph::Wave,
        FocusGlyph::Composite(CompositeId::Reverb),
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
            FocusGlyph::Staff => 5,
            FocusGlyph::Wave => 6,
            FocusGlyph::Composite(CompositeId::Reverb) => 7,
            FocusGlyph::Composite(CompositeId::DelayRings) => 8,
            FocusGlyph::Composite(CompositeId::ChorusBraid) => 9,
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
            FocusGlyph::Staff => {
                let n = fmt.max_int() as f32 / 2.0;
                staff(value * 2.0 * n - n)
            }
            FocusGlyph::Wave => Gauge::Wave(WaveId::clamped(libm::roundf(
                clampf(value, 0.0, 1.0) * fmt.max_int() as f32,
            ) as u8)),
            FocusGlyph::Composite(CompositeId::ChorusBraid) => composite(CompositeId::ChorusBraid),
            FocusGlyph::Composite(CompositeId::DelayRings) => composite(CompositeId::DelayRings),
            FocusGlyph::Composite(CompositeId::Reverb) => composite(CompositeId::Reverb),
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
            | Gauge::Crossfader { .. }
            | Gauge::Staff { .. }
            | Gauge::Wave(_) => false,
            Gauge::Braid(_) | Gauge::Rings(_) | Gauge::Cube(_) | Gauge::Tail(_) => true,
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

/// `x` (0 or more, a frame count's worth) into 0..period: whole periods
/// off by a truncating cast, not f64 `fmod`, which costs 3.6 KB of flash.
fn wrap(x: f64, period: f64) -> f32 {
    (x - period * (x / period) as u32 as f64).max(0.0) as f32
}
