//! The lab's stimuli: the CURTIS lab's three musical phrases (`bassline`,
//! `stab`, `pad`: input and a FREQ track, the input peak-normalised to 1)
//! and the CEM3320 lab's fixed-control cases (`sine_case`, `saw_case`,
//! `kick_case`). The sweeps and the probe are `util::harness`'s.

use crate::FS;
use crate::util::harness::{Case, saw};
use std::f64::consts::PI;

/// A naive saw with PolyBLEP edges, `-1..1`.
fn blep_saw(f: &dyn Fn(usize) -> f64, n: usize, ph0: f64) -> Vec<f64> {
    let mut ph = ph0;
    (0..n)
        .map(|i| {
            let dt = f(i) / FS;
            let mut y = 2.0 * ph - 1.0;
            if ph < dt {
                let t = ph / dt;
                y -= t + t - t * t - 1.0;
            } else if ph > 1.0 - dt {
                let t = (ph - 1.0) / dt;
                y -= t * t + t + t + 1.0;
            }
            ph += dt;
            if ph >= 1.0 {
                ph -= 1.0;
            }
            y
        })
        .collect()
}

fn hz(m: f64) -> f64 {
    440.0 * 2f64.powf((m - 69.0) / 12.0)
}

fn norm(mut x: Vec<f64>) -> Vec<f64> {
    let pk = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    x.iter_mut().for_each(|v| *v /= pk);
    x
}

/// A 16-step bass line, 120 BPM sixteenths, one saw, a plucked filter
/// envelope (FREQ 180 Hz → 2.9 kHz at each note, τ 90 ms), 2 s + 0.5 s.
pub fn bassline() -> (Vec<f64>, Vec<f64>) {
    let notes = [
        33., 33., 45., 33., 36., 33., 43., 45., 33., 33., 45., 40., 38., 36., 33., 31.,
    ];
    let step = (0.125 * FS) as usize;
    let n = step * 16 + (0.5 * FS) as usize;
    let note = |i: usize| notes[(i / step).min(15)];
    let x = blep_saw(&|i| hz(note(i)), n, 0.0);
    let mut out = vec![0.0; n];
    let mut fc = vec![0.0; n];
    for i in 0..n {
        let (k, t) = (i / step, (i % step) as f64 / FS);
        let gate = k < 16 && t < 0.105;
        let amp = if k >= 16 {
            0.0
        } else if gate {
            (t / 0.002).min(1.0) * (0.7 + 0.3 * (-t / 0.08).exp())
        } else {
            (0.7 + 0.3 * (-0.105f64 / 0.08).exp()) * (-(t - 0.105) / 0.004).exp()
        };
        out[i] = x[i] * amp;
        let tt = if k >= 16 { 1.0 } else { t };
        fc[i] = 180.0 * 2f64.powf(4.0 * (-tt / 0.09).exp());
    }
    (norm(out), fc)
}

/// Two brass-style stabs of an A minor 7 (A3 C4 E4 G4), two saws a note
/// ±6 cents, 0.4 s gates 0.75 s apart, FREQ 500 Hz → 4 kHz, τ 150 ms.
pub fn stab() -> (Vec<f64>, Vec<f64>) {
    let n = (2.0 * FS) as usize;
    let mut x = vec![0.0; n];
    for (j, m) in [57.0, 60.0, 64.0, 67.0].iter().enumerate() {
        for (d, c) in [(-6.0, 0.13), (6.0, 0.71)] {
            let f = hz(*m) * 2f64.powf(d / 1200.0);
            let s = blep_saw(&|_| f, n, (c + 0.17 * j as f64) % 1.0);
            x.iter_mut().zip(&s).for_each(|(a, b)| *a += b);
        }
    }
    let mut fc = vec![0.0; n];
    for i in 0..n {
        let t = i as f64 / FS;
        let (t0, on) = if t < 0.75 {
            (t, true)
        } else {
            (t - 0.75, t < 1.5)
        };
        let amp = if !on {
            0.0
        } else if t0 < 0.4 {
            (t0 / 0.004).min(1.0)
        } else {
            (-(t0 - 0.4) / 0.06).exp()
        };
        x[i] *= amp;
        fc[i] = 500.0 * 2f64.powf(3.0 * (-t0 / 0.15).exp());
    }
    (norm(x), fc)
}

/// A pad: F A C E G (F3 … G4), two saws a note ±7 cents, 0.8 s rise,
/// held to 3.5 s, 1 s release; FREQ 1.1 kHz, held.
pub fn pad() -> (Vec<f64>, Vec<f64>) {
    let n = (5.0 * FS) as usize;
    let mut x = vec![0.0; n];
    for (j, m) in [53.0, 57.0, 60.0, 64.0, 67.0].iter().enumerate() {
        for (d, c) in [(-7.0, 0.31), (7.0, 0.83)] {
            let f = hz(*m) * 2f64.powf(d / 1200.0);
            let s = blep_saw(&|_| f, n, (c + 0.23 * j as f64) % 1.0);
            x.iter_mut().zip(&s).for_each(|(a, b)| *a += b);
        }
    }
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f64 / FS;
        let amp = if t < 0.8 {
            (PI / 2.0 * t / 0.8).sin().powi(2)
        } else if t < 3.5 {
            1.0
        } else {
            (-(t - 3.5) / 0.3).exp()
        };
        *v *= amp;
    }
    (norm(x), vec![1100.0; n])
}

/// `secs` of a sine at `f`, `level_db` re one unit, controls held.
pub fn sine_case(f: f64, fc: f64, res: f64, level_db: f64, secs: f64) -> Case {
    let n = (secs * FS) as usize;
    let g = 10f64.powf(level_db / 20.0);
    Case {
        x: (0..n)
            .map(|i| g * (2.0 * PI * f * i as f64 / FS).sin())
            .collect(),
        fc: vec![fc; n],
        res: vec![res; n],
    }
}

/// `secs` of the harness saw at `f`, `level_db` re one unit, controls held.
pub fn saw_case(f: f64, fc: f64, res: f64, level_db: f64, secs: f64) -> Case {
    let n = (secs * FS) as usize;
    let g = 10f64.powf(level_db / 20.0);
    Case {
        x: saw(f, n).into_iter().map(|v| v * g).collect(),
        fc: vec![fc; n],
        res: vec![res; n],
    }
}

/// A 0.5 ms kick of 0.3 units, then silence: the self-oscillation probe.
pub fn kick_case(fc: f64, res: f64, secs: f64) -> Case {
    let n = (secs * FS) as usize;
    let mut x = vec![0.0; n];
    for v in x.iter_mut().take(24) {
        *v = 0.3;
    }
    Case {
        x,
        fc: vec![fc; n],
        res: vec![res; n],
    }
}
