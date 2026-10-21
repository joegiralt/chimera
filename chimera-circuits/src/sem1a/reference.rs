//! The SEM-1A VCF as built, solved by nodal Newton in f64 (#720 circuit lab,
//! `sem-circuit-lab` aa4cb6a2).
//!
//! Every part is the schematic's (SEM-1A, VCF sheet and pot board; notes in
//! `docs/sem1a/research.md`): the input pot,
//! C15 and R155 into A12 (a 301A, finite GBW and slew, its rails), R138 ‖
//! C18 from LP, the two CA3080 integrators behind 100 K/220 Ω (each a
//! bipolar pair in Ebers–Moll forward-active, its tail from pin 5, base
//! currents, output mirrors that saturate at the datasheet's swing, output
//! resistance), C20/C21, the 2N4302 source followers Q39/Q40 (Shichman–
//! Hodges with gate junctions) into R147/R153, A14, the RES pot PS16 with
//! R157, the R159/R158 tap, D11/D12 (Shockley) and R156, C22 into the
//! summing node. MODE is PS19 between A12's and A16's outputs into A17;
//! S504 takes A14 instead; C23 into the VCA's 100 K is the output.
//!
//! Solved as a nodal system: 15 unknowns, trapezoidal companions for the
//! five capacitors and A12's state, Newton with an analytic Jacobian (a
//! test checks it against finite differences) and partial-pivot LU.
//! Method: Holters & Zölzer's white-box state-space practice as used for
//! the Wasp VCF (Köper, Holters, Esqueda, Parker, DAFx-22, CC BY 4.0),
//! whose measured CA3080 fit (α 0.8635, β 0.9408) the OTA uses.

/// The parts. Values are the schematic's unless the comment says otherwise.
#[derive(Clone, Debug)]
pub struct Parts {
    /// R155 (one of R155/R136/R137), C15: one input at full.
    pub r155: f64,
    pub c15: f64,
    /// R139, A12's feedback; R138 ‖ C18, the LP feedback.
    pub r139: f64,
    pub r138: f64,
    pub c18: f64,
    /// A12, LM301A with C19 = 5 pF: GBW and slew scale as 1/Cc from the
    /// datasheet's 1 MHz and 0.5 V/µs at 30 pF; A0 160 V/mV typ; swing
    /// ±14 V typ into ≥ 10 K.
    pub gbw: f64,
    pub slew: f64,
    pub a0: f64,
    pub rail_a12: f64,
    /// R142/R144, R145/R143 (OTA 1); R148/R150, R151/R149 (OTA 2).
    pub r142: f64,
    pub r144: f64,
    pub r145: f64,
    pub r143: f64,
    pub r148: f64,
    pub r150: f64,
    pub r151: f64,
    pub r149: f64,
    /// CA3080: the Wasp paper's measured output scale and tanh scale.
    pub alpha: f64,
    pub beta_tanh: f64,
    /// Thermal voltage at 27 °C.
    pub vt: f64,
    /// β_F of the input pair: the datasheet's 2 µA bias at 500 µA → 125.
    pub beta_f: f64,
    /// Output resistance × I_ABC: 15 MΩ at 500 µA (datasheet) → 7500 V.
    pub va_ota: f64,
    /// Output headroom to each rail: +13.5 / −14.4 V swing typ on ±15.
    pub hr_up: f64,
    pub hr_dn: f64,
    /// The output transistors' saturation knee (V).
    pub v_knee: f64,
    /// C20/C21 300 pF plus the CA3080's 5.6 pF output and ~3 pF of JFET
    /// C_gd (both to AC ground).
    pub c_int: f64,
    /// 2N4302 (spec IDSS 0.5–5 mA, |V_GS(off)| ≤ 4 V): typical-of-spec
    /// values, our assumption; the follower needs IDSS > 2 mA to reach
    /// +13 V, so a working SEM's parts sit in the upper half.
    pub idss: f64,
    pub vto: f64,
    pub lambda: f64,
    pub is_gate: f64,
    /// R147, R153 to −15.
    pub r147: f64,
    pub r153: f64,
    /// A14, A16 (741) swing.
    pub rail_741: f64,
    /// PS16 50 K and each end's track resistance (our assumption).
    pub pot: f64,
    pub r_end: f64,
    /// R157, R159, R158, R156, C22.
    pub r157: f64,
    pub r159: f64,
    pub r158: f64,
    pub r156: f64,
    pub c22: f64,
    /// 1N4148 (the common SPICE card: IS 2.52 nA, N 1.752, RS 0.568 Ω).
    pub is_d: f64,
    pub n_d: f64,
    pub rs_d: f64,
    /// C23 into R174 + R175, the VCA's input.
    pub c23: f64,
    pub r_vca: f64,
    pub vcc: f64,
    pub vee: f64,
}

impl Default for Parts {
    fn default() -> Self {
        Self {
            r155: 220e3,
            c15: 0.15e-6,
            r139: 100e3,
            r138: 100e3,
            c18: 10e-12,
            gbw: 6e6,
            slew: 3e6,
            a0: 160e3,
            rail_a12: 14.0,
            r142: 100e3,
            r144: 220.0,
            r145: 220.0,
            r143: 1e6,
            r148: 100e3,
            r150: 220.0,
            r151: 220.0,
            r149: 1e6,
            alpha: 0.8635,
            beta_tanh: 0.9408,
            vt: 0.025_852,
            beta_f: 125.0,
            va_ota: 7500.0,
            hr_up: 1.5,
            hr_dn: 0.6,
            v_knee: 0.05,
            c_int: 300e-12 + 5.6e-12 + 3e-12,
            idss: 3e-3,
            vto: -2.0,
            lambda: 0.005,
            is_gate: 1e-14,
            r147: 15e3,
            r153: 15e3,
            rail_741: 13.5,
            pot: 50e3,
            r_end: 100.0,
            r157: 33e3,
            r159: 47e3,
            r158: 10e3,
            r156: 100e3,
            c22: 0.33e-6,
            is_d: 2.52e-9,
            n_d: 1.752,
            rs_d: 0.568,
            c23: 0.15e-6,
            r_vca: 100e3 + 220.0,
            vcc: 15.0,
            vee: -15.0,
        }
    }
}

impl Parts {
    /// A12's input gain, R139/R155.
    pub fn a_in(&self) -> f64 {
        self.r139 / self.r155
    }
    /// The OTA divider, R144/(R142 + R144).
    pub fn a_div(&self) -> f64 {
        self.r144 / (self.r142 + self.r144)
    }
    /// The pair's tanh argument scale, 1/(2·n·V_T) with n = 1/β.
    pub fn k_pair(&self) -> f64 {
        self.beta_tanh / (2.0 * self.vt)
    }
    fn beta_j(&self) -> f64 {
        self.idss / (self.vto * self.vto)
    }
}

// ----------------------------------------------------------------- devices

/// An op-amp's output limit `x/(1 + (x/r)^24)^(1/24)` and its slope:
/// linear to within ~1 % until half a volt from the swing (a 741's or
/// 301's driver saturating), then flat.
pub fn rail(x: f64, r: f64) -> (f64, f64) {
    let u = (x / r).powi(24);
    let q = (1.0 + u).powf(-1.0 / 24.0);
    (x * q, q / (1.0 + u))
}

/// An output transistor's headroom factor: 1 when `x` ≫ the knee, 0 at
/// saturation, reversing beyond (Ebers–Moll's collapse of I_C as V_CE
/// falls), linearly extended past −6 knees.
fn knee(x: f64, vk: f64) -> (f64, f64) {
    let u = x / vk;
    if u > -6.0 {
        let e = (-u).exp();
        (1.0 - e, e / vk)
    } else {
        let e = 6f64.exp();
        (1.0 - e - e * (-6.0 - u), e / vk)
    }
}

fn exp_lim(u: f64) -> (f64, f64) {
    if u < 40.0 {
        let e = u.exp();
        (e, e)
    } else {
        let e = 40f64.exp();
        (e * (1.0 + u - 40.0), e)
    }
}

/// The CA3080: `(i_out, ∂/∂vd, ∂/∂vo, i_b+, ∂i_b+/∂vd, i_b−, ∂i_b−/∂vd)`
/// for `vd = v+ − v−`, output at `vo`, bias `i`. The pair's collector
/// currents are Ebers–Moll's exact forward-active split of the tail,
/// `α_F·I·(1 ± tanh(vd/2nV_T))/2`; the mirrors carry them to the output
/// (scale α) through output devices that saturate at the swing limits.
pub struct Ota {
    pub i_out: f64,
    pub d_vd: f64,
    pub d_vo: f64,
    pub ib_p: f64,
    pub dib_p: f64,
    pub ib_n: f64,
    pub dib_n: f64,
}

pub fn ota(p: &Parts, vd: f64, vo: f64, i: f64) -> Ota {
    let k = p.k_pair();
    let th = (k * vd).tanh();
    let dth = k * (1.0 - th * th);
    let af = p.beta_f / (p.beta_f + 1.0);
    let icp = af * i * (1.0 + th) * 0.5;
    let icn = af * i * (1.0 - th) * 0.5;
    let dic = af * i * dth * 0.5;
    let (up, dup) = knee(p.vcc - p.hr_up - vo, p.v_knee);
    let (dn, ddn) = knee(vo - (p.vee + p.hr_dn), p.v_knee);
    let go = i / p.va_ota;
    Ota {
        i_out: p.alpha * (icp * up - icn * dn) - vo * go,
        d_vd: p.alpha * dic * (up + dn),
        d_vo: p.alpha * (-icp * dup - icn * ddn) - go,
        ib_p: icp / p.beta_f,
        dib_p: dic / p.beta_f,
        ib_n: icn / p.beta_f,
        dib_n: -dic / p.beta_f,
    }
}

/// The 2N4302 follower, drain at V+: `(i_d, ∂/∂vg, ∂/∂vs, i_gs, ∂/∂vgs)`.
/// Shichman–Hodges (saturation and triode) with λ; the gate–source
/// junction a Shockley diode (gate–drain is reverse-biased by ≥ 1.5 V
/// throughout and carries I_s only, folded into nothing).
pub struct Jfet {
    pub id: f64,
    pub d_vg: f64,
    pub d_vs: f64,
    pub igs: f64,
    pub digs: f64,
}

pub fn jfet(p: &Parts, vg: f64, vs: f64) -> Jfet {
    let b = p.beta_j();
    let vgs = vg - vs;
    let vds = p.vcc - vs;
    let vov = vgs - p.vto;
    let (id, d_gs, d_ds) = if vov <= 0.0 {
        (0.0, 0.0, 0.0)
    } else if vds >= vov {
        let cl = 1.0 + p.lambda * vds;
        (
            b * vov * vov * cl,
            2.0 * b * vov * cl,
            b * vov * vov * p.lambda,
        )
    } else {
        let cl = 1.0 + p.lambda * vds;
        let q = 2.0 * vov * vds - vds * vds;
        (
            b * q * cl,
            2.0 * b * vds * cl,
            b * (2.0 * vov - 2.0 * vds) * cl + b * q * p.lambda,
        )
    };
    let (e, de) = exp_lim(vgs / p.vt);
    Jfet {
        id,
        d_vg: d_gs,
        d_vs: -d_gs - d_ds,
        igs: p.is_gate * (e - 1.0),
        digs: p.is_gate * de / p.vt,
    }
}

/// D11/D12 anti-parallel: `(i, di/dv)`.
pub fn diodes(p: &Parts, v: f64) -> (f64, f64) {
    let nvt = p.n_d * p.vt;
    let (ep, dp) = exp_lim(v / nvt);
    let (en, dn) = exp_lim(-v / nvt);
    (p.is_d * (ep - en), p.is_d * (dp + dn) / nvt)
}

// ------------------------------------------------------------------ nodes

pub const N: usize = 15;
const S: usize = 0; // A12's inverting input
const X: usize = 1; // A12's internal state; HP = rail(X)
const I: usize = 2; // C15–R155
const P1: usize = 3;
const N1: usize = 4;
const C1: usize = 5; // OTA 1's output, C20, Q39's gate
const B: usize = 6; // Q39's source: BP
const P2: usize = 7;
const N2: usize = 8;
const C2: usize = 9;
const L: usize = 10; // Q40's source: LP
const W: usize = 11; // PS16's wiper
const T: usize = 12; // R159/R158 tap
const D: usize = 13; // D11/D12 – R156
const J: usize = 14; // R157/R156/C22

/// A capacitor's trapezoidal companion: voltage and current last step.
#[derive(Clone, Copy, Debug, Default)]
struct Cap {
    v: f64,
    i: f64,
}

/// Row scale: KCL residuals in amps × 10 K, so every row reads in volts.
const R0: f64 = 1e4;

#[derive(Clone, Debug)]
pub struct Circuit {
    pub p: Parts,
    /// The step (s).
    pub h: f64,
    pub x: [f64; N],
    c15: Cap,
    c18: Cap,
    c22: Cap,
    c20: Cap,
    c21: Cap,
    /// A12's state derivative last step.
    fa: f64,
    /// C23's high-pass state (the output stage).
    hp_x: f64,
    hp_y: f64,
    /// I_ABC per OTA (A), RES (0..1, the knob), the source (V).
    pub i_abc: f64,
    pub res: f64,
    pub iters: u64,
    pub steps: u64,
    pub fails: u64,
}

/// One step's outputs, in volts at the jacks.
#[derive(Clone, Copy, Debug)]
pub struct Out {
    pub hp: f64,
    pub bp: f64,
    pub lp: f64,
}

impl Circuit {
    /// At rest at `fs`: the followers' gates at their quiescent point, then
    /// 50 ms of silence to settle the offsets.
    pub fn new(p: Parts, fs: f64) -> Self {
        let mut c = Self::unsettled(p, fs);
        for _ in 0..(0.05 * fs) as usize {
            c.step(0.0);
        }
        c
    }

    /// The parts at `fs`, the gates at their quiescent point, not stepped:
    /// enough for the static helpers (`gate_for`, `follower_gain`,
    /// `i_for`), which read only the parts. The lab built these with `new`;
    /// the settle changes none of their results.
    pub fn unsettled(p: Parts, fs: f64) -> Self {
        let mut c = Self {
            p,
            h: 1.0 / fs,
            x: [0.0; N],
            c15: Cap::default(),
            c18: Cap::default(),
            c22: Cap::default(),
            c20: Cap::default(),
            c21: Cap::default(),
            fa: 0.0,
            hp_x: 0.0,
            hp_y: 0.0,
            i_abc: 50e-6,
            res: 0.0,
            iters: 0,
            steps: 0,
            fails: 0,
        };
        // The followers' quiescent gate: BP = LP = 0 needs 1 mA into R147.
        let vg = c.gate_for(0.0);
        c.x[C1] = vg;
        c.x[C2] = vg;
        c.c20.v = vg;
        c.c21.v = vg;
        c
    }

    /// The gate voltage that holds the source at `vs` (follower static).
    pub fn gate_for(&self, vs: f64) -> f64 {
        let p = &self.p;
        let need = (vs - p.vee) / p.r147 + vs / (p.r148 + p.r150);
        let mut vg = vs + p.vto * 0.5;
        for _ in 0..100 {
            let j = jfet(p, vg, vs);
            let f = j.id + j.igs - need;
            let df = j.d_vg + j.digs;
            let d = f / df;
            vg -= d;
            if d.abs() < 1e-13 {
                break;
            }
        }
        vg
    }

    /// The follower's small-signal gain at `vs`.
    pub fn follower_gain(&self, vs: f64) -> f64 {
        let e = 1e-4;
        2.0 * e / (self.gate_for(vs + e) - self.gate_for(vs - e))
    }

    /// I_ABC per OTA for a small-signal cutoff `fc`: ω = gm·A_div·g_f/C
    /// per integrator, gm = α·I·β/(2V_T) — how T11/T12 would be trimmed.
    pub fn i_for(&self, fc: f64) -> f64 {
        let p = &self.p;
        let gf = self.follower_gain(0.0);
        2.0 * std::f64::consts::PI * fc * p.c_int / (p.alpha * p.k_pair() * p.a_div() * gf)
    }

    fn pots(&self) -> (f64, f64) {
        let p = &self.p;
        let r = self.res.clamp(0.0, 1.0);
        (p.r_end + p.pot * r, p.r_end + p.pot * (1.0 - r))
    }

    fn a12(&self, xs: f64, s: f64) -> (f64, f64, f64) {
        let p = &self.p;
        let w = 2.0 * std::f64::consts::PI * p.gbw;
        let kk = w / p.slew;
        let th = (-kk * s).tanh();
        let wp = w / p.a0;
        let gw = 1e7;
        let (rx, drx) = rail(xs, p.rail_a12 + 0.3);
        let f = p.slew * th - wp * xs - gw * (xs - rx);
        let df_s = -p.slew * kk * (1.0 - th * th);
        let df_x = -wp - gw * (1.0 - drx);
        (f, df_s, df_x)
    }

    /// Residuals (rows in volts) and, if asked, the Jacobian.
    fn eval(&self, x: &[f64; N], src: f64, jac: Option<&mut [[f64; N]; N]>) -> [f64; N] {
        let p = &self.p;
        let h = self.h;
        let mut f = [0.0; N];
        let (hp, dhp) = rail(x[X], p.rail_a12);
        let (a14, da14) = rail(x[B], p.rail_741);
        let g15 = 2.0 * p.c15 / h;
        let g18 = 2.0 * p.c18 / h;
        let g22 = 2.0 * p.c22 / h;
        let gc = 2.0 * p.c_int / h;
        let i15 = g15 * ((src - x[I]) - self.c15.v) - self.c15.i;
        let i18 = g18 * ((x[L] - x[S]) - self.c18.v) - self.c18.i;
        let i22 = g22 * ((x[J] - x[S]) - self.c22.v) - self.c22.i;
        let i20 = gc * (x[C1] - self.c20.v) - self.c20.i;
        let i21 = gc * (x[C2] - self.c21.v) - self.c21.i;
        let o1 = ota(p, x[P1] - x[N1], x[C1], self.i_abc);
        let o2 = ota(p, x[P2] - x[N2], x[C2], self.i_abc);
        let j1 = jfet(p, x[C1], x[B]);
        let j2 = jfet(p, x[C2], x[L]);
        let (rt, rb) = self.pots();
        let rd = p.r156 + p.rs_d;
        let (id, did) = diodes(p, x[T] - x[D]);
        let (fa, fa_s, fa_x) = self.a12(x[X], x[S]);

        f[S] = R0
            * ((x[S] - x[I]) / p.r155 + (x[S] - hp) / p.r139 + (x[S] - x[L]) / p.r138 - i18 - i22);
        f[X] = x[X] - self.x[X] - 0.5 * h * (fa + self.fa);
        f[I] = R0 * ((x[I] - x[S]) / p.r155 - i15);
        f[P1] = R0 * ((x[P1] - hp) / p.r142 + x[P1] / p.r144 + o1.ib_p);
        f[N1] = R0 * (x[N1] / p.r145 + x[N1] / p.r143 + o1.ib_n);
        f[C1] = R0 * (i20 - o1.i_out + j1.igs);
        f[B] = R0 * (-j1.id - j1.igs + (x[B] - p.vee) / p.r147 + (x[B] - x[P2]) / p.r148);
        f[P2] = R0 * ((x[P2] - x[B]) / p.r148 + x[P2] / p.r150 + o2.ib_p);
        f[N2] = R0 * (x[N2] / p.r151 + x[N2] / p.r149 + o2.ib_n);
        f[C2] = R0 * (i21 - o2.i_out + j2.igs);
        f[L] = R0 * (-j2.id - j2.igs + (x[L] - p.vee) / p.r153 + (x[L] - x[S]) / p.r138 + i18);
        f[W] = R0 * ((x[W] - a14) / rt + x[W] / rb + (x[W] - x[J]) / p.r157);
        f[T] = R0 * ((x[T] - a14) / p.r159 + x[T] / p.r158 + id);
        f[D] = R0 * (-id + (x[D] - x[J]) / rd);
        f[J] = R0 * ((x[J] - x[W]) / p.r157 + (x[J] - x[D]) / rd + i22);

        if let Some(m) = jac {
            *m = [[0.0; N]; N];
            let mut s = |r: usize, c: usize, v: f64| m[r][c] += v;
            // S
            s(
                S,
                S,
                R0 * (1.0 / p.r155 + 1.0 / p.r139 + 1.0 / p.r138 + g18 + g22),
            );
            s(S, I, -R0 / p.r155);
            s(S, X, -R0 * dhp / p.r139);
            s(S, L, -R0 * (1.0 / p.r138 + g18));
            s(S, J, -R0 * g22);
            // X
            s(X, X, 1.0 - 0.5 * h * fa_x);
            s(X, S, -0.5 * h * fa_s);
            // I
            s(I, I, R0 * (1.0 / p.r155 + g15));
            s(I, S, -R0 / p.r155);
            // P1, N1
            s(P1, P1, R0 * (1.0 / p.r142 + 1.0 / p.r144 + o1.dib_p));
            s(P1, N1, -R0 * o1.dib_p);
            s(P1, X, -R0 * dhp / p.r142);
            s(N1, N1, R0 * (1.0 / p.r145 + 1.0 / p.r143 - o1.dib_n));
            s(N1, P1, R0 * o1.dib_n);
            // C1
            s(C1, C1, R0 * (gc - o1.d_vo + j1.digs));
            s(C1, P1, -R0 * o1.d_vd);
            s(C1, N1, R0 * o1.d_vd);
            s(C1, B, -R0 * j1.digs);
            // B
            s(
                B,
                B,
                R0 * (-j1.d_vs + j1.digs + 1.0 / p.r147 + 1.0 / p.r148),
            );
            s(B, C1, R0 * (-j1.d_vg - j1.digs));
            s(B, P2, -R0 / p.r148);
            // P2, N2
            s(P2, P2, R0 * (1.0 / p.r148 + 1.0 / p.r150 + o2.dib_p));
            s(P2, N2, -R0 * o2.dib_p);
            s(P2, B, -R0 / p.r148);
            s(N2, N2, R0 * (1.0 / p.r151 + 1.0 / p.r149 - o2.dib_n));
            s(N2, P2, R0 * o2.dib_n);
            // C2
            s(C2, C2, R0 * (gc - o2.d_vo + j2.digs));
            s(C2, P2, -R0 * o2.d_vd);
            s(C2, N2, R0 * o2.d_vd);
            s(C2, L, -R0 * j2.digs);
            // L
            s(
                L,
                L,
                R0 * (-j2.d_vs + j2.digs + 1.0 / p.r153 + 1.0 / p.r138 + g18),
            );
            s(L, C2, R0 * (-j2.d_vg - j2.digs));
            s(L, S, -R0 * (1.0 / p.r138 + g18));
            // W
            s(W, W, R0 * (1.0 / rt + 1.0 / rb + 1.0 / p.r157));
            s(W, B, -R0 * da14 / rt);
            s(W, J, -R0 / p.r157);
            // T
            s(T, T, R0 * (1.0 / p.r159 + 1.0 / p.r158 + did));
            s(T, D, -R0 * did);
            s(T, B, -R0 * da14 / p.r159);
            // D
            s(D, D, R0 * (did + 1.0 / rd));
            s(D, T, -R0 * did);
            s(D, J, -R0 / rd);
            // J
            s(J, J, R0 * (1.0 / p.r157 + 1.0 / rd + g22));
            s(J, W, -R0 / p.r157);
            s(J, D, -R0 / rd);
            s(J, S, -R0 * g22);
        }
        f
    }

    /// One step at `src` volts at the input jack.
    pub fn step(&mut self, src: f64) -> Out {
        let mut x = self.x;
        let mut jm = [[0.0; N]; N];
        let mut ok = false;
        for _ in 0..80 {
            self.iters += 1;
            let f = self.eval(&x, src, Some(&mut jm));
            let mut dx = f;
            if !lu_solve(&mut jm, &mut dx) {
                break;
            }
            let big = dx.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let sc = if big > 1.0 { 1.0 / big } else { 1.0 };
            for k in 0..N {
                x[k] -= sc * dx[k];
            }
            if big < 1e-10 {
                ok = true;
                break;
            }
        }
        if !ok {
            self.fails += 1;
        }
        self.steps += 1;
        self.commit(&x, src);
        let p = &self.p;
        Out {
            hp: rail(x[X], p.rail_a12).0,
            bp: rail(x[B], p.rail_741).0,
            lp: rail(x[L], p.rail_741).0,
        }
    }

    fn commit(&mut self, x: &[f64; N], src: f64) {
        let p = &self.p;
        let h = self.h;
        let upd = |c: &mut Cap, cap: f64, v: f64| {
            let g = 2.0 * cap / h;
            let i = g * (v - c.v) - c.i;
            c.v = v;
            c.i = i;
        };
        upd(&mut self.c15, p.c15, src - x[I]);
        upd(&mut self.c18, p.c18, x[L] - x[S]);
        upd(&mut self.c22, p.c22, x[J] - x[S]);
        upd(&mut self.c20, p.c_int, x[C1]);
        upd(&mut self.c21, p.c_int, x[C2]);
        self.fa = self.a12(x[X], x[S]).0;
        self.x = *x;
    }

    /// C23 into the VCA's input: the output stage's high-pass, bilinear.
    pub fn out_stage(&mut self, v: f64) -> f64 {
        let tau = self.p.c23 * self.p.r_vca;
        let a = 2.0 * tau / self.h;
        let y = (a * (v - self.hp_x) + (a - 1.0) * self.hp_y) / (a + 1.0);
        self.hp_x = v;
        self.hp_y = y;
        y
    }

    /// Node voltages, for the DC report.
    pub fn nodes(&self) -> [(&'static str, f64); N] {
        let n = [
            "S", "X(HP)", "I", "P1", "N1", "C1", "BP", "P2", "N2", "C2", "LP", "W", "T", "D", "J",
        ];
        let mut o = [("", 0.0); N];
        for k in 0..N {
            o[k] = (n[k], self.x[k]);
        }
        o
    }

    #[cfg(test)]
    pub fn check_jacobian(&self, x: &[f64; N], src: f64) -> f64 {
        let mut jm = [[0.0; N]; N];
        let f0 = self.eval(x, src, Some(&mut jm));
        let mut worst = 0.0f64;
        for c in 0..N {
            let mut xp = *x;
            let e = 1e-7 * (1.0 + x[c].abs());
            xp[c] += e;
            let f1 = self.eval(&xp, src, None);
            for r in 0..N {
                let fd = (f1[r] - f0[r]) / e;
                let sc = jm[r][c].abs().max(fd.abs()).max(1e-3);
                worst = worst.max((fd - jm[r][c]).abs() / sc);
            }
        }
        worst
    }
}

/// In-place partial-pivot LU solve of `a·x = b`; `b` becomes `x`.
pub fn lu_solve(a: &mut [[f64; N]; N], b: &mut [f64; N]) -> bool {
    for k in 0..N {
        let mut piv = k;
        for r in k + 1..N {
            if a[r][k].abs() > a[piv][k].abs() {
                piv = r;
            }
        }
        if a[piv][k].abs() < 1e-300 {
            return false;
        }
        a.swap(k, piv);
        b.swap(k, piv);
        let d = 1.0 / a[k][k];
        for r in k + 1..N {
            let m = a[r][k] * d;
            if m != 0.0 {
                for c in k..N {
                    a[r][c] -= m * a[k][c];
                }
                b[r] -= m * b[k];
            }
        }
    }
    for k in (0..N).rev() {
        let mut s = b[k];
        for c in k + 1..N {
            s -= a[k][c] * b[c];
        }
        b[k] = s / a[k][k];
    }
    true
}

#[cfg(test)]
mod tests {
    //! The reference's checks against the datasheets and the schematic
    //! (`docs/sem1a/validation.md` §§ 1–3), and its solver.
    use super::*;

    fn near(v: f64, want: f64, tol: f64, what: &str) {
        assert!((v - want).abs() <= tol, "{what}: {v} vs {want} ± {tol}");
    }

    /// The analytic Jacobian against finite differences along a 6 V
    /// drive at ×8, RES 0.7, 1 kHz: under 1e-4 relative (the lab's
    /// bound), and every step converged.
    #[test]
    fn jacobian_matches_finite_differences() {
        let mut c = Circuit::unsettled(Parts::default(), 384_000.0);
        c.res = 0.7;
        c.i_abc = c.i_for(1000.0);
        let mut worst = 0.0f64;
        for k in 0..2000 {
            let v = 6.0 * (k as f64 * 0.05).sin();
            c.step(v);
            if k % 97 == 0 {
                worst = worst.max(c.check_jacobian(&c.x.clone(), v));
            }
        }
        assert!(worst < 1e-4, "worst relative Jacobian error {worst}");
        assert_eq!(c.fails, 0);
    }

    /// § 1: silence at ×8, settled. HP, BP and LP sit within 5 mV of 0
    /// (base-current offsets, which T9/T10 would trim), S at 0, the
    /// followers' gates one V_GS below at 1 mA (−0.886 V) within 5 mV;
    /// each node as the lab recorded it to 2e-5 V.
    #[test]
    fn dc_operating_point() {
        let c = Circuit::new(Parts::default(), 384_000.0);
        let want = [
            0.00000, -0.00351, 0.00000, -0.00005, -0.00004, -0.88995, -0.00349, -0.00005, -0.00004,
            -0.88253, 0.00361, -0.00348, -0.00061, -0.00340, -0.00344,
        ];
        for ((n, v), w) in c.nodes().into_iter().zip(want) {
            near(v, w, 2e-5, n);
        }
        let vg = c.gate_for(0.0);
        near(vg, -0.8863, 1e-4, "quiescent gate");
        for k in [X, B, L] {
            assert!(c.x[k].abs() < 5e-3, "node {k}: {}", c.x[k]);
        }
        near(c.x[C1], vg, 5e-3, "C1");
        near(c.x[C2], vg, 5e-3, "C2");
        assert_eq!(c.fails, 0);
    }

    /// § 2: the CA3080 on its own against FN475 and the Wasp paper's unit:
    /// gm at 500 µA 7794 µS (the paper's α·β·I/2V_T 7856, inside the
    /// datasheet's 6700–13000), peak current 428 µA (350–650), swing
    /// +13.50 / −14.40 V, R_out 15 MΩ, and the transfer at 50 µA.
    #[test]
    fn ca3080_alone() {
        let p = Parts::default();
        let gm = ota(&p, 1e-6, 0.0, 500e-6).i_out / 1e-6;
        near(gm * 1e6, 7794.0, 1.0, "gm µS");
        assert!((6700e-6..13000e-6).contains(&gm));
        let wasp = p.alpha * p.beta_tanh * 500e-6 / (2.0 * p.vt);
        near(gm / wasp, 1.0, 0.01, "gm against the Wasp unit");
        let pk = ota(&p, 1.0, 0.0, 500e-6).i_out;
        near(pk * 1e6, 428.0, 1.0, "peak µA");
        let swing = |vd: f64| {
            let (mut lo, mut hi) = (-15.0, 15.0);
            for _ in 0..200 {
                let m = 0.5 * (lo + hi);
                if ota(&p, vd, m, 500e-6).i_out > 0.0 {
                    lo = m
                } else {
                    hi = m
                }
            }
            0.5 * (lo + hi)
        };
        near(swing(0.2), 13.50, 0.01, "swing up");
        near(swing(-0.2), -14.40, 0.01, "swing down");
        near(p.va_ota / 500e-6, 15e6, 1.0, "R_out");
        for (vd, want) in [(0.01, 0.154), (0.025, 0.365), (0.05, 0.618), (0.1, 0.813)] {
            near(
                ota(&p, vd, 0.0, 50e-6).i_out / 50e-6,
                want,
                6e-4,
                "transfer",
            );
        }
    }

    /// § 3: the 2N4302 followers (IDSS 3 mA, V_P −2 V, assumed): gate and
    /// gain against the source, the lab's table to its digits; the gain
    /// rises 0.872 → 0.966 from −12 to +13 V (the circuit's even order),
    /// and the gate barely conducts at +13 V.
    #[test]
    fn followers() {
        let c = Circuit::unsettled(Parts::default(), 384_000.0);
        for (vs, vg, gain) in [
            (-12.0, -13.693, 0.8716),
            (-6.0, -7.193, 0.9442),
            (0.0, -0.886, 0.9567),
            (6.0, 5.365, 0.9624),
            (12.0, 11.588, 0.9656),
            (13.0, 12.623, 0.9660),
        ] {
            near(c.gate_for(vs), vg, 6e-4, "gate");
            near(c.follower_gain(vs), gain, 6e-5, "gain");
        }
        let j = jfet(&c.p, c.gate_for(13.0), 13.0);
        near(j.igs, -1.0e-14, 1e-16, "gate current at +13 V");
    }
}
