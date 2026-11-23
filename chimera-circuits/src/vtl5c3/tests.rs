// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The lpg-circuit lab's vactrol checks (`validate()` §2–3, `vac_compare`,
//! `fit_vtl5c3`) as assertions. Numbers from `docs/lpg/validation.md`
//! (cell VTL5C3/2) and `docs/lpg/notes.md` (the VTL5C3 refit).

use super::fit::{VTL5C3_OFF, fit_vtl5c3, off_error};
use super::fw::{FwLight, Vac32, ros2_f64};
use super::*;
use crate::FS;

fn near(got: f64, want: f64, rel: f64) -> bool {
    (got / want - 1.0).abs() <= rel
}

#[test]
fn static_law_meets_the_sheet() {
    // validation.md §2: 55.0 kΩ at 1 mA, 2.0 kΩ at 40 mA (sheet 55 k, 2 k);
    // slope R(0.5)/R(5 mA) 19.5 (sheet 19).
    let c = VTL5C3_2;
    assert!(near(c.r_sheet(1e-3), 55e3, 0.01), "{}", c.r_sheet(1e-3));
    assert!(near(c.r_sheet(40e-3), 2e3, 0.02), "{}", c.r_sheet(40e-3));
    let s = c.r_sheet(0.5e-3) / c.r_sheet(5e-3);
    assert!((s - 19.5).abs() < 0.1, "slope {s}");
    // P&D eq. 39 for the single cell: 56.0 kΩ at 1 mA, 1.45 kΩ at 40 mA.
    assert!(near(VTL5C3.r_sheet(1e-3), 56.0e3, 0.01));
    assert!(near(VTL5C3.r_sheet(40e-3), 1.45e3, 0.01));
}

#[test]
fn settled_model_lands_on_the_static_law() {
    // validation.md §2: "Najnudel model steady" equals the law at every
    // listed current (142.4 k, 55.0 k, 7.3 k, 3.8 k, 2.0 k); low-light
    // slope 4.56.
    let l = Light::new(VTL5C3_2);
    for i in [0.5e-3, 1e-3, 5e-3, 10e-3, 40e-3] {
        let (law, model) = (l.cell.r_sheet(i), l.r_static(i));
        assert!(near(model, law, 0.01), "{i}: law {law}, model {model}");
    }
    let s = VTL5C3_2.f_low_slope();
    assert!((s - 4.56).abs() < 0.03, "low-light slope {s}");
}

#[test]
fn printed_optical_law_is_unusable() {
    // validation.md §2: eq. 12 as printed is negative below 56 mW (f must
    // be ≥ 0, their eq. 10): why the drive is solved from the sheet.
    for pd in [1e-3, 10e-3, 30e-3] {
        assert!(f_printed(pd) < 0.0, "{pd}");
    }
    for pd in [70e-3, 0.3, 1.0, 12.0] {
        assert!(f_printed(pd) > 0.0, "{pd}");
    }
}

#[test]
fn led_law_round_trips_and_meets_the_sheet() {
    // validation.md §2: I(V(10 mA)) = 10.0000 mA; 1.482 / 1.557 / 1.601 /
    // 1.684 V at 1 / 10 / 20 / 40 mA (sheet ≈ 1.65 V typ at 20 mA).
    assert!((led_i(led_v(10e-3)) - 10e-3).abs() < 1e-8);
    for (i, v) in [
        (1e-3, 1.482),
        (10e-3, 1.557),
        (20e-3, 1.601),
        (40e-3, 1.684),
    ] {
        assert!((led_v(i) - v).abs() < 0.001, "{i}: {}", led_v(i));
    }
}

#[test]
fn vtl5c3_refit_meets_the_sheet_turn_off() {
    // notes.md: the refit's turn-off rms log error 0.048 over the sheet's
    // seven points; Najnudel's VTL5C3/2 as published is 2.5–5× slow there
    // (19 kΩ at 20 ms against 97 kΩ; 366 kΩ at 100 ms against 930 kΩ).
    let e = off_error(&VTL5C3);
    assert!(e < 0.05, "VTL5C3 rms log error {e}");
    let (roff, _) = fit::response_at(&VTL5C3_2);
    assert!(near(roff[1], 19.1e3, 0.05), "20 ms: {}", roff[1]);
    assert!(near(roff[6], 366e3, 0.05), "100 ms: {}", roff[6]);
    assert!(roff[1] < VTL5C3_OFF[1].1 / 4.0);
}

#[test]
fn f32_firmware_vactrol_tracks_the_reference() {
    // vac_compare: a 3 ms LED pulse, then dark; the firmware's ROS2 once a
    // 16-sample block against backward Euler at 384 kHz. notes.md: "f32
    // equals f64 here"; turn-on from dark "up to −6 % R in the first ms at
    // 0.6 mA" (here −1.3 % at 0.6 mA, but −11 % at 3 mA: see below).
    let c = VTL5C3_2;
    let light = Light::new(c);
    let fl = FwLight::new(&light);
    let sub = 16;
    let t = sub as f64 / FS;
    for i in [0.6e-3, 3e-3, 8e-3] {
        // The table against the reference drive: within 1 %.
        assert!(near(f64::from(fl.f(i as f32)), light.f(i), 0.01), "f({i})");
        let mut fw = Vac32::dark(&c);
        let mut r64 = Vactrol::DARK;
        let mut v = Vactrol::DARK;
        let n = (0.05 * FS) as usize / sub;
        for k in 0..n {
            let tt = (k + 1) as f64 * t;
            let cur = if tt <= 0.003 { i } else { 0.0 };
            let f = light.f(cur);
            let ff = fl.f(cur as f32);
            fw.piece(ff, t as f32);
            // The same sub-steps in f64, on the same drive.
            let s = 2.0 * r64.p + r64.x;
            let grow = 4.0 * f64::from(ff) * t / (s * s);
            let m = [(4.0, 16), (0.25, 8), (0.03, 4)]
                .iter()
                .find(|(g, _)| grow > *g)
                .map_or(1, |&(_, m)| m);
            for _ in 0..m {
                ros2_f64(&c, f64::from(ff), &mut r64, t / m as f64);
            }
            for _ in 0..sub * 8 {
                v.step(&c, f, t / (sub * 8) as f64);
            }
            let (rr, rf, r6) = (1.0 / v.g(), 1.0 / f64::from(fw.g()), 1.0 / r64.g());
            // Measured here: −16 % in the first block and −11 % at 0.67 ms
            // (3 mA), ≤ 4 % from 2 ms, ≤ 1 % from 40 ms.
            let tol = match tt {
                t if t < 0.0005 => 0.18,
                t if t < 0.002 => 0.12,
                t if t < 0.04 => 0.045,
                _ => 0.012,
            };
            assert!(near(rf, rr, tol), "{i} A at {tt}: fw {rf}, ref {rr}");
            // f32 loses nothing against the same steps in f64.
            assert!(near(rf, r6, 5e-4), "{i} A at {tt}: f32 {rf}, f64 {r6}");
        }
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn vtl5c3_2_dynamics_meet_the_ledger() {
    // validation.md §3 (Najnudel's VTL5C3/2): turn-on to 63 % of the final
    // conductance 1.87 ms at 40 mA, 1.90 ms at 10 mA (sheet 3.0 ms);
    // turn-off to 100 kΩ 44.4 / 39.2 ms (sheet ≤ 50 ms); 10 kΩ → 1 MΩ
    // in 229 ms (Najnudel Fig. 7a ≈ 200 ms).
    let c = VTL5C3_2;
    let l = Light::new(c);
    let h = 1e-5;
    for (i, on, off) in [(40e-3, 1.87e-3, 44.4e-3), (10e-3, 1.90e-3, 39.2e-3)] {
        let mut v = Vactrol::DARK;
        let f = l.f(i);
        let g_final = 1.0 / l.r_static(i);
        let mut t63 = None;
        for k in 0..200_000 {
            v.step(&c, f, h);
            if t63.is_none() && v.g() >= 0.632 * g_final {
                t63 = Some((k + 1) as f64 * h);
            }
        }
        let mut t100 = None;
        for k in 0..300_000 {
            v.step(&c, 0.0, h);
            if 1.0 / v.g() >= 100e3 {
                t100 = Some((k + 1) as f64 * h);
                break;
            }
        }
        let (t63, t100) = (t63.unwrap(), t100.unwrap());
        assert!(near(t63, on, 0.02), "{i}: on {t63}");
        assert!(near(t100, off, 0.02), "{i}: off {t100}");
        assert!(t100 <= 50e-3, "sheet max 50 ms");
    }
    let i10 = {
        let (mut lo, mut hi) = (1e-4f64, 0.04f64);
        for _ in 0..60 {
            let m = (lo * hi).sqrt();
            if l.r_static(m) > 10e3 { lo = m } else { hi = m }
        }
        lo
    };
    assert!(near(i10, 3.77e-3, 0.01), "{i10}");
    let mut v = Vactrol::DARK;
    for _ in 0..100 {
        v.step(&c, l.f(i10), 0.05);
    }
    let mut t1m = None;
    for k in 0..400_000 {
        v.step(&c, 0.0, 1e-5);
        if 1.0 / v.g() >= 1e6 {
            t1m = Some((k + 1) as f64 * 1e-5);
            break;
        }
    }
    let t1m = t1m.unwrap();
    assert!(near(t1m, 229e-3, 0.02), "10 k → 1 M: {t1m}");
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn refit_reproduces_the_vtl5c3_rates() {
    // notes.md / research.md: ν+ ×2.58 (348.7), ν− ×2.03 (3.634e8), rms
    // log error 0.048.
    let (c, e) = fit_vtl5c3();
    assert!(near(c.nu_p, VTL5C3.nu_p, 0.01), "nu_p {}", c.nu_p);
    assert!(near(c.nu_n, VTL5C3.nu_n, 0.01), "nu_n {}", c.nu_n);
    assert!(e < 0.049, "rms log error {e}");
}
