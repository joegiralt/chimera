//! The noise operator's pure laws (spec § 4): two one-poles on scaled
//! uniform noise, ring-modulated up to FC.

use super::math::{clampf, exp2, sqrt};
use core::f32::consts::{LOG2_E, TAU};

/// The level reference, −12 dB, as FRMT's `R_REF`.
pub const NOISE_GAIN: f32 = 0.251;

/// The poles' output clamp, in σ: `NOISE_GAIN·√(3.8² + 1) ≤ 0.99`.
pub const NOISE_CLAMP: f32 = 3.8;

/// The poles' corner per Hz of WIDTH: two in series are −3 dB at
/// 0.6436·f_lp, doubled by the ring modulation.
const CORNER: f32 = 0.777;

/// `(c, k_n)` for WIDTH `width` Hz: the one-poles' coefficient
/// `1 − e^(−2π·f_lp/sr)`, and the input gain that gives the second pole
/// unit RMS from uniform noise in [−1, 1) (variance 1/3).
pub fn noise_coef(width: f32, sr: f32) -> (f32, f32) {
    // WIDTH held to ≥ 1 Hz (NaN too), under every stored WIDTH, so c stays
    // far from 0 and k_n finite.
    let width = clampf(width, 1.0, f32::MAX);
    let c = 1.0 - exp2(-TAU * CORNER * width / sr * LOG2_E);
    let a = 1.0 - c;
    // √3 / (c²·√((1 + a²)/(1 − a²)³)), with 1 − a² = c(2 − c).
    let t = 2.0 - c;
    (c, sqrt(3.0 * t * t * t / (c * (1.0 + a * a))))
}

/// `(α, β) = (√(1 − R), √R)` at `R = RES/100`: band noise and the tone at
/// FC, at equal power.
pub fn res_mix(res: u8) -> (f32, f32) {
    let r = f32::from(res.min(100)) / 100.0;
    (sqrt(1.0 - r), sqrt(r))
}

/// The upper-edge fade: 1 while `fc + width/2` ≤ 16 kHz, linear to 0 at
/// 20 kHz.
pub fn noise_edge(fc: f32, width: f32) -> f32 {
    clampf((20_000.0 - (fc + width / 2.0)) / 4_000.0, 0.0, 1.0)
}

/// Operator `op`'s xorshift seed in voice `voice`: nonzero and distinct
/// for every voice and operator (an odd multiplier is a bijection).
pub const fn noise_seed(voice: u8, op: usize) -> u32 {
    ((voice as u32) << 8 | (op as u32 & 0xff))
        .wrapping_add(1)
        .wrapping_mul(0x9e37_79b9)
}
