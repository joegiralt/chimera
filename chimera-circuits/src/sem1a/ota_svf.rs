//! The first SEM lab (#720, `sem-lab` def4f753): the Oberheim SEM's 12 dB
//! OTA state-variable as simplified equations. Superseded by the circuit
//! lab (`reference`, `fw`) but not rejected: kept as the cheaper model (80–90
//! cycles a sample on the lab's M7 probe, against the circuit model's
//! 120–130) and for its oscillating floor (`K_OSC`), which the circuit
//! cannot reach.
//!
//! The circuit (SEM-1A schematic, VCF sheet; `docs/sem1a/sem-lab-research.md`):
//! A12 sums 0.455 × the input, −LP and the damping into HP; two CA3080
//! integrators, each input through 100 K/220 Ω, so each integrates
//! `V0·tanh(v/V0)` with V0 = 455.5 × 2V_T = 23.6 V; RES is a pot from BP
//! into the summing node (`k`), with an anti-parallel 1N4148 pair from a
//! 47 K/10 K tap of BP in parallel: an antisaturator in the damping path
//! (Zavalishin 2.1.0 Fig. 6.51). MODE is a 50 K pot from HP to LP,
//! `(1 − m)·LP + m·HP`; BP is a switch.
//!
//! Two renderers of the same equations: `Model` (f64, ×4, trapezoidal,
//! Newton on both caps, exact tanh and Shockley diodes: this lab's
//! reference) and `Sem` (f32, ×1, the TPT SVF linearised at 0 with the OTA
//! bend pushed through, the diodes on the TPT state: its candidate). What
//! the circuit lab found it got wrong (CLEAN's RES law, no followers, an
//! invented floor, a circular reference, 5 V a unit) is in
//! `docs/sem1a/notes.md`. Provenance: the schematic, Zavalishin (TPT SVF
//! ch. 4, OTA 1-pole § 6.9, antisaturator § 6.11), our own code.

use super::BLOCK;
use crate::FS;
use crate::util::sig::resample_poly;
use std::f64::consts::PI;

// ------------------------------------------------------------ the circuit

/// Volts at one SEM input for digital 1.0 (a VCO saw at the mixer's top;
/// our assumption, the schematic prints no saw level).
pub const VS: f64 = 5.0;
/// R139/R155: each input's gain into the loop.
pub const A_IN: f64 = 100.0 / 220.0;
/// 2·V_T at 300 K.
const TWO_VT: f64 = 2.0 * 0.025_852;
/// The OTA's tanh scale at the HP/BP node: 100 K/220 Ω × 2V_T.
pub const V0: f64 = (100_000.0 + 220.0) / 220.0 * TWO_VT;
/// A12's and the OTAs' swing on ±15 V.
pub const RAIL: f64 = 13.5;
/// R159 47 K over R158 10 K: the diodes' drive, × BP.
const DIV: f64 = 10.0 / 57.0;
/// The tap's 8.25 K plus R156 100 K.
const R_D: f64 = 47.0 * 10.0 / 57.0 * 1e3 + 100e3;
/// R139, the summer's feedback: the diodes' current in volts at HP.
const R_F: f64 = 100e3;
/// 1N4148 (the common SPICE card): I_s, n.
const IS: f64 = 2.52e-9;
const N_VT: f64 = 1.752 * 0.025_852;
/// The diodes' large-signal slope, `R_F·DIV/R_D`.
pub const KD: f64 = R_F * DIV / R_D;
/// The damping's floor: the pot's end, the OTAs' and JFETs' losses. Q 25:
/// it rings, it never holds a tone.
pub const K_MIN: f64 = 0.04;
/// The lab's "let it oscillate" floor: anti-damping held by the diodes.
pub const K_OSC: f64 = -0.06;

/// The diodes' damping at HP for `bp` volts, exact, and its slope.
pub fn diode(bp: f64) -> (f64, f64) {
    let vin = DIV * bp;
    let s = vin.signum();
    let a = vin.abs();
    // v + R·2I_s·sinh(v/nV_T) = a: convex, rising; Newton from the right.
    let mut v = a.min(1.0);
    for _ in 0..200 {
        let e = (v / N_VT).exp();
        let sh = 0.5 * (e - 1.0 / e);
        let ch = 0.5 * (e + 1.0 / e);
        let f = v + R_D * 2.0 * IS * sh - a;
        let df = 1.0 + R_D * 2.0 * IS * ch / N_VT;
        let dv = f / df;
        v -= dv;
        if dv.abs() < 1e-13 {
            break;
        }
    }
    let i = (a - v) / R_D;
    let gd = 2.0 * IS / N_VT * (v / N_VT).cosh();
    (s * R_F * i, R_F * DIV / (R_D + 1.0 / gd))
}

/// The diodes' cheap form: `kd·φ(|bp| − B0)`, `φ` 0 below 0, `e²/2W` to
/// `W`, then `e − W/2`: C1, no divide.
#[derive(Clone, Copy, Debug)]
pub struct DiodeFit {
    pub b0: f32,
    pub w: f32,
    pub kd: f32,
}

impl DiodeFit {
    /// Fitted by `fit_diode` (least worst error on 0–15 V), held here so
    /// the probe and the lab agree.
    pub const SEM: DiodeFit = DiodeFit {
        b0: 0.10,
        w: 3.32,
        kd: 0.1540,
    };

    #[inline(always)]
    pub fn eval(&self, bp: f32) -> f32 {
        let e = bp.abs() - self.b0;
        let y = if e <= 0.0 {
            0.0
        } else if e < self.w {
            e * e * (0.5 / self.w)
        } else {
            e - 0.5 * self.w
        };
        (self.kd * y).copysign(bp)
    }
}

/// The least-worst `B0`, `W` against `diode` on 0–13.5 V (the rails), and its error.
pub fn fit_diode() -> (DiodeFit, f64) {
    let pts: Vec<(f32, f64)> = (0..=1350)
        .map(|i| {
            let b = i as f64 * 0.01;
            (b as f32, diode(b).0)
        })
        .collect();
    let err = |f: &DiodeFit| {
        pts.iter()
            .map(|&(b, d)| (f.eval(b) as f64 - d).abs())
            .fold(0.0, f64::max)
    };
    let mut best = (DiodeFit::SEM, f64::MAX);
    for kq in 0..=40 {
        let kd = (KD * (0.85 + 0.005 * kq as f64)) as f32;
        for i in 0..200 {
            for j in 1..200 {
                let f = DiodeFit {
                    b0: i as f32 * 0.02,
                    w: j as f32 * 0.02,
                    kd,
                };
                let e = err(&f);
                if e < best.1 {
                    best = (f, e);
                }
            }
        }
    }
    best
}

/// CLEAN's RES law, `2·(1 − RES/0.95)` in f32, as the engine's `clean_k`
/// (a test holds the two equal): the law this lab used on every TYPE.
pub fn clean_k(res: f32) -> f32 {
    2.0 * (1.0 - res * (1.0 / 0.95))
}

/// RES's `k` on SEM: CLEAN's law (the same knob on every TYPE) down to
/// the SEM's floor.
pub fn sem_k(res: f64, floor: f64) -> f64 {
    (clean_k(res as f32) as f64).max(floor)
}

// --------------------------------------------------------------- the case

/// A render's input and controls, per 48 kHz sample.
#[derive(Clone)]
pub struct Case {
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub res: Vec<f64>,
    /// MODE, 0 LP … 1 HP.
    pub morph: Vec<f64>,
    pub bp: bool,
    /// The input's gain (LEVEL ¼, 1, 4), applied by each renderer and
    /// undone at its output; `x` is the undriven input.
    pub drive: f64,
    /// SEM's `k` floor.
    pub floor: f64,
}

// ------------------------------------------------------------- reference

/// The equations solved well: ×`os`, trapezoidal, Newton on (bp, lp).
pub struct Model {
    pub os: usize,
    bp: f64,
    lp: f64,
    /// Last tick's integrator inputs, `V0·tanh(·/V0)` of HP and BP.
    t_hp: f64,
    t_bp: f64,
    pub iters: u64,
}

fn rail(v: f64) -> (f64, f64) {
    let r = (v / RAIL).powi(8);
    let q = (1.0 + r).powf(-1.0 / 8.0);
    (v * q, q / (1.0 + r))
}

impl Model {
    pub fn new(os: usize) -> Self {
        Self {
            os,
            bp: 0.0,
            lp: 0.0,
            t_hp: 0.0,
            t_bp: 0.0,
            iters: 0,
        }
    }

    fn hp_of(u: f64, bp: f64, lp: f64, k: f64) -> (f64, f64, f64) {
        let (d, dd) = diode(bp);
        let (h, dh) = rail(u - lp - k * bp - d);
        (h, -dh * (k + dd), -dh)
    }

    /// One tick at `u` volts into the loop: `(hp, bp, lp)`.
    pub fn tick(&mut self, u: f64, g: f64, k: f64) -> (f64, f64, f64) {
        let (mut b, mut l) = (self.bp, self.lp);
        for _ in 0..50 {
            self.iters += 1;
            let (h, hb, hl) = Self::hp_of(u, b, l, k);
            let (th, tb) = ((h / V0).tanh(), (b / V0).tanh());
            let (sh, sb) = (1.0 - th * th, 1.0 - tb * tb);
            let f1 = b - self.bp - g * (V0 * th + self.t_hp);
            let f2 = l - self.lp - g * (V0 * tb + self.t_bp);
            let (j11, j12) = (1.0 - g * sh * hb, -g * sh * hl);
            let (j21, j22) = (-g * sb, 1.0);
            let det = j11 * j22 - j12 * j21;
            let db = (f1 * j22 - j12 * f2) / det;
            let dl = (j11 * f2 - j21 * f1) / det;
            b -= db;
            l -= dl;
            if db.abs() + dl.abs() < 1e-11 {
                break;
            }
        }
        let (h, _, _) = Self::hp_of(u, b, l, k);
        self.t_hp = V0 * (h / V0).tanh();
        self.t_bp = V0 * (b / V0).tanh();
        (self.bp, self.lp) = (b, l);
        (h, b, l)
    }

    pub fn render(&mut self, c: &Case) -> Vec<f64> {
        let os = self.os;
        let fs = FS * os as f64;
        let x = resample_poly(&c.x, os, 1);
        let mut y = Vec::with_capacity(x.len());
        for (j, &xv) in x.iter().enumerate() {
            let i = (j / os).min(c.x.len() - 1);
            let g = (PI * c.fc[i].min(0.45 * fs) / fs).tan();
            let k = sem_k(c.res[i], c.floor);
            let (h, b, l) = self.tick(A_IN * VS * c.drive * xv, g, k);
            let m = c.morph[i];
            let v = if c.bp { b } else { l + m * (h - l) };
            y.push(v / (A_IN * VS * c.drive));
        }
        let mut y = resample_poly(&y, 1, os);
        y.resize(c.x.len(), 0.0);
        y
    }
}

// ------------------------------------------------------------- candidate

/// Where the diodes read BP from inside the linear solve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Predict {
    /// The TPT state `s1`: the zero-input next BP (a free extrapolation).
    State,
    /// Last sample's BP.
    Last,
}

/// The cheap SEM, f32 at 48 kHz: the linear TPT solve with the diodes
/// read from `s1`, then each integrator's input through the OTA's bend.
#[derive(Clone, Debug)]
pub struct Sem {
    s1: f32,
    s2: f32,
    bp_z: f32,
    pub diode: DiodeFit,
    pub predict: Predict,
}

/// `V0·tanh(v/V0)` on the rails' ±13.5 V (|v/V0| ≤ 0.57): tanh's series
/// to the fifth power, 0.1 % at the rail.
#[inline(always)]
pub fn ota(v: f32) -> f32 {
    const A3: f32 = (1.0 / (3.0 * V0 * V0)) as f32;
    const A5: f32 = (2.0 / (15.0 * V0 * V0 * V0 * V0)) as f32;
    let v = v.clamp(-RAIL as f32, RAIL as f32);
    let v2 = v * v;
    v - v * v2 * (A3 - A5 * v2)
}

impl Default for Sem {
    fn default() -> Self {
        Self::new()
    }
}

impl Sem {
    pub fn new() -> Self {
        Self {
            s1: 0.0,
            s2: 0.0,
            bp_z: 0.0,
            diode: DiodeFit::SEM,
            predict: Predict::State,
        }
    }

    /// One sample at `u` volts, `d = 1/(1 + kg + g²)` given: (hp, bp, lp).
    #[inline(always)]
    pub fn tick(&mut self, u: f32, g: f32, k: f32, d: f32) -> (f32, f32, f32) {
        let pre = match self.predict {
            Predict::State => self.s1,
            Predict::Last => self.bp_z,
        };
        let dd = self.diode.eval(pre);
        let r = RAIL as f32;
        let hp = ((u - dd - (k + g) * self.s1 - self.s2) * d).clamp(-r, r);
        let v1 = g * ota(hp);
        let bp = self.s1 + v1;
        self.s1 = bp + v1;
        let v2 = g * ota(bp);
        let lp = self.s2 + v2;
        self.s2 = lp + v2;
        self.bp_z = bp;
        (hp, bp, lp)
    }

    /// The case at 48 kHz, its controls read once a block and ramped across
    /// it, as `Filter` reads CLEAN's.
    pub fn render(&mut self, c: &Case) -> Vec<f64> {
        let n = c.x.len();
        let at = |v: &[f64], i: usize| v[i.min(n - 1)];
        let mut y = Vec::with_capacity(n);
        let gk = |i: usize| {
            let g = (PI * at(&c.fc, i).min(0.45 * FS) / FS).tan();
            (g, sem_k(at(&c.res, i), c.floor))
        };
        let mut from = gk(0);
        let gain = (A_IN * VS * c.drive) as f32;
        for b0 in (0..n).step_by(BLOCK) {
            let to = gk(b0 + BLOCK - 1);
            for i in b0..(b0 + BLOCK).min(n) {
                let t = (i - b0 + 1) as f64 / BLOCK as f64;
                let g = (from.0 + (to.0 - from.0) * t) as f32;
                let k = (from.1 + (to.1 - from.1) * t) as f32;
                let d = 1.0 / (1.0 + k * g + g * g);
                let (h, b, l) = self.tick(gain * c.x[i] as f32, g, k, d);
                let m = c.morph[i] as f32;
                let v = if c.bp { b } else { l + m * (h - l) };
                y.push((v / gain) as f64);
            }
            from = to;
        }
        y
    }
}

// ------------------------------------------------------------ the A/B set

/// The lab's RES settings; RES 1 is where the floor shows.
pub const RES: [f64; 4] = [0.0, 0.5, 0.85, 1.0];
/// LEVEL's −12 / 0 / +12 dB of drive.
pub const LEVELS_DB: [f64; 3] = [-12.0, 0.0, 12.0];

/// The lab's 75 A/B jobs: every stimulus at every RES and level, and the
/// RES sweep at each level, on the `K_MIN` floor.
pub fn jobs() -> Vec<(String, Case)> {
    let mut v = Vec::new();
    let st = super::stimuli::stimuli();
    let (sx, sfc, sres) = super::stimuli::res_sweep();
    for &db in &LEVELS_DB {
        let drive = 10f64.powf(db / 20.0);
        let tag = |r: f64| format!("r{:02}_{:+03}dB", (r * 100.0).round() as i32, db as i32);
        for s in &st {
            for &r in &RES {
                let case = Case {
                    x: s.x.clone(),
                    fc: s.fc.clone(),
                    res: vec![r; s.x.len()],
                    morph: s.morph.clone(),
                    bp: s.bp,
                    drive,
                    floor: K_MIN,
                };
                v.push((format!("{}/{}", s.name, tag(r)), case));
            }
        }
        let case = Case {
            morph: vec![0.0; sx.len()],
            x: sx.clone(),
            fc: sfc.clone(),
            res: sres.clone(),
            bp: false,
            drive,
            floor: K_MIN,
        };
        v.push((format!("res_sweep/{}", tag(1.0)), case));
    }
    v
}

#[cfg(test)]
mod tests {
    //! The first lab's probes (`docs/sem1a/sem-lab-probes.md`) and A/B
    //! (`docs/sem1a/sem-lab-ab-table.md`) as assertions.
    use super::super::stimuli::{burst, ring_ms, sine};
    use super::super::{lsd_matched, par_map, tail};
    use super::*;
    use crate::util::measure::{db, null_db, tone};

    fn case(x: Vec<f64>, fc: f64, res: f64, drive: f64, morph: f64) -> Case {
        let n = x.len();
        Case {
            x,
            fc: vec![fc; n],
            res: vec![res; n],
            morph: vec![morph; n],
            bp: false,
            drive,
            floor: K_MIN,
        }
    }

    /// CLEAN's law here is the engine's, bit for bit.
    #[test]
    fn clean_k_is_the_engines() {
        for i in 0..=100 {
            let r = i as f32 / 100.0;
            assert_eq!(clean_k(r), chimera_core::dsp::filter::clean::clean_k(r));
        }
    }

    /// The diodes, exact and fitted: D(1 … 10 V) = 0.010 / 0.082 / 0.206 /
    /// 0.346 / 0.644 / 1.264 V and the held fit 0.019 / 0.084 / 0.195 /
    /// 0.345 / 0.653 / 1.269 V (to 6e-4), its worst error on 0–13.5 V
    /// under 0.012 V (recorded 0.011).
    #[test]
    fn diode_fit_holds() {
        for (b, d, f) in [
            (1.0, 0.010, 0.019),
            (2.0, 0.082, 0.084),
            (3.0, 0.206, 0.195),
            (4.0, 0.346, 0.345),
            (6.0, 0.644, 0.653),
            (10.0, 1.264, 1.269),
        ] {
            assert!((diode(b).0 - d).abs() < 6e-4, "D({b}) {}", diode(b).0);
            let e = DiodeFit::SEM.eval(b as f32) as f64;
            assert!((e - f).abs() < 6e-4, "fit({b}) {e}");
        }
        let worst = (0..=1350)
            .map(|i| {
                let b = i as f64 * 0.01;
                (DiodeFit::SEM.eval(b as f32) as f64 - diode(b).0).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 0.012, "{worst}");
    }

    /// `fit_diode`'s grid search finds the held fit: B0 0.10 V, W 3.32 V,
    /// KD 0.1540, worst error 0.011 V.
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn fit_diode_finds_the_held_fit() {
        let (f, e) = fit_diode();
        assert!(
            (f.b0 - 0.10).abs() < 1e-6 && (f.w - 3.32).abs() < 1e-6,
            "{f:?}"
        );
        assert!((f.kd - 0.154).abs() < 5e-5, "{f:?}");
        assert!((e - 0.011).abs() < 6e-4, "{e}");
    }

    /// The ring at RES 1 (a 2 ms burst at fc, 5 ms envelopes): SEM falls
    /// 40 dB in 175 / 35 / 10 ms at 200 Hz / 1 kHz / 4 kHz, as recorded;
    /// on the oscillating floor it never does.
    #[test]
    fn ring_out() {
        for (fc, ms) in [(200.0, 175.0), (1000.0, 35.0), (4000.0, 10.0)] {
            let mut c = case(burst(fc, 1.0), fc, 1.0, 1.0, 0.0);
            let r = ring_ms(&Sem::new().render(&c), 0.005);
            assert!((r.expect("holds") - ms).abs() < 0.5, "{fc}: {r:?}");
            c.floor = K_OSC;
            assert_eq!(ring_ms(&Sem::new().render(&c), 0.005), None, "{fc}");
        }
    }

    /// Gain at the cutoff (1 kHz at fc 1 kHz, LP), RES × level, and THD
    /// (H2–H9): the candidate's column as recorded (to 0.06 dB, 0.006 %).
    const GAIN: [(f64, f64, f64, f64, f64); 15] = [
        // RES, level dB, SEM dB, ref dB, SEM THD %
        (0.0, -12.0, -6.0, -6.0, 0.01),
        (0.0, 0.0, -6.1, -6.1, 0.05),
        (0.0, 12.0, -6.4, -6.4, 0.20),
        (0.5, -12.0, 0.4, 0.4, 0.03),
        (0.5, 0.0, 0.1, 0.1, 0.11),
        (0.5, 12.0, -0.8, -0.8, 0.32),
        (0.85, -12.0, 12.0, 12.0, 0.11),
        (0.85, 0.0, 9.7, 9.7, 0.23),
        (0.85, 12.0, 3.0, 3.2, 0.54),
        (0.95, -12.0, 18.0, 18.0, 0.20),
        (0.95, 0.0, 13.5, 13.2, 0.42),
        (0.95, 12.0, 3.0, 3.2, 0.57),
        (1.0, -12.0, 18.0, 18.0, 0.20),
        (1.0, 0.0, 13.5, 13.2, 0.42),
        (1.0, 12.0, 3.0, 3.2, 0.57),
    ];

    fn thd(y: &[f64]) -> f64 {
        let h1 = tone(y, 1000.0);
        let hs: f64 = (2..=9).map(|h| tone(y, 1000.0 * h as f64).powi(2)).sum();
        100.0 * hs.sqrt() / h1.max(1e-12)
    }

    #[test]
    fn gain_at_cutoff() {
        for (r, l, sem, _, th) in GAIN {
            let c = case(sine(1000.0, 1.0), 1000.0, r, 10f64.powf(l / 20.0), 0.0);
            let y = Sem::new().render(&c);
            let g = db(tone(tail(&y), 1000.0));
            assert!((g - sem).abs() < 0.06, "RES {r} {l}: {g}");
            assert!((thd(tail(&y)) - th).abs() < 0.006, "RES {r} {l}: THD");
        }
    }

    /// The same at the lab's reference (`Model` ×4), to 0.06 dB: within
    /// 0.35 dB of the candidate everywhere.
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn gain_at_cutoff_reference() {
        let got = par_map(&GAIN, |&(r, l, ..)| {
            let c = case(sine(1000.0, 1.0), 1000.0, r, 10f64.powf(l / 20.0), 0.0);
            db(tone(tail(&Model::new(4).render(&c)), 1000.0))
        });
        for ((r, l, sem, rf, _), g) in GAIN.into_iter().zip(got) {
            assert!((g - rf).abs() < 0.06, "RES {r} {l}: {g}");
            assert!((g - sem).abs() < 0.35, "RES {r} {l}: {g}");
        }
    }

    /// MODE's notch (fc 1 kHz, RES 0, 0 dB) on a 1/24-octave sweep: 2934 /
    /// 1745 / 1008 / 582 / 336 Hz at −54.8 / −47.7 / −47.5 / −50.7 / −56.9
    /// dB, as recorded (to 1 Hz, 0.1 dB), within 2.5 % of fc·√((1−m)/m).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn mode_notch() {
        let ms = [
            (0.1, 2934.0, -54.8),
            (0.25, 1745.0, -47.7),
            (0.5, 1008.0, -47.5),
            (0.75, 582.0, -50.7),
            (0.9, 336.0, -56.9),
        ];
        let got = par_map(&ms, |&(m, ..)| {
            let mut best = (0.0, f64::MAX);
            let mut f = 100.0;
            while f <= 10_000.0 {
                let c = case(sine(f, 1.0), 1000.0, 0.0, 1.0, m);
                let g = db(tone(tail(&Sem::new().render(&c)), f));
                if g < best.1 {
                    best = (f, g);
                }
                f *= 2f64.powf(1.0 / 24.0);
            }
            best
        });
        for ((m, fw, dw), (f, d)) in ms.into_iter().zip(got) {
            assert!((f - fw).abs() < 1.0, "m {m}: {f}");
            assert!((d - dw).abs() < 0.1, "m {m}: {d}");
            let pred = 1000.0 * ((1.0 - m) / m).sqrt();
            assert!((f / pred - 1.0).abs() < 0.025, "m {m}");
        }
    }

    /// The 75 jobs, candidate against the lab's reference: LSD mean /
    /// worst 0.17 / 0.54 dB, null −39.3 / −7.8 dB (recorded; held to 0.05
    /// and 0.2 dB). The reference is the same equations at ×4, so this
    /// checks only the discretisation (the circuit lab's point 4).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn ab_set_against_its_reference() {
        let js = jobs();
        assert_eq!(js.len(), 75);
        let rows = par_map(&js, |(_, c)| {
            let b = Sem::new().render(c);
            let r = Model::new(4).render(c);
            (lsd_matched(&b, &r), null_db(&b, &r))
        });
        let n = rows.len() as f64;
        let ml = rows.iter().map(|r| r.0).sum::<f64>() / n;
        let wl = rows.iter().map(|r| r.0).fold(0.0, f64::max);
        let mn = rows.iter().map(|r| r.1).sum::<f64>() / n;
        let wn = rows.iter().map(|r| r.1).fold(f64::MIN, f64::max);
        eprintln!("LSD {ml:.3} / {wl:.2}, null {mn:.2} / {wn:.1}");
        assert!((ml - 0.169).abs() < 0.05 && (wl - 0.54).abs() < 0.2);
        assert!((mn + 39.35).abs() < 0.1 && (wn + 7.8).abs() < 0.2);
    }
}
