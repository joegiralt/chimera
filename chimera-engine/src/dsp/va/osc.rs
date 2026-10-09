//! The variable-wave oscillator (spec § Oscillator). WAVE morphs sine →
//! triangle → saw → pulse, a linear crossfade of two neighbours; SHAPE
//! varies whichever two are sounding. Every wave, at every SHAPE, is
//! DC-free at one RMS `R`, with its fundamental at `sin 2πφ`, so a
//! crossfade never cancels it. Steps and corners, fixed or moving, are
//! found every sample and corrected by the 2-point kernels (`blep`). The
//! precedent is Ambika's variable-waveform oscillator, architecture only.

use super::blep::Corrector;
use crate::dsp::provenance::{PatentLine, Provenance};
use crate::dsp::{clampf, fast_sin_turns};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Textbook {
        cite: "phase accumulator",
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
const INV_TAU: f32 = 1.0 / core::f32::consts::TAU;

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
    /// The first of the two live waves and the second's weight.
    #[inline(always)]
    fn segment(self) -> (usize, f32) {
        let x3 = 3.0 * self.wave.get();
        let k = (x3 as usize).min(2);
        (k, x3 - k as f32)
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
    const ALL: [Wave; 4] = [Wave::Sine, Wave::Tri, Wave::Saw, Wave::Pulse];

    /// The slot of a wave with edges (the sine has none).
    fn edge_slot(self) -> Option<usize> {
        match self {
            Wave::Sine => None,
            Wave::Tri => Some(0),
            Wave::Saw => Some(1),
            Wave::Pulse => Some(2),
        }
    }
}

/// A step or a corner at phase `pos`: crossing it forward, the output
/// jumps by `step` and its slope by `slope` per cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Edge {
    pos: f32,
    step: f32,
    slope: f32,
}

impl Edge {
    const NONE: Self = Self {
        pos: 0.0,
        step: 0.0,
        slope: 0.0,
    };

    fn weighted(self, g: f32) -> Self {
        Self {
            pos: self.pos,
            step: self.step * g,
            slope: self.slope * g,
        }
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
                self.saw_g = R / libm::sqrtf(ms);
            }
            Wave::Pulse => {
                let w = (0.5 - 0.45 * s).max(inc);
                self.pulse_w = w;
                self.pulse_g = R / (2.0 * libm::sqrtf(w * (1.0 - w)));
            }
        }
    }

    /// The value at phase `p` (0..1) and its slope per cycle.
    #[inline(always)]
    fn eval(&self, wave: Wave, p: f32) -> (f32, f32) {
        match wave {
            Wave::Sine => {
                let (s, g) = (self.fold, 1.0 + 2.0 * self.fold);
                let st = sin_q(p);
                let arg = g * st * INV_TAU;
                let y = (1.0 - s) * st + s * sin_q(arg);
                let d = core::f32::consts::TAU
                    * sin_q(p + 0.25)
                    * ((1.0 - s) + s * g * sin_q(arg + 0.25));
                (self.sine_g * y, self.sine_g * d)
            }
            Wave::Tri => {
                let w = self.tri_w;
                let (up, down) = (self.tri_j * (1.0 - w), self.tri_j * w);
                let mut u = p + 0.5 * w;
                if u >= 1.0 {
                    u -= 1.0;
                }
                if u < w {
                    (-TRI_PEAK + u * up, up)
                } else {
                    (TRI_PEAK - (u - w) * down, -down)
                }
            }
            Wave::Saw => {
                let mut u1 = p - self.saw_e1;
                if u1 >= 1.0 {
                    u1 -= 1.0;
                }
                let mut u2 = p - self.saw_e2;
                if u2 < 0.0 {
                    u2 += 1.0;
                }
                let y = self.saw_a * (1.0 - 2.0 * u1) + self.saw_b * (1.0 - 2.0 * u2);
                (self.saw_g * y, -2.0 * self.saw_g)
            }
            Wave::Pulse => {
                let w = self.pulse_w;
                let mut u = p - (0.25 - 0.5 * w);
                if u < 0.0 {
                    u += 1.0;
                }
                let y = if u < w { 1.0 - w } else { -w };
                (2.0 * self.pulse_g * y, 0.0)
            }
        }
    }

    /// The wave's two edges (the sine has none).
    #[inline(always)]
    fn edges(&self, wave: Wave) -> [Edge; 2] {
        let e = |pos, step, slope| Edge { pos, step, slope };
        match wave {
            Wave::Sine => [Edge::NONE; 2],
            Wave::Tri => {
                let h = 0.5 * self.tri_w;
                [e(-h, 0.0, self.tri_j), e(h, 0.0, -self.tri_j)]
            }
            Wave::Saw => [
                e(self.saw_e1, 2.0 * self.saw_a * self.saw_g, 0.0),
                e(self.saw_e2, 2.0 * self.saw_b * self.saw_g, 0.0),
            ],
            Wave::Pulse => {
                let h = 0.5 * self.pulse_w;
                let s = 2.0 * self.pulse_g;
                [e(0.25 - h, s, 0.0), e(0.25 + h, -s, 0.0)]
            }
        }
    }

    fn peak(&self, wave: Wave) -> f32 {
        match wave {
            Wave::Sine => self.sine_g,
            Wave::Tri => TRI_PEAK,
            Wave::Saw => self.saw_g,
            Wave::Pulse => 2.0 * (1.0 - self.pulse_w) * self.pulse_g,
        }
    }

    /// The constants of `m`'s two live waves.
    fn of(m: Morph, inc: f32) -> (Self, Wave, Wave, f32) {
        let (k, f) = m.segment();
        let (a, b) = (Wave::ALL[k], Wave::ALL[k + 1]);
        let mut sh = Self::ZERO;
        sh.fill(a, m.shape.get(), inc);
        sh.fill(b, m.shape.get(), inc);
        (sh, a, b, f)
    }
}

/// The naive value at `phase` (0..1) and its slope per cycle: the morph's
/// two waves, weighted, the pulse never narrower than `inc`. The reference
/// for tests and for sync.
pub fn wave_at(phase: f32, m: Morph, inc: f32) -> (f32, f32) {
    let (sh, a, b, f) = Shaped::of(m, inc);
    let ((ya, da), (yb, db)) = (sh.eval(a, phase), sh.eval(b, phase));
    (ya + (yb - ya) * f, da + (db - da) * f)
}

/// A bound on |`wave_at`|: each live wave's peak, weighted.
pub fn peak_bound(m: Morph, inc: f32) -> f32 {
    let (sh, a, b, f) = Shaped::of(m, inc);
    (1.0 - f) * sh.peak(a) + f * sh.peak(b)
}

/// Whether edges are corrected; `Off` is the alias gate's baseline.
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

/// `fast_sin_turns` folded onto the quarter turn about 0, where its table
/// is exact (it errs up to 4.6e-4 near half a turn).
#[inline(always)]
fn sin_q(t: f32) -> f32 {
    let t = t - floor(t + 0.5) as f32;
    let t = if t > 0.25 {
        0.5 - t
    } else if t < -0.25 {
        -0.5 - t
    } else {
        t
    };
    fast_sin_turns(t)
}

/// ⌊x⌋ for |x| < 2³¹.
#[inline(always)]
fn floor(x: f32) -> i32 {
    let i = x as i32;
    if (i as f32) > x { i - 1 } else { i }
}

#[cfg(any(test, feature = "test-support"))]
const EDGE_LOG: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct VarOsc {
    phase: f32,
    inv_sr: f32,
    correction: Correction,
    corrector: Corrector,
    shaped: Shaped,
    /// What `shaped` was computed for: SHAPE, the segment, the increment.
    key: (f32, usize, f32),
    /// Each edged wave's edges last sample, and which waves were live.
    last: [[Edge; 2]; 3],
    live: u8,
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
        correction: Correction::On,
        corrector: Corrector::NEW,
        shaped: Shaped::ZERO,
        key: (f32::NAN, 0, f32::NAN),
        last: [[Edge::NONE; 2]; 3],
        live: 0,
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

    /// Phase 0, the corrector cleared, the edges re-anchored.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.corrector.clear();
        self.live = 0;
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Jumps the phase (wrapped into 0..1; NaN is 0). The jump itself is
    /// not an edge.
    pub fn set_phase(&mut self, p: f32) {
        let f = p - libm::floorf(p);
        self.phase = if (0.0..1.0).contains(&f) { f } else { 0.0 };
    }

    pub fn with_correction(mut self, c: Correction) -> Self {
        self.correction = c;
        self
    }

    /// Advances one sample at `inc` cycles a sample. `moving` says SHAPE or
    /// a floored pulse's `inc` moves: the constants are then exact every
    /// sample (a still oscillator also refreshes them when its SHAPE,
    /// segment or floored `inc` changed). `sync` is a master's wrap `t`
    /// samples ago; not applied yet.
    #[inline(always)]
    pub fn tick(&mut self, inc: f32, m: Morph, moving: bool, sync: Option<f32>) -> Tick {
        let _ = sync;
        let (k, f) = m.segment();
        let s = m.shape.get();
        let (ks, kk, ki) = self.key;
        let floor_moves = k == 2 && inc != ki && 0.5 - 0.45 * s < inc.max(ki);
        if moving || s != ks || k != kk || floor_moves {
            self.shaped.fill(Wave::ALL[k], s, inc);
            self.shaped.fill(Wave::ALL[k + 1], s, inc);
            self.key = (s, k, inc);
        }
        let live = [(Wave::ALL[k], 1.0 - f), (Wave::ALL[k + 1], f)];

        #[cfg(any(test, feature = "test-support"))]
        {
            self.logged = 0;
        }
        let p0 = self.phase;
        let mut p1 = p0 + inc;
        let wrapped = p1 >= 1.0;
        if wrapped {
            p1 -= 1.0;
        }
        if self.correction == Correction::On {
            let mut now = 0;
            for (wave, g) in live {
                let Some(i) = wave.edge_slot() else { continue };
                let cur = self.shaped.edges(wave).map(|e| e.weighted(g));
                let last = if self.live & (1 << i) != 0 {
                    self.last[i]
                } else {
                    cur
                };
                for (a, b) in last.into_iter().zip(cur) {
                    self.cross(p0, p1, wrapped, a, b);
                }
                self.last[i] = cur;
                now |= 1 << i;
            }
            self.live = now;
        }
        self.phase = p1;

        let (ya, _) = self.shaped.eval(live[0].0, p1);
        let (yb, _) = self.shaped.eval(live[1].0, p1);
        Tick {
            out: self.corrector.push(ya + (yb - ya) * f),
            wrap: wrapped.then(|| p1 / inc),
        }
    }

    /// An edge moving from `a` to `b` while the phase moves from `p0` to
    /// `p1` (`wrapped` past 1): corrected if they crossed, either way.
    #[inline(always)]
    fn cross(&mut self, p0: f32, p1: f32, wrapped: bool, a: Edge, b: Edge) {
        let w = wrapped as i32;
        // the next sample's d0 is bit for bit this d1, so no crossing is
        // seen twice or missed
        let (d0, d1) = (p0 - a.pos, p1 - b.pos);
        let (f0, f1) = (floor(d0), floor(d1) + w);
        if f0 == f1 {
            return;
        }
        let m = f0.max(f1);
        let (e0, e1) = (d0 - m as f32, d1 - (m - w) as f32);
        let r = e1 - e0;
        let t = clampf(e1 / r, 0.0, 1.0);
        let step = (b.step + (a.step - b.step) * t) * r.signum();
        let slope = (b.slope + (a.slope - b.slope) * t) * r.abs();
        if step != 0.0 || slope != 0.0 {
            self.edge(t, step, slope);
        }
    }

    #[inline(always)]
    fn edge(&mut self, t: f32, step: f32, slope: f32) {
        #[cfg(any(test, feature = "test-support"))]
        {
            match self.skip {
                Some(0) => {
                    self.skip = None;
                    return;
                }
                Some(n) => self.skip = Some(n - 1),
                None => {}
            }
            if self.logged < EDGE_LOG {
                self.log[self.logged] = (t, step, slope);
                self.logged += 1;
            }
        }
        self.corrector.edge(t, step, slope);
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
