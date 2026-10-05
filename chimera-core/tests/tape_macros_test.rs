//! TAPE's TAPE and AGE macros (spec § 2): each voice's own transport, wow
//! and flutter as a speed deviation in cents (ADR 0053's `Transport`), and
//! the wear: a falling one-pole, hiss and dropouts. Precedent: a Mellotron
//! gives each note its own strip of tape.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::age::{Age, age_runs};
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::stretch::WRAP_SAMPLES;
use chimera_core::dsp::tape_engine::{
    INC_MAX, MIN_PASS_SAMPLES, NOTE_ON_RAMP, TapeEngine, TapeEngineParams, TapeModel,
};
use chimera_hal::BLOCK_SIZE;
use common::XorShift;
use common::tape::{held_plain, held_transwave, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;

const MODELS: [TapeModel; 6] = [
    TapeModel::Loop,
    TapeModel::Reverse,
    TapeModel::PingPong,
    TapeModel::Scrub,
    TapeModel::Transwave,
    TapeModel::Stretch,
];

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    *TapeEngine::init_in_place(&mut slot)
}

/// A long 1 kHz sine at 48 kHz (no wrap inside `secs`), or TRANSWAVE's
/// one-frame sine.
fn source(model: TapeModel, secs: f32) -> Vec<u8> {
    if model == TapeModel::Transwave {
        held_transwave(512, 1, |_, h| if h == 1 { 0.5 } else { 0.0 })
    } else {
        held_plain(&sine(48_000, 48.0, (secs * 48_000.0) as u32), 48_000)
    }
}

/// `model` at its own speed: SCRUB's vari-speed at 1×, STRETCH's time at
/// 1×, locked to the sine's period.
fn params(model: TapeModel) -> TapeEngineParams {
    TapeEngineParams {
        model,
        motion: match model {
            TapeModel::Scrub | TapeModel::Stretch => 0.5,
            _ => 0.0,
        },
        period: 48.0,
        ..TapeEngineParams::default()
    }
}

/// The note each model plays its sine at: 1 kHz on a source, 440 Hz on a
/// transwave.
fn note_hz(model: TapeModel) -> (u8, f64) {
    if model == TapeModel::Transwave {
        (69, 440.0)
    } else {
        (60, 1_000.0)
    }
}

/// `blocks` blocks of `e`, each block's params from `p(block)`.
fn play(
    e: &mut TapeEngine,
    v: &SourceView<'_>,
    blocks: usize,
    p: impl Fn(usize) -> TapeEngineParams,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(blocks * B);
    let mut b = [0.0; B];
    for i in 0..blocks {
        e.render(&mut b, &p(i), v, SR);
        out.extend_from_slice(&b);
    }
    out
}

/// Each period's pitch against `f0`, cents, at its middle (seconds): the
/// upward zero crossings, linearly interpolated.
fn deviation(x: &[f32], f0: f64) -> Vec<(f64, f64)> {
    let ups: Vec<f64> = x
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
        .map(|(i, w)| i as f64 + w[0] as f64 / (w[0] - w[1]) as f64)
        .collect();
    ups.windows(2)
        .map(|c| {
            let period = c[1] - c[0];
            let t = 0.5 * (c[0] + c[1]) / SR as f64;
            (t, 1_200.0 * (SR as f64 / f0 / period).log2())
        })
        .collect()
}

/// `curve` at `t`, linearly (`t` inside it).
fn at(curve: &[(f64, f64)], t: f64) -> f64 {
    let i = curve
        .partition_point(|&(s, _)| s < t)
        .clamp(1, curve.len() - 1);
    let ((t0, a), (t1, b)) = (curve[i - 1], curve[i]);
    a + (b - a) * (t - t0) / (t1 - t0)
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma) * (x - ma)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb) * (y - mb)).sum();
    cov / (va * vb).sqrt()
}

#[test]
fn tape_zero_adds_exact_zeros() {
    for model in MODELS {
        let held = source(model, 2.5);
        let v = SourceView::strict(&held).unwrap();
        let (note, _) = note_hz(model);
        // MOTION and a SIZE LFO keep every path busy.
        let p = |i: usize| TapeEngineParams {
            size: 0.3 + 0.2 * (i as f32 * 0.05).sin(),
            ..params(model)
        };
        let (mut a, mut b) = (fresh(), fresh());
        a.seed(3);
        b.seed(3);
        b.without_transport();
        a.note_on(note, &p(0), &v, SR);
        b.note_on(note, &p(0), &v, SR);
        let (ya, yb) = (play(&mut a, &v, 1_500, p), play(&mut b, &v, 1_500, p));
        for (i, (s, t)) in ya.iter().zip(&yb).enumerate() {
            assert_eq!(
                s.to_bits(),
                t.to_bits(),
                "{model:?}: sample {i}: {s} against {t}"
            );
        }
    }
}

#[test]
fn wow_depth_bounds() {
    for model in MODELS {
        let held = source(model, 4.5);
        let v = SourceView::strict(&held).unwrap();
        let (note, f0) = note_hz(model);
        let p = TapeEngineParams {
            tape: 1.0,
            ..params(model)
        };
        let mut e = fresh();
        e.seed(1);
        e.note_on(note, &p, &v, SR);
        let y = play(&mut e, &v, 4 * SR as usize / B, |_| p);
        let dev = deviation(&y[NOTE_ON_RAMP as usize..], f0);
        let peak = dev.iter().fold(0.0f64, |m, &(_, c)| m.max(c.abs()));
        assert!(peak <= 33.0, "{model:?}: {peak} cents");
        assert!(peak >= 20.0, "{model:?}: {peak} cents");
        // The wow, ±25 cents at 0.5 Hz, and the capstan, half the ±8 of
        // flutter at 9 Hz: each its own line in the deviation's spectrum.
        let (wow, capstan) = (line(&dev, 0.5), line(&dev, 9.0));
        assert!((wow - 25.0).abs() <= 2.0, "{model:?}: wow {wow} cents");
        assert!(
            (capstan - 4.0).abs() <= 1.0,
            "{model:?}: capstan {capstan} cents"
        );
    }
}

/// The amplitude of `curve`'s `hz` line over its whole periods (4 s here),
/// on a 1 ms grid.
fn line(curve: &[(f64, f64)], hz: f64) -> f64 {
    let (t0, t1) = (curve[0].0, curve[curve.len() - 1].0);
    let n = (((t1 - t0) * hz).floor() / hz * 1_000.0) as usize;
    let (re, im) = (0..n).fold((0.0, 0.0), |(re, im), k| {
        let t = t0 + k as f64 * 1e-3;
        let (w, c) = (core::f64::consts::TAU * hz * t, at(curve, t));
        (re + c * w.cos(), im + c * w.sin())
    });
    2.0 * (re * re + im * im).sqrt() / n as f64
}

#[test]
fn voices_have_own_transport() {
    let model = TapeModel::Loop;
    let held = source(model, 4.5);
    let v = SourceView::strict(&held).unwrap();
    let (note, f0) = note_hz(model);
    let p = TapeEngineParams {
        tape: 1.0,
        ..params(model)
    };
    let curve = |s: u32| {
        let mut e = fresh();
        e.seed(s);
        e.note_on(note, &p, &v, SR);
        let y = play(&mut e, &v, 4 * SR as usize / B, |_| p);
        let dev = deviation(&y[NOTE_ON_RAMP as usize..], f0);
        (1..399)
            .map(|k| at(&dev, k as f64 * 0.01))
            .collect::<Vec<f64>>()
    };
    let (a, b) = (curve(1), curve(2));
    // Each moves: a transport, not a flat line.
    for c in [&a, &b] {
        assert!(c.iter().fold(0.0f64, |m, x| m.max(x.abs())) > 10.0);
    }
    let r = correlation(&a, &b);
    assert!(r < 0.5, "seeds 1 and 2 correlate {r}");
}

#[test]
fn age_zero_is_bypassed() {
    assert!(!age_runs(0.0, false));
    assert!(age_runs(0.0, true));
    assert!(age_runs(0.01, false));
    let mut rng = XorShift(0xA6E0_0001);
    let mut age = Age::new();
    age.seed(5);
    for _ in 0..200 {
        let x: [f32; B] = core::array::from_fn(|_| rng.next_u32() as i32 as f32 / i32::MAX as f32);
        let mut y = x;
        age.process(&mut y, 0.0);
        assert!(x.iter().zip(&y).all(|(a, b)| a.to_bits() == b.to_bits()));
    }
}

/// AGE eased down to 0, unrouted, stops: the wear's bypass, so the output
/// comes back bit-identical to a note never worn (its bill drops with it).
#[test]
fn age_stops_once_eased_to_zero() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = |age: f32| TapeEngineParams {
        age,
        ..TapeEngineParams::default()
    };
    let (mut a, mut b) = (fresh(), fresh());
    a.seed(6);
    b.seed(6);
    a.note_on(60, &p(0.5), &v, SR);
    b.note_on(60, &p(0.0), &v, SR);
    let ya = play(&mut a, &v, 600, |i| p(if i < 100 { 0.5 } else { 0.0 }));
    let yb = play(&mut b, &v, 600, |_| p(0.0));
    assert!(ya[..100 * B] != yb[..100 * B], "AGE ½ wears");
    // 0.7ⁿ under 1e-3 by 20 blocks; the filter's state is gone in a few.
    let from = 200 * B;
    assert!(
        ya[from..]
            .iter()
            .zip(&yb[from..])
            .all(|(x, y)| x.to_bits() == y.to_bits())
    );
}

/// `hold()` before a fresh note on an idle voice: the wear starts from
/// silence, as the note-on ramp does, not from the last note's sample.
#[test]
fn fresh_note_after_hold_starts_from_silence() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        age: 0.4,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.seed(8);
    e.note_on(60, &p, &v, SR);
    let y = play(&mut e, &v, 100, |_| p);
    assert!(y[y.len() - 1].abs() > 0.2, "the old note ends loud");
    e.hold();
    e.note_on(60, &p, &v, SR);
    let y = play(&mut e, &v, 1, |_| p);
    // START's sample is 0; the hiss at AGE 0.4 peaks under 0.006.
    assert!(y[0].abs() < 0.006, "{}", y[0]);
}

/// `f` Hz at 0.5 through a fresh `Age` seeded 9 at `age`, its level over
/// the last 2 s of 3.
fn level_at(f: f32, age: f32) -> f64 {
    let mut a = Age::new();
    a.seed(9);
    let n = 3 * SR as usize;
    let mut out = Vec::with_capacity(n);
    for blk in 0..n / B {
        let mut x: [f32; B] = core::array::from_fn(|i| {
            let t = (blk * B + i) as f64 / SR as f64;
            (0.5 * (core::f64::consts::TAU * f as f64 * t).sin()) as f32
        });
        a.process(&mut x, age);
        out.extend_from_slice(&x);
    }
    // The output's projection on `f`, in f64 (an f32 Goertzel drifts over
    // 2 s).
    let (re, im) = out[SR as usize..]
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (i, &s)| {
            let w = core::f64::consts::TAU * f as f64 * i as f64 / SR as f64;
            (re + s as f64 * w.cos(), im + s as f64 * w.sin())
        });
    (re * re + im * im).sqrt()
}

#[test]
fn age_corner_falls() {
    // The −3 dB point at AGE 1, against 100 Hz: the dropouts fall on both
    // runs alike (seeded, independent of the input), so they cancel.
    let low = level_at(100.0, 1.0);
    let db = |f: f32| 20.0 * (level_at(f, 1.0) / low).log10();
    let (mut lo, mut hi) = (1_000.0f32, 6_000.0f32);
    assert!(db(lo) > -3.0 && db(hi) < -3.0);
    for _ in 0..20 {
        let mid = 0.5 * (lo + hi);
        if db(mid) > -3.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    assert!((lo - 2_500.0).abs() <= 250.0, "−3 dB at {lo} Hz");
}

#[test]
fn hiss_floor() {
    // Through the engine: a silent source at AGE 1.
    let held = held_plain(&vec![0.0; 48_000], 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        age: 1.0,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.seed(4);
    e.note_on(60, &p, &v, SR);
    let y = play(&mut e, &v, 2 * SR as usize / B, |_| p);
    let db = 20.0 * (common::rms(&y[SR as usize / 2..]) as f64).log10();
    assert!((db + 42.0).abs() <= 1.0, "{db} dBFS");
    // Each voice its own hiss: seeded 4 and 5, uncorrelated.
    let mut f = fresh();
    f.seed(5);
    f.note_on(60, &p, &v, SR);
    let z = play(&mut f, &v, 2 * SR as usize / B, |_| p);
    let (a, b): (Vec<f64>, Vec<f64>) = y
        .iter()
        .zip(&z)
        .map(|(&a, &b)| (a as f64, b as f64))
        .unzip();
    let r = correlation(&a, &b);
    assert!(r.abs() < 0.05, "seeds 4 and 5: hiss correlates {r}");
}

/// A full-scale 750 Hz cosine (one period a block, its peak on a block's
/// edge) through `Age` seeded `seed` at `age` for `secs`: the output, and
/// each block's level in dB, its 750 Hz bin.
fn worn(age: f32, secs: usize, seed: u32) -> (Vec<f32>, Vec<f64>) {
    let mut a = Age::new();
    a.seed(seed);
    let mut out = Vec::new();
    let levels = (0..secs * SR as usize / B)
        .map(|_| {
            let w = |i: usize| core::f64::consts::TAU * i as f64 / B as f64;
            let mut x: [f32; B] = core::array::from_fn(|i| w(i).cos() as f32);
            a.process(&mut x, age);
            out.extend_from_slice(&x);
            let (re, im) = x.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, &s)| {
                (re + s as f64 * w(i).cos(), im + s as f64 * w(i).sin())
            });
            10.0 * (re * re + im * im).log10()
        })
        .collect();
    (out, levels)
}

fn median(x: &[f64]) -> f64 {
    let mut sorted = x.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

/// Each dip below the gain's half-way to its deepest (−3.4 dB at 9 dB) in
/// a full-scale 750 Hz sine (one period a block) through `Age` at `age`,
/// `secs` long: `(depth dB, length s)`. Each block's level is its 750 Hz
/// bin; a dip's depth, against the median block, is its deepest four
/// blocks' mean power, so the hiss moves it by under 0.02 dB (σ).
fn dips(age: f32, secs: usize, seed: u32) -> Vec<(f64, f64)> {
    let (_, levels) = worn(age, secs, seed);
    let median = median(&levels);
    let half = 20.0 * ((1.0 + 10f64.powf(-9.0 / 20.0)) / 2.0).log10();
    let four = |i: usize| {
        let p: f64 = levels[i..i + 4].iter().map(|l| 10f64.powf(l / 10.0)).sum();
        median - 10.0 * (p / 4.0).log10()
    };
    let mut out = Vec::new();
    let mut from: Option<usize> = None;
    for (i, l) in levels.iter().enumerate() {
        match (from, median - l > -half) {
            (None, true) => from = Some(i),
            (Some(f), false) => {
                let deepest = (f..i.saturating_sub(3).max(f + 1))
                    .map(four)
                    .fold(0.0, f64::max);
                out.push((deepest, (i - f) as f64 * B as f64 / SR as f64));
                from = None;
            }
            _ => {}
        }
    }
    out
}

#[test]
fn dropouts_only_above_half() {
    assert!(
        dips(0.5, 10, 11).is_empty(),
        "AGE 0.5: {:?}",
        dips(0.5, 10, 11)
    );
    let d = dips(1.0, 60, 12);
    // One each 2 s on average: 30 in a minute.
    assert!((18..=42).contains(&d.len()), "{} dips in 60 s", d.len());
    let deepest = d.iter().fold(0.0f64, |m, &(db, _)| m.max(db));
    assert!((8.0..=9.0 + 0.1).contains(&deepest), "deepest {deepest} dB");
    for &(db, len) in &d {
        // A block either side for where a dip is read.
        assert!(
            (0.020 - 0.0014..=0.080 + 0.0014).contains(&len),
            "{db} dB for {len} s"
        );
    }
    // Each eases a block at a time, in and out (EASE 0.3 a block): about
    // four blocks between 2 and 7 dB down on the way in, three on the way
    // out. A dip taken in one block's ramp would have one each way.
    let (out, levels) = worn(1.0, 60, 12);
    let m = median(&levels);
    let between = levels
        .iter()
        .filter(|&&l| (2.0..7.0).contains(&(m - l)))
        .count();
    assert!(
        between >= 5 * d.len(),
        "{between} easing blocks for {} dips",
        d.len()
    );
    // And within each block the gain ramps: at the blocks' edges, where
    // the cosine is flat, no step past the click rule's 4× AGE ½'s.
    let edges = |x: &[f32]| {
        (B..x.len())
            .step_by(B)
            .map(|i| (x[i] - x[i - 1]).abs())
            .fold(0.0f32, f32::max)
    };
    let (steady, _) = worn(0.5, 60, 12);
    let (s, w) = (edges(&steady), edges(&out));
    assert!(
        w <= 4.0 * s,
        "dropouts step {w} at a block's edge, against {s}"
    );
    // Half-way up the dropouts' range: half the depth, half the rate.
    let d = dips(0.75, 60, 13);
    assert!((7..=24).contains(&d.len()), "{} dips at AGE 0.75", d.len());
    for &(db, _) in &d {
        assert!(db <= 4.5 + 0.1, "AGE 0.75: {db} dB");
    }
}

/// TAPE 1 on the highest note: the wow rides inside `inc`'s clamp, so a
/// head never passes `INC_MAX` frames a sample (a block's move on a
/// source long enough for a whole wow cycle, 2 s at `inc` 16), and each model's floors take the block's fastest `inc`, wow
/// included: on the shortest window, 33 cents under the clamp so the wow
/// can lift it, the events stay within the floors' bounds.
#[test]
fn full_tape_keeps_the_clamp_and_floors() {
    let long = held_plain(&sine(48_000, 100.7, 1_600_000), 48_000);
    let v = SourceView::strict(&long).unwrap();
    let p = TapeEngineParams {
        tape: 1.0,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.seed(1);
    e.set_pitch(4_800.0);
    e.note_on(127, &p, &v, SR);
    let mut b = [0.0; B];
    let mut at = e.probe().head;
    for _ in 0..1_550 {
        e.render(&mut b, &p, &v, SR);
        let now = e.probe().head;
        assert!(
            now - at <= INC_MAX as u32 * B as u32,
            "{} frames a block",
            now - at
        );
        at = now;
    }

    let short = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&short).unwrap();
    // The note's `inc`: 16 · 2^(−33/1200), by PITCH from note 127.
    let pitch = 1_200.0 * INC_MAX.log2() - 33.0 - (12_700.0 - 6_000.0);
    for model in [
        TapeModel::Loop,
        TapeModel::Reverse,
        TapeModel::PingPong,
        TapeModel::Scrub,
        TapeModel::Stretch,
    ] {
        let p = TapeEngineParams {
            model,
            start: 0.5,
            end: 0.5 + 64.0 / 48_000.0,
            size: 0.0,
            xfade: 0.0,
            // SCRUB at its own speed (MOTION ½ is 1×); STRETCH's time 2×.
            motion: if model == TapeModel::Scrub { 0.5 } else { 1.0 },
            tape: 1.0,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.seed(2);
        e.set_pitch(pitch);
        e.note_on(127, &p, &v, SR);
        play(&mut e, &v, 4, |_| p);
        let before = e.events();
        let blocks = 1_500;
        let y = play(&mut e, &v, blocks, |_| p);
        let per = (e.events() - before) as f32 / blocks as f32;
        let most = if model == TapeModel::Stretch {
            1 + 2 * B / WRAP_SAMPLES as usize
        } else {
            B / MIN_PASS_SAMPLES as usize + 1
        };
        assert!(
            per <= most as f32,
            "{model:?}: {per} events a block, at most {most}"
        );
        assert!(y.iter().all(|s| s.is_finite()));
    }
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

/// `p` with one macro set: POSITION, MOTION, TAPE, AGE or SIZE.
fn with(p: TapeEngineParams, which: usize, x: f32) -> TapeEngineParams {
    let mut q = p;
    match which {
        0 => q.position = x,
        1 => q.motion = x,
        2 => q.tape = x,
        3 => q.age = x,
        _ => q.size = x,
    }
    q
}

#[test]
fn sweeps_are_click_free() {
    let wave = held_transwave(512, 4, |f, h| match h {
        1 => 0.5,
        h if h == f as u32 + 2 => 0.2,
        _ => 0.0,
    });
    let plain = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    for model in MODELS {
        let held = if model == TapeModel::Transwave {
            &wave
        } else {
            &plain
        };
        let v = SourceView::strict(held).unwrap();
        // SCRUB's finger at its slowest DRAG: a fast one scrubs the tape up
        // to `INC_MAX`, pitch by design (`scrub_finger_is_bounded`).
        let base = TapeEngineParams {
            drag: 1.0,
            period: 100.7,
            size: 0.5,
            ..params(model)
        };
        for which in 0..5 {
            let run = |x: &dyn Fn(usize) -> f32| {
                let mut e = fresh();
                e.seed(1);
                e.note_on(57, &with(base, which, x(0)), &v, SR);
                play(&mut e, &v, 330, |i| with(base, which, x(i)))
            };
            let steady = [0.0, 1.0]
                .map(|c| max_step(&run(&|_| c)[2 * B..]))
                .into_iter()
                .fold(0.0, f32::max);
            // Swept 0 → 1 → 0 in 100 ms (75 blocks), then stepped 0 ↔ 1.
            let x = |i: usize| match i {
                40..=77 => (i - 40) as f32 / 37.5,
                78..=115 => (115 - i) as f32 / 37.5,
                200..260 => 1.0,
                _ => 0.0,
            };
            let worst = max_step(&run(&x)[2 * B..]);
            assert!(
                worst <= 4.0 * steady,
                "{model:?} macro {which}: {worst} against {steady}"
            );
        }
    }
}
