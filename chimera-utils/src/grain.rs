// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! GRAIN and HF share (ADR 0095, ROOM as built: GRAIN). GRAIN is a
//! noise-power ratio: the notched noise burst's notch cores against their
//! flanks. An LTI reverb leaves the notches empty; arithmetic grain (or any
//! nonlinear or time-varying part) fills them, so no float twin is needed.
//! HF share is the tail's 4–11.7 kHz against its 0.5–2 kHz.

use crate::SR;
use crate::sig::{firwin_kaiser, plan};
use crate::stim::noise_stim;
use rayon::prelude::*;
use rustfft::num_complex::Complex64;
use std::sync::OnceLock;

/// Notched bursts per measurement.
pub const SEEDS: usize = 4;
const NFFT: usize = 1024;
const HOP: usize = 512;

/// A band, Hz: `lo` up to `hi`.
#[derive(Clone, Copy)]
struct Band {
    lo: f64,
    hi: f64,
}

impl Band {
    const fn new(lo: f64, hi: f64) -> Self {
        assert!(lo >= 0.0 && lo < hi, "a band: 0 ≤ lo < hi");
        Self { lo, hi }
    }

    const fn width(self) -> f64 {
        self.hi - self.lo
    }
}

/// A notch: its core and its two flanks.
struct Notch {
    core: Band,
    flanks: [Band; 2],
}

const NOTCHES: [Notch; 2] = [
    Notch {
        core: Band::new(2_800.0, 3_200.0),
        flanks: [Band::new(2_000.0, 2_400.0), Band::new(3_600.0, 4_000.0)],
    },
    Notch {
        core: Band::new(8_600.0, 9_400.0),
        flanks: [Band::new(7_600.0, 8_100.0), Band::new(9_900.0, 10_400.0)],
    },
];
/// HF share's two bands.
const HF: Band = Band::new(4_000.0, 11_700.0);
const MID: Band = Band::new(500.0, 2_000.0);
/// GRAIN's windows, ms from the burst's end: its last 500 ms, then two of
/// the tail.
const NPR_MS: [(f64, f64); 3] = [(-500.0, 0.0), (50.0, 200.0), (200.0, 400.0)];
/// HF share's windows, ms from the burst's end.
const HF_MS: [(f64, f64); 3] = [(0.0, 50.0), (50.0, 200.0), (200.0, 600.0)];

/// The bar: GRAIN's six values within 3.5 dB rms, none past 6.5 dB; each
/// HF share within 1 dB.
pub mod tol {
    pub const NPR_RMS_DB: f64 = 3.5;
    pub const NPR_MAX_DB: f64 = 6.5;
    pub const HF_DB: f64 = 1.0;
}

/// GRAIN's six values (3 kHz then 9 kHz notch, per window) and the three
/// HF shares, dB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    pub npr: [f64; 6],
    pub hf: [f64; 3],
}

/// The notched bursts, as `noise_stim(seed, 1, 3)` through a 2001-tap
/// Kaiser (β 12) band-stop at 2.5–3.5 and 8.25–9.75 kHz, low-passed at
/// 10.8 kHz so no rate change folds into a notch; and the burst's end.
pub fn notched() -> &'static (Vec<Vec<f64>>, usize) {
    static N: OnceLock<(Vec<Vec<f64>>, usize)> = OnceLock::new();
    N.get_or_init(|| {
        let n = 2001;
        let lp = |f: f64| firwin_kaiser(n, f / (SR / 2.0), 12.0);
        let (a, b, c, d, e) = (
            lp(2_500.0),
            lp(3_500.0),
            lp(8_250.0),
            lp(9_750.0),
            lp(10_800.0),
        );
        let h: Vec<f64> = (0..n).map(|i| a[i] - b[i] + c[i] - d[i] + e[i]).collect();
        let cut = noise_stim(0, 1.0, 3.0).1;
        let xs = (0..SEEDS as u32)
            .map(|s| fir(&noise_stim(s, 1.0, 3.0).0, &h))
            .collect();
        (xs, cut)
    })
}

/// `x` through the odd-length linear-phase FIR `h`, centred.
fn fir(x: &[f64], h: &[f64]) -> Vec<f64> {
    let half = h.len() / 2;
    (0..x.len())
        .into_par_iter()
        .map(|k| {
            let (lo, hi) = (k.saturating_sub(half), (k + half).min(x.len() - 1));
            (lo..=hi).map(|i| x[i] * h[k + half - i]).sum()
        })
        .collect()
}

/// Power per bin of each 1024-point Hann frame, hop 512, summed over `ys`:
/// frame k centred at k·512 + 512.
fn frames(ys: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let win: Vec<f64> = (0..NFFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / NFFT as f64).cos())
        .collect();
    let f = plan(NFFT, false);
    let mut acc: Vec<Vec<f64>> = Vec::new();
    let mut b = vec![Complex64::new(0.0, 0.0); NFFT];
    for y in ys {
        let mut at = 0;
        let mut k = 0;
        while at + NFFT <= y.len() {
            for i in 0..NFFT {
                b[i] = Complex64::new(y[at + i] * win[i], 0.0);
            }
            f.process(&mut b);
            if k == acc.len() {
                acc.push(vec![0.0; NFFT / 2 + 1]);
            }
            for (a, v) in acc[k].iter_mut().zip(&b) {
                *a += v.norm_sqr();
            }
            (at, k) = (at + HOP, k + 1);
        }
    }
    acc
}

/// Mean power a frame in `band`, over the frames centred in `w` (samples).
fn band_power(s: &[Vec<f64>], b: Band, (t0, t1): (usize, usize)) -> f64 {
    let df = SR / NFFT as f64;
    let (j0, j1) = ((b.lo / df).ceil() as usize, (b.hi / df).ceil() as usize);
    let fr: Vec<&Vec<f64>> = s
        .iter()
        .enumerate()
        .filter(|(k, _)| (t0..t1).contains(&(k * HOP + NFFT / 2)))
        .map(|(_, f)| f)
        .collect();
    fr.iter()
        .map(|f| f[j0..j1.min(f.len())].iter().sum::<f64>())
        .sum::<f64>()
        / fr.len().max(1) as f64
}

fn db(x: f64) -> f64 {
    10.0 * (x + 1e-300).log10()
}

/// `ms` after `cut`, in samples.
fn at(cut: usize, ms: f64) -> usize {
    (cut as f64 + ms * SR / 1000.0) as usize
}

/// GRAIN of the responses `ys` to `notched()`.
pub fn npr(ys: &[Vec<f64>], cut: usize) -> [f64; 6] {
    let s = frames(ys);
    let mut o = [0.0; 6];
    for (w, &(a, b)) in NPR_MS.iter().enumerate() {
        let win = (at(cut, a), at(cut, b));
        for (
            n,
            Notch {
                core,
                flanks: [f1, f2],
            },
        ) in NOTCHES.iter().enumerate()
        {
            let pc = band_power(&s, *core, win) / core.width();
            let pf =
                (band_power(&s, *f1, win) + band_power(&s, *f2, win)) / (f1.width() + f2.width());
            o[2 * w + n] = db((pc + 1e-30) / (pf + 1e-30));
        }
    }
    o
}

/// HF share of the responses `ys` to noise bursts ending at `cut`.
pub fn hf(ys: &[Vec<f64>], cut: usize) -> [f64; 3] {
    let s = frames(ys);
    HF_MS.map(|(a, b)| {
        let w = (at(cut, a), at(cut, b));
        db(band_power(&s, HF, w) / band_power(&s, MID, w))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An LTI system leaves the notches near the stimulus floor; white
    /// noise added at −40 dB re the burst fills them.
    #[test]
    fn grain_reads_added_noise_not_the_signal() {
        let (xs, cut) = notched();
        let clean = npr(xs, *cut);
        // The stimulus floor, during the burst.
        assert!(clean[0] < -65.0 && clean[1] < -70.0, "{clean:?}");
        let mut rng = crate::stim::Rng::new(9);
        let noisy: Vec<Vec<f64>> = xs
            .iter()
            .map(|x| {
                x.iter()
                    .map(|v| v + 1.26e-3 * (rng.uniform() - 0.5) * 12f64.sqrt())
                    .collect()
            })
            .collect();
        let g = npr(&noisy, *cut);
        // During the burst (RMS −18 dBFS, about −20 in the flanks' density
        // terms) the notch reads the noise, about 40 dB down.
        assert!(g[0] > -45.0 && g[0] < -30.0, "{g:?}");
        // After the burst there's only noise: the notch reads 0 dB.
        assert!(g[2].abs() < 1.5 && g[5].abs() < 1.5, "{g:?}");
    }

    #[test]
    fn hf_share_of_white_noise_is_its_bandwidth_ratio() {
        let mut rng = crate::stim::Rng::new(3);
        let y: Vec<Vec<f64>> = (0..16)
            .map(|_| (0..60_000).map(|_| rng.uniform() - 0.5).collect())
            .collect();
        let h = hf(&y, 24_000);
        let want = db(7_700.0 / 1_500.0);
        assert!(
            h.iter().all(|v| (v - want).abs() < 0.3),
            "{h:?} want {want}"
        );
    }
}
