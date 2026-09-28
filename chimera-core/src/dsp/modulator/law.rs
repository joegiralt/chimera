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
