// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::util::harness::{cutoff_sweep, probe, res_sweep, tail};
use crate::util::measure::{db, lsd, null_db, tone};
use chimera_core::dsp::filter::{Filter, FilterKind, FilterMods, K2040Voice};
use chimera_core::params::FilterParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::path::Path;

fn model(c: &Case) -> Vec<f64> {
    ac(&pad_run(c, |c| Ok(Model::new(Circuit::PROPHET, 2, 8).render(c))).unwrap())
}

fn fast(c: &Case, os: usize, sh: Shaper) -> Vec<f64> {
    ac(&pad_run(c, |c| {
        let mut f = Fast::new(&Circuit::PROPHET, os);
        f.shaper = sh;
        Ok(f.render(c))
    })
    .unwrap())
}

/// Today's 2040 REV2 through the shipped `Filter`, LEVEL ½ (its default),
/// params read once a block: the lab's `chimera_kind`.
fn rev2(case: &Case) -> Vec<f64> {
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
    ac(&y.into_iter().map(f64::from).collect::<Vec<_>>())
}

/// Gain at `f0` (dB) of a sine probe at `level_db`.
fn gain(r: impl Fn(&Case) -> Vec<f64>, f0: f64, fc: f64, res: f64, level_db: f64) -> f64 {
    let y = r(&probe(f0, fc, res, level_db, true));
    db(tone(tail(&y), f0)) - level_db
}

/// Compression re small-signal and H2, H3 (dBc) of a 220 Hz sine at
/// `level_db`, FREQ 2 kHz, RES 0: the drive probe of `drive.md`.
fn drive(r: impl Fn(&Case) -> Vec<f64>, level_db: f64) -> (f64, f64, f64) {
    let ss = gain(&r, 220.0, 2000.0, 0.0, -18.0);
    let y = r(&probe(220.0, 2000.0, 0.0, level_db, true));
    let t = tail(&y);
    let h1 = tone(t, 220.0);
    (
        db(h1) - level_db - ss,
        db(tone(t, 440.0) / h1),
        db(tone(t, 660.0) / h1),
    )
}

fn small_signal(name: &str, r: impl Fn(&Case) -> Vec<f64>) {
    // Unity in the passband (less the 15.9 Hz coupling), four poles at
    // −3 dB at FREQ, and the uncompensated bass loss 1/(1 + k) at RES 0.85:
    // −13.1 dB on every version (ab-notes.md).
    let dc = gain(&r, 200.0, 8000.0, 0.0, -18.0);
    let at = gain(&r, 1000.0, 1000.0, 0.0, -18.0);
    let bass = gain(&r, 110.0, 2000.0, 0.85, -18.0);
    assert!(dc.abs() < 0.1, "{name}: passband {dc:.2} dB");
    assert!((at + 12.0).abs() < 0.3, "{name}: {at:.2} dB at FREQ");
    let want = -20.0 * (1.0 + k_of(0.85)).log10();
    assert!(
        (bass - want).abs() < 0.2,
        "{name}: bass {bass:.2} dB at RES 0.85, want {want:.2}"
    );
}

#[test]
fn the_candidates_are_the_circuits_linear_filter() {
    small_signal("d", |c| fast(c, 2, Shaper::Pade));
    small_signal("e", |c| fast(c, 1, Shaper::Quintic));
}

#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn the_model_is_the_circuits_linear_filter() {
    small_signal("model", model);
}

/// `drive.md`, the circuit (ngspice): −0.75 dB at +6, −5.5 at +12; H2
/// −38 dBc at +6, −45 at +12, from the cells' offsets. The model measures
/// −0.51 / −5.28 and −38.3 / −44.8.
#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn the_model_compresses_early_and_lopsided_as_the_circuit() {
    let (c6, h2_6, _) = drive(model, 6.0);
    let (c12, h2_12, _) = drive(model, 12.0);
    assert!((-1.0..=-0.3).contains(&c6), "{c6:.2} dB at +6");
    assert!((-6.0..=-5.0).contains(&c12), "{c12:.2} dB at +12");
    assert!((-42.0..=-34.0).contains(&h2_6), "H2 {h2_6:.1} dBc at +6");
    assert!((-49.0..=-41.0).contains(&h2_12), "H2 {h2_12:.1} dBc at +12");
    // The even harmonics are the offsets': a chip without them has none.
    let even = |c: &Case| {
        let ch = Circuit {
            vos: [0.0; 4],
            ..Circuit::PROPHET
        };
        ac(&pad_run(c, |c| Ok(Model::new(ch, 2, 8).render(c))).unwrap())
    };
    let (_, h2, _) = drive(even, 6.0);
    assert!(h2 < -90.0, "H2 {h2:.1} dBc with no offsets");
}

/// Both candidates keep the circuit's push: within 0.5 dB of the model's
/// compression and with its even harmonics (`d` −5.51, `e` −5.45 dB at
/// +12; H2 −37.6 / −37.3 dBc at +6).
#[test]
fn the_candidates_compress_as_the_circuit() {
    for (name, r) in [
        (
            "d",
            &(|c: &Case| fast(c, 2, Shaper::Pade)) as &dyn Fn(&Case) -> Vec<f64>,
        ),
        ("e", &|c: &Case| fast(c, 1, Shaper::Quintic)),
    ] {
        let (c6, h2, _) = drive(r, 6.0);
        let (c12, _, _) = drive(r, 12.0);
        assert!((-1.0..=-0.3).contains(&c6), "{name}: {c6:.2} dB at +6");
        assert!((-6.0..=-5.0).contains(&c12), "{name}: {c12:.2} dB at +12");
        assert!(
            (-42.0..=-34.0).contains(&h2),
            "{name}: H2 {h2:.1} dBc at +6"
        );
    }
}

/// Today's REV2, the KIND that shipped in the circuit's place: the same
/// linear filter, but perfectly odd and clipping later (`drive.md`:
/// −4.5 dB at +12 where the circuit has −5.5, and no H2).
#[test]
fn todays_rev2_is_the_linear_filter_without_the_push() {
    small_signal("rev2", rev2);
    let (c6, h2, _) = drive(rev2, 6.0);
    let (c12, _, _) = drive(rev2, 12.0);
    assert!(c6 > -0.2, "{c6:.2} dB at +6");
    assert!((-5.0..=-3.5).contains(&c12), "{c12:.2} dB at +12");
    assert!(h2 < -150.0, "H2 {h2:.1} dBc");
}

/// The candidates against the model over the cutoff sweeps (RES 0, ½,
/// 0.85) and the RES sweep: third-octave LSD. `ab-notes.md` has d 0.16,
/// e 0.35 and REV2 0.65 mean against ngspice over every job; on these
/// four, d 0.02–0.22, e 0.10–0.18, REV2 0.13–0.43.
#[test]
#[ignore = "slow: cargo test -p chimera-circuits --release -- --ignored"]
fn the_candidates_track_the_model_on_sweeps() {
    let cases: Vec<Case> = [0.0, 0.5, 0.85]
        .map(cutoff_sweep)
        .into_iter()
        .chain([res_sweep()])
        .collect();
    let refs: Vec<Vec<f64>> = cases.iter().map(model).collect();
    for (name, r, bound) in [
        (
            "d",
            &(|c: &Case| fast(c, 2, Shaper::Pade)) as &dyn Fn(&Case) -> Vec<f64>,
            0.3,
        ),
        ("e", &|c: &Case| fast(c, 1, Shaper::Quintic), 0.3),
    ] {
        for (c, m) in cases.iter().zip(&refs) {
            let l = lsd(&r(c), m);
            assert!(l < bound, "{name}: LSD {l:.2} dB");
        }
    }
    // At RES 0.85 the cheap candidate beats today's REV2 (0.18 vs 0.43).
    let (e, today) = (
        lsd(&fast(&cases[2], 1, Shaper::Quintic), &refs[2]),
        lsd(&rev2(&cases[2]), &refs[2]),
    );
    assert!(e < today, "e {e:.2} vs REV2 {today:.2}");
}

/// The model against ngspice on the drive probes: `ab-notes.md` has the
/// model within about 0.1 dB of third-octave level, nulls of −40 to −53 dB
/// on tonal stimuli and −17 to −31 dB at RES 0.85, where the peak's few
/// cents of tuning dominate (measured here: −46, −45 and −19 dB).
#[test]
#[ignore = "needs ngspice: NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored"]
fn the_model_tracks_ngspice() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-out/c2040-spice");
    for (tag, case, null) in [
        ("sine_r00_+0", probe(220.0, 2000.0, 0.0, 0.0, true), -40.0),
        ("sine_r00_+12", probe(220.0, 2000.0, 0.0, 12.0, true), -40.0),
        ("saw_r85_+0", probe(110.0, 1000.0, 0.85, 0.0, false), -15.0),
    ] {
        let b = ac(&pad_run(&case, |c| {
            reference::spice(&Circuit::PROPHET, c, &dir.join(tag))
        })
        .unwrap());
        let c = model(&case);
        let (l, n) = (lsd(&c, &b), null_db(&c, &b));
        println!("{tag}: LSD {l:.2} null {n:.1}");
        assert!(l < 0.3, "{tag}: LSD {l:.2} dB");
        assert!(n < null, "{tag}: null {n:.1} dB");
    }
}
