//! The variable-wave oscillator (spec § Oscillator). WAVE morphs sine →
//! triangle → saw → pulse, a linear crossfade of two neighbours; SHAPE
//! varies whichever two are sounding. Every wave, at every SHAPE, is
//! DC-free at one RMS `R`, with its fundamental at `sin 2πφ`, so a
//! crossfade never cancels it. Steps and corners, fixed or moving, are
//! found every sample and corrected by the 2-point kernels (`blep`). The
//! precedent is Ambika's variable-waveform oscillator, architecture only.
//!
//! The live pair's constants (crossfade weight folded in) and edge places
//! are computed at the ends of a run (a block, or the part of one on one
//! WAVE segment) and lerped per sample. Each edge keeps an offset `o` (its
//! place plus a whole number of cycles) with `u = phase − o` in `0..1`: `u`
//! leaving that range is the crossing, and `u` is the eval's phase since
//! the edge, so the eval and the crossings never disagree.

use super::blep::{blamp_into, blamp6_into, blep_into};
use crate::dsp::provenance::{PatentLine, Provenance};
use crate::dsp::{clampf, sqrt};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Textbook {
        cite: "Tierney, Rader & Gold, \"A Digital Frequency Synthesizer\", \
               IEEE Trans. Audio Electroacoustics 19(1), 1971 (phase accumulator)",
    },
    Provenance::Original {
        author: "AI-assisted, owner-directed and reviewed",
    },
];

pub const PATENT: &[PatentLine] = &[];

pub const GPL_CHECK: &str = "clean room: Ambika manual (architecture) only; MI firmware not read";

/// Every wave's RMS (−14 dBFS).
pub const R: f32 = 0.2;

/// The sine fold's RMS at SHAPE k/16 (lerped), before its gain `R / rms`.
#[allow(clippy::approx_constant)] // 0.7071 is the table's 4-digit entry
pub const SINE_RMS: [f32; 17] = [
    0.7071, 0.7052, 0.7090, 0.7171, 0.7279, 0.7398, 0.7512, 0.7607, 0.7668, 0.7686, 0.7650, 0.7558,
    0.7409, 0.7209, 0.6974, 0.6731, 0.6517,
];

/// Increments clamp here, cycles a sample: the top keeps every pitch
/// finite, the floor keeps `t = d / inc` finite under flush-to-zero.
pub const MAX_INC: f32 = 0.45;
pub const MIN_INC: f32 = 1e-6;

/// The triangle's peak, `R√3`.
const TRI_PEAK: f32 = R * 1.732_050_8;
const TAU: f32 = core::f32::consts::TAU;

/// `sin x` for |x| ≤ π within 6.1·10⁻⁶ (f32): an odd degree-9 polynomial,
/// least-squares weighted toward minimax. One argument reduction serves
/// both the phase (`2π(φ − ½)`) and the fold (|g·sin| ≤ 3).
const S: [f32; 5] = [
    0.999_979_4,
    -0.166_624_38,
    0.008_308_984,
    -0.000_192_649_82,
    2.147_865_6e-6,
];

#[inline(always)]
fn psin(x: f32) -> f32 {
    // Estrin's split of the Horner chain: one multiply-add fewer
    let (x2, x4) = (x * x, (x * x) * (x * x));
    let lo = S[0] + x2 * S[1];
    let hi = S[2] + x2 * S[3] + x4 * S[4];
    x * (lo + x4 * hi)
}

/// `psin`'s derivative.
#[inline(always)]
fn dpsin(x: f32) -> f32 {
    let x2 = x * x;
    S[0] + x2 * (3.0 * S[1] + x2 * (5.0 * S[2] + x2 * (7.0 * S[3] + x2 * (9.0 * S[4]))))
}

/// An `f32` in 0..=1; NaN is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Unit(f32);

impl Unit {
    pub const fn new(v: f32) -> Self {
        Self(clampf(v, 0.0, 1.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// One oscillator's WAVE and SHAPE.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Morph {
    pub wave: Unit,
    pub shape: Unit,
}

impl Morph {
    /// The live pair and the second wave's weight.
    #[inline(always)]
    fn segment(self) -> (Pair, f32) {
        let x3 = 3.0 * self.wave.get();
        let p = Pair::of(x3);
        (p, x3 - p as usize as f32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wave {
    Sine,
    Tri,
    Saw,
    Pulse,
}

impl Wave {
    /// The slot of a wave with edges (the sine has none).
    #[inline(always)]
    fn edge_slot(self) -> Option<usize> {
        match self {
            Wave::Sine => None,
            Wave::Tri => Some(0),
            Wave::Saw => Some(1),
            Wave::Pulse => Some(2),
        }
    }

    /// The run constants (`Kind::value`'s `c`), weighted by `x`.
    #[inline(always)]
    fn consts(self, sh: &Shaped, x: f32) -> [f32; 3] {
        match self {
            // y = −x·g·((1 − s)·P + s·psin(g'·P)), P = psin(2π(φ − ½)) = −sin 2πφ
            Wave::Sine => [
                -x * sh.sine_g * (1.0 - sh.fold),
                -x * sh.sine_g * sh.fold,
                1.0 + 2.0 * sh.fold,
            ],
            Wave::Tri => {
                let w = sh.tri_w;
                [-x * TRI_PEAK, x * sh.tri_j * (1.0 - w), x * sh.tri_j * w]
            }
            Wave::Saw => {
                let g = x * sh.saw_g;
                [g, -2.0 * g * sh.saw_a, -2.0 * g * sh.saw_b]
            }
            Wave::Pulse => {
                let g = 2.0 * x * sh.pulse_g;
                [-g * sh.pulse_w, g, 0.0]
            }
        }
    }

    /// The edges' places, in 0..=1 (the sine's are unused).
    #[inline(always)]
    fn pos(self, sh: &Shaped) -> [f32; 2] {
        match self {
            Wave::Sine => [0.0; 2],
            Wave::Tri => [1.0 - 0.5 * sh.tri_w, 0.5 * sh.tri_w],
            Wave::Saw => [1.0 + sh.saw_e1, sh.saw_e2],
            Wave::Pulse => [0.25 - 0.5 * sh.pulse_w, 0.25 + 0.5 * sh.pulse_w],
        }
    }

    fn value_slope(self, c: [f32; 3], p: f32, pos: [f32; 2]) -> (f32, f32) {
        match self {
            Wave::Sine => Sine::value_slope(c, p, pos),
            Wave::Tri => Tri::value_slope(c, p, pos),
            Wave::Saw => Saw::value_slope(c, p, pos),
            Wave::Pulse => Pulse::value_slope(c, p, pos),
        }
    }
}

/// The two live waves: WAVE's segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    SineTri = 0,
    TriSaw = 1,
    SawPulse = 2,
}

impl Pair {
    /// The pair at `3 × WAVE`.
    #[inline(always)]
    fn of(x3: f32) -> Pair {
        match x3 as usize {
            0 => Pair::SineTri,
            1 => Pair::TriSaw,
            _ => Pair::SawPulse,
        }
    }

    /// The neighbouring pair up or down WAVE, the end pairs their own.
    #[inline(always)]
    fn next(self, up: bool) -> Pair {
        match (self, up) {
            (Pair::SineTri, true) | (Pair::SawPulse, false) => Pair::TriSaw,
            (Pair::TriSaw, true) | (Pair::SawPulse, true) => Pair::SawPulse,
            (Pair::TriSaw, false) | (Pair::SineTri, false) => Pair::SineTri,
        }
    }

    fn waves(self) -> (Wave, Wave) {
        match self {
            Pair::SineTri => (Wave::Sine, Wave::Tri),
            Pair::TriSaw => (Wave::Tri, Wave::Saw),
            Pair::SawPulse => (Wave::Saw, Wave::Pulse),
        }
    }

    /// Both waves' constants and edge places at `m`, the second weighted
    /// by `3·WAVE − self` (outside 0..1 when `m` is past the segment: a
    /// run starting on it extrapolates its lines from the sample before).
    fn consts(self, m: Morph, inc: f32) -> ([f32; 6], [f32; 4]) {
        let (a, b) = self.waves();
        let f = 3.0 * m.wave.get() - self as usize as f32;
        let mut sh = Shaped::ZERO;
        sh.fill(a, m.shape.get(), inc);
        sh.fill(b, m.shape.get(), inc);
        let (ca, cb) = (a.consts(&sh, 1.0 - f), b.consts(&sh, f));
        let (pa, pb) = (a.pos(&sh), b.pos(&sh));
        (
            [ca[0], ca[1], ca[2], cb[0], cb[1], cb[2]],
            [pa[0], pa[1], pb[0], pb[1]],
        )
    }
}

/// One wave's per-sample work: the hot loop's eval and crossing, and the
/// value with its slope for the reference and the sync reset. `c` holds
/// `Wave::consts`.
trait Kind {
    const WAVE: Wave;
    /// Two edges (all but the sine).
    const EDGES: bool;
    /// The edges are corners (BLAMP), not steps (BLEP).
    const CORNERS: bool;
    /// The pulse: its level `h` (0 or 1) flips at every crossing.
    const FLIPS: bool;
    /// `c[0]` is a constant term (all but the sine): a pair of two keeps
    /// one, their sum.
    const BIAS: bool;

    /// The value at phase `p`, `u` the phases since the edges.
    fn value(c: [f32; 3], p: f32, u: [f32; 2], h: f32) -> f32;

    /// Edge `e` crossed (`fwd`, else backward) `t` samples ago: the step,
    /// or the corner's slope jump a cycle, from the constants lerped back to
    /// the crossing.
    fn jump(c: [Lin; 3], t: f32, e: usize, fwd: bool, h: f32) -> f32;

    /// The slope a cycle at `p`, `u` as `value`'s (a sync reset's).
    fn slope(c: [f32; 3], p: f32, u: [f32; 2]) -> f32;

    /// The value at `p` and its slope a cycle, edges at `pos` (mod 1).
    fn value_slope(c: [f32; 3], p: f32, pos: [f32; 2]) -> (f32, f32);
}

struct Sine;
struct Tri;
struct Saw;
struct Pulse;

impl Kind for Sine {
    const WAVE: Wave = Wave::Sine;
    const EDGES: bool = false;
    const CORNERS: bool = false;
    const FLIPS: bool = false;
    const BIAS: bool = false;

    #[inline(always)]
    fn value(c: [f32; 3], p: f32, _: [f32; 2], _: f32) -> f32 {
        let q = psin(TAU * (p - 0.5));
        c[0] * q + c[1] * psin(c[2] * q)
    }

    fn jump(_: [Lin; 3], _: f32, _: usize, _: bool, _: f32) -> f32 {
        0.0
    }

    #[inline(always)]
    fn slope(c: [f32; 3], p: f32, _: [f32; 2]) -> f32 {
        let th = TAU * (p - 0.5);
        TAU * dpsin(th) * (c[0] + c[1] * c[2] * dpsin(c[2] * psin(th)))
    }

    fn value_slope(c: [f32; 3], p: f32, _: [f32; 2]) -> (f32, f32) {
        let th = TAU * (p - 0.5);
        let q = psin(th);
        let y = c[0] * q + c[1] * psin(c[2] * q);
        let d = TAU * dpsin(th) * (c[0] + c[1] * c[2] * dpsin(c[2] * q));
        (y, d)
    }
}

impl Kind for Tri {
    const WAVE: Wave = Wave::Tri;
    const EDGES: bool = true;
    const CORNERS: bool = true;
    const FLIPS: bool = false;
    const BIAS: bool = true;

    /// The two sides from the trough (`u[0]`), the lower one: continuous
    /// whatever the lerp does to `up` and `down`.
    #[inline(always)]
    fn value(c: [f32; 3], _: f32, u: [f32; 2], _: f32) -> f32 {
        c[0] + (c[1] * u[0]).min(c[2] * (1.0 - u[0]))
    }

    #[inline(always)]
    fn jump(c: [Lin; 3], t: f32, e: usize, _: bool, _: f32) -> f32 {
        // a trough bends up, a peak down, whichever way it is crossed
        // (the slopes' sum lerped back: its step is the run's)
        let j = (c[1].v + c[2].v) - (c[1].d + c[2].d) * t;
        if e == 0 { j } else { -j }
    }

    #[inline(always)]
    fn slope(c: [f32; 3], _: f32, u: [f32; 2]) -> f32 {
        if c[1] * u[0] < c[2] * (1.0 - u[0]) {
            c[1]
        } else {
            -c[2]
        }
    }

    fn value_slope(c: [f32; 3], p: f32, pos: [f32; 2]) -> (f32, f32) {
        let u = frac(p - pos[0]);
        let (up, down) = (c[1] * u, c[2] * (1.0 - u));
        if up < down {
            (c[0] + up, c[1])
        } else {
            (c[0] + down, -c[2])
        }
    }
}

impl Kind for Saw {
    const WAVE: Wave = Wave::Saw;
    const EDGES: bool = true;
    const CORNERS: bool = false;
    const FLIPS: bool = false;
    const BIAS: bool = true;

    #[inline(always)]
    fn value(c: [f32; 3], _: f32, u: [f32; 2], _: f32) -> f32 {
        c[0] + c[1] * u[0] + c[2] * u[1]
    }

    #[inline(always)]
    fn jump(c: [Lin; 3], t: f32, e: usize, fwd: bool, _: f32) -> f32 {
        let s = if e == 0 { -c[1].back(t) } else { -c[2].back(t) };
        if fwd { s } else { -s }
    }

    #[inline(always)]
    fn slope(c: [f32; 3], _: f32, _: [f32; 2]) -> f32 {
        c[1] + c[2]
    }

    fn value_slope(c: [f32; 3], p: f32, pos: [f32; 2]) -> (f32, f32) {
        let (u1, u2) = (frac(p - pos[0]), frac(p - pos[1]));
        (c[0] + c[1] * u1 + c[2] * u2, c[1] + c[2])
    }
}

impl Kind for Pulse {
    const WAVE: Wave = Wave::Pulse;
    const EDGES: bool = true;
    const CORNERS: bool = false;
    const FLIPS: bool = true;
    const BIAS: bool = true;

    #[inline(always)]
    fn value(c: [f32; 3], _: f32, _: [f32; 2], h: f32) -> f32 {
        c[0] + c[1] * h
    }

    #[inline(always)]
    fn jump(c: [Lin; 3], t: f32, _: usize, _: bool, h: f32) -> f32 {
        c[1].back(t) * (1.0 - 2.0 * h)
    }

    #[inline(always)]
    fn slope(_: [f32; 3], _: f32, _: [f32; 2]) -> f32 {
        0.0
    }

    fn value_slope(c: [f32; 3], p: f32, pos: [f32; 2]) -> (f32, f32) {
        (c[0] + c[1] * high(p, pos), 0.0)
    }
}

/// 1 where the pulse is high at `p` (edges at `pos`), else 0: high is
/// `[rise, fall)`, so the phase since the rise is the shorter.
#[inline(always)]
fn high(p: f32, pos: [f32; 2]) -> f32 {
    if frac(p - pos[0]) < frac(p - pos[1]) {
        1.0
    } else {
        0.0
    }
}

/// Each wave's constants at one SHAPE (only the live waves' are current).
#[derive(Clone, Copy, Debug)]
struct Shaped {
    fold: f32,
    sine_g: f32,
    tri_w: f32,
    /// The corners' slope jump per cycle, `2R√3 / (w(1 − w))`.
    tri_j: f32,
    saw_a: f32,
    saw_b: f32,
    saw_e1: f32,
    saw_e2: f32,
    saw_g: f32,
    pulse_w: f32,
    pulse_g: f32,
}

impl Shaped {
    const ZERO: Self = Self {
        fold: 0.0,
        sine_g: 0.0,
        tri_w: 0.5,
        tri_j: 0.0,
        saw_a: 1.0,
        saw_b: 0.0,
        saw_e1: 0.0,
        saw_e2: 0.0,
        saw_g: 0.0,
        pulse_w: 0.5,
        pulse_g: 0.0,
    };

    fn fill(&mut self, wave: Wave, s: f32, inc: f32) {
        match wave {
            Wave::Sine => {
                let x = s * 16.0;
                let i = (x as usize).min(15);
                let f = x - i as f32;
                self.fold = s;
                self.sine_g = R / (SINE_RMS[i] + (SINE_RMS[i + 1] - SINE_RMS[i]) * f);
            }
            Wave::Tri => {
                let w = 0.5 + 0.45 * s;
                self.tri_w = w;
                self.tri_j = 2.0 * TRI_PEAK / (w * (1.0 - w));
            }
            Wave::Saw => {
                let (a, b, q) = (1.0 - 0.5 * s, 0.5 * s, 0.25 * s);
                let ms = (a * a + b * b) / 3.0 + 2.0 * a * b * (1.0 / 3.0 - 2.0 * q + 2.0 * q * q);
                (self.saw_a, self.saw_b) = (a, b);
                (self.saw_e1, self.saw_e2) = (-q * b, q * a);
                self.saw_g = R / sqrt(ms);
            }
            Wave::Pulse => {
                let w = (0.5 - 0.45 * s).max(inc);
                self.pulse_w = w;
                self.pulse_g = R / (2.0 * sqrt(w * (1.0 - w)));
            }
        }
    }

    /// Every wave's constants at SHAPE `s`, the pulse floored at `inc`.
    fn all(s: f32, inc: f32) -> Self {
        let mut sh = Self::ZERO;
        for w in WAVES {
            sh.fill(w, s, inc);
        }
        sh
    }

    fn peak(&self, wave: Wave) -> f32 {
        match wave {
            Wave::Sine => self.sine_g,
            Wave::Tri => TRI_PEAK,
            Wave::Saw => self.saw_g,
            Wave::Pulse => 2.0 * (1.0 - self.pulse_w) * self.pulse_g,
        }
    }

    /// Every wave's `peak`, in WAVE's order.
    #[inline(always)]
    fn peaks(&self) -> [f32; 4] {
        WAVES.map(|w| self.peak(w))
    }
}

/// The naive value at `phase` (0..1) and its slope per cycle: the morph's
/// two waves, weighted, the pulse never narrower than `inc`. The reference
/// for tests.
pub fn wave_at(phase: f32, m: Morph, inc: f32) -> (f32, f32) {
    let (pair, _) = m.segment();
    let (a, b) = pair.waves();
    let (c, pos) = pair.consts(m, inc);
    let (ya, da) = a.value_slope([c[0], c[1], c[2]], phase, [pos[0], pos[1]]);
    let (yb, db) = b.value_slope([c[3], c[4], c[5]], phase, [pos[2], pos[3]]);
    (ya + yb, da + db)
}

/// A bound on |`wave_at`|: each live wave's peak, weighted.
pub fn peak_bound(m: Morph, inc: f32) -> f32 {
    let (pair, f) = m.segment();
    let (a, b) = pair.waves();
    let mut sh = Shaped::ZERO;
    sh.fill(a, m.shape.get(), inc);
    sh.fill(b, m.shape.get(), inc);
    (1.0 - f) * sh.peak(a) + f * sh.peak(b)
}

/// Whether edges are corrected; `Off` is the alias gate's baseline.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Correction {
    On,
    Off,
}

/// One sample: the output, one sample late, and `t`, the samples since
/// the phase wrapped, if it wrapped this sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    pub out: f32,
    pub wrap: Option<f32>,
}

/// An oscillator's settings over a block, each a ramp `(from, step)`:
/// sample `i` plays `from + step·(i + 1)` (`ease::at`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Ramps {
    pub inc: (f32, f32),
    pub wave: (f32, f32),
    pub shape: (f32, f32),
}

/// A ramp's value `j` samples in: sample `i` is `j = i + 1`, 0 the one
/// before the block.
#[inline(always)]
fn val((from, step): (f32, f32), j: usize) -> f32 {
    from + step * j as f32
}

/// The pair sample `i` plays on `r`.
#[inline(always)]
fn seg(r: &Ramps, i: usize) -> Pair {
    Pair::of(3.0 * Unit::new(val(r.wave, i + 1)).get())
}

/// WAVE's segment edges, `k/3`.
const THIRDS: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];

/// The end of the run on `pair` from `i0` in an `n`-sample block that
/// leaves it: the first sample past the segment's edge. WAVE's ramp is
/// linear, so the crossing is solved for, and the solve is the split: a
/// sample that rounding leaves on either side plays its pair within a hair
/// of the edge, where both pairs are the anchor's pure wave. At least one
/// sample, at most the block.
#[inline(never)]
fn run_end(r: &Ramps, pair: Pair, i0: usize, n: usize) -> usize {
    let (from, step) = r.wave;
    let k = pair as usize + usize::from(step > 0.0);
    // sample i plays j = i + 1: the first j past the edge (`as` saturates,
    // NaN to 0); max/min, not clamp: no assert (and no panic path) in audio
    (((THIRDS[k.min(3)] - from) / step) as usize).max(i0 + 1).min(n)
}

/// The pair `a`, `b`'s constants and places from SHAPE constants `sh`,
/// `b` weighted `f`.
#[inline(always)]
fn at(a: Wave, b: Wave, sh: &Shaped, f: f32) -> ([f32; 6], [f32; 4]) {
    let (ca, cb) = (a.consts(sh, 1.0 - f), b.consts(sh, f));
    let (pa, pb) = (a.pos(sh), b.pos(sh));
    (
        [ca[0], ca[1], ca[2], cb[0], cb[1], cb[2]],
        [pa[0], pa[1], pb[0], pb[1]],
    )
}

/// `at` between the block's SHAPE ends, `x` of the way: a run's end or
/// start at a WAVE segment crossing. The constants themselves are lerped
/// (each linear in its weight), so a value stays between its ends' and the
/// block's peak bound holds.
#[inline(never)]
fn at_lerp<A: Kind, B: Kind>(blk: &Blk, x: f32, f: f32) -> ([f32; 6], [f32; 4]) {
    let (c0, p0) = at(A::WAVE, B::WAVE, &blk.sh0, f);
    let (c1, p1) = at(A::WAVE, B::WAVE, &blk.sh1, f);
    let l = |a: f32, b: f32| a + (b - a) * x;
    (
        core::array::from_fn(|k| l(c0[k], c1[k])),
        core::array::from_fn(|k| l(p0[k], p1[k])),
    )
}

/// Every wave's SHAPE constants at the end of the block on `r`.
#[inline(never)]
fn end_shape(r: &Ramps, n: usize) -> Shaped {
    Shaped::all(Unit::new(val(r.shape, n)).get(), val(r.inc, n))
}

/// 1/n for n in 0..=64 (0 unused): a run's per-sample steps, undivided.
static RECIP: [f32; 65] = {
    let mut t = [0.0f32; 65];
    let mut n = 1;
    while n < 65 {
        t[n] = 1.0 / n as f32;
        n += 1;
    }
    t
};

/// The waves in WAVE's order.
const WAVES: [Wave; 4] = [Wave::Sine, Wave::Tri, Wave::Saw, Wave::Pulse];

/// The bound on |output| over a block whose WAVE runs `w0` → `w1` (linear)
/// while each wave's constants lerp from `sh0`'s to `sh1`'s (a wave's peak
/// lerps with them): the path is split where it passes a pure wave (an
/// anchor, which can peak above both ends); on each piece the crossfaded
/// peak is a quadratic in time, bounded by its ends plus a quarter of its
/// curvature.
fn bound(sh0: &Shaped, sh1: &Shaped, w0: f32, w1: f32) -> f32 {
    let (k0, k1) = (Pair::of(3.0 * w0) as usize, Pair::of(3.0 * w1) as usize);
    // no anchor passed (the steady case): one piece
    if k0 == k1 {
        let (p0, p1) = (sh0.peaks(), sh1.peaks());
        return match k0 {
            0 => piece::<0>(&p0, &p1, (w0, w1), (0.0, 1.0)),
            1 => piece::<1>(&p0, &p1, (w0, w1), (0.0, 1.0)),
            _ => piece::<2>(&p0, &p1, (w0, w1), (0.0, 1.0)),
        };
    }
    bound_split(sh0, sh1, (w0, w1), (k0, k1))
}

/// `bound` across anchors: a piece for each pair the path is on (pairs
/// `k0` to `k1`), over the stretch it is on it.
#[inline(never)]
fn bound_split(sh0: &Shaped, sh1: &Shaped, (w0, w1): (f32, f32), (k0, k1): (usize, usize)) -> f32 {
    let dw = w1 - w0;
    // when the path is at WAVE `a`, held to the block (an anchor not
    // passed lands on an end)
    let at = |a: f32| clampf((a - w0) / dw, 0.0, 1.0);
    let (t1, t2) = (at(1.0 / 3.0), at(2.0 / 3.0));
    let (p0, p1) = (sh0.peaks(), sh1.peaks());
    let w = (w0, w1);
    let (lo, hi) = (k0.min(k1), k0.max(k1));
    // the path rises through the pairs or falls; a pair it is not on adds 0
    let on = |k: usize, v: f32| if lo <= k && k <= hi { v } else { 0.0 };
    let (s0, s1, s2) = if dw > 0.0 {
        ((0.0, t1), (t1, t2), (t2, 1.0))
    } else {
        ((t1, 1.0), (t2, t1), (0.0, t2))
    };
    on(0, piece::<0>(&p0, &p1, w, s0))
        .max(on(1, piece::<1>(&p0, &p1, w, s1)))
        .max(on(2, piece::<2>(&p0, &p1, w, s2)))
}

/// `bound` on the piece `ta..tb` of the path, on pair `K`, each wave's
/// peak lerping from `pk0`'s to `pk1`'s.
#[inline(always)]
fn piece<const K: usize>(
    pk0: &[f32; 4],
    pk1: &[f32; 4],
    (w0, w1): (f32, f32),
    (ta, tb): (f32, f32),
) -> f32 {
    let dw = w1 - w0;
    let (wa, wb) = (w0 + dw * ta, w0 + dw * tb);
    let pk = |i: usize, t: f32| {
        let (a, b) = (pk0[i], pk1[i]);
        a + (b - a) * t
    };
    let (a, b) = (K, K + 1);
    let base = K as f32;
    let f = |w: f32| clampf(3.0 * w - base, 0.0, 1.0);
    let (fa, fb) = (f(wa), f(wb));
    let g = |t: f32, fw: f32| (1.0 - fw) * pk(a, t) + fw * pk(b, t);
    let curve = (fb - fa) * ((pk(b, tb) - pk(b, ta)) - (pk(a, tb) - pk(a, ta)));
    g(ta, fa).max(g(tb, fb)) + 0.25 * curve.abs()
}

/// One block's SHAPE ends and edge rates (`VarOsc::block`).
struct Blk {
    sh0: Shaped,
    sh1: Shaped,
    /// Tri, saw, pulse: each edge's `1/r` and `|r|/6`.
    rate: [[(f32, f32); 2]; 3],
    rinc: f32,
    /// Each wave's value and slope at phase 0, unit weight (SYNC).
    zero: [(f32, f32); 4],
}

/// "No wrap" in a wrap buffer (a wrap's `t` is in 0..=1).
pub const NO_WRAP: f32 = -1.0;

/// The fraction of `x` (|x| < 2³¹), in 0..1.
#[inline(always)]
fn frac(x: f32) -> f32 {
    x - floor(x)
}

/// ⌊x⌋ for |x| < 2³¹, exact either way: VRINTM on the M7, the integer
/// round trip on the host.
#[inline(always)]
fn floor(x: f32) -> f32 {
    #[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
    {
        let r: f32;
        // SAFETY: one register-to-register VFP op (FPv5's VRINTM); no
        // memory or stack touched. It may set FPSCR's IOC on a signalling
        // NaN; nothing reads it.
        unsafe {
            core::arch::asm!("vrintm.f32 {r}, {x}", r = lateout(sreg) r, x = in(sreg) x,
                options(pure, nomem, nostack));
        }
        r
    }
    #[cfg(not(all(target_arch = "arm", target_feature = "vfp2")))]
    {
        let i = x as i32;
        (if (i as f32) > x { i - 1 } else { i }) as f32
    }
}

/// A run constant: its value this sample and its step a sample.
#[derive(Clone, Copy, Debug, Default)]
struct Lin {
    v: f32,
    d: f32,
}

impl Lin {
    /// The value `t` samples back (0..1): the crossing instant.
    #[inline(always)]
    fn back(self, t: f32) -> f32 {
        self.v - self.d * t
    }
}

/// One run's lerps: the pair's six constants; each edge's phase since it
/// (`u`, in 0..1 between crossings) and its place's step a sample (`od`);
/// each edge's `1/r` and `|r|/6` (`r` the phase's speed past it, cycles a
/// sample; a corner's BLAMP takes `/6`) and the increment. Built in
/// registers at a run's start (`setup`, inlined): never in memory.
#[derive(Clone, Copy)]
struct Run {
    c: [Lin; 6],
    u: [f32; 4],
    od: [f32; 4],
    rcp: [f32; 4],
    ar: [f32; 4],
    /// `1/inc` mid-run: a wrap's `t` (jitter and the ramp move it by
    /// under 10⁻³ of itself).
    rinc: f32,
    inc: f32,
    dinc: f32,
    /// The value at phase 0 and its slope a cycle, at the run's end: a
    /// sync reset's landing.
    y0: f32,
    d0: f32,
}

impl Run {
    const ZERO: Self = Self {
        c: [Lin { v: 0.0, d: 0.0 }; 6],
        u: [0.0; 4],
        od: [0.0; 4],
        rcp: [0.0; 4],
        ar: [0.0; 4],
        rinc: 0.0,
        inc: 0.0,
        dinc: 0.0,
        y0: 0.0,
        d0: 0.0,
    };
}

#[cfg(any(test, feature = "test-support"))]
const EDGE_LOG: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct VarOsc {
    phase: f32,
    inv_sr: f32,
    #[cfg(any(test, feature = "test-support"))]
    correction: Correction,
    /// The corrector's held sample: the output, one sample late.
    held: f32,
    /// The pair whose constants and edge places (at the last sample) are
    /// held, if they are current.
    pair: Option<Pair>,
    c: [f32; 6],
    pos: [f32; 4],
    /// Each edged wave's phase since its edges (tri, saw, pulse), where
    /// `live` has it.
    since: [[f32; 2]; 3],
    live: u8,
    /// The pulse's level, 0 or 1.
    h: f32,
    /// Every wave's constants at the last block's end, if current; and the
    /// next block's end, if `begin` set it.
    sh: Shaped,
    sh_ok: bool,
    next: Option<Shaped>,
    #[cfg(any(test, feature = "test-support"))]
    log: [(f32, f32, f32); EDGE_LOG],
    #[cfg(any(test, feature = "test-support"))]
    logged: usize,
    #[cfg(any(test, feature = "test-support"))]
    skip: Option<usize>,
}

impl VarOsc {
    pub const NEW: Self = Self {
        phase: 0.0,
        inv_sr: 1.0 / 48_000.0,
        #[cfg(any(test, feature = "test-support"))]
        correction: Correction::On,
        held: 0.0,
        pair: None,
        c: [0.0; 6],
        pos: [0.0; 4],
        since: [[0.0; 2]; 3],
        live: 0,
        h: 0.0,
        sh: Shaped::ZERO,
        sh_ok: false,
        next: None,
        #[cfg(any(test, feature = "test-support"))]
        log: [(0.0, 0.0, 0.0); EDGE_LOG],
        #[cfg(any(test, feature = "test-support"))]
        logged: 0,
        #[cfg(any(test, feature = "test-support"))]
        skip: None,
    };

    pub fn prepare(&mut self, sample_rate: f32) {
        self.inv_sr = 1.0 / sample_rate;
    }

    /// Cycles a sample for `hz`, clamped to `[MIN_INC, MAX_INC]` (NaN is
    /// `MIN_INC`).
    pub fn inc_for(&self, hz: f32) -> f32 {
        clampf(hz * self.inv_sr, MIN_INC, MAX_INC)
    }

    /// Phase 0, the corrector cleared, the edges re-anchored and the
    /// constants recomputed at the next sample.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.held = 0.0;
        self.live = 0;
        self.pair = None;
        self.sh_ok = false;
    }

    /// The next block's constants start on its settings, not where the
    /// last block left them (a fresh note's ramps start on their values).
    pub fn hold(&mut self) {
        self.pair = None;
        self.sh_ok = false;
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Jumps the phase (wrapped into 0..1; NaN is 0). The jump itself is
    /// not an edge: the edges re-anchor.
    pub fn set_phase(&mut self, p: f32) {
        let f = p - libm::floorf(p);
        self.phase = if (0.0..1.0).contains(&f) { f } else { 0.0 };
        self.live = 0;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_correction(mut self, c: Correction) -> Self {
        self.correction = c;
        self
    }

    /// One sample at `inc` cycles a sample and morph `m`, exact (its own
    /// run). `sync` is a master's wrap `t` samples ago: the phase restarts
    /// at 0 there (spec § Combine).
    pub fn tick(&mut self, inc: f32, m: Morph, sync: Option<f32>) -> Tick {
        #[cfg(any(test, feature = "test-support"))]
        {
            self.logged = 0;
        }
        let r = Ramps {
            inc: (inc, 0.0),
            wave: (m.wave.get(), 0.0),
            shape: (m.shape.get(), 0.0),
        };
        let mut out = [0.0f32];
        let mut wr = [sync.map_or(NO_WRAP, |t| clampf(t, 0.0, 1.0))];
        self.render::<1, true, true>(&mut out, &mut wr, &r, &mut 1.0, &mut || 1.0);
        Tick {
            out: out[0],
            wrap: (wr[0] >= 0.0).then_some(wr[0]),
        }
    }

    /// `N` samples into `out`, one sample late. `REC` writes each sample's
    /// wrap `t` (else `NO_WRAP`) into `wr`; `SYNC` restarts the phase at
    /// each `t` it reads there first. `jit` scales the increment; `draw`
    /// gives the next at each wrap (STABILITY's jitter).
    #[inline(never)]
    pub fn render<const N: usize, const REC: bool, const SYNC: bool>(
        &mut self,
        out: &mut [f32; N],
        wr: &mut [f32; N],
        r: &Ramps,
        jit: &mut f32,
        draw: &mut impl FnMut() -> f32,
    ) {
        let blk = self.block::<SYNC>(r, N, *jit);
        // WAVE's ramp is linear: the pairs run from the first sample's to
        // the last's, one segment at a time
        let (mut pair, last, up) = (seg(r, 0), seg(r, N - 1), r.wave.1 > 0.0);
        let mut i0 = 0;
        while i0 < N {
            let i1 = if pair == last { N } else { run_end(r, pair, i0, N) };
            let span = (i0, i1);
            match pair {
                Pair::SineTri => self.run::<Sine, Tri, N, REC, SYNC>(&blk, span, r, out, wr, jit, draw),
                Pair::TriSaw => self.run::<Tri, Saw, N, REC, SYNC>(&blk, span, r, out, wr, jit, draw),
                Pair::SawPulse => {
                    self.run::<Saw, Pulse, N, REC, SYNC>(&blk, span, r, out, wr, jit, draw)
                }
            }
            i0 = i1;
            pair = pair.next(up);
        }
    }

    /// Every wave's SHAPE constants at the end of the block on `r` (`n`
    /// samples), kept for `render`, and the block's peak bound: the larger
    /// of its two ends' (`peak_bound`'s, from the same constants).
    pub fn begin(&mut self, r: &Ramps, n: usize) -> f32 {
        let sh1 = Shaped::all(Unit::new(val(r.shape, n)).get(), val(r.inc, n));
        let sh0 = if self.sh_ok { self.sh } else { sh1 };
        self.next = Some(sh1);
        bound(
            &sh0,
            &sh1,
            Unit::new(val(r.wave, 0)).get(),
            Unit::new(val(r.wave, n)).get(),
        )
    }

    /// The block's ends (the start is the last block's end), each edged
    /// wave's `1/r` and `|r|/6` (`r` the phase's speed past an edge at the
    /// block's middle increment; places move linearly across a block) and,
    /// for a sync slave, each wave's value and slope at phase 0 (unit weight)
    /// at the block's end.
    #[inline(never)]
    fn block<const SYNC: bool>(&mut self, r: &Ramps, n: usize, jit: f32) -> Blk {
        // the engine's blocks have `begin`'s; a lone `tick` fills here
        let sh1 = match self.next.take() {
            Some(sh) => sh,
            None => end_shape(r, n),
        };
        let sh0 = if self.sh_ok { self.sh } else { sh1 };
        (self.sh, self.sh_ok) = (sh1, true);
        let ie = val(r.inc, n.div_ceil(2)) * jit;
        let il = RECIP[n.min(RECIP.len() - 1)];
        let mut blk = Blk {
            sh0,
            sh1,
            rate: [[(0.0, 0.0); 2]; 3],
            rinc: 1.0 / ie,
            zero: [(0.0, 0.0); 4],
        };
        for (e, w) in [Wave::Tri, Wave::Saw, Wave::Pulse].into_iter().enumerate() {
            let (p0, p1) = (w.pos(&sh0), w.pos(&sh1));
            for q in 0..2 {
                let rate = ie - (p1[q] - p0[q]) * il;
                blk.rate[e][q] = (1.0 / rate, rate.abs() * (1.0 / 6.0));
            }
        }
        if SYNC {
            for (k, w) in WAVES.into_iter().enumerate() {
                blk.zero[k] = w.value_slope(w.consts(&sh1, 1.0), 0.0, w.pos(&sh1));
            }
        }
        blk
    }

    /// The run of samples `i0..i1` on the pair `A`, `B`: its constants lerp
    /// from the held ones (or, on a new pair, from the sample before) to
    /// those at its last sample, from the block's SHAPE ends (no fill, no
    /// divide); each edge's place glides to its place there. Inlined into
    /// the run's loop, so the `Run` stays in registers.
    #[inline(always)]
    fn setup<A: Kind, B: Kind, const SYNC: bool>(
        &mut self,
        blk: &Blk,
        (i0, i1): (usize, usize),
        r: &Ramps,
        n: usize,
    ) -> Run {
        let mut run = Run::ZERO;
        let il = RECIP[(i1 - i0).min(RECIP.len() - 1)];
        let (a, b) = (A::WAVE, B::WAVE);
        let pair = Pair::of(a as usize as f32);
        let weight = |j: usize| 3.0 * Unit::new(val(r.wave, j)).get() - pair as usize as f32;
        // the block's end is its SHAPE end itself; a run ending earlier
        // (a WAVE segment crossing) lerps the two ends
        let (c1, p1) = if i1 == n {
            at(a, b, &blk.sh1, weight(i1))
        } else {
            at_lerp::<A, B>(blk, i1 as f32 * RECIP[n.min(RECIP.len() - 1)], weight(i1))
        };
        // held: lerp on; a new note: start on the values (as its ramps do);
        // a new segment: the pair's lines from the sample before
        let (c0, p0) = match self.pair {
            Some(q) if q == pair => (self.c, self.pos),
            None => (c1, p1),
            Some(_) => at_lerp::<A, B>(blk, i0 as f32 * RECIP[n.min(RECIP.len() - 1)], weight(i0)),
        };
        (self.pair, self.c, self.pos) = (Some(pair), c1, p1);
        (run.rinc, run.inc, run.dinc) = (blk.rinc, val(r.inc, i0), r.inc.1);
        for (k, l) in run.c.iter_mut().enumerate() {
            *l = Lin {
                v: c0[k],
                d: (c1[k] - c0[k]) * il,
            };
        }
        if A::BIAS && B::BIAS {
            // one constant term, the sum (`run`'s B leaves its own out)
            let (v, e) = (c0[0] + c0[3], c1[0] + c1[3]);
            run.c[0] = Lin { v, d: (e - v) * il };
        }
        if SYNC {
            let f = weight(i1);
            let (za, zb) = (blk.zero[a as usize], blk.zero[b as usize]);
            run.y0 = (1.0 - f) * za.0 + f * zb.0;
            run.d0 = (1.0 - f) * za.1 + f * zb.1;
        }
        let mut live = 0;
        if let Some((e, u, od)) = self.glide(a, [p0[0], p0[1]], [p1[0], p1[1]], il) {
            live |= 1 << e;
            ([run.u[0], run.u[1]], [run.od[0], run.od[1]]) = (u, od);
            let [x, y] = blk.rate[e];
            ([run.rcp[0], run.rcp[1]], [run.ar[0], run.ar[1]]) = ([x.0, y.0], [x.1, y.1]);
        }
        if let Some((e, u, od)) = self.glide(b, [p0[2], p0[3]], [p1[2], p1[3]], il) {
            live |= 1 << e;
            ([run.u[2], run.u[3]], [run.od[2], run.od[3]]) = (u, od);
            let [x, y] = blk.rate[e];
            ([run.rcp[2], run.rcp[3]], [run.ar[2], run.ar[3]]) = ([x.0, y.0], [x.1, y.1]);
        }
        self.live = live;
        run
    }

    /// Wave `w`'s two edges over a run from places `p0` to `p1`: its slot,
    /// the phase since each edge and each place's step a sample, gliding
    /// to its place at the run's end. A wave entering starts on its
    /// places, uncrossed.
    #[inline(always)]
    fn glide(
        &mut self,
        w: Wave,
        p0: [f32; 2],
        p1: [f32; 2],
        il: f32,
    ) -> Option<(usize, [f32; 2], [f32; 2])> {
        let e = w.edge_slot()?;
        // slots are 0..=2: the bound lets the indexing go unchecked
        let e = e.min(2);
        if self.live & (1 << e) == 0 {
            self.since[e] = self.enter(w, p0);
        }
        let (u, p) = (self.since[e], self.phase);
        let step = |u: f32, end: f32| {
            // the edge's place now (a whole number of cycles off); places
            // span under ½ a cycle, so the nearest whole is its own
            let o = p - u;
            (end + floor(o - end + 0.5) - o) * il
        };
        Some((e, u, [step(u[0], p1[0]), step(u[1], p1[1])]))
    }

    /// The phase since each edge for a wave entering at places `pos`: no
    /// crossing.
    #[inline(never)]
    fn enter(&mut self, w: Wave, pos: [f32; 2]) -> [f32; 2] {
        // places are in 0..=1 and the phase in 0..1: the edge is at the
        // place or a cycle before it
        let p = self.phase;
        let u = pos.map(|x| {
            let mut o = if p < x { x - 1.0 } else { x };
            if p - o >= 1.0 {
                o += 1.0;
            }
            p - o
        });
        if w == Wave::Pulse {
            // high where the phase since the rise is the shorter (`high`)
            self.h = if u[0] < u[1] { 1.0 } else { 0.0 };
        }
        u
    }

    /// The run of samples `i0..i1` on the pair `A`, `B`: its setup, then
    /// its per-sample loop.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn run<A: Kind, B: Kind, const N: usize, const REC: bool, const SYNC: bool>(
        &mut self,
        blk: &Blk,
        (i0, i1): (usize, usize),
        r: &Ramps,
        out: &mut [f32; N],
        wr: &mut [f32; N],
        jit: &mut f32,
        draw: &mut impl FnMut() -> f32,
    ) {
        let run = self.setup::<A, B, SYNC>(blk, (i0, i1), r, N);
        let (mut p, mut held, mut h, mut j) = (self.phase, self.held, self.h, *jit);
        let (mut c, mut u, od, rcp, ar) = (run.c, run.u, run.od, run.rcp, run.ar);
        let (mut inc, dinc) = (run.inc, run.dinc);
        // each edge's phase since it moves with the phase and against its
        // place; a wrap moves both, so it leaves `u` alone
        let edged = |k: usize| if k < 2 { A::EDGES } else { B::EDGES };
        // B's constant term rides in A's when both have one; -0.0 in its
        // place adds nothing (`x + -0.0` is `x`)
        let merged = A::BIAS && B::BIAS;
        let bias = |l: Lin| if merged { -0.0 } else { l.v };
        for i in i0..i1.min(N) {
            inc += dinc;
            let ie = inc * j;
            for (k, l) in c.iter_mut().enumerate() {
                if !(merged && k == 3) {
                    l.v += l.d;
                }
            }
            let ca = [c[0], c[1], c[2]];
            let cb = [c[3], c[4], c[5]];
            // -0.0: `y + acc` is `y` itself when nothing crossed
            let mut acc = -0.0f32;
            // a lone `tick` both reads a sync and writes its wrap here
            let t = if SYNC { wr[i] } else { NO_WRAP };
            if REC {
                wr[i] = NO_WRAP;
            }
            let p1 = if SYNC && t >= 0.0 {
                // the reset `t` samples ago, in time order: own edges up to
                // it, the reset, own edges after it (below, as any sample's)
                let dp = ie * (1.0 - t);
                let pre = p + dp;
                for (k, u) in u.iter_mut().enumerate() {
                    if edged(k) {
                        *u += dp - od[k];
                    }
                }
                if A::EDGES {
                    for e in 0..2 {
                        let k = (rcp[e], ar[e]);
                        self.cross::<A>(e, &mut u[e], t, k, ca, &mut h, (&mut held, &mut acc));
                    }
                }
                if B::EDGES {
                    for e in 0..2 {
                        let k = (rcp[e + 2], ar[e + 2]);
                        let ue = &mut u[e + 2];
                        self.cross::<B>(e, ue, t, k, cb, &mut h, (&mut held, &mut acc));
                    }
                }
                // y(0) − y(φ⁻) and the slopes' difference × inc
                let pw = if pre >= 1.0 { pre - 1.0 } else { pre };
                let (va, vb) = (ca.map(|l| l.v), [bias(cb[0]), cb[1].v, cb[2].v]);
                let y = A::value(va, pw, [u[0], u[1]], h) + B::value(vb, pw, [u[2], u[3]], h);
                let d = A::slope(va, pw, [u[0], u[1]]) + B::slope(vb, pw, [u[2], u[3]]);
                let (step, slope) = (run.y0 - y, (run.d0 - d) * ie);
                if self.apply(t, step, slope) {
                    blep_into(&mut held, &mut acc, t, step);
                    blamp_into(&mut held, &mut acc, t, slope);
                }
                if pre >= 1.0 {
                    if REC {
                        wr[i] = clampf(t + (pre - 1.0) * run.rinc, 0.0, 1.0);
                    }
                    j = draw();
                }
                // each edge's phase at 0 is `u − φ⁻` mod 1 (`u` and the
                // wrapped φ⁻ are in 0..1); phase 0 itself is no edge
                let p1 = t * ie;
                let mut v = [0.0f32; 4];
                for (k, u) in u.iter_mut().enumerate() {
                    if !edged(k) {
                        continue;
                    }
                    v[k] = *u - pw;
                    if v[k] < 0.0 {
                        v[k] += 1.0;
                    }
                    *u = v[k] + p1;
                }
                // the pulse at 0: high where the phase since the rise is
                // the shorter (`high`)
                if A::FLIPS {
                    h = if v[0] < v[1] { 1.0 } else { 0.0 };
                }
                if B::FLIPS {
                    h = if v[2] < v[3] { 1.0 } else { 0.0 };
                }
                p1
            } else {
                let mut q = p + ie;
                if q >= 1.0 {
                    q -= 1.0;
                    if REC {
                        wr[i] = (q * run.rinc).min(1.0);
                    }
                    j = draw();
                }
                for (k, u) in u.iter_mut().enumerate() {
                    if edged(k) {
                        *u += ie - od[k];
                    }
                }
                q
            };
            // -0.0: no time offset (`x + -0.0` is `x`)
            if A::EDGES {
                for e in 0..2 {
                    let k = (rcp[e], ar[e]);
                    self.cross::<A>(e, &mut u[e], -0.0, k, ca, &mut h, (&mut held, &mut acc));
                }
            }
            if B::EDGES {
                for e in 0..2 {
                    let k = (rcp[e + 2], ar[e + 2]);
                    let ue = &mut u[e + 2];
                    self.cross::<B>(e, ue, -0.0, k, cb, &mut h, (&mut held, &mut acc));
                }
            }
            let y = A::value([c[0].v, c[1].v, c[2].v], p1, [u[0], u[1]], h)
                + B::value([bias(c[3]), c[4].v, c[5].v], p1, [u[2], u[3]], h);
            out[i] = held;
            held = y + acc;
            p = p1;
        }
        (self.phase, self.held, self.h, *jit) = (p, held, h, j);
        for (base, w) in [(0, A::EDGES), (2, B::EDGES)] {
            if w {
                let e = self.slot_at(base).min(2);
                self.since[e] = [u[base], u[base + 1]];
            }
        }
    }

    /// The edge slot of the live pair's wave at run index `base` (0 or 2).
    fn slot_at(&self, base: usize) -> usize {
        let (a, b) = self.pair.map_or((Wave::Sine, Wave::Sine), Pair::waves);
        let w = if base == 0 { a } else { b };
        w.edge_slot().unwrap_or(0)
    }

    /// Edge `e` of wave `K`, `u` the phase since it (`t0` samples before
    /// the sample): if `u` left 0..1, the edge was crossed, corrected at
    /// `t0 + d₁/r` samples ago, forward or backward (an edge outrunning the
    /// phase).
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn cross<K: Kind>(
        &mut self,
        e: usize,
        u: &mut f32,
        t0: f32,
        (rcp, ar): (f32, f32),
        c: [Lin; 3],
        h: &mut f32,
        (held, acc): (&mut f32, &mut f32),
    ) {
        let bits = u.to_bits();
        // in 0..1 below 1's bits; a negative `u` has the sign bit
        if bits < 1.0f32.to_bits() {
            return;
        }
        if (bits as i32) < 0 {
            let t = clampf(t0 + *u * rcp, 0.0, 1.0);
            *u += 1.0;
            self.correct::<K>(e, false, t, c, ar, h, (held, acc));
        } else {
            *u -= 1.0;
            let t = clampf(t0 + *u * rcp, 0.0, 1.0);
            self.correct::<K>(e, true, t, c, ar, h, (held, acc));
        }
    }

    /// The crossing's correction at `t`, constants lerped back to it.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn correct<K: Kind>(
        &mut self,
        e: usize,
        fwd: bool,
        t: f32,
        c: [Lin; 3],
        ar: f32,
        h: &mut f32,
        (held, acc): (&mut f32, &mut f32),
    ) {
        let jump = K::jump(c, t, e, fwd, *h);
        if K::FLIPS {
            *h = 1.0 - *h;
        }
        if K::CORNERS {
            // `ar` is |r|/6: the slope a sample over 6
            let s6 = jump * ar;
            if self.apply(t, 0.0, 6.0 * s6) {
                blamp6_into(held, acc, t, s6);
            }
        } else if self.apply(t, jump, 0.0) {
            blep_into(held, acc, t, jump);
        }
    }

    /// Whether to correct an edge: always, outside tests.
    #[cfg(not(any(test, feature = "test-support")))]
    #[inline(always)]
    fn apply(&mut self, _: f32, _: f32, _: f32) -> bool {
        true
    }

    /// Logs the edge (a silent one, of a wave at weight 0, is neither
    /// logged nor counted); false when uncorrected or the planted skip.
    #[cfg(any(test, feature = "test-support"))]
    fn apply(&mut self, t: f32, step: f32, slope: f32) -> bool {
        if step == 0.0 && slope == 0.0 {
            return true;
        }
        match self.skip {
            Some(0) => {
                self.skip = None;
                return false;
            }
            Some(n) => self.skip = Some(n - 1),
            None => {}
        }
        if self.logged < EDGE_LOG {
            self.log[self.logged] = (t, step, slope);
            self.logged += 1;
        }
        self.correction == Correction::On
    }

    /// The `(t, step, slope)` corrected in the last `tick`, up to 8.
    #[cfg(any(test, feature = "test-support"))]
    pub fn last_edges(&self) -> &[(f32, f32, f32)] {
        &self.log[..self.logged]
    }

    /// The `n`-th edge from now (0 is the next) goes uncorrected: a
    /// planted defect for the click reference.
    #[cfg(any(test, feature = "test-support"))]
    pub fn skip_edge(&mut self, n: usize) {
        self.skip = Some(n);
    }
}

/// The sub: a square of ±R one octave under OSC 1, flipped at each of its
/// wraps and BLEP-corrected, one sample late like the oscillators.
#[derive(Clone, Copy, Debug)]
pub struct Sub {
    /// The level, R or −R.
    level: f32,
    held: f32,
}

impl Sub {
    pub const NEW: Self = Self { level: R, held: 0.0 };

    /// +R, the corrector cleared.
    pub fn reset(&mut self) {
        *self = Self::NEW;
    }

    /// One sample; `wrap` is OSC 1's wrap `t` samples ago (`NO_WRAP` for
    /// none).
    #[inline(always)]
    pub fn tick(&mut self, wrap: Option<f32>) -> f32 {
        self.step(wrap.map_or(NO_WRAP, |t| clampf(t, 0.0, 1.0)))
    }

    /// `tick` on a wrap buffer's entry.
    #[inline(always)]
    pub fn step(&mut self, wrap: f32) -> f32 {
        let mut out = self.held;
        if wrap >= 0.0 {
            // `blep_into` of the step 2·level: its half is the level
            self.level = -self.level;
            let (h, u) = (self.level, 1.0 - wrap);
            out += h * (wrap * wrap);
            self.held = self.level - h * (u * u);
        } else {
            self.held = self.level;
        }
        out
    }

    pub fn high(&self) -> bool {
        self.level > 0.0
    }

    /// Restores the level (the carry); not an edge.
    pub fn set_high(&mut self, h: bool) {
        self.level = if h { R } else { -R };
    }
}
