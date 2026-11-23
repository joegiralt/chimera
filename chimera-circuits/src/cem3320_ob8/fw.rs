// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The firmware-shaped model: the reference's equations cut to what an M7
//! can run at 1× (`Fw`, per-block `FwCoef`), f32 inside, f64 at the edges.

use super::chip::{Chip, VT};
use super::k_of;
use crate::FS;
use crate::util::harness::{Case, lerp_up};
use crate::util::sig::resample_poly;
use std::f64::consts::PI;

/// The firmware candidate: the same cell, f32, one TPT step a sample, 1×.
///
/// Units: currents in `I_REF`, voltages in `I_REF·R_EQ` (5.73 V). Stage
/// `k`'s diode current over `I_REF` is `d = max(1 + ε, 0)`, `ε` the resistor
/// currents' change (the real corner is ~1 % wide; `w > 0` rounds it as
/// `(z + √(z² + w²))/2`, which changed nothing measurable). The cap
/// integrates `G·I_REF·(1 − d) − ρ·u` (ρ: the reflected 1 MΩ). Each stage's
/// implicit trapezoidal step is closed form: `m·z + g·max(z, 0) = c` puts `z`
/// on `c`'s side of the corner, so it is a multiply either way. The loop:
/// the stages' linear maps predict OUT 4; pin 8's 1 nF is a TPT one-pole;
/// the OTA (a degenerated pair, `2V_T·atanh x + R_E·I·x = v`) is one Newton
/// step from last sample's `x` with `atanh = Q/(1 − x²)` (a polynomial `Q`)
/// and last sample's derivative (one divide, off the critical path); then
/// the stages run nonlinearly against the OTA's current, each clamped at
/// its own rails; the loop is AC-coupled (CX20) as on the board.
#[derive(Clone, Debug)]
pub struct Fw {
    /// Oversampling: 1 is the candidate, 2 the lab's check that 1 is enough.
    pub os: usize,
    /// The diode corner's rounding (0 = hard, the physical ~1 %).
    pub w: f32,
    /// OTA solve: 0 linear, 1 one polynomial Newton step, ≥ 2 exact.
    pub newton: usize,
    s: [f32; 4],
    y: f32,
    t: f32,
    d: f32,
    s8: f32,
    inv_dh: f32,
}

/// Per-block coefficients for `Fw`.
#[derive(Clone, Copy, Debug)]
pub struct FwCoef {
    pub g: f32,
    /// `1/(1+g)`, `−g/(1+g)`, the stage linear gains.
    pub inv1g: f32,
    pub pi1: f32,
    pub pi: f32,
    pub a: f32,
    /// Stage closed form: `m`, `m + g/2`, `g/2`, `1/(m(m+g))`, `m(m+g)·w²`,
    /// `1/(1−ρ)`.
    pub m: f32,
    pub mb: f32,
    pub b: f32,
    pub inv_mm: f32,
    pub mmw2: f32,
    pub inv1r: f32,
    /// OTA: `2V_T`, `R_E·I_CR` (volts per unit of x), pin-8 volts per unit
    /// of OUT 4, tail over I_REF.
    pub av: f32,
    pub bv: f32,
    pub k8: f32,
    pub ct: f32,
    /// Input scale: digital unit → I_REF units.
    pub ein: f32,
    /// Each stage's rails about its own quiescent point (the bias network
    /// leaves stages at 6.95 / 7.14 / 6.93 / 7.14 V, so ±6 V is lopsided).
    pub lo: [f32; 4],
    pub hi: [f32; 4],
    /// The resonance path's coupling, one-pole high-pass coefficient.
    pub hp: f32,
    /// Hard diode corner: `1/(m+g)`, `1/m`.
    pub hard: bool,
    pub inv_mg: f32,
    pub inv_m: f32,
    /// Pin 8's pole: `g8/(1+g8)`, `1/(1+g8)`.
    pub c8a: f32,
    pub inv18: f32,
    /// `1/(2V_T + bb)`, bb with pin 8's share of the loop.
    pub inv_ab: f32,
}

impl FwCoef {
    /// The coefficients at cutoff `fc`, RES `res`, rate `fs`, corner `w`.
    pub fn new(ch: &Chip, fc: f64, res: f64, fs: f64, w: f64) -> Self {
        let g = (PI * fc / fs).min(1.45).tan();
        let req = ch.r_eq();
        let rho = req / ch.rf;
        let rho = 1.0 - rho; // reflected share
        let a = req / ch.rc;
        let m = (1.0 + g * rho) / (1.0 - rho);
        let u = ch.i_ref * req;
        let icr = if res > 0.0 {
            ch.icr_of_k(k_of(res))
        } else {
            0.0
        };
        let a8 = ch.r8 / (ch.r8 + ch.r_ri);
        let ein = match ch.r_in {
            Some(r) => ch.fs_in / r / ch.i_ref,
            None => ch.fs_in / ch.i_ref,
        };
        // Buffer quiescent points: stage 1 from R_F alone, then each stage's
        // R_F, R_C and R_B against I_REF.
        let mut yq = [ch.vd0 + ch.i_ref * ch.rf; 4];
        for k in 1..4 {
            yq[k] = ch.vd0
                + ch.rf * (ch.i_ref - (yq[k - 1] - ch.vd0) / ch.rc + (ch.vd0 + 15.0) / ch.rb);
        }
        // Pin 8's pole sits above Nyquist: its bilinear g is warped with the
        // cutoff's, so its phase is right at FREQ (where the ring is).
        let g8 = if ch.c8 > 0.0 {
            let f8 = 1.0 / (2.0 * PI * ch.c8 * (ch.r8 * ch.r_ri / (ch.r8 + ch.r_ri)));
            let wc = PI * fc.min(0.45 * fs) / fs;
            PI * f8 / fs * wc.tan() / wc
        } else {
            1e9
        };
        Self {
            g: g as f32,
            inv1g: (1.0 / (1.0 + g)) as f32,
            pi1: (-g / (1.0 + g)) as f32,
            pi: (-g * a / (1.0 + g)) as f32,
            a: a as f32,
            m: m as f32,
            mb: (m + g / 2.0) as f32,
            b: (g / 2.0) as f32,
            inv_mm: (1.0 / (m * (m + g))) as f32,
            mmw2: (m * (m + g) * w * w) as f32,
            inv1r: (1.0 / (1.0 - rho)) as f32,
            av: (2.0 * VT) as f32,
            bv: (ch.r_ota * icr) as f32,
            k8: (a8 * u) as f32,
            ct: (icr / ch.i_ref) as f32,
            ein: ein as f32,
            lo: yq.map(|y| ((ch.v_lo - ch.vbe0 - y) / u) as f32),
            hi: yq.map(|y| ((ch.v_hi - ch.vbe0 - y) / u) as f32),
            hp: (2.0 * PI / (2.0 * PI * ch.c_ri * (ch.r_ri + ch.r8)) / fs) as f32,
            inv_ab: {
                let pp = (-g / (1.0 + g)) * (-g * a / (1.0 + g)).powi(3);
                let bb = ch.r_ota * icr + g8 / (1.0 + g8) * a8 * u * pp * icr / ch.i_ref;
                (1.0 / (2.0 * VT + bb)) as f32
            },
            c8a: (g8 / (1.0 + g8)) as f32,
            inv18: (1.0 / (1.0 + g8)) as f32,
            hard: w == 0.0,
            inv_mg: (1.0 / (m + g)) as f32,
            inv_m: (1.0 / m) as f32,
        }
    }
}

/// tanh, Lambert's continued fraction to 7th order (|err| < 2e-7 to 4.97).
#[inline(always)]
pub fn tanh_cf(y: f32) -> f32 {
    let y = y.clamp(-4.97, 4.97);
    let y2 = y * y;
    let n = y * (135135.0 + y2 * (17325.0 + y2 * (378.0 + y2)));
    let d = 135135.0 + y2 * (62370.0 + y2 * (3150.0 + 28.0 * y2));
    n / d
}

impl Fw {
    /// At `os`× the lab's defaults: the hard diode corner (`w` 0, the lab's
    /// `CEM_W`) and one polynomial Newton step on the OTA (`newton` 1, the
    /// lab's `CEM_NEWTON`: 0 a linear pair, ≥ 2 Newton with `tanh_cf`).
    pub fn new(os: usize) -> Self {
        Self {
            os,
            w: 0.0,
            newton: 1,
            s: [0.0; 4],
            y: 0.0,
            t: 0.0,
            d: 0.0,
            s8: 0.0,
            inv_dh: 1.0,
        }
    }

    /// One stage: the exact trapezoidal step of
    /// `du/dt = −ω·[ψ(1 + (1−ρ)u + xin) − 1 + ρu]`.
    #[inline(always)]
    fn stage(s: &mut f32, xin: f32, c: &FwCoef, k: usize) -> f32 {
        let cc = *s + c.g + c.m * (1.0 + xin);
        // Hard corner (w = 0): `m·z + g·max(z, 0) = cc` has z of cc's sign,
        // so the branch is known before the solve and costs no root.
        let z = if c.hard {
            cc * if cc >= 0.0 { c.inv_mg } else { c.inv_m }
        } else {
            (c.mb * cc - c.b * (cc * cc + c.mmw2).sqrt()) * c.inv_mm
        };
        let u = ((z - 1.0 - xin) * c.inv1r).clamp(c.lo[k], c.hi[k]);
        *s = 2.0 * u - *s;
        u
    }

    /// One sample: input `x` in digital units; returns OUT 4 in `I_REF·R_EQ`
    /// units, about its quiescent offset.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, c: &FwCoef) -> f32 {
        let e_in = x * c.ein;
        let [s1, s2, s3, s4] = self.s;
        // The stages' linear maps: OUT 4 = S + P·(e_in + e_ota).
        let sig = |s: f32| s * c.inv1g;
        let sum = sig(s4) + c.pi * (sig(s3) + c.pi * (sig(s2) + c.pi * sig(s1)));
        let p = c.pi1 * c.pi * c.pi * c.pi;
        // OTA: 2V_T·y + (R_E·I + k8·P·c_t)·tanh y = k8·(S + P·e_in).
        // Pin 8's 1 nF (CX16) against 3.6 K ‖ R_RI: a TPT one-pole on the
        // OTA's input, `v8 = c8a·x8 + s8/(1+g8)`.
        let b8 = self.s8 * c.inv18;
        let rhs = c.c8a * c.k8 * (sum + p * e_in - self.d) + b8;
        let bb = c.bv + c.c8a * c.k8 * p * c.ct;
        // The pair's fraction x = I_out/I_CR solves
        // `2V_T·atanh(x) + bb·x = rhs`. With atanh(x) = Q(x)/(1 − x²), Q
        // the series' polynomial (coefficients −2/((2n+1)(2n−1)), to x⁹),
        // one Newton step from last sample's x: one divide, no tanh.
        // `newton = 0`: the pair taken as linear; `newton ≥ 2`: Newton in
        // y = atanh(x) with an exact tanh (two divides a step).
        let (mut y, mut t) = (self.y, self.t);
        if self.newton == 0 {
            y = rhs / (c.av + bb);
            t = y;
        } else if self.newton == 1 {
            // Normalised by 2V_T + bb (per block). The derivative's
            // reciprocal is last sample's (a lagged Jacobian): the divide
            // runs off the critical path.
            let (av, bn, rn) = (c.av * c.inv_ab, bb * c.inv_ab, rhs * c.inv_ab);
            let x = t;
            let x2 = x * x;
            let q = x
                * (1.0
                    - x2 * (2.0 / 3.0 + x2 * (2.0 / 15.0 + x2 * (2.0 / 35.0 + x2 * (2.0 / 63.0)))));
            let om = 1.0 - x2;
            let lin = bn * x - rn;
            let h = av * q + lin * om;
            t = (x - h * self.inv_dh).clamp(-0.999, 0.999);
            let t2 = t * t;
            let dq = 1.0 - t2 * (2.0 + t2 * (2.0 / 3.0 + t2 * (2.0 / 5.0)));
            let dh = (av * dq + bn * (1.0 - t2) - 2.0 * t * (bn * t - rn)).max(0.05);
            self.inv_dh = 1.0 / dh;
        } else {
            for _ in 0..self.newton - 1 {
                let f = c.av * y + bb * t - rhs;
                y -= f / (c.av + bb * (1.0 - t * t));
                t = tanh_cf(y);
            }
        }
        self.t = t;
        let e_ota = -c.ct * t;
        let mut u = Self::stage(&mut self.s[0], e_in + e_ota, c, 0);
        for (k, s) in self.s.iter_mut().enumerate().skip(1) {
            u = Self::stage(s, c.a * u, c, k);
        }
        self.y = y;
        let v8 = c.c8a * c.k8 * (u - self.d) + b8;
        self.s8 = 2.0 * v8 - self.s8;
        // CX20 into R_RI + 3.6 K: the loop carries no DC.
        self.d += c.hp * (u - self.d);
        u
    }

    /// A case at 48 kHz, output in digital units (unity at DC).
    pub fn render(&mut self, ch: &Chip, case: &Case) -> Vec<f64> {
        let os = self.os;
        let fs = FS * os as f64;
        let x = if os == 1 {
            case.x.clone()
        } else {
            resample_poly(&case.x, os, 1)
        };
        let (fc, res) = (lerp_up(&case.fc, os), lerp_up(&case.res, os));
        let scale = ch.i_ref * ch.r_eq() / ch.out_per_unit();
        let w = f64::from(self.w);
        let mut y: Vec<f64> = Vec::with_capacity(x.len());
        // 0.1 s of rest at RES 0 for the quiescent offset of ψ (the loop is
        // AC-coupled, so RES does not move it; at RES ≥ 0.96 the rest would
        // ring).
        let c0 = FwCoef::new(ch, fc[0], 0.0, fs, w);
        let mut q = 0.0;
        for _ in 0..(0.1 * fs) as usize {
            q = f64::from(self.tick(0.0, &c0));
        }
        let blk = 16 * os;
        let mut c = c0;
        for (i, &xi) in x.iter().enumerate() {
            if i % blk == 0 {
                let j = i.min(fc.len() - 1);
                c = FwCoef::new(ch, fc[j], res[j], fs, w);
            }
            y.push((f64::from(self.tick(xi as f32, &c)) - q) * scale);
        }
        if os == 1 {
            return y;
        }
        let mut y = resample_poly(&y, 1, os);
        y.resize(case.x.len(), 0.0);
        y
    }
}
