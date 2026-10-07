//! TAPE's LOOP (spec § 2 The macros and the window, LOOP, Parameters never
//! snap): the pitch law, the folded crossfade, bounds latched at a wrap,
//! the note-on ramp, the macros' easing and MOTION's walk. Precedent:
//! Digitakt II manual § Playback modes; Morphagene manual (Organize).

mod common;

use core::f32::consts::TAU;
use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::wave;
use chimera_core::dsp::tape_engine::{
    INC_MAX, NOTE_ON_RAMP, TapeEngine, TapeEngineParams, TapeModel, inc, loop_len,
};
use chimera_hal::BLOCK_SIZE;
use common::tape::{Mach, held_plain, held_transwave, sine};

const SR: u32 = 48_000;

/// The models that play heads through a window (TRANSWAVE has its own).
/// LOOP either way (END before START plays reversed), and the rest.
const MODELS: [Mach; 5] = [
    Mach::fwd(TapeModel::Loop),
    Mach::rev(TapeModel::Loop),
    Mach::fwd(TapeModel::PingPong),
    Mach::fwd(TapeModel::Scrub),
    Mach::fwd(TapeModel::Stretch),
];

/// `p` as `m` plays it in a loop test: SCRUB's head moves only with
/// MOTION, so it runs at the note's speed.
fn as_model(m: Mach, p: TapeEngineParams) -> TapeEngineParams {
    let motion = if m.model == TapeModel::Scrub {
        0.5
    } else {
        p.motion
    };
    m.params(TapeEngineParams { motion, ..p })
}

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    *TapeEngine::init_in_place(&mut slot)
}

/// `blocks` blocks of `e` under `p`, with each block's probe head.
fn play(
    e: &mut TapeEngine,
    p: &TapeEngineParams,
    v: &SourceView<'_>,
    blocks: usize,
) -> (Vec<f32>, Vec<u32>) {
    let (mut out, mut heads) = (Vec::new(), Vec::new());
    let mut b = [0.0; BLOCK_SIZE];
    for _ in 0..blocks {
        e.render(&mut b, p, v, SR);
        out.extend_from_slice(&b);
        heads.push(e.probe().head);
    }
    (out, heads)
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

/// Times the probe head moved back: the wraps.
fn wraps(heads: &[u32]) -> usize {
    heads.windows(2).filter(|w| w[1] < w[0]).count()
}

/// Times the probe head moved against its usual way: a reverse loop's
/// wraps jump forward.
fn wraps_either(heads: &[u32]) -> usize {
    let back = wraps(heads);
    back.min(heads.len() - 1 - back)
}

/// Rising zero crossings, linearly interpolated: the frequency over `x`.
fn zero_cross_hz(x: &[f32]) -> f64 {
    let mut t = Vec::new();
    for i in 1..x.len() {
        let (a, b) = (x[i - 1] as f64, x[i] as f64);
        if a < 0.0 && b >= 0.0 {
            t.push((i - 1) as f64 + a / (a - b));
        }
    }
    (t.len() - 1) as f64 / (t[t.len() - 1] - t[0]) * SR as f64
}

#[test]
fn plays_in_tune() {
    for rate in [48_000u16, 32_000, 24_000, 16_000, 12_000, 8_000] {
        // 220 Hz at the source's rate, ROOT 6 000 (C4): 5 s, so 1 s at
        // four times its speed never reaches the fade.
        let src = sine(rate, rate as f32 / 220.0, 5 * rate as u32);
        let held = held_plain(&src, rate);
        let v = SourceView::strict(&held).unwrap();
        for note in (36..=84).step_by(12) {
            let p = TapeEngineParams::default();
            let mut e = fresh();
            e.note_on(note, &p, &v, SR);
            let (out, _) = play(&mut e, &p, &v, 1 + SR as usize / BLOCK_SIZE);
            let f = zero_cross_hz(&out[BLOCK_SIZE..]);
            let want = 220.0 * 2f64.powf((note as f64 * 100.0 - 6_000.0) / 1_200.0);
            let cents = 1_200.0 * (f / want).log2();
            assert!(
                cents.abs() < 0.1,
                "{rate} Hz source, note {note}: {cents} cents"
            );
        }
    }
}

#[test]
fn inc_clamps_at_16() {
    assert_eq!(inc(48_000, 127, 2_400.0, 6_000.0, SR), INC_MAX);
    let held = held_plain(&sine(48_000, 100.0, 10_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams::default();
    let mut e = fresh();
    e.set_pitch(2_400.0);
    e.note_on(127, &p, &v, SR);
    let (_, heads) = play(&mut e, &p, &v, 3);
    assert_eq!(heads[2] - heads[1], 16 * BLOCK_SIZE as u32);
}

/// Neutral on a sine whose length isn't a whole number of periods: every
/// wrap crossfades, so no step is more than 4 times the steady sine's.
#[test]
fn init_never_hard_wraps() {
    let held = held_plain(&sine(48_000, 100.7, 4_801), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams::default();
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, heads) = play(&mut e, &p, &v, 3_500);
    assert!(wraps(&heads) >= 50, "{} wraps", wraps(&heads));
    let steady = max_step(&out[BLOCK_SIZE..4_000]);
    let worst = max_step(&out[BLOCK_SIZE..]);
    assert!(worst <= 4.0 * steady, "{worst} against {steady}");
}

#[test]
fn loops_are_click_free() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for m in [
        Mach::fwd(TapeModel::Loop),
        Mach::rev(TapeModel::Loop),
        Mach::fwd(TapeModel::Scrub),
    ] {
        for xfade in [0.1, 0.5] {
            for position in [0.0, 0.5] {
                let p = as_model(
                    m,
                    TapeEngineParams {
                        xfade,
                        position,
                        size: 0.6,
                        ..TapeEngineParams::default()
                    },
                );
                let mut e = fresh();
                e.note_on(60, &p, &v, SR);
                let (out, heads) = play(&mut e, &p, &v, 1_500);
                assert!(
                    wraps_either(&heads) >= 10,
                    "{m:?} XFADE {xfade} POSITION {position}"
                );
                let steady = max_step(&out[BLOCK_SIZE..2_000]);
                let worst = max_step(&out[BLOCK_SIZE..]);
                assert!(
                    worst <= 4.0 * steady,
                    "{m:?} XFADE {xfade} POSITION {position}: {worst} against {steady}"
                );
            }
        }
    }
}

/// The largest bend standing alone: a sample's second difference over the
/// larger of its neighbours' (and `floor`), a corner where a wave bends in
/// one sample, not over several.
fn lone_bend(x: &[f32], floor: f32) -> f32 {
    let d2: Vec<f32> = x
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .collect();
    d2.windows(3)
        .map(|w| w[1] / w[0].max(w[2]).max(floor))
        .fold(0.0, f32::max)
}

/// Tiny loops (SIZE sweeping 0.1–0.4, XFADE 0.1: fades of 10–60 samples)
/// on a sine at C4's period: each wrap's fade lands without a corner
/// (#385). A linear fade bends at its ends by the heads' difference over
/// its length, in one sample (the QA's EPIANO at 8.58 s: 0.005 → −0.010).
#[test]
fn tiny_loop_fades_have_no_corner() {
    let held = held_plain(&sine(48_000, 183.4, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let own = (TAU / 183.4).powi(2);
    for m in [Mach::fwd(TapeModel::Loop), Mach::rev(TapeModel::Loop)] {
        let mut p = as_model(
            m,
            TapeEngineParams {
                size: 0.1,
                ..TapeEngineParams::default()
            },
        );
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let mut out = Vec::new();
        let mut b = [0.0; BLOCK_SIZE];
        for n in 0..1_500 {
            p.size = 0.1 + 0.3 * (n as f32 / 1_500.0);
            e.render(&mut b, &p, &v, SR);
            out.extend_from_slice(&b);
        }
        let lone = lone_bend(&out[BLOCK_SIZE..], 2.0 * own);
        assert!(lone <= 3.0, "{m:?}: a bend {lone}× its neighbours'");
    }
}

#[test]
fn xfade_zero_hard_splices() {
    let held = held_plain(&sine(48_000, 100.7, 4_801), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        xfade: 0.0,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let mut b = [0.0; BLOCK_SIZE];
    let mut heads = Vec::new();
    for _ in 0..500 {
        e.render(&mut b, &p, &v, SR);
        assert_eq!(e.probe().other, None);
        heads.push(e.probe().head);
    }
    assert!(wraps(&heads) >= 5);
    // No second head but at a flip (END moved across START), which fades
    // over the clamp's 5 ms.
    let q = TapeEngineParams { start: 0.5, ..p };
    let mut e = fresh();
    e.note_on(60, &q, &v, SR);
    let mut seconds = 0;
    for n in 0..500 {
        let end = if n < 100 { 1.0 } else { 0.0 };
        e.render(&mut b, &TapeEngineParams { end, ..q }, &v, SR);
        let flip = e.dirs()[0] != e.dirs()[1];
        assert_eq!(e.probe().other.is_some(), flip, "block {n}");
        seconds += usize::from(flip);
    }
    assert!((4..=5).contains(&seconds), "{seconds} blocks fading");
}

/// SIZE 1 → 0.2 mid-pass: the head plays on to the old loop's end, then
/// loops inside the new bounds.
#[test]
fn bounds_latch_at_wrap() {
    let w = 48_000;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams::default();
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    play(&mut e, &p, &v, 375);
    p.size = 0.2;
    let (_, heads) = play(&mut e, &p, &v, 600);
    let wrap = heads.windows(2).position(|h| h[1] < h[0]).expect("a wrap");
    assert!(
        heads[wrap] >= w - BLOCK_SIZE as u32,
        "wrapped at {}",
        heads[wrap]
    );
    let short = loop_len(0.2, w, 48_000);
    assert!(
        heads[wrap + 1..].iter().all(|&h| h < 2 * short),
        "after the wrap: {:?}",
        &heads[wrap + 1..wrap + 8]
    );
}

#[test]
fn note_on_ramps_from_zero() {
    // A peak 100 frames in.
    let held = held_plain(&sine(48_000, 400.0, 4_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        start: 0.025,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, _) = play(&mut e, &p, &v, 1);
    let n = NOTE_ON_RAMP as usize;
    for (k, &y) in out[..n].iter().enumerate() {
        let x = (k + 1) as f32 / n as f32;
        assert!(y.abs() <= x * x * (3.0 - 2.0 * x), "{k}: {y}");
    }
    // The head starts on START's frame, the peak.
    assert!(out[0] == 0.0 && (out[n] - (n as f32 * TAU / 400.0).cos()).abs() < 2e-3);
}

/// The note-on ramp lands without a corner on every machine that loops
/// (#385): a smoothstep, as ONESHOT's. A fast attack (a slow sine at its
/// peak, START a quarter period in) is near full scale when the ramp ends; a linear ramp
/// bends there by `1/NOTE_ON_RAMP` of it (0.02), a smoothstep by at most
/// `6/NOTE_ON_RAMP²` (0.003).
#[test]
fn note_on_ramp_has_no_corner() {
    let held = held_plain(&sine(48_000, 4_000.0, 8_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let n = NOTE_ON_RAMP as f32;
    for model in [TapeModel::Loop, TapeModel::PingPong, TapeModel::Stretch] {
        let p = TapeEngineParams {
            model,
            start: 0.125,
            motion: if model == TapeModel::Stretch {
                0.5
            } else {
                0.0
            },
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (out, _) = play(&mut e, &p, &v, 1);
        let bend = out
            .windows(3)
            .fold(0f32, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()));
        assert!(bend <= 7.0 / (n * n), "{model:?}: bend {bend}");
    }
}

#[test]
fn macros_ease() {
    let held = held_plain(&sine(48_000, 100.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        position: 0.2,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    p.position = 0.6;
    play(&mut e, &p, &v, 1);
    assert_eq!(e.macros().position, 0.6);
    p.position = 1.0;
    let mut want = 0.6f32;
    for _ in 0..5 {
        play(&mut e, &p, &v, 1);
        want += 0.3 * (1.0 - want);
        assert!((e.macros().position - want).abs() < 1e-6);
    }
}

/// MOTION 1: the loop's start walks one window (less the loop) a second,
/// and wraps.
#[test]
fn walk_moves_the_loop() {
    let w = 48_000u32;
    let held = held_plain(&sine(48_000, 100.0, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        motion: 1.0,
        size: 0.0,
        ..TapeEngineParams::default()
    };
    let ll = loop_len(0.0, w, 48_000);
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (_, heads) = play(&mut e, &p, &v, 1_000);
    for t in [0.25f32, 0.5, 0.75, 1.25] {
        let b = (t * SR as f32 / BLOCK_SIZE as f32) as usize - 1;
        let want = t.fract() * (w - ll) as f32;
        let got = heads[b] as f32;
        assert!((got - want).abs() < 300.0, "at {t} s: {got}, want {want}");
    }
}

/// STEAL GLIDE glides `inc`: an octave up at τ 0.1 s is 63 % of the way
/// (in log pitch) 0.1 s on.
#[test]
fn steal_glide_glides_inc() {
    let held = held_plain(&sine(48_000, 100.0, 3 * 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams::default();
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    play(&mut e, &p, &v, 10);
    e.glide_on(72, &p, &v, (SR, 0.1));
    // `heads[i]` ends block i + 1 of the glide.
    let (_, heads) = play(&mut e, &p, &v, 200);
    // `inc` at the end of block j: the mean over blocks j − 3 ..= j + 4,
    // whole frames over 8 blocks.
    let at = (5..190).find(|&j| {
        let inc = (heads[j + 3] - heads[j - 5]) as f32 / (8 * BLOCK_SIZE) as f32;
        inc.log2() >= 0.632
    });
    let want = (0.1 * SR as f32 / BLOCK_SIZE as f32) as usize;
    let at = at.expect("the glide arrives");
    assert!(at.abs_diff(want) <= 1, "63 % at block {at}, want {want}");
}

/// Review Focus 3: a source under `MIN_SOURCE_FRAMES` (64) is refused
/// (STRETCH TAPE Task 8, fix round 2: no window is shorter); the shortest,
/// 64 and 66 frames, with START == END and every setting: finite, in the
/// row, no panic.
#[test]
fn tiny_sources_play_finite() {
    // Held in pairs: 62 frames is the longest refused (63 samples pad to
    // 64 frames; the importer refuses those by their count).
    for frames in [1u32, 2, 4, 12, 62] {
        let held = held_plain(&vec![0.5; frames as usize], 48_000);
        assert!(SourceView::new(&held).is_none(), "{frames} frames");
    }
    for frames in [64u32, 65] {
        for rate in [8_000u16, 48_000] {
            let src: Vec<f32> = (0..frames)
                .map(|i| [0.9, -0.7, 0.4, -0.2][i as usize % 4])
                .collect();
            let held = held_plain(&src, rate);
            let v = SourceView::strict(&held).unwrap();
            for m in MODELS {
                let model = m.model;
                for (start, end) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)] {
                    for (size, xfade) in [(0.0, 0.0), (0.0, 0.5), (1.0, 0.1), (1.0, 0.5)] {
                        let p = TapeEngineParams {
                            model,
                            start,
                            end,
                            size,
                            xfade,
                            motion: -1.0,
                            ..TapeEngineParams::default()
                        };
                        let mut e = fresh();
                        e.note_on(84, &p, &v, SR);
                        let (out, _) = play(&mut e, &p, &v, SR as usize / BLOCK_SIZE);
                        assert!(
                            out.iter().all(|y| y.is_finite()),
                            "{frames} frames, {model:?}, {start}..{end}"
                        );
                    }
                }
            }
            // ONESHOT (SLICE OFF, NOTE, 16) and TRON, every window either
            // way, at the clamp: they end, finite.
            for (model, slice) in [
                (TapeModel::Oneshot, 0u8),
                (TapeModel::Oneshot, 1),
                (TapeModel::Oneshot, 17),
                (TapeModel::Tron, 0),
            ] {
                for (start, end) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)] {
                    for size in [0.0, 1.0] {
                        let p = TapeEngineParams {
                            model,
                            slice,
                            start,
                            end,
                            size,
                            ..TapeEngineParams::default()
                        };
                        let mut e = fresh();
                        e.set_pitch(2_400.0);
                        e.note_on(127, &p, &v, SR);
                        let (out, _) = play(&mut e, &p, &v, 8);
                        assert!(
                            out.iter().all(|y| y.is_finite()),
                            "{frames} frames, {model:?} {slice}, {start}..{end}"
                        );
                        assert!(!e.is_active(), "{model:?} {slice}: ends");
                    }
                }
            }
            // TRANSWAVE can't play a `Plain`: silent, not stuck.
            let p = TapeEngineParams {
                model: TapeModel::Transwave,
                ..TapeEngineParams::default()
            };
            let mut e = fresh();
            e.note_on(84, &p, &v, SR);
            let (out, _) = play(&mut e, &p, &v, 4);
            assert!(out.iter().all(|&y| y == 0.0), "{frames} frames");
        }
    }
    // The smallest transwave, one frame, at every corner of the scan.
    let held = held_transwave(512, 1, |_, h| 0.5 / h as f32);
    let v = SourceView::strict(&held).unwrap();
    for (position, motion, size) in [(0.0, -1.0, 1.0), (1.0, 1.0, 1.0), (0.5, 0.0, 0.0)] {
        let p = TapeEngineParams {
            model: TapeModel::Transwave,
            position,
            motion,
            size,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(84, &p, &v, SR);
        let (out, heads) = play(&mut e, &p, &v, SR as usize / BLOCK_SIZE);
        assert!(out.iter().all(|y| y.is_finite()));
        assert!(heads.iter().all(|&h| h == 0));
        assert!(out.iter().any(|&y| y != 0.0), "it sounds");
    }
}

/// Review Focus 4: `inc` at its clamps, never 0, and finite output.
#[test]
fn midi_extremes_stay_finite() {
    let held = held_plain(&sine(8_000, 37.0, 8_000), 8_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams::default();
    let mut e = fresh();
    e.note_on(0, &p, &v, SR);
    let (out, heads) = play(&mut e, &p, &v, SR as usize / BLOCK_SIZE);
    assert!(heads[heads.len() - 1] > heads[0], "the head still advances");
    assert!(out.iter().all(|y| y.is_finite()));
    assert!(inc(8_000, 0, -2_400.0, 12_700.0, SR) >= f32::EPSILON);

    let held = held_plain(&sine(48_000, 100.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        tape: 1.0,
        ..TapeEngineParams::default()
    };
    assert_eq!(inc(48_000, 127, 2_400.0, p.root, SR), 16.0);
    let mut e = fresh();
    e.set_pitch(2_400.0);
    e.note_on(127, &p, &v, SR);
    let (out, _) = play(&mut e, &p, &v, SR as usize / BLOCK_SIZE);
    assert!(out.iter().all(|y| y.is_finite()));
    // The machines, at both ends of MIDI, either way round.
    for model in [TapeModel::Oneshot, TapeModel::Tron, TapeModel::Loop] {
        for slice in [0u8, 1, 17] {
            for (note, pitch) in [(0u8, -2_400.0f32), (127, 2_400.0)] {
                for (start, end) in [(0.2, 0.8), (0.8, 0.2)] {
                    let p = TapeEngineParams {
                        model,
                        slice,
                        start,
                        end,
                        tape: 1.0,
                        ..TapeEngineParams::default()
                    };
                    let mut e = fresh();
                    e.set_pitch(pitch);
                    e.note_on(note, &p, &v, SR);
                    let (out, _) = play(&mut e, &p, &v, SR as usize / BLOCK_SIZE);
                    assert!(
                        out.iter().all(|y| y.is_finite()),
                        "{model:?} {slice} note {note} {start}..{end}"
                    );
                }
            }
        }
    }

    // TRANSWAVE: note 0 at −24 st and note 127 at +24 st. Its step is at
    // its clamp (Nyquist) at the top and never 0 at the bottom; the levels
    // sit at their ends. At Nyquist a sine-phase table reads its zeros, so
    // only the bottom is asked to sound.
    let held = held_transwave(1_024, 4, |f, h| (f + 1) as f32 * 0.1 / h as f32);
    let v = SourceView::strict(&held).unwrap();
    assert_eq!(wave::cycles(127, 2_400.0, SR), wave::CYCLES_MAX);
    assert!(wave::cycles(0, -2_400.0, SR) >= f32::EPSILON);
    for (note, pitch, level) in [(0u8, -2_400.0f32, 9u8), (127, 2_400.0, 4)] {
        let p = TapeEngineParams {
            model: TapeModel::Transwave,
            position: 0.5,
            motion: 1.0,
            tape: 1.0,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.set_pitch(pitch);
        e.note_on(note, &p, &v, SR);
        let (out, _) = play(&mut e, &p, &v, SR as usize / BLOCK_SIZE);
        assert!(out.iter().all(|y| y.is_finite()), "note {note}");
        assert!(
            note == 127 || out.iter().any(|&y| y != 0.0),
            "note {note} sounds"
        );
        assert_eq!(e.probe().level, level, "note {note}");
    }
}

/// A note-on on a sounding engine ramps from where the output was, so the
/// head's jump to START never steps.
#[test]
fn retrigger_is_click_free() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for m in MODELS {
        let model = m.model;
        let p = as_model(m, TapeEngineParams::default());
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (mut out, _) = play(&mut e, &p, &v, 20);
        let steady = max_step(&out[BLOCK_SIZE..]);
        // Retriggered at frame 0, half a period on, and 30 % in: each a jump.
        for start in [0.0, 50.0 / 48_000.0, 0.3] {
            e.note_on(60, &TapeEngineParams { start, ..p }, &v, SR);
            let (more, _) = play(&mut e, &p, &v, 4);
            out.extend_from_slice(&more);
        }
        let worst = max_step(&out[BLOCK_SIZE..]);
        assert!(worst <= 4.0 * steady, "{model:?}: {worst} against {steady}");
    }
}

/// `inc` ramps across a block: a PITCH move of an octave advances the head
/// by the ramp's mean, 1.5 frames a sample, not by its end's 2.
#[test]
fn inc_ramps_across_the_block() {
    let held = held_plain(&sine(48_000, 100.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams::default();
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (_, a) = play(&mut e, &p, &v, 1);
    e.set_pitch(1_200.0);
    let (_, b) = play(&mut e, &p, &v, 1);
    let moved = b[0] - a[0];
    assert!((95..=97).contains(&moved), "{moved} frames");
}

/// SIZE and POSITION moved every block: each pass takes its bounds at its
/// wrap, the fade covering the step, and a fade with too little room waits
/// rather than splicing.
#[test]
fn moving_bounds_are_click_free() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let steady = {
        let src = sine(48_000, 100.7, 4_800);
        max_step(&src)
    };
    for m in MODELS {
        let model = m.model;
        for xfade in [0.1, 0.5] {
            let mut p = as_model(
                m,
                TapeEngineParams {
                    xfade,
                    ..TapeEngineParams::default()
                },
            );
            let mut e = fresh();
            e.note_on(60, &p, &v, SR);
            let mut out = Vec::new();
            let mut b = [0.0; BLOCK_SIZE];
            for n in 0..3_000 {
                let t = n as f32;
                p.size = 0.6 + 0.4 * (t * 0.05).sin();
                // SCRUB's POSITION is the finger: moved, it bends the pitch.
                if model != TapeModel::Scrub {
                    p.position = 0.5 + 0.5 * (t * 0.031).sin();
                }
                e.render(&mut b, &p, &v, SR);
                out.extend_from_slice(&b);
            }
            let worst = max_step(&out[BLOCK_SIZE..]);
            assert!(
                worst <= 4.0 * steady,
                "{model:?} XFADE {xfade}: {worst} against {steady}"
            );
        }
    }
}

/// `hold` before a fresh note on an idle voice: the ramp starts at 0, not at
/// the last note's sample.
#[test]
fn fresh_note_after_hold_starts_at_zero() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams::default();
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, _) = play(&mut e, &p, &v, 7);
    assert!(
        out[out.len() - 1].abs() > 0.1,
        "the last note ends off zero"
    );
    e.hold();
    e.note_on(60, &TapeEngineParams { start: 0.3, ..p }, &v, SR);
    let (out, _) = play(&mut e, &p, &v, 1);
    assert_eq!(out[0], 0.0);
}
