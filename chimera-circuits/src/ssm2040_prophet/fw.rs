// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The firmware-shaped models: today's 2040 structure (TPT stages, the OTA
//! on the stage's linear error, the loop solved linearly in one step) on
//! the circuit's own numbers, in f32. `Fast` at 2× with the Padé tanh is
//! candidate `d`; at 1× with the quintic, candidate `e`, the one the lab
//! found near the unit's budget (`docs/ssm2040_prophet/ab-notes.md`).

use super::k_of;
use super::reference::Circuit;
use crate::FS;
use crate::util::harness::{Case, lerp_up};
use crate::util::sig::resample_poly;
use std::f64::consts::PI;

/// Padé (3,2) tanh, exact to 0.024 and held at ±1 past ±3: one divide.
#[inline(always)]
pub fn tanh_p(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// The OTA's tanh in `Fast`: Padé (3,2) (one divide), or divide-free: the
/// C1 quintic `u − 3u³/16 + u⁵/64`, flat at 1 from |u| = 2 (tanh(2) = 0.96).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shaper {
    Pade,
    Quintic,
}

#[inline(always)]
pub fn tanh_q(u: f32) -> f32 {
    let u = u.clamp(-2.0, 2.0);
    let u2 = u * u;
    u * (1.0 - u2 * (0.1875 - 0.015625 * u2))
}

/// The candidate for the unit: today's 2040 structure (TPT stages, the
/// OTA on the stage's linear error, the loop solved linearly in one step)
/// on the circuit's own numbers, in f32 at `os` × 48 kHz: the OTA's knee
/// at the attenuator's scale (`V_k = 2V_T/a`, 2.7 V of stage error, not
/// 7.5), a real tanh shape, the cells' offsets, and a soft swing limit in
/// place of the hard clamp. Variables are the stages' outputs with the
/// 2040's inversions taken out: `y_i` in volts at OUT i.
#[derive(Clone, Debug)]
pub struct Fast {
    pub os: usize,
    pub shaper: Shaper,
    /// Where the swing starts to bend, and its limit (volts).
    pub knee: f32,
    pub lim: f32,
    s: [f32; 4],
    inv_vk: [f32; 4],
    vk: [f32; 4],
    off: [f32; 4],
    /// Stage 1's pole over the others' (its own node).
    w1: f32,
}

impl Fast {
    pub fn new(c: &Circuit, os: usize) -> Self {
        let a = c.a();
        let a1 = (1.0 / c.rf) / (1.0 / c.rin + 1.0 / c.rf + 1.0 / c.ra);
        let vk = [
            2.0 * c.vt / a1,
            2.0 * c.vt / a,
            2.0 * c.vt / a,
            2.0 * c.vt / a,
        ];
        let sgn = [1.0, -1.0, 1.0, -1.0];
        Self {
            os,
            shaper: Shaper::Pade,
            knee: 0.85,
            lim: 1.0,
            s: [0.0; 4],
            inv_vk: vk.map(|v| (1.0 / v) as f32),
            vk: vk.map(|v| v as f32),
            off: std::array::from_fn(|i| (sgn[i] * c.vos[i] / (2.0 * c.vt)) as f32),
            w1: (a1 / a) as f32,
        }
    }

    #[inline(always)]
    fn swing(&self, y: f32) -> f32 {
        let m = y.abs();
        if m <= self.knee {
            return y;
        }
        let w = self.lim - self.knee;
        let r = (m - self.knee) / w;
        (self.knee + w * r / (1.0 + r)).copysign(y)
    }

    /// One oversampled tick: `e` the input at OUT 1's DC scale (0.5 V a
    /// unit), `g` the stages' `tan(πf/fs)`, `k` the loop gain.
    #[inline(always)]
    pub fn tick(&mut self, e: f32, g: f32, k: f32) -> f32 {
        let gs = [g * self.w1, g, g, g];
        let big: [f32; 4] = gs.map(|g| g / (1.0 + g));
        let [s1, s2, s3, s4] = self.s;
        let sum = (1.0 - big[3]) * s4
            + big[3]
                * ((1.0 - big[2]) * s3
                    + big[2] * ((1.0 - big[1]) * s2 + big[1] * (1.0 - big[0]) * s1));
        let gg = big[0] * big[1] * big[2] * big[3];
        let mut x = (e - k * sum) / (1.0 + k * gg);
        for i in 0..4 {
            let s = self.s[i];
            let v = (1.0 - big[i]) * (x - s);
            let u = v * self.inv_vk[i] + self.off[i];
            let th = match self.shaper {
                Shaper::Pade => tanh_p(u),
                Shaper::Quintic => tanh_q(u),
            };
            let t = gs[i] * self.vk[i] * th;
            let y = self.swing(s + t);
            self.s[i] = y + t;
            x = y;
        }
        x
    }

    pub fn render(&mut self, case: &Case) -> Vec<f64> {
        let os = self.os;
        let fs = FS * os as f64;
        let x = resample_poly(&case.x, os, 1);
        let (fc, res) = (lerp_up(&case.fc, os), lerp_up(&case.res, os));
        let run = |n: usize, xs: &dyn Fn(usize) -> f64, me: &mut Self| -> Vec<f64> {
            (0..n)
                .map(|i| {
                    let g = (PI * fc[i.min(fc.len() - 1)] / fs).min(1.4).tan() as f32;
                    let k = k_of(res[i.min(res.len() - 1)]) as f32;
                    2.0 * f64::from(me.tick((0.5 * xs(i)) as f32, g, k))
                })
                .collect()
        };
        let settle = (0.1 * fs) as usize;
        let _ = run(settle, &|_| 0.0, self);
        let y = run(x.len(), &|i| x[i], self);
        let mut y = resample_poly(&y, 1, os);
        y.resize(case.x.len(), 0.0);
        y
    }
}
