//! A transwave from a recording (TAPE spec § 6 step 5): whole cycles cut
//! from across it, each built into octave levels, nominal `N` from the top
//! down to 16, each keeping harmonics `1..=⌊0.35 N⌋` by FFT truncation and
//! held at `wave::stored_log2`'s oversampled length. The PPG Wave and
//! Waldorf Microwave approach (band-limited copies per octave), for the
//! Ensoniq VFX/SD-1's transwaves. Host only: the firmware reads the levels.

use chimera_core::dsp::tape_engine::wave::{MIN_LOG2, nominal_top, stored_log2};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use super::convert::resample_ratio;

/// The stored top level unless `--top 1024` asks for a sixth level (179
/// harmonics at 1 024).
pub const DEFAULT_TOP: u32 = 512;
/// A transwave's most frames (the descriptor's `count`).
pub const MAX_FRAMES: usize = 32;
/// Samples at each end of the stretched input the sinc's edges reach.
const EDGE: usize = 128;

/// Whole cycles of `cycle` samples (fractional) from across `x`, at most
/// `MAX_FRAMES`: consecutive when `x` holds fewer, else spread evenly over
/// it. `x` is stretched by `rubato`'s sinc so a cycle is `⌈cycle⌉` samples;
/// each starts at a rising zero crossing. Too short an `x`, or no cycle
/// length, gives none.
pub fn cut_cycles(x: &[f32], cycle: f32) -> Vec<Vec<f32>> {
    if !(cycle.is_finite() && cycle >= 2.0) {
        return Vec::new();
    }
    let len = cycle.ceil() as usize;
    let Ok(y) = resample_ratio(x, len as f64 / cycle as f64) else {
        return Vec::new();
    };
    // A cycle's room after each place for its zero crossing.
    let Some(span) = y.len().checked_sub(2 * EDGE + 2 * len) else {
        return Vec::new();
    };
    let count = (span / len + 1).min(MAX_FRAMES);
    let gap = if count > 1 { span / (count - 1) } else { 0 };
    (0..count)
        .map(|k| {
            let from = EDGE + k * gap;
            let s = (from..from + len)
                .find(|&i| y[i] <= 0.0 && y[i + 1] > 0.0)
                .map_or(from, |i| i + 1);
            y[s..s + len].to_vec()
        })
        .collect()
}

/// `cycle`'s levels, nominal `top / 2` down to 16, one after another, each
/// at its stored length (1 216 samples for top 512, 2 240 for 1 024). Level
/// `N` is the cycle's harmonics `1..=⌊0.35 N⌋` (those it has: under half its
/// length) and nothing else, not even DC; every frame turned so harmonic 1
/// is in sine phase, so frames cut at any point line up and a scan between
/// two never cancels their fundamentals.
pub fn levels(cycle: &[f32], top: u32) -> Vec<f32> {
    let l = cycle.len();
    let mut planner = FftPlanner::<f64>::new();
    let mut x: Vec<Complex<f64>> = cycle.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    if l > 0 {
        planner.plan_fft_forward(l).process(&mut x);
    }
    // A time shift: harmonic k turns by k times harmonic 1's turn to −π/2.
    let peak = x.iter().fold(0.0f64, |m, c| m.max(c.norm()));
    let turn = match x.get(1) {
        Some(x1) if l > 2 && x1.norm() > 1e-9 * peak => x1.arg() + std::f64::consts::FRAC_PI_2,
        _ => 0.0,
    };
    let mut out = Vec::new();
    let mut nl = nominal_top(top).trailing_zeros();
    while nl >= MIN_LOG2 {
        let n = 1usize << stored_log2(nl);
        let keep = (0.35 * (1u64 << nl) as f64) as usize;
        let keep = keep.min(l.saturating_sub(1) / 2);
        let mut y = vec![Complex::new(0.0, 0.0); n];
        let scale = 1.0 / l as f64;
        for k in 1..=keep {
            let c = x[k] * Complex::from_polar(scale, -(k as f64) * turn);
            y[k] = c;
            y[n - k] = c.conj();
        }
        planner.plan_fft_inverse(n).process(&mut y);
        out.extend(y.iter().map(|c| c.re as f32));
        nl -= 1;
    }
    out
}
