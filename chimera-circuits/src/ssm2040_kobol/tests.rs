//! The Kobol lab's numbers, kept: EXP-C's small-signal behaviour, ring and
//! drive (fast); EXP-C against the firmware's EXP (dev-dependency); EXP-C
//! against the transistor-level circuit (needs ngspice); and the lab's
//! readings of the owner's takes (needs `KOBOL_DIR`). Every threshold
//! names the ledger number behind it (`docs/ssm2040_kobol/`).

use super::circuit::{self, Kobol, V_FULL};
use super::fw::{E_FULL, Exp2, K_MAX, Offsets};
use super::takes::{self, *};
use crate::FS;
use crate::util::measure::{lsd, null_db};
use std::f64::consts::PI;
use std::path::PathBuf;

/// EXP-C on `x` at FREQ `fc` and loop gain `k`.
fn expc(o: Offsets, x: &[f64], fc: &dyn Fn(usize) -> f64, k: &dyn Fn(usize) -> f64) -> Vec<f64> {
    Exp2::new(o, FS as f32).render(x, fc, k)
}

fn unit(x: &[f64], fc: &dyn Fn(usize) -> f64, k: &dyn Fn(usize) -> f64) -> Vec<f64> {
    expc(Offsets::UNIT, x, fc, k)
}

fn sine(f: f64, a: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| a * (2.0 * PI * f * i as f64 / FS).sin())
        .collect()
}

/// `kexp csine`'s stimulus at `amp_v` volts at the mixer (EXP-C's full
/// scale is `V_FULL`): a sine with a 20 ms fade-in, or, at 0, a 1 ms
/// 10 mV kick then silence (self-oscillation).
fn csine(f0: f64, amp_v: f64, secs: f64) -> Vec<f64> {
    let n = (secs * FS) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / FS;
            if amp_v == 0.0 {
                if i < 48 { 0.01 } else { 0.0 }
            } else {
                amp_v * (2.0 * PI * f0 * t).sin() * (t / 0.02).min(1.0)
            }
        })
        .collect()
}

/// The last half of `y`.
fn half(y: &[f64]) -> &[f64] {
    &y[y.len() / 2..]
}

/// A self-oscillation probe: (ring Hz, H1, [H2…H5 dBc]) over the last half.
fn ring(y: &[f64]) -> (f64, f64, [f64; 4]) {
    let h = half(y);
    let f = f0_near(h, f_peak(h).0, 1);
    let (a, d) = hreport(h, f);
    (f, a, d)
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

/// The firmware's 2040 (`voice`) at LEVEL `level`, FREQ and RES read once
/// a block, as the lab's `ours()`.
fn ours(
    voice: chimera_core::dsp::filter::K2040Voice,
    x: &[f64],
    fc: &dyn Fn(usize) -> f64,
    res: &dyn Fn(usize) -> f64,
    level: f32,
) -> Vec<f64> {
    use chimera_core::dsp::filter::{Filter, FilterKind, FilterMods};
    use chimera_core::params::FilterParams;
    use chimera_hal::BLOCK_SIZE;
    let mut p = FilterParams::default();
    p.set_kind(FilterKind::K2040);
    p.voice = voice;
    p.level = level;
    let mut f = Filter::new();
    let mut y: Vec<f32> = x.iter().map(|&v| v as f32).collect();
    y.resize(x.len().next_multiple_of(BLOCK_SIZE), 0.0);
    for (b, blk) in y.as_chunks_mut::<BLOCK_SIZE>().0.iter_mut().enumerate() {
        let i = b * BLOCK_SIZE;
        (p.cutoff, p.resonance) = (fc(i) as f32, res(i) as f32);
        f.process(blk, &p, chimera_hal::SAMPLE_RATE, &FilterMods::NONE);
    }
    y.truncate(x.len());
    y.into_iter().map(f64::from).collect()
}

fn exp(x: &[f64], fc: &dyn Fn(usize) -> f64, res: &dyn Fn(usize) -> f64) -> Vec<f64> {
    ours(chimera_core::dsp::filter::K2040Voice::Exp, x, fc, res, 0.5)
}

/// A model with its RES fixed: `(x, FREQ) → y`.
type Tuned = dyn Fn(&[f64], &dyn Fn(usize) -> f64) -> Vec<f64>;

/// Where ngspice runs keep their files (and their cache).
fn spice_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../target/spice/ssm2040_kobol/{name}"))
}

/// The circuit on `kexp csine`'s probe (`amp_v` volts at the mixer),
/// `secs` long, CI5 at `k`: the output on EXP-C's scale, and OUT 4.
fn circuit_probe(
    c: &Kobol,
    tag: &str,
    f0: f64,
    amp_v: f64,
    fc: f64,
    k: f64,
    secs: f64,
) -> (Vec<f64>, Vec<f64>) {
    let x = csine(f0, amp_v, secs);
    let n = x.len();
    let o = circuit::spice(
        c,
        &circuit::Case {
            x,
            fc: vec![fc; n],
            k: vec![k; n],
        },
        &spice_dir(tag),
    )
    .expect("ngspice (set NGSPICE)");
    let s = c.exp_c_scale();
    (o.y.iter().map(|v| v * s).collect(), o.o4)
}

/// EXP-C on the same probe.
fn expc_probe(o: Offsets, f0: f64, amp_v: f64, fc: f64, k: f64, secs: f64) -> Vec<f64> {
    let x: Vec<f64> = csine(f0, amp_v, secs).iter().map(|v| v / V_FULL).collect();
    expc(o, &x, &|_| fc, &|_| k)
}

/// The takes' directory, or `None` (the test then passes vacuously and
/// says so).
fn takes_dir() -> Option<PathBuf> {
    let d = takes::session();
    if d.is_none() {
        eprintln!(
            "skipped: KOBOL_DIR unset or missing (the owner's .superpowers/ledgers/kobol-session)"
        );
    }
    d
}

const SLOW: &str = "slow: cargo test -p chimera-circuits --release -- --ignored";

#[test]
fn exp_c_is_unity_in_the_passband_at_res_0() {
    // fw.rs: "unity at RES 0 in the passband" (CI3's cubic undone by
    // 1/E_FULL, C6 at 10.2 Hz).
    for o in [Offsets::NONE, Offsets::REFERENCE, Offsets::UNIT] {
        let y = expc(o, &sine(200.0, 0.1, 24_000), &|_| 10_000.0, &|_| 0.0);
        let g = db(tone_at(half(&y), 200.0) / 0.1);
        eprintln!("unity {o:?}: {g:+.3} dB");
        assert!(g.abs() < 0.05, "{o:?}: {g:+.3} dB");
    }
}

fn tone_at(y: &[f64], f: f64) -> f64 {
    amp_at(y, &bh(y.len()), f)
}

#[test]
fn exp_c_pole_sits_where_the_circuit_puts_it() {
    // research.md § 2: four poles at FREQ, stage 1's 1.96 % higher:
    // −3.01 × 3 − 2.93 = −11.96 dB at FREQ.
    for fc in [250.0, 1000.0, 4000.0] {
        let y = expc(Offsets::NONE, &sine(fc, 0.05, 24_000), &|_| fc, &|_| 0.0);
        let g = db(tone_at(half(&y), fc) / 0.05);
        eprintln!("pole {fc}: {g:+.3} dB");
        assert!((g + 11.96).abs() < 0.1, "{fc}: {g:+.3} dB");
    }
}

#[test]
fn exp_c_loses_the_circuit_s_bass_with_res() {
    // notes.md, "Circuit bass loss" (the lab's b0/b2/b5 probes: 56.4 Hz,
    // 1.75 V at the mixer, FREQ 1 kHz): circuit −10.3 dB at k 2.13 and
    // −18.7 at k 5.6; 1/(1+k) is −9.9 / −16.4.
    let h1 = |k: f64| {
        tone_at(
            half(&expc_probe(Offsets::UNIT, 56.4, 1.75, 1000.0, k, 1.5)),
            56.4,
        )
    };
    let b0 = h1(0.0);
    let (b2, b5) = (db(h1(2.13) / b0), db(h1(5.6) / b0));
    eprintln!("bass: k 2.13 {b2:+.2} dB, k 5.6 {b5:+.2} dB");
    assert!((b2 + 10.3).abs() < 0.6, "{b2}");
    assert!((b5 + 18.7).abs() < 1.5, "{b5}");
}

#[test]
fn exp_c_starts_ringing_at_res_0_72() {
    // research.md § 4: on k = 5.59·RES, onset at RES 0.72 (k ≈ 4).
    let rings = |res: f64| {
        let y = expc(
            Offsets::NONE,
            &csine(0.0, 0.0, 1.0)
                .iter()
                .map(|v| v / V_FULL)
                .collect::<Vec<_>>(),
            &|_| 1000.0,
            &|_| super::k_of(res),
        );
        let (a, b) = (rms(seg(&y, 0.2, 0.4)), rms(seg(&y, 0.8, 1.0)));
        eprintln!("onset RES {res}: {:.1} → {:.1} dB", db(a), db(b));
        b > a
    };
    assert!(!rings(0.69));
    assert!(rings(0.75));
}

#[test]
fn exp_c_rings_as_the_lab_measured() {
    // notes.md / ab.md, EXP-C on track2's 15 notes (CI5 1.5 mV): H2
    // −37.7 dBc, H3 −54.2, level spread 1.9 dB; ring 0.37 V at OUT 4
    // (so1k: 0.369); +54 c over FREQ at 1 kHz (degree-7 tanh); tracking
    // slope 0.988, ±1 c.
    let fcs = [
        342.0, 436.0, 491.1, 624.9, 792.5, 891.4, 1127.5, 1423.9, 1789.8, 2006.2, 2508.3, 3132.0,
        3492.5, 4330.7, 5345.9,
    ];
    let seed = noise(1e-5, (0.8 * FS) as usize);
    let rs: Vec<(f64, f64, [f64; 4])> = fcs
        .iter()
        .map(|&fc| ring(&unit(&seed, &|_| fc, &|_| f64::from(K_MAX))))
        .collect();
    let lv: Vec<f64> = rs.iter().map(|r| db(r.1)).collect();
    let spread =
        lv.iter().fold(f64::MIN, |m, v| m.max(*v)) - lv.iter().fold(f64::MAX, |m, v| m.min(*v));
    let h2 = rs.iter().map(|r| r.2[0]).sum::<f64>() / 15.0;
    let h3 = rs.iter().map(|r| r.2[1]).sum::<f64>() / 15.0;
    let at1k = ring(&unit(&seed, &|_| 1000.0, &|_| f64::from(K_MAX)));
    let c1k = cents(at1k.0, 1000.0);
    let out4 = at1k.1 * f64::from(E_FULL);
    // The ring's pitch against FREQ, a line in log-log.
    let (lx, ly): (Vec<f64>, Vec<f64>) = fcs
        .iter()
        .zip(&rs)
        .map(|(f, r)| (f.log2(), r.0.log2()))
        .unzip();
    let (mx, my) = (lx.iter().sum::<f64>() / 15.0, ly.iter().sum::<f64>() / 15.0);
    let slope = lx
        .iter()
        .zip(&ly)
        .map(|(x, y)| (x - mx) * (y - my))
        .sum::<f64>()
        / lx.iter().map(|x| (x - mx).powi(2)).sum::<f64>();
    let resid = lx
        .iter()
        .zip(&ly)
        .map(|(x, y)| (1200.0 * (y - my - slope * (x - mx))).abs())
        .fold(0.0f64, f64::max);
    eprintln!(
        "ring: H2 {h2:.1} H3 {h3:.1} spread {spread:.2} | 1k {:.1} Hz {c1k:+.1} c, OUT4 {out4:.3} V | slope {slope:.4} resid {resid:.1} c",
        at1k.0
    );
    eprintln!(
        "per note: {:?}",
        rs.iter()
            .map(|r| (
                r.0.round(),
                (db(r.1) * 10.0).round() / 10.0,
                (r.2[0] * 10.0).round() / 10.0,
                (r.2[1] * 10.0).round() / 10.0
            ))
            .collect::<Vec<_>>()
    );
    assert!((h2 + 37.7).abs() < 2.0, "H2 {h2}");
    assert!((h3 + 54.2).abs() < 3.0, "H3 {h3}");
    assert!((spread - 1.9).abs() < 0.6, "spread {spread}");
    assert!((out4 - 0.369).abs() < 0.03, "OUT 4 {out4}");
    assert!((c1k - 54.0).abs() < 10.0, "{c1k} c");
    assert!((slope - 0.988).abs() < 0.008, "slope {slope}");
}

#[test]
fn exp_c_stays_linear_across_the_kobol_s_vco_range() {
    // ab.md, drive linearity (RES 7¼: k 2.32, FREQ 1017 Hz, VCO1 ¼ / ½ /
    // full at −17.27 / −10.02 / 0 dB): EXP-C's worst shape change 0.01 /
    // 0.07 dB (the Kobol's 0.15 / 0.37), H1 ¼→full +17.27 dB.
    let f0 = 56.4;
    let tab = Wave::saw().table(f0).0;
    let n = (1.5 * FS) as usize;
    let x = play(&tab, &|_| f0, n);
    let hs: Vec<Vec<f64>> = DRIVE
        .iter()
        .map(|(_, d)| {
            let a = 10f64.powf(d / 20.0);
            let xa: Vec<f64> = x.iter().map(|v| v * a).collect();
            let y = unit(&xa, &|_| 1017.0, &|_| 2.32);
            harmonics(seg(&y, 0.5, 1.5), f0, (FMAX / f0) as usize)
        })
        .collect();
    let (_, _, _, w1, _) = steps(&hs[0], &hs[1]);
    let (d2, _, _, w2, _) = steps(&hs[0], &hs[2]);
    eprintln!("drive: worst ½ {w1:.3} full {w2:.3}, H1 ¼→full {d2:+.3}");
    assert!(w1 < 0.05 && w2 < 0.15, "{w1} {w2}");
    assert!((d2 - 17.27).abs() < 0.03, "{d2}");
}

#[test]
fn exp_c_hard_drive_bends_as_the_lab_measured() {
    // notes.md: hard drive (220 Hz, 12 V at the mixer, +13 dB over full
    // VCO1, FREQ 600 Hz, RES 0): EXP-C's H3 −26.8 dBc (the circuit's
    // −34.9 at OUT 4: a known gap). 6 V (d6): EXP-C still clean of H2.
    let y = expc_probe(Offsets::REFERENCE, 220.0, 12.0, 600.0, 0.0, 0.3);
    let (_, d) = hreport(half(&y), 220.0);
    let y6 = expc_probe(Offsets::REFERENCE, 220.0, 6.0, 600.0, 0.0, 0.3);
    let (_, d6) = hreport(half(&y6), 220.0);
    eprintln!("d12: {d:?}; d6: {d6:?}");
    assert!((d[1] + 26.8).abs() < 1.5, "{d:?}");
}

#[test]
fn shipped_exp_keeps_the_bass_exp_c_loses_it() {
    // ADR 0191 / 2040-bass.md: a full-scale A1 saw at FREQ 9.4 kHz, LEVEL
    // ½, the fundamental re RES 0: the firmware's EXP −3.3 dB at RES 0.4
    // and −7.0 at RES 1. EXP-C at the same RES (k = 5.59·RES) keeps the
    // circuit's 1/(1+k): −10.2 dB at 0.4 (ab.md's sweeps: −9.8).
    let f0 = 55.0;
    let n = (1.0 * FS) as usize;
    let x = play(&Wave::saw().table(f0).0, &|_| f0, n);
    let h = |y: Vec<f64>| tone_at(seg(&y, 0.5, 1.0), f0);
    let ex: Vec<f64> = [0.0, 0.4, 1.0]
        .iter()
        .map(|&r| h(exp(&x, &|_| 9400.0, &|_| r)))
        .collect();
    let ec: Vec<f64> = [0.0, 0.4]
        .iter()
        .map(|&r| h(unit(&x, &|_| 9400.0, &|_| super::k_of(r))))
        .collect();
    let (e4, e1, c4) = (db(ex[1] / ex[0]), db(ex[2] / ex[0]), db(ec[1] / ec[0]));
    eprintln!("bass: EXP {e4:+.2} / {e1:+.2}, EXP-C {c4:+.2}");
    assert!(
        (e4 + 3.3).abs() < 0.3 && (e1 + 7.0).abs() < 0.3,
        "{e4} {e1}"
    );
    assert!((c4 + 10.2).abs() < 0.6, "{c4}");
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn exp_c_and_shipped_exp_meet_on_the_drive_series() {
    // ab.md: EXP-C vs today's EXP, third-octave LSD on drive_q / _h / _f
    // (RMS-matched): 2.28 / 0.97 / 0.27 dB.
    let _ = SLOW;
    let f0 = 56.4;
    let tab = Wave::saw().table(f0).0;
    let n = (8.0 * FS) as usize;
    let x = play(&tab, &|_| f0, n);
    for (i, (t, d)) in DRIVE.iter().enumerate() {
        let a = 10f64.powf(d / 20.0);
        let xa: Vec<f64> = x.iter().map(|v| v * a).collect();
        let c = unit(&xa, &|_| 1017.0, &|_| 2.32);
        let e = exp(&xa, &|_| 1017.0, &|_| 0.525);
        let (gc, ge) = (1.0 / loud_rms(&c), 1.0 / loud_rms(&e));
        let c: Vec<f64> = c.iter().map(|v| v * gc).collect();
        let e: Vec<f64> = e.iter().map(|v| v * ge).collect();
        let l = lsd(&c, &e);
        eprintln!("{t}: LSD {l:.2}, null {:+.1}", null_db(&c, &e));
        let want = [2.28, 0.97, 0.27][i];
        assert!((l - want).abs() < 0.5, "{t}: {l}");
    }
}

// ------------------------------------------------------------ ngspice

#[test]
#[ignore = "needs ngspice: NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored"]
fn exp_c_rings_like_the_circuit() {
    // The lab's u15 probe (CI5 1.5 mV, k 5.6, FREQ 1 kHz, a 10 mV kick):
    // the circuit rings at 1015.2 Hz, OUT 4 H1 0.392 V, H2 −38.6 dBc and
    // H3 −46.9 at the output. EXP-C (notes.md): H2 within 1–2 dB, H3 short
    // (ab.md, track2: circuit −47.1, EXP-C −54.2).
    let (y, o4) = circuit_probe(&Kobol::UNIT, "u15", 0.0, 0.0, 1000.0, 5.6, 1.0);
    let (f, _, d) = ring(&y);
    let a4 = tone_at(half(&o4), f);
    let e = ring(&expc_probe(Offsets::UNIT, 0.0, 0.0, 1000.0, 5.6, 1.0));
    eprintln!(
        "circuit {f:.2} Hz OUT4 {a4:.4} {d:?}; EXP-C {:.2} Hz {:?}",
        e.0, e.2
    );
    assert!((f - 1015.21).abs() < 1.0, "{f}");
    assert!((a4 - 0.3922).abs() < 0.005, "{a4}");
    assert!(
        (d[0] + 38.6).abs() < 0.5 && (d[1] + 46.9).abs() < 0.5,
        "{d:?}"
    );
    assert!((e.2[0] - d[0]).abs() < 2.5, "H2 {:?} vs {d:?}", e.2);
    assert!(e.2[1] < d[1], "EXP-C's H3 is short of the circuit's");
}

#[test]
#[ignore = "needs ngspice: NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored"]
fn circuit_loses_more_than_one_over_one_plus_k() {
    // notes.md, "Circuit bass loss" (b0/b2/b5: 56.4 Hz, 1.75 V, FREQ 1
    // kHz): OUT 4's H1 0.1737 / 0.0528 / 0.0202 V, −10.3 / −18.7 dB; EXP-C
    // within 0.6 / 1.5 dB.
    let h = |k: f64, tag: &str| {
        let (y, o4) = circuit_probe(&Kobol::UNIT, tag, 56.4, 1.75, 1000.0, k, 1.5);
        (tone_at(half(&o4), 56.4), tone_at(half(&y), 56.4))
    };
    let (b0, b2, b5) = (h(0.0, "b0"), h(2.13, "b2"), h(5.6, "b5"));
    eprintln!("OUT4 {:.4} {:.4} {:.4}", b0.0, b2.0, b5.0);
    let (l2, l5) = (db(b2.0 / b0.0), db(b5.0 / b0.0));
    assert!((b0.0 - 0.1737).abs() < 0.003, "{}", b0.0);
    assert!(
        (l2 + 10.3).abs() < 0.2 && (l5 + 18.7).abs() < 0.3,
        "{l2} {l5}"
    );
    let e = |k: f64| {
        tone_at(
            half(&expc_probe(Offsets::UNIT, 56.4, 1.75, 1000.0, k, 1.5)),
            56.4,
        )
    };
    let (e0, e2, e5) = (e(0.0), e(2.13), e(5.6));
    // On one scale: EXP-C's output × E_FULL is OUT 4 (b0: 0.1711 against
    // 0.1737 V). The circuit's CI2A output reads 0.5 dB over
    // `out_gain()`·OUT 4 (3.838 V against 3.614), so `exp_c_scale` is
    // nominal; the A/B level-matched anyway.
    let e4 = e0 * f64::from(E_FULL);
    eprintln!(
        "EXP-C {:+.2} / {:+.2}; OUT 4 {e4:.4} ({:+.2} dB); CI2A over out_gain·OUT 4 {:+.2} dB",
        db(e2 / e0),
        db(e5 / e0),
        db(e4 / b0.0),
        db(b0.1 * f64::from(E_FULL) / b0.0)
    );
    assert!((db(e2 / e0) - l2).abs() < 0.6 && (db(e5 / e0) - l5).abs() < 1.5);
    assert!(db(e4 / b0.0).abs() < 0.25, "OUT 4's scale");
}

#[test]
#[ignore = "needs ngspice: NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored"]
fn exp_c_tracks_the_circuit_on_a_saw() {
    // ab.md, drive_f (VCO1 full, k 2.32, FREQ 1017 Hz): EXP-C vs circuit
    // third-octave LSD 0.38 dB, null −12.1 dB (RMS-matched); and the small
    // probe ss (110 Hz, 1 V, FREQ 1 kHz, RES 0): OUT 4 H1 0.0975 V, EXP-C
    // 0.0973.
    let f0 = 56.4;
    let n = (1.5 * FS) as usize;
    let x = play(&Wave::saw().table(f0).0, &|_| f0, n);
    let c = Kobol::UNIT;
    let o = circuit::spice(
        &c,
        &circuit::Case {
            x: x.iter().map(|v| v * V_FULL).collect(),
            fc: vec![1017.0; n],
            k: vec![2.32; n],
        },
        &spice_dir("drive_f"),
    )
    .expect("ngspice");
    let s = c.exp_c_scale();
    let ci: Vec<f64> = o.y.iter().map(|v| v * s).collect();
    let e = unit(&x, &|_| 1017.0, &|_| 2.32);
    let (ci, e) = (seg(&ci, 0.5, 1.5), seg(&e, 0.5, 1.5));
    let (gc, ge) = (1.0 / loud_rms(ci), 1.0 / loud_rms(e));
    let ci: Vec<f64> = ci.iter().map(|v| v * gc).collect();
    let e: Vec<f64> = e.iter().map(|v| v * ge).collect();
    let (l, nl) = (lsd(&e, &ci), null_db(&e, &ci));
    eprintln!(
        "drive_f: LSD {l:.2} null {nl:+.1}; level {:+.2} dB",
        db(gc / ge)
    );
    assert!(l < 0.38 + 0.3, "{l}");
    assert!(nl < -12.1 + 3.0, "{nl}");
    let (_, o4) = circuit_probe(&Kobol::SCHEMATIC, "ss", 110.0, 1.0, 1000.0, 0.0, 0.3);
    let a4 = tone_at(half(&o4), 110.0);
    let a = tone_at(
        half(&expc_probe(
            Offsets::REFERENCE,
            110.0,
            1.0,
            1000.0,
            0.0,
            0.3,
        )),
        110.0,
    ) * f64::from(E_FULL);
    eprintln!("ss: circuit {a4:.4} EXP-C {a:.4}");
    assert!((a4 - 0.0975).abs() < 0.001 && db(a / a4).abs() < 0.1);
}

#[test]
#[ignore = "needs ngspice: NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored"]
fn exp_c_swing_fits_the_cell_s_dc_curve() {
    // fw.rs / research.md § 4: the swing, least squares on the reference
    // cell's DC curve, "9–11 mV rms" at FREQ 100 Hz / 1 kHz / 10 kHz. That
    // holds over ±1.5 V of unlimited OUT with the cell's own resting offset
    // (−20 mV) taken out: 10.0 / 10.0 / 11.1 mV (the lab's curves.json and
    // this netlist agree to the µV); over ±3 V it is 12.4 / 11.6 / 11.0.
    use super::fw::{sides, swing};
    for fc in [100.0, 1000.0, 10_000.0] {
        let curve = circuit::dc_cell(&Kobol::SCHEMATIC, fc, &spice_dir(&format!("dccell_{fc}")))
            .expect("ngspice");
        let y0 = curve
            .iter()
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
            .map_or(0.0, |p| p.1);
        let (up, down) = sides(fc as f32);
        let d: Vec<f64> = curve
            .iter()
            .filter(|(x, _)| x.abs() <= 1.5)
            .map(|&(x, y)| f64::from(swing(x as f32, up, down)) - (y - y0))
            .collect();
        let e = rms(&d);
        eprintln!(
            "dc cell {fc}: rest {:.1} mV, fit {:.1} mV rms",
            y0 * 1e3,
            e * 1e3
        );
        assert!(e < 0.0115, "{fc}: {e}");
    }
}

// ------------------------------------------------------------ the takes

#[test]
#[ignore = "needs the owner's takes: KOBOL_DIR=…/kobol-session cargo test -p chimera-circuits --release -- --ignored"]
fn the_kobol_rings_as_the_lab_read_it() {
    // ab.md, track2: 15 notes, level spread 0.7 dB, H2 −39.3 dBc, H3 −43.6.
    let Some(d) = takes_dir() else { return };
    let k = load(&d, "track2").expect("track2");
    let (n, spread, h2, h3) = osc_summary(&osc_notes(&k));
    eprintln!("kobol: {n} notes, spread {spread:.2}, H2 {h2:.2}, H3 {h3:.2}");
    assert_eq!(n, 15);
    assert!((spread - 0.7).abs() < 0.06 && (h2 + 39.3).abs() < 0.06 && (h3 + 43.6).abs() < 0.06);
}

#[test]
#[ignore = "needs the owner's takes: KOBOL_DIR=…/kobol-session cargo test -p chimera-circuits --release -- --ignored"]
fn exp_c_on_track2_rings_as_in_the_ab() {
    // ab.md, track2 through EXP-C (unit offsets, each note tuned to the
    // Kobol's pitch): 15 notes, spread 1.9 dB, H2 −37.7, H3 −54.2, pitch
    // 0.0 c; the firmware's EXP: spread 0.1, H2 −150.6, H3 −54.6.
    let Some(d) = takes_dir() else { return };
    let k = load(&d, "track2").expect("track2");
    let (job, notes) = selfosc_job(&k);
    let tuned = |ratio: &dyn Fn(f64) -> f64, m: &Tuned| {
        let t = tune_to(&notes, ratio);
        let y = m(&job.x, &|i| job.fc[i] * t[note_index(&notes, i)]);
        let y: Vec<f64> = y.iter().zip(&job.vca).map(|(a, b)| a * b).collect();
        osc_notes(&y)
    };
    let ec = tuned(&|fc| ring_ratio(&unit, fc, 5.59), &|x, fc| {
        unit(x, fc, &|_| 5.59)
    });
    let ex = tuned(&|fc| ring_ratio(&exp, fc, 1.0), &|x, fc| {
        exp(x, fc, &|_| 1.0)
    });
    let (n, s, h2, h3) = osc_summary(&ec);
    let (nx, sx, h2x, h3x) = osc_summary(&ex);
    let pitch = notes
        .iter()
        .zip(&ec)
        .map(|(a, b)| cents(b.2, a.2).abs())
        .sum::<f64>()
        / 15.0;
    eprintln!(
        "EXP-C {n} {s:.2} {h2:.2} {h3:.2} pitch {pitch:.2} c; EXP {nx} {sx:.2} {h2x:.1} {h3x:.1}"
    );
    assert_eq!((n, nx), (15, 15));
    assert!((s - 1.9).abs() < 0.3 && (h2 + 37.7).abs() < 0.5 && (h3 + 54.2).abs() < 0.5);
    assert!(pitch < 1.0);
    assert!(sx < 0.4 && h2x < -100.0 && (h3x + 54.6).abs() < 0.5);
}

#[test]
#[ignore = "needs the owner's takes: KOBOL_DIR=…/kobol-session cargo test -p chimera-circuits --release -- --ignored"]
fn the_kobol_and_exp_c_stay_linear_on_the_drive_takes() {
    // compare-drive.md / ab.md: the Kobol ¼→½ +7.25 dB, ¼→full +17.27,
    // worst shape change 0.15 / 0.37 dB; EXP-C on the A/B's jobs 0.01 /
    // 0.07; EXP-C vs the firmware's EXP LSD 2.28 / 0.97 / 0.27.
    let Some(d) = takes_dir() else { return };
    let takes: Vec<Vec<f64>> = DRIVE
        .iter()
        .map(|t| load(&d, t.0).expect("drive take"))
        .collect();
    let hs = |ys: &[Vec<f64>]| -> Vec<Vec<f64>> {
        ys.iter()
            .map(|y| {
                let s = seg(y, 2.0, 8.0);
                let f0 = f0_near(s, 56.4, 4);
                harmonics(s, f0, (FMAX / f0) as usize)
            })
            .collect()
    };
    let hk = hs(&takes);
    let (s1, _, _, w1, _) = steps(&hk[0], &hk[1]);
    let (s2, _, _, w2, _) = steps(&hk[0], &hk[2]);
    eprintln!("kobol: +{s1:.2} +{s2:.2}, worst {w1:.2} {w2:.2}");
    assert!((s1 - 7.25).abs() < 0.02 && (s2 - 17.27).abs() < 0.02);
    assert!((w1 - 0.15).abs() < 0.02 && (w2 - 0.37).abs() < 0.02);
    let jobs: Vec<Job> = DRIVE
        .iter()
        .zip(&takes)
        .map(|(t, k)| drive_job(t.0, k).expect("job"))
        .collect();
    let ec: Vec<Vec<f64>> = jobs
        .iter()
        .map(|j| unit(&j.x, &|i| j.fc[i], &|i| j.k[i]))
        .collect();
    let he = hs(&ec);
    let (_, _, _, e1, _) = steps(&he[0], &he[1]);
    let (_, _, _, e2, _) = steps(&he[0], &he[2]);
    eprintln!("EXP-C worst {e1:.3} {e2:.3}");
    assert!(e1 < 0.03 && e2 < 0.1);
    // LSD against the firmware's EXP: one gain per source, from drive_f.
    let ex: Vec<Vec<f64>> = jobs
        .iter()
        .map(|j| exp(&j.x, &|i| j.fc[i], &|_| j.res_exp))
        .collect();
    let (gc, ge) = (1.0 / loud_rms(&ec[2]), 1.0 / loud_rms(&ex[2]));
    for (i, want) in [2.28, 0.97, 0.27].iter().enumerate() {
        let c: Vec<f64> = ec[i].iter().map(|v| v * gc).collect();
        let e: Vec<f64> = ex[i].iter().map(|v| v * ge).collect();
        let l = lsd(&c, &e);
        eprintln!("{}: EXP-C vs EXP LSD {l:.2}", DRIVE[i].0);
        assert!((l - want).abs() < 0.1, "{l}");
    }
}

#[test]
#[ignore = "needs the owner's takes: KOBOL_DIR=…/kobol-session cargo test -p chimera-circuits --release -- --ignored"]
fn the_sweeps_bass_step_is_flat_and_exp_c_keeps_the_circuit_s() {
    // notes.md: sweep_res5 and sweep_res10 sit −3.27 / −6.81 dB under
    // sweep1, flat ±0.1 dB from H1 to H31 (a gain change, not a filter);
    // ab.md, the sweeps' bass (H1 over the last 2 s re sweep1's): Kobol
    // −3.3 / −6.9, EXP-C −9.8 / −15.8, the firmware's EXP −4.2 / −7.1.
    let Some(d) = takes_dir() else { return };
    let s1 = load(&d, "sweep1").expect("sweep1");
    let plateau = seg(&s1, 12.0, 19.0);
    let f0 = f0_near(plateau, 56.4, 8);
    let aref = harmonics(plateau, f0, 31);
    for (take, want) in [("sweep_res5", -3.27), ("sweep_res10", -6.81)] {
        let x = load(&d, take).expect("take");
        // The open end, as the A/B's bass row reads it: the last 3.5–1.5 s.
        let t1 = x.len() as f64 / FS;
        let s = seg(&x, t1 - 3.5, t1 - 1.5);
        let r = masked(&harmonics(s, f0_near(s, 56.4, 4), 31), &aref);
        let v: Vec<f64> = r.iter().copied().filter(|v| v.is_finite()).collect();
        let m = v.iter().sum::<f64>() / v.len() as f64;
        let dev = v.iter().fold(0.0f64, |a, x| a.max((x - m).abs()));
        eprintln!(
            "{take}: mean {m:+.2} dB, max dev {dev:.2} over {} harmonics",
            v.len()
        );
        // sweep_res5 reproduces −3.27 ±0.04; sweep_res10's last 2 s read
        // −6.95 ±0.17 (the ledger's −6.81 came from a window it did not
        // record; ab.md's bass row on this window: −6.9).
        assert!((m - want).abs() < 0.2 && dev < 0.25, "{m} {dev}");
    }
    let h1 = |y: &[f64]| {
        let n = y.len();
        let s = &y[n - (3.5 * FS) as usize..n - (1.5 * FS) as usize];
        tone_at(s, f0_near(s, 56.4, 4))
    };
    let mut rows = Vec::new();
    for take in ["sweep1", "sweep_res5", "sweep_res10"] {
        let k = load(&d, take).expect("take");
        let j = sweep_job(take, &k).expect("job");
        let ec = unit(&j.x, &|i| j.fc[i], &|i| j.k[i]);
        let ex = exp(&j.x, &|i| j.fc[i], &|_| j.res_exp);
        rows.push((h1(&k), h1(&ec), h1(&ex)));
    }
    for (i, want) in [(1, [-3.3, -9.8, -4.2]), (2, [-6.9, -15.8, -7.1])] {
        let got = [
            db(rows[i].0 / rows[0].0),
            db(rows[i].1 / rows[0].1),
            db(rows[i].2 / rows[0].2),
        ];
        eprintln!("bass {i}: {got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 0.15, "{g} vs {w}");
        }
    }
}

#[test]
#[ignore = "needs the owner's takes: KOBOL_DIR=…/kobol-session cargo test -p chimera-circuits --release -- --ignored"]
fn exp_c_on_the_phrases_misses_the_kobol_as_much_as_the_rest() {
    // ab.md, phrases: spectral-centroid error vs the Kobol (octaves rms),
    // EXP-C 0.37 / 0.39 / 1.89 / 0.42, the firmware's EXP 0.38 / 0.39 /
    // 1.86 / 0.42: the plain saw and the fitted patches dominate.
    let Some(d) = takes_dir() else { return };
    for (take, we, wx) in [
        ("phrase_bass_vca8", 0.37, 0.38),
        ("phrase_bass", 0.39, 0.39),
        ("phrase_acid_vca8", 1.89, 1.86),
        ("phrase_acid", 0.42, 0.42),
    ] {
        let k = load(&d, take).expect("take");
        let j = phrase_job(take, &k).expect("job");
        let kt = ltrack(&k);
        let vca = |y: Vec<f64>| -> Vec<f64> { y.iter().zip(&j.vca).map(|(a, b)| a * b).collect() };
        let ec = vca(unit(&j.x, &|i| j.fc[i], &|i| j.k[i]));
        let ex = vca(exp(&j.x, &|i| j.fc[i], &|_| j.res_exp));
        let (a, b) = (
            centroid_err(&kt, &ltrack(&ec)),
            centroid_err(&kt, &ltrack(&ex)),
        );
        eprintln!("{take}: EXP-C {a:.2} EXP {b:.2}");
        assert!((a - we).abs() < 0.05 && (b - wx).abs() < 0.05);
    }
}
