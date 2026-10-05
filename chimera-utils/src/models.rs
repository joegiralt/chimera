//! Design sketches. A new reverb structure (or any DSP idea measured
//! against a reference) is prototyped here, in Rust, against the same
//! metrics as the real devices.
//!
//! BURST v2 (ADR 0095 § GATE and REVERSE): a float model of `rev/burst.rs`
//! at the 24 kHz ring, with the options under trial: PRE-DLY, the diffuser
//! after the taps, more allpasses, TAIL's damper, the end hit, ONSET, the
//! dense ramp. `model-fit VARIANT PROG …` fits a variant's knobs.

use crate::Config;
use crate::metrics::{Metrics, distance, ir_metrics, noise_band_rt60, residuals, verdict};
use crate::reference::read_wav;
use crate::sig::{ERFC_INV_SQRT2, box_same, hanning, resample_poly, welch};
use crate::SR;
use crate::stim::{CLICK_AT, click, lhs, noise_stim};
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

const FS: f64 = 24_000.0;
/// The bus's half-band latency, 48 kHz samples (matched to the Rust render).
const LATENCY: usize = 34;
const BURST_MAX: usize = 19_200;
const DEC_G: f64 = 0.6;
const TAIL_G: f64 = 0.5;
const MAX_GAIN: f64 = 0.985;
const TAP_SEED: u32 = 0x5EED_0343;
/// DENSITY fades these taps in, in this order.
const FADE: [usize; 6] = [5, 6, 3, 8, 1, 10];

fn is_prime(n: u32) -> bool {
    n >= 2
        && (2..)
            .take_while(|i| i * i <= n)
            .all(|i| !n.is_multiple_of(i))
}

/// `parts::prime_ms`.
pub fn prime_ms(ms: f64) -> usize {
    let want = (ms as f32 * 24.0 + 0.5) as u32;
    let mut d = 0;
    loop {
        if is_prime(want - d) {
            return (want - d) as usize;
        }
        if is_prime(want + d) {
            return (want + d) as usize;
        }
        d += 1;
    }
}

const fn lcg(x: u32) -> u32 {
    x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// `n` taps' jitter (±`jit`, 0 at both ends) and sign, two LCG draws each.
/// `f32j` rounds the jitter to f32 as `burst::tap_table` does.
fn tap_table(n: usize, jit: f64, f32j: bool) -> Vec<(f64, f64)> {
    let mut x = TAP_SEED;
    (0..n)
        .map(|k| {
            x = lcg(x);
            let mut j = if k == 0 || k == n - 1 {
                0.0
            } else {
                ((x >> 8) as f64 / 16_777_216.0 * 2.0 - 1.0) * jit
            };
            if f32j {
                j = j as f32 as f64;
            }
            x = lcg(x);
            (j, if x >> 31 == 0 { 1.0 } else { -1.0 })
        })
        .collect()
}

/// A delay line read `d` back (d ≤ len).
struct Line {
    b: Vec<f64>,
    h: usize,
}

impl Line {
    fn new(n: usize) -> Self {
        Self {
            b: vec![0.0; n + 1],
            h: 0,
        }
    }
    fn at(&self, d: usize) -> f64 {
        let n = self.b.len();
        self.b[(self.h + n - d) % n]
    }
    fn push(&mut self, x: f64) {
        self.h = (self.h + 1) % self.b.len();
        self.b[self.h] = x;
    }
}

/// An allpass of `m` samples: `v = x − g z`, `y = z + g v`.
struct Ap {
    l: Line,
    m: usize,
}

impl Ap {
    fn new(m: usize) -> Self {
        Self { l: Line::new(m), m }
    }
    fn run(&mut self, x: f64, g: f64) -> f64 {
        let z = self.l.at(self.m - 1);
        let v = x - g * z;
        self.l.push(v);
        z + g * v
    }
}

fn one_pole(hz: f64) -> f64 {
    1.0 - (-2.0 * PI * hz / FS).exp()
}

/// What the model plays (EngineKnobs-like, plus the trial options).
#[derive(Clone, Debug)]
pub struct Ek {
    pub length_ms: f64,
    pub rt60_s: f64,
    pub tail: f64,
    pub regen: f64,
    pub tone_hz: f64,
    pub taps: f64,
    pub g: f64,
    pub pre_ms: f64,
    pub shape_db: f64,
    pub rising: bool,
    /// The allpasses' lengths after the input (v1) or after the sum (v2).
    pub diff_ms: Vec<f64>,
    /// TAIL's damper, Hz (0: none).
    pub tdamp_hz: f64,
    /// v2: the taps read the TONE'd send; the diffusers follow the sum.
    pub post: bool,
    /// v2 REVERSE: the ramp's tap count (0: the 12-tap DENSITY curve).
    pub n: usize,
    pub hit_db: f64,
    pub onset_ms: f64,
    /// Ramp taps as equal-weight steps (the unit's groups): 0 = a smooth curve.
    pub steps: usize,
    /// A ramp allpass chain after the taps, before the shared diffusers
    /// (the dense ramp's cheap stand-in), ms, at `ramp_g`.
    pub ramp_ms: Vec<f64>,
    pub ramp_g: f64,
    /// The post diffuser's share against the dry tap sum (1: all diffused).
    pub wet: f64,
    /// The ramp's curve exponent (1: straight in dB).
    pub gamma: f64,
    /// REGEN's darkening one-pole, Hz (0: none).
    pub regen_hz: f64,
}

/// `weights`: 12 taps, |shape| dB end to end, DENSITY's fade, Σw² = 1.
fn weights12(shape_db: f64, rising: bool, taps: f64, tab: &[(f64, f64)]) -> Vec<f64> {
    let nn = (taps - 6.0).clamp(0.0, 6.0);
    let step = 10f64.powf(-shape_db.abs() / 20.0 / 11.0);
    let mut w = [0.0; 12];
    let mut g = 1.0;
    for i in 0..12 {
        w[if rising { 11 - i } else { i }] = g;
        g *= step;
    }
    for (i, &k) in FADE.iter().enumerate() {
        w[k] *= ((nn - 0.8 * i as f64) * 0.5).clamp(0.0, 1.0);
    }
    let s = w.iter().map(|x| x * x).sum::<f64>().sqrt();
    w.iter().zip(tab).map(|(x, t)| x / s * t.1).collect()
}

fn tail_gain(rt: f64) -> f64 {
    let trip = (prime_ms(37.0) + prime_ms(11.0)) as f64;
    10f64.powf(-3.0 * trip / (rt.max(1e-3) * FS)).min(MAX_GAIN)
}

/// The engine at 24 kHz on the decimated send `u`.
pub fn render24(ek: &Ek, u: &[f64]) -> Vec<f64> {
    let a = one_pole(ek.tone_hz);
    let len_s = ek.length_ms * FS / 1000.0;
    let pre = (ek.pre_ms * FS / 1000.0).round() as usize;
    // Taps: offsets and weights; the hit apart.
    let (d, w, dh, wh): (Vec<usize>, Vec<f64>, usize, f64) = if ek.n == 0 {
        let tab = tap_table(12, 0.3, true);
        let mut d = vec![0usize];
        for k in 0..11 {
            let pl = (k as f64 + 1.0 + tab[k + 1].0) / 11.0;
            d.push((len_s * pl + 0.5).clamp(2.0, BURST_MAX as f64) as usize);
        }
        let d = d.into_iter().map(|x| x + pre).collect();
        (d, weights12(ek.shape_db, ek.rising, ek.taps, &tab), 0, 0.0)
    } else {
        let n = ek.n;
        let tab = tap_table(n, 0.3, false);
        let on = ek.onset_ms * FS / 1000.0;
        let span = len_s - 2.7 * FS / 1000.0 - on;
        let d: Vec<usize> = (0..n)
            .map(|k| {
                (on + span * (k as f64 + tab[k].0) / (n - 1) as f64 + 0.5)
                    .clamp(0.0, BURST_MAX as f64) as usize
                    + pre
            })
            .collect();
        // dB below the top: CURVE·(1 − x)^γ, x the tap's place 0 → 1.
        let mut w: Vec<f64> = (0..n)
            .map(|k| {
                let x = k as f64 / (n - 1) as f64;
                10f64.powf(-ek.shape_db.abs() * (1.0 - x).powf(ek.gamma) / 20.0)
            })
            .collect();
        if ek.steps > 0 {
            // Equal weights inside each step, the step's mean level.
            let per = n.div_ceil(ek.steps);
            for c in w.chunks_mut(per) {
                let m = (c.iter().map(|x| x * x).sum::<f64>() / c.len() as f64).sqrt();
                c.iter_mut().for_each(|x| *x = m);
            }
        }
        let s = w.iter().map(|x| x * x).sum::<f64>().sqrt();
        let w: Vec<f64> = w.iter().zip(&tab).map(|(x, t)| x / s * t.1).collect();
        let wh = w[n - 1].abs() * 10f64.powf(ek.hit_db / 20.0);
        let dh = (len_s + 0.5) as usize + pre;
        (d, w, dh, wh)
    };
    let dmax = d.iter().copied().max().unwrap_or(0).max(dh) + 2;
    let mut line = Line::new(dmax);
    let mut pre_ap: Vec<Ap> = ek.diff_ms.iter().map(|&m| Ap::new(prime_ms(m))).collect();
    let mut ramp_ap: Vec<Ap> = ek.ramp_ms.iter().map(|&m| Ap::new(prime_ms(m))).collect();
    let mut dec = [Ap::new(73), Ap::new(199), Ap::new(149)];
    let (td, tap_m) = (prime_ms(37.0), prime_ms(11.0));
    let mut tail_l = Line::new(td + 1);
    let mut tail_ap = Ap::new(tap_m);
    let gt = ek.tail * tail_gain(ek.rt60_s);
    let ta = if ek.tdamp_hz > 0.0 {
        one_pole(ek.tdamp_hz)
    } else {
        1.0
    };
    let ra = if ek.regen_hz > 0.0 {
        one_pole(ek.regen_hz)
    } else {
        1.0
    };
    let regen_at = if ek.n == 0 { d[11] } else { dh };
    let (mut tone, mut regen, mut tlp) = (0.0, 0.0, 0.0);
    let mut out = Vec::with_capacity(u.len());
    for &x in u {
        tone += a * (x - tone);
        let mut p = tone;
        if !ek.post {
            for ap in &mut pre_ap {
                p = ap.run(p, ek.g);
            }
        }
        // REGEN: the end tap through its one-pole back into the line.
        regen += ra * (line.at(regen_at - 1) - regen);
        let x0 = p + ek.regen * regen;
        line.push(x0);
        let mut s: f64 = d.iter().zip(&w).map(|(&di, wi)| wi * line.at(di)).sum();
        // TAIL: T = S + gt·LP(AP(T delayed D)).
        if gt > 0.0 {
            let y = tail_ap.run(tail_l.at(td - 1), TAIL_G);
            tlp += ta * (y - tlp);
            s += gt * tlp;
            tail_l.push(s);
        }
        for ap in &mut ramp_ap {
            s = ap.run(s, ek.ramp_g);
        }
        if ek.post {
            let dry = s;
            for ap in &mut pre_ap {
                s = ap.run(s, ek.g);
            }
            s = dry + ek.wet * (s - dry);
        }
        let c = dec[0].run(s, DEC_G);
        let y = 0.5 * (dec[1].run(c, DEC_G) + dec[2].run(c, DEC_G));
        out.push(y + wh * line.at(dh));
    }
    out
}

/// 48 kHz in, 48 kHz out, through the bus's 2:1 and its latency.
pub fn render(ek: &Ek, x: &[f64]) -> Vec<f64> {
    let u = resample_poly(x, 1, 2);
    let y = resample_poly(&render24(ek, &u), 2, 1);
    let mut o = vec![0.0; LATENCY];
    o.extend_from_slice(&y[..x.len() - LATENCY]);
    o
}

// ── variants: knob → Ek ──

fn expl((lo, hi): (f64, f64), p: f64) -> f64 {
    lo * (hi / lo).powf(p.clamp(0.0, 1.0))
}

fn lin((lo, hi): (f64, f64), p: f64) -> f64 {
    lo + (hi - lo) * p.clamp(0.0, 1.0)
}

pub type K = Map<String, Value>;

fn kv(k: &K, n: &str) -> f64 {
    k.get(n).and_then(Value::as_f64).unwrap_or(0.0)
}

/// GATE's starting knobs (`reverb.rs` START).
const GATE_START: [(&str, f64); 7] = [
    ("size", 0.55),
    ("time", 0.3),
    ("tone", 0.2),
    ("diffuse", 0.7),
    ("predelay", 0.0),
    ("shape", 0.2),
    ("damping", 0.3),
];

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Gate,
    Rev,
}

struct Variant {
    ty: Ty,
    dims: &'static [&'static str],
    sweep: usize,
    starts: usize,
    ek: fn(&K) -> Ek,
}

fn gate_base(k: &K) -> Ek {
    let t = kv(k, "time");
    Ek {
        length_ms: expl((40.0, 600.0), kv(k, "size")),
        rt60_s: expl((0.15, 0.8), t),
        tail: (t / 0.05).clamp(0.0, 1.0),
        regen: 0.0,
        tone_hz: expl((10_000.0, 2_500.0), kv(k, "tone")),
        taps: lin((6.0, 12.0), kv(k, "diffuse")),
        g: lin((0.5, 0.7), kv(k, "diffuse")),
        pre_ms: 0.0,
        shape_db: lin((0.0, -18.0), kv(k, "shape")),
        rising: false,
        diff_ms: vec![4.3, 7.1],
        tdamp_hz: 0.0,
        post: false,
        n: 0,
        hit_db: 0.0,
        onset_ms: 0.0,
        steps: 0,
        ramp_ms: vec![],
        ramp_g: 0.0,
        wet: 1.0,
        gamma: 1.0,
        regen_hz: 6_000.0,
    }
}

/// gate4: PRE-DLY, TONE 12 k → 1 k, g 0–0.8, +11.3/17 ms allpasses, DAMP.
fn gate4(k: &K) -> Ek {
    Ek {
        tone_hz: expl((12_000.0, 1_000.0), kv(k, "tone")),
        g: lin((0.0, 0.8), kv(k, "diffuse")),
        pre_ms: lin((0.0, 80.0), kv(k, "predelay")),
        diff_ms: vec![4.3, 7.1, 11.3, 17.0],
        tdamp_hz: expl((12_000.0, 2_000.0), kv(k, "damping")),
        ..gate_base(k)
    }
}

/// gate5: gate4 with the diffuser after the taps (the approved move) and
/// SHAPE to −30 dB.
fn gate5(k: &K) -> Ek {
    Ek {
        post: true,
        shape_db: lin((0.0, -30.0), kv(k, "shape")),
        ..gate4(k)
    }
}

fn rev_v2(k: &K, n: usize) -> Ek {
    Ek {
        length_ms: expl((150.0, 800.0), kv(k, "size")),
        rt60_s: 1.0,
        tail: 0.0,
        regen: 0.8 * kv(k, "time"),
        tone_hz: expl((12_000.0, 1_200.0), kv(k, "tone")),
        taps: 12.0,
        g: lin((0.0, 0.7), kv(k, "diffuse")),
        pre_ms: 0.0,
        shape_db: lin((0.0, 60.0), kv(k, "shape")),
        rising: true,
        diff_ms: vec![4.3, 7.1],
        tdamp_hz: 0.0,
        post: true,
        n,
        hit_db: lin((0.0, 20.0), kv(k, "hit")),
        onset_ms: lin((0.0, 150.0), kv(k, "onset")),
        steps: 0,
        ramp_ms: vec![],
        ramp_g: 0.0,
        wet: 1.0,
        gamma: 1.0,
        regen_hz: 6_000.0,
    }
}

/// The dense-ramp trials: 12 taps through a ramp allpass chain.
fn rev_chain(k: &K, ms: &[f64]) -> Ek {
    Ek {
        ramp_ms: ms.to_vec(),
        ramp_g: lin((0.0, 0.75), kv(k, "ramp")),
        ..rev_v2(k, 12)
    }
}

const GATE_DIMS: &[&str] = &[
    "size", "time", "tone", "diffuse", "shape", "predelay", "damping",
];
const REV_DIMS: &[&str] = &["size", "time", "tone", "diffuse", "shape", "hit", "onset"];
const REVR_DIMS: &[&str] = &[
    "size", "time", "tone", "diffuse", "shape", "hit", "onset", "ramp",
];

fn variant(name: &str) -> Result<Variant, String> {
    let v = |ty, dims, sweep, starts, ek| Variant {
        ty,
        dims,
        sweep,
        starts,
        ek,
    };
    Ok(match name {
        "gate4" => v(Ty::Gate, GATE_DIMS, 280, 5, gate4),
        "gate5" => v(Ty::Gate, GATE_DIMS, 280, 5, gate5),
        "gate6" => v(Ty::Gate, GATE_DIMS, 280, 5, |k| Ek {
            diff_ms: vec![2.3, 4.3, 7.1, 11.3, 17.0],
            ..gate5(k)
        }),
        // gate6 with DENSITY's low end dry: the diffuser's share 1.5·DENSITY
        // (capped at 1), its g 0.2 → 0.8.
        "gate7" => v(Ty::Gate, GATE_DIMS, 280, 5, |k| Ek {
            diff_ms: vec![2.3, 4.3, 7.1, 11.3, 17.0],
            g: lin((0.2, 0.8), kv(k, "diffuse")),
            wet: (1.5 * kv(k, "diffuse")).clamp(0.0, 1.0),
            ..gate5(k)
        }),
        "rev16og" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            gamma: 1.6,
            ..rev_v2(k, 16)
        }),
        "rev12c3g" => v(Ty::Rev, REVR_DIMS, 160, 3, |k| Ek {
            gamma: 1.6,
            ..rev_chain(k, &[5.3, 9.7, 14.9])
        }),
        "rev16ogs" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            gamma: 1.6,
            steps: 4,
            ..rev_v2(k, 16)
        }),
        "rev24og" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            gamma: 1.6,
            ..rev_v2(k, 24)
        }),
        // rev16og on GATE's gate7 diffuser (one engine): five allpasses,
        // g 0.2 → 0.8, the diffused share 1.5·DENSITY.
        "rev16ogu" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            gamma: 1.6,
            diff_ms: vec![2.3, 4.3, 7.1, 11.3, 17.0],
            g: lin((0.2, 0.8), kv(k, "diffuse")),
            wet: (1.5 * kv(k, "diffuse")).clamp(0.0, 1.0),
            ..rev_v2(k, 16)
        }),
        // rev16ogu with REGEN's one-pole open (the unit's regen is flat).
        "rev16ogu2" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            gamma: 1.6,
            diff_ms: vec![2.3, 4.3, 7.1, 11.3, 17.0],
            g: lin((0.2, 0.8), kv(k, "diffuse")),
            wet: (1.5 * kv(k, "diffuse")).clamp(0.0, 1.0),
            regen_hz: 0.0,
            ..rev_v2(k, 16)
        }),
        // GATE's five allpasses, fully diffused (REVERSE's own DENSITY
        // law: g 0 → 0.8), REGEN's one-pole open.
        "rev16ogu3" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            gamma: 1.6,
            diff_ms: vec![2.3, 4.3, 7.1, 11.3, 17.0],
            g: lin((0.0, 0.8), kv(k, "diffuse")),
            regen_hz: 0.0,
            ..rev_v2(k, 16)
        }),
        "rev12c3s" => v(Ty::Rev, REVR_DIMS, 160, 3, |k| {
            rev_chain(k, &[5.3, 9.7, 14.9])
        }),
        "rev16c2" => v(Ty::Rev, REVR_DIMS, 160, 3, |k| Ek {
            n: 16,
            ..rev_chain(k, &[9.7, 14.9])
        }),
        "rev16o" => v(Ty::Rev, REV_DIMS, 120, 3, |k| rev_v2(k, 16)),
        "rev16os" => v(Ty::Rev, REV_DIMS, 120, 3, |k| Ek {
            steps: 4,
            ..rev_v2(k, 16)
        }),
        "rev12c2" => v(Ty::Rev, REVR_DIMS, 160, 3, |k| rev_chain(k, &[13.0, 23.0])),
        "rev12c3" => v(Ty::Rev, REVR_DIMS, 160, 3, |k| {
            rev_chain(k, &[13.0, 23.0, 31.0])
        }),
        "rev8c3" => v(Ty::Rev, REVR_DIMS, 160, 3, |k| Ek {
            n: 8,
            ..rev_chain(k, &[13.0, 23.0, 31.0])
        }),
        _ => return Err(format!("{name}: no such model variant")),
    })
}

// ── REVERSE's metrics, from the click ──

fn env_db(x: &[f64], w: usize) -> Vec<f64> {
    let sq: Vec<f64> = x.iter().map(|v| v * v).collect();
    let e: Vec<f64> = box_same(&sq, w)
        .into_iter()
        .map(|v| v.sqrt() + 1e-12)
        .collect();
    let m = e.iter().copied().fold(0.0, f64::max);
    e.iter().map(|v| 20.0 * (v / m).log10()).collect()
}

fn argmax(x: &[f64]) -> usize {
    let mut b = 0;
    for (i, &v) in x.iter().enumerate() {
        if v > x[b] {
            b = i;
        }
    }
    b
}

/// Abel–Huang NED at `t` ms (20 ms Hann), as `metrics::ned`.
fn ned1(h: &[f64], t: f64) -> f64 {
    let w = (20e-3 * SR) as usize;
    let win = hanning(w);
    let c = (t * 1e-3 * SR) as i64;
    let a = (c - w as i64 / 2).max(0) as usize;
    let seg = &h[a.min(h.len())..(a + w).min(h.len())];
    let ws: f64 = win[..seg.len()].iter().sum();
    let ww: Vec<f64> = win[..seg.len()].iter().map(|v| v / ws).collect();
    let sd = ww
        .iter()
        .zip(seg)
        .map(|(w, x)| w * x * x)
        .sum::<f64>()
        .sqrt();
    if sd > 0.0 {
        ww.iter()
            .zip(seg)
            .filter(|(_, x)| x.abs() > sd)
            .map(|(w, _)| w)
            .sum::<f64>()
            / ERFC_INV_SQRT2
    } else {
        0.0
    }
}

#[derive(Clone, Debug)]
pub struct RevM {
    hit_ms: f64,
    hit_db: f64,
    ramp: [f64; 4],
    on60: f64,
    on40: f64,
    fall_ms: f64,
    ned: [f64; 3],
    cen: f64,
    rt: [Option<f64>; 7],
}

/// `h` from the click; `whole` for the centroid (the burst to hit + 20 ms).
fn rev_metrics(h: &[f64], rt: [Option<f64>; 7]) -> RevM {
    let hb = &h[..h.len().min((0.75 * SR) as usize)];
    let pk = hb.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let first = |db: f64| {
        let thr = pk * 10f64.powf(db / 20.0);
        1000.0 * hb.iter().position(|v| v.abs() >= thr).unwrap_or(0) as f64 / SR
    };
    let e = env_db(hb, 240);
    let te = argmax(&e);
    let lo = te.saturating_sub(2880);
    let hi = te.saturating_sub(288).max(lo + 1);
    let top = e[lo..hi].iter().copied().fold(f64::MIN, f64::max);
    let e20 = env_db(hb, 960);
    let ramp = [0.2, 0.4, 0.6, 0.8].map(|f| e20[(te as f64 * f) as usize]);
    let fall = 1000.0 * e[te..].iter().position(|&v| v < e[te] - 20.0).unwrap_or(0) as f64 / SR;
    let ned = [0.5, 0.75, 0.9].map(|f| {
        let c = te as f64 * f / SR * 1000.0;
        (0..11).map(|i| ned1(hb, c - 5.0 + i as f64)).sum::<f64>() / 11.0
    });
    // The whole burst's centroid, from the full IR's envelope peak.
    let ef = env_db(h, 240);
    let tf = argmax(&ef);
    let x = &h[..(tf + 960).min(h.len())];
    let (f, p) = welch(x, SR, x.len().min(2048));
    let (mut num, mut den) = (0.0, 0.0);
    for (fi, pi) in f.iter().zip(&p) {
        if (60.0..=11_000.0).contains(fi) {
            num += fi * pi;
            den += pi;
        }
    }
    RevM {
        hit_ms: 1000.0 * te as f64 / SR,
        hit_db: e[te] - top,
        ramp,
        on60: first(-60.0),
        on40: first(-40.0),
        fall_ms: fall,
        ned,
        cen: num / den,
        rt,
    }
}

/// Proposed tolerances (ADR 0095).
const T_HIT_MS: f64 = 1.0;
const T_HIT_DB: f64 = 1.5;
const T_RAMP: f64 = 3.0;
const T_ON: f64 = 0.15;
const T_FALL: f64 = 5.0;
const T_NED: f64 = 0.10;

struct RevR {
    hit_ms: f64,
    hit_db: f64,
    ramp: [f64; 4],
    on: [f64; 2],
    fall: f64,
    ned: [f64; 3],
    cen: f64,
    rt: [f64; 5],
}

fn rev_resid(m: &RevM, r: &RevM) -> RevR {
    RevR {
        hit_ms: m.hit_ms - r.hit_ms,
        hit_db: m.hit_db - r.hit_db,
        ramp: core::array::from_fn(|i| m.ramp[i] - r.ramp[i]),
        on: [
            m.on60 / r.on60.max(1.0) - 1.0,
            m.on40 / r.on40.max(1.0) - 1.0,
        ],
        fall: m.fall_ms - r.fall_ms,
        ned: core::array::from_fn(|i| m.ned[i] / r.ned[i] - 1.0),
        cen: m.cen / r.cen - 1.0,
        rt: core::array::from_fn(|i| match (m.rt[i + 1], r.rt[i + 1]) {
            (Some(a), Some(b)) => a / b - 1.0,
            _ => 1.0,
        }),
    }
}

fn sq(a: f64, t: f64) -> f64 {
    (a / t).powi(2)
}

fn rev_distance(x: &RevR) -> f64 {
    2.0 * sq(x.hit_ms, T_HIT_MS)
        + 2.0 * sq(x.hit_db, T_HIT_DB)
        + x.ramp.iter().map(|r| sq(*r, T_RAMP)).sum::<f64>() / 2.0
        + x.on.iter().map(|o| sq(*o, T_ON)).sum::<f64>() / 2.0
        + sq(x.fall, T_FALL)
        + x.ned.iter().map(|n| sq(*n, T_NED)).sum::<f64>() / 3.0
        + 3.0 * x.rt.iter().map(|r| sq(*r, 0.05)).sum::<f64>() / 5.0
        + sq(x.cen, 0.05)
}

/// The failing REVERSE metrics; `rt_judged` as the Python pass count did
/// (every preset), else on the regens only (the proposal).
fn rev_fails(x: &RevR, rt_judged: bool) -> Vec<&'static str> {
    let mut f = vec![];
    if x.hit_ms.abs() > T_HIT_MS {
        f.push("hit_ms");
    }
    if x.hit_db.abs() > T_HIT_DB {
        f.push("hit_db");
    }
    if x.ramp.iter().any(|r| r.abs() > T_RAMP) {
        f.push("ramp");
    }
    if x.on.iter().any(|o| o.abs() > T_ON) {
        f.push("onset");
    }
    if x.fall.abs() > T_FALL {
        f.push("fall");
    }
    if x.ned.iter().any(|n| n.abs() > T_NED) {
        f.push("ned");
    }
    if x.cen.abs() > 0.05 {
        f.push("cen");
    }
    if rt_judged && x.rt.iter().any(|r| r.abs() > 0.05) {
        f.push("rt");
    }
    f
}

// ── measuring and fitting ──

struct Ref {
    m: Metrics,
    rev: Option<RevM>,
    name: String,
}

fn load_ref(cfg: &Config, p: u32, ty: Ty) -> Result<Ref, String> {
    let path = cfg
        .refs
        .clone()
        .unwrap_or_else(|| cfg.out.join("refs/metrics.json"));
    let all: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| e.to_string())?;
    let v = &all[p.to_string()];
    let m = Metrics::from_json(v)?;
    let rev = if ty == Ty::Rev {
        let dir = path.parent().unwrap_or(Path::new("."));
        let (h, _) = read_wav(&dir.join(format!("p{p:02}_ir.wav")))?;
        Some(rev_metrics(&h, m.band_rt60_s))
    } else {
        None
    };
    Ok(Ref {
        m,
        rev,
        name: v["name"].as_str().unwrap_or("").to_string(),
    })
}

struct Meas {
    m: Metrics,
    rev: Option<RevM>,
}

/// `long`: the references' 20 s tails (2 s charge), for a long REGEN.
fn measure(var: &Variant, k: &K, seeds: usize, long: bool) -> Meas {
    let ek = (var.ek)(k);
    let (cs, charge, tail) = if long {
        (20.0, 2.0, 20.0)
    } else {
        (4.0, 1.0, 3.0)
    };
    let c = click(cs);
    let (h, ys) = rayon::join(
        || render(&ek, &c),
        || {
            (0..seeds)
                .into_par_iter()
                .map(|s| render(&ek, &noise_stim(s as u32, charge, tail).0))
                .collect::<Vec<_>>()
        },
    );
    let rt = noise_band_rt60(&ys, (charge * SR) as usize);
    let h = &h[CLICK_AT..];
    let m = ir_metrics(&h[..h.len().min((3.0 * SR) as usize)], Some(&rt));
    let rev = (var.ty == Ty::Rev).then(|| rev_metrics(h, rt.bands));
    Meas { m, rev }
}

fn score(var: &Variant, m: &Meas, r: &Ref) -> f64 {
    match var.ty {
        Ty::Gate => distance(&residuals(&m.m, &r.m)),
        Ty::Rev => rev_distance(&rev_resid(m.rev.as_ref().unwrap(), r.rev.as_ref().unwrap())),
    }
}

/// The Python fitters' search: an LHS sweep (2 seeds), then a sequential
/// pattern search (first improvement, 4 seeds) from the best `starts`.
fn fit_one(var: &Variant, p: u32, r: &Ref, start: Option<&K>) -> (K, Meas, f64) {
    let dims: Vec<&str> = var
        .dims
        .iter()
        .copied()
        .filter(|d| var.ty == Ty::Gate || *d != "time" || p == 47 || p == 48)
        .collect();
    let long = var.ty == Ty::Rev && p == 48;
    let base: K = if var.ty == Ty::Gate {
        GATE_START
            .iter()
            .map(|(n, v)| (n.to_string(), json!(v)))
            .collect()
    } else {
        var.dims
            .iter()
            .map(|n| (n.to_string(), json!(0.0)))
            .collect()
    };
    let mk = |u: &[f64]| {
        let mut k = base.clone();
        for (d, x) in dims.iter().zip(u) {
            k.insert(d.to_string(), json!(x));
        }
        k
    };
    // A polish (from earlier fits): fine steps on 16 seeds.
    let (cands, steps): (Vec<(f64, K)>, &[f64]) = match start {
        Some(k) => (vec![(0.0, k.clone())], &[0.012, 0.004]),
        None => {
            let mut c: Vec<(f64, K)> = lhs(var.sweep, dims.len(), p)
                .par_iter()
                .map(|u| {
                    let k = mk(u);
                    (score(var, &measure(var, &k, 2, long), r), k)
                })
                .collect();
            c.sort_by(|a, b| a.0.total_cmp(&b.0));
            c.truncate(var.starts);
            (c, &[0.08, 0.025, 0.008])
        }
    };
    let best = cands
        .par_iter()
        .map(|(_, k0)| {
            let mut bk = k0.clone();
            let mut bs = score(var, &measure(var, &bk, 4, long), r);
            for &st in steps {
                let seeds = match (start, st < 0.01) {
                    (Some(_), _) => 16,
                    (None, true) => 8,
                    (None, false) => 4,
                };
                bs = score(var, &measure(var, &bk, seeds, long), r);
                let mut imp = true;
                while imp {
                    imp = false;
                    for d in &dims {
                        for s in [-st, st] {
                            let cur = kv(&bk, d);
                            let v = (cur + s).clamp(0.0, 1.0);
                            if v == cur {
                                continue;
                            }
                            let mut k = bk.clone();
                            k.insert(d.to_string(), json!(v));
                            let sc = score(var, &measure(var, &k, seeds, long), r);
                            if sc < bs - 1e-9 {
                                (bs, bk, imp) = (sc, k, true);
                            }
                        }
                    }
                }
            }
            (bs, bk)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    let m = measure(var, &best.1, 16, long);
    let d = score(var, &m, r);
    (best.1, m, d)
}

fn pct(x: f64) -> String {
    format!("{:+.0}", x * 100.0)
}

/// `model-fit VARIANT PROG …`: fits, prints a line per program and the pass
/// count, writes `out/models/VARIANT.json`.
pub fn fit(cfg: &Config, name: &str, progs: &[u32]) -> Result<(), String> {
    let var = variant(name)?;
    // `MODEL_START=fits.json`: polish those fits instead of searching.
    let start: Option<Value> = match std::env::var_os("MODEL_START") {
        Some(p) => Some(
            serde_json::from_str(&std::fs::read_to_string(&p).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        ),
        None => None,
    };
    let refs: Vec<(u32, Ref)> = progs
        .iter()
        .map(|&p| load_ref(cfg, p, var.ty).map(|r| (p, r)))
        .collect::<Result<_, _>>()?;
    let res: Vec<(u32, String, bool, bool, Value)> = refs
        .par_iter()
        .map(|(p, r)| {
            let k0 = start.as_ref().and_then(|s| s[&p.to_string()]["knobs"].as_object().cloned());
            let (k, m, d) = fit_one(&var, *p, r, k0.as_ref());
            let ek = format!("{:?}", (var.ek)(&k));
            let (line, pass, pass2) = match var.ty {
                Ty::Gate => {
                    let x = residuals(&m.m, &r.m);
                    let v = verdict(&x);
                    let (g, rg) = (m.m.gate.as_ref(), r.m.gate.as_ref());
                    let burst = match (g, rg) {
                        (Some(g), Some(rg)) => format!(
                            " cut {:+.0} slope {:+.0} peak {:+.0}",
                            g.cut_ms - rg.cut_ms,
                            g.body_slope_db_s - rg.body_slope_db_s,
                            g.peak_ms - rg.peak_ms
                        ),
                        _ => String::new(),
                    };
                    (
                        format!(
                            "rt {} pre {:+.1} c50 {:+.1} ned {} cen {}%{burst} {}",
                            x.band_rt60_rel[1..6]
                                .iter()
                                .map(|b| pct(b.unwrap_or(1.0)))
                                .collect::<Vec<_>>()
                                .join(" "),
                            x.predelay_ms,
                            x.c50_db,
                            x.ned_rel.map(pct).join("/"),
                            pct(x.centroid_rel),
                            v.to_json()
                        ),
                        v.all(),
                        v.all(),
                    )
                }
                Ty::Rev => {
                    let x = rev_resid(m.rev.as_ref().unwrap(), r.rev.as_ref().unwrap());
                    let regen = *p == 47 || *p == 48;
                    let f1 = rev_fails(&x, true);
                    let f2 = rev_fails(&x, regen);
                    (
                        format!(
                            "hit {:+.1}ms {:+.1}dB ramp {} on {}/{}% fall {:+.1} ned {} rt {} cen {}% fails {:?}",
                            x.hit_ms,
                            x.hit_db,
                            x.ramp.map(|v| format!("{v:+.1}")).join(" "),
                            pct(x.on[0]),
                            pct(x.on[1]),
                            x.fall,
                            x.ned.map(pct).join("/"),
                            x.rt.map(pct).join(" "),
                            pct(x.cen),
                            f2
                        ),
                        f1.is_empty(),
                        f2.is_empty(),
                    )
                }
            };
            println!("{name} #{p} {:22} d={d:.1} {line}", r.name);
            let j = json!({"knobs": k, "ek": ek, "distance": d, "line": line, "pass": pass, "pass_proposed": pass2});
            (*p, line, pass, pass2, j)
        })
        .collect();
    let n1 = res.iter().filter(|r| r.2).count();
    let n2 = res.iter().filter(|r| r.3).count();
    println!(
        "{name}: {n1}/{} pass (the Python verdict); {n2}/{} at the proposed metrics",
        res.len(),
        res.len()
    );
    let out: Map<String, Value> = res
        .into_iter()
        .map(|(p, _, _, _, j)| (p.to_string(), j))
        .collect();
    let tag = if start.is_some() { "_polish" } else { "" };
    let path: PathBuf = cfg.out.join(format!("models/{name}{tag}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&Value::Object(out)).unwrap(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_lengths_match_the_engine() {
        assert_eq!(
            [4.3, 7.1, 3.1, 8.3, 6.1, 37.0, 11.0].map(prime_ms),
            [103, 167, 73, 199, 149, 887, 263]
        );
    }

    #[test]
    fn an_undiffused_hit_lands_at_length() {
        let mut k = K::new();
        for (n, v) in [
            ("size", 0.5),
            ("shape", 0.5),
            ("hit", 0.5),
            ("diffuse", 0.5),
        ] {
            k.insert(n.into(), json!(v));
        }
        let ek = rev_v2(&k, 16);
        let h = render(&ek, &click(1.0));
        let at = argmax(&h.iter().map(|v| v.abs()).collect::<Vec<_>>()) - CLICK_AT - LATENCY;
        let want = ek.length_ms * SR / 1000.0;
        assert!((at as f64 - want).abs() < 4.0, "{at} vs {want}");
    }
}
