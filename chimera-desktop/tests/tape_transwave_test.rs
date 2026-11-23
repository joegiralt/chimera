// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The importer's transwave (TAPE spec § 6 step 5): whole cycles cut from
//! the input, each built into octave levels by FFT truncation and held
//! oversampled. Precedent: PPG Wave and Waldorf Microwave (band-limited
//! copies per octave).

use chimera_core::dsp::tape_engine::wave::{
    MIN_LOG2, harmonics, level_offset, nominal_top, stored_log2,
};
use chimera_desktop::import::tape::transwave::{
    DEFAULT_TOP, MAX_FRAMES, cut_cycles, levels, transwave,
};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

/// Magnitude per bin, 0..=n/2, of `x`.
fn spectrum(x: &[f32]) -> Vec<f64> {
    let mut b: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    FftPlanner::new().plan_fft_forward(b.len()).process(&mut b);
    b[..=x.len() / 2].iter().map(|c| c.norm()).collect()
}

/// A cycle with energy in every harmonic: a saw plus seeded noise, 301
/// samples long (not a power of two, as a real cycle isn't).
fn rich_cycle() -> Vec<f32> {
    let mut s = 0x5EED_u32;
    (0..301)
        .map(|i| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (i as f32 / 301.0 - 0.5) + 0.2 * (s as f32 / u32::MAX as f32 - 0.5)
        })
        .collect()
}

/// Each level's FFT at its stored length: no DC, nothing above its nominal
/// harmonics (⌊0.35 N⌋, 42 at 128; under −100 dB of its largest bin), and each kept harmonic at
/// the cycle's own magnitude.
#[test]
fn levels_hold_no_harmonic_above_limit() {
    let c = rich_cycle();
    let src = spectrum(&c);
    for top in [512u32, 1_024] {
        let lv = levels(&c, top);
        for nl in (MIN_LOG2..=nominal_top(top).trailing_zeros()).rev() {
            let (off, n) = (level_offset(top, nl) as usize, 1usize << stored_log2(nl));
            let sp = spectrum(&lv[off..off + n]);
            let peak = sp.iter().fold(0.0f64, |m, &x| m.max(x));
            let limit = harmonics(nl) as usize;
            assert!(sp[0] / peak < 1e-5, "top {top}, level {nl}: DC");
            for (k, &m) in sp.iter().enumerate().skip(limit + 1) {
                let db = 20.0 * (m / peak).log10();
                assert!(
                    db < -100.0,
                    "top {top}, level {nl}: harmonic {k} at {db:.1} dB"
                );
            }
            for k in 1..=limit.min(c.len() / 2) {
                let (want, got) = (src[k] / c.len() as f64, sp[k] / n as f64);
                assert!(
                    (got - want).abs() < 1e-4,
                    "top {top}, level {nl}, harmonic {k}: {got} against {want}"
                );
            }
        }
    }
}

/// 1 216 samples a frame by default (64 + 128 + 256 + 256 + 512), 2 240 at
/// top 1 024 (+ 1 024).
#[test]
fn level_sizes() {
    let c = rich_cycle();
    assert_eq!(DEFAULT_TOP, 512);
    assert_eq!(levels(&c, DEFAULT_TOP).len(), 1_216);
    assert_eq!(levels(&c, 1_024).len(), 2_240);
}

/// sin φ + 0.8 sin 2φ, cut at two places 0.37 of a cycle apart: their
/// levels come out the same, so a scan half way between them never
/// cancels the fundamental.
#[test]
fn frames_line_up() {
    let cut = |at: f64| -> Vec<f32> {
        (0..301)
            .map(|i| {
                let x = std::f64::consts::TAU * (i as f64 / 301.0 + at);
                (x.sin() + 0.8 * (2.0 * x).sin()) as f32 * 0.5
            })
            .collect()
    };
    let (a, b) = (levels(&cut(0.0), 512), levels(&cut(0.37), 512));
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() < 1e-4, "{x} against {y}");
    }
    let n = 1usize << stored_log2(8);
    let mid: Vec<f32> = a[..n]
        .iter()
        .zip(&b[..n])
        .map(|(x, y)| 0.5 * (x + y))
        .collect();
    let (m1, a1) = (spectrum(&mid)[1], spectrum(&a[..n])[1]);
    assert!(m1 > 0.99 * a1, "half way: {m1} against {a1}");
}

/// Whole cycles from across the input, at most `MAX_FRAMES` (32): each
/// exactly one cycle long, so its spectrum is harmonic 1 alone (60 dB
/// over the rest), starting at a rising zero crossing.
#[test]
fn cut_cycles_takes_whole_cycles() {
    let period = 109.37f64;
    let x: Vec<f32> = (0..48_000)
        .map(|i| (std::f64::consts::TAU * (i as f64 + 17.0) / period).sin() as f32)
        .collect();
    let cycles = cut_cycles(&x, period as f32, MAX_FRAMES);
    assert_eq!(MAX_FRAMES, 32);
    assert_eq!(cycles.len(), 32);
    for c in &cycles {
        assert_eq!(c.len(), period.ceil() as usize);
        let sp = spectrum(c);
        for (k, &m) in sp.iter().enumerate().skip(2) {
            let db = 20.0 * (m / sp[1]).log10();
            assert!(db < -60.0, "harmonic {k} at {db:.1} dB");
        }
        assert!(
            c[0] >= 0.0 && c[0] < 0.1 && c[1] > c[0],
            "rising: {:?}",
            &c[..2]
        );
    }
    // Fewer cycles in the input, fewer frames.
    assert_eq!(cut_cycles(&x[..1_000], period as f32, MAX_FRAMES).len(), 5);
    assert!(cut_cycles(&x[..400], period as f32, MAX_FRAMES).is_empty());
    assert!(cut_cycles(&x, 0.0, MAX_FRAMES).is_empty());
}

/// A frame whose fundamental is 50 dB under its 2nd harmonic can't say its
/// phase by harmonic 1: cut 0.37 of a cycle on, with its weak fundamental
/// at a phase of its own, it is turned to match the frame before by
/// cross-correlation, so their strong harmonics line up and a scan half
/// way between them keeps them whole.
#[test]
fn weak_fundamental_aligns_by_correlation() {
    let cut = |at: f64, h1: f64| -> Vec<f32> {
        (0..301)
            .map(|i| {
                let x = std::f64::consts::TAU * (i as f64 / 301.0 + at);
                let y = 0.00316 * (x + h1).sin() + (2.0 * x).sin() + 0.5 * (3.0 * x).sin();
                (0.4 * y) as f32
            })
            .collect()
    };
    let w = transwave(&[cut(0.0, 0.0), cut(0.37, 2.1)], 512);
    let f = 1_216;
    let n = 1usize << stored_log2(8);
    let (a, b) = (&w[..n], &w[f..f + n]);
    let (sa, sb) = (spectrum(a), spectrum(b));
    let mid: Vec<f32> = a.iter().zip(b).map(|(x, y)| 0.5 * (x + y)).collect();
    let sm = spectrum(&mid);
    for k in [2, 3] {
        assert!((sa[k] - sb[k]).abs() < 1e-3 * sa[k], "harmonic {k}");
        assert!(
            sm[k] > 0.995 * sa[k],
            "harmonic {k} half way: {} against {}",
            sm[k],
            sa[k]
        );
    }
}
