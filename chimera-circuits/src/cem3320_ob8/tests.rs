//! The lab's `validate`, probes, onset and A/B as tests. Every threshold
//! is the ledger's number (`docs/cem3320_ob8/validation.md`, `probes.md`)
//! with a margin, beside the data sheet's bound where the lab compared
//! against one. The reference at 8× is slow: its tests are ignored in
//! DEBUG; run them with
//! `cargo test -p chimera-circuits --release -- --ignored cem3320`.

use super::chip::{CAL, Chip, VT, antilog, ota, ota_knee};
use super::fw::Fw;
use super::k_of;
use super::measure::{growth, harm, null_scaled_db, onset_k, peak_f, pole_of, thd};
use super::reference::{OS_REF, Reference, render_ref};
use super::stimuli::{bassline, kick_case, saw_case, sine_case};
use crate::FS;
use crate::util::harness::{Case, ac, pad_run, tail};
use crate::util::measure::{db, lsd, rms_db, tone};
use crate::util::sig::{firwin_kaiser, resample_poly};
use chimera_core::dsp::filter::{Filter, FilterKind, FilterMods, K2040Voice};
use chimera_core::params::FilterParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::f64::consts::PI;

fn close(what: &str, got: f64, want: f64, tol: f64) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: {got:.4}, ledger {want} ± {tol}"
    );
}

// ------------------------------------------------------------ renderers

const OB8: Chip = Chip::OB8;
const DS: Chip = Chip::DATASHEET;

/// The lab's `render(Which::Fw1 | Fw2, case)`: padded, AC-coupled.
fn fw(case: &Case, os: usize) -> Vec<f64> {
    ac(&pad_run(case, |c| Ok(Fw::new(os).render(&OB8, c))).unwrap())
}

/// The lab's `render(Which::Ref, case)`.
fn rf(case: &Case) -> Vec<f64> {
    ac(&pad_run(case, |c| Ok(render_ref(&OB8, c, OS_REF, 1.0))).unwrap())
}

/// The lab's `chimera_kind(case, K2040)`: today's REV2 through the shipped
/// `Filter`, params read once a block, padded and AC-coupled as `render`.
fn rev2(case: &Case) -> Vec<f64> {
    let f = |case: &Case| {
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::K2040);
        p.voice = K2040Voice::Rev2;
        let mut f = Filter::new();
        let n = case.x.len().next_multiple_of(BLOCK_SIZE);
        let mut y: Vec<f32> = case.x.iter().map(|&v| v as f32).collect();
        y.resize(n, 0.0);
        for (b, blk) in y.as_chunks_mut::<BLOCK_SIZE>().0.iter_mut().enumerate() {
            let i = (b * BLOCK_SIZE).min(case.x.len() - 1);
            (p.cutoff, p.resonance) = (case.fc[i] as f32, case.res[i] as f32);
            f.process(blk, &p, SAMPLE_RATE, &FilterMods::NONE);
        }
        y.truncate(case.x.len());
        y.into_iter().map(f64::from).collect()
    };
    ac(&pad_run(case, |c| Ok(f(c))).unwrap())
}

/// Probe 1: passband sine 220 Hz at FREQ 2 kHz, RES 0: (gain dB, H2 dBc,
/// H3 dBc) at `level`.
fn passband(r: &dyn Fn(&Case) -> Vec<f64>, level: f64) -> (f64, f64, f64) {
    let y = r(&sine_case(220.0, 2000.0, 0.0, level, 0.6));
    let h = harm(&y, 220.0, 5);
    let g = 10f64.powf(level / 20.0);
    (db(h[0] / g), db(h[1] / h[0]), db(h[2] / h[0]))
}

/// Probe 3: 55 Hz under FREQ 1 kHz, gain at `res` re RES 0 at `level`.
fn bass_loss(r: &dyn Fn(&Case) -> Vec<f64>, res: f64, level: f64) -> f64 {
    let t = |res| tone(tail(&r(&sine_case(55.0, 1000.0, res, level, 1.0))), 55.0);
    db(t(res) / t(0.0))
}

/// Probe 4: the ring after a kick at FREQ `f`, RES `res`, unpadded by the
/// blocker: (peak, f_ring / FREQ, THD %, H2 dBc, mean % of peak).
fn ring(r: &dyn Fn(&Case) -> Vec<f64>, f: f64, res: f64) -> (f64, f64, f64, f64, f64) {
    let y = r(&kick_case(f, res, 1.5));
    let tl = tail(&y);
    let mean = tl.iter().sum::<f64>() / tl.len() as f64;
    let pk = tl.iter().fold(0.0f64, |a, v| a.max((v - mean).abs()));
    let (fr, _) = peak_f(tl, 0.6 * f, 1.4 * f);
    let h = harm(&y, fr, 8);
    (
        pk,
        fr / f,
        100.0 * thd(&h),
        db(h[1] / h[0]),
        100.0 * mean / pk,
    )
}

fn fw_raw(c: &Case) -> Vec<f64> {
    pad_run(c, |c| Ok(Fw::new(1).render(&OB8, c))).unwrap()
}

fn rf_raw(c: &Case) -> Vec<f64> {
    pad_run(c, |c| Ok(render_ref(&OB8, c, OS_REF, 1.0))).unwrap()
}

// ------------------------------------------------- the chip (fast)

#[test]
fn cem3320_chip_maps_match_the_ledger() {
    // The 1 MΩ reflected (data sheet 0.5–2 MΩ): V_A 126 V on each side.
    let reflected = 1.0 / (OB8.i_ref * (1.0 / OB8.va_n + 1.0 / OB8.va_p));
    close("reflected MΩ", reflected / 1e6, 1.0, 0.005);
    close("R_EQ kΩ", OB8.r_eq() / 1e3, 90.909, 0.01);
    // Data sheet circuit: loop gain per mho, onset I_CR, k at 100 µA.
    close("DS loop gain per mho", DS.k_per_gm(), 5976.0, 1.0);
    close("DS onset I_CR µA", DS.icr_of_k(4.0) * 1e6, 49.8, 0.1);
    close(
        "DS k_max(100 µA)",
        DS.gm(100e-6) * DS.k_per_gm(),
        6.15,
        0.01,
    );
    // OB-8: one unit is ±2.29 V at stage 1; k 4 and k 4.21 currents.
    close("OB-8 V per unit", OB8.out_per_unit(), 2.29, 0.005);
    close("OB-8 loop gain per mho", OB8.k_per_gm(), 2442.0, 1.0);
    close("OB-8 I_CR(k 4) µA", OB8.icr_of_k(4.0) * 1e6, 333.0, 1.0);
    close("OB-8 I_CR(k 4.21) µA", OB8.icr_of_k(4.21) * 1e6, 413.0, 1.0);
    close(
        "OB-8 gm at k 4.21 mmho",
        OB8.gm(OB8.icr_of_k(4.21)) * 1e3,
        1.72,
        0.005,
    );
    // RES CV 10 V through RX49 33 K against RX50 330 K to −5 V.
    let icr = 10.0 / 33e3 - 5.0 / 330e3;
    close("OB-8 I_CR at 10 V µA", icr * 1e6, 288.0, 0.5);
    close("OB-8 k at 10 V", OB8.gm(icr) * OB8.k_per_gm(), 3.85, 0.005);
    // The house law.
    close("k_of(1)", k_of(1.0), 4.21, 0.005);
}

#[test]
fn cem3320_resonance_gm_follows_fig6() {
    // Data sheet: 0.8 / 1.0 / 1.2 mmho at 100 µA; fig. 6 ≈1.4 at 200 µA,
    // ≈1.6 at 300, max ≈2.2. Ledger: 1.028, 1.401, 1.594 (R_E 455 Ω).
    let gm = |i: f64| (ota(1e-6, i, &DS).0 - ota(-1e-6, i, &DS).0) / 2e-6 * 1e3;
    let g100 = gm(100e-6);
    assert!((0.8..=1.2).contains(&g100), "gm(100 µA) {g100}");
    close("gm 25 µA", gm(25e-6), 0.396, 0.002);
    close("gm 50 µA", gm(50e-6), 0.671, 0.002);
    close("gm 100 µA", g100, 1.028, 0.002);
    close("gm 200 µA", gm(200e-6), 1.401, 0.002);
    close("gm 300 µA", gm(300e-6), 1.594, 0.002);
    close("gm 1 mA", gm(1e-3), 1.973, 0.002);
    close("max gm", 1e3 / DS.r_ota, 2.2, 0.005);
    // The small-signal formula agrees with the pair itself.
    close("gm formula 100 µA", DS.gm(100e-6) * 1e3, g100, 1e-4);
    // Large-signal knee at pin 8 (I_out 90 % of I_CR), mV.
    for (i, mv) in [
        (25e-6, 86.0),
        (50e-6, 97.0),
        (100e-6, 117.0),
        (200e-6, 158.0),
    ] {
        close("knee mV", ota_knee(i, &DS) * 1e3, mv, 1.0);
    }
}

#[test]
fn cem3320_icr_of_k_inverts_gm() {
    // Up to the pair's ceiling (gm < 1/R_E: k 5.36 on the OB-8).
    for ch in [OB8, DS] {
        for i in 1..=20 {
            let k = 0.25 * i as f64;
            let back = ch.gm(ch.icr_of_k(k)) * ch.k_per_gm();
            assert!((back / k - 1.0).abs() < 1e-9, "k {k}: {back}");
        }
    }
}

#[test]
fn cem3320_ota_solves_the_degenerated_pair() {
    for &i in &[25e-6, 100e-6, 413e-6] {
        for &v in &[-0.6, -0.1, -1e-3, 0.0, 2e-3, 0.05, 0.3] {
            let (io, d) = ota(v, i, &DS);
            let x: f64 = io / i;
            assert!(x.abs() < 1.0);
            let back = 2.0 * VT * x.atanh() + DS.r_ota * i * x;
            assert!((back - v).abs() < 1e-9, "v {v} i {i}: {back}");
            let h = 1e-7;
            let fd = (ota(v + h, i, &DS).0 - ota(v - h, i, &DS).0) / (2.0 * h);
            assert!((fd - d).abs() <= 1e-4 * d.abs() + 1e-12, "{fd} vs {d}");
        }
    }
    assert_eq!(ota(0.1, 0.0, &DS), (0.0, 0.0));
}

#[test]
fn cem3320_antilog_is_the_cell() {
    // re = 0: exactly G·I_D. G = 1: the ohmic terms cancel, I_C = I_D.
    let ideal = Chip { re: 0.0, ..OB8 };
    let id = 40e-6;
    assert_eq!(antilog(id, 0.3, &ideal), (0.3 * id, 0.3));
    let (ic, _) = antilog(id, 1.0, &OB8);
    assert!((ic / id - 1.0).abs() < 1e-12, "{ic}");
    // Starved: no current, either way.
    assert_eq!(antilog(-1e-6, 0.3, &OB8).0, 0.0);
    // The derivative is the slope.
    for &g in &[0.01, 0.3, 2.0] {
        for &id in &[5e-6, 63e-6, 150e-6] {
            let (ic, d) = antilog(id, g, &OB8);
            let h = 1e-10;
            let fd = (antilog(id + h, g, &OB8).0 - antilog(id - h, g, &OB8).0) / (2.0 * h);
            assert!((fd / d - 1.0).abs() < 1e-5, "g {g} id {id}: {fd} vs {d}");
            // The implicit equation holds.
            let r = OB8.re / VT;
            let rhs = g * id * (r * ((id - ic) - (1.0 - g) * OB8.i_ref)).exp();
            assert!((ic / rhs - 1.0).abs() < 1e-10);
        }
    }
}

#[test]
fn cem3320_g_set_inverts_its_error_model() {
    let rho = OB8.re * OB8.i_ref / VT;
    for fc in [50.0, 1000.0, 12_000.0] {
        let g = OB8.g_set(fc);
        let eff = g / (1.0 + g / OB8.beta) * (1.0 + rho) / (1.0 + g * rho);
        let want = OB8.g_of(fc) * CAL;
        assert!((eff / want - 1.0).abs() < 1e-6, "{fc}: {eff} vs {want}");
    }
}

#[test]
fn cem3320_quiescent_points() {
    // Data sheet: inputs a nominal 650 mV, buffers near 0.46·V_CC = 6.9 V.
    // The bias network leaves each stage at its own point (ledger).
    let fs = FS * OS_REF as f64;
    let ds = Reference::new(DS, fs, 0.9);
    for (k, y) in [7.645, 6.476, 7.642, 6.479].into_iter().enumerate() {
        let (n, _, out) = ds.node(k);
        close("DS input node V", n, 0.650, 0.001);
        close("DS buffer V", out, y, 0.002);
    }
    let ob = Reference::new(OB8, fs, OB8.g_of(1000.0));
    for (k, y) in [6.955, 7.123, 6.955, 7.123].into_iter().enumerate() {
        let (n, _, out) = ob.node(k);
        close("OB-8 input node V", n, 0.650, 0.001);
        close("OB-8 buffer V", out, y, 0.002);
        assert!((out - 6.9).abs() < 0.8, "far from 0.46·V_CC");
    }
    assert_eq!(ob.fails, 0);
}

// ------------------------------------------------- the fw model (fast)

#[test]
fn cem3320_fw_is_finite_and_unity_at_dc() {
    // Unity at DC with RES 0. (With RES up the loop also passes DC until
    // CX20 settles, 1.2 Hz, so only finiteness is checked there.)
    for (os, res) in [(1, 0.0), (1, 0.5), (2, 0.0)] {
        let n = 9600;
        let case = Case {
            x: vec![0.1; n],
            fc: vec![2000.0; n],
            res: vec![res; n],
        };
        let y = Fw::new(os).render(&OB8, &case);
        assert_eq!(y.len(), n);
        assert!(y.iter().all(|v| v.is_finite()));
        let end = y[n - 100..].iter().sum::<f64>() / 100.0;
        if res == 0.0 {
            assert!((end / 0.1 - 1.0).abs() < 0.005, "os {os}: {end}");
        }
    }
}

#[test]
fn cem3320_fw_pole_sits_on_freq() {
    // Four poles at FREQ: |H(FREQ)| = 1/4 (−12.04 dB) re DC, which is
    // unity (above). Unblocked, so low FREQs keep their passband.
    for fc in [250.0, 1000.0, 4000.0] {
        let y = fw_raw(&sine_case(fc, fc, 0.0, -20.0, 0.6));
        let hdb = db(tone(tail(&y), fc)) + 20.0;
        let fp = pole_of(hdb, fc);
        assert!((fp / fc - 1.0).abs() < 0.01, "FREQ {fc}: pole {fp:.1}");
    }
}

#[test]
fn cem3320_fw_probes_match_the_ledger() {
    let r = |c: &Case| fw(c, 1);
    // Probe 1: gain / H2 / H3 (ledger fw 1×).
    let (g0, _, _) = passband(&r, 0.0);
    close("passband +0 dB gain", g0, -0.23, 0.01);
    let (g9, h2, h3) = passband(&r, 9.0);
    close("passband +9 gain", g9, -0.51, 0.02);
    close("passband +9 H2", h2, -45.0, 1.0);
    close("passband +9 H3", h3, -32.0, 1.0);
    let (g12, h2, h3) = passband(&r, 12.0);
    close("passband +12 gain", g12, -2.62, 0.02);
    close("passband +12 H2", h2, -41.0, 1.0);
    close("passband +12 H3", h3, -17.0, 1.0);
    // Probe 2: 110 Hz saw at FREQ 1 kHz, RES 0.85, 0 dB.
    let y = r(&saw_case(110.0, 1000.0, 0.85, 0.0, 0.8));
    let h = harm(&y, 110.0, 40);
    close("saw H1 gain", db(h[0] / 0.585), -13.19, 0.02);
    close("saw H9 re H1", db(h[8] / h[0]), 3.3, 0.1);
    let (num, den) = h.iter().enumerate().fold((0.0, 0.0), |(a, b), (i, v)| {
        (a + 110.0 * (i + 1) as f64 * v * v, b + v * v)
    });
    close("saw centroid Hz", num / den, 686.0, 2.0);
    // Probe 3: bass loss at RES 0.25, +9 dB.
    close("bass loss 0.25 @ +9", bass_loss(&r, 0.25, 9.0), -4.12, 0.02);
}

#[test]
fn cem3320_fw_ring_matches_the_ledger() {
    // Probe 4 at FREQ 1 kHz, RES 1 (k 4.21): 0.963 / 0.996 / 0.4 % /
    // −49 dBc / +3.4 %. The data sheet's oscillation THD: 0.5 typ, 1.5 max.
    let (pk, fr, thd, h2, mean) = ring(&fw_raw, 1000.0, 1.0);
    close("ring peak", pk, 0.963, 0.005);
    close("ring f / FREQ", fr, 0.996, 0.002);
    close("ring THD %", thd, 0.4, 0.1);
    assert!(thd < 1.5);
    close("ring H2 dBc", h2, -49.0, 1.5);
    close("ring DC shift %", mean, 3.4, 0.3);
}

#[test]
fn cem3320_fw_onset_brackets_the_ledger() {
    // Ledger onset k, fw 1×: 3.919 at 1 kHz (reference 3.910).
    let r = |c: &Case| Fw::new(1).render(&OB8, c);
    assert!(growth(&r, 1000.0, 3.88, 1e-2) < 0.0);
    assert!(growth(&r, 1000.0, 3.96, 1e-2) > 0.0);
}

// ------------------------------------------- the reference (slow)

/// A sine through the reference at fixed controls, decimated: the lab's
/// `ref_sine`.
fn ref_sine(ch: &Chip, f: f64, amp: f64, g: f64, icr: f64, secs: f64) -> Vec<f64> {
    let fs = FS * OS_REF as f64;
    let mut r = Reference::new(*ch, fs, g);
    let n = (secs * fs) as usize;
    let y: Vec<f64> = (0..n)
        .map(|i| {
            let x = amp * (2.0 * PI * f * i as f64 / fs).sin();
            r.step(x, g, icr) - r.y_q
        })
        .collect();
    assert_eq!(r.fails, 0);
    let mut y = resample_poly(&y, 1, OS_REF);
    y.truncate(n / OS_REF);
    y
}

/// The data sheet's pole at `V_C` (mV): (formula Hz, measured Hz).
fn ds_pole(vc: f64) -> (f64, f64) {
    let g = 0.9 * (-(vc * 1e-3) / VT).exp();
    let fp = g / (2.0 * PI * DS.r_eq() * DS.c);
    let f0 = fp.min(40.0) * 0.25;
    let secs = ((12.0 / fp).max(0.06) + 0.3).max(40.0 / f0 + 0.3);
    let half = |v: Vec<f64>| v[v.len() / 2..].to_vec();
    let y = half(ref_sine(&DS, fp, 0.05, g, 0.0, secs));
    let y0 = half(ref_sine(&DS, f0, 0.05, g, 0.0, secs));
    (fp, pole_of(db(tone(&y, fp)) - db(tone(&y0, f0)), fp))
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_reference_pole_tracks_vc() {
    // Data sheet: 57.5 / 60 / 62.5 mV/decade. Ledger: 60.37 (0 → 120 mV);
    // f_p vs formula −1.57 % at 0 mV, +0.64 % at 60, +4.78 % at 120.
    let (p0, m0) = ds_pole(0.0);
    let (p60, m60) = ds_pole(60.0);
    let (p120, m120) = ds_pole(120.0);
    let slope = 120.0 / (m0 / m120).log10();
    assert!((57.5..=62.5).contains(&slope), "{slope} mV/decade");
    close("mV/decade", slope, 60.37, 0.05);
    close("error % at 0 mV", 100.0 * (m0 / p0 - 1.0), -1.57, 0.05);
    close("error % at 60 mV", 100.0 * (m60 / p60 - 1.0), 0.64, 0.05);
    close("error % at 120 mV", 100.0 * (m120 / p120 - 1.0), 4.78, 0.05);
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_reference_swing_and_distortion() {
    // Swing at clip: data sheet 10 / 12 / 14 V p-p; ledger 11.37
    // (−5.10 … +6.28 V).
    let y = ref_sine(&DS, 50.0, 12.0, DS.g_of(5000.0), 0.0, 0.5);
    let (mn, mx) = tail(&y)
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    assert!((10.0..=14.0).contains(&(mx - mn)));
    close("swing V p-p", mx - mn, 11.37, 0.02);
    close("swing low V", mn, -5.10, 0.02);
    #[allow(clippy::approx_constant)] // the ledger's +6.28 V, not τ
    close("swing high V", mx, 6.28, 0.02);
    // THD 3 dB under clip: data sheet passband 0.1 % typ / 0.3 max,
    // bandreject 0.3 / 1, mostly H2. Ledger (f_p, passband %, bandreject %,
    // bandreject H2 dBc).
    let amp = 6.0 * 10f64.powf(-3.0 / 20.0);
    for (fp, pass, rej, h2) in [
        (500.0, 0.031, 0.111, -59.1),
        (2000.0, 0.017, 0.112, -59.0),
        (5000.0, 0.018, 0.114, -58.9),
        (12_000.0, 0.019, 0.122, -58.2),
    ] {
        let g = DS.g_of(fp);
        let h = harm(&ref_sine(&DS, fp / 10.0, amp, g, 0.0, 0.6), fp / 10.0, 8);
        let t = 100.0 * thd(&h);
        assert!(t < 0.1, "passband THD {t} % at {fp}");
        close("passband THD %", t, pass, 0.003);
        let f = 0.4349 * fp;
        let h = harm(&ref_sine(&DS, f, amp, g, 0.0, 0.6), f, 8);
        let t = 100.0 * thd(&h);
        assert!(t < 0.3, "bandreject THD {t} % at {fp}");
        close("bandreject THD %", t, rej, 0.003);
        close("bandreject H2 dBc", db(h[1] / h[0]), h2, 0.3);
        assert!(h[1] > 10.0 * h[2], "H2 dominates");
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_reference_resonance_onset_and_peak() {
    // Data sheet circuit, f_p 1 kHz, a 0.2 V kick: the ring decays just
    // under the onset I_CR (49.8 µA), grows just over it, and rings within
    // 0.5 % of f_p unclipped (data sheet: < 0.5 %) at 0.13 % THD (data
    // sheet 0.5 typ). Ledger: growth −5.43 / +2.18 dB per 50 ms at 0.99 /
    // 1.01; at 1.1×: f 1.0051, THD 0.13 %, peak 0.721 V.
    let fs = FS * OS_REF as f64;
    let fp = 1000.0;
    let g = DS.g_of(fp);
    let on = DS.icr_of_k(4.0);
    let ring = |icr: f64| {
        let mut r = Reference::new(DS, fs, g);
        let n = (1.2 * fs) as usize;
        let y: Vec<f64> = (0..n)
            .map(|i| r.step(if i < 40 { 0.2 } else { 0.0 }, g, icr) - r.y_q)
            .collect();
        assert_eq!(r.fails, 0);
        let y = resample_poly(&y, 1, OS_REF);
        let env: Vec<f64> = y
            .chunks((0.05 * FS) as usize)
            .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
            .collect();
        (db(env[6] / env[2].max(1e-30)) / 4.0, y)
    };
    let (under, _) = ring(0.99 * on);
    let (over, _) = ring(1.01 * on);
    close("growth at 0.99×", under, -5.43, 0.1);
    close("growth at 1.01×", over, 2.18, 0.1);
    let (_, y) = ring(1.1 * on);
    let tl = tail(&y);
    let pk = tl.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    let (fr, _) = peak_f(tl, 0.5 * fp, 1.5 * fp);
    let t = 100.0 * thd(&harm(&y, fr, 6));
    assert!((fr / fp - 1.0).abs() < 0.0055 && t < 0.5);
    close("ring f / f_p", fr / fp, 1.0051, 0.0005);
    close("ring THD %", t, 0.13, 0.02);
    close("ring peak V", pk, 0.721, 0.01);
    // Resonance before oscillation: data sheet 20 min / 30 typ dB. Ledger
    // at k 3.9: +33.0 dB re DC at 0.991·f_p.
    let icr = DS.icr_of_k(3.9);
    let lin = |f: f64| tone(tail(&ref_sine(&DS, f, 0.02, g, icr, 0.7)), f);
    let peak = db(lin(0.9913 * fp) / lin(30.0));
    assert!(peak > 30.0, "{peak}");
    close("peak re DC at k 3.9", peak, 33.0, 0.3);
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_reference_noise() {
    // Data sheet: −76 min / −86 typ dB re max output, LP at 20 kHz.
    // Ledger: 124.4 µV rms, −90.7 dB re 12 V p-p (white only, no 1/f).
    let fs = FS * OS_REF as f64;
    let g = DS.g_of(20_000.0);
    let mut r = Reference::new(DS, fs, g).with_noise();
    let n = (0.6 * fs) as usize;
    let y: Vec<f64> = (0..n).map(|_| r.step(0.0, g, 0.0) - r.y_q).collect();
    let y = resample_poly(&y, 1, OS_REF);
    let y = ac(&y[(0.1 * FS) as usize..]);
    let h = firwin_kaiser(255, 20_000.0 / (FS / 2.0), 10.0);
    let yl: Vec<f64> = (h.len()..y.len())
        .map(|i| h.iter().enumerate().map(|(j, c)| c * y[i - j]).sum())
        .collect();
    let rms = (yl.iter().map(|v| v * v).sum::<f64>() / yl.len() as f64).sqrt();
    let re = db(rms / 4.243);
    assert!(re < -86.0, "{re} dB");
    close("noise µV", rms * 1e6, 124.4, 1.0);
    close("noise dB", re, -90.7, 0.1);
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_reference_ob8_pole_calibration() {
    // Through g_of alone the OB-8 pole sits +0.49 % high at 1 kHz; g_set
    // (with CAL) puts it on FREQ: ledger −0.01 % at 1 kHz, +0.10 % at
    // 4 kHz, +0.31 % at 12 kHz. (50 and 200 Hz, +0.01 % each, take 33 s
    // and 8 s of 384 kHz Newton a run and are left to the lab.)
    let fs = FS * OS_REF as f64;
    let pole = |g: f64, fc: f64| {
        let f0 = (fc / 40.0).min(10.0);
        let secs = 40.0 / f0 + 0.3;
        let lin = |f: f64| {
            let mut r = Reference::new(OB8, fs, g);
            let n = (secs * fs) as usize;
            let y: Vec<f64> = (0..n)
                .map(|i| r.step(0.05 * (2.0 * PI * f * i as f64 / fs).sin(), g, 0.0) - r.y_q)
                .collect();
            let y = resample_poly(&y, 1, OS_REF);
            tone(&y[y.len() / 2..], f)
        };
        100.0 * (pole_of(db(lin(fc) / lin(f0)), fc) / fc - 1.0)
    };
    close("g_of 1 kHz %", pole(OB8.g_of(1000.0), 1000.0), 0.49, 0.03);
    for (fc, e) in [(1000.0, -0.01), (4000.0, 0.10), (12_000.0, 0.31)] {
        close("g_set %", pole(OB8.g_set(fc), fc), e, 0.03);
    }
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_fw_matches_reference_on_the_probes() {
    // Ledger probe 1 at +9 dB: ref −0.63 / −47 / −29, fw −0.51 / −45 / −32;
    // probe 3 at RES 0.25, +9 dB: ref −4.05, fw −4.12; probe 4 at 1 kHz,
    // RES 1: ref 0.942 / 0.996 / H2 −54 / +5.0 %, fw 0.963 / 0.996 / −49 /
    // +3.4 %.
    let (g, h2, h3) = passband(&rf, 9.0);
    close("ref passband +9 gain", g, -0.63, 0.02);
    close("ref passband +9 H2", h2, -47.0, 1.0);
    close("ref passband +9 H3", h3, -29.0, 1.0);
    let (fg, _, _) = passband(&|c| fw(c, 1), 9.0);
    assert!((fg - g).abs() < 0.2, "fw {fg} vs ref {g}");
    let b = bass_loss(&rf, 0.25, 9.0);
    close("ref bass loss 0.25 @ +9", b, -4.05, 0.02);
    let (pk, fr, _, h2, mean) = ring(&rf_raw, 1000.0, 1.0);
    close("ref ring peak", pk, 0.942, 0.005);
    close("ref ring f / FREQ", fr, 0.996, 0.002);
    close("ref ring H2 dBc", h2, -54.0, 1.5);
    close("ref ring DC shift %", mean, 5.0, 0.3);
    // 2× is within 0.1 dB of 1× (the lab's reason to stay at 1×).
    let r2 = passband(&|c| fw(c, 2), 9.0).0;
    assert!((r2 - fg).abs() < 0.1);
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_fw_onset_tracks_reference() {
    // Ledger onset k: ref 3.910 (1 kHz), 3.708 (4 kHz); CX16 removed
    // 3.995 (1 kHz); fw 3.919, 3.695. Scans start on the lab's 4.3 grid.
    let ref_on = |ch: Chip, fc: f64, from: f64| {
        onset_k(&|c| render_ref(&ch, c, OS_REF, 1.0), fc, 1e-4, from)
    };
    let fw_on = |fc: f64, from: f64| onset_k(&|c| Fw::new(1).render(&OB8, c), fc, 1e-2, from);
    close("ref onset 1 kHz", ref_on(OB8, 1000.0, 4.02), 3.910, 0.01);
    close("ref onset 4 kHz", ref_on(OB8, 4000.0, 3.82), 3.708, 0.01);
    close(
        "ref onset 1 kHz, no CX16",
        ref_on(Chip { c8: 0.0, ..OB8 }, 1000.0, 4.1),
        3.995,
        0.01,
    );
    close("fw onset 1 kHz", fw_on(1000.0, 4.02), 3.919, 0.01);
    close("fw onset 4 kHz", fw_on(4000.0, 3.82), 3.695, 0.01);
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn cem3320_fw_nulls_reference_on_music() {
    // Ledger A/B, bassline at 0 dB: fw vs ref null −49.6 / LSD 0.03 at
    // RES 0, −44.0 / 0.07 at RES 0.5; ref vs REV2 −32.3 / 0.14 at RES 0
    // (the finding: the chip and REV2 are the same sound there).
    let (x, fc) = bassline();
    let case = |res: f64| Case {
        x: x.clone(),
        fc: fc.clone(),
        res: vec![res; x.len()],
    };
    // LSD on RMS-matched renders, as the lab's anchored WAVs were.
    let matched = |y: Vec<f64>, to: &[f64]| {
        let g = 10f64.powf((rms_db(to) - rms_db(&y)) / 20.0);
        y.into_iter().map(|v| v * g).collect::<Vec<f64>>()
    };
    for (res, null, l) in [(0.0, -49.6, 0.03), (0.5, -44.0, 0.07)] {
        let c = case(res);
        let r = rf(&c);
        let f = matched(fw(&c, 1), &r);
        close("fw vs ref null dB", null_scaled_db(&f, &r), null, 1.0);
        close("fw vs ref LSD dB", lsd(&f, &r), l, 0.02);
        if res == 0.0 {
            let v = matched(rev2(&c), &r);
            close("REV2 vs ref null dB", null_scaled_db(&v, &r), -32.3, 1.0);
            close("REV2 vs ref LSD dB", lsd(&v, &r), 0.14, 0.02);
        }
    }
}
