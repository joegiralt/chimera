// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The CEM3320 lab's own measures, on top of `util::measure`: the
//! gain-matched null, harmonics and THD, the strongest line, the pole from
//! one gain reading, and the self-oscillation onset.

use super::stimuli::kick_case;
use crate::FS;
use crate::util::harness::{Case, tail};
use crate::util::measure::{db, tone};

/// The residual after the best scale, dB re `x`: how far `x` is from `r`
/// once level is taken out. (`util::measure::null_db` is the raw one.)
pub fn null_scaled_db(x: &[f64], r: &[f64]) -> f64 {
    let (xr, rr) = x
        .iter()
        .zip(r)
        .fold((0.0, 0.0), |(a, b), (u, v)| (a + u * v, b + v * v));
    let g = xr / rr.max(1e-30);
    let e: f64 = x.iter().zip(r).map(|(u, v)| (u - g * v).powi(2)).sum();
    let ex: f64 = x.iter().map(|u| u * u).sum();
    10.0 * (e / ex.max(1e-30)).max(1e-15).log10()
}

/// Harmonics 1..=n of `f` in the last 0.25 s (amplitudes).
pub fn harm(y: &[f64], f: f64, n: usize) -> Vec<f64> {
    let t = tail(y);
    (1..=n).map(|k| tone(t, f * k as f64)).collect()
}

/// THD of harmonics `h` (fundamental first), as a ratio.
pub fn thd(h: &[f64]) -> f64 {
    (h[1..].iter().map(|v| v * v).sum::<f64>()).sqrt() / h[0].max(1e-30)
}

/// The strongest line between `lo` and `hi`, refined to 0.05 Hz:
/// `(f, amplitude)`.
pub fn peak_f(y: &[f64], lo: f64, hi: f64) -> (f64, f64) {
    let mut best = (lo, 0.0);
    let mut f = lo;
    while f < hi {
        let m = tone(y, f);
        if m > best.1 {
            best = (f, m);
        }
        f += 2.0;
    }
    let (a, b) = (best.0 - 2.0, best.0 + 2.0);
    let mut f = a;
    while f < b {
        let m = tone(y, f);
        if m > best.1 {
            best = (f, m);
        }
        f += 0.05;
    }
    best
}

/// A four-pole's pole frequency from its gain `hdb` at `f` re DC, with
/// `|H| = 1/(1 + (f/f_p)²)²` (RES 0).
pub fn pole_of(hdb: f64, f: f64) -> f64 {
    f / (10f64.powf(-hdb / 40.0) - 1.0).sqrt()
}

/// Growth of a kicked ring, dB per 50 ms (0.05–0.1 s against 0.15–0.2 s),
/// at loop gain `k` and FREQ `fc`, the kick scaled by `amp` (the lab: 1e-4
/// on the reference, 1e-2 on the f32 model). `render` is unpadded.
pub fn growth(render: &dyn Fn(&Case) -> Vec<f64>, fc: f64, k: f64, amp: f64) -> f64 {
    let mut c = kick_case(fc, k * 0.95 / 4.0, 0.25);
    c.x.iter_mut().for_each(|v| *v *= amp);
    let y = render(&c);
    let e = |a: f64, b: f64| {
        let s = &y[(a * FS) as usize..(b * FS) as usize];
        (s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt()
    };
    db(e(0.15, 0.2) / e(0.05, 0.1).max(1e-30)) / 2.0
}

/// The loop gain at which a kicked ring neither grows nor decays: scanned
/// down from `from` (above the onset; the lab started at 4.3) in 0.02
/// steps to the first clear decay, the zero crossing interpolated.
pub fn onset_k(render: &dyn Fn(&Case) -> Vec<f64>, fc: f64, amp: f64, from: f64) -> f64 {
    let mut k = from;
    let mut prev = growth(render, fc, k, amp);
    while k > 2.8 {
        let g = growth(render, fc, k - 0.02, amp);
        if g < -0.3 {
            return k - 0.02 * prev / (prev - g).max(1e-9);
        }
        prev = g;
        k -= 0.02;
    }
    k
}
