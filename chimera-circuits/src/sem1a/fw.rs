//! The firmware-feasible SEM (`sem-circuit-lab` aa4cb6a2): the same circuit
//! at 48 kHz ×1 in f32, its parts folded into per-block coefficients and
//! five cheap per-sample terms. Derived from `reference` (every constant
//! comes from `Parts`). Shipped as filter KIND SEM (ADR 0192,
//! `chimera-engine/src/dsp/filter/sem.rs`); `export` reproduces its
//! constants and fixtures, and the tests hold the two equal.
//!
//! Per sample: Zavalishin's TPT SVF solve (one divide a block, or one a
//! sample while the controls move), in the circuit's own sign and volts:
//! - A12: `hp = rail(−u − lp − k·bp − D(bp))`, ±14 V.
//! - CA3080: each integrator input through `V0·tanh(v/V0)`, V0 = 2V_T /
//!   (β·A_div) (the Wasp fit's β), as its series to v⁵.
//! - Q39/Q40: the follower's slope against its own output, `r(b)` =
//!   F′(G(b))/F′(0), scales each integrator's step (the chain rule on
//!   `b = F(v_C)`): a cubic fitted to the 2N4302 model. This is the even-
//!   order term; the textbook SVF has none.
//! - D11/D12: the antisaturator `D(bp)` on the TPT state, a C1 dead-zone
//!   fit to the Shockley solve (the old lab's fit; the network is the same).
//! - The OTA swing and the followers' limits clamp the states, asymmetric
//!   (−12.7 / +13.9 V at the sources), A14/A16 clamp at ±13.5 V.
//!
//! Per block, from the parts: the RES pot law (PS16's Thevenin into R157),
//! the integrators' leak, C18's lead, A12's lag, C22's coupling, and the
//! trimmed cutoff's droop.

use super::BLOCK;
use super::reference::{Circuit, Parts};
use super::validate::{k_parasitic, k_pot};
use crate::FS;
use crate::util::sig::resample_poly;
use std::f64::consts::PI;

/// Volts at one SEM input for digital 1.0: one VCO saw at the mixer's
/// top, 7.2 V p-p (VCO sheet: the reset comparator's thresholds, 8.0 and
/// 16.7 V at Q4, through A4's −75 K/91 K; our derivation).
pub const VCAL: f64 = 3.6;

#[derive(Clone, Debug)]
pub struct Coefs {
    pub a_in: f32,
    pub v0: f32,
    pub rail_hp: f32,
    pub lo: f32,
    pub hi: f32,
    pub rail_out: f32,
    /// `r(b) = 1 + r1·b + … + r6·b⁶`.
    pub r: [f32; RDEG],
    /// Diode fit: `kd·φ(|b| − b0)` with knee width `w`.
    pub b0: f32,
    pub w: f32,
    pub kd: f32,
    /// The trim: the reference's f0/fc at 1 kHz, and its droop per Hz.
    pub droop: f64,
}

impl Coefs {
    pub fn from(p: &Parts) -> Self {
        let c = Circuit::unsettled(p.clone(), 384_000.0);
        let g0 = c.follower_gain(0.0);
        // Least squares on 0.25 V steps over the followers' range.
        // Weighted 1/(1 + (b/4 V)²): the slope at 0 sets H2 at every level.
        let pts: Vec<(f64, f64, f64)> = (-50..=54)
            .map(|i| {
                let b = i as f64 * 0.25;
                (
                    b,
                    c.follower_gain(b) / g0 - 1.0,
                    1.0 / (1.0 + (b / 2.5) * (b / 2.5)),
                )
            })
            .collect();
        let r = lsq(&pts);
        // The sources' limits: the gate at the OTA's swing.
        let src_at = |vg: f64| {
            let (mut lo, mut hi) = (-15.0, 15.0);
            for _ in 0..100 {
                let m = 0.5 * (lo + hi);
                if c.gate_for(m) > vg { hi = m } else { lo = m }
            }
            0.5 * (lo + hi)
        };
        let lo = src_at(p.vee + p.hr_dn);
        let hi = src_at(p.vcc - p.hr_up).min(p.rail_741 + 0.4);
        Self {
            a_in: p.a_in() as f32,
            v0: (1.0 / (p.k_pair() * p.a_div())) as f32,
            rail_hp: p.rail_a12 as f32,
            lo: lo as f32,
            hi: hi as f32,
            rail_out: p.rail_741 as f32,
            r: r.map(|v| v as f32),
            b0: 0.10,
            w: 3.32,
            kd: 0.1540,
            droop: 1.9e-6,
        }
    }
}

/// Degree of the follower's rate polynomial.
pub const RDEG: usize = 3;

/// `y ≈ Σ c_i·x^(i+1)`, i < RDEG, weighted least squares on `x/13`
/// (normal equations, conditioned), returned in powers of `x`.
pub fn lsq(pts: &[(f64, f64, f64)]) -> [f64; RDEG] {
    const SC: f64 = 13.0;
    let mut m = [[0.0; RDEG + 1]; RDEG];
    for &(x, y, w) in pts {
        let u = x / SC;
        let mut ph = [0.0; RDEG];
        let mut q = u;
        for v in ph.iter_mut() {
            *v = q;
            q *= u;
        }
        for i in 0..RDEG {
            m[i][RDEG] += w * ph[i] * y;
            for j in 0..RDEG {
                m[i][j] += w * ph[i] * ph[j];
            }
        }
    }
    for k in 0..RDEG {
        for r in k + 1..RDEG {
            let f = m[r][k] / m[k][k];
            for c in k..=RDEG {
                m[r][c] -= f * m[k][c];
            }
        }
    }
    let mut o = [0.0; RDEG];
    for k in (0..RDEG).rev() {
        let mut v = m[k][RDEG];
        for c in k + 1..RDEG {
            v -= m[k][c] * o[c];
        }
        o[k] = v / m[k][k];
    }
    let mut sc = SC;
    for v in o.iter_mut() {
        *v /= sc;
        sc *= SC;
    }
    o
}

/// `g` and `k` for (fc, RES): the trimmed cutoff with its droop, the pot
/// law plus the parasitic damping at that cutoff.
pub fn coefs(p: &Parts, cf: &Coefs, c: &Circuit, fc: f64, res: f64) -> (f64, f64) {
    let f = fc * (1.0 - cf.droop * (fc - 1000.0));
    let g = (PI * f.min(0.45 * FS) / FS).tan();
    let kp = k_pot(p, res);
    let (lk, c18, lag, c22) = k_parasitic(c, fc, kp);
    (g, kp + lk + c18 - lag + c22)
}

#[inline(always)]
fn ota(cf: &Coefs, v: f32) -> f32 {
    let a3 = 1.0 / (3.0 * cf.v0 * cf.v0);
    let a5 = 2.0 / (15.0 * cf.v0 * cf.v0 * cf.v0 * cf.v0);
    let v2 = v * v;
    v - v * v2 * (a3 - a5 * v2)
}

#[inline(always)]
fn diode(cf: &Coefs, b: f32) -> f32 {
    let e = b.abs() - cf.b0;
    let y = if e <= 0.0 {
        0.0
    } else if e < cf.w {
        e * e * (0.5 / cf.w)
    } else {
        e - 0.5 * cf.w
    };
    (cf.kd * y).copysign(b)
}

#[inline(always)]
fn rate(cf: &Coefs, b: f32) -> f32 {
    let mut a = 0.0;
    for &c in cf.r.iter().rev() {
        a = (a + c) * b;
    }
    1.0 + a
}

#[derive(Clone, Debug)]
pub struct Fw {
    pub cf: Coefs,
    s1: f32,
    s2: f32,
    /// Fold the followers' rate into the solve (a divide a sample).
    pub exact: bool,
}

impl Fw {
    pub fn new(p: &Parts) -> Self {
        Self {
            cf: Coefs::from(p),
            s1: 0.0,
            s2: 0.0,
            exact: false,
        }
    }

    /// One sample at `u` volts into the loop (circuit sign: the summer
    /// adds −u): `(hp, bp, lp)` in volts.
    #[inline(always)]
    pub fn tick(&mut self, u: f32, g: f32, k: f32, d: f32) -> (f32, f32, f32) {
        let cf = &self.cf;
        let (s1, s2) = (self.s1, self.s2);
        let dd = diode(cf, s1);
        let (r1, r2) = (rate(cf, s1), rate(cf, s2));
        let (g1, g2, dd_) = if self.exact {
            let g1 = g * r1;
            let g2 = g * r2;
            (g1, g2, 1.0 / (1.0 + k * g1 + g1 * g2))
        } else {
            (g * r1, g * r2, d)
        };
        let hp = ((-u - dd - (k + g) * s1 - s2) * dd_).clamp(-cf.rail_hp, cf.rail_hp);
        let v1 = g1 * ota(cf, hp);
        let bp = s1 + v1;
        self.s1 = (bp + v1).clamp(cf.lo, cf.hi);
        let v2 = g2 * ota(cf, bp);
        let lp = s2 + v2;
        self.s2 = (lp + v2).clamp(cf.lo, cf.hi);
        let r = cf.rail_out;
        (hp, bp.clamp(-r, r), lp.clamp(-r, r))
    }
}

/// A render's input and controls, per 48 kHz sample; `x` digital.
#[derive(Clone)]
pub struct Case {
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub res: Vec<f64>,
    pub morph: Vec<f64>,
    pub bp: bool,
    /// LEVEL's gain (¼ … 4).
    pub drive: f64,
}

/// The output's volts back to digital (the SEM inverts; 1.0 = the LP
/// passband for a unit input), drive left in.
pub fn norm(p: &Parts) -> f64 {
    -1.0 / (p.a_in() * VCAL)
}

/// A one-pole high-pass at `f` Hz, bilinear at 48 kHz (C15 into R155 on
/// the way in, C23 into the VCA's 100 K on the way out).
pub fn hp1(x: &[f64], f: f64) -> Vec<f64> {
    let a = 1.0 / (PI * f / FS).tan();
    let (mut xp, mut yp) = (0.0, 0.0);
    x.iter()
        .map(|&v| {
            let y = (a * (v - xp) + (a - 1.0) * yp) / (a + 1.0);
            xp = v;
            yp = y;
            y
        })
        .collect()
}

/// The input and output couplings' corners (Hz).
pub fn couplings(p: &Parts) -> (f64, f64) {
    (
        1.0 / (2.0 * PI * p.r155 * p.c15),
        1.0 / (2.0 * PI * p.r_vca * p.c23),
    )
}

/// The firmware model on a case: controls read once a block and ramped,
/// the couplings on either side.
pub fn render(p: &Parts, c: &Case, exact: bool) -> Vec<f64> {
    let (f_in, f_out) = couplings(p);
    let xin = hp1(&c.x, f_in);
    let mut m = Fw::new(p);
    m.exact = exact;
    let refc = Circuit::unsettled(p.clone(), 384_000.0);
    let n = c.x.len();
    let at = |v: &[f64], i: usize| v[i.min(n - 1)];
    let cf = m.cf.clone();
    let gk = |i: usize| coefs(p, &cf, &refc, at(&c.fc, i), at(&c.res, i));
    let mut from = gk(0);
    let gain = (VCAL * c.drive * p.a_in()) as f32;
    let nm = norm(p);
    let mut y = Vec::with_capacity(n);
    for b0 in (0..n).step_by(BLOCK) {
        let to = gk(b0 + BLOCK - 1);
        for i in b0..(b0 + BLOCK).min(n) {
            let t = (i - b0 + 1) as f64 / BLOCK as f64;
            let g = (from.0 + (to.0 - from.0) * t) as f32;
            let k = (from.1 + (to.1 - from.1) * t) as f32;
            let d = 1.0 / (1.0 + k * g + g * g);
            let (h, b, l) = m.tick(gain * xin[i] as f32, g, k, d);
            let mm = c.morph[i] as f32;
            let v = if c.bp { b } else { l + mm * (h - l) };
            y.push(v as f64 * nm);
        }
        from = to;
    }
    hp1(&y, f_out)
}

/// The reference on a case: ×8, controls interpolated per step, C23 out.
pub fn render_ref(p: &Parts, c: &Case, os: usize) -> Vec<f64> {
    let fs = FS * os as f64;
    let mut r = Circuit::new(p.clone(), fs);
    let trim = trim(p);
    let x = resample_poly(&c.x, os, 1);
    let n = c.x.len();
    let at = |v: &[f64], j: usize| {
        let f = j as f64 / os as f64;
        let i = (f as usize).min(n - 1);
        let t = f - i as f64;
        v[i] + (v[(i + 1).min(n - 1)] - v[i]) * t
    };
    let mut y = Vec::with_capacity(x.len());
    let gain = VCAL * c.drive;
    for (j, &xv) in x.iter().enumerate() {
        r.i_abc = r.i_for(at(&c.fc, j)) * trim;
        r.res = at(&c.res, j);
        let o = r.step(gain * xv);
        let m = at(&c.morph, j);
        let v = if c.bp {
            o.bp
        } else {
            (1.0 - m) * o.lp + m * o.hp
        };
        y.push(r.out_stage(v));
    }
    let nm = norm(p);
    let mut y = resample_poly(&y, 1, os);
    y.resize(n, 0.0);
    y.iter_mut().for_each(|v| *v *= nm);
    y
}

/// The INIT FREQ trim: I_ABC × this puts the BP peak at fc at 1 kHz (the
/// small-signal f0/fc there is 0.9901, `validate` § 5).
pub fn trim(_p: &Parts) -> f64 {
    1.0 / 0.9901
}

#[cfg(test)]
mod tests {
    //! The firmware model against the reference: the circuit lab's probes
    //! (`docs/sem1a/probes.md`) and its 75-job A/B (`docs/sem1a/ab-table.md`)
    //! as assertions. All at ×8, so all ignored but the cheap ones.
    use super::super::stimuli::{self, burst, ring_ms, sine};
    use super::super::{OS, harm, lsd_matched, null_ls, par_map, tail};
    use super::*;
    use crate::util::measure::{db, null_db, tone};

    fn case(x: Vec<f64>, fc: f64, res: f64, drive: f64) -> Case {
        let n = x.len();
        Case {
            x,
            fc: vec![fc; n],
            res: vec![res; n],
            morph: vec![0.0; n],
            bp: false,
            drive,
        }
    }

    /// The follower fit against the 2N4302 model, the lab's table to 4
    /// digits: `r(b)` within 0.8 % of the model's slope from −10 to +12 V;
    /// the cubic misses only the last volts before each limit (−12.5 V:
    /// 0.927 against 0.869; +13.5 V: 1.013 against 0.920).
    #[test]
    fn follower_rate_fit() {
        let p = Parts::default();
        let cf = Coefs::from(&p);
        let c = Circuit::unsettled(p, 384_000.0);
        let g0 = c.follower_gain(0.0);
        for (b, model, want) in [
            (-12.5, 0.8688, 0.9271),
            (-12.0, 0.9111, 0.9332),
            (-10.0, 0.9612, 0.9544),
            (-8.0, 0.9779, 0.9707),
            (-4.0, 0.9928, 0.9912),
            (-1.0, 0.9986, 0.9986),
            (1.0, 1.0013, 1.0010),
            (4.0, 1.0044, 1.0023),
            (8.0, 1.0073, 1.0034),
            (12.0, 1.0093, 1.0087),
            (13.5, 0.9196, 1.0127),
        ] {
            let fit = 1.0 + cf.r.iter().rev().fold(0.0f64, |a, &c| (a + c as f64) * b);
            let m = c.follower_gain(b) / g0;
            assert!((fit - want).abs() < 6e-5, "{b} V: fit {fit}");
            assert!((m - model).abs() < 6e-5, "{b} V: model {m}");
            if (-10.0..=12.0).contains(&b) {
                assert!((fit - m).abs() < 0.008, "{b} V: {fit} vs {m}");
            }
        }
    }

    /// The fw's MODE notch (fc 1 kHz, RES 0, −12 dB, a 1/48-octave sweep)
    /// at 2980 / 1722 / 995 / 575 / 332 Hz, as recorded (to 1 Hz), within
    /// 0.65 % of the reference's 2970 / 1715 / 990 / 572 / 330 Hz (0.34 …
    /// 0.61 %; ADR 0192's "within 0.4 %" holds only for m 0.1 and ¼), and
    /// deeper than −54 dB (−55 … −63 recorded).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn fw_notch_is_the_references() {
        let p = Parts::default();
        let ms = [
            (0.1, 2970.0),
            (0.25, 1715.0),
            (0.5, 990.0),
            (0.75, 572.0),
            (0.9, 330.0),
        ];
        let got = par_map(&ms, |&(m, _)| {
            let mut best = (0.0, f64::MAX);
            let mut f = 150.0;
            while f <= 8000.0 {
                let mut c = case(sine(f, 0.3), 1000.0, 0.0, 0.25);
                c.morph = vec![m; c.x.len()];
                let g = db(tone(tail(&render(&p, &c, false)), f) / 0.25);
                if g < best.1 {
                    best = (f, g);
                }
                f *= 2f64.powf(1.0 / 48.0);
            }
            best
        });
        let fw = [2980.0, 1722.0, 995.0, 575.0, 332.0];
        for (((m, rn), (f, d)), fw) in ms.into_iter().zip(got).zip(fw) {
            eprintln!("m {m}: {f:.0} Hz {d:.1} dB");
            assert!((f - fw).abs() < 1.0, "m {m}: {f} vs {fw}");
            assert!((f / rn - 1.0).abs() < 0.0065, "m {m}: {f} vs {rn}");
            assert!(d < -54.0, "m {m}: {d}");
        }
    }

    /// Gain at fc and harmonics (1 kHz at fc 1 kHz, LP), RES 0 / ½ / 0.9 /
    /// 1 × −12 / 0 / +12 dB: the reference's gain as recorded (to 0.06
    /// dB), the fw's within 0.25 dB of it (recorded: within 0.2), H2
    /// within 2.5 dB (recorded: within 2).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn gain_at_fc_and_h2() {
        let p = Parts::default();
        let grid: [(f64, f64, f64); 12] = [
            (0.0, -12.0, -9.6),
            (0.0, 0.0, -9.6),
            (0.0, 12.0, -9.7),
            (0.5, -12.0, -0.9),
            (0.5, 0.0, -1.1),
            (0.5, 12.0, -1.7),
            (0.9, -12.0, 10.5),
            (0.9, 0.0, 8.7),
            (0.9, 12.0, 5.5),
            (1.0, -12.0, 20.1),
            (1.0, 0.0, 15.4),
            (1.0, 12.0, 5.7),
        ];
        let rows = par_map(&grid, |&(r, l, _)| {
            let g = 10f64.powf(l / 20.0);
            let c = case(sine(1000.0, 0.6), 1000.0, r, g);
            let a = harm(tail(&render_ref(&p, &c, OS)), 1000.0, g);
            let b = harm(tail(&render(&p, &c, false)), 1000.0, g);
            (a, b)
        });
        for ((r, l, want), (a, b)) in grid.into_iter().zip(rows) {
            eprintln!("RES {r} {l:+} dB: ref {a:.1?} fw {b:.1?}");
            assert!((a.0 - want).abs() < 0.06, "RES {r} {l}: ref {}", a.0);
            assert!(
                (b.0 - a.0).abs() < 0.25,
                "RES {r} {l}: fw {} ref {}",
                b.0,
                a.0
            );
            assert!(
                (b.1 - a.1).abs() < 2.5,
                "RES {r} {l}: H2 {} vs {}",
                b.1,
                a.1
            );
        }
    }

    /// H2 against drive, the followers' even order: LP (200 Hz at fc 1 kHz,
    /// RES 0) and BP (200 Hz at fc 200 Hz, RES 0.9), −12 … +18 dB. The fw
    /// within 2.5 dB of the reference everywhere (recorded: 1–2), its H3 at
    /// BP within 3.5 (recorded: 3); the reference's H2 rises with drive
    /// (LP −80 → −46 dB, BP −63 → −31 dB as recorded, to 0.6 dB).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn h2_against_drive() {
        let p = Parts::default();
        let lv = [
            (-12.0, -80.0, -63.0),
            (-6.0, -74.0, -58.0),
            (0.0, -67.0, -53.0),
            (6.0, -61.0, -47.0),
            (12.0, -55.0, -34.0),
            (18.0, -46.0, -31.0),
        ];
        let rows = par_map(&lv, |&(l, ..)| {
            let g = 10f64.powf(l / 20.0);
            let c = case(sine(200.0, 0.6), 1000.0, 0.0, g);
            let lp = (
                harm(tail(&render_ref(&p, &c, OS)), 200.0, g),
                harm(tail(&render(&p, &c, false)), 200.0, g),
            );
            let mut c = case(sine(200.0, 0.8), 200.0, 0.9, g);
            c.bp = true;
            let bp = (
                harm(tail(&render_ref(&p, &c, OS)), 200.0, g),
                harm(tail(&render(&p, &c, false)), 200.0, g),
            );
            (lp, bp)
        });
        for ((l, lpw, bpw), (lp, bp)) in lv.into_iter().zip(rows) {
            eprintln!("{l:+} dB: LP {lp:.1?}; BP {bp:.1?}");
            assert!((lp.0.1 - lpw).abs() < 0.6, "{l}: LP ref H2 {}", lp.0.1);
            assert!((bp.0.1 - bpw).abs() < 0.6, "{l}: BP ref H2 {}", bp.0.1);
            assert!((lp.1.1 - lp.0.1).abs() < 2.5, "{l}: LP fw H2");
            assert!((bp.1.1 - bp.0.1).abs() < 2.5, "{l}: BP fw H2");
            assert!((bp.1.2 - bp.0.2).abs() < 3.5, "{l}: BP fw H3");
        }
    }

    /// The RES ceiling: a 2 ms burst at fc, RES 1, time to fall 40 dB on
    /// 0.25 ms envelopes. The reference 382 / 286 / 54.8 / 8.0 / 3.5 ms
    /// and the fw 401.8 / 278.2 / 54.8 / 8.2 / 2.8 ms at 100 Hz … 10 kHz,
    /// as recorded (to 0.3 ms): every ring decays.
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn res_ceiling_rings_out() {
        let p = Parts::default();
        let fcs = [
            (100.0, 382.0, 401.8),
            (200.0, 286.0, 278.2),
            (1000.0, 54.8, 54.8),
            (4000.0, 8.0, 8.2),
            (10000.0, 3.5, 2.8),
        ];
        let rows = par_map(&fcs, |&(fc, ..)| {
            let c = case(burst(fc, 1.5), fc, 1.0, 1.0);
            (
                ring_ms(&render_ref(&p, &c, OS), 0.00025),
                ring_ms(&render(&p, &c, false), 0.00025),
            )
        });
        for ((fc, rw, fw), (r, f)) in fcs.into_iter().zip(rows) {
            eprintln!("{fc}: ref {r:?} fw {f:?}");
            assert!((r.expect("ref holds") - rw).abs() < 0.3, "{fc}: ref {r:?}");
            assert!((f.expect("fw holds") - fw).abs() < 0.3, "{fc}: fw {f:?}");
        }
    }

    /// Resonance under level (1 kHz at fc 1 kHz, RES 1): the diodes take
    /// 20 dB off the peak, +25.8 dB at −24 dB in to +5.7 at +12 (the
    /// reference as recorded, to 0.06 dB); the fw within 0.35 dB of it
    /// (recorded: 0.3).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn resonance_under_level() {
        let p = Parts::default();
        let lv = [
            (-24.0, 25.8),
            (-18.0, 22.8),
            (-12.0, 20.1),
            (-6.0, 17.8),
            (0.0, 15.4),
            (6.0, 11.4),
            (12.0, 5.7),
        ];
        let rows = par_map(&lv, |&(l, _)| {
            let g = 10f64.powf(l / 20.0);
            let c = case(sine(1000.0, 0.8), 1000.0, 1.0, g);
            let f = |y: &[f64]| db(tone(tail(y), 1000.0) / g);
            (f(&render_ref(&p, &c, OS)), f(&render(&p, &c, false)))
        });
        for ((l, want), (r, f)) in lv.into_iter().zip(rows) {
            eprintln!("{l:+} dB: ref {r:+.2} fw {f:+.2}");
            assert!((r - want).abs() < 0.06, "{l}: ref {r}");
            assert!((f - r).abs() < 0.35, "{l}: fw {f} ref {r}");
        }
    }

    /// The numerics (saw 110 Hz, fc 1.2 kHz, RES 0.9, +12 dB, 1 s): ×8
    /// against ×16 nulls under −85 dB (recorded −87.6), LSD under 0.005 dB
    /// (0.001); none of Newton's steps at ×8 unconverged. Iterations a step:
    /// the lab's 3.32 counts the 50 ms settle with the second (under 3.4
    /// here); over the driven second alone it is 3.84 (under 3.9).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn x8_is_converged() {
        let p = Parts::default();
        let c = case(stimuli::saw(110.0, 48_000, 0.0), 1200.0, 0.9, 4.0);
        let r = par_map(&[8, 16], |&os| render_ref(&p, &c, os));
        let (n, l) = (null_db(&r[0], &r[1]), lsd_matched(&r[0], &r[1]));
        eprintln!("x8 vs x16: null {n:.1} dB, LSD {l:.4} dB");
        assert!(n < -85.0 && l < 0.005);
        let mut r8 = Circuit::new(p, 384_000.0);
        (r8.i_abc, r8.res) = (r8.i_for(1200.0), 0.9);
        let (i0, s0) = (r8.iters, r8.steps);
        for v in &c.x {
            r8.step(4.0 * VCAL * v);
        }
        let it = (r8.iters - i0) as f64 / (r8.steps - s0) as f64;
        let all = r8.iters as f64 / r8.steps as f64;
        eprintln!("{it:.2} iterations a step driven, {all:.2} with the settle");
        assert!(it < 3.9, "{it}");
        assert!(all < 3.4, "{all}");
        assert_eq!(r8.fails, 0);
    }

    /// The 2N4302's spread (IDSS 1.5 / 3 / 5 mA, V_P −1 / −2 / −3.5 V): H2
    /// at BP (200 Hz at fc 200 Hz, RES 0.9, 0 dB) −56 / −53 / −50 dB, H3
    /// −45 throughout, as recorded (to 0.6 dB).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn follower_spread() {
        let sp = [
            (1.5e-3, -1.0, -56.0),
            (3e-3, -2.0, -53.0),
            (5e-3, -3.5, -50.0),
        ];
        let rows = par_map(&sp, |&(idss, vto, _)| {
            let mut q = Parts::default();
            (q.idss, q.vto) = (idss, vto);
            let mut c = case(sine(200.0, 0.8), 200.0, 0.9, 1.0);
            c.bp = true;
            harm(tail(&render_ref(&q, &c, OS)), 200.0, 1.0)
        });
        for ((idss, _, h2), h) in sp.into_iter().zip(rows) {
            eprintln!("IDSS {idss}: {h:.1?}");
            assert!((h.1 - h2).abs() < 0.6, "{idss}: H2 {}", h.1);
            assert!((h.2 + 45.0).abs() < 0.6, "{idss}: H3 {}", h.2);
        }
    }

    /// The circuit lab's 75 jobs (`ab::jobs`: seven stimuli, RES 0 / ½ /
    /// 0.9 / 1, LEVEL −12 / 0 / +12 dB). Recorded over the set, mean /
    /// worst: fw LSD 0.31 / 4.00 dB, null −32.4 / −14.2 dB; the exact
    /// variant the same; the old lab's null LS through the couplings −15.6
    /// / −1.7; CLEAN at the SEM's Q LSD 1.82 / 6.66, null LS through the
    /// couplings −15.0 / −0.2. Each held to 0.1 dB of the recorded (the means) and 0.2 (the
    /// worsts), so a better or worse figure both show.
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn ab_set_against_the_reference() {
        let p = Parts::default();
        let js = jobs();
        assert_eq!(js.len(), 75);
        let rows = par_map(&js, |(_, c)| {
            let r = render_ref(&p, c, OS);
            let f = render(&p, c, false);
            let fx = render(&p, c, true);
            let ol = old_sem(c);
            let (fi, fo) = couplings(&p);
            let olc = hp1(&hp1(&ol, fi), fo);
            let cl = clean(&p, c);
            let clc = hp1(&hp1(&cl, fi), fo);
            [
                lsd_matched(&f, &r),
                null_db(&f, &r),
                lsd_matched(&fx, &r),
                null_db(&fx, &r),
                null_ls(&olc, &r),
                lsd_matched(&cl, &r),
                null_ls(&clc, &r),
            ]
        });
        let agg = |k: usize| {
            let v: Vec<f64> = rows.iter().map(|r| r[k]).collect();
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            (mean, v.iter().cloned().fold(f64::MIN, f64::max))
        };
        for (k, what, mean, worst) in [
            (0, "fw LSD", 0.31, 4.00),
            (1, "fw null", -32.4, -14.2),
            (2, "fw exact LSD", 0.31, 4.00),
            (3, "fw exact null", -32.4, -14.2),
            (4, "old lab null LS", -15.6, -1.7),
            (5, "CLEAN at the SEM's Q LSD", 1.82, 6.66),
            (6, "CLEAN at the SEM's Q null LS", -15.0, -0.2),
        ] {
            let (m, w) = agg(k);
            eprintln!("{what}: mean {m:.2} worst {w:.2}");
            assert!((m - mean).abs() < 0.1, "{what}: mean {m}");
            assert!((w - worst).abs() < 0.2, "{what}: worst {w}");
        }
        clean_at_half(&js, &rows);
    }

    /// The headline of `notes.md`: at matched Q and RES ½, the SEM is a
    /// clean 2-pole SVF. CLEAN nulls the circuit at −37.7 / −37.6 dB mean
    /// at −12 / 0 dB (MODE sweep left out), held to 0.3 dB.
    fn clean_at_half(js: &[(String, Case)], rows: &[[f64; 7]]) {
        for (db, want) in [("-12dB", -37.7), ("+0dB", -37.6)] {
            let v: Vec<f64> = js
                .iter()
                .zip(rows)
                .filter(|((n, _), _)| {
                    n.contains("/r0.5_") && n.ends_with(db) && !n.starts_with("mode_sweep")
                })
                .map(|(_, r)| r[6])
                .collect();
            let m = v.iter().sum::<f64>() / v.len() as f64;
            eprintln!("CLEAN at RES 1/2, {db}: {m:.1} dB over {}", v.len());
            assert!((m - want).abs() < 0.3, "{db}: {m}");
        }
    }

    /// The SEM's small-signal k at (fc, RES): what CLEAN is matched to.
    fn sem_k(p: &Parts, c: &Circuit, fc: f64, r: f64) -> f64 {
        let kp = k_pot(p, r);
        let (a, b, l, d) = k_parasitic(c, fc, kp);
        kp + a + b - l + d
    }

    /// CLEAN (the real `Filter`) at the SEM's Q per block: its RES law
    /// inverted (k ≥ 2 is RES 0), LP2 / BP2, MODE by thirds LP2 / NOTCH1 /
    /// HP4, LEVEL as the drive; output with the drive left in.
    fn clean(p: &Parts, c: &Case) -> Vec<f64> {
        use chimera_core::dsp::filter::{Filter, FilterKind, FilterMode, FilterMods};
        use chimera_core::params::FilterParams;
        let rc = Circuit::unsettled(p.clone(), 384_000.0);
        let mut fp = FilterParams::default();
        fp.set_kind(FilterKind::Svf);
        fp.level = ((c.drive.log2() + 2.0) / 4.0) as f32;
        let mut f = Filter::new();
        let n = c.x.len();
        let mut y: Vec<f32> = c.x.iter().map(|&v| v as f32).collect();
        y.resize(n.next_multiple_of(BLOCK), 0.0);
        for (b, blk) in y.as_chunks_mut::<BLOCK>().0.iter_mut().enumerate() {
            let i = (b * BLOCK).min(n - 1);
            fp.set_mode(if c.bp {
                FilterMode::Bp12
            } else {
                match c.morph[i] {
                    m if m < 1.0 / 3.0 => FilterMode::Lp12,
                    m if m < 2.0 / 3.0 => FilterMode::Notch,
                    _ => FilterMode::Hp24,
                }
            });
            let k = sem_k(p, &rc, c.fc[i], c.res[i]);
            fp.cutoff = c.fc[i] as f32;
            fp.resonance = (0.95 * (1.0 - k / 2.0)).clamp(0.0, 1.0) as f32;
            f.process(blk, &fp, 48_000, &FilterMods::NONE);
        }
        y.truncate(n);
        y.into_iter().map(f64::from).collect()
    }

    /// The circuit lab's A/B jobs: every stimulus at RES 0 / ½ / 0.9 / 1 and
    /// each level, and the RES sweep at each level.
    fn jobs() -> Vec<(String, Case)> {
        let mut v = Vec::new();
        let st = stimuli::stimuli();
        let (sx, sfc, sres) = stimuli::res_sweep();
        for db in [-12.0f64, 0.0, 12.0] {
            let drive = 10f64.powf(db / 20.0);
            for s in &st {
                for r in [0.0, 0.5, 0.9, 1.0] {
                    let c = Case {
                        x: s.x.clone(),
                        fc: s.fc.clone(),
                        res: vec![r; s.x.len()],
                        morph: s.morph.clone(),
                        bp: s.bp,
                        drive,
                    };
                    v.push((format!("{}/r{r}_{db:+}dB", s.name), c));
                }
            }
            let mut c = case(sx.clone(), 0.0, 0.0, drive);
            (c.fc, c.res) = (sfc.clone(), sres.clone());
            v.push((format!("res_sweep/{db:+}dB"), c));
        }
        v
    }

    /// The old lab's candidate on a circuit case: CLEAN's RES law, 5 V a
    /// unit, its output with the drive left in.
    fn old_sem(c: &Case) -> Vec<f64> {
        use super::super::ota_svf;
        let oc = ota_svf::Case {
            x: c.x.clone(),
            fc: c.fc.clone(),
            res: c.res.clone(),
            morph: c.morph.clone(),
            bp: c.bp,
            drive: c.drive,
            floor: ota_svf::K_MIN,
        };
        ota_svf::Sem::new()
            .render(&oc)
            .iter()
            .map(|v| v * c.drive)
            .collect()
    }
}
