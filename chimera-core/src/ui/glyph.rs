//! Focus glyphs: the gauge at the right of the focus band, hand-assigned
//! per parameter on its spec (`ParamSpec::glyph`), ARC by default.

use crate::block::ValFmt;
use crate::dsp::algo::formant::Skirt;
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::waves::{WAVE_LEN, WaveId};
use crate::dsp::chorus::ChorusMode;
use crate::dsp::clampf;
use crate::dsp::rev::sync::{FoldState, Tempo, index_of};
use crate::dsp::reverb::{EngineKnobs, RevSync, ReverbParams, ReverbType, knobs};
use crate::dsp::va::combine::{Crusher, Peaks, am_gain, combine, ring_gain};
use crate::dsp::va::lbfm::{lbfm, target_b0};
use crate::dsp::va::osc::{MIN_INC, Morph, Unit, peak_bound, wave_at};
use crate::dsp::va::{VaMode, fm_index, phase_depth};
use crate::ui::theme;

pub use chimera_engine::glyph::{
    BRAID_PARAMS, CUBE_PARAMS, CompositeId, FocusGlyph, REVERB_PARAMS, RINGS_PARAMS,
    TAPE_GLYPH_PARAMS, VA_MIX_PARAMS, VaGlyph, VaOsc,
};

use crate::ui::tape_glyph::TapeGauge;

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
    /// The scope, tracing `kind_trace` of this operator kind.
    Wave(OpKind),
    /// The chorus braid (`CompositeId::ChorusBraid`).
    Braid(Braid),
    /// The delay's rings (`CompositeId::DelayRings`).
    Rings(Rings),
    /// The reverb's cube (`CompositeId::Reverb`), under QUAD.
    Cube(Cube),
    /// The reverb's TAIL (`CompositeId::Reverb`), under the other TYPEs.
    Tail(Tail),
    /// A VA oscillator's scope: `va_wave_trace` of its WAVE and SHAPE.
    VaWave(Morph),
    /// OSC 1 MODE OSC 2: `va_mix_trace`.
    VaMix(VaMix),
    /// A crossfader at the set AMOUNT, its ends named by MODE
    /// (`blend_ends`).
    Blend { value: f32, mode: VaMode },
    /// TAPE's machine glyph (`tape_glyph`): still on MODEL.
    Tape(TapeGauge),
    /// The LFO page's glyph (`lfo_glyph`), kept dimmed too.
    Lfo(crate::ui::lfo_glyph::LfoGauge),
    /// STABILITY's spirit level: the bubble `offset` px right of centre
    /// and the wander's reach `reach` px either side, both whole pixels
    /// of the set value, so it redraws only when one moves.
    Stability { offset: i8, reach: i8 },
    /// The filter's FEEDBACK (ADR 0190): `q` in -`FEEDBACK_STEPS`..=`FEEDBACK_STEPS`,
    /// the set value in eighths, so it redraws only when a step moves.
    Feedback { q: i8 },
}

/// What a gauge built from more than the focused value reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reads {
    Composite(CompositeId),
    Va(VaGlyph),
    /// `TAPE_GLYPH_PARAMS`.
    Tape,
    /// The focused LFO slot's params.
    Lfo,
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

/// FRMT's grain: SKIRT 1 at 4 cycles. The cells that move it are on the
/// operator page; WAVE only names the kind.
const GRAIN_SKIRT: Skirt = Skirt::S1;
const GRAIN_CYCLES: f32 = 4.0;

/// `kind`'s scope trace across `N` points, up positive: a table's wave, a
/// FRMT's grain, a NOIS's fixed band of noise (a seeded walk, smoothed).
pub fn kind_trace<const N: usize>(kind: OpKind, half: i32) -> [i32; N] {
    match kind {
        OpKind::Table(w) => wave_trace(w, half),
        OpKind::Formant => resample(
            &crate::ui::op_page::grain_curve(GRAIN_SKIRT, GRAIN_CYCLES),
            half,
        ),
        OpKind::Noise => resample(&BAND, half),
    }
}

/// NOIS's band: a seeded walk smoothed by a three-tap average, scaled so
/// its peak is ±127. A constant, so the gauge runs no generator.
const BAND: [i8; 64] = {
    let (mut raw, mut r, mut i) = ([0i32; 64], 0x2545_f491u32, 0);
    while i < 64 {
        r = r.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        raw[i] = (r >> 24) as i32 - 128;
        i += 1;
    }
    let (mut sm, mut peak) = ([0i32; 64], 1);
    i = 0;
    while i < 64 {
        let (a, c) = (
            if i == 0 { 0 } else { i - 1 },
            if i == 63 { 63 } else { i + 1 },
        );
        sm[i] = raw[a] + raw[i] + raw[c];
        peak = if sm[i].abs() > peak {
            sm[i].abs()
        } else {
            peak
        };
        i += 1;
    }
    let mut out = [0i8; 64];
    i = 0;
    while i < 64 {
        out[i] = (sm[i] * 127 / peak) as i8;
        i += 1;
    }
    out
};

/// 64 points to ±127, across `N` points to ±`half`.
pub fn resample<const N: usize>(c: &[i8; 64], half: i32) -> [i32; N] {
    core::array::from_fn(|i| (i32::from(c[i * 63 / (N - 1).max(1)]) * half + 63) / 127)
}

/// `ys` scaled so the largest is `half` px (rounded), up positive; a
/// trace quieter than `floor` is scaled as if `floor`.
fn scaled<const N: usize>(ys: [f32; N], floor: f32, half: i32) -> [i32; N] {
    let peak = ys.iter().fold(floor, |p, y| p.max(libm::fabsf(*y)));
    ys.map(|y| libm::roundf(y / peak * half as f32) as i32)
}

/// One period of a VA oscillator's naive wave (`osc::wave_at`, no BLEP)
/// at `m` across `N` points, the last the first again: its own shape
/// code, so WAVE's in-between values draw the mixed shape. Scaled so its
/// peak is `half` px, up positive.
pub fn va_wave_trace<const N: usize>(m: Morph, half: i32) -> [i32; N] {
    let last = (N - 1).max(1) as f32;
    scaled(
        core::array::from_fn(|i| wave_at(i as f32 / last, m, MIN_INC).0),
        0.0,
        half,
    )
}

/// OSC 2's cycles a cycle of OSC 1 on MODE's glyph: a fixed ratio, so
/// SYNC's reset shows whatever CRSE says (at unison it would not).
pub const VA_MIX_RATIO: f32 = 2.5;

/// OSC 1's periods MODE's glyph draws.
pub const VA_MIX_PERIODS: f32 = 2.0;

/// OSC 1 MODE OSC 2, from the VA block's set values (normalized).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VaMix {
    pub mode: VaMode,
    pub amount: f32,
    pub osc: [Morph; 2],
}

impl VaMix {
    /// From `VA_MIX_PARAMS`' set values, normalized.
    pub fn from_set(set: [f32; 6]) -> Self {
        let morph = |i: usize| Morph {
            wave: Unit::new(set[i]),
            shape: Unit::new(set[i + 1]),
        };
        Self {
            mode: va_mode(set[0]),
            amount: clampf(set[1], 0.0, 1.0),
            osc: [morph(2), morph(4)],
        }
    }
}

/// MODE at its normalized set value.
pub fn va_mode(v: f32) -> VaMode {
    VaMode::ALL[index_of(v, VaMode::ALL.len())]
}

/// The operator's output over `VA_MIX_PERIODS` of OSC 1 across `N`
/// points: both naive waves, OSC 2 at `VA_MIX_RATIO` (reset by OSC 1's
/// wrap under SYNC, running back from it and forward again at the next
/// under SOFT SYNC), through `combine::combine` at AMOUNT with the
/// engine's own peak bounds, RING gain and crusher. FM reads OSC 1 at a
/// phase integrated across the points, its rate `1 + I·b/pk.b` (`I`
/// `fm_index` uncapped), so the scope shows the read speeding up, slowing
/// and running backwards. PHASE reads OSC 1 at `p1 + phase_depth·b/pk.b`,
/// the wave bent where OSC 2 is high. Scaled so its peak is `half` px (a near-silent
/// output no louder than a tenth of the inputs' peak), up positive.
pub fn va_mix_trace<const N: usize>(v: &VaMix, half: i32) -> [i32; N] {
    let [m1, m2] = v.osc;
    let pk = Peaks {
        a: peak_bound(m1, MIN_INC),
        b: peak_bound(m2, MIN_INC),
    };
    let mut crush = Crusher::NEW;
    crush.set(v.amount);
    let k = if v.mode == VaMode::Am {
        am_gain(v.amount, pk)
    } else {
        ring_gain(pk)
    };
    let frac = |x: f32| x - libm::floorf(x);
    let last = (N - 1).max(1) as f32;
    let (dt, index) = (VA_MIX_PERIODS / last, fm_index(v.amount, 0.0));
    // LOOPFM: each oscillator's b0 (WAVE scales its own loop), uncapped
    let loops = v.osc.map(|m| target_b0(v.amount, m.wave.get()));
    let depth = phase_depth(v.amount);
    let mut read = 0.0;
    let ys = core::array::from_fn(|i| {
        let t = VA_MIX_PERIODS * i as f32 / last;
        let p1 = frac(t);
        let p2 = if v.mode == VaMode::Sync {
            frac(VA_MIX_RATIO * p1)
        } else if v.mode == VaMode::SoftSync {
            // forward over an even period of OSC 1, back over an odd one
            let odd = libm::floorf(t) as i32 & 1 == 1;
            frac(VA_MIX_RATIO * if odd { 1.0 - p1 } else { p1 })
        } else {
            frac(VA_MIX_RATIO * t)
        };
        if v.mode == VaMode::LoopFm {
            let [b1, b2] = loops;
            let (a, b) = (lbfm(p1, b1), lbfm(p2, b2));
            return a + (b - a) * (0.5 * v.amount);
        }
        let b = wave_at(p2, m2, MIN_INC).0;
        if v.mode == VaMode::Fm {
            let a = wave_at(frac(read), m1, MIN_INC).0;
            read += dt * (1.0 + index * b / pk.b);
            return a;
        }
        if v.mode == VaMode::Phase {
            return wave_at(frac(p1 + depth * b / pk.b), m1, MIN_INC).0;
        }
        let a = wave_at(p1, m1, MIN_INC).0;
        combine(v.mode, a, b, v.amount, k, pk, &crush)
    });
    scaled(ys, 0.1 * pk.a.max(pk.b), half)
}

/// The words at BLEND's ends under `mode`: what AMOUNT 0 and 1 give.
/// MIX and SYNC fade OSC 1 to OSC 2; RING and XOR the plain mix to the
/// operator; BITS crushes the mix from 16 bits to 2; FM runs OSC 1 from
/// plain (CLEAN) to its full index (DEEP); AM from OSC 1 alone through
/// full AM (BLEND 85) to ring; MAX the plain mix to the larger of the two;
/// LOOPFM from OSC 1 alone to both looping (LOOP); PHASE from OSC 1 plain
/// to the CZ's full warp.
pub const fn blend_ends(mode: VaMode) -> [&'static str; 2] {
    match mode {
        VaMode::Mix | VaMode::Sync | VaMode::SoftSync => ["1", "2"],
        VaMode::Ring => ["MIX", "RING"],
        VaMode::Xor => ["MIX", "XOR"],
        VaMode::Bits => ["CLEAN", "CRUSH"],
        VaMode::Fm => ["CLEAN", "DEEP"],
        VaMode::Am => ["CLEAN", "RING"],
        VaMode::Max => ["MIX", "MAX"],
        VaMode::LoopFm => ["1", "LOOP"],
        VaMode::Phase => ["CLEAN", "WARP"],
    }
}

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
/// - GATE: a PRE-DLY gap, a plateau sloped by SHAPE, a cliff at LENGTH,
///   then the TAIL, dimmed by DAMP.
/// - REVERSE: silence to ONSET, a ramp rising by CURVE to the hit at
///   LENGTH that cuts dead, its echoes under REGEN.
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
    // DENSITY: GATE's missing taps; REVERSE's sparse allpasses.
    let notch = match t {
        ReverbType::Reverse => (0.7 - k.diffuse_g).max(0.0) * 12.0,
        _ => (12.0 - k.taps).max(0.0) * 0.75,
    };
    // GATE's DAMP: its TAIL darker, up to 6 dB at 2 kHz.
    let damped = 6.0 * clampf((12_000.0 - k.damp_hz) / 10_000.0, 0.0, 1.0);
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
        // GATE's PRE-DLY shifts the burst; REVERSE's ONSET opens a gap
        // inside it.
        let shift = if t == ReverbType::Gate {
            k.predelay_ms
        } else {
            0.0
        };
        let x = (ms - shift).max(0.0) / len;
        let pass = x as u32;
        let at = x - pass as f32;
        let tap = notch * tri(11.0 * at);
        let level = match t {
            ReverbType::Gate if ms < shift => -2.0 * TAIL_DB,
            ReverbType::Gate if pass == 0 => -curve * at - tap,
            ReverbType::Gate => tail - damped + fall * (ms - shift - len),
            ReverbType::Reverse if at * len < k.predelay_ms => -2.0 * TAIL_DB,
            // The hit: the column under LENGTH.
            ReverbType::Reverse if at > 1.0 - 0.6 * span / TAIL_COLS as f32 / len => {
                pass as f32 * echo
            }
            ReverbType::Reverse => pass as f32 * echo - 9.0 - curve * (1.0 - at) - tap,
            // ROOM and HALL: a pre-delay gap, then the fall.
            ReverbType::Room | ReverbType::Hall if ms < k.predelay_ms => -2.0 * TAIL_DB,
            ReverbType::Room | ReverbType::Hall => fall * (ms - k.predelay_ms),
            ReverbType::Quad | ReverbType::Bloom => fall * ms,
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
    pub fn from_set(t: ReverbType, set: [f32; 11], focus: Option<usize>, frame: u32) -> Self {
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
            density: set[10],
        };
        // Unsynced: the set values alone, at the default tempo.
        let k = knobs(&p, Tempo::DEFAULT, &mut FoldState(0));
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
            mode: ChorusMode::from_u8(index_of(set[0], 4) as u8),
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

/// The gauge the focus band draws for `glyph` at the focused slot's
/// `value` in format `fmt`. A composite's or VA's comes from `outside`,
/// which reads their params' set values (`Renderer::eased_set`).
pub fn gauge(
    glyph: FocusGlyph,
    value: f32,
    fmt: ValFmt,
    outside: impl FnOnce(Reads) -> Gauge,
) -> Gauge {
    let bipolar = fmt.is_bipolar();
    match glyph {
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
        FocusGlyph::Wave => Gauge::Wave(OpKind::clamped(libm::roundf(
            clampf(value, 0.0, 1.0) * fmt.max_int() as f32,
        ) as u8)),
        FocusGlyph::Composite(id) => outside(Reads::Composite(id)),
        FocusGlyph::Va(v) => outside(Reads::Va(v)),
        FocusGlyph::Tape => outside(Reads::Tape),
        FocusGlyph::Lfo => outside(Reads::Lfo),
        FocusGlyph::Stability => stability(value),
        FocusGlyph::Feedback => feedback(value),
        FocusGlyph::Arc => Gauge::Arc { value, bipolar },
    }
}

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
            | Gauge::Wave(_)
            | Gauge::VaWave(_)
            | Gauge::VaMix(_)
            | Gauge::Blend { .. }
            | Gauge::Tape(_)
            | Gauge::Lfo(_)
            | Gauge::Stability { .. }
            | Gauge::Feedback { .. } => false,
            Gauge::Braid(_) | Gauge::Rings(_) | Gauge::Cube(_) | Gauge::Tail(_) => true,
        }
    }

    /// What a still gauge draws from beyond its slot's set value (VA's
    /// other params), as a key: the focus band redraws when it moves.
    pub fn outside_key(&self) -> u32 {
        let q = |v: f32| (clampf(v, 0.0, 1.0) * 1023.0) as u32;
        let m = |m: &Morph| q(m.wave.get()) | q(m.shape.get()) << 10;
        match self {
            Gauge::VaWave(o) => m(o),
            Gauge::VaMix(v) => (m(&v.osc[0]) ^ m(&v.osc[1]).rotate_left(11) ^ q(v.amount) << 22)
                .wrapping_add(v.mode as u32),
            Gauge::Blend { mode, .. } => *mode as u32,
            // Its quantised steps: it redraws when one moves a pixel.
            Gauge::Tape(t) => t.key(),
            // Its quantised steps: it redraws when one moves.
            Gauge::Lfo(g) => g.key(),
            _ => 0,
        }
    }
}

/// The bubble's travel, px either side of centre: to the vial's ends.
pub const BUBBLE_TRAVEL: i32 = (theme::VIAL_W - theme::BUBBLE_W) / 2 - 2;

/// The spirit level at STABILITY `v`: VA's depth law, `(1 − v)²`, so the
/// bubble leaves the marks as the voices start to wander and reaches the
/// vial's ends at 0. It rests at half its reach, the ghost showing the
/// rest.
fn stability(v: f32) -> Gauge {
    let x = 1.0 - clampf(v, 0.0, 1.0);
    let reach = libm::roundf(x * x * BUBBLE_TRAVEL as f32) as i8;
    Gauge::Stability {
        offset: reach / 2,
        reach,
    }
}

/// FEEDBACK's steps either side of 0.
pub const FEEDBACK_STEPS: i32 = 8;

/// FEEDBACK `v` (0..1 for -1..1) in whole steps.
fn feedback(v: f32) -> Gauge {
    let b = clampf(v, 0.0, 1.0) * 2.0 - 1.0;
    Gauge::Feedback {
        q: libm::roundf(b * FEEDBACK_STEPS as f32) as i8,
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
