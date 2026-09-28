//! Slider laws (spec § 1 "Sliders are positions") and the RC constants.

use crate::dsp::fast_exp2;
use crate::dsp::modulator::EnvSpeed;

/// An exponential slider: `q = min · (max/min)^p = min · 2^(p·oct)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    /// `log2(max / min)`.
    pub oct: f32,
}

impl Range {
    /// The quantity at position `p` (clamped to 0..1): one `fast_exp2`.
    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
    pub fn at(self, p: f32) -> f32 {
        self.min * fast_exp2(p.max(0.0).min(1.0) * self.oct)
    }
}

/// Envelope A's ranges at one SPEED, seconds: the manual's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedRanges {
    pub hold: Range,
    pub attack: Range,
    pub dec_rel: Range,
}

pub const fn speed_ranges(s: EnvSpeed) -> SpeedRanges {
    const fn r(min: f32, oct: f32) -> Range {
        Range { min, oct }
    }
    match s {
        // H 0.001 ms – 2.5 s, A 0.2 ms – 1.5 s, D and R 0.6 ms – 2.5 s.
        EnvSpeed::Fast => SpeedRanges {
            hold: r(1e-6, 21.253_497),
            attack: r(2e-4, 12.872_675),
            dec_rel: r(6e-4, 12.024_678),
        },
        // H 0.001 ms – 10 s, A 2 ms – 10 s, D and R 3.5 ms – 10 s.
        EnvSpeed::Med => SpeedRanges {
            hold: r(1e-6, 23.253_497),
            attack: r(2e-3, 12.287_712),
            dec_rel: r(3.5e-3, 11.480_357),
        },
        // H 0.001 ms – 60 s, A 9.3 ms – 60 s, D and R 30 ms – 60 s.
        EnvSpeed::Slow => SpeedRanges {
            hold: r(1e-6, 25.838_459),
            attack: r(9.3e-3, 12.655_444),
            dec_rel: r(3e-2, 10.965_784),
        },
    }
}

/// `log2` of an RC stage's per-sample retention, `−1 / (τ·fs·ln 2)`.
pub fn rc_k(tau: f32, fs: f32) -> f32 {
    -1.0 / (tau * fs * core::f32::consts::LN_2)
}

/// The per-sample step toward the target, `c = 1 − 2^k` (spec `rc_coeff`).
pub fn rc_coeff(k: f32) -> f32 {
    1.0 - fast_exp2(k)
}

/// Envelope B's ranges (spec § Envelope B): ENV RISE and FALL 2 ms – 5 s.
pub const B_TIME: Range = Range {
    min: 2e-3,
    oct: 11.287_712,
};
/// LFO RATE, 0.05 – 800 Hz.
pub const B_RATE: Range = Range {
    min: 0.05,
    oct: 13.965_784,
};
/// BURST pulse RATE, 0.05 Hz – 1 kHz.
pub const BURST_RATE: Range = Range {
    min: 0.05,
    oct: 14.287_712,
};
/// BURST LENGTH, 10 ms – 20 s.
pub const BURST_LEN: Range = Range {
    min: 0.01,
    oct: 10.965_784,
};
/// A B slot evaluated per block stops its rates here: the block rate ÷ 8
/// (spec § Rates), 93.75 Hz at 48 kHz and 86.1 Hz at 44.1 kHz.
pub fn block_rate_max(sample_rate: u32) -> f32 {
    sample_rate as f32 / chimera_hal::BLOCK_SIZE as f32 / 8.0
}

/// SHAPE's curve, `f(x) = x / (x + (1 − x)·w)`; linear (and no divide) at `w` = 1.
pub fn curve(x: f32, w: f32) -> f32 {
    if w == 1.0 { x } else { x / (x + (1.0 - x) * w) }
}

/// `f⁻¹(y) = w·y / (1 − y + w·y)`.
pub fn curve_inv(y: f32, w: f32) -> f32 {
    if w == 1.0 {
        y
    } else {
        w * y / (1.0 - y + w * y)
    }
}

/// SHAPE's position to `w = 2^(4·(2·SHAPE − 1))`; exactly 1 at the centre.
#[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
pub fn shape_w(shape: f32) -> f32 {
    fast_exp2(4.0 * (2.0 * shape.max(0.0).min(1.0) - 1.0))
}

/// TILT: rise over the fraction `r` of a cycle, fall over the rest.
/// `u = p/r` rising, `(1 − p)/(1 − r)` falling; r = 0 is `1 − p`, r = 1 is `p`.
pub fn tilt(p: f32, r: f32) -> f32 {
    if p < r {
        p / r
    } else if r < 1.0 {
        (1.0 - p) / (1.0 - r)
    } else {
        p
    }
}
