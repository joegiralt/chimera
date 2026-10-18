//! The filter picture's resonance peak against the engine's own response
//! (#574): one small impulse through each kind, its FFT's peak above the
//! pass band, against the drawn curve's peak above its own pass band.

mod common;

use chimera_core::dsp::filter::{Filter, FilterKind, FilterMode, FilterMods};
use chimera_core::params::FilterParams;
use chimera_core::ui::viz::{self, PLOT_TOP, Response};
use chimera_hal::BLOCK_SIZE;
use common::filter_ref::fft;

const SR: u32 = 48_000;
const FC: f32 = 1000.0;
const N: usize = 1 << 15;
/// The viz's scale: 98 px for 48 dB.
const PX_PER_DB: f32 = 98.0 / 48.0;
/// The plot's columns (x 12..=228).
const COLS: usize = 217;
/// Prototype against engine, px: 1 px is 0.5 dB.
const TOL_PX: f32 = 1.0;
/// A whole curve against the engine, px (about 1.5 dB).
const SHAPE_TOL_PX: f32 = 3.0;
/// At 8 kHz FREQ, below FREQ only: the warp's share, px (about 3 dB).
const WARP_TOL_PX: f32 = 6.0;

fn spectrum(kind: FilterKind, mode: FilterMode, fc: f32, res: f32) -> Vec<f64> {
    let mut p = FilterParams::default();
    p.set_kind(kind);
    assert!(p.set_mode(mode));
    (p.cutoff, p.resonance) = (fc, res);
    let mut f = Filter::new();
    let mut re = Vec::with_capacity(N);
    for b in 0..N / BLOCK_SIZE {
        let mut buf = [0.0f32; BLOCK_SIZE];
        if b == 0 {
            buf[0] = 0.01;
        }
        f.process(&mut buf, &p, SR, &FilterMods::NONE);
        re.extend(buf.iter().map(|&v| f64::from(v)));
    }
    let mut im = vec![0.0; N];
    fft(&mut re, &mut im);
    (0..N / 2)
        .map(|i| 20.0 * (re[i].hypot(im[i]) / 0.01).log10())
        .collect()
}

/// Gain in dB over the bins 1 Hz to 6 kHz (the plot's range at FREQ 1 kHz).
fn engine(kind: FilterKind, mode: FilterMode, res: f32) -> Vec<f64> {
    let hz = |i: usize| i as f64 * f64::from(SR) / N as f64;
    let g = spectrum(kind, mode, FC, res);
    (1..N / 2)
        .filter(|&i| hz(i) <= 6.0 * f64::from(FC))
        .map(|i| g[i])
        .collect()
}

/// (drawn pass-band y, drawn peak height in px) of `r` at FREQ plot 1/3.
fn drawn(r: Response, res: f32) -> (i32, f32) {
    let c = 1.0 / 3.0;
    let y = |i: usize| viz::response_y(i as f32 / (COLS - 1) as f32, c, res, r);
    let dc = y(0);
    let top = (0..COLS).map(y).min().unwrap();
    (dc, (dc - top) as f32)
}

#[test]
fn the_drawn_peak_is_the_engines() {
    use FilterKind as K;
    use FilterMode as M;
    let cases = [
        (K::Svf, M::Lp24, "CLEAN LP4"),
        (K::Svf, M::Lp12, "CLEAN LP2"),
        (K::Dirty, M::Lp12, "DIRTY LP2"),
        (K::Ladder, M::Lp24, "LADDER LP4"),
        (K::K2040, M::Lp24, "2040"),
    ];
    for (kind, mode, name) in cases {
        for res in [0.0, 0.3, 0.5, 0.75, 0.85, 0.9] {
            let g = engine(kind, mode, res);
            let want_db = g.iter().cloned().fold(f64::MIN, f64::max) - g[0];
            let (dc, got_px) = drawn(Response::of(kind, mode), res);
            // The plot stops at its top edge.
            let want_px = (want_db as f32 * PX_PER_DB)
                .min((dc - PLOT_TOP) as f32)
                .max(0.0);
            println!("{name} RES {res}: {want_db:.2} dB = {want_px:.1} px, drawn {got_px} px");
            assert!(
                (got_px - want_px).abs() <= TOL_PX,
                "{name} RES {res}: engine {want_px:.1} px, drawn {got_px} px"
            );
        }
    }
}

/// At RES 1 every kind rings: the peak runs off the top of the plot.
#[test]
fn the_ring_runs_off_the_plot() {
    use FilterKind as K;
    use FilterMode as M;
    for (kind, mode) in [
        (K::Svf, M::Lp24),
        (K::Dirty, M::Lp12),
        (K::Ladder, M::Lp24),
        (K::K2040, M::Lp24),
    ] {
        let r = Response::of(kind, mode);
        let (dc, h) = drawn(r, 1.0);
        assert_eq!(dc - h as i32, PLOT_TOP, "{kind:?}");
    }
}

/// A 4-pole at RES 0 has no peak, and loses its bass as RES rises.
#[test]
fn the_ladder_has_no_peak_at_res_0_and_loses_bass() {
    let r = Response::of(FilterKind::Ladder, FilterMode::Lp24);
    let (dc0, h0) = drawn(r, 0.0);
    assert_eq!((dc0, h0), (viz::FILTER_PASS_Y, 0.0));
    let (dc5, _) = drawn(r, 0.5);
    let want = 20.0 * (1.0 + 0.5 * 4.0 / 0.95f32).log10() * PX_PER_DB;
    assert!(
        ((dc5 - dc0) as f32 - want).abs() <= 1.0,
        "bass loss {} px, want {want}",
        dc5 - dc0
    );
}

#[test]
fn dirty_and_clean_share_their_q_law() {
    for r in [0.0, 0.3, 0.77, 1.0] {
        let d = chimera_core::dsp::filter::dirty::dirty_q(r);
        let c = chimera_core::dsp::filter::clean::clean_k(r);
        assert!((d - c).abs() < 1e-6);
    }
}

/// Drawn px against the engine's dB (2.04 px a dB) across the plot, for the
/// shapes with no pass band to measure from (the band-passes, the notches,
/// HP, LP1): the columns at 0.3 to 2.5 x FREQ at 1 kHz, the curve's shape
/// after the output level (DIRTY's gain, not drawn) is taken off as the
/// median difference. At 8 kHz the TPT's bilinear warp bends the skirt
/// above FREQ, which the prototype does not draw (the viz has no Hz), so
/// that case reads 0.3 to 1 x FREQ only. Columns near the plot's floor (a
/// notch's zero) are left out.
#[test]
fn the_drawn_curve_is_the_engines_across_the_plot() {
    use FilterKind as K;
    use FilterMode as M;
    let cases = [
        (K::Svf, M::Bp12, "CLEAN BP2"),
        (K::Svf, M::Bp24, "CLEAN BP4"),
        (K::Svf, M::Hp24, "CLEAN HP4"),
        (K::Svf, M::Lp6, "CLEAN LP1"),
        (K::Svf, M::Notch, "CLEAN NOTCH1"),
        (K::Dirty, M::Bp12, "DIRTY BP2"),
        (K::Dirty, M::Hp12, "DIRTY HP2"),
        (K::Dirty, M::Notch, "DIRTY NOTCH1"),
        (K::Ladder, M::Bp12, "LADDER BP2"),
        (K::Ladder, M::Bp24, "LADDER BP4"),
        (K::Ladder, M::Hp24, "LADDER HP4"),
        (K::Ladder, M::Lp12, "LADDER LP2"),
    ];
    let c = 1.0 / 3.0;
    for (fc, hi) in [(1000.0f32, 2.5), (8000.0, 1.0)] {
        for (kind, mode, name) in cases {
            // DIRTY's Chamberlin zero shifts with the warp: its notch steepens it
            // to 11 dB at 8 kHz, which the viz does not draw.
            if fc > 4000.0 && name == "DIRTY NOTCH1" {
                continue;
            }
            for res in [0.0, 0.3, 0.5] {
                let g = spectrum(kind, mode, fc, res);
                let r = Response::of(kind, mode);
                let mut diffs = Vec::new();
                for col in 0..COLS {
                    let t = col as f32 / (COLS - 1) as f32;
                    let hz = t / c * fc;
                    if !(0.3..=hi).contains(&(t / c)) {
                        continue;
                    }
                    let y = viz::response_y(t, c, res, r);
                    let want = viz::FILTER_PASS_Y as f32
                        - g[(f64::from(hz) * N as f64 / f64::from(SR)).round() as usize] as f32
                            * PX_PER_DB;
                    let edge = (PLOT_TOP + 4) as f32..(viz::PLOT_BASE - 40) as f32;
                    if edge.contains(&want) && edge.contains(&(y as f32)) {
                        diffs.push(y as f32 - want);
                    }
                }
                diffs.sort_by(|a, b| a.total_cmp(b));
                let med = diffs[diffs.len() / 2];
                let worst = diffs.iter().map(|d| (d - med).abs()).fold(0.0, f32::max);
                println!("{name} {fc} Hz RES {res}: level {med:.1} px, worst {worst:.1} px");
                let tol = if fc > 4000.0 {
                    WARP_TOL_PX
                } else {
                    SHAPE_TOL_PX
                };
                assert!(worst <= tol, "{name} {fc} Hz RES {res}: {worst} px");
            }
        }
    }
}
