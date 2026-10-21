//! The VTL5C3 vactrol (LED + CdS photoresistor) as a photoconductor:
//! Najnudel, Müller, Hélie & Roze, "Power-balanced dynamic modeling of
//! vactrols: application to a VTL5C3/2", DAFx-23 (CC BY 4.0). Their eq. 4
//! (holes, electrons and ionised traps), eqs 7–8 (resistance), eq. 13
//! (LED law) and Table 1's identified constants, adapted:
//!
//! - the optical law: their eq. 12 as printed is negative below 56 mW
//!   (`f_printed`), so the drive that holds the cell at the datasheet's
//!   static resistance is solved from eq. 4 in steady state (their §5.3
//!   method, with the PerkinElmer curve in place of their measurement);
//! - the static law: Parker & D'Angelo's form (DAFx-13, eq. 39),
//!   R = A/If^1.4 + B, refit to the sheet for the VTL5C3/2, and P&D's own
//!   VTL5C3 fit for the single cell;
//! - the VTL5C3's recombination rates ν± refit (scales on Table 1) to the
//!   sheet's 40 mA turn-off curve (`fit`).
//!
//! `Vactrol` is the f64 reference (backward Euler, Newton); `fw::Vac32` the
//! firmware shape (f32, ROS2 in u = s², once a sub-block). From the
//! lpg-circuit lab (`lpg_circuit.rs`); `docs/lpg/` has the sources.
//!
//! No audio voice here: a vactrol alone has no audio path. The Buchla 292B
//! (`crate::buchla292b`) is its voice.

pub mod fit;
pub mod fw;
#[cfg(test)]
mod tests;

/// A photocell: its static law (R = A/If^1.4 + B) and Najnudel's two
/// recombination rates. The rest of their Table 1 (µ±, qτ, Rℓ, R_dark) is
/// shared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub name: &'static str,
    pub a: f64,
    pub b: f64,
    pub nu_p: f64,
    pub nu_n: f64,
}

/// VTL5C3/2 (the 292C's dual cell): Najnudel's identified rates; the static
/// law refit to the PerkinElmer sheet (55 kΩ at 1 mA, 2 kΩ at 40 mA,
/// slope R0.5/R5 = 19.5, sheet 19).
pub const VTL5C3_2: Cell = Cell {
    name: "VTL5C3/2",
    a: 3.363,
    b: 1695.0,
    nu_p: 135.0,
    nu_n: 1.79e8,
};

/// VTL5C3 (single cell, material 3: the nearest documented part to the
/// 292B's Vactec VTL2C3): P&D's eq. 39 (their fit to the VTL5C3 sheet) and
/// Najnudel's model with ν± refit to the sheet's 40 mA turn-off
/// (`fit::fit_vtl5c3`: ×2.583 and ×2.030 on Table 1; rms log error 0.048
/// over the sheet's seven points, 6.75–100 ms).
pub const VTL5C3: Cell = Cell {
    name: "VTL5C3",
    a: 3.464,
    b: 1136.212,
    nu_p: 348.73,
    nu_n: 3.6337e8,
};

/// The lowest current the sheet's curve covers; below it the drive goes
/// on log-log.
pub const LDR_I_LO: f64 = 0.1e-3;

// Najnudel et al. Table 1 (VTL5C3/2), C± = 1 F.
pub const MU_P: f64 = 4.0;
pub const MU_N: f64 = 35.0;
pub const R_LIGHT: f64 = 2.0;
pub const R_DARK: f64 = 10e6;
pub const Q_TAU: f64 = 0.977;
// Their LED law, eq. 13.
pub const LED_VT: f64 = 1.52;
pub const LED_VS: f64 = 23.16e-3;
pub const LED_IS: f64 = 5.65e-3;
// Their optical law, eq. 12, as printed (3 significant figures).
const P0: f64 = -5.47e-5;
const P1: f64 = 5.63e-5;
const A0: f64 = 0.54;
const A1: f64 = 0.55;

impl Cell {
    /// The static law, Ω at LED current `i`.
    pub fn r_sheet(&self, i: f64) -> f64 {
        self.a / i.powf(1.4) + self.b
    }

    /// Steady ionised-trap charge x at hole charge p (q̇ = 0 in eq. 4).
    fn x_steady(&self, p: f64) -> f64 {
        let b = (self.nu_n + self.nu_p) * p;
        let c = self.nu_p * Q_TAU * p;
        // ν− x² + b x − c = 0, the stable root, written to avoid cancellation.
        2.0 * c / (b + (b * b + 4.0 * self.nu_n * c).sqrt())
    }

    /// The optical drive f (W, C± = 1) that holds the LDR at the sheet's
    /// static resistance for LED current i, from eq. 4 in steady state
    /// (their §5.3, with the datasheet curve in place of their measurement).
    pub fn f_for_current(&self, i: f64) -> f64 {
        let r = self.r_sheet(i);
        let gi = {
            let g = 1.0 / r - 1.0 / R_DARK;
            1.0 / (1.0 / g - R_LIGHT)
        };
        // G(p) = µ+ p + µ−(p + x(p)) is monotonic: bisect in log p.
        let (mut lo, mut hi) = (1e-20f64.ln(), 1.0f64.ln());
        for _ in 0..200 {
            let m = 0.5 * (lo + hi);
            let p = m.exp();
            if MU_P * p + MU_N * (p + self.x_steady(p)) < gi {
                lo = m;
            } else {
                hi = m;
            }
        }
        let p = (0.5 * (lo + hi)).exp();
        let x = self.x_steady(p);
        let rate = self.nu_n * x * (p + x);
        rate * (2.0 * p + x)
    }

    /// The low-light log-log slope of f(If) below 0.1 mA (≈ 4.5: light goes
    /// as If^4.5 there).
    pub fn f_low_slope(&self) -> f64 {
        let (a, b) = (LDR_I_LO, 2.0 * LDR_I_LO);
        (self.f_for_current(b) / self.f_for_current(a)).ln() / 2f64.ln()
    }

    /// Najnudel eq. 4 in (p, x): ṗ = g − ν+(qτ − x)p,
    /// ẋ = ν+(qτ − x)p − ν−x(p + x), g = f/(q+ + q−) = f/(2p + x).
    /// Returns (F, Jacobian).
    fn rhs(&self, f: f64, p: f64, x: f64) -> ([f64; 2], [[f64; 2]; 2]) {
        let s = (2.0 * p + x).max(1e-30);
        let g = f / s;
        let gs = f / (s * s);
        let hole = self.nu_p * (Q_TAU - x);
        let fp = g - hole * p;
        let fx = hole * p - self.nu_n * x * (p + x);
        let j = [
            [-2.0 * gs - hole, -gs + self.nu_p * p],
            [
                hole - self.nu_n * x,
                -self.nu_p * p - self.nu_n * (p + 2.0 * x),
            ],
        ];
        ([fp, fx], j)
    }
}

fn softplus(x: f64) -> f64 {
    if x > 30.0 { x } else { x.exp().ln_1p() }
}

/// LED current at forward voltage v (Najnudel eq. 13).
pub fn led_i(v: f64) -> f64 {
    LED_IS * (softplus((v - LED_VT) / LED_VS) - softplus(-LED_VT / LED_VS))
}

/// LED forward voltage at current i (eq. 13 inverted).
pub fn led_v(i: f64) -> f64 {
    let y = i.max(0.0) / LED_IS + softplus(-LED_VT / LED_VS);
    // softplus⁻¹(y) = ln(e^y − 1)
    let z = if y > 30.0 { y } else { y.exp_m1().ln() };
    LED_VT + LED_VS * z
}

/// The eq. 12 optical power at LED power pd, as printed (negative below
/// 56 mW: why the drive is solved from the sheet instead).
pub fn f_printed(pd: f64) -> f64 {
    P0 * pd.powf(A0) + P1 * pd.powf(A1)
}

/// Photoresistor conductance from the states (eqs 7–8), p = q+, x = q+τ,
/// q− = p + x.
pub fn g_ldr(p: f64, x: f64) -> f64 {
    let gi = MU_P * p + MU_N * (p + x);
    1.0 / (1.0 / gi.max(1e-30) + R_LIGHT) + 1.0 / R_DARK
}

/// A cell and its f(If), tabled log-log, 1 nA..40 mA (4001 points); below
/// the sheet's 0.1 mA the last decade's log-log slope continues; 0 below
/// 1 nA.
#[derive(Clone, Debug)]
pub struct Light {
    pub cell: Cell,
    lo: f64,
    step: f64,
    v: Vec<f64>,
}

impl Light {
    pub fn new(cell: Cell) -> Self {
        let (lo, hi, n) = (1e-9f64.ln(), 0.04f64.ln(), 4001);
        let step = (hi - lo) / (n - 1) as f64;
        let (a, b) = (LDR_I_LO, 2.0 * LDR_I_LO);
        let (fa, fb) = (cell.f_for_current(a).ln(), cell.f_for_current(b).ln());
        let slope = (fb - fa) / (b / a).ln();
        let v = (0..n)
            .map(|k| {
                let i = (lo + k as f64 * step).exp();
                if i >= LDR_I_LO {
                    cell.f_for_current(i).ln()
                } else {
                    fa + slope * (i / a).ln()
                }
            })
            .collect();
        Light { cell, lo, step, v }
    }

    /// The optical drive at LED current `i` (A).
    pub fn f(&self, i: f64) -> f64 {
        if i < 1e-9 {
            return 0.0;
        }
        let u = ((i.min(0.04).ln() - self.lo) / self.step).max(0.0);
        let k = (u as usize).min(self.v.len() - 2);
        let fr = u - k as f64;
        (self.v[k] * (1.0 - fr) + self.v[k + 1] * fr).exp()
    }

    /// The LDR's static resistance at LED current i: the model settled
    /// (sheet law ∥ R_dark).
    pub fn r_static(&self, i: f64) -> f64 {
        if i < 1e-9 {
            return R_DARK;
        }
        let mut v = Vactrol::DARK;
        // Settle: hold the drive long enough (the dark tail is slow).
        let f = self.f(i);
        for _ in 0..60 {
            v.step(&self.cell, f, 0.5);
        }
        1.0 / v.g()
    }
}

/// The vactrol's state: hole charge and ionised traps (electrons = p + x).
#[derive(Clone, Copy, Debug)]
pub struct Vactrol {
    pub p: f64,
    pub x: f64,
}

impl Vactrol {
    /// Dark: a trace of carriers (R ≈ R_dark).
    pub const DARK: Vactrol = Vactrol { p: 1e-14, x: 1e-12 };

    pub fn g(&self) -> f64 {
        g_ldr(self.p, self.x)
    }

    /// One step of h seconds at optical drive f (Newton; halves the step
    /// where Newton would not converge).
    pub fn step(&mut self, c: &Cell, f: f64, h: f64) {
        if !self.try_step(c, f, h) {
            self.step(c, f, 0.5 * h);
            self.step(c, f, 0.5 * h);
        }
    }

    fn try_step(&mut self, c: &Cell, f: f64, h: f64) -> bool {
        // Backward Euler: L-stable, so the 1/q generation term from dark
        // can't throw an explicit half-step (the trapezoid's) past the light.
        let (mut p, mut x) = (self.p, self.x);
        for it in 0..60 {
            let (f1, j) = c.rhs(f, p, x);
            let r0 = p - self.p - h * f1[0];
            let r1 = x - self.x - h * f1[1];
            let a = [
                [1.0 - h * j[0][0], -h * j[0][1]],
                [-h * j[1][0], 1.0 - h * j[1][1]],
            ];
            let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
            let dp = (r0 * a[1][1] - r1 * a[0][1]) / det;
            let dx = (a[0][0] * r1 - a[1][0] * r0) / det;
            // Damped: never more than halfway to zero in one iteration.
            let np = (p - dp).max(0.5 * p);
            let nx = (x - dx).max(0.5 * x);
            let conv = (np - p).abs() <= 1e-11 * np && (nx - x).abs() <= 1e-11 * nx;
            (p, x) = (np, nx);
            if conv {
                break;
            }
            if it == 59 && h > 1e-9 {
                return false;
            }
        }
        (self.p, self.x) = (p, x);
        true
    }
}

/// No audio voice: see the module's doc.
pub fn models() -> Vec<crate::Entry> {
    Vec::new()
}
