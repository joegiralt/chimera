//! TAPE's TRANSWAVE (spec § 2 TRANSWAVE): octave mip levels, the note-on
//! level, hysteresis at the alias limit, the one-block level fade, and the
//! per-sample scan. Precedent: Ensoniq VFX/SD-1 transwaves; PPG Wave and
//! Waldorf Microwave (band-limited copies per octave); Polyend Tracker v1.5.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::wave;
use chimera_core::dsp::tape_engine::wave::{
    ALIAS_STEP, CYCLES_MAX, FADE_CYCLES, RISE_STEP, frame_len, level_offset, next_level,
    note_on_level, nyquist_gain, playable, stored_log2, top_of,
};
use chimera_core::dsp::tape_engine::{Probe, TapeEngine, TapeEngineParams};
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, held_transwave, power_bh, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    unsafe { slot.assume_init() }
}

fn wave(p: TapeEngineParams) -> TapeEngineParams {
    TapeEngineParams {
        machine: TapeMachine::Wave,
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
    // Stored oversampled: nominal 512, 256, 128, 64, 32, 16 at 1 024, 512,
    // 256, 256, 128, 64 samples.
    assert_eq!((frame_len(512), frame_len(1_024)), (1_216, 2_240));
    assert_eq!(
        (4..=9).map(stored_log2).collect::<Vec<_>>(),
        [6, 7, 8, 8, 9, 10]
    );
    assert_eq!(top_of(16 * 1_216, 16), Some(512));
    assert_eq!(top_of(3 * 2_240, 3), Some(1_024));
    assert_eq!(top_of(16 * 1_216 - 2, 16), None);
    assert_eq!(top_of(1_216, 2), None);
    assert_eq!(level_offset(512, 8), 0);
    assert_eq!(level_offset(512, 7), 512);
    assert_eq!(level_offset(512, 4), 1_216 - 64);
    assert_eq!(level_offset(1_024, 9), 0);
    assert_eq!(level_offset(1_024, 4), 2_240 - 64);
    // Hysteresis on the nominal step: drops past ALIAS_STEP, rises under
    // RISE_STEP.
    assert_eq!(next_level(7, 1.42, 512), 7);
    assert_eq!(next_level(7, 1.44, 512), 6);
    assert_eq!(next_level(7, 0.36, 512), 7);
    assert_eq!(next_level(7, 0.34, 512), 8);
    assert_eq!(next_level(8, 0.34, 512), 8, "never above the top");
    assert_eq!(next_level(8, 0.34, 1_024), 9);
    assert_eq!(next_level(4, 9.0, 512), 4, "never below 16");
    assert_eq!(next_level(9, 5.9, 1_024), 6, "a big step drops in one go");
    assert_eq!(note_on_level(187.0, 512, SR), 256);
    assert_eq!(note_on_level(188.0, 512, SR), 128);
    assert_eq!(note_on_level(93.0, 1_024, SR), 512);
    assert_eq!(note_on_level(13_000.0, 512, SR), 16);
}

/// Every note, top 512 and 1 024 (nominal 256 and 512): the note-on level
/// is the largest whose step is at most 1.
#[test]
fn transwave_level_is_largest() {
    for top in [512u32, 1_024] {
        let held = held_transwave(top, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
        let v = SourceView::strict(&held).unwrap();
        let p = wave(TapeEngineParams::default());
        for note in 0..=127u8 {
            let (_, probes) = run(&v, &p, note, &|_| 0.0, 1);
            let want = largest(hz(note, 0.0), top / 2);
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
    // F♯3: its note-on step at level 256 is 0.987.
    let note = 54;
    let s0 = 256.0 * hz(note, 0.0) / SR as f64;
    assert!(s0 > 0.955 && s0 <= 1.0, "{s0}");
    let blocks = 2 * SR as usize / B;
    let vib = |n: usize| 700.0 * (core::f32::consts::TAU * 2.0 * (n * B) as f32 / SR as f32).sin();
    let (_, probes) = run(&v, &p, note, &vib, blocks);
    assert_eq!(probes[0].level, 8);
    assert!(
        probes.iter().any(|q| q.level != 8),
        "note {note}: ±7 st never changed the level"
    );
}

/// Cents on note 24 where nominal level `2^l` steps `step`.
fn cents_at(step: f64, l: u32) -> f32 {
    (1_200.0 * (step * SR as f64 / ((1u64 << l) as f64 * hz(24, 0.0))).log2()) as f32
}

/// The share of `x`'s energy (dB) off the harmonics of `f0`: Blackman-
/// Harris, ±5 bins about each harmonic left out, and DC.
fn off_harmonic_db(x: &[f32], f0: f64) -> f64 {
    let pw = power_bh(x);
    let bins = x.len() as f64 / SR as f64;
    let mut known = vec![false; pw.len()];
    let mut h = 0.0;
    while h * f0 <= SR as f64 / 2.0 {
        let k = (h * f0 * bins).round() as i64;
        for j in (k - 5).max(0)..=(k + 5).min(pw.len() as i64 - 1) {
            known[j as usize] = true;
        }
        h += 1.0;
    }
    let total: f64 = pw.iter().sum();
    let off: f64 = (0..pw.len()).filter(|&k| !known[k]).map(|k| pw[k]).sum();
    10.0 * (off / total).log10()
}

/// A 16-frame saw transwave swept C1 → C8 and back by a pitch
/// route, held every semitone and at each level's worst steps (just under
/// `ALIAS_STEP` going up, just over `RISE_STEP` coming down).
///
/// Everything off the note's harmonics, aliases and the Hermite read's
/// images alike, stays 60 dB under the whole, at every level, both ways.
/// (With each level stored at its nominal length it reached −32 dB at
/// level 16.) In every level change, the level faded to steps at most
/// `ALIAS_STEP` across the block, and the one faded from no more than the
/// block's own pitch move past it. Nothing is asserted above C8. The
/// levels are printed: `-- --nocapture`.
#[test]
fn transwave_is_alias_free() {
    for top in [512u32, 1_024] {
        alias_free_at(top);
    }
}

fn alias_free_at(top: u32) {
    let hi = top.trailing_zeros() - 1;
    // A pure saw, every harmonic at 1/h: the worst case for the images.
    let held = held_transwave(top, 16, |_, h| 0.5 / h as f32);
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams {
        position: 0.5,
        ..TapeEngineParams::default()
    });
    let note = 24u8;
    let (glide, len) = (8usize, 65_536usize);
    let mut e = fresh();
    e.note_on(note, &p, &v, SR);
    let mut b = [0.0; B];
    let mut cents = 0.0f32;
    let mut levels = std::collections::BTreeSet::new();
    let mut up: Vec<f32> = (0..=84).map(|k| k as f32 * 100.0).collect();
    let mut down = up.clone();
    for l in 4..=hi {
        up.push(cents_at(1.425, l));
        down.push(cents_at(0.355, l));
    }
    up.retain(|&c| (0.0..=8_400.0).contains(&c));
    down.retain(|&c| (0.0..=8_400.0).contains(&c));
    up.sort_by(f32::total_cmp);
    down.sort_by(|a, b| b.total_cmp(a));
    let mut worst = -200.0f64;
    for target in up.into_iter().chain(down) {
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
        let f0 = hz(note, cents);
        let db = off_harmonic_db(&out[out.len() - len..], f0);
        worst = worst.max(db);
        eprintln!(
            "top {top}, {cents:7.1} cents, level {}, step {:.3}: off-harmonic {db:.1} dB",
            q.level,
            (1u64 << q.level) as f64 * f0 / SR as f64,
        );
        assert!(
            db < -60.0,
            "{cents} cents (level {}): off-harmonic energy {db:.1} dB",
            q.level
        );
    }
    eprintln!("top {top}: worst {worst:.1} dB");
    assert_eq!(
        levels.into_iter().collect::<Vec<_>>(),
        (4..=hi as u8).collect::<Vec<_>>(),
        "every level heard"
    );
}

/// Nominal level `2^l`'s stored length, samples.
fn stored_len(l: u32) -> f64 {
    (1u64 << stored_log2(l)) as f64
}

/// The Hermite read's (Catmull-Rom's) gain for a harmonic at `nu` cycles a
/// stored sample: its kernel's Fourier transform.
fn hermite_gain(nu: f64) -> f64 {
    let k = |t: f64| {
        let t = t.abs();
        if t < 1.0 {
            1.5 * t * t * t - 2.5 * t * t + 1.0
        } else if t < 2.0 {
            -0.5 * t * t * t + 2.5 * t * t - 4.0 * t + 2.0
        } else {
            0.0
        }
    };
    let n = 4_000;
    let dt = 4.0 / n as f64;
    (0..n)
        .map(|i| {
            let t = -2.0 + (i as f64 + 0.5) * dt;
            k(t) * (core::f64::consts::TAU * nu * t).cos() * dt
        })
        .sum()
}

/// The block a level changes in, looked at alone: held just under the
/// change, one block's move of a few cents across it, then held, on a saw.
/// Over the eight blocks round it the output is the band-limited saw the
/// levels hold, the harmonics only the old level has fading out (or only
/// the new one has fading in) linearly across the change's block, read at
/// the engine's own phase, each harmonic taken through the Hermite read's
/// passband at its level's stored length: the residual stays 60 dB under
/// it, for every drop and rise. A fold, an image or a switch without its fade would sit
/// in that residual; a spectrum can't separate them from the fade's own
/// spread of the harmonics it moves.
#[test]
fn level_change_block_is_alias_free() {
    let held = held_transwave(512, 1, |_, h| 0.5 / h as f32);
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    let note = 24u8;
    // (level before, cents held, cents after, level after): drops at 1.43,
    // rises at 0.35.
    let mut cases = Vec::new();
    for l in 5..=8 {
        cases.push((l, cents_at(1.425, l), cents_at(1.435, l), l - 1));
    }
    for l in 4..=7 {
        cases.push((l, cents_at(0.355, l), cents_at(0.345, l), l + 1));
    }
    let ramp = 64usize;
    for (l, before, after, to) in cases {
        // Into level `l` from a note-on step under 1 there, then across.
        let start = cents_at(if to < l { 0.9 } else { 0.6 }, l);
        let pitch = |n: usize| {
            if n < ramp {
                start + (before - start) * n as f32 / ramp as f32
            } else if n < 2 * ramp {
                before
            } else {
                after
            }
        };
        let (out, probes) = run(&v, &p, note, &pitch, 3 * ramp);
        // A drop comes in the block the pitch rises in; a rise the block
        // after, the step's larger end being the block's start.
        let at = if to < l { 2 * ramp } else { 2 * ramp + 1 };
        assert_eq!(probes[at - 1].level as u32, l, "{l} → {to}");
        assert_eq!(probes[at].from, Some(l as u8), "{l} → {to}");
        assert_eq!(probes[at].level as u32, to);
        // The engine's phase, as it keeps it: a u32 cycle, its step ramped
        // across each block from the last block's fundamental.
        let cyc = |n: usize| wave::cycles(note, pitch(n), SR);
        let (mut ph, mut want) = (0u32, Vec::new());
        let (keep, moved) = {
            let (lo, hi) = (l.min(to), l.max(to));
            let h = wave::harmonics;
            (h(lo), h(hi))
        };
        let pass = [l, to].map(|l| {
            (0..=moved)
                .map(|h| hermite_gain(h as f64 / stored_len(l)))
                .collect::<Vec<_>>()
        });
        for n in 0..3 * ramp {
            let c0 = if n == 0 { cyc(0) } else { cyc(n - 1) };
            let c1 = cyc(n);
            let mut d = (c0 * 4_294_967_296.0) as u32;
            let dd = (((c1 - c0) * 4_294_967_296.0) as i64 / B as i64) as u32;
            for i in 0..B {
                let x = ph as f64 / 4_294_967_296.0 * core::f64::consts::TAU;
                // The moving harmonics' weight: the old level's before the
                // change block, fading across it.
                let g = match n.cmp(&at) {
                    core::cmp::Ordering::Less => 0.0,
                    core::cmp::Ordering::Equal => (i + 1) as f64 / B as f64,
                    core::cmp::Ordering::Greater => 1.0,
                };
                // The old level's share, and each level's passband.
                let w_old = 1.0 - g;
                let y: f64 = (1..=moved)
                    .map(|h| {
                        let a = 0.5 / h as f64 * (h as f64 * x).sin();
                        let in_old = h <= if to < l { moved } else { keep };
                        let in_new = h <= if to < l { keep } else { moved };
                        let mut gain = 0.0;
                        if in_old {
                            gain += w_old * pass[0][h as usize];
                        }
                        if in_new {
                            gain += g * pass[1][h as usize];
                        }
                        gain * a
                    })
                    .sum();
                want.push(y);
                d = d.wrapping_add(dd);
                ph = ph.wrapping_add(d);
            }
        }
        let (lo, hi) = ((at - 4) * B, (at + 4) * B);
        let err: f64 = (lo..hi).map(|i| (out[i] as f64 - want[i]).powi(2)).sum();
        let all: f64 = (lo..hi).map(|i| want[i] * want[i]).sum();
        let db = 10.0 * (err / all).log10();
        eprintln!("{l} → {to}: residual {db:.1} dB");
        assert!(db < -60.0, "{l} → {to}: residual {db:.1} dB");
    }
}

/// A glide into the clamp: the output fades to silence from 0.3 to 0.45
/// cycles a sample and stays silent above, with no tone left at the top of
/// the band and no DC held (a harmonic-2 frame at a frozen phase would
/// hold one); the phase keeps moving.
#[test]
fn nyquist_fades_never_freezes() {
    for h in [1u32, 2] {
        let held = held_transwave(512, 1, |_, k| if k == h { 0.9 } else { 0.0 });
        let v = SourceView::strict(&held).unwrap();
        let p = wave(TapeEngineParams::default());
        // Note 100 (2.6 kHz) up 5 octaves over 2 s.
        let blocks = 2 * SR as usize / B;
        let glide = |n: usize| 6_000.0 * (n as f32 / blocks as f32).min(1.0);
        let (out, _) = run(&v, &p, 100, &glide, blocks + 100);
        let cyc = |n: usize| hz(100, glide(n)) / SR as f64;
        for n in 0..blocks + 100 {
            let x = &out[n * B..(n + 1) * B];
            let peak = x.iter().fold(0.0f32, |m, y| m.max(y.abs()));
            let (lo, hi) = x
                .iter()
                .fold((1.0f32, -1.0f32), |(a, b), &y| (a.min(y), b.max(y)));
            if n > 0 && cyc(n - 1) >= CYCLES_MAX as f64 {
                assert_eq!(peak, 0.0, "harmonic {h}, block {n}: silent at the clamp");
            }
            // Sounding, it swings both ways: never a held value.
            assert!(
                peak < 1e-3 || (lo < 0.0 && hi > 0.0 && hi - lo > peak),
                "harmonic {h}, block {n}: held at {lo}..{hi}"
            );
        }
        let at = |c: f64| (0..blocks).find(|&n| cyc(n) >= c).unwrap();
        // Faded: well under full scale past 0.4 cycles, full under 0.3.
        let peak = |n: usize| {
            out[n * B..(n + 1) * B]
                .iter()
                .fold(0.0f32, |m, y| m.max(y.abs()))
        };
        assert!(peak(at(0.42)) < 0.25, "harmonic {h}: {}", peak(at(0.42)));
        assert!(peak(at(0.05)) > 0.8, "harmonic {h}: {}", peak(at(0.05)));
    }
    assert_eq!(nyquist_gain(FADE_CYCLES), 1.0);
    assert_eq!(nyquist_gain(CYCLES_MAX), 0.0);
    // A jump from 0.25 cycles past the clamp in one block: the gain ramps
    // down across that block, never stepping to 0 at its start.
    let held = held_transwave(512, 1, |_, k| if k == 1 { 0.9 } else { 0.0 });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    let c0 = 0.25 * SR as f64;
    let note = 69u8;
    let base = (1_200.0 * (c0 / hz(note, 0.0)).log2()) as f32;
    let jump = |n: usize| if n < 20 { base } else { base + 1_200.0 };
    let (out, _) = run(&v, &p, note, &jump, 22);
    let rms = |x: &[f32]| (x.iter().map(|y| y * y).sum::<f32>() / x.len() as f32).sqrt();
    let (before, start) = (rms(&out[19 * B..20 * B]), rms(&out[20 * B..20 * B + 16]));
    assert!(
        start > 0.5 * before,
        "the jump block starts at {start}, before {before}"
    );
    assert!(out[21 * B..].iter().all(|&y| y == 0.0));
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
/// 60th harmonic that level 256 has and 128 doesn't, so a level that
/// switched without its fade would step.
#[test]
fn level_change_crossfades_one_block() {
    let held = held_transwave(512, 1, |_, h| match h {
        1 => 0.3,
        60 => 0.5,
        _ => 0.0,
    });
    let v = SourceView::strict(&held).unwrap();
    let p = wave(TapeEngineParams::default());
    // F3: step 0.93 at level 256, so +12 st drops and −12 st rises.
    let note = 53;
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
    let plain = held_plain(&sine(48_000, 100.0, 1_216), 48_000);
    let pv = SourceView::strict(&plain).unwrap();
    let tw = held_transwave(512, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
    let tv = SourceView::strict(&tw).unwrap();
    let cut = &tw[..tw.len() - 3];
    let cv = SourceView::strict(cut).unwrap();
    assert!(!playable(TapeMachine::Wave, &pv));
    assert!(playable(TapeMachine::Wave, &tv));
    assert!(
        !playable(TapeMachine::Wave, &cv),
        "frames short of its count"
    );
    for m in TapeMachine::ALL {
        if m != TapeMachine::Wave {
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
