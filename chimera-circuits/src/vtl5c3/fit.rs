//! The VTL5C3's rates: Najnudel's ν± (identified on a VTL5C3/2) refit as
//! scales to the VTL5C3 sheet's 40 mA turn-off curve. From the
//! lpg-circuit lab (`fit_vtl5c3`, the `lpg-circuit-fit` command).

use super::{Cell, Vactrol};

/// The VTL5C3 sheet's 40 mA response curves (PerkinElmer, 2001, "Response
/// Time"), read off the plot: turn-off R(t) from 40 mA steady (s, Ω).
pub const VTL5C3_OFF: [(f64, f64); 7] = [
    (6.75e-3, 11.6e3),
    (20e-3, 97e3),
    (30e-3, 193e3),
    (40e-3, 295e3),
    (60e-3, 484e3),
    (80e-3, 684e3),
    (100e-3, 930e3),
];

/// Turn-on R(t) / R(final) from dark (s, ratio). Reported, not fitted: the
/// sheet's curve starts near 190 kΩ, not dark, and the rates can't match it.
pub const VTL5C3_ON: [(f64, f64); 3] = [(0.67e-3, 11.6), (2e-3, 3.1), (4e-3, 1.3)];

/// The cell's turn-off R and turn-on R/R_final at the sheet's times, 40 mA.
pub fn response_at(c: &Cell) -> (Vec<f64>, Vec<f64>) {
    let f = c.f_for_current(40e-3);
    let h = 1e-5;
    let mut v = Vactrol::DARK;
    for _ in 0..80 {
        v.step(c, f, 0.05);
    }
    let r_fin = 1.0 / v.g();
    let mut v = Vactrol::DARK;
    let mut ron = Vec::new();
    let mut t = 0.0;
    for &(tt, _) in &VTL5C3_ON {
        while t + h / 2.0 < tt {
            v.step(c, f, h);
            t += h;
        }
        ron.push(1.0 / v.g() / r_fin);
    }
    let mut v = Vactrol::DARK;
    for _ in 0..80 {
        v.step(c, f, 0.05);
    }
    let mut roff = Vec::new();
    let mut t = 0.0;
    for &(tt, _) in &VTL5C3_OFF {
        while t + h / 2.0 < tt {
            v.step(c, 0.0, h);
            t += h;
        }
        roff.push(1.0 / v.g());
    }
    (roff, ron)
}

/// RMS log error of the turn-off against the sheet.
pub fn off_error(c: &Cell) -> f64 {
    let (roff, _) = response_at(c);
    let e: f64 = roff
        .iter()
        .zip(&VTL5C3_OFF)
        .map(|(m, (_, s))| (m / s).ln().powi(2))
        .sum();
    (e / VTL5C3_OFF.len() as f64).sqrt()
}

/// Refit Najnudel's ν+ and ν− (as scales on Table 1) to the VTL5C3 sheet,
/// static law P&D eq. 39: a 21 × 21 grid over ¼…8×, then a pattern search.
/// Returns the cell and its rms log error.
pub fn fit_vtl5c3() -> (Cell, f64) {
    let base = Cell {
        name: "fit",
        a: 3.464,
        b: 1136.212,
        nu_p: 135.0,
        nu_n: 1.79e8,
    };
    let at = |kp: f64, kn: f64| Cell {
        nu_p: base.nu_p * kp,
        nu_n: base.nu_n * kn,
        ..base
    };
    let err = |kp: f64, kn: f64| off_error(&at(kp, kn));
    let mut best = (f64::INFINITY, 1.0, 1.0);
    for a in 0..=20 {
        for b in 0..=20 {
            let kp = 2f64.powf(-2.0 + 5.0 * a as f64 / 20.0);
            let kn = 2f64.powf(-2.0 + 5.0 * b as f64 / 20.0);
            let e = err(kp, kn);
            if e < best.0 {
                best = (e, kp, kn);
            }
        }
    }
    let (mut e0, mut kp, mut kn) = best;
    let mut step = 2f64.powf(0.125);
    for _ in 0..40 {
        let mut moved = false;
        for (dp, dn) in [
            (step, 1.0),
            (1.0 / step, 1.0),
            (1.0, step),
            (1.0, 1.0 / step),
        ] {
            let e = err(kp * dp, kn * dn);
            if e < e0 {
                (e0, kp, kn, moved) = (e, kp * dp, kn * dn, true);
            }
        }
        if !moved {
            step = step.sqrt();
            if step < 1.001 {
                break;
            }
        }
    }
    (at(kp, kn), e0)
}
