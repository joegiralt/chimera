// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! TAPE STRETCH GRAIN (ADR 0204): a key plays the source at its pitch over
//! LENGTH seconds, in four overlapping grains after Mutable Instruments
//! Beads; SPRAY scatters them; END HOLD parks at the end, STOP fades out;
//! SPEED 0 freezes.

// TAPE's DSP as bank 1 builds it: under `bank2` the engine is the door's
// proxy (tape_proxy_test), and none of this links (#811).
#![cfg(not(feature = "bank2"))]

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::{DESC_LEN, SourceView};
use chimera_core::dsp::tape_engine::stretch::units::StretchEnd;
use chimera_core::dsp::tape_engine::stretch::{HALF_SPEED, HOLD_S, time_speed};
use chimera_core::dsp::tape_engine::{
    STRETCH_DEFAULTS, StretchKnobs, StretchType, TapeEngine, TapeEngineParams, TapeMachine, Way,
};
use chimera_hal::BLOCK_SIZE;
use common::tape::held_plain;

const SR: u32 = 48_000;
const FS: f32 = SR as f32;
const B: usize = BLOCK_SIZE;
/// C4's period at 48 kHz, frames: the sources' YIN reading.
const PERIOD: f32 = 48_000.0 / 261.625_58;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    unsafe { slot.assume_init() }
}

/// LENGTH's knob for `secs`: `0.25 · 32^x`.
fn length(secs: f32) -> f32 {
    (secs / 0.25).log2() / 5.0
}

/// STRETCH over `secs`, SPRAY `spray`, END STOP or HOLD; ROOT at C4.
fn stretch(secs: f32, spray: f32, stop: bool) -> TapeEngineParams {
    let mut p = TapeEngineParams {
        machine: TapeMachine::Stretch,
        kind: StretchType::Grain,
        motion: 0.5,
        root: 6_000.0,
        period: PERIOD,
        ..TapeEngineParams::default()
    };
    p.stretch = StretchKnobs {
        length: length(secs),
        grain: STRETCH_DEFAULTS.grain,
        spray,
        finish: if stop { 1.0 } else { 0.0 },
    };
    p
}

/// ONE on the same source: the reference level.
fn one() -> TapeEngineParams {
    TapeEngineParams {
        drag: 1.0,
        root: 6_000.0,
        ..TapeEngineParams::default()
    }
    .with_way(Way::One)
}

/// `secs` of `p(block)` from a note on `note` (60 is the source's own
/// pitch), key held.
fn play(
    p: &dyn Fn(usize) -> TapeEngineParams,
    v: &SourceView<'_>,
    note: u8,
    secs: f32,
) -> Vec<f32> {
    let mut e = fresh();
    e.seed(3);
    e.note_on(note, &p(0), v, SR);
    let mut out = Vec::new();
    let mut b = [0.0; B];
    for n in 0..(secs * FS) as usize / B {
        e.render(&mut b, &p(n), v, SR);
        out.extend_from_slice(&b);
    }
    out
}

/// A bell: inharmonic partials over C4, each decaying so the whole falls
/// 30 dB over `secs`.
fn bell(secs: f32) -> Vec<f32> {
    let f0 = 261.625_58;
    let parts = [(1.0, 0.5), (2.756, 0.25), (5.404, 0.15), (8.933, 0.08)];
    let k = (10f32.powf(-30.0 / 20.0)).ln() / secs;
    (0..(secs * FS) as usize)
        .map(|i| {
            let t = i as f32 / FS;
            (k * t).exp()
                * parts
                    .iter()
                    .map(|&(m, a)| a * (core::f32::consts::TAU * f0 * m * t).sin())
                    .sum::<f32>()
        })
        .collect()
}

/// A 0.4 saw at C4, `secs` long.
fn saw(secs: f32) -> Vec<f32> {
    let f0 = 261.625_58;
    (0..(secs * FS) as usize)
        .map(|i| 0.4 * (2.0 * (i as f32 * f0 / FS).fract() - 1.0))
        .collect()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn frames(x: &[f32], ms: f32) -> Vec<f32> {
    x.chunks((ms * FS / 1000.0) as usize).map(rms).collect()
}

/// From the loudest 10 ms frame until one sits `d` dB under it, seconds.
fn fall(x: &[f32], d: f32) -> f32 {
    let f = frames(x, 10.0);
    let (i, p) = f
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
    f[i..]
        .iter()
        .position(|&v| db(v) < db(p) - d)
        .map_or(f32::INFINITY, |k| (i + k) as f32 * 0.01)
}

/// The splice tremolo: the RMS of 10 ms frames' dB about a 0.2 s moving
/// mean, over `[t0, t1)` s.
fn ripple(x: &[f32], t0: f32, t1: f32) -> f32 {
    let f: Vec<f32> = frames(x, 10.0).iter().map(|&v| db(v)).collect();
    let (a, b) = ((t0 * 100.0) as usize, ((t1 * 100.0) as usize).min(f.len()));
    let mut acc = (0.0, 0);
    for i in a.max(10)..b.saturating_sub(10) {
        let m = f[i - 10..i + 10].iter().sum::<f32>() / 20.0;
        acc = (acc.0 + (f[i] - m).powi(2), acc.1 + 1);
    }
    (acc.0 / acc.1.max(1) as f32).sqrt()
}

/// The strongest frequency within ±60 cents of `want` over `x`, Hann
/// windowed, on a 0.25-cent grid.
fn peak_hz(x: &[f32], want: f32) -> f32 {
    let n = x.len();
    let w: Vec<f32> = (0..n)
        .map(|i| x[i] * (0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / n as f32).cos()))
        .collect();
    let mut best = (0.0f32, want);
    for k in -240..=240 {
        let f = want * 2f32.powf(k as f32 / 4.0 / 1200.0);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let d = core::f64::consts::TAU * f as f64 / FS as f64;
        for (i, &v) in w.iter().enumerate() {
            re += v as f64 * (d * i as f64).cos();
            im += v as f64 * (d * i as f64).sin();
        }
        let m = (re * re + im * im) as f32;
        if m > best.0 {
            best = (m, f);
        }
    }
    best.1
}

fn cents(f: f32, want: f32) -> f32 {
    1200.0 * (f / want).log2()
}

#[test]
fn laws() {
    for (m, s) in [
        (0.0, 0.0),
        (HALF_SPEED, 0.5),
        (0.5, 1.0),
        (1.0, 2.0),
        (-1.0, -1.0),
    ] {
        assert!((time_speed(m) - s).abs() < 1e-4, "MOTION {m}");
    }
    // The laws at their ends and the defaults; the types hold the range.
    let secs = |p: &TapeEngineParams| (p.length().seconds().get(), p.grain().seconds().get());
    let mut p = TapeEngineParams {
        stretch: StretchKnobs {
            length: 0.0,
            grain: 0.0,
            spray: 0.0,
            finish: 0.0,
        },
        ..TapeEngineParams::default()
    };
    let (l, g) = secs(&p);
    assert!((l - 0.25).abs() < 1e-6 && (g - 0.03).abs() < 1e-6);
    assert_eq!(p.end_mode(), StretchEnd::Hold);
    p.stretch = StretchKnobs {
        length: 1.0,
        grain: 1.0,
        spray: 1.0,
        finish: 1.0,
    };
    let (l, g) = secs(&p);
    assert!((l - 8.0).abs() < 1e-3 && (g - 0.25).abs() < 1e-4);
    assert_eq!(p.end_mode(), StretchEnd::Stop);
    assert!((p.spray().reach() - 0.025).abs() < 1e-7);
    let (l, g) = secs(&TapeEngineParams::default());
    assert!((l - 2.0).abs() < 0.02, "{l}");
    assert!((g - 0.08).abs() < 0.001, "{g}");
}

/// The key's pitch at every LENGTH, on a bell's strongest partial and a
/// saw's fundamental, at the source's own key, a fifth and an octave up.
#[test]
fn pitch_holds_at_every_length() {
    let f0 = 261.625_58;
    for (name, src) in [("bell", bell(1.25)), ("saw", saw(1.25))] {
        let held = held_plain(&src, 48_000);
        let v = SourceView::new(&held).unwrap();
        for secs in [0.5f32, 1.25, 2.0, 4.0, 8.0] {
            for note in [60u8, 67, 72] {
                let want = f0 * 2f32.powf((note as f32 - 60.0) / 12.0);
                let x = play(
                    &|_| stretch(secs, STRETCH_DEFAULTS.spray, false),
                    &v,
                    note,
                    chimera_core::dsp::clampf(secs, 0.5, 1.5),
                );
                let mid = &x[(0.1 * FS) as usize..x.len() - (0.05 * FS) as usize];
                let c = cents(peak_hz(mid, want), want);
                eprintln!("{name} {secs} s note {note}: {c:+.2} c");
                assert!(c.abs() < 4.0, "{name} {secs} s note {note}: {c:+.2} c");
            }
        }
    }
}

/// A 1.25 s bell (−30 dB at its end) stretched falls 30 dB at LENGTH,
/// ±5 %, at its own key and an octave up.
#[test]
fn length_is_kept() {
    let held = held_plain(&bell(1.25), 48_000);
    let v = SourceView::new(&held).unwrap();
    let nat = fall(&play(&|_| one(), &v, 60, 2.0), 30.0);
    assert!((nat - 1.25).abs() < 0.05, "ONE falls 30 dB at {nat}");
    for secs in [1.25f32, 2.0, 4.0, 8.0] {
        for note in [60u8, 72] {
            let x = play(
                &|_| stretch(secs, STRETCH_DEFAULTS.spray, true),
                &v,
                note,
                secs + 1.0,
            );
            let t = fall(&x, 30.0);
            let err = t / secs - 1.0;
            eprintln!(
                "{secs} s note {note}: −30 dB at {t:.3} s ({:+.1} %)",
                100.0 * err
            );
            assert!(err.abs() < 0.05, "{secs} s note {note}: −30 dB at {t}");
        }
    }
}

/// At 1× (LENGTH the source's, SPEED 1×) with no SPRAY every grain reads
/// the pointer's sample: ONE's note but the reads' rounding; with SPRAY,
/// within half a dB of its level.
#[test]
fn one_times_is_the_source() {
    for src in [bell(1.25), saw(1.25)] {
        let held = held_plain(&src, 48_000);
        let v = SourceView::new(&held).unwrap();
        let a = play(&|_| one(), &v, 60, 1.2);
        let b = play(&|_| stretch(1.25, 0.0, true), &v, 60, 1.2);
        let n = (1.1 * FS) as usize;
        let d: Vec<f32> = (0..n).map(|i| a[i] - b[i]).collect();
        let null = db(rms(&d)) - db(rms(&a[..n]));
        eprintln!("null {null:.1} dB");
        assert!(null < -40.0, "null {null:.1} dB");
        let c = play(
            &|_| stretch(1.25, STRETCH_DEFAULTS.spray, true),
            &v,
            60,
            1.2,
        );
        let dl = db(rms(&c[..n])) - db(rms(&a[..n]));
        eprintln!("SPRAY's level {dl:+.2} dB");
        assert!(dl.abs() < 0.5, "SPRAY's level {dl:+.2} dB");
    }
}

/// The level holds stretched: a saw's RMS within 1 dB of ONE's at every
/// LENGTH and SPRAY, the grains in phase or not.
#[test]
fn level_holds_stretched() {
    let held = held_plain(&saw(1.25), 48_000);
    let v = SourceView::new(&held).unwrap();
    let a = rms(&play(&|_| one(), &v, 60, 1.0)[(0.1 * FS) as usize..]);
    for secs in [2.0, 4.0, 8.0] {
        for spray in [0.0, STRETCH_DEFAULTS.spray, 1.0] {
            let x = play(&|_| stretch(secs, spray, false), &v, 60, 1.0);
            let dl = db(rms(&x[(0.1 * FS) as usize..])) - db(a);
            eprintln!("{secs} s SPRAY {spray}: {dl:+.2} dB");
            assert!(dl.abs() < 1.0, "{secs} s SPRAY {spray}: {dl:+.2} dB");
        }
    }
}

/// The grains' overlap never trembles: under half a dB of splice ripple on
/// the bell at the default GRAIN, every LENGTH and SPRAY.
#[test]
fn splices_are_smooth() {
    let held = held_plain(&bell(1.25), 48_000);
    let v = SourceView::new(&held).unwrap();
    for secs in [2.0f32, 4.0, 8.0] {
        for spray in [0.0, STRETCH_DEFAULTS.spray] {
            let x = play(&|_| stretch(secs, spray, false), &v, 60, secs);
            let r = ripple(&x, 0.1, 0.9 * secs);
            eprintln!("{secs} s SPRAY {spray}: ripple {r:.3} dB");
            assert!(r < 0.5, "{secs} s SPRAY {spray}: ripple {r:.2} dB");
        }
    }
}

/// No step bigger than the source's own steepest, the strike's onset and
/// the fade out included: a sine stretched, held at the end and stopped.
#[test]
fn no_clicks() {
    let f = 440.0;
    let src: Vec<f32> = (0..(1.0 * FS) as usize)
        .map(|i| 0.5 * (core::f32::consts::TAU * f * i as f32 / FS).sin())
        .collect();
    let held = held_plain(&src, 48_000);
    let v = SourceView::new(&held).unwrap();
    let slope = 0.5 * core::f32::consts::TAU * f / FS;
    for (secs, stop) in [(0.5, true), (3.0, true), (3.0, false), (8.0, true)] {
        for spray in [0.0, STRETCH_DEFAULTS.spray, 1.0] {
            let x = play(&|_| stretch(secs, spray, stop), &v, 60, secs + 0.5);
            let m = x
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0, f32::max);
            eprintln!(
                "{secs} s SPRAY {spray}: step {:.3} of the sine's",
                m / slope
            );
            assert!(
                m < 1.6 * slope,
                "{secs} s SPRAY {spray}: step {m} ({slope})"
            );
        }
    }
}

/// GRAIN turned from its top to its foot mid-note: the new short grains
/// space by the long ones still sounding, so none is cut (`steal: None`).
#[test]
fn grain_turned_short_cuts_no_grain() {
    let f = 440.0;
    let src: Vec<f32> = (0..(1.0 * FS) as usize)
        .map(|i| 0.5 * (core::f32::consts::TAU * f * i as f32 / FS).sin())
        .collect();
    let held = held_plain(&src, 48_000);
    let v = SourceView::new(&held).unwrap();
    let slope = 0.5 * core::f32::consts::TAU * f / FS;
    let turn = (0.5 * FS) as usize / B;
    let p = |n: usize| {
        let mut p = stretch(3.0, 0.0, true);
        p.stretch.grain = if n < turn { 1.0 } else { 0.0 };
        p
    };
    let x = play(&p, &v, 60, 1.5);
    let m = x
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(m < 1.6 * slope, "step {:.2} of the sine's", m / slope);
}

/// END HOLD parks at the end and keeps sounding while held, 8 s at most;
/// STOP ends the note soon after LENGTH.
#[test]
fn end_holds_or_stops() {
    let held = held_plain(&saw(1.0), 48_000);
    let v = SourceView::new(&held).unwrap();
    for stop in [false, true] {
        let p = stretch(1.0, STRETCH_DEFAULTS.spray, stop);
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let mut b = [0.0; B];
        let mut ended = None;
        let mut late = 0.0f32;
        for n in 0..(12.0 * FS) as usize / B {
            e.render(&mut b, &p, &v, SR);
            let t = (n * B) as f32 / FS;
            if (2.0..2.5).contains(&t) {
                late = late.max(rms(&b));
            }
            if !e.is_active() && ended.is_none() {
                ended = Some(t);
            }
        }
        let ended = ended.expect("the note ends");
        if stop {
            assert!(ended < 1.3, "STOP ends at {ended}");
            assert_eq!(late, 0.0);
        } else {
            assert!(db(late) > -20.0, "HOLD sounds at 2 s: {}", db(late));
            assert!((ended - (1.0 + HOLD_S)).abs() < 0.4, "HOLD ends at {ended}");
        }
    }
}

/// SPEED 0 freezes: the pointer stays where it was, the sound goes on at
/// its level.
#[test]
fn speed_0_freezes() {
    let held = held_plain(&saw(1.0), 48_000);
    let v = SourceView::new(&held).unwrap();
    let p = |n: usize| {
        let mut p = stretch(1.0, STRETCH_DEFAULTS.spray, true);
        p.motion = if n * B < (0.3 * FS) as usize {
            0.5
        } else {
            0.0
        };
        p
    };
    let mut e = fresh();
    e.note_on(60, &p(0), &v, SR);
    let mut b = [0.0; B];
    let mut heads = Vec::new();
    let mut out = Vec::new();
    for n in 0..(4.0 * FS) as usize / B {
        e.render(&mut b, &p(n), &v, SR);
        heads.push(e.probe().head);
        out.extend_from_slice(&b);
    }
    assert!(e.is_active());
    let h = &heads[heads.len() / 2..];
    assert!(h.iter().all(|&x| x == h[0]), "the pointer moved");
    let r = ripple(&out, 1.0, 3.9);
    assert!(r < 0.5, "frozen ripple {r}");
    assert!(db(rms(&out[(3.0 * FS) as usize..])) > -20.0);
}

/// NaN or out-of-range params play finite samples.
#[test]
fn nan_is_safe() {
    let held = held_plain(&saw(0.5), 48_000);
    let v = SourceView::new(&held).unwrap();
    for bad in [f32::NAN, f32::INFINITY, -1.0, 9.0] {
        let mut p = stretch(2.0, 0.5, false);
        p.stretch = StretchKnobs {
            length: bad,
            grain: bad,
            spray: bad,
            finish: bad,
        };
        p.motion = bad;
        let x = play(&|_| p, &v, 60, 0.5);
        assert!(x.iter().all(|s| s.is_finite() && s.abs() < 2.0), "{bad}");
    }
}

/// END before START walks the window backward: the pointer moves down.
#[test]
fn reversed_walks_down() {
    let held = held_plain(&saw(1.0), 48_000);
    let v = SourceView::new(&held).unwrap();
    let mut p = stretch(2.0, STRETCH_DEFAULTS.spray, true);
    (p.start, p.end) = (1.0, 0.0);
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let mut b = [0.0; B];
    let mut heads = Vec::new();
    for _ in 0..(1.0 * FS) as usize / B {
        e.render(&mut b, &p, &v, SR);
        heads.push(e.probe().head);
    }
    assert!(heads.windows(2).all(|w| w[1] <= w[0]));
    assert!(
        heads[0] > 40_000 && *heads.last().unwrap() < 30_000,
        "{} → {}",
        heads[0],
        heads.last().unwrap()
    );
}

/// An unpitched source (no PERIOD) takes SPRAY's scatter alone: a bell
/// keeps its pitch and its splices stay smooth enough at the default.
#[test]
fn unpitched_bell_keeps_pitch() {
    let f0 = 261.625_58;
    let held = held_plain(&bell(1.25), 48_000);
    let v = SourceView::new(&held).unwrap();
    for secs in [2.0f32, 4.0, 8.0] {
        for spray in [0.0, STRETCH_DEFAULTS.spray] {
            let p = TapeEngineParams {
                period: 0.0,
                ..stretch(secs, spray, false)
            };
            let x = play(&|_| p, &v, 60, secs.min(4.0));
            let mid = &x[(0.1 * FS) as usize..x.len() - (0.05 * FS) as usize];
            let c = cents(peak_hz(mid, f0), f0);
            let r = ripple(&x, 0.1, 0.9 * secs.min(4.0));
            eprintln!("unpitched {secs} s SPRAY {spray}: {c:+.2} c, ripple {r:.3} dB");
        }
    }
}

/// A row's four taps in two words are its samples one by one, at every
/// frame but the last (which reads as the one before).
#[test]
fn row_taps_are_the_samples() {
    let mut rng = common::XorShift(9);
    let src: Vec<f32> = (0..1_000)
        .map(|_| rng.next_u32() as f32 / u32::MAX as f32 * 1.9 - 0.95)
        .collect();
    let held = held_plain(&src, 48_000);
    let v = SourceView::strict(&held).unwrap();
    let n = v.frames();
    let row = v.whole_row().unwrap();
    for i in 1..n - 3 {
        let want = [0u32, 1, 2, 3].map(|k| v.sample(i + k - 1));
        assert_eq!(row.row_taps(i), want, "frame {i}");
    }
    assert_eq!(row.row_taps(n - 2), row.row_taps(n - 4));
    assert_eq!(row.row_taps(0), row.row_taps(1));
}

/// Only a whole row reads `row_taps`: a span or silence is no `WholeRow`.
#[test]
fn spans_and_silence_are_no_whole_row() {
    let src: Vec<f32> = (0..256).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
    let held = held_plain(&src, 48_000);
    let v = SourceView::new(&held).unwrap();
    assert!(v.whole_row().is_some());
    let bytes = &held[DESC_LEN..];
    let at = |first: u32| &bytes[(3 * first / 2) as usize..];
    let span = SourceView::span(at(128), v.desc(), 128, v.frames()).unwrap();
    assert!(span.whole_row().is_none(), "a span from 128");
    let short = SourceView::span(&bytes[..96], v.desc(), 0, v.frames()).unwrap();
    assert!(short.whole_row().is_none(), "a span cut short");
    assert!(
        SourceView::silent(v.desc(), v.file_frames())
            .whole_row()
            .is_none()
    );
    let whole = SourceView::span(bytes, v.desc(), 0, v.frames()).unwrap();
    assert!(
        whole.whole_row().is_some(),
        "a span of the whole row is one"
    );
}

/// GRAIN's cheaper builds against GRAIN as shipped (ADR 0204, as asked): pitch, length and splice ripple, a report, not a gate.
#[test]
#[ignore]
fn thrift_report() {
    use chimera_core::dsp::tape_engine::stretch::grain::{THRIFT, Thrift};
    use core::sync::atomic::Ordering;
    let f0 = 261.625_58;
    let bellv = held_plain(&bell(1.25), 48_000);
    let sawv = held_plain(&saw(1.25), 48_000);
    let (bv, sv) = (
        SourceView::new(&bellv).unwrap(),
        SourceView::new(&sawv).unwrap(),
    );
    for t in Thrift::ALL {
        THRIFT.store(t as u8, Ordering::Relaxed);
        let mut worst_c = 0.0f32;
        let mut worst_len = 0.0f32;
        let mut worst_rip = 0.0f32;
        for secs in [1.25f32, 2.0, 4.0, 8.0] {
            for spray in [0.0, STRETCH_DEFAULTS.spray] {
                for (v, name) in [(&bv, "bell"), (&sv, "saw")] {
                    let x = play(&|_| stretch(secs, spray, false), v, 60, secs.min(2.0));
                    let mid = &x[(0.1 * FS) as usize..x.len() - (0.05 * FS) as usize];
                    let c = cents(peak_hz(mid, f0), f0);
                    worst_c = worst_c.max(c.abs());
                    if name == "bell" {
                        worst_rip = worst_rip.max(ripple(&x, 0.1, 0.9 * secs.min(2.0)));
                    }
                }
                let x = play(&|_| stretch(secs, spray, true), &bv, 60, secs + 1.0);
                worst_len = worst_len.max((fall(&x, 30.0) / secs - 1.0).abs());
            }
        }
        eprintln!(
            "{t:?}: pitch within {worst_c:.2} c, length within {:.1} %, ripple at most {worst_rip:.3} dB",
            100.0 * worst_len
        );
    }
    THRIFT.store(0, Ordering::Relaxed);
}

/// Linear interpolation's loss against the Hermite's, a source's band at
/// 1/8, 1/4 and 3/8 of its rate: what a linear read would cost the top.
#[test]
#[ignore]
fn linear_read_loss() {
    for f in [0.125f64, 0.25, 0.375] {
        let w = core::f64::consts::TAU * f;
        // The two kernels' mean gain over a uniform fractional place.
        let (mut lin, mut her) = (0.0f64, 0.0f64);
        let n = 1000;
        for k in 0..n {
            let t = (k as f64 + 0.5) / n as f64;
            let x = |i: f64| num_complex(w * (i - t));
            // Linear between taps 0 and 1.
            let l = add(scale(x(0.0), 1.0 - t), scale(x(1.0), t));
            // Catmull-Rom (the core's Hermite) on taps −1..=2.
            let (y0, y1, y2, y3) = (x(-1.0), x(0.0), x(1.0), x(2.0));
            let c1 = scale(sub(y2, y0), 0.5);
            let c2 = add(
                sub(add(y0, scale(y2, 2.0)), scale(y1, 2.5)),
                scale(y3, -0.5),
            );
            let c3 = add(scale(sub(y3, y0), 0.5), scale(sub(y1, y2), 1.5));
            let h = add(
                add(add(scale(c3, t * t * t), scale(c2, t * t)), scale(c1, t)),
                y1,
            );
            lin += mag(l);
            her += mag(h);
        }
        eprintln!(
            "f/fs {f}: linear {:+.2} dB, Hermite {:+.2} dB",
            20.0 * (lin / n as f64).log10(),
            20.0 * (her / n as f64).log10()
        );
    }
    fn num_complex(p: f64) -> (f64, f64) {
        (p.cos(), p.sin())
    }
    fn add(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        (a.0 + b.0, a.1 + b.1)
    }
    fn sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        (a.0 - b.0, a.1 - b.1)
    }
    fn scale(a: (f64, f64), k: f64) -> (f64, f64) {
        (a.0 * k, a.1 * k)
    }
    fn mag(a: (f64, f64)) -> f64 {
        (a.0 * a.0 + a.1 * a.1).sqrt()
    }
}

/// With no PERIOD a tone's grains lock to the window's own period (ADR
/// 0204): a sine and a two-note chord hold their level within 3 dB,
/// frozen and at 1×, SPRAY 0 and 0.4, from the window's start and from
/// START ½ (the run-up, every grain at the window's foot), A4 on ROOT C4.
#[test]
fn unpitched_tones_hold_their_level() {
    let n = FS as usize;
    let tau = core::f32::consts::TAU;
    let sine: Vec<f32> = (0..n)
        .map(|i| 0.5 * (tau * i as f32 * 440.0 / FS).sin())
        .collect();
    let chord: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / FS;
            0.25 * (tau * 261.625_58 * t).sin() + 0.25 * (tau * 391.995_44 * t).sin()
        })
        .collect();
    let mut bad = Vec::new();
    for (name, src) in [("sine", &sine), ("chord", &chord)] {
        let held = held_plain(src, 48_000);
        let v = SourceView::new(&held).unwrap();
        let want = rms(&src[..]);
        for (motion, start, spray) in [
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.4),
            (0.5, 0.0, 0.4),
            (0.5, 0.5, 0.0),
        ] {
            let mut p = TapeEngineParams {
                machine: TapeMachine::Stretch,
                kind: StretchType::Grain,
                motion,
                start,
                root: 6_000.0,
                ..TapeEngineParams::default()
            };
            p.stretch = StretchKnobs {
                length: 0.5,
                grain: 0.3,
                spray,
                finish: 0.0,
            };
            let x = play(&|_| p, &v, 69, 1.2);
            // 20 ms frames from 30 ms on: past the strike's own rise.
            let f = frames(&x[(0.03 * FS) as usize..], 20.0);
            let (lo, hi) = f
                .iter()
                .fold((f32::MAX, 0.0f32), |(l, h), &r| (l.min(r), h.max(r)));
            let (lo, hi) = (db(lo) - db(want), db(hi) - db(want));
            eprintln!("{name} SPEED {motion} START {start} SPRAY {spray}: {lo:+.1} .. {hi:+.1} dB");
            if !(lo > -3.0 && hi < 3.0) {
                bad.push(format!("{name} SPEED {motion} START {start} SPRAY {spray}"));
            }
        }
    }
    assert!(bad.is_empty(), "outside 3 dB: {bad:?}");
}
