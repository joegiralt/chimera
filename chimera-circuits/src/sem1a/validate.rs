//! The reference circuit's checks (`sem-circuit-lab` `validate.rs`): the
//! schematic's own algebra for `k` (the pot law and the parasitics, which
//! `fw` reads per block), the small-signal responses measured off the
//! circuit, and the antisaturator on its own. The lab's `run()` report is
//! now the tests below; its numbers are in `docs/sem1a/validation.md`.

use super::reference::{Circuit, Parts, diodes};
use rustfft::num_complex::Complex64;
use std::f64::consts::PI;

/// The reference's rate: ×8 of 48 kHz.
pub const OS: usize = 8;
pub const FS_REF: f64 = 48_000.0 * OS as f64;

/// The SEM's small-signal damping for the knob `r`, from the parts: the
/// pot's Thevenin into R157 at a virtual ground, × R139.
pub fn k_pot(p: &Parts, r: f64) -> f64 {
    let rt = p.r_end + p.pot * r;
    let rb = p.r_end + p.pot * (1.0 - r);
    let div = rb / (rt + rb);
    let rth = rt * rb / (rt + rb);
    p.r139 * div / (p.r157 + rth)
}

/// Extra damping the parts add at `fc`: each integrator's leak (finite
/// gm·r_o), C18's lead on the LP feedback, A12's lag, C22's coupling.
pub fn k_parasitic(c: &Circuit, fc: f64, k: f64) -> (f64, f64, f64, f64) {
    let p = &c.p;
    let w = 2.0 * PI * fc;
    let gf = c.follower_gain(0.0);
    // Integrator DC gain: gm·r_o·A_div·g_f with gm·r_o = α·k_pair·V_A.
    let a_int = p.alpha * p.k_pair() * p.va_ota * p.a_div() * gf;
    let leak = 2.0 / a_int;
    let c18 = w * p.r138 * p.c18;
    // A12's lag at fc: noise gain over GBW.
    let g_in = 1.0 / p.r155 + 1.0 / p.r139 + 1.0 / p.r138 + 1.0 / p.r157;
    let ng = 1.0 + p.r139 * g_in;
    let lag = ng * fc / p.gbw;
    // C22 in series with R157: Re of the high-pass at fc, times k.
    let wt = w * p.r157 * p.c22;
    let c22 = k * (wt * wt / (1.0 + wt * wt) - 1.0);
    (leak, c18, lag, c22)
}

pub struct Resp {
    pub df: f64,
    pub lp: Vec<Complex64>,
    pub bp: Vec<Complex64>,
    pub hp: Vec<Complex64>,
}

/// The linear responses at (fc, RES), normalised by the input gain so the
/// LP passband reads 1: a 1 mV one-step impulse at ×8, `n` = 2^19 steps.
pub fn small_signal(p: &Parts, fc: f64, res: f64) -> Resp {
    let mut c = Circuit::new(p.clone(), FS_REF);
    c.res = res;
    c.i_abc = c.i_for(fc);
    for _ in 0..(0.3 * FS_REF) as usize {
        c.step(0.0);
    }
    let n = 1 << 19;
    let amp = 1e-3;
    let a = p.a_in();
    let (mut lp, mut bp, mut hp) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    let mut c0 = c.clone();
    for k in 0..n {
        let base = c0.step(0.0);
        let o = c.step(if k == 0 { amp } else { 0.0 });
        lp[k] = (o.lp - base.lp) / (amp * a);
        bp[k] = (o.bp - base.bp) / (amp * a);
        hp[k] = (o.hp - base.hp) / (amp * a);
    }
    let f = |v: &[f64]| crate::util::sig::fft(v)[..n / 2].to_vec();
    Resp {
        df: FS_REF / n as f64,
        lp: f(&lp),
        bp: f(&bp),
        hp: f(&hp),
    }
}

/// The |BP| peak near `fc`: (frequency, |BP| there), parabolic on dB.
pub fn bp_peak(r: &Resp, fc: f64) -> (f64, f64) {
    let lo = ((fc * 0.5) / r.df) as usize;
    let hi = ((fc * 2.0) / r.df) as usize;
    let mut best = lo.max(1);
    for i in lo.max(1)..hi.min(r.bp.len() - 1) {
        if r.bp[i].norm() > r.bp[best].norm() {
            best = i;
        }
    }
    let y = |i: usize| 20.0 * r.bp[i].norm().log10();
    let (a, b, cc) = (y(best - 1), y(best), y(best + 1));
    let d = 0.5 * (a - cc) / (a - 2.0 * b + cc);
    let pk = b - 0.25 * (a - cc) * d;
    ((best as f64 + d) * r.df, 10f64.powf(pk / 20.0))
}

/// |H| at `f` by linear interpolation on the bins.
pub fn at(v: &[Complex64], df: f64, f: f64) -> f64 {
    let x = f / df;
    let i = x as usize;
    let t = x - i as f64;
    v[i].norm() * (1.0 - t) + v[i + 1].norm() * t
}

/// D11/D12's current (A) with BP at `bp` volts: A14 into the R159/R158
/// tap (10/57 × BP behind 8.25 K), the pair and R156 into a virtual
/// ground, bisected. × R139 it is the damping at HP in volts.
pub fn antisaturator(p: &Parts, bp: f64) -> f64 {
    let vth = bp * p.r158 / (p.r158 + p.r159);
    let rth = p.r158 * p.r159 / (p.r158 + p.r159) + p.r156 + p.rs_d;
    let (mut lo, mut hi) = (0.0, vth);
    for _ in 0..200 {
        let v = 0.5 * (lo + hi);
        if diodes(p, v).0 > (vth - v) / rth {
            hi = v
        } else {
            lo = v
        }
    }
    (vth - lo) / rth
}

#[cfg(test)]
mod tests {
    //! `docs/sem1a/validation.md` §§ 4–6 as assertions.
    use super::*;

    fn db(v: f64) -> f64 {
        20.0 * v.max(1e-15).log10()
    }

    /// The lab's § 5 grid: (fc, RES, f0 measured, k pot, k predicted, k
    /// measured, LP at fc/10 dB, HP at 10·fc dB).
    type Row = (f64, f64, f64, f64, f64, f64, f64, f64);
    const GRID: [Row; 20] = [
        (100.0, 0.0, 53.3, 3.0151, 2.9607, 2.8901, 0.42, -0.33),
        (100.0, 0.5, 92.8, 1.0977, 1.0833, 1.0961, -0.36, 0.02),
        (100.0, 0.9, 97.4, 0.2703, 0.2733, 0.2767, -0.71, 0.08),
        (100.0, 0.98, 98.7, 0.0643, 0.0716, 0.0737, -0.80, 0.08),
        (100.0, 1.0, 99.1, 0.0060, 0.0145, 0.0169, -0.81, 0.09),
        (1000.0, 0.0, 974.7, 3.0151, 3.0278, 3.0301, -0.05, -0.27),
        (1000.0, 0.5, 986.9, 1.0977, 1.1107, 1.1131, 0.09, 0.04),
        (1000.0, 0.9, 989.1, 0.2703, 0.2836, 0.2857, 0.10, 0.08),
        (1000.0, 0.98, 989.9, 0.0643, 0.0776, 0.0797, 0.08, 0.09),
        (1000.0, 1.0, 990.1, 0.0060, 0.0193, 0.0214, 0.08, 0.09),
        (5000.0, 0.0, 4954.3, 3.0151, 3.0492, 3.0515, -0.35, -0.14),
        (5000.0, 0.5, 4927.7, 1.0977, 1.1318, 1.1347, 0.01, 0.08),
        (5000.0, 0.9, 4915.9, 0.2703, 0.3044, 0.3070, 0.08, 0.10),
        (5000.0, 0.98, 4913.1, 0.0643, 0.0984, 0.1007, 0.09, 0.09),
        (5000.0, 1.0, 4912.3, 0.0060, 0.0401, 0.0424, 0.09, 0.09),
        (12000.0, 0.0, 11858.6, 3.0151, 3.0857, 3.0873, -0.41, 0.34),
        (12000.0, 0.5, 11692.2, 1.0977, 1.1682, 1.1716, -0.00, 0.32),
        (12000.0, 0.9, 11634.1, 0.2703, 0.3408, 0.3434, 0.08, 0.26),
        (12000.0, 0.98, 11618.9, 0.0643, 0.1348, 0.1369, 0.09, 0.23),
        (12000.0, 1.0, 11614.5, 0.0060, 0.0765, 0.0784, 0.09, 0.22),
    ];

    /// § 5's algebra: the pot law (3.02 at RES 0, Q 0.33; 0.006 at RES 1)
    /// and the parasitics' predicted `k`, the lab's table to its digits.
    #[test]
    fn k_from_the_parts() {
        let p = Parts::default();
        let c = Circuit::unsettled(p.clone(), FS_REF);
        for (fc, r, _, kp, pred, ..) in GRID {
            let k = k_pot(&p, r);
            assert!((k - kp).abs() < 6e-5, "{fc} {r}: k pot {k}");
            let (lk, c18, lag, c22) = k_parasitic(&c, fc, k);
            let got = k + lk + c18 - lag + c22;
            assert!((got - pred).abs() < 6e-5, "{fc} {r}: k {got} vs {pred}");
        }
    }

    /// § 4: D11/D12 alone. The extra damping at HP (× R139) is 0.010 /
    /// 0.082 / 0.206 / 0.346 / 0.644 / 1.264 V for BP 1 / 2 / 3 / 4 / 6 /
    /// 10 V; the first lab's exact `diode` (no R_S) agrees to 1 mV.
    #[test]
    fn antisaturator_alone() {
        let p = Parts::default();
        for (bp, ua, v) in [
            (1.0, 0.096, 0.010),
            (2.0, 0.821, 0.082),
            (3.0, 2.057, 0.206),
            (4.0, 3.460, 0.346),
            (6.0, 6.441, 0.644),
            (10.0, 12.642, 1.264),
        ] {
            let i = antisaturator(&p, bp);
            assert!((i * 1e6 - ua).abs() < 6e-4, "BP {bp}: {} µA", i * 1e6);
            assert!((i * p.r139 - v).abs() < 6e-4, "BP {bp}");
            let old = super::super::ota_svf::diode(bp).0;
            assert!((old - i * p.r139).abs() < 1e-3, "BP {bp}: old {old}");
        }
    }

    /// § 5 measured at ×8 (a 1 mV impulse, 2^19 steps): f0 and k = 1/|BP(f0)|
    /// as the lab recorded them (f0 to 0.1 Hz, k to 2e-4, the passbands to
    /// 0.02 dB); k measured within +0.0040 of the algebra's from 1 kHz up
    /// (recorded +0.0016 … +0.0034) and within 0.013 at 100 Hz for RES ≥ ½
    /// (recorded 0.0128 at ½, 0.0034 at 0.9, ≤ 0.0024 above: `notes.md`'s
    /// "within 0.002" holds only from RES 0.98);
    /// Q at RES 1 is 59 / 47 / 24 / 13 at 100 Hz … 12 kHz: it rings, never
    /// holds.
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn small_signal_matches_the_algebra() {
        let p = Parts::default();
        let rows = super::super::par_map(&GRID, |&(fc, r, ..)| {
            let rs = small_signal(&p, fc, r);
            let (f0, pk) = bp_peak(&rs, fc);
            let lp = db(at(&rs.lp, rs.df, fc / 10.0));
            let hp = db(at(&rs.hp, rs.df, (fc * 10.0).min(150e3)));
            (f0, 1.0 / pk, lp, hp)
        });
        for ((fc, r, f0w, _, pred, kw, lpw, hpw), (f0, k, lp, hp)) in GRID.into_iter().zip(rows) {
            eprintln!("{fc} {r}: f0 {f0:.1} k {k:.4} LP {lp:.2} HP {hp:.2}");
            assert!((f0 - f0w).abs() < 0.1, "{fc} {r}: f0 {f0}");
            assert!((k - kw).abs() < 2e-4, "{fc} {r}: k {k}");
            assert!((lp - lpw).abs() < 0.02, "{fc} {r}: LP {lp}");
            assert!((hp - hpw).abs() < 0.02, "{fc} {r}: HP {hp}");
            if fc >= 1000.0 {
                assert!((0.0..0.0040).contains(&(k - pred)), "{fc} {r}");
            } else if r >= 0.5 {
                assert!((k - pred).abs() < 0.013, "{fc} {r}");
            }
            if r == 1.0 {
                assert!(k > 0.0 && 1.0 / k < 60.0, "{fc}: Q {}", 1.0 / k);
            }
        }
    }

    /// § 6: MODE's notch at fc 1 kHz, RES 0 lands where PS19 puts it,
    /// fc·√((1−m)/m), less the trim's 1 %: 2970 / 1715 / 990 / 572 / 330 Hz
    /// (to 2 Hz), as deep as recorded (to 0.3 dB, −54 … −74 dB), and the
    /// bands at 100 Hz and 10 kHz as recorded (to 0.02 dB).
    #[test]
    #[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
    fn mode_notch() {
        let rs = small_signal(&Parts::default(), 1000.0, 0.0);
        for (m, fw, depth, lo, hi) in [
            (0.1, 2970.0, -74.1, -0.97, -21.07),
            (0.25, 1715.0, -64.1, -2.58, -12.57),
            (0.5, 990.0, -57.4, -6.16, -6.38),
            (0.75, 572.0, -54.4, -12.36, -2.80),
            (0.9, 330.0, -54.6, -20.88, -1.20),
        ] {
            let mix: Vec<Complex64> = rs
                .lp
                .iter()
                .zip(&rs.hp)
                .map(|(l, h)| l * (1.0 - m) + h * m)
                .collect();
            let (a, b) = ((100.0 / rs.df) as usize, (10_000.0 / rs.df) as usize);
            let best = (a..b)
                .min_by(|&i, &j| mix[i].norm().total_cmp(&mix[j].norm()))
                .unwrap();
            let f = best as f64 * rs.df;
            let pred = 1000.0 * ((1.0 - m) / m).sqrt();
            assert!((f - fw).abs() < 2.0, "m {m}: {f}");
            assert!((f / pred - 0.99).abs() < 0.003, "m {m}: {f} vs {pred}");
            assert!((db(mix[best].norm()) - depth).abs() < 0.3, "m {m}");
            assert!((db(at(&mix, rs.df, 100.0)) - lo).abs() < 0.02, "m {m}");
            assert!((db(at(&mix, rs.df, 10_000.0)) - hi).abs() < 0.02, "m {m}");
        }
    }
}
