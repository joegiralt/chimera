// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The SEM labs' stimuli (`sem-lab` def4f753, reused by the circuit lab):
//! the listening set (bass, brass stab at LP and BP, pad, the MODE sweep,
//! the cutoff sweep), the RES sweep, and the probes' sine, burst and
//! ring-out measure. Each A/B in the tests runs on these.

use crate::FS;
use std::f64::consts::PI;

/// A polyBLEP saw at `f`, peak 1.
pub fn saw(f: f64, n: usize, ph0: f64) -> Vec<f64> {
    let dt = f / FS;
    let mut ph = ph0;
    (0..n)
        .map(|_| {
            let mut v = 2.0 * ph - 1.0;
            if ph < dt {
                let t = ph / dt;
                v -= t + t - t * t - 1.0;
            } else if ph > 1.0 - dt {
                let t = (ph - 1.0) / dt;
                v -= t * t + t + t + 1.0;
            }
            ph += dt;
            if ph >= 1.0 {
                ph -= 1.0;
            }
            v
        })
        .collect()
}

pub fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Saws on `notes`, each `voices` detuned ±`cents`, summed and peaked at 1.
pub fn chord(notes: &[f64], voices: usize, cents: f64, n: usize) -> Vec<f64> {
    let mut x = vec![0.0; n];
    for (a, &m) in notes.iter().enumerate() {
        for v in 0..voices {
            let det = if voices == 1 {
                0.0
            } else {
                cents * (2.0 * v as f64 / (voices - 1) as f64 - 1.0)
            };
            let s = saw(hz(m + det / 100.0), n, (a * 7 + v * 3) as f64 * 0.137 % 1.0);
            x.iter_mut().zip(s).for_each(|(o, s)| *o += s);
        }
    }
    peak1(x)
}

pub fn peak1(mut x: Vec<f64>) -> Vec<f64> {
    let pk = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    x.iter_mut().for_each(|v| *v /= pk.max(1e-12));
    x
}

/// Linear attack, then held, then linear release over `rel` s.
pub fn env(n: usize, att: f64, hold_to: f64, rel: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let t = i as f64 / FS;
            if t < att {
                t / att
            } else if t < hold_to {
                1.0
            } else {
                (1.0 - (t - hold_to) / rel).max(0.0)
            }
        })
        .collect()
}

/// One listening stimulus: the input, the cutoff and MODE per sample, and
/// whether it is heard at BP.
pub struct Stim {
    pub name: &'static str,
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub morph: Vec<f64>,
    pub bp: bool,
}

/// The listening set: bass, brass stab (LP and BP), pad, the MODE sweep,
/// the cutoff sweep; the RES sweep is its own job.
pub fn stimuli() -> Vec<Stim> {
    let s = |t: f64| (t * FS) as usize;
    // Bass: A1 A1 E2 G1, 0.3 s each, the cutoff falling 2.6 kHz → 150 Hz.
    let notes = [33.0, 33.0, 40.0, 31.0];
    let (mut bx, mut bfc) = (Vec::new(), Vec::new());
    for &m in &notes {
        let n = s(0.3);
        let a = env(n, 0.002, 0.27, 0.03);
        let w = saw(hz(m), n, 0.0);
        for i in 0..n {
            let t = i as f64 / FS;
            bx.push(w[i] * a[i] * (-t / 0.4).exp());
            bfc.push(150.0 + 2450.0 * (-t / 0.12).exp());
        }
    }
    bx.extend(vec![0.0; s(0.5)]);
    bfc.resize(bx.len(), 150.0);
    let bx = peak1(bx);
    // Brass: Cm7 (C3 Eb3 G3 Bb3), two saws ±4 cents, 0.7 s; the cutoff
    // opens 250 Hz → 3 kHz in 80 ms and settles at 1.4 kHz.
    let n = s(1.3);
    let a = env(n, 0.03, 0.7, 0.12);
    let c = chord(&[48.0, 51.0, 55.0, 58.0], 2, 4.0, n);
    let brass: Vec<f64> = c.iter().zip(&a).map(|(c, a)| c * a).collect();
    let brass_fc: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 / FS;
            if t < 0.08 {
                250.0 * (12.0f64).powf(t / 0.08)
            } else {
                1400.0 + 1600.0 * (-(t - 0.08) / 0.2).exp()
            }
        })
        .collect();
    // Pad: A2 C3 E3 G3, three saws ±7 cents, 4 s, 1 s in and out, 900 Hz.
    let n = s(5.0);
    let a = env(n, 1.0, 4.0, 1.0);
    let p = chord(&[45.0, 48.0, 52.0, 55.0], 3, 7.0, n);
    let pad: Vec<f64> = p.iter().zip(&a).map(|(c, a)| c * a).collect();
    // MODE sweep: the pad's chord held 6 s at 1 kHz, MODE 0 → 1.
    let n = s(6.5);
    let a = env(n, 0.3, 6.0, 0.5);
    let p = chord(&[45.0, 48.0, 52.0, 55.0], 3, 7.0, n);
    let held: Vec<f64> = p.iter().zip(&a).map(|(c, a)| c * a).collect();
    let mm: Vec<f64> = (0..n)
        .map(|i| (i as f64 / s(6.0) as f64).min(1.0))
        .collect();
    // Cutoff sweep: a 110 Hz saw, 40 Hz → 15 kHz over 5 s.
    let n = s(5.0);
    let sw = saw(110.0, n, 0.0);
    let swfc = (0..n)
        .map(|i| 40.0 * (15_000.0f64 / 40.0).powf(i as f64 / n as f64))
        .collect();
    let lp = |n: usize| vec![0.0; n];
    vec![
        Stim {
            name: "bass",
            morph: lp(bx.len()),
            x: bx,
            fc: bfc,
            bp: false,
        },
        Stim {
            name: "brass",
            morph: lp(brass.len()),
            x: brass.clone(),
            fc: brass_fc.clone(),
            bp: false,
        },
        Stim {
            name: "brass_bp",
            morph: lp(brass.len()),
            x: brass,
            fc: brass_fc,
            bp: true,
        },
        Stim {
            name: "pad",
            morph: lp(pad.len()),
            fc: vec![900.0; pad.len()],
            x: pad,
            bp: false,
        },
        Stim {
            name: "mode_sweep",
            fc: vec![1000.0; held.len()],
            x: held,
            morph: mm,
            bp: false,
        },
        Stim {
            name: "cutoff_sweep",
            morph: lp(sw.len()),
            x: sw,
            fc: swfc,
            bp: false,
        },
    ]
}

/// A 110 Hz saw at 800 Hz, RES 0 → 1 over 4 s, held; the saw stops at
/// 4.5 s and the loop rings on alone for 1.5 s. `(x, fc, res)`.
pub fn res_sweep() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = (6.0 * FS) as usize;
    let mut x = saw(110.0, n, 0.0);
    let stop = (4.5 * FS) as usize;
    for (i, v) in x.iter_mut().enumerate().skip(stop) {
        *v *= (1.0 - (i - stop) as f64 / (0.005 * FS)).max(0.0);
    }
    let res = (0..n).map(|i| (i as f64 / (4.0 * FS)).min(1.0)).collect();
    (x, vec![800.0; n], res)
}

/// A sine of amplitude 1 at `f`, `secs` long.
pub fn sine(f: f64, secs: f64) -> Vec<f64> {
    (0..(secs * FS) as usize)
        .map(|i| (2.0 * PI * f * i as f64 / FS).sin())
        .collect()
}

/// A 2 ms burst of a half-amplitude sine at `f`, then silence, `secs` in
/// all: the ring-out probe.
pub fn burst(f: f64, secs: f64) -> Vec<f64> {
    let mut x = vec![0.0; (secs * FS) as usize];
    for (i, v) in x.iter_mut().take((0.002 * FS) as usize).enumerate() {
        *v = 0.5 * (2.0 * PI * f * i as f64 / FS).sin();
    }
    x
}

/// Time for `y` to fall 40 dB from its peak, ms, read on peak envelopes of
/// `win` seconds; `None` if it never does (it holds).
pub fn ring_ms(y: &[f64], win: f64) -> Option<f64> {
    let w = (win * FS) as usize;
    let env: Vec<f64> = y
        .chunks(w)
        .map(|c| c.iter().fold(0.0f64, |m, v| m.max(v.abs())))
        .collect();
    let pk = env.iter().cloned().fold(0.0, f64::max);
    let at = env.iter().position(|&e| e == pk).unwrap_or(0);
    env[at..]
        .iter()
        .position(|&e| e < pk * 0.01)
        .map(|q| q as f64 * win * 1000.0)
}
