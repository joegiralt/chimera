//! TAPE's TRANSWAVE (spec § 2 TRANSWAVE): octave mip levels, the note-on
//! level, hysteresis at the alias limit, the one-block level fade, and the
//! per-sample scan. Precedent: Ensoniq VFX/SD-1 transwaves; PPG Wave and
//! Waldorf Microwave (band-limited copies per octave); Polyend Tracker v1.5.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::wave::{
    ALIAS_STEP, RISE_STEP, level_offset, next_level, note_on_level, playable, top_of,
};
use chimera_core::dsp::tape_engine::{Probe, TapeEngine, TapeEngineParams, TapeModel};
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, held_transwave, power_bh, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    *TapeEngine::init_in_place(&mut slot)
}

fn wave(p: TapeEngineParams) -> TapeEngineParams {
    TapeEngineParams {
        model: TapeModel::Transwave,
        ..p
    }
}

/// A note's fundamental, Hz, with `cents` on it.
fn hz(note: u8, cents: f32) -> f64 {
    440.0 * 2f64.powf((note as f64 * 100.0 + cents as f64 - 6_900.0) / 1_200.0)
}

/// The largest level `N` (16 to `top`) whose step `N · f / SR` is ≤ 1.
fn largest(f: f64, top: u32) -> u32 {
    let mut n = top;
    while n > 16 && n as f64 * f > SR as f64 {
        n /= 2;
    }
    n
}

/// `blocks` blocks with `pitch(block)` cents on the note, from a note-on at
/// `pitch(0)`: the output and each block's probe.
fn run(
    v: &SourceView<'_>,
    p: &TapeEngineParams,
    note: u8,
    pitch: &dyn Fn(usize) -> f32,
    blocks: usize,
) -> (Vec<f32>, Vec<Probe>) {
    let mut e = fresh();
    e.set_pitch(pitch(0));
    e.note_on(note, p, v, SR);
    let (mut out, mut probes) = (Vec::new(), Vec::new());
    let mut b = [0.0; B];
    for n in 0..blocks {
        e.set_pitch(pitch(n));
        e.render(&mut b, p, v, SR);
        out.extend_from_slice(&b);
        probes.push(e.probe());
    }
    (out, probes)
}

/// The worst isolated corner: each sample's second difference against the
/// largest within ±256 samples, its own ±2 left out (the rig's detector).
/// A periodic wave scores about 1; a step in it scores high.
fn isolation(x: &[f32]) -> f32 {
    let d: Vec<f32> = x
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .collect();
    let floor = 1e-3;
    let mut worst = 0.0f32;
    for i in 0..d.len() {
        if d[i] <= floor {
            continue;
        }
        let near = |r: core::ops::Range<usize>| d[r].iter().fold(0.0f32, |m, &v| m.max(v));
        let left = near(i.saturating_sub(256)..i.saturating_sub(2));
        let right = near((i + 3).min(d.len())..(i + 257).min(d.len()));
        worst = worst.max(d[i] / left.max(right).max(f32::MIN_POSITIVE));
    }
    worst
}

#[test]
fn layout_and_levels() {
    assert_eq!(top_of(16 * 1_008, 16), Some(512));
    assert_eq!(top_of(3 * 2_032, 3), Some(1_024));
    assert_eq!(top_of(16 * 1_008 - 2, 16), None);
    assert_eq!(top_of(1_008, 2), None);
    assert_eq!(level_offset(512, 9), 0);
    assert_eq!(level_offset(512, 8), 512);
    assert_eq!(level_offset(512, 4), 1_008 - 16);
    assert_eq!(level_offset(1_024, 4), 2_032 - 16);
    // Hysteresis: drops past ALIAS_STEP, rises under RISE_STEP.
    assert_eq!(next_level(8, 1.42, 512), 8);
    assert_eq!(next_level(8, 1.44, 512), 7);
    assert_eq!(next_level(8, 0.36, 512), 8);
    assert_eq!(next_level(8, 0.34, 512), 9);
    assert_eq!(next_level(8, 0.34, 256), 8, "never above the top");
    assert_eq!(next_level(4, 9.0, 512), 4, "never below 16");
    assert_eq!(next_level(9, 5.9, 512), 6, "a big step drops in one go");
    assert_eq!(note_on_level(93.0, 512, SR), 512);
    assert_eq!(note_on_level(94.0, 512, SR), 256);
    assert_eq!(note_on_level(13_000.0, 512, SR), 16);
}

/// Every note, top 512 and 1 024: the note-on level is the largest whose
/// step is at most 1.
#[test]
fn transwave_level_is_largest() {
    for top in [512u32, 1_024] {
        let held = held_transwave(top, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
        let v = SourceView::strict(&held).unwrap();
        let p = wave(TapeEngineParams::default());
        for note in 0..=127u8 {
            let (_, probes) = run(&v, &p, note, &|_| 0.0, 1);
            let want = largest(hz(note, 0.0), top);
            assert_eq!(
                1u32 << probes[0].level,
                want,
                "top {top}, note {note}: level {}",
                probes[0].level
            );
            assert_eq!(note_on_level(hz(note, 0.0) as f32, top, SR), want);
        }
    }
}

/// A ±1 st vibrato (5 Hz, 2 s) never changes the level, on any note; nor
/// does a ±6 st one (under the ±6.2 st hysteresis).
#[test]
fn vibrato_never_changes_level() {
    let held = held_transwave(512, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    let blocks = 2 * SR as usize / B;
    for depth in [100.0f32, 600.0] {
        for note in 12..=120u8 {
            let vib = |n: usize| {
                depth * (core::f32::consts::TAU * 5.0 * (n * B) as f32 / SR as f32).sin()
            };
            let (_, probes) = run(&v, &p, note, &vib, blocks);
            assert!(
                probes
                    .iter()
                    .all(|q| q.level == probes[0].level && q.from.is_none()),
                "±{depth} cents, note {note}: the level moved"
            );
        }
    }
}

/// ±7 st does: from a note-on step near 1, +7 st passes 1.43.
#[test]
fn seven_semitones_does() {
    let held = held_transwave(512, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    // F♯2: its note-on step at level 512 is 0.987.
    let note = 42;
    let s0 = 512.0 * hz(note, 0.0) / SR as f64;
    assert!(s0 > 0.955 && s0 <= 1.0, "{s0}");
    let blocks = 2 * SR as usize / B;
    let vib = |n: usize| 700.0 * (core::f32::consts::TAU * 2.0 * (n * B) as f32 / SR as f32).sin();
    let (_, probes) = run(&v, &p, note, &vib, blocks);
    assert_eq!(probes[0].level, 9);
    assert!(
        probes.iter().any(|q| q.level != 9),
        "note {note}: ±7 st never changed the level"
    );
}

/// Frequencies `x` folds to at `SR`: into [0, SR/2].
fn fold(x: f64) -> f64 {
    let y = x.rem_euclid(SR as f64);
    y.min(SR as f64 - y)
}

/// A 16-frame saw-to-square transwave swept C1 → C8 and back by a pitch
/// route, held at steady points every 3 st (an octave's hysteresis holds
/// three or four, so a level read past `ALIAS_STEP` would be held at one).
///
/// No harmonic folds: the energy off the note's harmonics and off the
/// Hermite read's own images (`(k·N ± h)·f0`, folded; spec § 2's table, a
/// property of every read, not of the level) stays 60 dB under the whole,
/// at every level, both ways. In every level change, the level faded to
/// steps at most `ALIAS_STEP` across the block, and the one faded from no
/// more than the block's own pitch move past it. Nothing is asserted above
/// C8. The images are printed: `-- --nocapture`.
#[test]
fn transwave_is_alias_free() {
    let held = held_transwave(512, 16, |f, h| {
        let odd = if h % 2 == 1 { 1.0 } else { f as f32 / 15.0 };
        0.5 * odd / h as f32
    });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams {
        position: 0.5,
        ..TapeEngineParams::default()
    });
    let note = 24u8;
    let (glide, len) = (24usize, 131_072usize);
    let mut e = fresh();
    e.note_on(note, &p, &v, SR);
    let mut b = [0.0; B];
    let mut cents = 0.0f32;
    let mut levels = std::collections::BTreeSet::new();
    let bins = len as f64 / SR as f64;
    let mut targets: Vec<f32> = (0..=28).map(|k| k as f32 * 300.0).collect();
    let down: Vec<f32> = targets.iter().rev().skip(1).copied().collect();
    targets.extend(down);
    for target in targets {
        // Glide there over `glide` blocks, checking every level change.
        let from = cents;
        for k in 1..=glide {
            let c0 = cents;
            cents = from + (target - from) * k as f32 / glide as f32;
            e.set_pitch(cents);
            e.render(&mut b, &p, &v, SR);
            let q = e.probe();
            let cyc = |c: f32| hz(note, c) / SR as f64;
            let (lo, hi) = (cyc(c0).min(cyc(cents)), cyc(c0).max(cyc(cents)));
            assert!(
                hi * (1u64 << q.level) as f64 <= ALIAS_STEP as f64 * 1.0001,
                "{cents} cents: level {} steps {}",
                q.level,
                hi * (1u64 << q.level) as f64
            );
            if let Some(old) = q.from {
                let s = lo * (1u64 << old) as f64;
                assert!(
                    s <= ALIAS_STEP as f64 * 1.0001,
                    "{cents} cents: faded from level {old} at step {s}"
                );
            }
        }
        // Hold, then look.
        let mut out = Vec::with_capacity(len + 4 * B);
        while out.len() < len + 4 * B {
            e.render(&mut b, &p, &v, SR);
            out.extend_from_slice(&b);
        }
        let q = e.probe();
        levels.insert(q.level);
        let pw = power_bh(&out[out.len() - len..]);
        let f0 = hz(note, cents);
        let n = (1u64 << q.level) as f64;
        let mut known = vec![false; pw.len()];
        let mark = |known: &mut [bool], f: f64| {
            let k = (fold(f) * bins).round() as i64;
            for j in (k - 5).max(0)..=(k + 5).min(known.len() as i64 - 1) {
                known[j as usize] = true;
            }
        };
        let mut h = 0.0;
        while h * f0 <= SR as f64 / 2.0 {
            mark(&mut known, h * f0);
            h += 1.0;
        }
        let total: f64 = pw[8..].iter().sum();
        let off_harmonic: f64 = (8..pw.len()).filter(|&k| !known[k]).map(|k| pw[k]).sum();
        for k in 1..=8 {
            for h in 1..=(0.35 * n) as u64 {
                mark(&mut known, (k as f64 * n - h as f64) * f0);
                mark(&mut known, (k as f64 * n + h as f64) * f0);
            }
        }
        let off: f64 = (8..pw.len()).filter(|&k| !known[k]).map(|k| pw[k]).sum();
        let db = |x: f64| 10.0 * (x / total).log10();
        eprintln!(
            "{cents:5} cents, level {}, step {:.3}: images {:.1} dB, the rest {:.1} dB",
            q.level,
            n * f0 / SR as f64,
            db(off_harmonic),
            db(off)
        );
        assert!(
            db(off) < -60.0,
            "{cents} cents (level {}): off-harmonic energy {:.1} dB",
            q.level,
            db(off)
        );
    }
    assert_eq!(
        levels.into_iter().collect::<Vec<_>>(),
        (4..=9).collect::<Vec<_>>(),
        "every level heard"
    );
}

/// Notes 24 to 96 by octaves: the cycle is the note within 0.1 cent.
#[test]
fn transwave_cycle_in_tune() {
    let held = held_transwave(512, 2, |_, h| if h == 1 { 0.9 } else { 0.0 });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    for note in (24..=96u8).step_by(12) {
        let (out, _) = run(&v, &p, note, &|_| 0.0, 2 * SR as usize / B);
        let x = &out[B..];
        let mut t = Vec::new();
        for i in 1..x.len() {
            let (a, b) = (x[i - 1] as f64, x[i] as f64);
            if a < 0.0 && b >= 0.0 {
                t.push((i - 1) as f64 + a / (a - b));
            }
        }
        let f = (t.len() - 1) as f64 / (t[t.len() - 1] - t[0]) * SR as f64;
        let cents = 1_200.0 * (f / hz(note, 0.0)).log2();
        assert!(cents.abs() < 0.1, "note {note}: {cents:.4} cents out");
    }
}

/// A ±12 st square route on a note where each flip changes the level: four
/// heads for exactly the one block of each change, and the output's
/// corners no more isolated than at either pitch held. The frames hold a
/// 100th harmonic that level 512 has and 256 doesn't, so a level that
/// switched without its fade would step.
#[test]
fn level_change_crossfades_one_block() {
    let held = held_transwave(512, 1, |_, h| match h {
        1 => 0.3,
        100 => 0.5,
        _ => 0.0,
    });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    // F2: step 0.93 at level 512, so +12 st drops and −12 st rises.
    let note = 41;
    let fixed = [-1_200.0f32, 1_200.0]
        .map(|c| isolation(&run(&v, &p, note, &|_| c, 400).0[4 * B..]))
        .into_iter()
        .fold(0.0f32, f32::max);
    for half in [7usize, 13, 29] {
        let sq = |n: usize| {
            if (n / half).is_multiple_of(2) {
                -1_200.0
            } else {
                1_200.0
            }
        };
        let (out, probes) = run(&v, &p, note, &sq, 1_200);
        let mut changes = 0;
        for n in 1..probes.len() {
            // The level in play reads at most ALIAS_STEP across its block,
            // the pitch's whole move in it included.
            let most = hz(note, sq(n - 1)).max(hz(note, sq(n))) / SR as f64;
            let step = most * (1u64 << probes[n].level) as f64;
            assert!(
                step <= ALIAS_STEP as f64,
                "every {half}: block {n}: step {step}"
            );
            let moved = probes[n].level != probes[n - 1].level;
            assert_eq!(
                probes[n].from.is_some(),
                moved,
                "every {half}: block {n}: four heads only in a change's block"
            );
            if moved {
                assert_eq!(probes[n].from, Some(probes[n - 1].level));
                changes += 1;
            }
        }
        assert!(
            changes >= 1_200 / half - 1,
            "every {half}: {changes} changes"
        );
        let worst = isolation(&out[4 * B..]);
        assert!(
            worst <= 1.25 * fixed.max(1.0),
            "every {half} blocks: corners {worst} against {fixed}"
        );
    }
}

/// MOTION ±1 at a scan rate 94.5 times under 750 Hz, SIZE 1: the scan moves
/// `p` per sample, so nothing sits at the block rate. Frame `f` is a sine
/// of amplitude `f/15`, so the scan is an amplitude modulation, and a `p`
/// stepped once a block would put sidebands at the note ± 750 Hz (± the
/// scan's harmonics, half way between those of the scan itself).
#[test]
fn scan_has_no_block_sideband() {
    let held = held_transwave(
        512,
        16,
        |f, h| if h == 1 { 0.9 * f as f32 / 15.0 } else { 0.0 },
    );
    let v = SourceView::strict(&held).unwrap();
    let scan = 750.0 / 94.5;
    let len = 131_072;
    let bins = len as f64 / SR as f64;
    for motion in [1.0f32, -1.0] {
        let p = wave(TapeEngineParams {
            position: 0.5,
            size: 1.0,
            motion: motion * scan as f32 / 8.0,
            ..TapeEngineParams::default()
        });
        let note = 81;
        let (out, _) = run(&v, &p, note, &|_| 0.0, len / B + 16);
        let pw = power_bh(&out[out.len() - len..]);
        let f0 = hz(note, 0.0);
        let peak = |f: f64| {
            let k = (f * bins).round() as usize;
            pw[k - 1..=k + 1].iter().fold(0.0f64, |m, &x| m.max(x))
        };
        let fundamental = peak(f0);
        for side in [-1.0, 1.0] {
            for j in -4..=4 {
                let f = f0 + side * (94.5 + j as f64) * scan;
                let db = 10.0 * (peak(f) / fundamental).log10();
                assert!(db < -80.0, "MOTION {motion}: {f:.2} Hz at {db:.1} dB");
            }
        }
    }
}

/// The frame position: POSITION picks it, lerped between the two frames
/// either side; MOTION scans a triangle round it at `8 · MOTION` Hz, SIZE
/// frames wide either way, clamped to the table.
#[test]
fn scan_sweeps_frames() {
    let held = held_transwave(
        512,
        16,
        |f, h| if h == 1 { 0.9 * f as f32 / 15.0 } else { 0.0 },
    );
    let v = SourceView::strict(&held).unwrap();
    // Still: frame 7.25's amplitude, a quarter of the way from 7 to 8.
    let p = wave(TapeEngineParams {
        position: 7.25 / 15.0,
        ..TapeEngineParams::default()
    });
    let (out, probes) = run(&v, &p, 69, &|_| 0.0, 100);
    let peak = out[4 * B..].iter().fold(0.0f32, |m, &y| m.max(y.abs()));
    assert!((peak - 0.9 * 7.25 / 15.0).abs() < 0.005, "{peak}");
    assert_eq!((probes[99].head, probes[99].other), (7, Some(8)));
    assert!((probes[99].gain - 0.25).abs() < 1e-4, "{}", probes[99].gain);
    // MOTION 0.5: 4 Hz, round POSITION ½, SIZE 0.6: frames 0 to 15 and
    // back, clamped at each end.
    let p = wave(TapeEngineParams {
        position: 0.5,
        size: 0.6,
        motion: 0.5,
        ..TapeEngineParams::default()
    });
    let (_, probes) = run(&v, &p, 69, &|_| 0.0, SR as usize / B);
    let heads: Vec<u32> = probes.iter().map(|q| q.head).collect();
    assert_eq!(heads.iter().min(), Some(&0));
    assert_eq!(heads.iter().max(), Some(&15));
    let tops = heads.windows(2).filter(|w| w[0] < 15 && w[1] == 15).count();
    assert_eq!(tops, 4, "4 Hz: four peaks in a second");
    // SIZE 0.2: 3 frames either side of 7.5.
    let p = TapeEngineParams { size: 0.2, ..p };
    let (_, probes) = run(&v, &p, 69, &|_| 0.0, SR as usize / B);
    let heads: Vec<u32> = probes.iter().map(|q| q.head).collect();
    assert_eq!(
        (heads.iter().min(), heads.iter().max()),
        (Some(&4), Some(&10))
    );
}

/// STEAL GLIDE glides the fundamental: 63 % of the octave in `tau`.
#[test]
fn steal_glide_glides_the_cycle() {
    let held = held_transwave(512, 1, |_, h| if h == 1 { 0.9 } else { 0.0 });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let mut b = [0.0; B];
    for _ in 0..10 {
        e.render(&mut b, &p, &v, SR);
    }
    e.glide_on(72, &p, &v, (SR, 0.1));
    let mut out = Vec::new();
    for _ in 0..300 {
        e.render(&mut b, &p, &v, SR);
        out.extend_from_slice(&b);
    }
    let ups: Vec<f64> = (1..out.len())
        .filter(|&i| out[i - 1] < 0.0 && out[i] >= 0.0)
        .map(|i| (i - 1) as f64 + out[i - 1] as f64 / (out[i - 1] - out[i]) as f64)
        .collect();
    let at = ups
        .windows(2)
        .find(|w| (SR as f64 / (w[1] - w[0]) / hz(60, 0.0)).log2() >= 0.632)
        .map(|w| w[1] / SR as f64)
        .expect("the glide arrives");
    assert!((at - 0.1).abs() < 0.004, "63 % at {at} s, want 0.1 s");
}

/// TRANSWAVE plays only a `Transwave` whose frames fit its count; every
/// other model plays any source.
#[test]
fn plain_source_is_not_playable() {
    // As many frames as a one-frame transwave: only its shape refuses it.
    let plain = held_plain(&sine(48_000, 100.0, 1_008), 48_000);
    let pv = SourceView::strict(&plain).unwrap();
    let tw = held_transwave(512, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
    let tv = SourceView::strict(&tw).unwrap();
    let cut = &tw[..tw.len() - 3];
    let cv = SourceView::strict(cut).unwrap();
    assert!(!playable(TapeModel::Transwave, &pv));
    assert!(playable(TapeModel::Transwave, &tv));
    assert!(
        !playable(TapeModel::Transwave, &cv),
        "frames short of its count"
    );
    for m in TapeModel::ALL {
        if m != TapeModel::Transwave {
            for v in [&pv, &tv, &cv] {
                assert!(playable(m, v), "{m:?}");
            }
        }
    }
    // Played anyway (the Instrument refuses it first), it is silent; and a
    // row cut short under a playing note falls silent, never reading past it.
    let p = wave(TapeEngineParams::default());
    for v in [&pv, &cv] {
        let (out, _) = run(v, &p, 60, &|_| 0.0, 8);
        assert!(out.iter().all(|&y| y == 0.0));
    }
    let mut e = fresh();
    e.note_on(60, &p, &tv, SR);
    let mut b = [0.0; B];
    e.render(&mut b, &p, &tv, SR);
    assert!(b.iter().any(|&y| y != 0.0));
    e.render(&mut b, &p, &cv, SR);
    assert!(b.iter().all(|&y| y == 0.0));
}

/// The fundamental ramps across the block a pitch moves in, as `inc` does:
/// a +12 st step lands on the phase a per-sample ramp gives, never a
/// stepped one.
#[test]
fn pitch_ramps_across_the_block() {
    let held = held_transwave(512, 1, |_, h| if h == 1 { 0.9 } else { 0.0 });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    let (out, _) = run(&v, &p, 69, &|n| if n < 10 { 0.0 } else { 1_200.0 }, 20);
    let (c0, c1) = (hz(69, 0.0) / SR as f64, hz(69, 1_200.0) / SR as f64);
    let mut ph = 0.0f64;
    for (n, &y) in out.iter().enumerate() {
        let want = 0.9 * (core::f64::consts::TAU * ph).sin();
        if n >= B {
            assert!(
                (y as f64 - want).abs() < 2e-3,
                "sample {n}: {y} against {want}"
            );
        }
        let (blk, i) = (n / B, n % B);
        ph += match blk {
            0..=9 => c0,
            10 => c0 + (i + 1) as f64 * (c1 - c0) / B as f64,
            _ => c1,
        };
    }
}

/// RISE_STEP and ALIAS_STEP are the spec's: a level's content stops at
/// 0.35 N, alias-free to a step of 1/(2 · 0.35).
#[test]
fn hysteresis_constants() {
    assert_eq!(ALIAS_STEP, 1.43);
    assert_eq!(RISE_STEP, 0.35);
    assert!((1.0 / (2.0 * 0.35) - ALIAS_STEP).abs() < 0.01);
}

/// A retrigger on a sounding TRANSWAVE ramps from where the output was (its
/// phase and frame restart under the note-on ramp); `hold()` before a fresh
/// note starts it from 0.
#[test]
fn retrigger_ramps_from_last() {
    let held = held_transwave(512, 16, |f, h| (f + 1) as f32 * 0.05 / h as f32);
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams {
        position: 0.7,
        ..TapeEngineParams::default()
    });
    let mut e = fresh();
    e.note_on(57, &p, &v, SR);
    let mut b = [0.0; B];
    let mut out = Vec::new();
    for n in 0..60 {
        if n == 30 {
            e.note_on(64, &p, &v, SR);
        }
        e.render(&mut b, &p, &v, SR);
        if n == 30 {
            assert_eq!(b[0], out[out.len() - 1], "from the last sample");
        }
        out.extend_from_slice(&b);
    }
    let step = |x: &[f32]| x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()));
    let steady = step(&out[4 * B..30 * B]).max(step(&out[34 * B..]));
    let jump = step(&out[30 * B - 2..32 * B]);
    assert!(jump <= 1.1 * steady, "{jump} against {steady}");
    e.hold();
    e.note_on(64, &p, &v, SR);
    e.render(&mut b, &p, &v, SR);
    assert_eq!(b[0], 0.0);
}
