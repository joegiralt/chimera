//! `f32` approximations of the few transcendental functions the engine needs.

use core::f32::consts::LN_2;

/// `2^x`; relative error below 1e-6 for `x` in `-126..127`, exact at
/// integers. Reduces to `f` in `[-0.5, 0.5]` (round-to-nearest, not floor),
/// so the Taylor polynomial below is always evaluated near its center,
/// where it is most accurate — floor-based reduction put `f` near 1 for `x`
/// just below an integer, which is its least accurate point and made slow
/// decays run up to 18% fast.
pub const fn exp2(x: f32) -> f32 {
    let x = x.clamp(-126.0, 127.0);
    let bias = if x >= 0.0 { 0.5 } else { -0.5 };
    let xi = (x + bias) as i32;
    let f = x - xi as f32;
    let p = 1.0
        + f * (LN_2
            + f * (0.240_226_5
                + f * (0.055_504_11
                    + f * (0.009_618_129
                        + f * (0.001_333_355_8 + f * (0.000_154_035_3 + f * 0.000_015_252_734))))));
    f32::from_bits(((xi + 127) as u32) << 23) * p
}

/// `log2(x)` for `x > 0`; absolute error below 3e-5. Zero, negative and
/// subnormal inputs clamp up to the smallest positive normal,
/// `f32::MIN_POSITIVE`, rather than being undefined.
pub fn log2(x: f32) -> f32 {
    let x = x.max(f32::MIN_POSITIVE);
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    e as f32 + s * (2.885_39 + s2 * (0.961_797_6 + s2 * (0.577_078 + s2 * 0.412_198_6)))
}

/// `1 / sqrt(x)` for `x > 0`; relative error below 1e-5. Zero, negative and
/// subnormal inputs clamp up to the smallest positive normal,
/// `f32::MIN_POSITIVE`, rather than being undefined.
pub fn inv_sqrt(x: f32) -> f32 {
    let x = x.max(f32::MIN_POSITIVE);
    let y = f32::from_bits(0x5f37_59df - (x.to_bits() >> 1));
    let y = y * (1.5 - 0.5 * x * y * y);
    y * (1.5 - 0.5 * x * y * y)
}
