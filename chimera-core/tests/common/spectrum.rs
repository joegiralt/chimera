// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spectral measures for oscillator tests (VA plan, "Decided here"): the
//! alias gate, the pitch method, the fundamental and the click reference.
//! All in f64.

use std::f64::consts::TAU;

pub use super::filter_ref::fft;

const SETTLE: usize = 1 << 12;
const GATE_N: usize = 1 << 15;
const PITCH_N: usize = 1 << 17;
const MASK_BINS: f64 = 4.0;
const BAND: (f64, f64) = (20.0, 10_000.0);

/// Power per bin 1..n/2 of the gate window (2¹⁵ samples after a 2¹²
/// settle) under a 4-term Blackman–Harris window.
fn gate_power(x: &[f32]) -> Vec<f64> {
    let x = &x[SETTLE..SETTLE + GATE_N];
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let a = TAU * i as f64 / (GATE_N - 1) as f64;
            let w =
                0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos();
            f64::from(v) * w
        })
        .collect();
    let mut im = vec![0.0; GATE_N];
    fft(&mut re, &mut im);
    (0..GATE_N / 2)
        .map(|k| re[k] * re[k] + im[k] * im[k])
        .collect()
}

/// Unmasked power in 20 Hz–10 kHz over the total, dB.
fn gate(x: &[f32], sample_rate: f32, masked: impl Fn(f64) -> bool) -> f64 {
    let p = gate_power(x);
    let bin = f64::from(sample_rate) / GATE_N as f64;
    let (mut total, mut alias) = (0.0, 0.0);
    for (k, &pw) in p.iter().enumerate().skip(1) {
        total += pw;
        let f = k as f64 * bin;
        if f >= BAND.0 && f <= BAND.1 && !masked(f) {
            alias += pw;
        }
    }
    10.0 * (alias / total).log10()
}

/// The alias gate: everything but ±4 bins of each multiple of each `f0s`.
pub fn alias_db(x: &[f32], sample_rate: f32, f0s: &[f32]) -> f64 {
    let bin = f64::from(sample_rate) / GATE_N as f64;
    gate(x, sample_rate, |f| {
        f0s.iter().any(|&f0| {
            let f0 = f64::from(f0);
            let h = (f / f0).round();
            h >= 1.0 && ((f - h * f0) / bin).abs() <= MASK_BINS
        })
    })
}

/// The gate with ±4 bins of every |i·f1 + j·f2| masked, |i|, |j| up to
/// ⌈24 kHz / min(f1, f2)⌉ + 1.
pub fn alias_db_lattice(x: &[f32], sample_rate: f32, f1: f32, f2: f32) -> f64 {
    let bin = f64::from(sample_rate) / GATE_N as f64;
    let (f1, f2) = (f64::from(f1), f64::from(f2));
    let n = (24_000.0 / f1.min(f2)).ceil() as i64 + 1;
    let mut mask = vec![false; GATE_N / 2];
    for i in -n..=n {
        for j in -n..=n {
            let c = (i as f64 * f1 + j as f64 * f2).abs() / bin;
            let lo = (c - MASK_BINS).ceil().max(0.0) as usize;
            let hi = ((c + MASK_BINS).floor() as usize).min(GATE_N / 2 - 1);
            for m in mask.iter_mut().take(hi + 1).skip(lo) {
                *m = true;
            }
        }
    }
    gate(x, sample_rate, |f| mask[(f / bin).round() as usize])
}

/// The strongest frequency within ±10 % of `near`: a 2¹⁷-point Hann DFT
/// of `x`'s first 2¹⁷ samples, quadratic interpolation on log magnitude.
pub fn peak_hz(x: &[f32], sample_rate: f32, near: f32) -> f64 {
    let mut re: Vec<f64> = x[..PITCH_N]
        .iter()
        .enumerate()
        .map(|(i, &v)| f64::from(v) * (0.5 - 0.5 * (TAU * i as f64 / PITCH_N as f64).cos()))
        .collect();
    let mut im = vec![0.0; PITCH_N];
    fft(&mut re, &mut im);
    let bin = f64::from(sample_rate) / PITCH_N as f64;
    let mag = |k: usize| (re[k] * re[k] + im[k] * im[k]).sqrt().ln();
    let near = f64::from(near);
    let (lo, hi) = (
        (0.9 * near / bin) as usize,
        ((1.1 * near / bin) as usize).min(PITCH_N / 2 - 2),
    );
    let k = (lo.max(1)..=hi)
        .max_by(|&a, &b| mag(a).total_cmp(&mag(b)))
        .unwrap();
    let (a, b, c) = (mag(k - 1), mag(k), mag(k + 1));
    (k as f64 + 0.5 * (a - c) / (a - 2.0 * b + c)) * bin
}

/// The `f0` bin of `x` scaled to amplitude, `(2/N)·Σ x·e^(−iωn)`: a sine
/// `A·sin ωn` reads `(0, −A)`, so its in-phase part is `−im`.
pub fn fundamental(x: &[f32], sr: f32, f0: f32) -> (f32, f32) {
    let w = TAU * f64::from(f0) / f64::from(sr);
    let (re, im) = x.iter().enumerate().fold((0.0, 0.0), |(r, i), (n, &v)| {
        let (s, c) = (w * n as f64).sin_cos();
        (r + f64::from(v) * c, i - f64::from(v) * s)
    });
    let k = 2.0 / x.len() as f64;
    ((re * k) as f32, (im * k) as f32)
}

/// The continuous signal `f` through the 2-point kernel's own triangle
/// filter, one sample late: element k is ∫ f(k − 1 + u)·(1 − |u|) du over
/// [−1, 1], a 2 048-point midpoint sum.
pub fn tri_reference(f: impl Fn(f64) -> f64, n: usize) -> Vec<f64> {
    const POINTS: usize = 2048;
    let du = 2.0 / POINTS as f64;
    (0..n)
        .map(|k| {
            (0..POINTS)
                .map(|j| {
                    let u = -1.0 + (j as f64 + 0.5) * du;
                    f(k as f64 - 1.0 + u) * (1.0 - u.abs()) * du
                })
                .sum()
        })
        .collect()
}
