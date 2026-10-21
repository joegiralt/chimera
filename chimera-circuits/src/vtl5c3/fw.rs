//! The vactrol in the firmware's shape: f32, a 61-entry drive table, and
//! Najnudel's eq. 4 stepped once a sub-block by a two-stage Rosenbrock
//! (ROS2) in u = s², where the generation term is a constant (no
//! singularity from dark). From the lpg-circuit lab (`lpg_circuit.rs`).

use super::{Cell, Light, MU_N, MU_P, Q_TAU, R_DARK, Vactrol};

/// Fast log2 on f32 bits (the firmware has its own; same shape).
pub fn log2f(x: f32) -> f32 {
    // Mantissa into [√½, √2), then log2(m) = 2/ln2 · atanh z, z = (m−1)/(m+1):
    // three odd terms, one divide, error < 3e-6.
    let b = x.to_bits();
    let mut e = ((b >> 23) & 0xff) as i32 - 127;
    let mut m = f32::from_bits((b & 0x007f_ffff) | 0x3f80_0000);
    if m > std::f32::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    e as f32 + z * (2.885_39 + z2 * (0.961_797 + z2 * (0.577_078 + z2 * 0.412_198)))
}

/// Fast exp2 on f32 bits.
pub fn exp2f(x: f32) -> f32 {
    let x = x.clamp(-126.0, 126.0);
    let fl = x.floor();
    let t = x - fl;
    // 2^t, t in [0, 1): a quartic, error < 4e-6.
    let p = 1.0
        + t * (std::f32::consts::LN_2
            + t * (0.240_226_5 + t * (0.055_504_1 + t * (0.009_618_1 + t * 0.001_333_6))));
    f32::from_bits(((fl as i32 + 127) as u32) << 23) * p
}

/// log2 f against log2 If, 1 µA..32 mA, 4 an octave.
#[derive(Clone, Debug)]
pub struct FwLight {
    v: [f32; 61],
}

const FWT_LO: f32 = -19.931_57; // log2(1 µA)
const FWT_PER: f32 = 4.0;

impl FwLight {
    pub fn new(light: &Light) -> Self {
        FwLight {
            v: std::array::from_fn(|j| {
                let l = FWT_LO as f64 + j as f64 / FWT_PER as f64;
                light.f(2f64.powf(l)).log2() as f32
            }),
        }
    }

    /// The optical drive at LED current i: the table, log-log.
    pub fn f(&self, i: f32) -> f32 {
        if i < 1e-6 {
            return 0.0;
        }
        let u = (log2f(i) - FWT_LO) * FWT_PER;
        let k = (u as usize).min(59);
        let fr = u - k as f32;
        let tb = &self.v;
        exp2f(tb[k] + (tb[k + 1] - tb[k]) * fr)
    }
}

/// p, x scaled by this to stay well inside f32.
pub const QS: f32 = 1e6;

/// The f32 vactrol: (p, x) × QS and the cell's rates.
#[derive(Clone, Copy, Debug)]
pub struct Vac32 {
    pub p: f32,
    pub x: f32,
    nu_p: f32,
    nu_n: f32,
}

impl Vac32 {
    pub fn dark(c: &Cell) -> Self {
        Vac32 {
            p: Vactrol::DARK.p as f32 * QS,
            x: Vactrol::DARK.x as f32 * QS,
            nu_p: c.nu_p as f32,
            nu_n: c.nu_n as f32,
        }
    }

    /// Conductance straight from the states: µ+p + µ−(p + x) + 1/R_dark.
    pub fn g(&self) -> f32 {
        (MU_P as f32 * self.p + MU_N as f32 * (self.p + self.x)) * (1.0 / QS) + 1.0 / R_DARK as f32
    }

    /// One piece of `t` seconds at constant drive `f`. Turning on from near
    /// dark (one step's generation would grow u = s² by a quarter or more)
    /// is the stiff start: 4, 8 or 16 steps there, one otherwise. Only a
    /// note's first block or two pay.
    pub fn piece(&mut self, f: f32, t: f32) {
        let s = 2.0 * self.p + self.x;
        let grow = 4.0 * QS * QS * f * t / (s * s);
        let n = if grow > 4.0 {
            16
        } else if grow > 0.25 {
            8
        } else if grow > 0.03 {
            4
        } else {
            1
        };
        for _ in 0..n {
            self.step(f, t / n as f32);
        }
    }

    /// The right-hand side and Jacobian in (u, x), scaled.
    fn rhs(&self, f: f32, u: f32, x: f32) -> ([f32; 2], [f32; 4]) {
        let s = u.max(1e-30).sqrt();
        let x = x.min(s);
        let nn = self.nu_n / QS;
        let npq = self.nu_p / QS;
        let hole = self.nu_p * Q_TAU as f32 - npq * x;
        let (pp, qn) = (0.5 * (s - x), 0.5 * (s + x));
        let a = hole * pp + nn * x * qn;
        let a_s = 0.5 * hole + 0.5 * nn * x;
        let a_x = -npq * pp - 0.5 * hole + 0.5 * nn * (s + 2.0 * x);
        let b = hole * pp - nn * x * qn;
        let b_s = 0.5 * hole - 0.5 * nn * x;
        let b_x = -npq * pp - 0.5 * hole - 0.5 * nn * (s + 2.0 * x);
        let rs = 1.0 / s;
        (
            [4.0 * QS * QS * f - 2.0 * s * a, b],
            [-a * rs - a_s, -2.0 * s * a_x, 0.5 * b_s * rs, b_x],
        )
    }

    /// t seconds of ROS2 (L-stable, second order; one 2×2 inverse).
    /// States: u = s², s = q+ + q− = 2p + x, and x (scaled by QS). In u the
    /// generation f/s is the constant 4f.
    pub fn step(&mut self, f: f32, t: f32) {
        const G: f32 = 1.707_106_8; // 1 + 1/√2
        let s = 2.0 * self.p + self.x;
        let (u, x) = (s * s, self.x);
        let (f0, j) = self.rhs(f, u, x);
        let (a00, a01, a10, a11) = (
            1.0 - G * t * j[0],
            -G * t * j[1],
            -G * t * j[2],
            1.0 - G * t * j[3],
        );
        let rdet = 1.0 / (a00 * a11 - a01 * a10);
        let solve = |r: [f32; 2]| {
            [
                (r[0] * a11 - r[1] * a01) * rdet,
                (a00 * r[1] - a10 * r[0]) * rdet,
            ]
        };
        let k1 = solve(f0);
        let u1 = (u + t * k1[0]).max(0.01 * u);
        let x1 = (x + t * k1[1]).max(0.01 * x);
        let (f1, _) = self.rhs(f, u1, x1);
        let k2 = solve([f1[0] - 2.0 * k1[0], f1[1] - 2.0 * k1[1]]);
        let un = (u + t * (1.5 * k1[0] + 0.5 * k2[0])).max(0.01 * u);
        let sn = un.sqrt();
        let xn = (x + t * (1.5 * k1[1] + 0.5 * k2[1])).max(0.01 * x).min(sn);
        self.x = xn;
        self.p = 0.5 * (sn - xn);
    }
}

/// The same ROS2 step in f64, unscaled: the check that f32 loses nothing.
pub fn ros2_f64(c: &Cell, f: f64, v: &mut Vactrol, t: f64) {
    let rhs = |u: f64, x: f64| {
        let s = u.max(1e-60).sqrt();
        let x = x.min(s);
        let hole = c.nu_p * (Q_TAU - x);
        let (pp, qn) = (0.5 * (s - x), 0.5 * (s + x));
        let a = hole * pp + c.nu_n * x * qn;
        let a_s = 0.5 * hole + 0.5 * c.nu_n * x;
        let a_x = -c.nu_p * pp - 0.5 * hole + 0.5 * c.nu_n * (s + 2.0 * x);
        let b = hole * pp - c.nu_n * x * qn;
        let b_s = 0.5 * hole - 0.5 * c.nu_n * x;
        let b_x = -c.nu_p * pp - 0.5 * hole - 0.5 * c.nu_n * (s + 2.0 * x);
        (
            [4.0 * f - 2.0 * s * a, b],
            [-a / s - a_s, -2.0 * s * a_x, 0.5 * b_s / s, b_x],
        )
    };
    const G: f64 = 1.0 + std::f64::consts::FRAC_1_SQRT_2;
    let s = 2.0 * v.p + v.x;
    let (u, xx) = (s * s, v.x);
    let (f0, j) = rhs(u, xx);
    let (a00, a01, a10, a11) = (
        1.0 - G * t * j[0],
        -G * t * j[1],
        -G * t * j[2],
        1.0 - G * t * j[3],
    );
    let det = a00 * a11 - a01 * a10;
    let solve = |r: [f64; 2]| {
        [
            (r[0] * a11 - r[1] * a01) / det,
            (a00 * r[1] - a10 * r[0]) / det,
        ]
    };
    let k1 = solve(f0);
    let (f1, _) = rhs(
        (u + t * k1[0]).max(0.01 * u),
        (xx + t * k1[1]).max(0.01 * xx),
    );
    let k2 = solve([f1[0] - 2.0 * k1[0], f1[1] - 2.0 * k1[1]]);
    let un = (u + t * (1.5 * k1[0] + 0.5 * k2[0])).max(0.01 * u);
    let sn = un.sqrt();
    let xn = (xx + t * (1.5 * k1[1] + 0.5 * k2[1]))
        .max(0.01 * xx)
        .min(sn);
    v.x = xn;
    v.p = 0.5 * (sn - xn);
}
