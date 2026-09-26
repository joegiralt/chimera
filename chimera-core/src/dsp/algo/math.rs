//! `f32` approximations of the few transcendental functions the engine needs.

use core::f32::consts::LN_2;

/// `2^x`; relative error below 2e-6 for `x` in `-126..127`, exact at integers.
pub const fn exp2(x: f32) -> f32 {
    let x = x.clamp(-126.0, 127.0);
    let t = x as i32;
    let xi = if (t as f32) > x { t - 1 } else { t };
    let f = x - xi as f32;
    let p = 1.0
        + f * (LN_2
            + f * (0.240_226_5
                + f * (0.055_504_11
                    + f * (0.009_618_129
                        + f * (0.001_333_355_8 + f * (0.000_154_035_3 + f * 0.000_015_252_734))))));
    f32::from_bits(((xi + 127) as u32) << 23) * p
}

/// `log2(x)` for `x > 0`; absolute error below 3e-5.
pub fn log2(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    e as f32 + s * (2.885_39 + s2 * (0.961_797_6 + s2 * (0.577_078 + s2 * 0.412_198_6)))
}

/// `1 / sqrt(x)` for `x >= 1`; relative error below 1e-5.
pub fn inv_sqrt(x: f32) -> f32 {
    let y = f32::from_bits(0x5f37_59df - (x.to_bits() >> 1));
    let y = y * (1.5 - 0.5 * x * y * y);
    y * (1.5 - 0.5 * x * y * y)
}
