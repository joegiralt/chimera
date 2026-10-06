//! The filters' coefficients and their in-loop saturator.

use core::f64::consts::PI;

const P3: f64 = PI * PI * PI;
const P5: f64 = P3 * PI * PI;
const P7: f64 = P5 * PI * PI;
const P9: f64 = P7 * PI * PI;
const P11: f64 = P9 * PI * PI;

/// `tan(π·f)` by stmlib's `FREQUENCY_ACCURATE` polynomial (`dsp/filter.h`,
/// Copyright 2014 Emilie Gillet, MIT): its coefficients, each × π^n in
/// double and rounded to f32 as the C++ does. At 48 kHz it reads low by
/// 3e-6 at 5 kHz, 6e-4 at 10 kHz and 1.4 % at 15 kHz: a 20 kHz cutoff
/// lands at 19.3 kHz (`fast_tan`: 18.3, #57). It has no clamp; `g_of`
/// adds one at 0.497, where it reads 6.7 (21.7 kHz).
#[inline(always)]
#[allow(clippy::excessive_precision)] // stmlib's digits, as written
pub fn tan_accurate(f: f32) -> f32 {
    const A: f32 = (3.333314036e-01f32 as f64 * P3) as f32;
    const B: f32 = (1.333923995e-01f32 as f64 * P5) as f32;
    const C: f32 = (5.33740603e-02f32 as f64 * P7) as f32;
    const D: f32 = (2.900525e-03f32 as f64 * P9) as f32;
    const E: f32 = (9.5168091e-03f32 as f64 * P11) as f32;
    let f2 = f * f;
    f * (core::f32::consts::PI + f2 * (A + f2 * (B + f2 * (C + f2 * (D + f2 * E)))))
}

/// The SVF's `g` for cutoff `fc` at `fs`: `tan_accurate(min(fc/fs, 0.497))`.
/// The clamp is ours (stmlib's `FREQUENCY_EXACT` uses the same 0.497); a
/// NaN cutoff lands on it too.
#[inline(always)]
pub fn g_of(fc: f32, fs: f32) -> f32 {
    tan_accurate((fc / fs).min(0.497))
}

/// `2^x` for a FREQ tap (spec § 6): a cubic on the fraction, through 1 at
/// 0 and 2 at 1, plus the integer part added to the exponent bits. Within
/// 1.5e-4 (0.26 cents); divide-free. `x` is held to ±32, far past any tap's
/// ±8 octaves, so the exponent never wraps.
#[inline(always)]
#[allow(clippy::excessive_precision)]
pub fn exp2_poly(x: f32) -> f32 {
    let x = crate::dsp::clampf(x, -32.0, 32.0);
    // Floor by truncating a positive: `x + 64 > 0`.
    let i = (x + 64.0) as i32 - 64;
    let f = x - i as f32;
    let p = 1.0 + f * (0.696_065_64 + f * (0.224_494_34 + f * 0.079_440_24));
    f32::from_bits(p.to_bits().wrapping_add((i as u32) << 23))
}

/// Soft saturation of the integrators' state: linear to ±1, as it always
/// was, then `1 + u − u²/2` for `u = |x| − 1`, flat at ±1.5 from ±2. Its
/// value and slope are continuous (C1), so a loud signal bends, not steps:
/// the old curve jumped from 1 to 0.83 at ±1, and from 0.96 to 1 at ±1.5,
/// a buzz on anything that reached it (the owner's UAT, 2026-09-30). No
/// divide: it runs in every steady loop (spec H3).
#[inline(always)]
pub(crate) fn sat_c1(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.0 {
        return x;
    }
    let u = (a - 1.0).min(1.0);
    (1.0 + u - 0.5 * u * u).copysign(x)
}

#[cfg(test)]
mod tests {
    use super::sat_c1;

    /// C1: no step and no kink anywhere, odd, bounded by 1.5, linear to 1.
    #[test]
    fn sat_c1_is_smooth_and_bounded() {
        let h = 1e-3;
        let mut x = -3.0f32;
        while x < 3.0 {
            let (y0, y1, y2) = (sat_c1(x - h), sat_c1(x), sat_c1(x + h));
            assert!((y2 - y0).abs() <= 2.0 * h * 1.001, "step at {x}");
            let kink = ((y2 - y1) - (y1 - y0)).abs() / h;
            assert!(kink <= 2.0 * h + 1e-4, "kink at {x}: {kink}");
            assert!(y1.abs() <= 1.5 && sat_c1(-x) == -y1);
            x += 0.0137;
        }
        assert_eq!(sat_c1(0.9), 0.9);
        assert_eq!(sat_c1(2.5), 1.5);
    }
}
