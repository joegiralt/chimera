// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The labs' comparison measures, from the 2040 lab (`circuit2040.rs`):
//! level, third-octave bands, LSD and the null. Every one reads raw
//! levels; none matches gain first unless it says so.

use crate::FS;
use std::f64::consts::PI;

/// RMS level, dB re 1.
pub fn rms_db(x: &[f64]) -> f64 {
    let e = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    10.0 * e.max(1e-30).log10()
}

/// Third-octave band levels, 40 Hz – 16 kHz (dB).
pub fn bands(x: &[f64]) -> Vec<f64> {
    let n = x.len().next_power_of_two();
    let mut v = x.to_vec();
    v.resize(n, 0.0);
    let s = super::sig::fft(&v);
    let mut out = Vec::new();
    let mut f = 40.0f64;
    while f < 16_000.0 {
        let (lo, hi) = (f / 2f64.powf(1.0 / 6.0), f * 2f64.powf(1.0 / 6.0));
        let (a, b) = (
            (lo / FS * n as f64) as usize,
            ((hi / FS * n as f64) as usize).max(1),
        );
        let e: f64 = s[a..b.min(n / 2)].iter().map(|c| c.norm_sqr()).sum();
        out.push(10.0 * (e + 1e-20).log10());
        f *= 2f64.powf(1.0 / 3.0);
    }
    out
}

/// Mean |Δ| of third-octave levels, over bands within 50 dB of the loudest.
pub fn lsd(x: &[f64], r: &[f64]) -> f64 {
    let (bx, br) = (bands(x), bands(r));
    let top = br.iter().cloned().fold(f64::MIN, f64::max);
    let d: Vec<f64> = bx
        .iter()
        .zip(&br)
        .filter(|(_, b)| **b > top - 50.0)
        .map(|(a, b)| (a - b).abs())
        .collect();
    d.iter().sum::<f64>() / d.len().max(1) as f64
}

/// The residual `x − r` against `r`, dB.
pub fn null_db(x: &[f64], r: &[f64]) -> f64 {
    let d: Vec<f64> = x.iter().zip(r).map(|(a, b)| a - b).collect();
    rms_db(&d) - rms_db(r)
}

/// Amplitude in dB re 1.
pub fn db(v: f64) -> f64 {
    20.0 * v.max(1e-12).log10()
}

/// `|X(f)|` of the Hann-windowed `x`, normalised so a sine of amplitude 1
/// reads 1.
pub fn tone(x: &[f64], f: f64) -> f64 {
    let n = x.len();
    let (mut re, mut im, mut ws) = (0.0, 0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos();
        let ph = 2.0 * PI * f * i as f64 / FS;
        re += w * v * ph.cos();
        im -= w * v * ph.sin();
        ws += w;
    }
    2.0 * (re * re + im * im).sqrt() / ws
}
