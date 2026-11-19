//! The per-Part EQ's filters (ADR 0284): TILT about 810 Hz, LO CUT at
//! 150 Hz, both one-pole, gliding, and a true bypass at flat.

use chimera_engine::hw::MAX_PARTS;
use chimera_engine::instrument::Sides;
use chimera_engine::part_eq::dsp::{EqBlock, LO_CUT, PIVOT, run};
use chimera_engine::part_eq::{LoCut, PartEq, PartEqs, Tilt};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;
use core::f32::consts::PI;

const FS: u32 = 48_000;
type Buses = [[f32; BLOCK_SIZE]; MAX_PARTS];

/// Plays `blocks` blocks of `x(n)` on Part 0 (and its side, if `side`)
/// under `eq(block)`, every Part written; Part 0's output.
fn play(
    eqs: &mut PartEqs,
    blocks: usize,
    x: impl Fn(usize) -> f32,
    eq: impl Fn(usize) -> PartEq,
) -> Vec<f32> {
    let mut out = Vec::new();
    for b in 0..blocks {
        let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
        for (i, s) in buses[0].iter_mut().enumerate() {
            *s = x(b * BLOCK_SIZE + i);
        }
        let mut sides: Sides = [None; MAX_PARTS];
        let mut settings = [PartEq::FLAT; MAX_PARTS];
        settings[0] = eq(b);
        run(
            eqs,
            EqBlock {
                buses: &mut buses,
                sides: &mut sides,
                written: &[true; MAX_PARTS],
                settings: &settings,
                sample_rate: FS,
            },
        );
        out.extend_from_slice(&buses[0]);
    }
    out
}

fn sine(hz: f32) -> impl Fn(usize) -> f32 {
    move |n| (2.0 * PI * hz * n as f32 / FS as f32).sin()
}

/// RMS over the last `n` samples, in dB against a unit sine's.
fn level_db(y: &[f32], n: usize) -> f32 {
    let t = &y[y.len() - n..];
    let rms = (t.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
    20.0 * (rms * 2f32.sqrt()).log10()
}

/// The TPT one-pole low-pass's exact response at `hz`, gain `g`
/// (`tan(π fc / fs)` warped): `g (1 + z⁻¹) / ((1 + g) + (g − 1) z⁻¹)`.
fn lp(hz: f64, g: f64) -> (f64, f64) {
    let w = 2.0 * std::f64::consts::PI * hz / f64::from(FS);
    let (zr, zi) = (w.cos(), -w.sin());
    let (nr, ni) = (g * (1.0 + zr), g * zi);
    let (dr, di) = ((1.0 + g) + (g - 1.0) * zr, (g - 1.0) * zi);
    let d = dr * dr + di * di;
    ((nr * dr + ni * di) / d, (ni * dr - nr * di) / d)
}

fn warp(hz: f32) -> f64 {
    (std::f64::consts::PI * f64::from(hz) / f64::from(FS)).tan()
}

/// TILT's exact response at `hz`, dB: `k − (k − 1/k)·LP_{g = k·tan(π·810/fs)}`.
fn tilt_db(db: f32, hz: f64) -> f64 {
    let k = 10f64.powf(f64::from(db) / 20.0);
    let (r, i) = lp(hz, k * warp(PIVOT.get()));
    let c = k - 1.0 / k;
    let (yr, yi) = (k - c * r, -c * i);
    10.0 * (yr * yr + yi * yi).log10()
}

/// LO CUT's exact response at `hz`, dB: `1 − LP_{150}`.
fn cut_db(hz: f64) -> f64 {
    let (r, i) = lp(hz, warp(LO_CUT.get()));
    let (yr, yi) = (1.0 - r, -i);
    10.0 * (yr * yr + yi * yi).log10()
}

#[test]
fn flat_and_off_never_touch_a_bus() {
    let mut eqs = PartEqs::REST;
    let x = |n: usize| ((n * 7919) % 1000) as f32 / 500.0 - 1.0;
    let y = play(&mut eqs, 20, x, |_| PartEq::FLAT);
    for (n, v) in y.iter().enumerate() {
        assert_eq!(v.to_bits(), x(n).to_bits(), "sample {n}");
    }
    assert!(!eqs.any_live());
}

#[test]
fn the_rest_state_is_zero_bytes() {
    // SAFETY: `PartEqs` is plain floats (some in newtypes), bools,
    // `PhantomData` and a `repr(u8)` enum whose zero is a variant
    // (`Bypass`); every all-zero value is one.
    let zeroed: PartEqs = unsafe { core::mem::zeroed() };
    assert_eq!(zeroed, PartEqs::REST);
}

#[test]
fn the_shape_is_the_one_pole_tilt_pivoting_at_810_hz() {
    for step in [Tilt::MIN_STEP, -20, 32, Tilt::MAX_STEP] {
        let t = Tilt::at(step);
        for hz in [30.0f32, 100.0, 810.0, 3_000.0, 12_000.0] {
            let mut eqs = PartEqs::REST;
            let e = PartEq::new(t, LoCut::Off);
            // 0.4 s: the 20 ms ease long settled.
            let y = play(&mut eqs, 300, sine(hz), |_| e);
            let want = tilt_db(t.db().get(), f64::from(hz)) as f32;
            let got = level_db(&y, 9_600);
            assert!(
                (got - want).abs() < 0.05,
                "step {step} at {hz} Hz: {got:.3} dB, want {want:.3}"
            );
        }
    }
    // The model's own landmarks: unity at the pivot, the shelves at ±dB.
    assert!(tilt_db(10.0, 810.0).abs() < 1e-9);
    assert!((tilt_db(-10.0, 0.001) - 10.0).abs() < 1e-3);
    assert!((tilt_db(10.0, 0.001) + 10.0).abs() < 1e-3);
}

#[test]
fn lo_cut_is_a_150_hz_one_pole_high_pass() {
    let on = PartEq::new(Tilt::FLAT, LoCut::On);
    for hz in [37.5f32, 75.0, 150.0, 600.0, 5_000.0] {
        let mut eqs = PartEqs::REST;
        let y = play(&mut eqs, 300, sine(hz), |_| on);
        let want = cut_db(f64::from(hz)) as f32;
        let got = level_db(&y, 9_600);
        assert!(
            (got - want).abs() < 0.05,
            "{hz} Hz: {got:.3} dB, want {want:.3}"
        );
    }
    assert!((cut_db(150.0) + 3.0103).abs() < 1e-3, "−3 dB at the corner");
    // −6 dB an octave, well below it.
    assert!((cut_db(2.0) - cut_db(4.0) + 6.02).abs() < 0.01);
}

/// The largest sample-to-sample move of `y[from..to]`.
fn max_step(y: &[f32], from: usize, to: usize) -> f32 {
    y[from..to]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_full_tilt_turn_glides_without_a_step() {
    // Dark to bright in one turn, on a 200 Hz tone: every move inside the
    // glide stays within the tone's own steady moves either side of it.
    let mut eqs = PartEqs::REST;
    let turn = 40;
    let y = play(&mut eqs, 120, sine(200.0), |b| {
        let t = if b < turn {
            Tilt::MIN_STEP
        } else {
            Tilt::MAX_STEP
        };
        PartEq::new(Tilt::at(t), LoCut::Off)
    });
    let at = turn * BLOCK_SIZE;
    let before = max_step(&y, at - 960, at);
    let after = max_step(&y, y.len() - 960, y.len());
    let during = max_step(&y, at - 1, at + 2_400);
    assert!(
        during <= 1.01 * before.max(after),
        "{during} against {before} and {after}"
    );
    // And it took more than a block: not a snap.
    let first = max_step(&y, at, at + BLOCK_SIZE);
    assert!(first < 0.5 * (after - before).abs() + before, "{first}");
}

#[test]
fn lo_cut_switches_without_a_click() {
    // On, then off, under a 60 Hz tone: no move larger than the tone's own.
    for (from, to) in [(LoCut::Off, LoCut::On), (LoCut::On, LoCut::Off)] {
        let mut eqs = PartEqs::REST;
        let y = play(&mut eqs, 120, sine(60.0), |b| {
            PartEq::new(Tilt::FLAT, if b < 40 { from } else { to })
        });
        let at = 40 * BLOCK_SIZE;
        let steady = max_step(&y, at - 2_400, at).max(max_step(&y, y.len() - 2_400, y.len()));
        let during = max_step(&y, at - 1, at + 2_400);
        assert!(
            during <= 1.01 * steady,
            "{from:?}→{to:?}: {during} against {steady}"
        );
    }
}

#[test]
fn back_at_flat_the_eq_settles_into_bypass() {
    let mut eqs = PartEqs::REST;
    let x = sine(440.0);
    let bright = PartEq::new(Tilt::at(40), LoCut::On);
    let y = play(&mut eqs, 400, &x, |b| {
        if b < 20 { bright } else { PartEq::FLAT }
    });
    assert!(!eqs.any_live(), "settled at flat, off again");
    // The last blocks are the input itself, bit for bit.
    for (i, v) in y.iter().enumerate().skip(y.len() - BLOCK_SIZE) {
        assert_eq!(v.to_bits(), x(i).to_bits(), "sample {i}");
    }
}

#[test]
fn a_part_with_nothing_written_lands_its_eq_unheard() {
    let mut eqs = PartEqs::REST;
    let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
    let mut sides: Sides = [None; MAX_PARTS];
    let mut settings = [PartEq::FLAT; MAX_PARTS];
    settings[2] = PartEq::new(Tilt::at(-30), LoCut::On);
    let mut written = [false; MAX_PARTS];
    run(
        &mut eqs,
        EqBlock {
            buses: &mut buses,
            sides: &mut sides,
            written: &written,
            settings: &settings,
            sample_rate: FS,
        },
    );
    assert!(
        buses.iter().flatten().all(|&s| s == 0.0),
        "a silent bus stays silent"
    );
    // Written next, it plays the setting at once: landed, not gliding.
    written[2] = true;
    let mut buses: Buses = [[1.0; BLOCK_SIZE]; MAX_PARTS];
    let mut once = eqs;
    run(
        &mut once,
        EqBlock {
            buses: &mut buses,
            sides: &mut sides,
            written: &written,
            settings: &settings,
            sample_rate: FS,
        },
    );
    let mut cold = PartEqs::REST;
    let y = play(&mut cold, 1, |_| 0.0, |_| settings[2]);
    assert!(y.iter().all(|&s| s == 0.0));
    assert!(once.running(PartId::ALL[2]));
}

#[test]
fn a_side_is_filtered_as_its_mid() {
    // A DRUM pad's side (mid ± side → L, R) takes the same filter.
    let mut eqs = PartEqs::REST;
    let e = PartEq::new(Tilt::at(25), LoCut::On);
    let x = sine(300.0);
    for b in 0..30 {
        let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
        let mut side = [0.0; BLOCK_SIZE];
        for i in 0..BLOCK_SIZE {
            buses[0][i] = x(b * BLOCK_SIZE + i);
            side[i] = 0.5 * x(b * BLOCK_SIZE + i);
        }
        let mut sides: Sides = [None; MAX_PARTS];
        sides[0] = Some(side);
        let mut settings = [PartEq::FLAT; MAX_PARTS];
        settings[0] = e;
        run(
            &mut eqs,
            EqBlock {
                buses: &mut buses,
                sides: &mut sides,
                written: &[true; MAX_PARTS],
                settings: &settings,
                sample_rate: FS,
            },
        );
        let s = sides[0].expect("kept");
        for i in 0..BLOCK_SIZE {
            assert!(
                (s[i] - 0.5 * buses[0][i]).abs() < 1e-6,
                "block {b} sample {i}"
            );
        }
    }
}

/// One bad sample (NaN, or inf held on) on Part 0's bus and side at
/// block 100 of 300; the blocks after it whose output is not finite.
fn bad_blocks_after(bad: f32, held: bool) -> usize {
    let mut eqs = PartEqs::REST;
    let e = PartEq::new(Tilt::at(20), LoCut::On);
    let x = sine(300.0);
    let mut bad_blocks = 0;
    for b in 0..300 {
        let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
        let mut side = [0.0; BLOCK_SIZE];
        for i in 0..BLOCK_SIZE {
            buses[0][i] = x(b * BLOCK_SIZE + i);
            side[i] = 0.5 * buses[0][i];
        }
        let poisoned = b == 100 || (held && b > 100 && b < 103);
        if poisoned {
            buses[0][7] = bad;
            side[7] = bad;
        }
        let mut sides: Sides = [None; MAX_PARTS];
        sides[0] = Some(side);
        let mut settings = [PartEq::FLAT; MAX_PARTS];
        settings[0] = e;
        run(
            &mut eqs,
            EqBlock {
                buses: &mut buses,
                sides: &mut sides,
                written: &[true; MAX_PARTS],
                settings: &settings,
                sample_rate: FS,
            },
        );
        let s = sides[0].expect("kept");
        let finite = buses[0].iter().chain(s.iter()).all(|v| v.is_finite());
        if b > 100 && !poisoned && !finite {
            bad_blocks += 1;
        }
    }
    bad_blocks
}

#[test]
fn a_nan_or_inf_does_not_poison_the_eq() {
    // As CLEAN's filter (`filter::clean`): a block whose taps end up not
    // finite starts the next one from rest.
    assert_eq!(bad_blocks_after(f32::NAN, false), 0, "one NaN");
    assert_eq!(
        bad_blocks_after(f32::INFINITY, true),
        0,
        "inf held three blocks"
    );
    assert_eq!(bad_blocks_after(f32::NEG_INFINITY, false), 0, "one -inf");
}

/// Host timing (release, `--ignored --nocapture`): six Parts EQ'd, steady
/// and with TILT turned every block, ns a block. The unit's figure is the
/// bench's ROUTING rows EQ and EQ MOVE.
#[test]
#[ignore]
fn time_six_parts() {
    use std::hint::black_box;
    use std::time::Instant;
    for moving in [false, true] {
        let mut eqs = PartEqs::REST;
        let mut buses: Buses = [[0.25; BLOCK_SIZE]; MAX_PARTS];
        let mut sides: Sides = [None; MAX_PARTS];
        let n = 200_000;
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            for b in 0..n {
                let s = if moving { (b % 128) as i8 - 64 } else { 63 };
                let settings = [PartEq::new(Tilt::at(s), LoCut::On); MAX_PARTS];
                run(
                    &mut eqs,
                    EqBlock {
                        buses: black_box(&mut buses),
                        sides: &mut sides,
                        written: &[true; MAX_PARTS],
                        settings: &settings,
                        sample_rate: FS,
                    },
                );
            }
            best = best.min(t.elapsed().as_nanos() as f64 / n as f64);
        }
        let block_ns = BLOCK_SIZE as f64 * 1e9 / f64::from(FS);
        println!(
            "six Parts, {}: {best:.0} ns a block, {:.3} % of the block's {block_ns:.0} ns",
            if moving { "TILT moving" } else { "steady" },
            100.0 * best / block_ns
        );
    }
}
