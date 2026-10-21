//! The f64 reference: the cell from US 4,004,141 fig. 3 at device level,
//! Newton on all 13 node voltages per sample, oversampled (`render_ref`).
//!
//! Unknowns: four diode-node voltages, four cap-node voltages, four buffer
//! outputs and pin 8. Shockley/Ebers–Moll devices with Early effect,
//! trapezoidal caps, a dense LU with partial pivoting and step limits on
//! the junction nodes; converges to 1e-10 V in ~2–3 iterations a step.

use super::chip::{Chip, K_T, Q_E, VT, antilog, ex, ota};
use super::k_of;
use crate::FS;
use crate::util::harness::{Case, lerp_up};
use crate::util::sig::resample_poly;
use std::f64::consts::PI;

/// The oversampling every reference render and measure ran at (384 kHz).
pub const OS_REF: usize = 8;

const NU: usize = 13;
const fn n_(k: usize) -> usize {
    k
}
const fn v_(k: usize) -> usize {
    4 + k
}
const fn y_(k: usize) -> usize {
    8 + k
}
const P8: usize = 12;

/// Per-step noise currents (A): into each diode node, into each cap node.
#[derive(Clone, Copy, Default)]
struct Noise {
    node: [f64; 4],
    cap: [f64; 4],
}

/// The circuit's state at one rate. `y_q` is pin 10's quiescent level;
/// `iters`, `steps` and `fails` count Newton's work (`fails` stayed 0 in
/// every lab run).
#[derive(Clone)]
pub struct Reference {
    pub ch: Chip,
    pub fs: f64,
    z: [f64; NU],
    ic: [f64; 4],
    i8: f64,
    u20: f64,
    i20: f64,
    u21: f64,
    i21: f64,
    pub y_q: f64,
    pub iters: u64,
    pub steps: u64,
    pub fails: u64,
    noise: bool,
    rng: u64,
}

impl Reference {
    pub fn new(ch: Chip, fs: f64, g: f64) -> Self {
        let mut r = Self {
            ch,
            fs,
            z: [0.0; NU],
            ic: [0.0; 4],
            i8: 0.0,
            u20: 0.0,
            i20: 0.0,
            u21: 0.0,
            i21: 0.0,
            y_q: 0.0,
            iters: 0,
            steps: 0,
            fails: 0,
            noise: false,
            rng: 0x9e37_79b9_7f4a_7c15,
        };
        for k in 0..4 {
            r.z[n_(k)] = ch.vd0;
            r.z[v_(k)] = 6.9 + ch.vbe0;
            r.z[y_(k)] = 6.9;
        }
        r.dc(g);
        r
    }

    /// White shot noise of each diode, antilog transistor and mirror, and
    /// the stage resistors' thermal noise, per step (a fixed seed).
    pub fn with_noise(mut self) -> Self {
        self.noise = true;
        self
    }

    /// The operating point at no input, RES 0, caps open; coupling caps
    /// charged to it.
    fn dc(&mut self, g: f64) {
        let z0 = self.z;
        self.solve(0.0, g, 0.0, true, &Noise::default(), z0);
        self.u20 = self.z[y_(3)] - self.z[P8];
        self.u21 = self.z[y_(3)];
        self.i20 = 0.0;
        self.i21 = 0.0;
        self.ic = [0.0; 4];
        self.i8 = 0.0;
        self.y_q = self.z[y_(3)];
    }

    /// Stage `k`'s (input node, cap node, buffer out) in volts.
    pub fn node(&self, k: usize) -> (f64, f64, f64) {
        (self.z[n_(k)], self.z[v_(k)], self.z[y_(k)])
    }

    /// Residuals `F(z)` and Jacobian.
    #[allow(clippy::too_many_arguments)]
    fn eval(
        &self,
        z: &[f64; NU],
        x: f64,
        g: f64,
        icr: f64,
        dc: bool,
        nz: &Noise,
        f: &mut [f64; NU],
        j: &mut [[f64; NU]; NU],
    ) {
        let ch = &self.ch;
        *f = [0.0; NU];
        *j = [[0.0; NU]; NU];
        let h = 1.0 / self.fs;
        let ac = if dc { 0.0 } else { 2.0 * ch.c / h };
        let is_d = ch.i_ref * (-ch.vd0 / VT).exp();
        // Coupling branches out of pin 10: CX20 + R_RI into pin 8, CX21 +
        // R_load into a virtual ground. Trapezoidal, closed form.
        let (y4, p8) = (z[y_(3)], z[P8]);
        let (i20, d20) = if dc {
            (0.0, 0.0)
        } else {
            let t = h / (2.0 * ch.c_ri);
            let den = ch.r_ri + t;
            ((y4 - p8 - self.u20 - t * self.i20) / den, 1.0 / den)
        };
        let (i21, d21) = if dc {
            (0.0, 0.0)
        } else {
            let t = h / (2.0 * ch.c_load);
            let den = ch.r_load + t;
            ((y4 - self.u21 - t * self.i21) / den, 1.0 / den)
        };
        // OTA into stage 1's node.
        let (io, dio) = ota(p8, icr, ch);
        for k in 0..4 {
            let (n, v, y) = (z[n_(k)], z[v_(k)], z[y_(k)]);
            // --- diode node
            let mut fr = (y - n) / ch.rf;
            j[n_(k)][y_(k)] += 1.0 / ch.rf;
            j[n_(k)][n_(k)] -= 1.0 / ch.rf;
            if k == 0 {
                match ch.r_in {
                    Some(r) => {
                        fr += (x - n) / r;
                        j[n_(0)][n_(0)] -= 1.0 / r;
                    }
                    None => fr += x,
                }
                // Negative feedback: the pair's output sinks from node 1.
                fr -= io;
                j[n_(0)][P8] -= dio;
            } else {
                let yp = z[y_(k - 1)];
                fr += (yp - n) / ch.rc + (-15.0 - n) / ch.rb;
                j[n_(k)][y_(k - 1)] += 1.0 / ch.rc;
                j[n_(k)][n_(k)] -= 1.0 / ch.rc + 1.0 / ch.rb;
            }
            let e = ex(n / VT);
            let id = is_d * (e - 1.0);
            let did = is_d * e / VT;
            let (icn, dicn) = antilog(id, g, ch);
            let dicn_dn = dicn * if id > 0.0 { did } else { 0.0 };
            f[n_(k)] = fr + nz.node[k] - id - icn / ch.beta;
            j[n_(k)][n_(k)] -= did + dicn_dn / ch.beta;
            // --- cap node: the mirror sources G·I_REF, the antilog
            // transistor sinks I_C, both with Early, both saturating.
            let vr = 6.9 + ch.vbe0;
            let ip = g * ch.i_ref * (1.0 + (vr - v) / ch.va_p);
            let dip = -g * ch.i_ref / ch.va_p;
            let sp = ch.i_ref * ex((v - ch.v_hi) / VT);
            let sn = ch.i_ref * ex((ch.v_lo - v) / VT);
            let early = 1.0 + (v - vr) / ch.va_n;
            let i_cap = ip - sp - (icn * early - sn) + nz.cap[k];
            let di_dv = dip - sp / VT - (icn / ch.va_n + sn / VT);
            let (ic_new, dic_dv) = if dc {
                (0.0, 0.0)
            } else {
                (ac * (v - self.z[v_(k)]) - self.ic[k], ac)
            };
            f[v_(k)] = i_cap - ic_new;
            j[v_(k)][v_(k)] += di_dv - dic_dv;
            j[v_(k)][n_(k)] -= dicn_dn * early;
            // --- buffer: y = v − V_BE(I0 + I_load)
            let mut il = (y - n) / ch.rf;
            let mut dil = [(y_(k), 1.0 / ch.rf), (n_(k), -1.0 / ch.rf), (P8, 0.0)];
            if k < 3 {
                il += (y - z[n_(k + 1)]) / ch.rc;
                dil[0].1 += 1.0 / ch.rc;
                dil[2] = (n_(k + 1), -1.0 / ch.rc);
            } else {
                il += i20 + i21;
                dil[0].1 += d20 + d21;
                dil[2] = (P8, -d20);
            }
            let ie = (ch.i0 + il).max(1e-3 * ch.i0);
            f[y_(k)] = y - v + ch.vbe0 + VT * (ie / ch.i0).ln();
            j[y_(k)][y_(k)] += 1.0;
            j[y_(k)][v_(k)] -= 1.0;
            for (c, d) in dil {
                j[y_(k)][c] += VT / ie * d;
            }
        }
        // --- pin 8
        let (i8n, di8) = if dc || ch.c8 == 0.0 {
            (0.0, 0.0)
        } else {
            let a8 = 2.0 * ch.c8 / h;
            (a8 * (p8 - self.z[P8]) - self.i8, a8)
        };
        f[P8] = i20 - p8 / ch.r8 - i8n;
        j[P8][y_(3)] += d20;
        j[P8][P8] += -d20 - 1.0 / ch.r8 - di8;
    }

    fn solve(&mut self, x: f64, g: f64, icr: f64, dc: bool, nz: &Noise, z0: [f64; NU]) -> bool {
        let mut z = z0;
        let mut f = [0.0; NU];
        let mut j = [[0.0; NU]; NU];
        for _ in 0..80 {
            self.eval(&z, x, g, icr, dc, nz, &mut f, &mut j);
            let mut d = f.map(|v| -v);
            if !lu_solve(&mut j, &mut d) {
                break;
            }
            let mut big = 0.0f64;
            for k in 0..NU {
                let lim = if k < 4 {
                    if z[k] + d[k] > 0.3 { 0.05 } else { 1.0 }
                } else {
                    1.0
                };
                let dk = d[k].clamp(-lim, lim);
                z[k] += dk;
                big = big.max(dk.abs());
            }
            self.iters += 1;
            if big < 1e-10 {
                self.z = z;
                return true;
            }
        }
        self.fails += 1;
        self.z = z;
        false
    }

    fn gauss(&mut self) -> f64 {
        let mut u = || {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            ((self.rng >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let (a, b) = (u(), u());
        (-2.0 * a.ln()).sqrt() * (2.0 * PI * b).cos()
    }

    /// One step at `fs`: input `x` (digital units), cell gain `g`, pin-9
    /// current `icr`. Returns pin 10 in volts.
    pub fn step(&mut self, x: f64, g: f64, icr: f64) -> f64 {
        let ch = self.ch;
        let xin = x * ch.fs_in;
        let mut nz = Noise::default();
        if self.noise {
            let bw = self.fs / 2.0;
            let id = ch.i_ref;
            for k in 0..4 {
                let mut gth = 1.0 / ch.rf;
                if k > 0 {
                    gth += 1.0 / ch.rc + 1.0 / ch.rb;
                } else if let Some(r) = ch.r_in {
                    gth += 1.0 / r;
                }
                // Resistor thermal + diode shot at the node; the antilog
                // transistor and the mirror (in and out) at the cap.
                nz.node[k] = ((4.0 * K_T * gth + 2.0 * Q_E * id) * bw).sqrt() * self.gauss();
                nz.cap[k] = (2.0 * Q_E * g * id * 3.0 * bw).sqrt() * self.gauss();
            }
        }
        let z0 = self.z;
        self.solve(xin, g, icr, false, &nz, z0);
        self.steps += 1;
        // Commit the companion states.
        let h = 1.0 / self.fs;
        let ac = 2.0 * ch.c / h;
        for k in 0..4 {
            self.ic[k] = ac * (self.z[v_(k)] - z0[v_(k)]) - self.ic[k];
        }
        if ch.c8 > 0.0 {
            self.i8 = 2.0 * ch.c8 / h * (self.z[P8] - z0[P8]) - self.i8;
        }
        let (y4, p8) = (self.z[y_(3)], self.z[P8]);
        let t20 = h / (2.0 * ch.c_ri);
        let i20 = (y4 - p8 - self.u20 - t20 * self.i20) / (ch.r_ri + t20);
        self.u20 += t20 * (self.i20 + i20);
        self.i20 = i20;
        let t21 = h / (2.0 * ch.c_load);
        let i21 = (y4 - self.u21 - t21 * self.i21) / (ch.r_load + t21);
        self.u21 += t21 * (self.i21 + i21);
        self.i21 = i21;
        y4
    }
}

/// Dense LU with partial pivoting, in place; `b` becomes the solution.
fn lu_solve(a: &mut [[f64; NU]; NU], b: &mut [f64; NU]) -> bool {
    for c in 0..NU {
        let p = (c..NU)
            .max_by(|&i, &k| a[i][c].abs().total_cmp(&a[k][c].abs()))
            .unwrap();
        if a[p][c].abs() < 1e-300 {
            return false;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..NU {
            let m = a[r][c] / a[c][c];
            if m != 0.0 {
                for k in c..NU {
                    a[r][k] -= m * a[c][k];
                }
                b[r] -= m * b[c];
            }
        }
    }
    for c in (0..NU).rev() {
        let mut s = b[c];
        for k in c + 1..NU {
            s -= a[c][k] * b[k];
        }
        b[c] = s / a[c][c];
    }
    true
}

/// Renders a case through the reference at `os`× (the controls lerped up),
/// output in digital units (pin 10's deviation over `out_per_unit`).
/// `fc_trim` scales every cutoff (the lab's `CEM_TRIM`; 1.0 throughout,
/// since `g_set` already sits the pole on FREQ).
pub fn render_ref(ch: &Chip, case: &Case, os: usize, fc_trim: f64) -> Vec<f64> {
    let fs = FS * os as f64;
    let x = resample_poly(&case.x, os, 1);
    let (fc, res) = (lerp_up(&case.fc, os), lerp_up(&case.res, os));
    let mut r = Reference::new(*ch, fs, ch.g_set(fc[0] * fc_trim));
    let scale = 1.0 / ch.out_per_unit();
    let y: Vec<f64> = (0..x.len())
        .map(|i| {
            let j = i.min(fc.len() - 1);
            let g = ch.g_set(fc[j] * fc_trim);
            let icr = if res[j] > 0.0 {
                ch.icr_of_k(k_of(res[j]))
            } else {
                0.0
            };
            (r.step(x[i], g, icr) - r.y_q) * scale
        })
        .collect();
    let mut y = resample_poly(&y, 1, os);
    y.resize(case.x.len(), 0.0);
    y
}
