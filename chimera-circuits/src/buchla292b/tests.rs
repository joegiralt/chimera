// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The lpg-circuit lab's checks as assertions: `validate()` (§1, §4–6 of
//! `docs/lpg/validation.md`, cell VTL5C3/2 as the lab ran it), the ping
//! timelines (`docs/lpg/levels.md`, VTL5C3/2) and the gate checks
//! (`docs/lpg/gate.md`, cell VTL5C3).

use super::core::{Core, f3, h_mag};
use super::driver::{IZT, VZK, i_rail, static_current, static_table, threshold, v_req};
use super::fw::{Fw, SUB};
use super::parts::{B292, Mode, PD13, Parts};
use super::reference::{OS, Trace, run_48, run_ref};
use super::stim::{Checks, Env, Wave, centroid, checks, perc, tone};
use super::{CV_FS, VREF};
use crate::FS;
use crate::util::measure::{db, null_db, rms_db};
use crate::vtl5c3::{Cell, Light, VTL5C3, VTL5C3_2};
use std::f64::consts::PI;

fn near(got: f64, want: f64, rel: f64) -> bool {
    (got / want - 1.0).abs() <= rel
}

/// Steady sine gain of the core alone at fixed Rf, `os`× 48 kHz.
fn sine_gain(parts: Parts, mode: Mode, rf: f64, f: f64, os: usize) -> f64 {
    let h = 1.0 / (FS * os as f64);
    let mut c = Core::new(parts, mode, h);
    let n = (0.25 / h) as usize;
    let amp = 1e-3;
    let mut acc = (0.0, 0.0);
    let w = 2.0 * PI * f;
    let start = n / 2;
    let periods = ((n - start) as f64 * h * f).floor();
    let stop = start + (periods / f / h) as usize;
    for k in 0..stop {
        let t = k as f64 * h;
        let y = c.step(amp * (w * t).sin(), 1.0 / rf);
        if k >= start {
            acc.0 += y * (w * t).sin();
            acc.1 += y * (w * t).cos();
        }
    }
    let m = (stop - start) as f64;
    2.0 * (acc.0 * acc.0 + acc.1 * acc.1).sqrt() / m / amp
}

#[test]
fn tpt_core_at_48k_meets_the_ledger() {
    // validation.md §1, the "48 kHz TPT" column (P&D Table 1, a = 1): the
    // same equations stepped at 48 kHz, ≤ 5 kHz.
    for (mode, rf, f, want) in [
        (Mode::Combo, 10e3, 5000.0, -1.72),
        (Mode::Combo, 100e3, 1000.0, -4.57),
        (Mode::Gate, 10e3, 1000.0, -13.98),
        (Mode::Gate, 100e3, 5000.0, -32.92),
        (Mode::Lowpass, 10e3, 5000.0, 1.19),
        (Mode::Lowpass, 100e3, 1000.0, -4.70),
    ] {
        let got = db(sine_gain(PD13, mode, rf, f, 1));
        assert!((got - want).abs() < 0.03, "{mode:?} {rf} {f}: {got}");
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn reference_core_meets_pd_eq7() {
    // validation.md §1: worst 8× error against eq. 7 ≤ 5 kHz 0.010 dB,
    // three modes, Rf 1 k–1 M.
    let mut worst: f64 = 0.0;
    for mode in [Mode::Combo, Mode::Gate, Mode::Lowpass] {
        for rf in [1e3, 10e3, 100e3, 1e6] {
            for f in [100.0, 1000.0, 5000.0] {
                let want = db(h_mag(&PD13, mode, rf, 1.0, f));
                let got = db(sine_gain(PD13, mode, rf, f, OS));
                worst = worst.max((got - want).abs());
            }
        }
    }
    assert!(worst < 0.02, "worst {worst} dB");
}

#[test]
fn dc_gain_and_a_max_meet_pd() {
    // validation.md §1: eq. 12 Rα/(Rα + 2Rf) exact to 0.01 dB, 1 k–10 M;
    // eq. 11 a_max 1.473 / 1.493 / 1.682 / 3.566.
    for (rf, amax_want) in [(1e3, 1.473), (100e3, 1.493), (1e6, 1.682), (10e6, 3.566)] {
        let want = db(PD13.r_alpha / (PD13.r_alpha + 2.0 * rf));
        let mut c = Core::new(PD13, Mode::Combo, 1.0 / FS);
        let mut y = 0.0;
        for _ in 0..(0.5 * FS) as usize {
            y = c.step(1e-3, 1.0 / rf);
        }
        let got = db(y / 1e-3);
        assert!((got - want).abs() < 0.01, "{rf}: {got} against {want}");
        let amax = (2.0 * PD13.c_out * PD13.r_alpha
            + (PD13.c_mid + PD13.c_fb) * (PD13.r_alpha + rf))
            / (PD13.c_fb * PD13.r_alpha);
        assert!((amax - amax_want).abs() < 0.001, "{rf}: a_max {amax}");
        // LOWPASS is stable with the 292B's follower (a = 0.957 < a_max).
        assert!(Core::follower_gain(&B292) < amax);
    }
}

#[test]
fn network_responses_meet_the_ledger() {
    // validation.md §1: COMBO's slope −5.9 dB/oct between 2× and 8× its
    // −3 dB (P&D: "closer to −6 than −12"); the 292B against Table 1.
    for rf in [3e3, 30e3, 300e3] {
        let f0 = f3(&PD13, Mode::Combo, rf, 1.0);
        let s = db(h_mag(&PD13, Mode::Combo, rf, 1.0, 8.0 * f0)
            / h_mag(&PD13, Mode::Combo, rf, 1.0, 2.0 * f0))
            / 2.0;
        assert!((s + 5.9).abs() < 0.06, "{rf}: {s}");
    }
    let a = Core::follower_gain(&B292);
    assert!((a - 0.9566).abs() < 1e-4, "follower {a}");
    for (mode, rf, pd_f3, b_f3, pd_dc, b_dc) in [
        (Mode::Combo, 2e3, 37546.0, 41048.0, -0.0, -0.0),
        (Mode::Combo, 200e3, 405.0, 483.0, -0.7, -1.5),
        (Mode::Combo, 2e6, 67.0, 113.0, -5.1, -9.0),
        (Mode::Gate, 20e3, 28591.0, 14463.0, -19.1, -11.3),
        (Mode::Gate, 2e6, 722.0, 718.0, -58.1, -48.6),
        (Mode::Lowpass, 20e3, 4570.0, 4667.0, -0.1, -0.2),
        (Mode::Lowpass, 2e6, 48.0, 44.0, -5.1, -9.0),
    ] {
        let (p, b) = (f3(&PD13, mode, rf, 1.0), f3(&B292, mode, rf, a));
        assert!(
            near(p, pd_f3, 0.01) && near(b, b_f3, 0.01),
            "{mode:?} {rf}: {p} {b}"
        );
        let (p, b) = (
            db(h_mag(&PD13, mode, rf, 1.0, 0.0)),
            db(h_mag(&B292, mode, rf, a, 0.0)),
        );
        assert!(
            (p - pd_dc).abs() < 0.06 && (b - b_dc).abs() < 0.06,
            "{mode:?} {rf}: {p} {b}"
        );
    }
}

#[test]
fn led_driver_meets_the_ledger() {
    // validation.md §4 (VTL5C3/2): the static current, Rf and COMBO −3 dB
    // against CV at OFFSET 0; the 0.77 V threshold; 67.6 µA/V by hand;
    // the zener fit 23 Ω / 1880 Ω; 15.4 mA at the rail with the top at −3.9 V.
    let l = Light::new(VTL5C3_2);
    let a = Core::follower_gain(&B292);
    for (cv, top, ma, rf, f) in [
        (0.0, 0.22, 0.0, 10.00e6, 66.0),
        (2.0, -0.26, 0.087, 1.60e6, 123.0),
        (4.0, -0.75, 0.238, 398.9e3, 279.0),
        (8.0, -1.72, 0.676, 94.0e3, 946.0),
        (12.0, -2.68, 2.255, 18.8e3, 4441.0),
        (15.0, -3.41, 7.988, 4.6e3, 17885.0),
        (16.0, -3.65, 12.789, 3.2e3, 25694.0),
    ] {
        assert!(
            (v_req(cv, 0.0) - top).abs() < 0.006,
            "{cv} V: top {}",
            v_req(cv, 0.0)
        );
        let i = static_current(cv, 0.0);
        assert!(
            (i * 1e3 - ma).abs() <= 0.002 + 0.005 * ma,
            "{cv} V: {} mA",
            i * 1e3
        );
        let r = l.r_static(i);
        assert!(near(r, rf, 0.015), "{cv} V: R {r}");
        let got = f3(&B292, Mode::Combo, r, a);
        assert!(near(got, f, 0.015), "{cv} V: −3 dB {got}");
    }
    // The cached table at OFFSET k·0.25 V is the same law (R2 = R3).
    let t = static_table();
    assert!(near(t[32], static_current(8.0, 0.0), 1e-3));
    let th = threshold();
    assert!((th - 0.77).abs() < 0.01, "threshold {th}");
    let slope = (super::driver::CR4 / super::driver::CR2 / super::driver::CR5
        + 1.0 / super::driver::CR2)
        * 1e6;
    assert!((slope - 67.6).abs() < 0.05, "{slope}");
    assert!((VZK / IZT - 23.0).abs() < 0.6 && (VZK / 0.25e-3 - 1880.0).abs() < 1.0);
    assert!((i_rail(-3.9) * 1e3 - 15.4).abs() < 0.05, "{}", i_rail(-3.9));
}

/// Total harmonic distortion of the reference GATE path at full light
/// (Rf 2 kΩ), 94 Hz bin-exact at the 8× rate: (out peak V, THD, H2, H3 dB).
fn thd(amp: f64) -> (f64, f64, f64, f64) {
    let n = 1 << 15;
    let h = 1.0 / (FS * OS as f64);
    let mut c = Core::new(B292, Mode::Gate, h);
    let f0 = FS * OS as f64 / n as f64 * 8.0;
    let total = 3 * n;
    let mut y = Vec::with_capacity(n);
    for k in 0..total {
        let v = c.step(amp * VREF * (2.0 * PI * f0 * k as f64 * h).sin(), 1.0 / 2e3);
        if k >= total - n {
            y.push(v);
        }
    }
    let b = crate::util::sig::fft(&y);
    let fund = b[8].norm();
    let harm: f64 = (2..10).map(|k| b[8 * k].norm_sqr()).sum::<f64>().sqrt();
    (
        2.0 * fund / n as f64,
        db(harm / fund),
        db(b[16].norm() / fund),
        db(b[24].norm() / fund),
    )
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn follower_and_rails_distort_as_the_ledger() {
    // validation.md §5: 2.5 V peak in → 4.23 V out, THD −57.8 dB (H2
    // −57.8, H3 −81.7); 10 V in → 14.57 V, THD −21.1 (H2 −46.8, H3 −21.3).
    let (o, t, h2, _) = thd(1.0);
    assert!((o - 4.23).abs() < 0.02 && (t + 57.8).abs() < 0.5 && (h2 + 57.8).abs() < 0.5);
    let (o, t, h2, h3) = thd(4.0);
    assert!(
        (o - 14.57).abs() < 0.05 && (t + 21.1).abs() < 0.3,
        "{o} {t}"
    );
    assert!(
        (h2 + 46.8).abs() < 0.5 && (h3 + 21.3).abs() < 0.3,
        "{h2} {h3}"
    );
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn oversampling_converges() {
    // validation.md §6: a ping on a C4 bar, COMBO, 8× against the same
    // equations at 48 kHz: −44.0 dB null with the lab's engine-rendered
    // bar. Ours (a sine and its fourth partial) stands in for it: < −40.
    let l = Light::new(VTL5C3_2);
    let x = tone(Wave::Bar, 261.63, (0.6 * FS) as usize);
    let n = x.len();
    let cv: Vec<f64> = (0..n).map(|i| if i < 144 { CV_FS } else { 0.0 }).collect();
    let y8 = run_ref(B292, Mode::Combo, &l, &x, &cv, &vec![0.0; n], 1.0, None);
    let y1 = run_48(B292, Mode::Combo, &l, &x, &cv, 0.0);
    let null = null_db(&y1, &y8);
    assert!(null < -40.0, "null {null}");
}

/// The firmware shape, a block at a time, its LED current and conductance
/// after each block.
fn fw_run(mode: Mode, cell: Cell, x: &[f64], cv: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let l = Light::new(cell);
    let mut fw = Fw::new(&B292, mode, &l);
    let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    let off = [0.0; SUB];
    let (mut y, mut i, mut g) = (Vec::new(), Vec::new(), Vec::new());
    for (xb, cb) in xf.chunks(SUB).zip(cv.chunks(SUB)) {
        y.extend(
            fw.run(xb, cb, &off[..xb.len()], 1.0)
                .into_iter()
                .map(f64::from),
        );
        i.push(f64::from(fw.i));
        g.push(f64::from(fw.g));
    }
    (y, i, g)
}

const MARKS_MS: [f64; 14] = [
    0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 25.0, 50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 2400.0,
];

fn pulse(v: f64, n: usize) -> Vec<f64> {
    let w = (0.003 * FS) as usize;
    (0..n).map(|i| if i < w { v } else { 0.0 }).collect()
}

#[test]
fn fw_ping_timeline_meets_the_ledger() {
    // levels.md, "Ping, 15 V / 7.5 V for 3 ms (COMBO)", the fw columns
    // (VTL5C3/2): I_LED 7.99 / 0.60 mA during the pulse; R at each mark.
    let n = (2.5 * FS) as usize;
    for (v, ma, rs) in [
        (
            15.0,
            7.99,
            [
                8.7e3, 7.2e3, 6.2e3, 6.0e3, 7.4e3, 12.7e3, 46.5e3, 152.6e3, 393.2e3, 843.1e3,
                1.63e6, 2.85e6, 4.47e6, 5.49e6,
            ],
        ),
        (
            7.5,
            0.60,
            [
                17.7e3, 18.2e3, 19.3e3, 20.7e3, 24.0e3, 35.3e3, 88.5e3, 205.9e3, 444.9e3, 890.0e3,
                1.67e6, 2.88e6, 4.49e6, 5.50e6,
            ],
        ),
    ] {
        let (_, i, g) = fw_run(Mode::Combo, VTL5C3_2, &vec![0.0; n], &pulse(v, n));
        assert!((i[(0.001 * FS) as usize / SUB] * 1e3 - ma).abs() < 0.01);
        for (ms, want) in MARKS_MS.iter().zip(rs) {
            let b = ((ms * 1e-3 * FS) as usize).min(n - 1) / SUB;
            let r = 1.0 / g[b];
            assert!(near(r, want, 0.015), "{v} V at {ms} ms: R {r}");
        }
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn reference_ping_timeline_meets_the_ledger() {
    // levels.md, the ref columns (VTL5C3/2, COMBO): R at each mark; the
    // fw's −3 dB against the ref's: notes.md says "within 1 % from 3 ms
    // on" at 15 V, but its own table has 2.1 % at 3 ms and 2.4 % at 5 ms
    // (14073 / 13773 Hz, 11351 / 11080 Hz), which this reproduces: 2.5 %.
    // Within 4 % from 0.5 ms at 7.5 V.
    let n = (2.5 * FS) as usize;
    let l = Light::new(VTL5C3_2);
    let a = Core::follower_gain(&B292);
    for (v, rs, tol, from) in [
        (
            15.0,
            [
                9.8e3, 8.1e3, 6.5e3, 5.9e3, 7.3e3, 12.5e3, 46.2e3, 152.5e3, 393.1e3, 843.0e3,
                1.63e6, 2.85e6, 4.47e6, 5.49e6,
            ],
            0.025,
            3.0,
        ),
        (
            7.5,
            [
                18.4e3, 18.7e3, 19.5e3, 20.7e3, 23.7e3, 34.7e3, 87.5e3, 205.0e3, 444.0e3, 889.3e3,
                1.67e6, 2.88e6, 4.49e6, 5.50e6,
            ],
            0.04,
            0.5,
        ),
    ] {
        let cv = pulse(v, n);
        let mut tr = Trace::default();
        let _ = run_ref(
            B292,
            Mode::Combo,
            &l,
            &vec![0.0; n],
            &cv,
            &vec![0.0; n],
            1.0,
            Some(&mut tr),
        );
        let (_, _, g) = fw_run(Mode::Combo, VTL5C3_2, &vec![0.0; n], &cv);
        for (ms, want) in MARKS_MS.iter().zip(rs) {
            let k = ((ms * 1e-3 * FS) as usize).min(n - 1);
            let rr = tr.r_ldr[k];
            assert!(near(rr, want, 0.015), "{v} V at {ms} ms: R {rr}");
            if *ms >= from {
                let (fr, ff) = (
                    f3(&B292, Mode::Combo, rr, a),
                    f3(&B292, Mode::Combo, 1.0 / g[k / SUB], a),
                );
                assert!(
                    near(ff, fr, tol),
                    "{v} V at {ms} ms: −3 dB fw {ff}, ref {fr}"
                );
            }
        }
    }
}

/// One 220 Hz hit with the gate checks' ping × 15 V.
fn hit(w: Wave) -> (Vec<f64>, Vec<f64>) {
    let n = (1.6 * FS) as usize;
    let cv = perc(n, 1.0).into_iter().map(|c| c * CV_FS).collect();
    (tone(w, 220.0, n), cv)
}

/// (onset centroid, centroid 50 ms, to −20 dB s, to −40 dB s or None,
/// level at 1 s dB) from gate.md.
type Row = (f64, f64, f64, Option<f64>, f64);

fn assert_row(tag: &str, c: &Checks, want: Row) {
    let (on, c50, t20, t40, floor) = want;
    assert!(
        near(c.centroid_on, on, 0.03),
        "{tag}: onset {}",
        c.centroid_on
    );
    assert!(near(c.c50, c50, 0.03), "{tag}: 50 ms {}", c.c50);
    let t = c.t20.expect("reaches −20 dB");
    assert!(
        (t - t20).abs() <= 0.003 + 0.02 * t20,
        "{tag}: −20 dB at {t}"
    );
    match (c.t40, t40) {
        (Some(t), Some(w)) => assert!((t - w).abs() <= 0.003 + 0.02 * w, "{tag}: −40 dB at {t}"),
        (None, None) => {}
        (g, w) => panic!("{tag}: −40 dB {g:?}, want {w:?}"),
    }
    assert!(
        (c.floor_1s - floor).abs() < 0.6,
        "{tag}: 1 s {}",
        c.floor_1s
    );
}

const FW_ROWS: [(Wave, Mode, Row); 6] = [
    (Wave::Saw, Mode::Combo, (2264.0, 609.0, 0.312, None, -26.6)),
    (
        Wave::Saw,
        Mode::Lowpass,
        (1903.0, 301.0, 0.104, Some(0.388), -49.5),
    ),
    (
        Wave::Saw,
        Mode::Gate,
        (3117.0, 1669.0, 0.023, Some(0.175), -52.8),
    ),
    (Wave::Fold, Mode::Combo, (667.0, 518.0, 0.184, None, -32.1)),
    (
        Wave::Fold,
        Mode::Lowpass,
        (674.0, 331.0, 0.079, Some(0.272), -54.7),
    ),
    (
        Wave::Fold,
        Mode::Gate,
        (676.0, 661.0, 0.021, Some(0.159), -56.4),
    ),
];

#[test]
fn fw_gate_checks_meet_the_ledger() {
    // gate.md, the fw rows (cell VTL5C3): LOWPASS (the 292B's FILT) opens
    // bright and shuts (−40 dB by 0.39 s); COMBO leaks (never −40 dB).
    for (w, mode, row) in FW_ROWS {
        let (x, cv) = hit(w);
        let (y, _, _) = fw_run(mode, VTL5C3, &x, &cv);
        assert_row(&format!("fw {w:?} {mode:?}"), &checks(&y), row);
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn reference_gate_checks_meet_the_ledger() {
    // gate.md, the ref rows (VTL5C3), and the old cell's (VTL5C3/2); the
    // fw tracking the ref (notes.md: "≤ 3 ms, ≤ 0.6 dB on every check",
    // though its own table has 4 ms on the saw in GATE, 179 / 175 ms;
    // ref − fw nulls −37.8…−48.7 dB on the lab's engine-rendered takes).
    let rows: [(Wave, Mode, Cell, Row); 8] = [
        (
            Wave::Saw,
            Mode::Combo,
            VTL5C3,
            (2455.0, 631.0, 0.312, None, -26.7),
        ),
        (
            Wave::Saw,
            Mode::Lowpass,
            VTL5C3,
            (1804.0, 322.0, 0.101, Some(0.386), -49.9),
        ),
        (
            Wave::Saw,
            Mode::Gate,
            VTL5C3,
            (3391.0, 1843.0, 0.023, Some(0.179), -52.6),
        ),
        (
            Wave::Fold,
            Mode::Combo,
            VTL5C3,
            (666.0, 517.0, 0.184, None, -32.1),
        ),
        (
            Wave::Fold,
            Mode::Lowpass,
            VTL5C3,
            (673.0, 327.0, 0.078, Some(0.270), -55.0),
        ),
        (
            Wave::Fold,
            Mode::Gate,
            VTL5C3,
            (675.0, 663.0, 0.021, Some(0.162), -56.2),
        ),
        (
            Wave::Saw,
            Mode::Lowpass,
            VTL5C3_2,
            (1955.0, 429.0, 0.206, Some(0.779), -42.6),
        ),
        (
            Wave::Saw,
            Mode::Combo,
            VTL5C3_2,
            (2575.0, 891.0, 0.626, None, -22.2),
        ),
    ];
    for (w, mode, cell, row) in rows {
        let (x, cv) = hit(w);
        let n = x.len();
        let l = Light::new(cell);
        let y = run_ref(B292, mode, &l, &x, &cv, &vec![0.0; n], 1.0, None);
        let c = checks(&y);
        assert_row(&format!("ref {w:?} {mode:?} {}", cell.name), &c, row);
        if cell == VTL5C3 {
            let (yf, _, _) = fw_run(mode, cell, &x, &cv);
            let f = checks(&yf);
            let dt = |a: Option<f64>, b: Option<f64>| match (a, b) {
                (Some(a), Some(b)) => (a - b).abs(),
                _ => 0.0,
            };
            assert!(
                dt(c.t20, f.t20) <= 0.0045 && dt(c.t40, f.t40) <= 0.0045,
                "{w:?} {mode:?}: {:?} {:?} / {:?} {:?}",
                c.t20,
                f.t20,
                c.t40,
                f.t40
            );
            assert!((c.floor_1s - f.floor_1s).abs() <= 0.6, "{w:?} {mode:?}");
            let null = null_db(&yf, &y);
            // New stimuli, no ledger number: measured on the fold −31.5 /
            // −26.0 / −23.4 dB (COMBO / LOWPASS / GATE), on the saw −28.1 /
            // −25.8 / −18.6 (its top octave is where 48 kHz warps).
            if w == Wave::Fold {
                assert!(null < -21.0, "{w:?} {mode:?}: ref − fw null {null}");
            }
        }
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn a_ping_opens_the_cell_as_the_ledger() {
    // gate.md, "How far a ping opens it" (ref LOWPASS, VTL5C3): the
    // checks' ping peaks at 14.2 mA, 6.3 kΩ at 1.1 ms (LOWPASS −3 dB
    // 14.7 kHz there); a 3 ms 15 V pulse 18.9 mA (IC1's rail), 4.3 kΩ.
    let n = (0.1 * FS) as usize;
    let l = Light::new(VTL5C3);
    let a = Core::follower_gain(&B292);
    let ping: Vec<f64> = perc(n, 1.0).iter().map(|c| c * CV_FS).collect();
    for (cv, ma, r_min, at_ms, lp) in [
        (ping, 14.2, 6.3e3, 1.1, 14693.0),
        (pulse(15.0, n), 18.9, 4.3e3, 3.0, 21887.0),
    ] {
        let mut tr = Trace::default();
        let _ = run_ref(
            B292,
            Mode::Lowpass,
            &l,
            &vec![0.0; n],
            &cv,
            &vec![0.0; n],
            1.0,
            Some(&mut tr),
        );
        let (k, r) =
            tr.r_ldr.iter().enumerate().fold(
                (0, f64::INFINITY),
                |m, (k, &r)| if r < m.1 { (k, r) } else { m },
            );
        let ip = tr.i_led.iter().fold(0.0f64, |m, &v| m.max(v));
        assert!((ip * 1e3 - ma).abs() < 0.06, "peak {}", ip * 1e3);
        assert!(near(r, r_min, 0.015), "min R {r}");
        assert!((k as f64 / FS * 1e3 - at_ms).abs() < 0.06, "at {k}");
        assert!(near(f3(&B292, Mode::Lowpass, r, a), lp, 0.01));
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn velocity_trades_level_for_brightness() {
    // levels.md, brightness against velocity (COMBO, VTL5C3/2, a C4 bar,
    // centroid and RMS 10–53 ms): with an envelope (A 1 ms, D 0.22 s, gate
    // 80 ms) the level spans 6.7 dB from velocity 1 to 0.2 and the centroid
    // falls (562 → 371 Hz); with the 3 ms ping C1's kick flattens it to
    // 1.0 dB. fw matches ref to ≤ 3 Hz and ≤ 0.1 dB. Our bar stands in for
    // the engine-rendered one, so the spans, not the absolute levels, are
    // checked.
    let n = 48_000;
    let x = tone(Wave::Bar, 261.63, n);
    let l = Light::new(VTL5C3_2);
    let env = Env {
        a: 0.001,
        d: 0.22,
        s: 0.0,
        r: 0.2,
    }
    .render(n, (0.08 * FS) as usize);
    let mut lv = [Vec::new(), Vec::new()];
    let mut cent = Vec::new();
    for vel in [1.0, 0.2] {
        for (k, cv) in [
            env.iter().map(|e| CV_FS * e * vel).collect::<Vec<_>>(),
            pulse(CV_FS * vel, n),
        ]
        .into_iter()
        .enumerate()
        {
            let y = run_ref(B292, Mode::Combo, &l, &x, &cv, &vec![0.0; n], 1.0, None);
            let (yf, _, _) = fw_run(Mode::Combo, VTL5C3_2, &x, &cv);
            let (r, f) = (rms_db(&y[480..2528]), rms_db(&yf[480..2528]));
            let (cr, cf) = (centroid(&y, 480, 2048), centroid(&yf, 480, 2048));
            assert!((r - f).abs() < 0.15 && (cr - cf).abs() < 5.0, "{vel} {k}");
            lv[k].push(r);
            if k == 0 {
                cent.push(cr);
            }
        }
    }
    let (env_span, ping_span) = (lv[0][0] - lv[0][1], lv[1][0] - lv[1][1]);
    assert!(env_span > 5.0, "envelope span {env_span}");
    assert!(ping_span < 2.0, "ping span {ping_span}");
    assert!(cent[0] > cent[1] * 1.3, "centroid {cent:?}");
}

#[test]
fn voices_strike_on_the_gate_and_rest_at_the_cutoff() {
    // CUTOFF → OFFSET: a resting −3 dB at the asked cutoff; below the shut
    // corner the offset is 0 V.
    let l = Light::new(VTL5C3);
    let map = super::OffsetMap::new(Mode::Lowpass, &l);
    assert_eq!(map.volts(5.0), 0.0);
    let a = Core::follower_gain(&B292);
    let v = map.volts(1000.0);
    let i = static_current(0.0, v);
    let got = f3(&B292, Mode::Lowpass, l.r_static(i), a);
    assert!(near(got, 1000.0, 0.05), "{v} V rests at {got} Hz");
    // A strike on the gate's rise, silence (−60 dB under it) before.
    let n = 9600;
    let x = tone(Wave::Saw, 220.0, n);
    let c: Vec<crate::Controls> = (0..n)
        .map(|i| crate::Controls {
            cutoff: 20.0,
            res: 0.0,
            gate: if i >= 4800 { 0.8 } else { 0.0 },
        })
        .collect();
    for name in ["buchla292b/fw", "buchla292b/fw-gate"] {
        let y = crate::make(name).expect("registered").render(&x, &c);
        let (pre, post) = (rms_db(&y[2400..4800]), rms_db(&y[4800..7200]));
        assert!(post - pre > 30.0, "{name}: {pre} → {post}");
    }
}
