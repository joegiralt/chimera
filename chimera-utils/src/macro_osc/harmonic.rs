// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! HARMONIC: additive, 24 sine partials at whole multiples of the pitch.
//! HARMONICS moves a bump's peak over the partials (log-spaced: 1 at 0,
//! 24 at 1), TIMBRE widens it (from one partial to a flat, bright comb),
//! MORPH masks the partials: 0 every one, ½ odd only (hollow), 1 the
//! organ's drawbar set (1 2 3 4 6 8 10 12 16, the 16' to 1' footages
//! over a 16' fundamental). Level held at VA's RMS, under a peak ceiling.
//!
//! Partials past 0.40 fs fade out linearly, silent from 0.45 fs: nothing
//! folds back, and a glide never clicks a partial in or out.
//!
//! Per sample: the fundamental's sine and cosine from a quadrature phasor
//! (rotated by `e^{iω}` a sample, four multiplies; `ω` and the phasor's
//! renormalisation once a block: exact where a LUT's error, multiplied up
//! the recurrence, left a −79 dB floor above Nyquist), then each
//! partial from the two below it by the Chebyshev recurrence
//! `sin kθ = 2 cos θ · sin (k−1)θ − sin (k−2)θ` (one multiply-add) and two
//! multiply-adds into the block's start and end amplitude sums, lerped:
//! amplitudes move per block without a store a partial a sample. About
//! 3 FPU ops a partial: ~24 × 3.5 + 15 ≈ 100 M7 cycles a voice at full
//! count; partials above Nyquist are not run, so high notes cost less.
//!
//! Debt to Plaits (structure only): an additive bank of integer harmonics
//! by the recurrence, shaped by a movable bump. Plaits' control laws (bump
//! count, peak index, shape) are not ours: here HARMONICS is the peak,
//! TIMBRE the width and MORPH the odd/even/drawbar mask.

use super::{B, Blk, Ctl, FS, Model};
use crate::drums::parts::expo;

/// Partials. 24 reaches 3.1 kHz on C3, 6.3 kHz on C4: bright enough for
/// leads and pads, and ~110 cycles a voice keeps seven voices near 15 %
/// of the M7.
pub const N: usize = 24;
/// Fade-out start and silence, in Hz.
const FADE: f32 = 0.40 * FS;
pub const TOP: f32 = 0.45 * FS;
/// VA's oscillator RMS (`va::osc::R`).
const RMS: f32 = chimera_core::dsp::va::osc::R;
/// Σ|a| never above: the peak bound, whatever the phases.
const PEAK: f32 = 0.9;

fn drawbar(k: usize) -> f32 {
    matches!(k, 1 | 2 | 3 | 4 | 6 | 8 | 10 | 12 | 16) as u8 as f32
}

/// The partials' amplitudes at `hz` and `c`, level-normalised.
pub fn amplitudes(hz: f32, c: Ctl) -> [f32; N] {
    let centre = c.harmonics.clamp(0.0, 1.0) * (N as f32).log2();
    // bump σ in octaves of partial number: one partial to flat
    let w = expo(c.timbre.clamp(0.0, 1.0), 0.12, 4.0);
    let m = c.morph.clamp(0.0, 1.0);
    let mut a = [0.0f32; N];
    for (i, a) in a.iter_mut().enumerate() {
        let k = i + 1;
        let d = ((k as f32).log2() - centre) / w;
        let bump = (-0.5 * d * d).exp();
        let odd = (k % 2 == 1) as u8 as f32;
        let mask = if m <= 0.5 {
            odd + (1.0 - odd) * (1.0 - 2.0 * m)
        } else {
            odd + (drawbar(k) - odd) * (2.0 * m - 1.0)
        };
        let f = k as f32 * hz;
        let band = ((TOP - f) / (TOP - FADE)).clamp(0.0, 1.0);
        *a = bump * mask * band;
    }
    let ss: f32 = a.iter().map(|x| x * x).sum();
    let sa: f32 = a.iter().sum();
    if ss <= 1e-20 {
        return [0.0; N];
    }
    let g = (RMS / (0.5 * ss).sqrt()).min(PEAK / sa);
    a.map(|x| x * g)
}

pub struct Harmonic {
    /// The fundamental's phasor: (cos, sin).
    z: (f32, f32),
    amp: [f32; N],
}

impl Harmonic {
    pub fn new() -> Self {
        Self {
            z: (1.0, 0.0),
            amp: [0.0; N],
        }
    }
}

impl Model for Harmonic {
    fn block(&mut self, out: &mut Blk, hz: f32, c: Ctl) {
        let to = amplitudes(hz, c);
        let from = self.amp;
        // partials silent at both ends are not run
        let live = (0..N)
            .rev()
            .find(|&i| from[i] != 0.0 || to[i] != 0.0)
            .map_or(0, |i| i + 1);
        let w = core::f32::consts::TAU * (hz / FS).clamp(0.0, 0.5);
        let (wc, ws) = (w.cos(), w.sin());
        let (mut zc, mut zs) = self.z;
        let n = 1.0 / (zc * zc + zs * zs).sqrt();
        (zc, zs) = (zc * n, zs * n);
        for (s, o) in out.iter_mut().enumerate() {
            (zc, zs) = (zc * wc - zs * ws, zc * ws + zs * wc);
            let (sn, c2) = (zs, 2.0 * zc);
            let (mut p, mut q) = (0.0f32, sn);
            let (mut y0, mut y1) = (0.0f32, 0.0f32);
            for k in 0..live {
                y0 += from[k] * q;
                y1 += to[k] * q;
                (p, q) = (q, c2 * q - p);
            }
            let t = (s + 1) as f32 / B as f32;
            *o = y0 + (y1 - y0) * t;
        }
        self.z = (zc, zs);
        self.amp = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::{FftPlanner, num_complex::Complex};

    #[test]
    fn no_partial_past_045_fs_and_no_energy_above_it() {
        let c = Ctl::new(1.0, 1.0, 0.0);
        for hz in [440.0, 1500.0, 2093.0, 5000.0] {
            let a = amplitudes(hz, c);
            for (i, a) in a.iter().enumerate() {
                assert!(
                    *a == 0.0 || (i + 1) as f32 * hz < TOP,
                    "{hz}: partial {}",
                    i + 1
                );
            }
            let mut m = Harmonic::new();
            let n = 1 << 15;
            let mut x = Vec::with_capacity(n + B);
            let mut b: Blk = [0.0; B];
            while x.len() < n + B * 4 {
                m.block(&mut b, hz, c);
                x.extend_from_slice(&b);
            }
            let x = &x[B * 4..B * 4 + n];
            let mut buf: Vec<Complex<f32>> = x
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let w = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos();
                    Complex::new(s * w, 0.0)
                })
                .collect();
            FftPlanner::new().plan_fft_forward(n).process(&mut buf);
            let bin_hz = FS / n as f32;
            let (mut lo, mut hi) = (0.0f64, 0.0f64);
            for (i, v) in buf[..n / 2].iter().enumerate() {
                let e = v.norm_sqr() as f64;
                if i as f32 * bin_hz > 0.46 * FS {
                    hi += e;
                } else {
                    lo += e;
                }
            }
            let db = 10.0 * (hi / lo).log10();
            assert!(db < -80.0, "{hz} Hz: {db:.1} dB above 0.46 fs");
        }
    }
}
