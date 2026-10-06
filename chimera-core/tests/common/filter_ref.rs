//! The filter types' references (spec § Tests): CLEAN's topology in f64 with
//! exact divides, the per-sample `g` a block's ramp gives, and level measures.

use chimera_core::dsp::clampf;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::filter::K2040Voice;
use chimera_core::dsp::filter::clean::trim;
use chimera_core::dsp::filter::coef::g_of;
use chimera_core::dsp::filter::g_at;
use chimera_core::dsp::filter::k2040::{
    ALPHA_EXP, ALPHA_REV2, BETA_EXP, INV_BETA_EXP, L_2040, OUT_2040, R_2040,
};
use chimera_hal::BLOCK_SIZE;
use std::ops::{Add, Div, Mul, Sub};

/// The self-oscillation band every resonant type shares at RES 1, dBFS RMS
/// (spec test 2, H-new-1): a TYPE switch at RES 1 jumps no more than its
/// width. Tasks 8 and 9 may widen it only to take their type in, with the
/// reason in § Measured. Task 9 raised the top from −4 to −2: DIRTY rings
/// into its ±2 state clamps, through its ½ output, at −2.8 dBFS.
#[allow(dead_code)] // not every test binary reads it
pub const OSC_BAND_DB: (f32, f32) = (-12.0, -2.0);

/// The 2040's self-oscillation band on EXP, dBFS RMS: REV2's (`OSC_BAND_DB`)
/// less EXP's 6 dB of drive, which LEVEL ½'s unity costs (spec § 3).
#[allow(dead_code)]
pub const OSC_BAND_EXP_DB: (f32, f32) = (-18.0, -10.0);

/// `sat_c1` in f64.
fn sat(x: f64) -> f64 {
    let a = x.abs();
    if a <= 1.0 {
        return x;
    }
    let u = (a - 1.0).min(1.0);
    (1.0 + u - 0.5 * u * u).copysign(x)
}

/// One TPT SVF stage, exact divide: `(lp, bp, hp)`.
fn stage(x: f64, g: f64, k: f64, s1: &mut f64, s2: &mut f64) -> (f64, f64, f64) {
    let (v1, v2) = (sat(*s1), sat(*s2));
    let hp = (x - (k + g) * v1 - v2) / (1.0 + k * g + g * g);
    let bp = g * hp + v1;
    let lp = g * bp + v2;
    *s1 = bp + g * hp;
    *s2 = lp + g * bp;
    (lp, bp, hp)
}

/// NOTCH2's feedback at `k`: `0.7·RES`, with RES read back through
/// `k = 2·(1 − RES/0.95)`.
pub fn notch2_fb_ref(k: f64) -> f64 {
    0.7 * 0.95 * (1.0 - k / 2.0)
}

/// CLEAN's output trim at `k`: 1 for `k ≥ 0.5`, linear to the mode's
/// `TRIM_OSC` (read as `trim` at `k = −1`) at `k ≤ 0`; NOTCH2's is its
/// output scale, `√(1 − fb)·½`.
pub fn trim_ref(mode: FilterMode, k: f32) -> f32 {
    match mode {
        FilterMode::Phaser => (0.5 * (1.0 - notch2_fb_ref(k.into())).sqrt()) as f32,
        _ => {
            let osc = trim(mode, -1.0);
            osc + (1.0 - osc) * clampf(2.0 * k, 0.0, 1.0)
        }
    }
}

/// CLEAN, sample by sample, from rest, with `g[n]`, `k[n]` and the output
/// trim `t[n]` per sample, and the second stage at `k₂ = max(1.2k + 0.3,
/// 0.1)`.
pub fn clean_ref(x: &[f32], g: &[f64], k: &[f64], t: &[f64], mode: FilterMode) -> Vec<f64> {
    let (mut s1, mut s2) = ([0.0f64; 2], [0.0f64; 2]);
    let (mut ap, mut w) = ([0.0f64; 4], 0.0f64);
    let mut out = Vec::with_capacity(x.len());
    for n in 0..x.len() {
        let (x, g, k) = (f64::from(x[n]), g[n], k[n]);
        let (k2, t) = ((1.2 * k + 0.3).max(0.1), t[n]);
        let [a1, b1] = &mut s1;
        let [a2, b2] = &mut s2;
        let y = match mode {
            FilterMode::Lp6 => {
                let (lp, bp, _) = stage(x, g, k, a1, a2);
                lp + bp
            }
            FilterMode::Lp12 => stage(x, g, k, a1, a2).0,
            FilterMode::Lp24 => {
                let lp = stage(x, g, k, a1, a2).0;
                stage(lp, g, k2, b1, b2).0
            }
            FilterMode::Bp12 => 2.0 * stage(x, g, k, a1, a2).1,
            FilterMode::Bp24 => {
                let bp = stage(x, g, k, a1, a2).1;
                2.0 * stage(bp, g, k2, b1, b2).1
            }
            // HP12 is DIRTY's; CLEAN runs HP4 for it.
            FilterMode::Hp24 | FilterMode::Hp12 => {
                let hp = stage(x, g, k, a1, a2).2;
                stage(hp, g, k2, b1, b2).2
            }
            FilterMode::Notch => {
                let (lp, _, hp) = stage(x, g, k, a1, a2);
                lp + 0.25 * hp
            }
            // Dry plus four TPT all-passes, fed back one sample late; the
            // trim `t` is the output scale.
            FilterMode::Phaser => {
                let u = x + notch2_fb_ref(k) * w;
                let g1 = g / (1.0 + g);
                let mut v = u;
                for s in &mut ap {
                    let lp = (v - *s) * g1 + *s;
                    *s = 2.0 * lp - *s;
                    v = 2.0 * lp - v;
                }
                w = v;
                u + v
            }
        };
        out.push(t * y);
    }
    out
}

/// One block's `g`, sample by sample, as CLEAN ramps it from `from_hz`'s
/// to `to_hz`'s.
pub fn g_track(from_hz: f64, to_hz: f64, fs: f64) -> Vec<f64> {
    ramp_track(
        g_of(from_hz as f32, fs as f32),
        g_of(to_hz as f32, fs as f32),
    )
}

/// One block of a coefficient CLEAN ramps from `a` to `b`, sample by sample.
pub fn ramp_track(a: f32, b: f32) -> Vec<f64> {
    let step = (b - a) / BLOCK_SIZE as f32;
    (0..BLOCK_SIZE)
        .map(|i| f64::from(g_at(a, step, i)))
        .collect()
}

/// `out`'s level at `f`, dB re a full-scale sine: one DFT bin.
pub fn mag_db(out: &[f32], f: f32, fs: f32) -> f32 {
    let w = core::f64::consts::TAU * f64::from(f) / f64::from(fs);
    let (re, im) = out
        .iter()
        .enumerate()
        .fold((0.0f64, 0.0f64), |(r, i), (n, &v)| {
            let (s, c) = (w * n as f64).sin_cos();
            (r + f64::from(v) * c, i + f64::from(v) * s)
        });
    (20.0 * (re.hypot(im) * 2.0 / out.len() as f64).log10()) as f32
}

/// RMS, dBFS.
pub fn rms_db(x: &[f32]) -> f32 {
    let ms = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len() as f64;
    (10.0 * ms.log10()) as f32
}

/// In-place iterative radix-2 FFT; `re.len()` is a power of two.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The arithmetic the 2040's reference runs in: f64 for the jumps, f32 to
/// check the model op for op.
pub trait Real:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
{
    fn of(v: f32) -> Self;
    fn f64(self) -> f64;
}

impl Real for f32 {
    fn of(v: f32) -> Self {
        v
    }
    fn f64(self) -> f64 {
        f64::from(self)
    }
}

impl Real for f64 {
    fn of(v: f32) -> Self {
        f64::from(v)
    }
    fn f64(self) -> f64 {
        self
    }
}

fn clamp_r<T: Real>(x: T, lo: T, hi: T) -> T {
    let x = if x < lo { lo } else { x };
    if x > hi { hi } else { x }
}

/// The 2040's cubic, `c − (4/27)·c³` for `c = clamp(v, ±1.5)`.
fn sat_2040_r<T: Real>(v: T) -> T {
    let c = clamp_r(v, T::of(-1.5), T::of(1.5));
    c - T::of(4.0 / 27.0) * c * c * c
}

/// The 2040 (spec § 3), sample by sample from rest: `x[n]` scaled by
/// `a[n]` (`α·gain`), with `g[n]` and `k[n]`; `G` and `D` divided every
/// sample; the 3080 output stage if `post`. In `T`'s arithmetic, the
/// model's own order of operations.
pub fn k2040_ref<T: Real>(
    x: &[f32],
    g: &[f64],
    k: &[f64],
    a: &[f64],
    voice: K2040Voice,
    post: bool,
) -> Vec<f64> {
    let alpha = match voice {
        K2040Voice::Rev2 => ALPHA_REV2,
        K2040Voice::Exp => ALPHA_EXP,
    };
    let out = if post {
        T::of(OUT_2040 / alpha * INV_BETA_EXP)
    } else {
        T::of(OUT_2040 / alpha)
    };
    let one = T::of(1.0);
    let (r, l) = (T::of(R_2040), T::of(L_2040));
    let of64 = |v: f64| T::of(v as f32);
    let mut s = [T::of(0.0); 4];
    let mut y4s = Vec::with_capacity(x.len());
    for n in 0..x.len() {
        let (g, k) = (of64(g[n]), of64(k[n]));
        let big_g = g / (one + g);
        let g2 = big_g * big_g;
        let d = one / (one + k * g2 * g2);
        let (ga, one_g, kd) = (g * T::of(4.0 / 27.0), one - big_g, k * d);
        let x_c = T::of(x[n]) * of64(a[n]);
        let big_s = one_g * (s[3] + big_g * (s[2] + big_g * (s[1] + big_g * s[0])));
        let u_l = x_c * d - kd * big_s;
        let mut v = x_c - l * sat_2040_r((x_c - u_l) * T::of(1.0 / L_2040));
        for st in &mut s {
            let e = clamp_r(one_g * (v - *st), T::of(-1.5), T::of(1.5));
            let t = e * (g - ga * e * e);
            let y = clamp_r(*st + t, T::of(0.0) - r, r);
            *st = y + t;
            v = y;
        }
        let y = if post {
            sat_2040_r(T::of(BETA_EXP) * v) * out
        } else {
            v * out
        };
        y4s.push(y.f64());
    }
    y4s
}

/// One block of a coefficient the 2040 ramps (`g`, `k`, the input scale
/// `α·gain`), sample by sample: by adding its step.
pub fn a_track(from: f32, to: f32) -> Vec<f64> {
    let step = (to - from) / BLOCK_SIZE as f32;
    let mut a = from;
    (0..BLOCK_SIZE)
        .map(|_| {
            a += step;
            f64::from(a)
        })
        .collect()
}

/// The bound grid's inputs (spec test 8): silence, a full-scale saw, a
/// square at fs/4, white noise ±1 and impulses of ±10.
#[allow(dead_code)]
pub fn grid_input(which: usize, i: usize) -> f32 {
    match which {
        0 => 0.0,
        1 => (i % 109) as f32 / 54.5 - 1.0,
        2 => {
            if i % 4 < 2 {
                1.0
            } else {
                -1.0
            }
        }
        3 => {
            let h = (i as u32)
                .wrapping_mul(2_654_435_761)
                .rotate_left(13)
                .wrapping_mul(2_246_822_519);
            h as f32 / u32::MAX as f32 * 2.0 - 1.0
        }
        _ => match i % 480 {
            0 => 10.0,
            240 => -10.0,
            _ => 0.0,
        },
    }
}

/// Total harmonic distortion of `y`, a tone of `f0` Hz: harmonics 2–10
/// against the fundamental, as a fraction (one DFT bin each).
#[allow(dead_code)]
pub fn thd(y: &[f32], f0: f32, fs: f32) -> f64 {
    let amp = |f: f32| 10f64.powf(f64::from(mag_db(y, f, fs)) / 20.0);
    let h: f64 = (2..=10).map(|n| amp(f0 * n as f32).powi(2)).sum();
    h.sqrt() / amp(f0)
}
