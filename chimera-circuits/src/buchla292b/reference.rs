//! The whole reference unit: f64, 8× oversampled (384 kHz), the LED
//! driver, the vactrol and the audio path stepped together each sample.

use super::VREF;
use super::core::Core;
use super::driver::Driver;
use super::parts::{Mode, Parts};
use crate::FS;
use crate::vtl5c3::{Light, Vactrol};
use std::sync::OnceLock;

/// The reference's oversampling.
pub const OS: usize = 8;

/// Kaiser-windowed sinc, 8× interpolate/decimate, 64 taps a phase.
fn os_fir() -> &'static [f64] {
    static T: OnceLock<Vec<f64>> = OnceLock::new();
    T.get_or_init(|| {
        let n = 64 * OS;
        let fc = 21_000.0 / (FS * OS as f64);
        let beta = 9.0;
        let i0 = |x: f64| {
            let (mut s, mut t) = (1.0, 1.0);
            for k in 1..50 {
                t *= (x / (2.0 * k as f64)).powi(2);
                s += t;
            }
            s
        };
        let m = (n - 1) as f64 / 2.0;
        let mut h: Vec<f64> = (0..n)
            .map(|k| {
                let t = k as f64 - m;
                let sinc = if t == 0.0 {
                    2.0 * fc
                } else {
                    (2.0 * std::f64::consts::PI * fc * t).sin() / (std::f64::consts::PI * t)
                };
                let w = i0(beta * (1.0 - (t / m).powi(2)).max(0.0).sqrt()) / i0(beta);
                sinc * w
            })
            .collect();
        let s: f64 = h.iter().sum();
        h.iter_mut().for_each(|v| *v /= s);
        h
    })
}

/// 8× up, delay removed.
fn upsample(x: &[f64]) -> Vec<f64> {
    let h = os_fir();
    let n = x.len() * OS;
    let half = h.len() / 2;
    let mut y = vec![0.0; n];
    for (k, yk) in y.iter_mut().enumerate() {
        // y[k] = OS · Σ_j x[j] h[k + half − j·OS]
        let mut acc = 0.0;
        let kk = k + half;
        let j0 = (kk + 1).saturating_sub(h.len()).div_ceil(OS);
        let j1 = (kk / OS).min(x.len().saturating_sub(1));
        for j in j0..=j1 {
            acc += x[j] * h[kk - j * OS];
        }
        *yk = acc * OS as f64;
    }
    y
}

/// 8× down, delay removed.
fn downsample(y: &[f64]) -> Vec<f64> {
    let h = os_fir();
    let half = h.len() / 2;
    (0..y.len() / OS)
        .map(|k| {
            let c = k * OS + half;
            let mut acc = 0.0;
            for (t, &hv) in h.iter().enumerate() {
                if let Some(&v) = c.checked_sub(t).and_then(|i| y.get(i)) {
                    acc += v * hv;
                }
            }
            acc
        })
        .collect()
}

/// A trace of the control side (one sample per 48 kHz output sample).
#[derive(Clone, Debug, Default)]
pub struct Trace {
    pub i_led: Vec<f64>,
    pub r_ldr: Vec<f64>,
}

/// The whole reference unit on one voice: AUD IN x (digital, × VREF volts)
/// × `gain`, CV and OFFSET (volts, a sample each, 48 kHz; CV ramped
/// between samples). Output digital (÷ VREF), as long as `x`.
#[allow(clippy::too_many_arguments)]
pub fn run_ref(
    parts: Parts,
    mode: Mode,
    light: &Light,
    x: &[f64],
    cv: &[f64],
    off: &[f64],
    gain: f64,
    trace: Option<&mut Trace>,
) -> Vec<f64> {
    let n = x.len();
    if n == 0 {
        return Vec::new();
    }
    let xin: Vec<f64> = x.iter().map(|&v| v * VREF * gain).collect();
    let up = upsample(&xin);
    let h = 1.0 / (FS * OS as f64);
    let mut core = Core::new(parts, mode, h);
    let mut drv = Driver::new(off[0]);
    let mut vac = Vactrol::DARK;
    let cell = light.cell;
    let mut out = vec![0.0; n * OS];
    let mut tr = Trace::default();
    for k in 0..n * OS {
        let i = k / OS;
        let fr = (k % OS) as f64 / OS as f64;
        let c = cv[i] * (1.0 - fr) + cv[(i + 1).min(n - 1)] * fr;
        drv.step(c, off[i], h);
        vac.step(&cell, light.f(drv.i), h);
        let g = vac.g();
        out[k] = core.step(up[k], g) / VREF;
        if k % OS == OS - 1 {
            tr.i_led.push(drv.i);
            tr.r_ldr.push(1.0 / g);
        }
    }
    if let Some(t) = trace {
        *t = tr;
    }
    downsample(&out)
}

/// The same equations at 48 kHz, no oversampling (the convergence check:
/// what the 48 kHz firmware loses to warping).
pub fn run_48(
    parts: Parts,
    mode: Mode,
    light: &Light,
    x: &[f64],
    cv: &[f64],
    off: f64,
) -> Vec<f64> {
    let h = 1.0 / FS;
    let mut core = Core::new(parts, mode, h);
    let mut drv = Driver::new(off);
    let mut vac = Vactrol::DARK;
    x.iter()
        .zip(cv)
        .map(|(&xi, &c)| {
            drv.step(c, off, h);
            vac.step(&light.cell, light.f(drv.i), h);
            core.step(xi * VREF, vac.g()) / VREF
        })
        .collect()
}
