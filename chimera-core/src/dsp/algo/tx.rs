//! TX81Z operator facts from the owner's manual, re-implemented (ADR 0023).

use crate::dsp::algo::math::exp2;

// 3.14 and 6.28 are TX81Z ratios, not approximations of pi and tau.
#[allow(clippy::approx_constant)]
pub static COARSE: [f32; 64] = [
    0.50, 0.71, 0.78, 0.87, 1.00, 1.41, 1.57, 1.73, 2.00, 2.82, 3.00, 3.14, 3.46, 4.00, 4.24, 4.71,
    5.00, 5.19, 5.65, 6.00, 6.28, 6.92, 7.00, 7.07, 7.85, 8.00, 8.48, 8.65, 9.00, 9.42, 9.89,
    10.00, 10.38, 10.99, 11.00, 11.30, 12.00, 12.11, 12.56, 12.72, 13.00, 13.84, 14.00, 14.10,
    14.13, 15.00, 15.55, 15.57, 15.70, 16.96, 17.27, 17.30, 18.37, 18.84, 19.03, 19.78, 20.41,
    20.76, 21.20, 21.98, 22.49, 23.55, 24.22, 25.95,
];

pub static FINE_TOP: [f32; 64] = [
    0.93, 1.32, 1.37, 1.62, 1.93, 2.73, 3.04, 3.35, 2.93, 4.14, 3.93, 4.61, 5.08, 4.93, 5.55, 6.18,
    5.93, 6.81, 6.96, 6.93, 7.75, 8.54, 7.93, 8.37, 9.32, 8.93, 9.78, 10.27, 9.93, 10.89, 11.19,
    10.93, 12.00, 12.46, 11.93, 12.60, 12.93, 13.73, 14.03, 14.01, 13.93, 15.46, 14.93, 15.42,
    15.60, 15.93, 16.83, 17.19, 17.17, 18.24, 18.74, 18.92, 19.65, 20.31, 20.65, 21.06, 21.88,
    22.38, 22.47, 23.45, 24.11, 25.02, 25.84, 27.57,
];

pub static COARSE_NAMES: [&str; 64] = [
    "0.50", "0.71", "0.78", "0.87", "1.00", "1.41", "1.57", "1.73", "2.00", "2.82", "3.00", "3.14",
    "3.46", "4.00", "4.24", "4.71", "5.00", "5.19", "5.65", "6.00", "6.28", "6.92", "7.00", "7.07",
    "7.85", "8.00", "8.48", "8.65", "9.00", "9.42", "9.89", "10.00", "10.38", "10.99", "11.00",
    "11.30", "12.00", "12.11", "12.56", "12.72", "13.00", "13.84", "14.00", "14.10", "14.13",
    "15.00", "15.55", "15.57", "15.70", "16.96", "17.27", "17.30", "18.37", "18.84", "19.03",
    "19.78", "20.41", "20.76", "21.20", "21.98", "22.49", "23.55", "24.22", "25.95",
];

/// FINE takes 16 even steps toward `FINE_TOP`, 8 below coarse index 4
/// (where FINE 8-15 hold the top).
pub fn ratio(coarse: u8, fine: u8) -> f32 {
    let c = (coarse as usize).min(63);
    let (steps, f) = if c < 4 {
        (7.0, fine.min(7))
    } else {
        (15.0, fine.min(15))
    };
    COARSE[c] + (FINE_TOP[c] - COARSE[c]) * f as f32 / steps
}

const LEVEL_STEP_OCT: f32 = 0.75 / 6.020_6;

pub static LEVEL_GAIN: [f32; 100] = {
    let mut g = [0.0; 100];
    let mut l = 1;
    while l < 100 {
        g[l] = exp2(-((99 - l) as f32) * LEVEL_STEP_OCT);
        l += 1;
    }
    g
};

/// A modulated LEVEL is fractional: linear between steps, so it never zippers.
pub fn level_gain(level: f32) -> f32 {
    let l = level.clamp(0.0, 99.0);
    let i = (l as usize).min(98);
    LEVEL_GAIN[i] + (LEVEL_GAIN[i + 1] - LEVEL_GAIN[i]) * (l - i as f32)
}

/// D1L 15 is full level; each step is 3 dB (four LEVEL steps) down; 0 is silent.
pub fn d1l_level(d1l: u8) -> f32 {
    match d1l.min(15) {
        0 => 0.0,
        d => LEVEL_GAIN[99 - 4 * (15 - d as usize)],
    }
}

/// Phase swing, in cycles, of a full-scale output fed back.
pub static FEEDBACK_CYCLES: [f32; 8] =
    [0.0, 1.0 / 32.0, 1.0 / 16.0, 1.0 / 8.0, 0.25, 0.5, 1.0, 2.0];

pub const DETUNE_CENTS: f32 = 1.5;

pub fn detune_factor(detune: i8) -> f32 {
    exp2(detune.clamp(-3, 3) as f32 * DETUNE_CENTS / 1200.0)
}
