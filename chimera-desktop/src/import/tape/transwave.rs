// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A transwave from a recording (TAPE spec § 6 step 5): whole cycles cut
//! from across it, each built into octave levels, nominal `N` from the top
//! down to 16, each keeping harmonics `1..=wave::harmonics` (⌊0.35 N⌋, 42
//! at 128) by FFT truncation and
//! held at `wave::stored_log2`'s oversampled length. The PPG Wave and
//! Waldorf Microwave approach (band-limited copies per octave), for the
//! Ensoniq VFX/SD-1's transwaves. Host only: the firmware reads the levels.

use chimera_core::dsp::clampf;
use chimera_core::dsp::tape_engine::wave::{MIN_LOG2, harmonics, nominal_top, stored_log2};
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
/// `max` (itself at most `MAX_FRAMES`): consecutive when `x` holds fewer, else spread evenly over
/// it. `x` is stretched by `rubato`'s sinc so a cycle is `⌈cycle⌉` samples;
/// each starts at a rising zero crossing. Too short an `x`, or no cycle
/// length, gives none.
pub fn cut_cycles(x: &[f32], cycle: f32, max: usize) -> Vec<Vec<f32>> {
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
    let count = (span / len + 1).min(max.clamp(1, MAX_FRAMES));
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

/// Harmonic 1 sets a frame's phase unless it is more than 40 dB under the
/// frame's largest harmonic.
const WEAK_FUNDAMENTAL: f64 = 0.01;
/// A cycle's phase steps, aligning by cross-correlation.
const XCORR_STEPS: usize = 4_096;

/// A cycle's harmonics `1..=K` over its length (`[0]` unused), turned to
/// one phase: harmonic 1 in sine phase; or, with harmonic 1 too weak to
/// say, at the turn best correlated with `prev` (the frame before, as
/// turned), by a circular cross-correlation over the harmonics both hold.
fn spectrum(cycle: &[f32], prev: Option<&[Complex<f64>]>) -> Vec<Complex<f64>> {
    let l = cycle.len();
    if l < 3 {
        return vec![Complex::new(0.0, 0.0)];
    }
    let mut planner = FftPlanner::<f64>::new();
    let mut x: Vec<Complex<f64>> = cycle.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    planner.plan_fft_forward(l).process(&mut x);
    let mut x: Vec<Complex<f64>> = x[..=(l - 1) / 2].iter().map(|c| c / l as f64).collect();
    x[0] = Complex::new(0.0, 0.0);
    let peak = x.iter().fold(0.0f64, |m, c| m.max(c.norm()));
    // A time shift of `turn` radians turns harmonic k by k · turn.
    let turn = if peak > 0.0 && x[1].norm() > WEAK_FUNDAMENTAL * peak {
        x[1].arg() + std::f64::consts::FRAC_PI_2
    } else if let Some(p) = prev {
        // c(φ) = Re Σ X_k e^{−ikφ} conj(P_k): a forward FFT over φ's steps.
        let mut z = vec![Complex::new(0.0, 0.0); XCORR_STEPS];
        for k in 1..x.len().min(p.len()).min(XCORR_STEPS) {
            z[k] = x[k] * p[k].conj();
        }
        planner.plan_fft_forward(XCORR_STEPS).process(&mut z);
        let best = (0..XCORR_STEPS)
            .max_by(|&a, &b| z[a].re.total_cmp(&z[b].re))
            .unwrap_or(0);
        std::f64::consts::TAU * best as f64 / XCORR_STEPS as f64
    } else {
        0.0
    };
    for (k, c) in x.iter_mut().enumerate() {
        *c *= Complex::from_polar(1.0, -(k as f64) * turn);
    }
    x
}

/// Levels from a turned spectrum: nominal `top / 2` down to 16, each at
/// its stored length, keeping `wave::harmonics` (those the cycle has).
fn levels_of(x: &[Complex<f64>], top: u32) -> Vec<f32> {
    let mut planner = FftPlanner::<f64>::new();
    let mut out = Vec::new();
    let mut nl = nominal_top(top).trailing_zeros();
    while nl >= MIN_LOG2 {
        let n = 1usize << stored_log2(nl);
        let keep = (harmonics(nl) as usize).min(x.len() - 1);
        let mut y = vec![Complex::new(0.0, 0.0); n];
        for k in 1..=keep {
            y[k] = x[k];
            y[n - k] = x[k].conj();
        }
        planner.plan_fft_inverse(n).process(&mut y);
        out.extend(y.iter().map(|c| c.re as f32));
        nl -= 1;
    }
    out
}

/// `cycle`'s levels, nominal `top / 2` down to 16, one after another, each
/// at its stored length (1 216 samples for top 512, 2 240 for 1 024). Level
/// `N` is the cycle's harmonics `1..=wave::harmonics` (those it has: under
/// half its length) and nothing else, not even DC; the frame turned so
/// harmonic 1 is in sine phase, so frames cut at any point line up and a
/// scan between two never cancels their fundamentals.
pub fn levels(cycle: &[f32], top: u32) -> Vec<f32> {
    levels_of(&spectrum(cycle, None), top)
}

/// A transwave's frames, each as `levels` builds it, but a frame whose
/// harmonic 1 is more than 40 dB under its largest is turned to best
/// match the frame before (circular cross-correlation), so it lines up
/// too.
pub fn transwave(cycles: &[Vec<f32>], top: u32) -> Vec<f32> {
    let mut prev: Option<Vec<Complex<f64>>> = None;
    let mut out = Vec::new();
    for c in cycles {
        let x = spectrum(c, prev.as_deref());
        out.extend(levels_of(&x, top));
        prev = Some(x);
    }
    out
}

/// A transwave's levels to 12 bits: the whole set's peak to −0.5 dBFS,
/// rounded. A fixed single-cycle table gains nothing from dither, and its
/// noise would eat the levels' 60 dB. `None` when the levels are silent.
pub fn round12(x: &[f32]) -> Option<Vec<i16>> {
    let peak = x.iter().fold(0f32, |a, s| a.max(s.abs()));
    if peak <= 0.0 {
        return None;
    }
    let gain = 10f32.powf(-0.5 / 20.0) / peak;
    Some(
        x.iter()
            .map(|s| clampf((s * gain * 2048.0).round(), -2048.0, 2047.0) as i16)
            .collect(),
    )
}
