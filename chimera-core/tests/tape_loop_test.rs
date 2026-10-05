//! TAPE's LOOP (spec § 2 The macros and the window, LOOP, Parameters never
//! snap): the pitch law, the folded crossfade, bounds latched at a wrap,
//! the note-on ramp, the macros' easing and MOTION's walk. Precedent:
//! Digitakt II manual § Playback modes; Morphagene manual (Organize).

mod common;

use core::f32::consts::TAU;
use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    INC_MAX, NOTE_ON_RAMP, TapeEngine, TapeEngineParams, TapeModel, inc, loop_len,
};
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, sine};

const SR: u32 = 48_000;

/// The models this task plays; Tasks 6–8 add theirs.
const MODELS: [TapeModel; 1] = [TapeModel::Loop];

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
    for xfade in [0.1, 0.5] {
        for position in [0.0, 0.5] {
            let p = TapeEngineParams {
                xfade,
                position,
                size: 0.6,
                ..TapeEngineParams::default()
            };
            let mut e = fresh();
            e.note_on(60, &p, &v, SR);
            let (out, heads) = play(&mut e, &p, &v, 1_500);
            assert!(wraps(&heads) >= 10, "XFADE {xfade} POSITION {position}");
            let steady = max_step(&out[BLOCK_SIZE..2_000]);
            let worst = max_step(&out[BLOCK_SIZE..]);
            assert!(
                worst <= 4.0 * steady,
                "XFADE {xfade} POSITION {position}: {worst} against {steady}"
            );
        }
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
        assert!(y.abs() <= (k + 1) as f32 / n as f32, "{k}: {y}");
    }
    // The head starts on START's frame, the peak.
    assert!(out[0] == 0.0 && (out[n] - (n as f32 * TAU / 400.0).cos()).abs() < 2e-3);
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

/// Review Focus 3: 1–4 frames, START == END, every setting: finite, in the
/// row, no panic.
#[test]
fn tiny_sources_play_finite() {
    for frames in 1..=4u32 {
        for rate in [8_000u16, 48_000] {
            let src: Vec<f32> = (0..frames)
                .map(|i| [0.9, -0.7, 0.4, -0.2][i as usize])
                .collect();
            let held = held_plain(&src, rate);
            let v = SourceView::strict(&held).unwrap();
            for model in MODELS {
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
        }
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
}
