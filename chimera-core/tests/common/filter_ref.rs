//! The filter types' references (spec § Tests): CLEAN's topology in f64 with
//! exact divides, the per-sample `g` a block's ramp gives, and level measures.

use chimera_core::dsp::clampf;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::filter::clean::trim;
use chimera_core::dsp::filter::coef::g_of;
use chimera_core::dsp::filter::g_at;
use chimera_hal::BLOCK_SIZE;

/// The self-oscillation band every resonant type shares at RES 1, dBFS RMS
/// (spec test 2, H-new-1): a TYPE switch at RES 1 jumps no more than its
/// width. Tasks 8 and 9 may widen it only to take their type in, with the
/// reason in § Measured.
#[allow(dead_code)] // not every test binary reads it
pub const OSC_BAND_DB: (f32, f32) = (-12.0, -4.0);

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

/// CLEAN's output trim at `k`: 1 for `k ≥ 0.5`, linear to the mode's
/// `TRIM_OSC` (read as `trim` at `k = −1`) at `k ≤ 0`; PHASER untrimmed.
pub fn trim_ref(mode: FilterMode, k: f32) -> f32 {
    let osc = trim(mode, -1.0);
    match mode {
        FilterMode::Phaser => 1.0,
        _ => osc + (1.0 - osc) * clampf(2.0 * k, 0.0, 1.0),
    }
}

/// CLEAN, sample by sample, from rest, with `g[n]`, `k[n]` and the output
/// trim `t[n]` per sample, and the second stage at `k₂ = max(1.2k + 0.3,
/// 0.1)`.
pub fn clean_ref(x: &[f32], g: &[f64], k: &[f64], t: &[f64], mode: FilterMode) -> Vec<f64> {
    let (mut s1, mut s2) = ([0.0f64; 2], [0.0f64; 2]);
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
            FilterMode::Hp24 => {
                let hp = stage(x, g, k, a1, a2).2;
                stage(hp, g, k2, b1, b2).2
            }
            FilterMode::Notch => {
                let (lp, _, hp) = stage(x, g, k, a1, a2);
                lp + 0.25 * hp
            }
            FilterMode::Phaser => {
                let ap1 = x - 2.0 * k * stage(x, g, k, a1, a2).1;
                ap1 - 2.0 * k * stage(ap1, g, k, b1, b2).1
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
