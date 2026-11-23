// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The reference audio path: input divider, C2/R10 and IC2, the LDR
//! network (two Rf, C_mid, C_out, C3 in FILT, Rα, the GATE shunt), Q1's
//! square-law JFET follower, C5/R17 and IC3 with its rails. Nodal Newton on
//! four nodes, trapezoidal capacitor companions. And P&D's eq. 7, the
//! network's continuous response.

use super::parts::*;
use crate::FS;

/// A trapezoidal capacitor companion.
#[derive(Clone, Copy, Debug, Default)]
struct Cap {
    v: f64,
    i: f64,
}

impl Cap {
    fn i_at(&self, k: f64, v: f64) -> f64 {
        k * (v - self.v) - self.i
    }
    fn commit(&mut self, k: f64, v: f64) {
        self.i = self.i_at(k, v);
        self.v = v;
    }
}

fn jfet(vgs: f64) -> (f64, f64) {
    if vgs <= VP {
        (0.0, 0.0)
    } else {
        let u = 1.0 - vgs / VP;
        (IDSS * u * u, -2.0 * IDSS * u / VP)
    }
}

/// The reference circuit's audio path state.
#[derive(Clone, Debug)]
pub struct Core {
    pub parts: Parts,
    pub mode: Mode,
    h: f64,
    // Input HP: C2 between the divider and R10.
    c_in: Cap,
    c_mid: Cap,
    c_out: Cap,
    c_fb: Cap,
    c5: Cap,
    v: [f64; 4],
}

impl Core {
    /// At its DC point, stepping `h` seconds.
    pub fn new(parts: Parts, mode: Mode, h: f64) -> Self {
        let mut c = Core {
            parts,
            mode,
            h,
            c_in: Cap::default(),
            c_mid: Cap::default(),
            c_out: Cap::default(),
            c_fb: Cap::default(),
            c5: Cap::default(),
            v: [0.0; 4],
        };
        // DC point: x = p = o = 0, the source where the JFET settles.
        let mut s = 0.0f64;
        if parts.buffer == Buffer::Jfet {
            for _ in 0..100 {
                let (id, gm) = jfet(-s);
                let f = id - (s - VNEG) / R16;
                let d = -gm - 1.0 / R16;
                s -= f / d;
            }
        }
        c.v = [0.0, 0.0, s, 0.0];
        c.c_fb.v = s;
        c.c5.v = s;
        c
    }

    /// The source follower's small-signal gain at the DC point (0.957).
    pub fn follower_gain(parts: &Parts) -> f64 {
        match parts.buffer {
            Buffer::OpAmp => 1.0,
            Buffer::Jfet => {
                let c = Core::new(*parts, Mode::Combo, 1.0 / FS);
                let (_, gm) = jfet(-c.v[2]);
                // AC load: R16 ∥ R17 (C5 a short).
                let rl = R16 * R17 / (R16 + R17);
                gm * rl / (1.0 + gm * rl)
            }
        }
    }

    /// One step: AUD IN (volts), LDR conductance → the unit's output (volts).
    pub fn step(&mut self, vin: f64, g: f64) -> f64 {
        let h = self.h;
        let p = self.parts;
        // Input: divider → C2 → R10 (one node), IC2 × gain, rails.
        let u = if p.stages {
            let (k, rth) = self.mode.thevenin();
            let kc = 2.0 * C2IN / h;
            let vth = k * vin;
            // i = (vth − vcap − v3)/rth, v3 = i R10, the cap's trapezoid:
            let i = (vth - self.c_in.v - self.c_in.i / kc) / (rth + R10 + 1.0 / kc);
            let vcap = vth - i * (rth + R10);
            self.c_in.commit(kc, vcap);
            rail(IC2_GAIN * i * R10)
        } else {
            vin
        };
        let ga = self.mode.shunt(&p);
        let lp = self.mode == Mode::Lowpass;
        let (km, ko, kf, k5) = (
            2.0 * p.c_mid / h,
            2.0 * p.c_out / h,
            2.0 * self.mode.c_fb(&p) / h,
            2.0 * C5OUT / h,
        );
        let jf = p.buffer == Buffer::Jfet;
        let mut v = self.v;
        for _ in 0..20 {
            let [x, pn, s, o] = v;
            let ifb = if lp { self.c_fb.i_at(kf, s - x) } else { 0.0 };
            let i5 = if p.stages {
                self.c5.i_at(k5, s - o)
            } else {
                0.0
            };
            let mut f = [0.0; 4];
            let mut j = [[0.0; 4]; 4];
            f[0] = g * (u - x) + g * (pn - x) - self.c_mid.i_at(km, x) + ifb;
            j[0] = [-2.0 * g - km - kf, g, kf, 0.0];
            f[1] = g * (x - pn) - ga * pn - self.c_out.i_at(ko, pn);
            j[1] = [g, -g - ga - ko, 0.0, 0.0];
            if jf {
                let (id, gm) = jfet(pn - s);
                f[2] = id - (s - VNEG) / R16 - i5 - ifb;
                j[2] = [
                    kf,
                    gm,
                    -gm - 1.0 / R16 - if p.stages { k5 } else { 0.0 } - kf,
                    if p.stages { k5 } else { 0.0 },
                ];
            } else {
                f[2] = s - pn;
                j[2] = [0.0, -1.0, 1.0, 0.0];
            }
            if p.stages {
                f[3] = i5 - o / R17;
                j[3] = [0.0, 0.0, k5, -k5 - 1.0 / R17];
            } else {
                f[3] = o - s;
                j[3] = [0.0, 0.0, -1.0, 1.0];
            }
            let d = solve4(j, f);
            let mut done = true;
            for k in 0..4 {
                v[k] -= d[k];
                if d[k].abs() > 1e-12 * (1.0 + v[k].abs()) {
                    done = false;
                }
            }
            if done {
                break;
            }
        }
        let [x, pn, s, o] = v;
        self.c_mid.commit(km, x);
        self.c_out.commit(ko, pn);
        if lp {
            self.c_fb.commit(kf, s - x);
        } else {
            self.c_fb = Cap { v: s - x, i: 0.0 };
        }
        if p.stages {
            self.c5.commit(k5, s - o);
        }
        self.v = v;
        if p.stages { rail(IC3_GAIN * o) } else { o }
    }
}

fn solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> [f64; 4] {
    for c in 0..4 {
        let piv = (c..4)
            .max_by(|&i, &k| a[i][c].abs().total_cmp(&a[k][c].abs()))
            .unwrap_or(c);
        a.swap(c, piv);
        b.swap(c, piv);
        for r in c + 1..4 {
            let m = a[r][c] / a[c][c];
            for k in c..4 {
                a[r][k] -= m * a[c][k];
            }
            b[r] -= m * b[c];
        }
    }
    let mut x = [0.0; 4];
    for r in (0..4).rev() {
        let mut s = b[r];
        for k in r + 1..4 {
            s -= a[r][k] * x[k];
        }
        x[r] = s / a[r][r];
    }
    x
}

/// |H(j2πf)| of the network with buffer gain a (P&D eq. 7, generalised).
pub fn h_mag(parts: &Parts, mode: Mode, rf: f64, a: f64, f: f64) -> f64 {
    let (c1, c2, c3) = (parts.c_out, parts.c_mid, mode.c_fb(parts));
    let ra = 1.0 / mode.shunt(parts);
    let a1 = 1.0 + 2.0 * rf / ra;
    let a2 = rf * (2.0 * c1 + c2 - c3 * (a - 1.0) + (c2 + c3) * rf / ra);
    let a3 = rf * rf * c1 * (c2 + c3);
    let w = 2.0 * std::f64::consts::PI * f;
    let re = a1 - a3 * w * w;
    let im = a2 * w;
    1.0 / (re * re + im * im).sqrt()
}

/// −3 dB re DC of the network at Rf.
pub fn f3(parts: &Parts, mode: Mode, rf: f64, a: f64) -> f64 {
    let dc = h_mag(parts, mode, rf, a, 0.0);
    let (mut lo, mut hi) = (0.01f64.ln(), 1e7f64.ln());
    for _ in 0..80 {
        let m = 0.5 * (lo + hi);
        if h_mag(parts, mode, rf, a, m.exp()) > dc * std::f64::consts::FRAC_1_SQRT_2 {
            lo = m;
        } else {
            hi = m;
        }
    }
    lo.exp()
}
