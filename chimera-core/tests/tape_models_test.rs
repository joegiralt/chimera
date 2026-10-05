//! TAPE's REVERSE, PING-PONG and SCRUB (spec § 2 The models). Precedent:
//! Digitakt II manual § Playback modes (REVERSE LOOP); Polyend Tracker
//! v1.5 § Instrument (ping-pong); Morphagene manual (Slide, Vari-Speed).

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    CLAMP_FADE_S, INC_MAX, NOTE_ON_RAMP, TapeEngine, TapeEngineParams, TapeModel, drag_s, loop_len,
};
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, saw_with_burst, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    *TapeEngine::init_in_place(&mut slot)
}

/// `blocks` blocks of `e` under `p`, with each block's probe.
fn play(
    e: &mut TapeEngine,
    p: &TapeEngineParams,
    v: &SourceView<'_>,
    blocks: usize,
) -> (Vec<f32>, Vec<chimera_core::dsp::tape_engine::Probe>) {
    let (mut out, mut probes) = (Vec::new(), Vec::new());
    let mut b = [0.0; BLOCK_SIZE];
    for _ in 0..blocks {
        e.render(&mut b, p, v, SR);
        out.extend_from_slice(&b);
        probes.push(e.probe());
    }
    (out, probes)
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

/// The largest second difference: a corner, where a step is a click.
fn max_bend(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0f32, |s, w| s.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

#[test]
fn reverse_is_loop_mirrored() {
    let x = saw_with_burst(48_000, 0.5);
    let rev: Vec<f32> = x.iter().rev().copied().collect();
    let (held, held_rev) = (held_plain(&x, 48_000), held_plain(&rev, 48_000));
    let (v, v_rev) = (
        SourceView::strict(&held).unwrap(),
        SourceView::strict(&held_rev).unwrap(),
    );
    assert_eq!(v.frames() as usize, x.len());
    for (start, end, position, size, xfade, motion) in [
        (0.0, 1.0, 0.0, 1.0, 0.1, 0.0),
        (0.1, 0.8, 0.3, 0.6, 0.2, 0.0),
        (0.2, 0.9, 0.7, 0.4, 0.5, 0.3),
    ] {
        let p = TapeEngineParams {
            model: TapeModel::Reverse,
            start,
            end,
            position,
            size,
            xfade,
            motion,
            ..TapeEngineParams::default()
        };
        let q = TapeEngineParams {
            model: TapeModel::Loop,
            start: 1.0 - end,
            end: 1.0 - start,
            ..p
        };
        let (mut a, mut b) = (fresh(), fresh());
        a.note_on(57, &p, &v, SR);
        b.note_on(57, &q, &v_rev, SR);
        let (ya, _) = play(&mut a, &p, &v, 800);
        let (yb, _) = play(&mut b, &q, &v_rev, 800);
        for (i, (s, t)) in ya.iter().zip(&yb).enumerate().skip(NOTE_ON_RAMP as usize) {
            assert!(
                (s - t).abs() < 1e-6,
                "{start}..{end}: sample {i}: {s} against {t}"
            );
        }
    }
}

/// The blocks where the probe head turned: its block-to-block move changed
/// sign.
fn turns(probes: &[chimera_core::dsp::tape_engine::Probe]) -> usize {
    let d: Vec<i64> = probes
        .windows(2)
        .map(|w| w[1].head as i64 - w[0].head as i64)
        .collect();
    d.windows(2).filter(|w| (w[0] > 0) != (w[1] > 0)).count()
}

#[test]
fn ping_pong_turns_are_continuous() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = TapeEngineParams {
        model: TapeModel::PingPong,
        size: 0.45,
        position: 0.5,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, probes) = play(&mut e, &p, &v, 4_000);
    assert!(turns(&probes) >= 100, "{} turns", turns(&probes));
    assert!(probes.iter().all(|pr| pr.other.is_none()));
    let src = sine(48_000, 100.7, 4_800);
    let steady = max_step(&src);
    let worst = max_step(&out[B..]);
    assert!(worst <= 4.0 * steady, "{worst} against {steady}");
    // The head slows to a stop at a turn: no corner in the wave.
    let (steady, worst) = (max_bend(&src), max_bend(&out[B..]));
    assert!(worst <= 2.0 * steady, "bend {worst} against {steady}");
}

/// SIZE 1 → 0.1: at the next turn the head is far outside the new loop, so
/// a second head starts inside it and a 5 ms fade moves the output there.
#[test]
fn ping_pong_clamps_with_a_fade() {
    let w = 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        model: TapeModel::PingPong,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _) = play(&mut e, &p, &v, 100);
    p.size = 0.1;
    let (more, probes) = play(&mut e, &p, &v, 800);
    out.extend_from_slice(&more);
    let ll = loop_len(0.1, w, 48_000);
    let j = probes
        .iter()
        .position(|pr| pr.other.is_some())
        .expect("a clamp");
    let fade = (CLAMP_FADE_S * SR as f32).round() as usize;
    assert_eq!(fade, 240);
    // The old head plays on near the turn; the new one is inside the loop.
    assert!(
        probes[j].head > w - 2 * B as u32,
        "old head at {}",
        probes[j].head
    );
    let s = probes[j].gain * fade as f32;
    assert!(
        (s - s.round()).abs() < 1e-3 && (0.0..=B as f32).contains(&s),
        "the fade's first block: {s} samples"
    );
    let s = s.round() as usize;
    for (k, pr) in probes[j..].iter().enumerate() {
        let done = s + k * B;
        if done >= fade {
            assert_eq!(
                pr.other, None,
                "block {k}: the fade outlived {fade} samples"
            );
            break;
        }
        let other = pr.other.expect("the fade runs on");
        assert!(other <= ll + 1, "new head at {other}, loop {ll}");
        assert!(
            (pr.gain - done as f32 / fade as f32).abs() < 1e-5,
            "block {k}"
        );
    }
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    let worst = max_step(&out[B..]);
    assert!(worst <= 4.0 * steady, "{worst} against {steady}");
}

/// The probe heads' block-to-block moves.
fn moves(probes: &[chimera_core::dsp::tape_engine::Probe]) -> Vec<i64> {
    probes
        .windows(2)
        .map(|w| w[1].head as i64 - w[0].head as i64)
        .collect()
}

#[test]
fn scrub_finger_is_bounded() {
    let w = 4 * 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let most = INC_MAX as i64 * B as i64;
    let ll = loop_len(0.0, w, 48_000) as i64;
    for (motion, from, to) in [(0.0, 0.0, 1.0), (0.0, 1.0, 0.0), (0.5, 0.0, 1.0)] {
        let mut p = TapeEngineParams {
            model: TapeModel::Scrub,
            size: 0.0,
            drag: 0.0,
            motion,
            position: from,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (_, a) = play(&mut e, &p, &v, 4);
        p.position = to;
        let (_, b) = play(&mut e, &p, &v, 200);
        let probes: Vec<_> = a[a.len() - 1..].iter().chain(&b).copied().collect();
        let d = moves(&probes);
        // Within a block the speed is constant: a block's move bounds it.
        for (k, &m) in d.iter().enumerate() {
            assert!(
                m.abs() <= most + 1 || (m < 0 && motion > 0.0 && -m <= most + ll),
                "MOTION {motion}, {from} → {to}: block {k} moved {m}"
            );
        }
        let at_bound = d.iter().filter(|m| m.abs() >= most - 1).count();
        assert!(
            at_bound >= 20,
            "MOTION {motion}: {at_bound} blocks at the bound"
        );
    }
}

#[test]
fn scrub_drag_slews() {
    let w = 9_600u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for drag in [0.3, 0.5, 1.0] {
        let mut p = TapeEngineParams {
            model: TapeModel::Scrub,
            size: 0.0,
            drag,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (_, a) = play(&mut e, &p, &v, 2);
        let h0 = a[1].head as f32;
        p.position = 1.0;
        let (_, b) = play(&mut e, &p, &v, 600);
        let span = (w - loop_len(0.0, w, 48_000)) as f32;
        let n = 1 + b
            .iter()
            .position(|pr| pr.head as f32 - h0 >= 0.632 * span)
            .expect("the finger arrives");
        let want = drag_s(drag) * SR as f32;
        let got = (n * B) as f32;
        assert!(
            (got - want).abs() <= B as f32,
            "DRAG {drag}: 63 % at {got} samples, want {want}"
        );
    }
}

/// A stopped tape is silent (spec § 2 SCRUB): MOTION 0 and a still finger
/// hold no value, from the note-on; once the tape moves the output eases
/// back in, and when it stops again it eases out, each without a step.
#[test]
fn scrub_still_is_silent() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        model: TapeModel::Scrub,
        position: 0.3,
        size: 0.2,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, _) = play(&mut e, &p, &v, 50);
    assert!(out.iter().all(|&y| y == 0.0), "a still tape sounds");
    p.motion = 0.5;
    let (moving, _) = play(&mut e, &p, &v, 50);
    assert!(moving[moving.len() - B..].iter().any(|y| y.abs() > 0.5));
    p.motion = 0.0;
    let (stopping, _) = play(&mut e, &p, &v, 400);
    let tail = &stopping[stopping.len() - B..];
    assert!(
        tail.iter().all(|&y| y == 0.0),
        "the tape stopped, still sounding"
    );
    let all: Vec<f32> = [moving, stopping].concat();
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    assert!(max_step(&all) <= 4.0 * steady);
}

/// MOTION 0.5 plays the loop at the note's speed (`inc` 1 here); POSITION
/// ramped 0 → 1 over a second drags the head a further `W − Ll` frames a
/// second.
#[test]
fn scrub_moving_finger_bends_pitch() {
    let w = 4 * 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        model: TapeModel::Scrub,
        size: 0.5,
        motion: 0.5,
        drag: 0.0,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let blocks = SR as usize / B;
    let mut probes = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    for n in 0..blocks {
        p.position = (n * B) as f32 / SR as f32;
        e.render(&mut b, &p, &v, SR);
        probes.push(e.probe());
    }
    let ll = loop_len(0.5, w, 48_000);
    let want = (1.0 + (w - ll) as f64 / SR as f64) * B as f64;
    let mut d = moves(&probes[blocks / 4..3 * blocks / 4]);
    d.sort();
    let median = d[d.len() / 2] as f64;
    assert!(
        (median / want - 1.0).abs() < 0.01,
        "{median} frames a block, want {want}"
    );
}

/// MOTION swept through 0 again and again, with a long fade half the time:
/// the loop turns round in and out of fades, and never clicks.
#[test]
fn scrub_turns_are_click_free() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    for (size, xfade) in [(0.3, 0.5), (0.5, 0.2)] {
        let mut p = TapeEngineParams {
            model: TapeModel::Scrub,
            size,
            xfade,
            position: 0.4,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (mut out, mut flips, mut was) = (Vec::new(), 0, 0.0f32);
        let mut b = [0.0; BLOCK_SIZE];
        for n in 0..6_000 {
            p.motion = (n as f32 * 0.037).sin();
            if p.motion.signum() != was.signum() {
                flips += 1;
            }
            was = p.motion;
            e.render(&mut b, &p, &v, SR);
            out.extend_from_slice(&b);
        }
        assert!(flips >= 50, "{flips} turns");
        assert!(out.iter().all(|y| y.is_finite()));
        // At twice the source's speed, the sine's own bend is 4 times.
        let bend = max_bend(&out[B..]);
        let steady_bend = 4.0 * max_bend(&sine(48_000, 100.7, 4_800));
        assert!(
            bend <= 2.0 * steady_bend,
            "SIZE {size} XFADE {xfade}: bend {bend} against {steady_bend}"
        );
        let worst = max_step(&out[B..]);
        assert!(
            worst <= 4.0 * steady,
            "SIZE {size} XFADE {xfade}: {worst} against {steady}"
        );
    }
}

/// MOTION below 0 plays the loop backward, at its own speed: from the
/// note-on, and after turning round from forward.
#[test]
fn scrub_motion_below_zero_plays_backward() {
    let w = 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (from, to) in [(-0.5, -0.5), (0.5, -0.5), (-0.25, 1.0)] {
        let mut p = TapeEngineParams {
            model: TapeModel::Scrub,
            size: 0.8,
            position: 0.5,
            motion: from,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        play(&mut e, &p, &v, 50);
        p.motion = to;
        let (_, probes) = play(&mut e, &p, &v, 400);
        let mut d = moves(&probes[100..]);
        d.sort();
        let median = d[d.len() / 2] as f32;
        let want = 2.0 * to * B as f32;
        assert!(
            (median - want).abs() <= 1.0,
            "MOTION {from} → {to}: {median} frames a block, want {want}"
        );
    }
}

/// MOTION crawling about 0, so the loop's fades and its turns come at a
/// crawl, where any corner stands out: no bend past the sine's own at the
/// fastest crawl, ten times over.
#[test]
fn scrub_crawl_has_no_corner() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        model: TapeModel::Scrub,
        size: 0.0,
        xfade: 0.5,
        position: 0.3,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, mut flips, mut was) = (Vec::new(), 0, 0.0f32);
    let mut b = [0.0; BLOCK_SIZE];
    for n in 0..20_000 {
        p.motion = 0.02 * (n as f32 * 0.004).sin();
        // The finger creeps (2e-3 frames a sample), so the tape never
        // counts as stopped: its silence is `scrub_still_is_silent`'s.
        p.position = 0.3 + n as f32 * 2.7e-6;
        if p.motion.signum() != was.signum() {
            flips += 1;
        }
        was = p.motion;
        e.render(&mut b, &p, &v, SR);
        out.extend_from_slice(&b);
    }
    assert!(flips >= 10, "{flips} turns");
    let fastest = 0.042f32;
    let steady = fastest * fastest * max_bend(&sine(48_000, 100.7, 4_800));
    // From block 200: the note starts with the tape stopped, so silent,
    // and eases in as the finger gets going.
    let bend = max_bend(&out[200 * B..]);
    assert!(bend <= 10.0 * steady, "bend {bend} against {steady}");
}

/// POSITION creeping, so each turn latches bounds a fraction of a frame
/// from the last: the head plays on from where it turned, never nudged.
#[test]
fn ping_pong_creeping_bounds_bend_nothing() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        model: TapeModel::PingPong,
        size: 0.3,
        position: 0.3,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, mut probes) = (Vec::new(), Vec::new());
    let mut b = [0.0; BLOCK_SIZE];
    for n in 0..3_000 {
        p.position = 0.3 + n as f32 * 2e-7;
        e.render(&mut b, &p, &v, SR);
        out.extend_from_slice(&b);
        probes.push(e.probe());
    }
    assert!(turns(&probes) >= 50, "{} turns", turns(&probes));
    assert!(probes.iter().all(|pr| pr.other.is_none()));
    let steady = max_bend(&sine(48_000, 100.7, 4_800));
    let bend = max_bend(&out[B..]);
    assert!(bend <= 2.0 * steady, "bend {bend} against {steady}");
}

/// A loop shrunk by 25 to 240 frames under a head going down at POSITION 0
/// (H1): the head is just outside, below frame 0 in the new frame; it
/// clamps under a fade, never wraps.
#[test]
fn ping_pong_small_shrink_at_position_0() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    let w = 48_000u32;
    for shrink in [25u32, 60, 120, 240] {
        let size = 0.8f32;
        let ll = loop_len(size, w, 48_000);
        // The SIZE whose loop is `shrink` frames shorter.
        let min = 96.0f32;
        let want = (ll - shrink) as f32;
        let smaller = (want / min).ln() / (w as f32 / min).ln();
        let mut p = TapeEngineParams {
            model: TapeModel::PingPong,
            size,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (mut out, _) = play(&mut e, &p, &v, 20);
        p.size = smaller;
        for _ in 0..8 {
            let (more, probes) = play(&mut e, &p, &v, ll as usize / B + 2);
            out.extend_from_slice(&more);
            let reach = w + INC_MAX as u32;
            assert!(probes.iter().all(|pr| pr.head <= reach), "shrink {shrink}");
        }
        let worst = max_step(&out[B..]);
        assert!(
            worst <= 4.0 * steady,
            "shrink {shrink}: {worst} against {steady}"
        );
    }
}

/// PING-PONG under a SIZE LFO at several rates and POSITIONs: bounded heads,
/// finite, click-free.
#[test]
fn ping_pong_size_lfo_soak() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    for (rate, position) in [(0.003f32, 0.0f32), (0.02, 0.0), (0.05, 1.0), (0.2, 0.5)] {
        let mut p = TapeEngineParams {
            model: TapeModel::PingPong,
            position,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let mut out = Vec::new();
        let mut b = [0.0; BLOCK_SIZE];
        for n in 0..6_000 {
            p.size = 0.5 + 0.5 * (n as f32 * rate).sin();
            e.render(&mut b, &p, &v, SR);
            out.extend_from_slice(&b);
            assert!(e.probe().head <= 48_000 + INC_MAX as u32);
        }
        assert!(out.iter().all(|y| y.is_finite()));
        let worst = max_step(&out[B..]);
        assert!(worst <= 4.0 * steady, "LFO {rate} at {position}: {worst}");
    }
}

/// Every loop is at least `MIN_LOOP_FRAMES` (or the source), so at `inc`'s
/// clamp a loop wraps at most once in 4 samples; folded at XFADE ½ a pass
/// is half the loop, so at most a fade's start and a wrap in 4.
#[test]
fn events_are_bounded_per_block() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for model in [
        TapeModel::Loop,
        TapeModel::Reverse,
        TapeModel::PingPong,
        TapeModel::Scrub,
    ] {
        for xfade in [0.0, 0.5] {
            let p = TapeEngineParams {
                model,
                size: 0.0,
                xfade,
                motion: 1.0,
                ..TapeEngineParams::default()
            };
            let mut e = fresh();
            e.set_pitch(4_800.0);
            e.note_on(127, &p, &v, SR);
            play(&mut e, &p, &v, 4);
            let before = e.events();
            let blocks = 200;
            play(&mut e, &p, &v, blocks);
            let per = (e.events() - before) as f32 / blocks as f32;
            // A wrap at most every 4 samples; folded at XFADE ½, a pass is
            // half the loop, so a fade's start and its wrap every 2.
            let most = if xfade == 0.0 { B / 4 + 1 } else { B + 1 };
            assert!(
                per <= most as f32,
                "{model:?} XFADE {xfade}: {per} events a block"
            );
        }
    }
}

/// The worst case for events (H2): SIZE 0 at `inc`'s clamp on an 8-frame
/// source, where a loop is the whole source. Its host time stays within a
/// few times LOOP's fade path (a long loop fading every sample). This is
/// the row the step-3 bench measures on the unit.
#[test]
fn tiny_loop_at_the_clamp_costs_about_a_fade() {
    use std::time::Instant;
    let tiny = held_plain(&[0.9, -0.7, 0.4, -0.2, 0.6, -0.1, 0.3, -0.8], 48_000);
    let tv = SourceView::strict(&tiny).unwrap();
    let long = held_plain(&sine(48_000, 100.7, 480_000), 48_000);
    let lv = SourceView::strict(&long).unwrap();
    let blocks = 20_000;
    let time = |p: &TapeEngineParams, v: &SourceView<'_>, note: u8, pitch: f32| {
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut e = fresh();
            e.set_pitch(pitch);
            e.note_on(note, p, v, SR);
            let mut b = [0.0; BLOCK_SIZE];
            let t = Instant::now();
            for _ in 0..blocks {
                e.render(&mut b, p, v, SR);
                std::hint::black_box(&b);
            }
            best = best.min(t.elapsed().as_secs_f64());
        }
        best
    };
    for model in [
        TapeModel::Loop,
        TapeModel::Reverse,
        TapeModel::PingPong,
        TapeModel::Scrub,
    ] {
        let p = TapeEngineParams {
            model,
            size: 0.0,
            xfade: 0.5,
            motion: 1.0,
            ..TapeEngineParams::default()
        };
        let tiny_t = time(&p, &tv, 127, 4_800.0);
        // A long loop's fade at its own speed: half of every pass fades.
        let fade = TapeEngineParams {
            xfade: 0.5,
            ..TapeEngineParams::default()
        };
        let fade_t = 2.0 * time(&fade, &lv, 60, 0.0);
        let ratio = tiny_t / fade_t;
        eprintln!("{model:?}: tiny loop at the clamp {ratio:.2} × LOOP's fade");
        assert!(ratio < 4.0, "{model:?}: {ratio:.2} × LOOP's fade");
    }
}

/// MOTION dropping 1 → 0 while POSITION jumps 0 → 1: the finger's speed
/// keeps the head's within `INC_MAX` every block (L1).
#[test]
fn scrub_motion_drop_with_a_jump_stays_bounded() {
    let w = 4 * 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let most = INC_MAX as i64 * B as i64;
    let ll = loop_len(0.5, w, 48_000) as i64;
    let mut p = TapeEngineParams {
        model: TapeModel::Scrub,
        size: 0.5,
        drag: 0.0,
        motion: 1.0,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.set_pitch(1_200.0);
    e.note_on(60, &p, &v, SR);
    let (_, a) = play(&mut e, &p, &v, 10);
    p.motion = 0.0;
    p.position = 1.0;
    let (_, b) = play(&mut e, &p, &v, 100);
    let probes: Vec<_> = a[a.len() - 1..].iter().chain(&b).copied().collect();
    for (k, m) in moves(&probes).into_iter().enumerate() {
        assert!(m <= most + 1 && m >= -(ll + 1), "block {k}: moved {m}");
    }
}
