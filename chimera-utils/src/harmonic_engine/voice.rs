//! One HARMONIC voice: the control frame (pure, once a 64-sample block) and
//! the partial bank (per sample).
//!
//! Control order: X/Y (+ PATH, ENV → X, VARIETY) → MAP → DENSITY → the
//! four slots' masks → SUB → TAPER → LEVEL → PLUCK → AIR. LEVEL sits
//! before PLUCK: normalising after it would undo the decay.
//!
//! The bank: one Vicanek quadrature oscillator a lane (3 multiply-adds a
//! sample: `w = u − k₁v; v += k₂w; u = w − k₁v`, k₁ = tan(ω/2),
//! k₂ = 2k₁/(1+k₁²); exact sine and cosine, no renormalisation), the
//! amplitude ramped linearly across the block, the loop partial-outer,
//! sample-inner. Lanes 0 and 1 are SUB (½ and ¼ of the pitch), then the
//! partials. Start phases are Schroeder's (πk(k−1)/N): a low crest factor,
//! so the RMS level holds a predictable peak.

use super::map::Map;
use super::modules::{Module, ModuleKind, stretch_ratio};
use super::{MAX, Partials};
use crate::drums::parts::{B, Blk, FS};

pub const SLOTS: usize = 4;
const SUB: usize = 2;
pub const LANES: usize = MAX + SUB;
/// Every voice's RMS at sustain: VA's oscillator RMS.
pub const RMS: f32 = chimera_core::dsp::va::osc::R;
/// No partial sounds at or above this share of fs.
pub const TOP: f32 = 0.45;
/// DENSITY's soft knee under the threshold, dB.
const KNEE: f32 = 6.0;
/// DENSITY's range: the threshold sits `RANGE·d²` dB under the peak.
const RANGE: f32 = 96.0;
/// VARIETY at 1 offsets each varied control by up to ±this.
const VARY_SPREAD: f32 = 0.25;
/// AIR's noise streams: low-passed at this many Hz.
const AIR_LP: f32 = 300.0;
/// AIR's weight is f / (f + this): partials above a few kHz get it all.
const AIR_KNEE_HZ: f32 = 2500.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathShape {
    Off,
    Line,
    Circle,
    Figure8,
    Zigzag,
}

impl PathShape {
    pub fn name(self) -> &'static str {
        match self {
            PathShape::Off => "OFF",
            PathShape::Line => "LINE",
            PathShape::Circle => "CIRCLE",
            PathShape::Figure8 => "FIGURE-8",
            PathShape::Zigzag => "ZIGZAG",
        }
    }
}

/// Seconds a bar at the renders' 120 BPM.
pub const BAR: f32 = 2.0;

/// PATH: a shape on the map, synced (one lap every `bars`), `size` its
/// reach from X/Y. Its phase is the song's, not the note's.
#[derive(Clone, Copy, Debug)]
pub struct PathCtl {
    pub shape: PathShape,
    pub bars: f32,
    pub size: f32,
}

impl PathCtl {
    pub const OFF: Self = Self {
        shape: PathShape::Off,
        bars: 1.0,
        size: 0.0,
    };

    pub fn offset(&self, clock: f32) -> (f32, f32) {
        let ph = clock / (self.bars * BAR);
        let tri = |p: f32| 1.0 - 4.0 * (p.rem_euclid(1.0) - 0.5).abs();
        let tau = core::f32::consts::TAU;
        let (x, y) = match self.shape {
            PathShape::Off => (0.0, 0.0),
            PathShape::Line => (tri(ph), 0.0),
            PathShape::Circle => ((tau * ph).cos(), (tau * ph).sin()),
            PathShape::Figure8 => ((tau * ph).sin(), (2.0 * tau * ph).sin()),
            PathShape::Zigzag => (tri(ph), tri(3.0 * ph)),
        };
        (self.size * x, self.size * y)
    }
}

/// ENV → X: a decay from the note-on, `amount` (−1..1) of X.
#[derive(Clone, Copy, Debug)]
pub struct EnvX {
    pub amount: f32,
    pub decay: f32,
}

/// The amp envelope standing in for the voice chain: linear attack,
/// exponential decay to sustain and release (time constants in s).
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
}

impl Adsr {
    pub const fn new(a: f32, d: f32, s: f32, r: f32) -> Self {
        Self { a, d, s, r }
    }
}

/// A Sound: every control a HARMONIC Part stores. Knobs are 0..1.
#[derive(Clone, Copy, Debug)]
pub struct Sound {
    pub x: f32,
    pub y: f32,
    pub density: f32,
    pub air: f32,
    pub sub: f32,
    pub variety: f32,
    pub chain: [ModuleKind; SLOTS],
    /// The module knobs at the four corners, [x0y0, x1y0, x0y1, x1y1]:
    /// all four equal is a plain knob.
    pub corners: [[[f32; 3]; SLOTS]; 4],
    pub path: PathCtl,
    pub env_x: EnvX,
    pub amp: Adsr,
}

impl Sound {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            density: 1.0,
            air: 0.0,
            sub: 0.0,
            variety: 0.0,
            chain: [ModuleKind::None; SLOTS],
            corners: [[[0.5; 3]; SLOTS]; 4],
            path: PathCtl::OFF,
            env_x: EnvX {
                amount: 0.0,
                decay: 0.3,
            },
            amp: Adsr::new(0.005, 0.3, 0.8, 0.3),
        }
    }

    /// Slot `i` holds `kind` at knobs `k` at every corner.
    pub fn slot(mut self, i: usize, kind: ModuleKind, k: [f32; 3]) -> Self {
        self.chain[i] = kind;
        for c in &mut self.corners {
            c[i] = k;
        }
        self
    }

    /// Corner `c`'s knobs for slot `i`.
    pub fn corner(mut self, c: usize, i: usize, k: [f32; 3]) -> Self {
        self.corners[c][i] = k;
        self
    }

    /// Sets one knob at every corner.
    pub fn set_knob(&mut self, slot: usize, knob: usize, v: f32) {
        for c in &mut self.corners {
            c[slot][knob] = v;
        }
    }

    pub fn with(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }

    /// The module knobs at (x, y): the corners mixed bilinearly.
    pub fn knobs(&self, x: f32, y: f32) -> [[f32; 3]; SLOTS] {
        let w = [(1.0 - x) * (1.0 - y), x * (1.0 - y), (1.0 - x) * y, x * y];
        core::array::from_fn(|i| {
            core::array::from_fn(|j| (0..4).map(|c| w[c] * self.corners[c][i][j]).sum())
        })
    }
}

/// VARIETY's per-note offsets, latched at the note-on. Never on pitch:
/// STRETCH's knobs and the note are not in the set.
#[derive(Clone, Copy, Debug, Default)]
pub struct Vary {
    pub x: f32,
    pub y: f32,
    pub density: f32,
    pub air: f32,
    pub knobs: [[f32; 3]; SLOTS],
}

/// xorshift32: the noise and VARIETY's dice.
fn rnd(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s as f32 / u32::MAX as f32) * 2.0 - 1.0
}

impl Vary {
    pub fn latch(seed: u32, variety: f32) -> Self {
        let mut r = seed.wrapping_mul(0x9E37_79B9) | 1;
        let sp = variety.clamp(0.0, 1.0) * VARY_SPREAD;
        let mut d = || sp * rnd(&mut r);
        Self {
            x: d(),
            y: d(),
            density: d(),
            air: d(),
            knobs: core::array::from_fn(|_| [d(), d(), d()]),
        }
    }
}

/// A block's targets for every lane.
pub struct Frame {
    pub hz: [f32; LANES],
    /// after the taper and LEVEL, before PLUCK
    pub amp: [f32; LANES],
    /// PLUCK's decay rate, 1 / T60
    pub rate: [f32; LANES],
    /// AIR's modulation depth
    pub air: [f32; LANES],
    /// AIR's band above the top partial: RMS and lower edge (Hz)
    pub band: f32,
    pub band_lo: f32,
}

/// Plaits' (1 − 2f) taper, forced to silence at `TOP`·fs.
pub fn taper(hz: f32) -> f32 {
    let f = hz / FS;
    (1.0 - 2.0 * f).max(0.0) * ((TOP - f) / 0.05).clamp(0.0, 1.0)
}

/// The control frame of a voice at `f0`: pure.
pub fn frame(n: Partials, s: &Sound, f0: f32, vary: &Vary, clock: f32, since_on: f32) -> Frame {
    let n = n.get();
    let mut f = Frame {
        hz: [0.0; LANES],
        amp: [0.0; LANES],
        rate: [0.0; LANES],
        air: [0.0; LANES],
        band: 0.0,
        band_lo: 0.0,
    };
    let (dx, dy) = s.path.offset(clock);
    let ex = s.env_x.amount * (-since_on / s.env_x.decay.max(1e-3)).exp();
    let x = (s.x + dx + ex + vary.x).clamp(0.0, 1.0);
    let y = (s.y + dy + vary.y).clamp(0.0, 1.0);
    let knobs = s.knobs(x, y);
    // STRETCH reads its corners where X/Y would be without VARIETY: VARIETY
    // never reaches pitch, not even through the corners
    let plain = s.knobs((s.x + dx + ex).clamp(0.0, 1.0), (s.y + dy).clamp(0.0, 1.0));
    let mods: [Module; SLOTS] = core::array::from_fn(|i| {
        let kind = s.chain[i];
        let k = if kind == ModuleKind::Stretch {
            plain[i]
        } else {
            core::array::from_fn(|j| knobs[i][j] + vary.knobs[i][j])
        };
        Module::from_cells(kind, k)
    });

    // frequencies: harmonic, or STRETCH's law (B's add), kept rising
    let b: f32 = mods.iter().map(|m| m.stretch_b(f0)).sum();
    f.hz[0] = 0.5 * f0;
    f.hz[1] = 0.25 * f0;
    for i in 0..n {
        let mut hz = f0 * stretch_ratio(i + 1, b);
        if i > 0 {
            hz = hz.max(f.hz[SUB + i - 1] + 0.05 * f0);
        }
        f.hz[SUB + i] = hz;
    }

    // map → DENSITY: the threshold under the strongest audible partial
    let mut db = [f32::NEG_INFINITY; MAX];
    Map::get().mix(x, y, n, &mut db);
    let audible = |i: usize| f.hz[SUB + i] < TOP * FS;
    let peak = (0..n)
        .filter(|&i| audible(i))
        .fold(f32::NEG_INFINITY, |m, i| m.max(db[i]));
    if peak == f32::NEG_INFINITY {
        return f;
    }
    let d = (s.density + vary.density).clamp(0.0, 1.0);
    let thr = peak - RANGE * d * d;
    let mut amax = 0.0f32;
    for i in 0..n {
        let l = db[i];
        if l == f32::NEG_INFINITY {
            continue;
        }
        let k = ((l - (thr - KNEE)) / KNEE).clamp(0.0, 1.0);
        let hz = f.hz[SUB + i];
        let mut a = 10f32.powf(l / 20.0) * k * k * (3.0 - 2.0 * k);
        for m in &mods {
            a *= m.gain(i + 1, hz, f0);
            f.rate[SUB + i] += m.decay_rate(hz / f0);
        }
        f.amp[SUB + i] = a;
        amax = amax.max(a);
    }

    // SUB: ½ and ¼ of the pitch, against the strongest partial
    let sub = s.sub.clamp(0.0, 1.0);
    f.amp[0] = sub * amax;
    f.amp[1] = 0.5 * sub * amax;
    for l in 0..SUB {
        f.rate[l] = mods.iter().map(|m| m.decay_rate(f.hz[l] / f0)).sum();
    }

    // TAPER, then LEVEL: the RMS held whatever the map, DENSITY or slots
    let mut ss = 0.0f32;
    for l in 0..SUB + n {
        f.amp[l] *= taper(f.hz[l]);
        ss += f.amp[l] * f.amp[l];
    }
    if ss < 1e-20 {
        f.amp = [0.0; LANES];
        return f;
    }
    let g = RMS / (0.5 * ss).sqrt();
    for a in &mut f.amp[..SUB + n] {
        *a *= g;
    }

    // AIR: noise on each partial, more on high ones, and on low notes a
    // band above the last partial at the top partials' level
    let air = (s.air + vary.air).clamp(0.0, 1.0);
    if air > 0.0 {
        for l in 0..SUB + n {
            f.air[l] = air * f.hz[l] / (f.hz[l] + AIR_KNEE_HZ);
        }
        if let Some(top) = (SUB..SUB + n).rev().find(|&l| f.amp[l] > 0.0) {
            let lo = f.hz[top];
            if lo < 0.3 * FS {
                let hi = (4.0 * lo).min(0.4 * FS);
                let tail = &f.amp[top.saturating_sub(3).max(SUB)..=top];
                let a = (tail.iter().map(|a| a * a).sum::<f32>() / tail.len() as f32).sqrt();
                let slots = ((hi - lo) / f0).min(64.0);
                f.band = air * a * slots.sqrt() * 0.5;
                f.band_lo = lo;
            }
        }
    }
    f
}

pub struct Voice {
    n: Partials,
    f0: f32,
    vary: Vary,
    u: [f32; LANES],
    v: [f32; LANES],
    k1: [f32; LANES],
    k2: [f32; LANES],
    hz: [f32; LANES],
    amp: [f32; LANES],
    pluck: [f32; LANES],
    /// samples since the note-on
    on: usize,
    gate: bool,
    env: f32,
    rng: u32,
    /// AIR's four streams: two one-pole low-pass states each
    lp: [[f32; 2]; 4],
    /// the band: high-pass and low-pass states, its last RMS
    band: [f32; 2],
    band_lvl: f32,
}

impl Voice {
    pub fn new(n: Partials, f0: f32, seed: u32, s: &Sound) -> Self {
        let np = n.get() as f32;
        let mut u = [1.0; LANES];
        let mut v = [0.0; LANES];
        for l in SUB..LANES {
            let k = (l - SUB + 1) as f32;
            let ph = core::f32::consts::PI * k * (k - 1.0) / np;
            (u[l], v[l]) = (ph.cos(), ph.sin());
        }
        Self {
            n,
            f0,
            vary: Vary::latch(seed, s.variety),
            u,
            v,
            k1: [0.0; LANES],
            k2: [0.0; LANES],
            hz: [0.0; LANES],
            amp: [0.0; LANES],
            pluck: [1.0; LANES],
            on: 0,
            gate: true,
            env: 0.0,
            rng: seed.wrapping_mul(0x2545_F491) | 1,
            lp: [[0.0; 2]; 4],
            band: [0.0; 2],
            band_lvl: 0.0,
        }
    }

    pub fn release(&mut self) {
        self.gate = false;
    }

    /// `N` lanes into `acc`, ramping each to `to`; with AIR, lane j's
    /// weighted output also into noise stream `accn[j]`.
    fn bank<const N: usize>(
        &mut self,
        ids: [usize; N],
        to: &[f32; LANES],
        air: Option<&[f32; LANES]>,
        acc: &mut Blk,
        accn: &mut [Blk],
    ) {
        let (mut u, mut v) = (ids.map(|l| self.u[l]), ids.map(|l| self.v[l]));
        let (k1, k2) = (ids.map(|l| self.k1[l]), ids.map(|l| self.k2[l]));
        let mut a = ids.map(|l| self.amp[l]);
        let da = ids.map(|l| (to[l] - self.amp[l]) / B as f32);
        match air {
            None => {
                for y in acc.iter_mut() {
                    let mut sum = 0.0;
                    for j in 0..N {
                        a[j] += da[j];
                        let t = u[j] - k1[j] * v[j];
                        v[j] += k2[j] * t;
                        u[j] = t - k1[j] * v[j];
                        sum += a[j] * v[j];
                    }
                    *y += sum;
                }
            }
            Some(w) => {
                let w = ids.map(|l| w[l]);
                for (s, y) in acc.iter_mut().enumerate() {
                    let mut sum = 0.0;
                    for j in 0..N {
                        a[j] += da[j];
                        let t = u[j] - k1[j] * v[j];
                        v[j] += k2[j] * t;
                        u[j] = t - k1[j] * v[j];
                        let p = a[j] * v[j];
                        sum += p;
                        accn[j][s] += w[j] * p;
                    }
                    *y += sum;
                }
            }
        }
        for (j, &l) in ids.iter().enumerate() {
            (self.u[l], self.v[l]) = (u[j], v[j]);
        }
    }

    /// One block at song time `clock` (s) into `out` (overwritten);
    /// false once released and silent.
    pub fn block(&mut self, out: &mut Blk, s: &Sound, clock: f32) -> bool {
        let fr = frame(self.n, s, self.f0, &self.vary, clock, self.on as f32 / FS);
        let dt = B as f32 / FS;
        let mut to = [0.0f32; LANES];
        for l in 0..LANES {
            if fr.hz[l] != self.hz[l] {
                self.hz[l] = fr.hz[l];
                let k1 = (core::f32::consts::PI * (fr.hz[l] / FS).min(0.49)).tan();
                self.k1[l] = k1;
                self.k2[l] = 2.0 * k1 / (1.0 + k1 * k1);
            }
            if fr.rate[l] > 0.0 {
                // −60 dB in 1/rate seconds
                self.pluck[l] *= (-6.9078 * fr.rate[l] * dt).exp();
            }
            to[l] = fr.amp[l] * self.pluck[l];
        }

        let mut acc: Blk = [0.0; B];
        let mut accn = [[0.0f32; B]; 4];
        let air_on = fr.air.iter().any(|&w| w > 0.0);
        // live lanes, four at a time: four independent recurrences per
        // inner loop hide the FPU's latency (Plaits batches for the same)
        let mut ids = [0usize; LANES];
        let mut live = 0;
        for l in 0..LANES {
            if self.amp[l] != 0.0 || to[l] != 0.0 {
                ids[live] = l;
                live += 1;
            }
        }
        let air = air_on.then_some(&fr.air);
        let mut i = 0;
        while i + 4 <= live {
            let g: [usize; 4] = core::array::from_fn(|j| ids[i + j]);
            self.bank(g, &to, air, &mut acc, &mut accn);
            i += 4;
        }
        while i < live {
            self.bank([ids[i]], &to, air, &mut acc, &mut accn[i & 3..]);
            i += 1;
        }
        self.amp = to;

        if air_on {
            let c = 1.0 - (-core::f32::consts::TAU * AIR_LP / FS).exp();
            // two one-poles on uniform noise: RMS √(1/3)·√(c/4), scaled to 1
            let norm = 1.0 / ((1.0f32 / 3.0).sqrt() * (c / 4.0).sqrt());
            for (j, n) in accn.iter().enumerate() {
                let st = &mut self.lp[j];
                for (y, n) in acc.iter_mut().zip(n) {
                    st[0] += c * (rnd(&mut self.rng) - st[0]);
                    st[1] += c * (st[0] - st[1]);
                    *y += norm * st[1] * n;
                }
            }
        }
        if fr.band > 0.0 || self.band_lvl > 0.0 {
            let lo = fr.band_lo.max(20.0);
            let hi = (4.0 * lo).min(0.4 * FS);
            let ch = 1.0 - (-core::f32::consts::TAU * lo / FS).exp();
            let cl = 1.0 - (-core::f32::consts::TAU * hi / FS).exp();
            let bw = ((hi - lo) / (0.5 * FS)).max(0.01);
            let norm = 1.0 / ((1.0f32 / 3.0).sqrt() * bw.sqrt());
            let (l0, l1) = (self.band_lvl, fr.band);
            for (i, y) in acc.iter_mut().enumerate() {
                let x = rnd(&mut self.rng);
                self.band[0] += ch * (x - self.band[0]);
                self.band[1] += cl * ((x - self.band[0]) - self.band[1]);
                let lv = l0 + (l1 - l0) * (i + 1) as f32 / B as f32;
                *y += lv * norm * self.band[1];
            }
            self.band_lvl = l1;
        }

        let e = s.amp;
        let (dk, rk) = ((-1.0 / (e.d * FS)).exp(), (-1.0 / (e.r * FS)).exp());
        for (o, y) in out.iter_mut().zip(acc) {
            let t = self.on as f32 / FS;
            self.env = if !self.gate {
                self.env * rk
            } else if t < e.a {
                t / e.a
            } else {
                e.s + (self.env.max(e.s) - e.s) * dk
            };
            *o = y * self.env;
            self.on += 1;
        }
        self.gate || self.env > 1e-4
    }
}
