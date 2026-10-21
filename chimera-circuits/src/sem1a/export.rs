//! How the shipped SEM's constants and fixtures were made (`sem-circuit-lab`
//! 9582667d, the `semc-consts` and `semc-fixtures` commands): `consts()`
//! derives every constant `chimera-engine/src/dsp/filter/sem.rs` holds from
//! `Parts`, and `fixtures()` renders `fw::Fw` on the lab's stimuli with the
//! controls held a block (as the firmware reads them), no couplings (the
//! firmware uses the voice's DC blocker). The tests hold both equal to what
//! shipped: the constants to the kernel's, the fixture bytes to
//! `chimera-core/tests/fixtures/sem_fw_v1.bin`.

use super::BLOCK;
use super::fw::{self, Coefs, VCAL};
use super::reference::{Circuit, Parts};
use super::stimuli;
use super::validate::{k_parasitic, k_pot};
use std::f64::consts::PI;
use std::fmt;

/// The firmware's constants, from the parts.
#[derive(Clone, Debug)]
pub struct Consts {
    pub cf: Coefs,
    /// R139/R155.
    pub a_in: f64,
    /// The output's volts back to digital, `−1/(a_in·VCAL)`.
    pub norm: f64,
    /// The followers' small-signal gain at 0 V.
    pub gf: f64,
    /// The integrators' leak, `2/(gm·r_o·A_div·g_f)`.
    pub leak: f64,
    /// C18's lead per Hz, `2π·R138·C18`.
    pub c18_hz: f64,
    /// A12's lag per Hz, the noise gain over the 301A's GBW; and that gain.
    pub lag_hz: f64,
    pub ng: f64,
    /// C22 with R157: `2π·R157·C22` per Hz.
    pub wt_hz: f64,
    pub p: Parts,
}

pub fn consts() -> Consts {
    let p = Parts::default();
    let cf = Coefs::from(&p);
    let c = Circuit::unsettled(p.clone(), 384_000.0);
    let gf = c.follower_gain(0.0);
    let a_int = p.alpha * p.k_pair() * p.va_ota * p.a_div() * gf;
    let g_in = 1.0 / p.r155 + 1.0 / p.r139 + 1.0 / p.r138 + 1.0 / p.r157;
    let ng = 1.0 + p.r139 * g_in;
    Consts {
        cf,
        a_in: p.a_in(),
        norm: fw::norm(&p),
        gf,
        leak: 2.0 / a_int,
        c18_hz: 2.0 * PI * p.r138 * p.c18,
        lag_hz: ng / p.gbw,
        ng,
        wt_hz: 2.0 * PI * p.r157 * p.c22,
        p,
    }
}

/// The lab's `semc-consts` printout: the constants, then `k` and `g` at six
/// settings with `k`'s parts.
impl fmt::Display for Consts {
    fn fmt(&self, s: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (p, cf) = (&self.p, &self.cf);
        writeln!(s, "a_in {:.9e}", self.a_in)?;
        writeln!(s, "vcal {VCAL}")?;
        writeln!(s, "norm {:.9e}", self.norm)?;
        writeln!(s, "v0 {:.9e}", cf.v0)?;
        writeln!(
            s,
            "rail_hp {:.9e} lo {:.9e} hi {:.9e} rail_out {:.9e}",
            cf.rail_hp, cf.lo, cf.hi, cf.rail_out
        )?;
        writeln!(s, "r {:.9e} {:.9e} {:.9e}", cf.r[0], cf.r[1], cf.r[2])?;
        writeln!(
            s,
            "b0 {} w {} kd {} droop {:e}",
            cf.b0, cf.w, cf.kd, cf.droop
        )?;
        writeln!(s, "gf {:.9e} leak {:.9e}", self.gf, self.leak)?;
        writeln!(s, "c18/Hz {:.9e}", self.c18_hz)?;
        writeln!(s, "lag/Hz {:.9e} (ng {})", self.lag_hz, self.ng)?;
        writeln!(s, "wt/Hz {:.9e}", self.wt_hz)?;
        writeln!(
            s,
            "pot {} r_end {} r139 {} r157 {}",
            p.pot, p.r_end, p.r139, p.r157
        )?;
        let c = Circuit::unsettled(p.clone(), 384_000.0);
        for (fc, r) in [
            (1000.0, 0.0),
            (1000.0, 0.5),
            (1000.0, 0.9),
            (1000.0, 1.0),
            (100.0, 1.0),
            (10000.0, 1.0),
        ] {
            let kp = k_pot(p, r);
            let (a, b, l, d) = k_parasitic(&c, fc, kp);
            let (g, k) = fw::coefs(p, cf, &c, fc, r);
            writeln!(
                s,
                "fc {fc} res {r}: kp {kp:.6} leak {a:.6} c18 {b:.6} lag {l:.6} c22 {d:.6} -> g {g:.9} k {k:.9}"
            )?;
        }
        Ok(())
    }
}

/// One fixture: the input, the controls a block, the drive and the output.
pub struct Fixture {
    pub name: &'static str,
    pub x: Vec<f64>,
    /// Per block: fc, RES, MODE (the block's last sample's).
    pub fc: Vec<f64>,
    pub res: Vec<f64>,
    pub morph: Vec<f64>,
    pub bp: bool,
    pub drive: f64,
}

/// `fw::render`'s loop without the couplings, the controls a block's.
pub fn render(p: &Parts, c: &Fixture) -> Vec<f32> {
    let mut m = fw::Fw::new(p);
    let refc = Circuit::unsettled(p.clone(), 384_000.0);
    let cf = m.cf.clone();
    let blocks = c.x.len() / BLOCK;
    let gk = |b: usize| fw::coefs(p, &cf, &refc, c.fc[b], c.res[b]);
    let mut from = gk(0);
    let mut mf = c.morph[0];
    let gain = (VCAL * c.drive * p.a_in()) as f32;
    let nm = fw::norm(p);
    let mut y = Vec::new();
    for b in 0..blocks {
        let to = gk(b);
        let mt = c.morph[b];
        for j in 0..BLOCK {
            let i = b * BLOCK + j;
            let t = (j + 1) as f64 / BLOCK as f64;
            let g = (from.0 + (to.0 - from.0) * t) as f32;
            let k = (from.1 + (to.1 - from.1) * t) as f32;
            let mm = (mf + (mt - mf) * t) as f32;
            let d = 1.0 / (1.0 + k * g + g * g);
            let (h, bb, l) = m.tick(gain * c.x[i] as f32, g, k, d);
            let v = if c.bp { bb } else { l + mm * (h - l) };
            y.push((v as f64 * nm) as f32);
        }
        (from, mf) = (to, mt);
    }
    y
}

/// The six fixture cases: bass at RES 0.9, brass BP at RES 1 and +12 dB,
/// the MODE sweep, the pad at −12 dB, the cutoff sweep at RES 1 and +12 dB,
/// and the RES sweep across its top.
pub fn cases() -> Vec<Fixture> {
    let st = stimuli::stimuli();
    let pick = |name: &str| st.iter().find(|s| s.name == name).unwrap();
    // (stimulus, start sample, blocks, RES (−1: the sweep's own), drive dB)
    let cases: [(&'static str, usize, usize, f64, f64); 6] = [
        ("bass", 0, 32, 0.9, 0.0),
        ("brass_bp", 2400, 32, 1.0, 12.0),
        ("mode_sweep", 9600, 32, 0.5, 0.0),
        ("pad", 36000, 32, 0.0, -12.0),
        ("cutoff_sweep", 24000, 32, 1.0, 12.0),
        ("res_sweep", 4 * 48000 - 1024, 48, -1.0, 0.0),
    ];
    let mut out = Vec::new();
    for (name, start, blocks, res, db) in cases {
        let drive = 10f64.powf(db / 20.0);
        let n = blocks * BLOCK;
        let at = |v: &[f64], b: usize| v[start + b * BLOCK + BLOCK - 1];
        out.push(if name == "res_sweep" {
            let (x, fc, rs) = stimuli::res_sweep();
            Fixture {
                name,
                x: x[start..start + n].to_vec(),
                fc: (0..blocks).map(|b| at(&fc, b)).collect(),
                res: (0..blocks).map(|b| at(&rs, b)).collect(),
                morph: vec![0.0; blocks],
                bp: false,
                drive,
            }
        } else {
            let s = pick(name);
            Fixture {
                name,
                x: s.x[start..start + n].to_vec(),
                fc: (0..blocks).map(|b| at(&s.fc, b)).collect(),
                res: vec![res; blocks],
                morph: (0..blocks).map(|b| at(&s.morph, b)).collect(),
                bp: s.bp,
                drive,
            }
        });
    }
    out
}

/// The fixture file: little-endian, per case u32 blocks, f32 drive, u32
/// bp, then per block f32 fc, res, morph, then the input, then the output.
pub fn fixtures() -> Vec<u8> {
    let p = Parts::default();
    let mut out: Vec<u8> = Vec::new();
    let put = |o: &mut Vec<u8>, v: f32| o.extend_from_slice(&v.to_le_bytes());
    for fx in cases() {
        let blocks = fx.fc.len();
        let y = render(&p, &fx);
        out.extend_from_slice(&(blocks as u32).to_le_bytes());
        put(&mut out, fx.drive as f32);
        out.extend_from_slice(&(fx.bp as u32).to_le_bytes());
        for b in 0..blocks {
            put(&mut out, fx.fc[b] as f32);
            put(&mut out, fx.res[b] as f32);
            put(&mut out, fx.morph[b] as f32);
        }
        for &v in &fx.x {
            put(&mut out, v as f32);
        }
        for &v in &y {
            put(&mut out, v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! The shipped KIND SEM is the model derived here: its constants are
    //! `consts()`'s, its fixture file is `fixtures()`'s byte for byte, and
    //! the real `Filter` nulls this module's `fw` on it.
    use super::*;
    use chimera_core::dsp::filter::sem::{IN, OUT, k_pot as fw_k_pot, sem_g, sem_k};
    use chimera_core::dsp::filter::{Filter, FilterKind, FilterMode, FilterMods};
    use chimera_core::params::FilterParams;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-30)
    }

    /// `chimera-engine/src/dsp/filter/sem.rs`'s literals, each from
    /// `consts()` to f32's precision (2e-7 relative, 1e-6 for the
    /// follower cubic's least squares).
    #[test]
    fn consts_are_the_shipped_kernels() {
        let c = consts();
        let cf = &c.cf;
        let pairs: [(&str, f64, f64, f64); 13] = [
            ("V0", cf.v0 as f64, 25.035_631, 2e-7),
            ("LO", cf.lo as f64, -12.600_327, 2e-7),
            ("HI", cf.hi as f64, 13.775_356, 2e-7),
            ("R0", cf.r[0] as f64, 1.163_084_8e-3, 1e-6),
            ("R1", cf.r[1] as f64, -2.017_724_9e-4, 1e-6),
            ("R2", cf.r[2] as f64, 1.374_420_8e-5, 1e-6),
            ("LEAK", c.leak, 8.081_521_5e-3, 2e-7),
            ("C18_HZ", c.c18_hz, 6.283_185_3e-6, 2e-7),
            ("LAG_HZ", c.lag_hz, 1.080_808_1e-6, 2e-7),
            ("WT_HZ", c.wt_hz, 6.842_388_8e-2, 2e-7),
            ("DROOP", cf.droop, 1.9e-6, 0.0),
            ("IN", c.a_in * VCAL, IN as f64, 2e-7),
            ("OUT", c.norm, OUT as f64, 2e-7),
        ];
        for (n, got, want, tol) in pairs {
            assert!(rel(got, want) <= tol, "{n}: {got:e} vs {want:e}");
        }
        assert_eq!((cf.rail_hp, cf.rail_out), (14.0, 13.5));
        assert_eq!((cf.b0, cf.w, cf.kd), (0.10, 3.32, 0.154));
        assert_eq!(
            (c.p.pot, c.p.r_end, c.p.r157, c.p.r139),
            (50e3, 100.0, 33e3, 100e3)
        );
        let s = c.to_string();
        assert!(s.contains("v0 2.503563"), "{s}");
        assert!(s.contains("fc 1000 res 1: kp 0.006"), "{s}");
    }

    /// The per-block `g` and `k` (`fw::coefs`) are the kernel's `sem_g` and
    /// `sem_k`, and the pot laws agree, from 30 Hz to 15 kHz and RES 0 to
    /// 1: to 3e-5 relative (`k`, f32 against f64). `g` to 2e-5 up to 3 kHz;
    /// above, the engine's `tan_accurate` (stmlib's) reads low as its own
    /// doc says, 5.2e-4 at 10 kHz and 1.1 % at 15 kHz: bounds 7e-4 and
    /// 1.5 %. The lab's `fw` used `tan`; the fixtures never reach that high.
    #[test]
    fn per_block_coefficients_are_the_kernels() {
        let p = Parts::default();
        let cf = Coefs::from(&p);
        let c = Circuit::unsettled(p.clone(), 384_000.0);
        for fc in [30.0, 100.0, 440.0, 1000.0, 3000.0, 10000.0, 15000.0] {
            for r in [0.0, 0.25, 0.5, 0.9, 0.98, 1.0] {
                let (g, k) = fw::coefs(&p, &cf, &c, fc, r);
                let (sg, sk) = (sem_g(fc as f32, 48_000.0), sem_k(fc as f32, r as f32));
                assert!(rel(sk as f64, k) < 3e-5, "{fc} {r}: k {sk} vs {k}");
                let tol = match fc {
                    f if f <= 3000.0 => 2e-5,
                    f if f <= 10000.0 => 7e-4,
                    _ => 0.015,
                };
                assert!(rel(sg as f64, g) < tol, "{fc} {r}: g {sg} vs {g}");
                let kp = (fw_k_pot(r as f32) as f64, k_pot(&p, r));
                assert!(rel(kp.0, kp.1) < 3e-6, "{fc} {r}: k pot {kp:?}");
            }
        }
    }

    fn shipped_fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../chimera-core/tests/fixtures/sem_fw_v1.bin"
        );
        std::fs::read(path).expect("the shipped SEM fixture")
    }

    /// `semc-fixtures` regenerated: byte for byte the file the firmware's
    /// `the_firmware_is_the_labs_model` reads.
    #[test]
    fn fixtures_are_the_shipped_file() {
        let (ours, shipped) = (fixtures(), shipped_fixture());
        assert_eq!(ours.len(), shipped.len());
        let diff = ours.iter().zip(&shipped).filter(|(a, b)| a != b).count();
        assert_eq!(diff, 0, "{diff} bytes differ");
    }

    /// The real `Filter` at KIND SEM on each fixture case nulls this
    /// module's `fw` under −90 dB (the firmware test's bound; −103 to −138
    /// recorded in ADR 0192).
    #[test]
    fn the_shipped_filter_is_this_fw() {
        let p = Parts::default();
        for fx in cases() {
            let want = render(&p, &fx);
            let mut fp = FilterParams::default();
            fp.set_kind(FilterKind::Sem);
            assert!(fp.set_mode(if fx.bp {
                FilterMode::Bp12
            } else {
                FilterMode::Lp12
            }));
            fp.level = ((fx.drive.log2() + 2.0) / 4.0) as f32;
            let mut f = Filter::new();
            let mut y = Vec::new();
            for b in 0..fx.fc.len() {
                (fp.cutoff, fp.resonance, fp.morph) =
                    (fx.fc[b] as f32, fx.res[b] as f32, fx.morph[b] as f32);
                let mut buf: [f32; BLOCK] = std::array::from_fn(|i| fx.x[b * BLOCK + i] as f32);
                f.process(&mut buf, &fp, 48_000, &FilterMods::NONE);
                y.extend_from_slice(&buf);
            }
            let r: Vec<f64> = want.iter().map(|&v| v as f64).collect();
            let y: Vec<f64> = y.iter().map(|&v| v as f64).collect();
            let null = crate::util::measure::null_db(&y, &r);
            assert!(null < -90.0, "{}: null {null:.1} dB", fx.name);
        }
    }
}
