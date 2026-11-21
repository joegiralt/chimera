//! The delay's tempo sync (ADR 0302): FREE as it always played, bit for
//! bit; under SYNC each note on its time at any tempo, folded onto the
//! 500 ms line by halves and never off the grid; GRAIN on the same note;
//! a small tempo change gliding TAPE's head within 2 % of its speed,
//! anything else crossfading; and what a project stores.

use chimera_core::addr::{BlockRead, BlockRef, BlockView, BlockViewMut, Blocks};
use chimera_core::block::{Block, find_spec};
use chimera_core::dsp::delay::sync::{GLIDE_MAX_S, GLIDE_SPEED, NOTES};
use chimera_core::dsp::delay::{
    DELAY_SPECS, DelayNote, DelayParams, DelaySync, DelayType, TIME_FADE, TapeDelay,
};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::rev::sync::{DivKind, FoldState, Tempo};
use chimera_core::storage::{RecordBuf, decode_block, encode_block};
use chimera_hal::BLOCK_SIZE;

const FS: u32 = 48_000;

/// The place of `1/den` of `kind` in the notes.
fn note(den: u8, kind: DivKind) -> DelayNote {
    let i = NOTES
        .iter()
        .position(|d| d.den == den && d.kind == kind)
        .expect("in the list");
    DelayNote::new(i)
}

/// `p` under SYNC on `n`.
fn on(p: DelayParams, n: DelayNote) -> DelayParams {
    p.with(DelayParams::SYNC, DelaySync::Sync.value())
        .with(DelayParams::NOTE, n.value())
}

/// One clean repeat: no feedback, no MECHANICS, the return at unity.
fn clean() -> DelayParams {
    DelayParams {
        feedback: 0.0,
        wow_flutter: 0.0,
        saturation: 0.0,
        tone: 1.0,
        mix: 1.0,
        ..DelayParams::DEFAULT
    }
}

fn grain(p: DelayParams) -> DelayParams {
    p.with(DelayParams::TYPE, DelayType::Grain.value())
}

/// `blocks` of the send's return over `input`, each block at `at`'s
/// settings and tempo.
fn play(
    d: &mut TapeDelay,
    blocks: usize,
    input: impl Fn(usize) -> [f32; BLOCK_SIZE],
    at: impl Fn(usize) -> (DelayParams, f32),
) -> Vec<f32> {
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    for b in 0..blocks {
        let mut buf = input(b);
        let (p, bpm) = at(b);
        d.play(&mut buf, &p, Tempo::from_bpm(bpm), FS);
        out.extend_from_slice(&buf);
    }
    out
}

fn click(b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| if b == 0 && i == 0 { 1.0 } else { 0.0 })
}

fn sine(hz: f32, amp: f32) -> impl Fn(usize) -> [f32; BLOCK_SIZE] {
    move |b| {
        core::array::from_fn(|i| {
            let n = (b * BLOCK_SIZE + i) as f64;
            (f64::from(amp) * (core::f64::consts::TAU * f64::from(hz) * n / f64::from(FS)).sin())
                as f32
        })
    }
}

/// Plucks every 0.3 s over a quiet noise bed (`delay_tape_hash_test`'s).
fn plucks(b: usize) -> [f32; BLOCK_SIZE] {
    let mut rng = 0x1234_5678u32 ^ (b as u32).wrapping_mul(0x9E37_79B9);
    core::array::from_fn(|i| {
        let t = ((b * BLOCK_SIZE + i) % 14_400) as f32;
        let pluck = 0.8 * (t * 0.031).sin() * (-t / 2400.0).exp();
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        pluck + (rng as f32 / u32::MAX as f32 - 0.5) * 0.05
    })
}

fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|s| s.to_bits()).collect()
}

fn blocks(secs: f32) -> usize {
    (secs * FS as f32) as usize / BLOCK_SIZE
}

/// The largest |x[n] − 2x[n−1] + x[n−2]|: a step or a click stands far
/// out of a sine's.
fn max_d2(x: &[f32]) -> f32 {
    x.windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f32::max)
}

/// Each period's frequency, from `x`'s rising zero crossings placed
/// between their two samples.
fn periods_hz(x: &[f32]) -> Vec<f64> {
    let mut at = Vec::new();
    for (i, w) in x.windows(2).enumerate() {
        if w[0] < 0.0 && w[1] >= 0.0 {
            at.push(i as f64 + f64::from(-w[0] / (w[1] - w[0])));
        }
    }
    at.windows(2)
        .map(|w| f64::from(FS) / (w[1] - w[0]))
        .collect()
}

/// A note's length at `bpm` halved onto the line from rest, in samples,
/// with its halvings: the sum done in `f64`, apart from the engine's.
fn fitted(n: DelayNote, bpm: f64) -> (f64, i8) {
    let d = NOTES[n.index()];
    let k = match d.kind {
        DivKind::Straight => 1.0,
        DivKind::Triplet => 2.0 / 3.0,
        DivKind::Dotted => 1.5,
    };
    let mut ms = 240_000.0 / bpm * f64::from(d.num) / f64::from(d.den) * k;
    let mut halved = 0;
    while ms > 500.001 {
        ms /= 2.0;
        halved += 1;
    }
    (ms * f64::from(FS) / 1000.0, halved)
}

// ---------------------------------------------------------------------------
// FREE
// ---------------------------------------------------------------------------

/// The settings `delay_tape_hash_test` moves through, TIME twice.
fn moving(b: usize) -> DelayParams {
    let mut p = DelayParams {
        time_ms: 375.0,
        feedback: 0.7,
        wow_flutter: 0.3,
        saturation: 0.4,
        tone: 0.5,
        mix: 0.8,
        ..DelayParams::DEFAULT
    };
    if b >= 400 {
        p.time_ms = 120.0;
    }
    if b >= 700 {
        (p.feedback, p.wow_flutter) = (0.95, 0.9);
    }
    if b >= 1000 {
        (p.time_ms, p.saturation, p.tone) = (499.0, 0.0, 1.0);
    }
    p
}

/// FREE never reads the tempo or the note: at any of either, TAPE and
/// GRAIN play what `process_wet` plays (whose hashes
/// `delay_tape_hash_test` and `fx_golden_test` hold), bit for bit.
#[test]
fn free_plays_the_same_at_any_tempo_and_note() {
    for ty in [DelayType::Tape, DelayType::Grain] {
        let at = |b: usize| moving(b).with(DelayParams::TYPE, ty.value());
        let mut d = Box::new(TapeDelay::new());
        let mut want = Vec::new();
        for b in 0..1200 {
            let mut buf = plucks(b);
            d.process_wet(&mut buf, &at(b), FS);
            want.extend_from_slice(&buf);
        }
        for (bpm, n) in [(30.0, 0), (90.0, 10), (174.25, 4), (1_200.0, 7)] {
            let mut d = Box::new(TapeDelay::new());
            let got = play(&mut d, 1200, plucks, |b| {
                // The tempo wanders too.
                let p = at(b).with(DelayParams::NOTE, DelayNote::new(n).value());
                (p, bpm + (b % 7) as f32)
            });
            assert!(bits(&got) == bits(&want), "{ty:?} at {bpm} BPM, note {n}");
            assert_eq!(d.fold(), FoldState(0));
        }
    }
}

// ---------------------------------------------------------------------------
// The note's time
// ---------------------------------------------------------------------------

/// A click comes back a note later, at 60, 90, 120, 140 and 174 BPM, for
/// every note in the list: on the sample where the note is whole samples,
/// else between the two either side of it. A note longer than the line
/// plays halved, and `fold` says how often.
#[test]
fn each_note_repeats_on_its_time() {
    for bpm in [60.0, 90.0, 120.0, 140.0, 174.0] {
        for (i, div) in NOTES.iter().enumerate() {
            let n = DelayNote::new(i);
            let (want, halved) = fitted(n, bpm);
            let mut d = Box::new(TapeDelay::new());
            let out = play(&mut d, blocks(0.6), click, |_| (on(clean(), n), bpm as f32));
            let peak = (0..out.len())
                .max_by(|&a, &b| out[a].abs().total_cmp(&out[b].abs()))
                .unwrap();
            let what = format!("{div:?} at {bpm} BPM: {want} samples");
            assert!((peak as f64 - want).abs() < 1.0, "{what}, peak at {peak}");
            // Linear interpolation: the click, whole, across two samples.
            let sum: f32 = out.iter().sum();
            assert!((sum - 1.0).abs() < 1e-3, "{what}, sum {sum}");
            assert_eq!(d.fold(), FoldState(halved), "{what}");
        }
    }
}

/// The design's two: 1/8D at 120 BPM is 18,000 samples, and 1/4 at 90 BPM,
/// 667 ms, plays halved at 16,000.
#[test]
fn a_dotted_eighth_at_120_and_a_halved_quarter_at_90() {
    for (n, bpm, at, fold) in [
        (DelayNote::DEFAULT, 120.0, 18_000, 0),
        (note(4, DivKind::Straight), 90.0, 16_000, 1),
        (note(4, DivKind::Straight), 120.0, 24_000, 0),
    ] {
        let mut d = Box::new(TapeDelay::new());
        let out = play(&mut d, blocks(0.6), click, |_| (on(clean(), n), bpm));
        assert!((out[at] - 1.0).abs() < 1e-3, "{n:?} at {bpm}: {}", out[at]);
        assert_eq!(d.fold(), FoldState(fold));
    }
}

/// SYNC on a note is FREE at that note's ms, bit for bit, TAPE and GRAIN,
/// with feedback and MECHANICS running: SYNC changes where TIME comes
/// from and nothing else. 1/8D at 120 BPM is the default 375 ms; 1/4 at
/// 90 BPM the halved 333.3 ms.
#[test]
fn sync_is_free_at_the_notes_ms() {
    let quarter = note(4, DivKind::Straight);
    for ty in [DelayType::Tape, DelayType::Grain] {
        for (n, bpm, ms) in [
            (DelayNote::DEFAULT, 120.0, 375.0),
            (quarter, 90.0, NOTES[quarter.index()].ms(Tempo(90.0)) / 2.0),
        ] {
            let base = DelayParams {
                feedback: 0.7,
                wow_flutter: 0.3,
                ..DelayParams::DEFAULT
            }
            .with(DelayParams::TYPE, ty.value());
            let free = DelayParams {
                time_ms: ms,
                ..base
            };
            let mut d = Box::new(TapeDelay::new());
            let want = play(&mut d, 600, plucks, |_| (free, 77.0));
            // The knob's own ms stays where it was: 20 ms here.
            let synced = on(
                DelayParams {
                    time_ms: 20.0,
                    ..base
                },
                n,
            );
            let mut d = Box::new(TapeDelay::new());
            let got = play(&mut d, 600, plucks, |_| (synced, bpm));
            assert!(bits(&got) == bits(&want), "{ty:?} {n:?} at {bpm} BPM");
        }
    }
}

/// Eight repeats each land within a sample of the grid, on a note of
/// whole samples and on one that isn't (1/8T at 140 BPM, 6,857.14), and
/// on a halved one: the fold never holds a time off the grid.
#[test]
fn eight_repeats_stay_on_the_grid() {
    for (n, bpm) in [
        (DelayNote::DEFAULT, 120.0),
        (note(8, DivKind::Triplet), 140.0),
        (note(4, DivKind::Straight), 97.0),
    ] {
        let p = DelayParams {
            feedback: 0.9,
            ..clean()
        };
        let (every, _) = fitted(n, bpm);
        let mut d = Box::new(TapeDelay::new());
        let out = play(&mut d, blocks(4.2), click, |_| (on(p, n), bpm as f32));
        for k in 1..=8 {
            let want = every * f64::from(k);
            let (lo, hi) = (want as usize - 8, want as usize + 9);
            let peak = (lo..hi)
                .max_by(|&a, &b| out[a].abs().total_cmp(&out[b].abs()))
                .unwrap();
            assert!(
                (peak as f64 - want).abs() <= 1.0,
                "{n:?} at {bpm}: repeat {k} at {peak}, grid {want}"
            );
            assert!(out[peak].abs() > 0.1, "repeat {k} is there");
        }
    }
}

// ---------------------------------------------------------------------------
// The fold
// ---------------------------------------------------------------------------

/// 1/4 through a sweep from 123 BPM down to 118 and back, a grid step
/// every 20 ms: it halves once, under 120 BPM, and comes back whole once,
/// 2 % inside the line (122.4 BPM); the head ends where it began.
#[test]
fn a_tempo_sweep_flips_the_fold_once_each_way() {
    let n = note(4, DivKind::Straight);
    let steps: Vec<f32> = (118 * 4..=123 * 4)
        .rev()
        .chain(118 * 4..=123 * 4)
        .map(|q| q as f32 / 4.0)
        .collect();
    let mut d = Box::new(TapeDelay::new());
    let (mut folds, mut buf) = (vec![], [0.0; BLOCK_SIZE]);
    for &bpm in &steps {
        for _ in 0..15 {
            d.play(&mut buf, &on(clean(), n), Tempo(bpm), FS);
        }
        folds.push((bpm, d.fold()));
    }
    let flips: Vec<_> = folds.windows(2).filter(|w| w[0].1 != w[1].1).collect();
    assert_eq!(flips.len(), 2, "{flips:?}");
    assert_eq!((flips[0][1].0, flips[0][1].1), (119.75, FoldState(1)));
    assert_eq!((flips[1][1].0, flips[1][1].1), (122.5, FoldState(0)));
    // FREE folds nothing.
    d.play(&mut buf, &clean(), Tempo(60.0), FS);
    assert_eq!(d.fold(), FoldState(0));
}

/// The same note at the same tempo plays the same however the knob got
/// there. At 81 BPM 1/4T is 493.8 ms and fits; 1/4 and 1/8D are halved.
/// Turned down to from 1/4, up to from 1/8, through 1/8D, or from rest,
/// 1/4T plays whole; and 1/4, halved, plays halved from either side.
#[test]
fn a_note_plays_the_same_however_it_was_reached() {
    let (q, qt, ed, e) = (
        note(4, DivKind::Straight),
        note(4, DivKind::Triplet),
        note(8, DivKind::Dotted),
        note(8, DivKind::Straight),
    );
    let lands = |turns: &[DelayNote]| {
        let mut d = Box::new(TapeDelay::new());
        let mut buf = [0.0; BLOCK_SIZE];
        for &n in turns {
            for _ in 0..40 {
                d.play(&mut buf, &on(clean(), n), Tempo(81.0), FS);
            }
        }
        (d.head().0, d.fold())
    };
    let whole = (fitted(qt, 81.0).0 as f32, FoldState(0));
    assert!((whole.0 - 493.83 * 48.0).abs() < 1.0);
    for turns in [
        &[qt][..],
        &[q, qt],
        &[q, ed, qt],
        &[e, qt],
        &[ed, qt],
        &[qt, q, qt],
    ] {
        assert_eq!(lands(turns), whole, "{turns:?}");
    }
    let halved = lands(&[q]);
    assert_eq!(halved.1, FoldState(1));
    for turns in [&[qt, q][..], &[e, q], &[q, ed, q]] {
        assert_eq!(lands(turns), halved, "{turns:?}");
    }
}

// ---------------------------------------------------------------------------
// GRAIN
// ---------------------------------------------------------------------------

/// GRAIN's repeat follows the note: a click comes back from the grains a
/// note later (within the read's four taps), at three tempos, halved
/// where it must be. DENSITY stays the knob's: free.
#[test]
fn grain_repeats_on_the_note() {
    let p = grain(clean())
        .with(DelayParams::TEXTURE, 0.0)
        .with(DelayParams::PITCH, 0.0)
        .with(DelayParams::DENSITY, 1.0)
        .with(DelayParams::SIZE, 0.6);
    for (n, bpm) in [
        (DelayNote::DEFAULT, 120.0),
        (note(8, DivKind::Straight), 100.0),
        (note(4, DivKind::Straight), 90.0),
        (note(16, DivKind::Triplet), 140.0),
    ] {
        let (want, halved) = fitted(n, bpm);
        // The grains run before the click, so one is reading as it passes.
        let late = move |b: usize| click(b.wrapping_sub(blocks(1.0)));
        let mut d = Box::new(TapeDelay::new());
        let out = play(&mut d, blocks(1.7), late, |_| (on(p, n), bpm as f32));
        let from = blocks(1.0) * BLOCK_SIZE;
        let peak = (from..out.len())
            .max_by(|&a, &b| out[a].abs().total_cmp(&out[b].abs()))
            .unwrap();
        assert!(out[peak].abs() > 0.05, "{n:?} at {bpm}: no repeat");
        assert!(
            ((peak - from) as f64 - want).abs() <= 2.0,
            "{n:?} at {bpm}: {} for {want}",
            peak - from
        );
        assert_eq!(d.fold(), FoldState(halved));
    }
}

// ---------------------------------------------------------------------------
// Tempo changes
// ---------------------------------------------------------------------------

const HZ: f32 = 440.0;

/// A 440 Hz sine through one clean repeat, `at`'s settings and tempo a
/// block; with the head's delay and its crossfade's samples left after
/// each block.
fn through(secs: f32, at: impl Fn(usize) -> (DelayParams, f32)) -> (Vec<f32>, Vec<(f32, u16)>) {
    let mut d = Box::new(TapeDelay::new());
    let (mut out, mut heads) = (vec![], vec![]);
    let tone = sine(HZ, 0.5);
    for b in 0..blocks(secs) {
        let mut buf = tone(b);
        let (p, bpm) = at(b);
        d.play(&mut buf, &p, Tempo::from_bpm(bpm), FS);
        out.extend_from_slice(&buf);
        heads.push(d.head());
    }
    (out, heads)
}

/// 120 → 121 → 120 BPM on 1/8D: the head glides, never crossfades; the
/// repeat bends no more than 2 % either way and does bend; no sample
/// steps; each glide lands inside 250 ms, after which the repeat is the
/// one a delay always at that tempo plays, bit for bit.
#[test]
fn a_small_tempo_change_glides_within_two_percent() {
    let p = on(clean(), DelayNote::DEFAULT);
    let (up, down) = (blocks(1.0), blocks(2.0));
    let bpm = move |b: usize| {
        if (up..down).contains(&b) {
            121.0
        } else {
            120.0
        }
    };
    let (out, heads) = through(3.0, |b| (p, bpm(b)));
    assert!(heads.iter().all(|h| h.1 == 0), "a crossfade ran");

    // Each glide's blocks: the head moving, a block's 2 % at most.
    let most = GLIDE_SPEED * BLOCK_SIZE as f32;
    let moved: Vec<usize> = (1..heads.len())
        .filter(|&b| heads[b].0 != heads[b - 1].0)
        .collect();
    for &b in &moved {
        let step = (heads[b].0 - heads[b - 1].0).abs();
        assert!(step <= most * 1.0001, "block {b} moved {step}");
    }
    // 250 ms at most at full speed, then the slow-in's few blocks.
    let land = blocks(GLIDE_MAX_S) + 40;
    assert!(
        moved
            .iter()
            .all(|&b| (up..up + land).contains(&b) || (down..down + land).contains(&b))
    );
    let at_121 = NOTES[DelayNote::DEFAULT.index()].ms(Tempo(121.0)) * FS as f32 / 1000.0;
    assert_eq!(heads[up + land].0, at_121);
    assert_eq!(heads.last().unwrap().0, 18_000.0);

    // The pitch: every period within 2 %, and the glides heard.
    let from = blocks(0.5) * BLOCK_SIZE;
    let hz = periods_hz(&out[from..]);
    let (lo, hi) = hz
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), &f| (l.min(f), h.max(f)));
    let (lo, hi) = (lo / f64::from(HZ), hi / f64::from(HZ));
    println!("glide: pitch ×{lo:.4} .. ×{hi:.4}");
    assert!(lo >= 1.0 - 0.0205 && hi <= 1.0 + 0.0205, "×{lo} .. ×{hi}");
    assert!(lo < 0.985 && hi > 1.015, "no bend: ×{lo} .. ×{hi}");

    // No step and no kink: within a 2 % faster sine's own curve, read
    // off the line as the steady one is (its 2⁻¹⁴ steps and all).
    let still = max_d2(&out[from..up * BLOCK_SIZE]);
    let d2 = max_d2(&out[from..]);
    println!("glide: d2 ×{:.3} of the steady repeat's", d2 / still);
    assert!(d2 <= still * 1.06, "{d2} against {still}");

    // Landed, it is the delay that was always at 121 BPM.
    let (always, _) = through(2.0, |_| (p, 121.0));
    let at = (up + land) * BLOCK_SIZE..down * BLOCK_SIZE;
    assert!(bits(&out[at.clone()]) == bits(&always[at]));
}

/// What a change that isn't a small tempo move does: `to` from `from` at
/// one second crossfades (the head never slides), ends within 20 ms, and
/// is then the delay always at `to`; no sample steps on the way.
fn crossfades(what: &str, from: (DelayParams, f32), to: (DelayParams, f32)) {
    let at = blocks(1.0);
    let (out, heads) = through(2.0, |b| if b < at { from } else { to });
    let fade = usize::from(TIME_FADE) / BLOCK_SIZE;
    assert_eq!(
        heads[at].1,
        TIME_FADE - BLOCK_SIZE as u16,
        "{what}: no fade"
    );
    assert_eq!(heads[at + fade - 1].1, 0, "{what}: the fade ran on");
    let slid = (1..heads.len())
        .filter(|&b| heads[b].0 != heads[b - 1].0)
        .collect::<Vec<_>>();
    assert_eq!(slid, [at + fade - 1], "{what}: the head moved once");
    let (always, _) = through(2.0, |_| to);
    let after = (at + fade) * BLOCK_SIZE..;
    assert!(bits(&out[after.clone()]) == bits(&always[after]), "{what}");
    // Two heads of one sine sum to no more than it, each read off the
    // line's 2⁻¹⁴ steps: no step. (A step of a hundredth of the sine's
    // height would be ×3.)
    let still = max_d2(&out[blocks(0.6) * BLOCK_SIZE..at * BLOCK_SIZE]);
    let d2 = max_d2(&out[blocks(0.6) * BLOCK_SIZE..]);
    println!("{what}: d2 ×{:.3} of the steady repeat's", d2 / still);
    assert!(d2 <= still * 1.25, "{what}: {d2} against {still}");
}

/// A tempo jump too far to glide in 250 ms (120 → 90 BPM, 375 → 500 ms),
/// a note turned, SYNC switched on and off, and a tempo move that flips
/// the fold all crossfade, as a FREE TIME turn does.
#[test]
fn a_jump_a_note_a_fold_and_sync_itself_crossfade() {
    let p = on(clean(), DelayNote::DEFAULT);
    crossfades("120 → 90 BPM", (p, 120.0), (p, 90.0));
    let eighth = on(clean(), note(8, DivKind::Straight));
    crossfades("1/8D → 1/8", (p, 120.0), (eighth, 120.0));
    let free = DelayParams {
        time_ms: 200.0,
        ..clean()
    };
    let synced = on(free, DelayNote::DEFAULT);
    crossfades("FREE → SYNC", (free, 120.0), (synced, 120.0));
    crossfades("SYNC → FREE", (synced, 120.0), (free, 120.0));
    crossfades("FREE TIME", (free, 120.0), (clean(), 120.0));
    let quarter = on(clean(), note(4, DivKind::Straight));
    crossfades("1/4 folds", (quarter, 120.0), (quarter, 119.75));
}

/// A DAW's ritardando, 126 → 100 BPM over four seconds on its 0.25 BPM
/// grid, on 1/8 and (through its fold) on 1/4: glides and crossfades in
/// turn, and never a step or a zipper in the repeat.
#[test]
fn a_tempo_ramp_never_steps() {
    for n in [note(8, DivKind::Straight), note(4, DivKind::Straight)] {
        let p = on(clean(), n);
        let ramp = |b: usize| {
            let t = chimera_core::dsp::clampf(b as f32 / blocks(4.0) as f32 - 0.25, 0.0, 1.0);
            (p, libm::roundf((126.0 - 26.0 * t) * 4.0) / 4.0)
        };
        let (out, heads) = through(6.0, ramp);
        let still = max_d2(&out[blocks(0.6) * BLOCK_SIZE..blocks(1.0) * BLOCK_SIZE]);
        let d2 = max_d2(&out[blocks(0.6) * BLOCK_SIZE..]);
        let fades = heads.windows(2).filter(|w| w[0].1 == 0 && w[1].1 != 0);
        println!(
            "ramp on {n:?}: d2 ×{:.3} of the steady repeat's, {} crossfades",
            d2 / still,
            fades.count()
        );
        // A crossfade's two reads, its moving weights and a 2 % bend
        // add up to ×1.3 here; a step of a hundredth of the sine is ×3.
        assert!(d2 <= still * 1.5, "{n:?}: {d2} against {still}");
    }
}

/// GRAIN on a tempo jump (120 → 90 BPM) and on a small move (120 → 121):
/// the grains already playing finish where they were and the next start
/// on the new note, each under its own bell, so nothing clicks.
#[test]
fn grain_takes_a_tempo_change_without_a_click() {
    let p = on(grain(clean()), DelayNote::DEFAULT);
    for to in [90.0, 121.0] {
        let at = blocks(2.0);
        let (out, _) = through(4.0, |b| (p, if b < at { 120.0 } else { to }));
        let (a, z) = (blocks(1.0) * BLOCK_SIZE, at * BLOCK_SIZE);
        let steady = max_d2(&out[a..z]);
        let moved = max_d2(&out[z..z + blocks(1.0) * BLOCK_SIZE]);
        println!("GRAIN 120 → {to}: d2 ×{:.2} of its steady", moved / steady);
        assert!(moved <= steady * 1.5, "120 → {to}: {moved} over {steady}");
    }
}

// ---------------------------------------------------------------------------
// What is stored
// ---------------------------------------------------------------------------

/// The FX's delay as a `Blocks`, for the codec.
struct Fx<'a>(&'a mut FxParams);

impl BlockRead for Fx<'_> {
    fn block(&self, b: BlockRef) -> Option<BlockView<'_>> {
        (b == BlockRef::Delay).then_some(BlockView::Plain(&self.0.delay))
    }
}

impl Blocks for Fx<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>> {
        (b == BlockRef::Delay).then_some(BlockViewMut::Plain(&mut self.0.delay))
    }
}

/// SYNC is id 12 and NOTE id 13, FREE and 1/8D by default; the free TIME
/// and the note are two values, and neither moves the other.
#[test]
fn the_free_time_and_the_note_are_kept_apart() {
    let mut p = DelayParams::default();
    assert_eq!((DelayParams::SYNC.0, DelayParams::NOTE.0), (12, 13));
    assert_eq!((p.sync(), p.note()), (DelaySync::Free, DelayNote::DEFAULT));
    assert_eq!(
        find_spec(&DELAY_SPECS, DelayParams::SYNC).unwrap().ident,
        "SYNC"
    );
    assert_eq!(
        find_spec(&DELAY_SPECS, DelayParams::NOTE).unwrap().ident,
        "NOTE"
    );

    p.set(DelayParams::TIME_MS, 122.0);
    p.set(DelayParams::SYNC, 1.0);
    let n = note(16, DivKind::Dotted);
    p.set(DelayParams::NOTE, n.value());
    assert_eq!((p.sync(), p.note(), p.time_ms), (DelaySync::Sync, n, 122.0));
    // A note a tick, end to end.
    p.set(DelayParams::NOTE, 0.0);
    for i in 1..DelayNote::COUNT {
        p.nudge(DelayParams::NOTE, 1);
        assert_eq!(p.note().index(), i);
    }
    p.nudge(DelayParams::NOTE, 1);
    assert_eq!(p.note().index(), DelayNote::COUNT - 1);
    // Back in FREE the ms is the one left there, and the note waits.
    p.set(DelayParams::SYNC, 0.0);
    assert_eq!((p.sync(), p.time_ms), (DelaySync::Free, 122.0));
    assert_eq!(p.note().index(), DelayNote::COUNT - 1);
}

/// A project saved before SYNC has no id 12 or 13: it loads FREE on 1/8D
/// with its own TIME, whatever the box held. One saved with them reads
/// both back, and its free TIME beside them.
#[test]
fn an_old_project_loads_free() {
    let mut saved = FxParams::default();
    (saved.delay.time_ms, saved.delay.feedback) = (250.0, 0.6);
    let mut rec = RecordBuf::new();
    encode_block(BlockRef::Delay, &saved.delay, None, &mut rec);
    let new = rec.as_slice().to_vec();
    // The same record as 0.3 wrote it: ids 0–11.
    let old: Vec<u8> = new[..1]
        .iter()
        .copied()
        .chain(
            new[1..]
                .chunks(5)
                .filter(|e| e[0] < DelayParams::SYNC.0)
                .flatten()
                .copied(),
        )
        .collect();
    assert_eq!(old.len(), 1 + 12 * 5);
    assert_eq!(new.len(), 1 + 14 * 5);

    let mut fx = FxParams::default();
    decode_block(&old, &[], &[], Some(&mut Fx(&mut fx))).unwrap();
    assert_eq!(fx.delay, saved.delay);
    assert_eq!(
        (fx.delay.sync(), fx.delay.note()),
        (DelaySync::Free, DelayNote::DEFAULT)
    );

    let n = note(8, DivKind::Triplet);
    saved.delay = on(saved.delay, n);
    let mut rec = RecordBuf::new();
    encode_block(BlockRef::Delay, &saved.delay, None, &mut rec);
    let mut fx = FxParams::default();
    decode_block(rec.as_slice(), &[], &[], Some(&mut Fx(&mut fx))).unwrap();
    assert_eq!(
        (fx.delay.sync(), fx.delay.note(), fx.delay.time_ms),
        (DelaySync::Sync, n, 250.0)
    );
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

/// The delay's host cost, ns a block, best of five runs of 20,000 blocks
/// over the plucks: FREE and SYNC, steady and with the tempo always
/// moving (118..122 BPM, a grid step every eight blocks: TAPE glides
/// nearly every block), TAPE and GRAIN.
/// `cargo test --release -p chimera-core --test delay_sync_test -- --ignored host_cost --nocapture`
/// A block's tempo.
type Bpm<'a> = &'a dyn Fn(usize) -> f32;

#[test]
#[ignore]
fn host_cost() {
    let busy = DelayParams {
        feedback: 0.6,
        wow_flutter: 0.3,
        ..DelayParams::DEFAULT
    };
    let steady = |_: usize| 120.0;
    let moving = |b: usize| {
        let q = (b / 8) % 32;
        118.0 + (if q < 16 { q } else { 32 - q }) as f32 * 0.25
    };
    let cases: [(&str, DelayParams, Bpm); 6] = [
        ("TAPE FREE", busy, &steady),
        ("TAPE SYNC", on(busy, DelayNote::DEFAULT), &steady),
        (
            "TAPE SYNC, tempo moving",
            on(busy, DelayNote::DEFAULT),
            &moving,
        ),
        ("GRAIN FREE", grain(busy), &steady),
        ("GRAIN SYNC", on(grain(busy), DelayNote::DEFAULT), &steady),
        (
            "GRAIN SYNC, tempo moving",
            on(grain(busy), DelayNote::DEFAULT),
            &moving,
        ),
    ];
    const BLOCKS: usize = 20_000;
    let input: Vec<[f32; BLOCK_SIZE]> = (0..64).map(plucks).collect();
    for (name, p, bpm) in cases {
        let mut best = f64::MAX;
        let mut sum = 0.0f32;
        for _ in 0..5 {
            let mut d = Box::new(TapeDelay::new());
            let t = std::time::Instant::now();
            for b in 0..BLOCKS {
                let mut buf = input[b % 64];
                d.play(&mut buf, &p, Tempo::from_bpm(bpm(b)), FS);
                sum += buf[0];
            }
            best = best.min(t.elapsed().as_nanos() as f64 / BLOCKS as f64);
        }
        println!("{name}: {best:.0} ns a block ({sum:e})");
    }
}

/// A project stored with NOTE between two notes (an editor's drag before
/// NOTE was stepped, ADR 0306) loads to the nearest note, and the engine
/// reads it through `DelayNote::at` as before.
#[test]
fn an_off_step_note_loads_to_the_nearest() {
    let mut saved = FxParams::default();
    saved.delay.write(DelayParams::NOTE, 0.55);
    let mut rec = RecordBuf::new();
    encode_block(BlockRef::Delay, &saved.delay, None, &mut rec);
    let mut fx = FxParams::default();
    decode_block(rec.as_slice(), &[], &[], Some(&mut Fx(&mut fx))).unwrap();
    let v = fx.delay.get(DelayParams::NOTE);
    assert_eq!(fx.delay.note(), DelayNote::at(v));
    assert!((v - 0.6).abs() < 1e-6, "{v}");
}
