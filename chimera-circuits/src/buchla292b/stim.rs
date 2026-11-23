// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The LPG labs' stimuli and gate measures, shared by the 292B and Plaits
//! tests: bright tones (lpg-circuit lab, `lpg_gate.rs`), the ping, an ADSR
//! (lpg-lab, `lpg.rs`), and the behaviour checks a gate must pass.

use crate::FS;
use crate::util::measure::rms_db;
use crate::util::sig::fft;
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    /// Band-limited saw.
    Saw,
    /// Band-limited square.
    Square,
    /// A sine through a sine folder, sin(4.5·sin ωt): the 259-into-292 tone.
    Fold,
    /// A sine FM'd by a sine an octave up, index 3 (259 FM).
    Fm,
    /// A marimba-ish bar: a sine and its fourth partial at 0.3 (standing in
    /// for the lpg-lab's VA "bar", which rendered through the engine).
    Bar,
}

/// `n` samples of `w` at `f` Hz, peak about 0.5.
pub fn tone(w: Wave, f: f64, n: usize) -> Vec<f64> {
    let norm = |y: Vec<f64>| {
        let pk = y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        y.into_iter().map(|v| 0.5 * v / pk).collect()
    };
    let harm = |odd: bool| -> Vec<f64> {
        let kmax = (20_000.0 / f) as usize;
        let mut y = vec![0.0f64; n];
        for k in 1..=kmax {
            if odd && k % 2 == 0 {
                continue;
            }
            let a = 1.0 / k as f64;
            let wk = TAU * f * k as f64 / FS;
            for (i, v) in y.iter_mut().enumerate() {
                *v += a * (wk * i as f64).sin();
            }
        }
        norm(y)
    };
    let ph = |i: usize| TAU * f * i as f64 / FS;
    match w {
        Wave::Saw => harm(false),
        Wave::Square => harm(true),
        Wave::Fold => (0..n).map(|i| 0.5 * (4.5 * ph(i).sin()).sin()).collect(),
        Wave::Fm => (0..n)
            .map(|i| 0.5 * (ph(i) + 3.0 * (2.0 * ph(i)).sin()).sin())
            .collect(),
        Wave::Bar => norm(
            (0..n)
                .map(|i| ph(i).sin() + 0.3 * (4.0 * ph(i)).sin())
                .collect(),
        ),
    }
}

/// The ping every gate check uses: an exponential-curve attack-release,
/// 1 ms up and 10 ms down, curvature −4, 0..1, × velocity (the shape
/// SuperCollider calls Env.perc(0.001, 0.01); written here from the curve
/// formula, (1 − e^(c·x)) / (1 − e^c)).
pub fn perc(n: usize, vel: f64) -> Vec<f64> {
    (0..n).map(|i| perc_at(i, vel)).collect()
}

/// `perc`'s sample `i`.
pub fn perc_at(i: usize, vel: f64) -> f64 {
    let (a, r) = ((0.001 * FS) as usize, 0.010 * FS);
    let curve = -4.0f64;
    let shape = |x: f64| (1.0 - (curve * x).exp()) / (1.0 - curve.exp());
    if i < a {
        vel * shape(i as f64 / a as f64)
    } else {
        let x = (i - a) as f64 / r;
        if x >= 1.0 {
            0.0
        } else {
            vel * (1.0 - shape(x))
        }
    }
}

/// An ADSR: a linear attack, exponential decay to S and release (τ in
/// seconds), 0..1. From the lpg-lab harness.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    pub a: f64,
    pub d: f64,
    pub s: f64,
    pub r: f64,
}

impl Env {
    /// `n` samples, the gate held for `gate`.
    pub fn render(&self, n: usize, gate: usize) -> Vec<f64> {
        let mut v = 0.0;
        let (kd, kr) = ((-1.0 / (self.d * FS)).exp(), (-1.0 / (self.r * FS)).exp());
        let mut attacking = true;
        (0..n)
            .map(|i| {
                if i < gate {
                    if attacking {
                        v += 1.0 / (self.a * FS).max(1.0);
                        if v >= 1.0 {
                            (v, attacking) = (1.0, false);
                        }
                    } else {
                        v = self.s + (v - self.s) * kd;
                    }
                } else {
                    v *= kr;
                }
                v
            })
            .collect()
    }
}

/// Short-time RMS (5 ms windows, 1 ms hop), dB.
pub fn env_db(y: &[f64]) -> Vec<f64> {
    let w = (0.005 * FS) as usize;
    let hop = (0.001 * FS) as usize;
    (0..y.len().saturating_sub(w) / hop)
        .map(|k| rms_db(&y[k * hop..k * hop + w]))
        .collect()
}

/// Magnitude-weighted spectral centroid of `n` samples from `at` (Hann,
/// zero-padded 4×).
pub fn centroid(y: &[f64], at: usize, n: usize) -> f64 {
    let m = n.next_power_of_two() * 4;
    let v: Vec<f64> = (0..m)
        .map(|i| {
            if i < n {
                let w = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
                y.get(at + i).copied().unwrap_or(0.0) * w
            } else {
                0.0
            }
        })
        .collect();
    let b = fft(&v);
    let (mut num, mut den) = (0.0, 0.0);
    for (k, c) in b.iter().take(m / 2).enumerate() {
        let p = c.norm();
        num += p * k as f64 * FS / m as f64;
        den += p;
    }
    num / den.max(1e-30)
}

/// One hit at sample 0: onset centroid (1–11 ms), time to −20 and −40 dB
/// re the loudest 5 ms (s), the level at 1 s re it (dB), and the centroids
/// at 50 and 200 ms.
#[derive(Clone, Copy, Debug)]
pub struct Checks {
    pub centroid_on: f64,
    pub t20: Option<f64>,
    pub t40: Option<f64>,
    pub floor_1s: f64,
    pub c50: f64,
    pub c200: f64,
}

pub fn checks(y: &[f64]) -> Checks {
    let e = env_db(y);
    let (kmax, pk) = e
        .iter()
        .copied()
        .enumerate()
        .fold((0, f64::MIN), |m, (k, v)| if v > m.1 { (k, v) } else { m });
    let cross = |d: f64| {
        e[kmax..]
            .iter()
            .position(|&v| v < pk - d)
            .map(|k| (kmax + k) as f64 * 1e-3)
    };
    let at = |ms: f64| (ms * 1e-3 * FS) as usize;
    Checks {
        centroid_on: centroid(y, at(1.0), at(10.0)),
        t20: cross(20.0),
        t40: cross(40.0),
        floor_1s: e.get(1000).map_or(f64::NAN, |v| v - pk),
        c50: centroid(y, at(45.0), at(10.0)),
        c200: centroid(y, at(195.0), at(10.0)),
    }
}

/// The strike a harness's gate plays: on each rise of `gate` (0 between
/// notes, else velocity), the ping from its start × the new velocity;
/// 0..1 a sample.
pub fn strikes(c: &[crate::Controls]) -> Vec<f64> {
    let (mut prev, mut k, mut vel) = (0.0, usize::MAX, 0.0);
    c.iter()
        .map(|c| {
            if c.gate > prev {
                (k, vel) = (0, c.gate);
            }
            prev = c.gate;
            let v = if k == usize::MAX {
                0.0
            } else {
                perc_at(k, vel)
            };
            k = k.saturating_add(1);
            v
        })
        .collect()
}
