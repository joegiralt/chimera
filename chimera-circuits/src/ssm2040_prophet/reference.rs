// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The reference: the circuit's parts (`Circuit`), ngspice on a netlist
//! this file writes (a transistor pair per stage; the lab's reference), and
//! `Model`, the same node equations in our own TR-BDF2 + Newton solver,
//! oversampled. Model tracks ngspice to about 0.1 dB of third-octave level
//! (`docs/ssm2040_prophet/ab-notes.md`).

use super::k_of;
use crate::FS;
use crate::util::harness::{Case, lerp_up};
use crate::util::sig::resample_poly;
use crate::util::spice;
use std::f64::consts::PI;
use std::fmt::Write as _;
use std::path::Path;

/// The circuit's parts. Values from SD411 unless marked.
#[derive(Clone, Copy, Debug)]
pub struct Circuit {
    /// C411–C414, 0.001 µF.
    pub c: f64,
    /// Stage-to-stage input resistors (R483, R462, R466), 10 K.
    pub ri: f64,
    /// Each stage's feedback (R480, R484, R465, R467), 10 K.
    pub rf: f64,
    /// Attenuators to ground (R475, R485, R464, R463), 200 Ω. R475 is
    /// illegible on the scan (200 or 230); the data sheet's 200.
    pub ra: f64,
    /// Stage 1's input: R476 200 K plus the bus's R460 100 K.
    pub rin: f64,
    /// kT/q at 27 °C (ngspice's default temperature).
    pub vt: f64,
    /// Each cell's input offset (data sheet: 2 mV typical a cell, 5 max;
    /// 0.6 mV typical for the four in series). Chosen, not measured: one
    /// plausible chip, its alternating sum 0.6 mV.
    pub vos: [f64; 4],
    /// The swing limit: the cap node clamps softly at about ±1 V through a
    /// diode (`is_d`) into ±`vclip`. Assumed: the data sheet gives only
    /// "±1 V"; the die's real mechanism is not published.
    pub is_d: f64,
    pub vclip: f64,
    /// U479's gain, R478/R461.
    pub out_gain: f64,
    /// The 2020's `I = I_c·V/14 V` (data sheet), with a cubic bend `c3`
    /// fitted to its 0.1 % THD at 6 V p-p.
    pub vca_scale: f64,
    pub vca_c3: f64,
    /// Volts at the bus's Thévenin source per digital unit: −15 puts a
    /// full-scale input at 0.5 V on OUT 1 at DC, the "500-mV level" each
    /// pole shows for a maximum saw (tech manual § 5-13), read as peak.
    pub vin_scale: f64,
}

impl Circuit {
    pub const PROPHET: Circuit = Circuit {
        c: 1e-9,
        ri: 10e3,
        rf: 10e3,
        ra: 200.0,
        rin: 300e3,
        vt: 1.380649e-23 * 300.15 / 1.602176634e-19,
        vos: [1.2e-3, 0.9e-3, -0.4e-3, -0.7e-3],
        is_d: 1e-14,
        vclip: 0.447,
        out_gain: 10.0,
        vca_scale: 14.0,
        vca_c3: 0.087,
        vin_scale: -15.0,
    };

    /// Stage 2–4's node gain from each resistor, `(1/R_i)/ΣG`.
    pub fn a(&self) -> f64 {
        (1.0 / self.ri) / (1.0 / self.ri + 1.0 / self.rf + 1.0 / self.ra)
    }

    /// The tail current that puts stages 2–4's pole at `fc`.
    pub fn ic_of(&self, fc: f64) -> f64 {
        2.0 * PI * fc * self.c * 2.0 * self.vt / self.a()
    }

    /// The 2020's control current for loop gain `k`: `k = R_f·gain·I_c/14`.
    pub fn ic2_of(&self, k: f64) -> f64 {
        k * self.vca_scale / (self.out_gain * self.rf)
    }

    /// U479's output per digital unit at DC, RES 0.
    pub fn out_per_unit(&self) -> f64 {
        -self.out_gain * (self.rf / self.rin) * self.vin_scale
    }
}

// ---------------------------------------------------------------- ngspice

const SPICE_UP: usize = 4;

/// The netlist: our own, from the data sheets and SD411.
pub fn netlist(c: &Circuit, dir: &Path, t_end: f64) -> String {
    let f = |n: &str| dir.join(n).display().to_string();
    let mut s = String::new();
    let _ = writeln!(
        s,
        "* SSM2040 + Prophet-5 rev 1/2 resonance loop (chimera lab, #711)"
    );
    for (node, file, scale) in [
        ("vin", "in.txt", c.vin_scale),
        ("icn", "ic.txt", 1.0),
        ("irn", "ir.txt", 1.0),
    ] {
        let _ = writeln!(s, "a_{node} %v([{node}]) src_{node}");
        let _ = writeln!(
            s,
            ".model src_{node} filesource (file=\"{}\" amploffset=[0] amplscale=[{scale}] \
             timeoffset=0 timescale=1 timerelative=false amplstep=false)",
            f(file)
        );
    }
    let _ = writeln!(s, "Vcc vcc 0 15\nVee vee 0 -15");
    let _ = writeln!(s, "Vcp vcp 0 {}\nVcn vcn 0 {}", c.vclip, -c.vclip);
    let _ = writeln!(s, ".model qn npn(is=1e-14 bf=200 vaf=100)");
    let _ = writeln!(s, ".model dclip d(is={} n=1 rs=0)", c.is_d);
    for i in 1..=4 {
        // The cell: a pair on the tail, its collectors sensed and mirrored
        // into the cap, (−) on the node, (+) at the offset.
        let _ = writeln!(s, "Bt{i} e{i} vee I = v(icn)*1e-6");
        let _ = writeln!(s, "Vs1_{i} vcc c1_{i} 0\nVs2_{i} vcc c2_{i} 0");
        let _ = writeln!(s, "Q1_{i} c1_{i} n{i} e{i} qn\nQ2_{i} c2_{i} b{i} e{i} qn");
        let _ = writeln!(s, "Vos{i} b{i} 0 {}", c.vos[i - 1]);
        let _ = writeln!(s, "Bo{i} 0 cap{i} I = i(Vs2_{i}) - i(Vs1_{i})");
        let _ = writeln!(s, "C{i} cap{i} 0 {}", c.c);
        let _ = writeln!(s, "Dp{i} cap{i} vcp dclip\nDn{i} vcn cap{i} dclip");
        let _ = writeln!(s, "Eb{i} o{i} 0 cap{i} 0 1");
        if i == 1 {
            let _ = writeln!(s, "Rin vin n1 {}", c.rin);
        } else {
            let _ = writeln!(s, "Ri{i} o{} n{i} {}", i - 1, c.ri);
        }
        let _ = writeln!(s, "Rf{i} o{i} n{i} {}\nRa{i} n{i} 0 {}", c.rf, c.ra);
    }
    let _ = writeln!(s, "Eo vo 0 o4 0 {}", -c.out_gain);
    let _ = writeln!(
        s,
        "Bres 0 n1 I = v(irn)*1e-6/{vs} * v(vo) * (1 - {c3}*(v(vo)/{vs})*(v(vo)/{vs}))",
        vs = c.vca_scale,
        c3 = c.vca_c3
    );
    let step = 1.0 / (FS * SPICE_UP as f64);
    let _ = writeln!(
        s,
        ".options method=gear reltol=1e-5 abstol=1e-14 vntol=1e-8 temp=27 tnom=27 interp"
    );
    let _ = writeln!(
        s,
        ".control\ntran {step:e} {t_end:e} 0 {:e}\nwrdata {} v(vo)\n.endc\n.end",
        step / 4.0,
        f("out.txt")
    );
    s
}

/// ngspice's render of `case`, in `dir` (kept: its netlist and data).
pub fn spice(c: &Circuit, case: &Case, dir: &Path) -> Result<Vec<f64>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dt = 1.0 / (FS * SPICE_UP as f64);
    let x = resample_poly(&case.x, SPICE_UP, 1);
    let ic: Vec<f64> = lerp_up(&case.fc, SPICE_UP)
        .iter()
        .map(|&f| c.ic_of(f) * 1e6)
        .collect();
    let ir: Vec<f64> = lerp_up(&case.res, SPICE_UP)
        .iter()
        .map(|&r| c.ic2_of(k_of(r)) * 1e6)
        .collect();
    spice::write_series(&dir.join("in.txt"), &x, dt)?;
    spice::write_series(&dir.join("ic.txt"), &ic, dt)?;
    spice::write_series(&dir.join("ir.txt"), &ir, dt)?;
    let t_end = (x.len() - 1) as f64 * dt;
    let net = netlist(c, dir, t_end);
    let y = spice::run(
        dir,
        "filter.cir",
        &net,
        &spice::stamp(x.iter().chain(&ic).chain(&ir)),
    )?;
    let scale = 1.0 / c.out_per_unit();
    let mut y: Vec<f64> = resample_poly(&y, 1, SPICE_UP)
        .iter()
        .map(|v| v * scale)
        .collect();
    y.resize(case.x.len(), 0.0);
    Ok(y)
}

/// The component model: the same node equations at `os` × 48 kHz,
/// TR-BDF2 with Newton on the four cap voltages.
#[derive(Clone, Debug)]
pub struct Model {
    pub c: Circuit,
    pub os: usize,
    /// Newton iterations, at most.
    pub iters: usize,
    v: [f64; 4],
    f: [f64; 4],
    /// Iterations spent, for the report.
    pub spent: u64,
}

struct Eval {
    f: [f64; 4],
    /// `∂F_i/∂V_i`, `∂F_i/∂V_{i−1}` (i ≥ 2), and `∂F_1/∂V_4`.
    d: [f64; 4],
    l: [f64; 4],
    c14: f64,
}

impl Model {
    pub fn new(c: Circuit, os: usize, iters: usize) -> Self {
        Self {
            c,
            os,
            iters,
            v: [0.0; 4],
            f: [0.0; 4],
            spent: 0,
        }
    }

    fn diode(&self, v: f64) -> (f64, f64) {
        let c = &self.c;
        let e = |u: f64| (u / c.vt).min(80.0).exp();
        let (ep, en) = (e(v - c.vclip), e(-c.vclip - v));
        (c.is_d * (ep - en), c.is_d * (ep + en) / c.vt)
    }

    fn eval(&self, v: &[f64; 4], vin: f64, ic: f64, ic2: f64) -> Eval {
        let c = &self.c;
        let (gi, gf, ga, gin) = (1.0 / c.ri, 1.0 / c.rf, 1.0 / c.ra, 1.0 / c.rin);
        let vo = -c.out_gain * v[3];
        let w = vo / c.vca_scale;
        let ires = ic2 * w * (1.0 - c.vca_c3 * w * w);
        let dires = ic2 / c.vca_scale * (1.0 - 3.0 * c.vca_c3 * w * w) * -c.out_gain;
        let mut e = Eval {
            f: [0.0; 4],
            d: [0.0; 4],
            l: [0.0; 4],
            c14: 0.0,
        };
        let two_vt = 2.0 * c.vt;
        for i in 0..4 {
            let (gs, node, dn_prev) = if i == 0 {
                let gs = gin + gf + ga;
                (gs, (vin * gin + v[0] * gf + ires) / gs, 0.0)
            } else {
                let gs = gi + gf + ga;
                (gs, (v[i - 1] * gi + v[i] * gf) / gs, gi / gs)
            };
            let t = ((c.vos[i] - node) / two_vt).tanh();
            let (id, gd) = self.diode(v[i]);
            e.f[i] = ic * t - id;
            // ∂F/∂node = −I_c·sech²/2V_T.
            let dfn = -ic * (1.0 - t * t) / two_vt;
            e.d[i] = dfn * gf / gs - gd;
            e.l[i] = dfn * dn_prev;
            if i == 0 {
                e.c14 = dfn * dires / gs;
            }
        }
        e
    }

    /// Settle at the controls, no input: the offsets' DC point.
    pub fn settle(&mut self, fc: f64, res: f64) {
        for _ in 0..(0.1 * FS) as usize * self.os {
            self.tick(0.0, fc, res);
        }
    }

    /// Newton on `V − b − c·F(V) = 0` from `v`.
    fn solve(&mut self, v: &mut [f64; 4], b: [f64; 4], cf: f64, vin: f64, ic: f64, ic2: f64) {
        for _ in 0..self.iters.max(1) {
            self.spent += 1;
            let e = self.eval(v, vin, ic, ic2);
            let r: [f64; 4] = std::array::from_fn(|i| v[i] - b[i] - cf * e.f[i]);
            let jd: [f64; 4] = std::array::from_fn(|i| 1.0 - cf * e.d[i]);
            let jl: [f64; 4] = std::array::from_fn(|i| -cf * e.l[i]);
            let j14 = -cf * e.c14;
            // Lower bidiagonal plus the loop's corner: Δ_i = p_i + q_i·Δ₄.
            let (mut p, mut q) = ([0.0; 4], [0.0; 4]);
            p[0] = -r[0] / jd[0];
            q[0] = -j14 / jd[0];
            for i in 1..4 {
                p[i] = (-r[i] - jl[i] * p[i - 1]) / jd[i];
                q[i] = -jl[i] * q[i - 1] / jd[i];
            }
            let d4 = p[3] / (1.0 - q[3]);
            let mut big = 0.0f64;
            for i in 0..4 {
                let d = (p[i] + q[i] * d4).clamp(-0.5, 0.5);
                v[i] += d;
                big = big.max(d.abs());
            }
            if big < 1e-10 {
                break;
            }
        }
    }

    /// One step, TR-BDF2 (γ = 2 − √2): a trapezoid to γh, then BDF2 to h.
    /// L-stable, so the stiff swing clamp cannot ring at Nyquist as a plain
    /// trapezoid does; second order. `h` is prewarped at the stages' pole.
    fn tick(&mut self, x: f64, fc: f64, res: f64) -> f64 {
        let c = self.c;
        let h = 1.0 / (FS * self.os as f64);
        let ic = c.ic_of(fc);
        let ic2 = c.ic2_of(k_of(res));
        let wc = 2.0 * PI * fc;
        let hp = 2.0 / wc * (wc * h / 2.0).min(1.4).tan();
        let vin = c.vin_scale * x;
        let gam = 2.0 - std::f64::consts::SQRT_2;
        let (vn, fnn) = (self.v, self.f);
        // Stage 1, trapezoid over γh.
        let g1 = gam * hp / (2.0 * c.c);
        let mut vg = vn;
        let b1: [f64; 4] = std::array::from_fn(|i| vn[i] + g1 * fnn[i]);
        self.solve(&mut vg, b1, g1, vin, ic, ic2);
        // Stage 2, BDF2 through vn, vg.
        let den = gam * (2.0 - gam);
        let b2: [f64; 4] = std::array::from_fn(|i| vg[i] / den - (1.0 - gam).powi(2) / den * vn[i]);
        let g2 = (1.0 - gam) / (2.0 - gam) * hp / c.c;
        let mut v = vg;
        self.solve(&mut v, b2, g2, vin, ic, ic2);
        self.v = v;
        self.f = self.eval(&v, vin, ic, ic2).f;
        -c.out_gain * v[3] / c.out_per_unit()
    }

    pub fn render(&mut self, case: &Case) -> Vec<f64> {
        self.settle(case.fc[0], case.res[0]);
        let x = resample_poly(&case.x, self.os, 1);
        let (fc, res) = (lerp_up(&case.fc, self.os), lerp_up(&case.res, self.os));
        let y: Vec<f64> = (0..x.len())
            .map(|i| self.tick(x[i], fc[i], res[i]))
            .collect();
        let mut y = resample_poly(&y, 1, self.os);
        y.resize(case.x.len(), 0.0);
        y
    }
}
