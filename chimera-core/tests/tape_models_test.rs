//! TAPE's PING-PONG and SCRUB, and LOOP reversed (spec § 2 The models;
//! END before START, `tape_machines_test`). Precedent:
//! Digitakt II manual § Playback modes (REVERSE LOOP); Polyend Tracker
//! v1.5 § Instrument (ping-pong); Morphagene manual (Slide, Vari-Speed).

mod common;

use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::stream::units::SourceRate;
use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    CLAMP_FADE_S, INC_MAX, MIN_PASS_SAMPLES, Direction, TapeEngine, TapeEngineParams, TapeModel,
    drag_s, loop_len,
};
use chimera_hal::BLOCK_SIZE;
use common::tape::{Mach, held_plain, held_transwave, sine};

const SR: u32 = 48_000;
/// The bills (spec § 5) the shortest-loop timings are held under.
const LOOP_BILL: f64 = 450.0;
const PING_PONG_BILL: f64 = 490.0;
const SCRUB_BILL: f64 = 480.0;
/// TRANSWAVE's (spec § 5): two heads; four in a level change's block.
const WAVE_BILL: f64 = 400.0;
const WAVE_CHANGE_BILL: f64 = 770.0;
const B: usize = BLOCK_SIZE;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    unsafe { slot.assume_init() }
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

/// The largest |sample|.
fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, y| m.max(y.abs()))
}

/// A full-scale sine's peak through PING-PONG's clamps: 1.05× it.
const PEAK_BOUND: f32 = 1.05;

/// The largest second difference: a corner, where a step is a click.
fn max_bend(x: &[f32]) -> f32 {
    x.windows(3)
        .fold(0.0f32, |s, w| s.max((w[2] - 2.0 * w[1] + w[0]).abs()))
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
        machine: TapeMachine::Play,
        direction: Direction::Ping,
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

/// SIZE 1 → 0.1 on the way up, past the first turn (before it the first
/// pass just follows SIZE): at the top the head is far outside the new
/// loop, so a second head starts inside it and a 5 ms fade moves the output
/// there.
#[test]
fn ping_pong_clamps_with_a_fade() {
    let w = 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        machine: TapeMachine::Play,
        direction: Direction::Ping,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _) = play(&mut e, &p, &v, 1_550);
    p.size = 0.1;
    let (more, probes) = play(&mut e, &p, &v, 900);
    out.extend_from_slice(&more);
    let ll = loop_len(0.1, w, SourceRate::MAX);
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
    let ll = loop_len(0.0, w, SourceRate::MAX) as i64;
    for (motion, from, to) in [(0.0, 0.0, 1.0), (0.0, 1.0, 0.0), (0.5, 0.0, 1.0)] {
        let mut p = TapeEngineParams {
            machine: TapeMachine::Scrub,
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
            machine: TapeMachine::Scrub,
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
        let span = (w - loop_len(0.0, w, SourceRate::MAX)) as f32;
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
        machine: TapeMachine::Scrub,
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

/// The tape just under `STILL` (the finger creeping at 0.8 of it) stays
/// stopped and silent from its note-on, never toggling; at 1.5× it stays
/// stopped from a stop and moving from a start (hysteresis). The heads play
/// on underneath either way (N1): a hushed block is still read.
#[test]
fn scrub_stop_has_hysteresis() {
    let w = 48_000u32;
    let held = held_plain(&sine(48_000, 100.7, w), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let span = (w - loop_len(0.2, w, SourceRate::MAX)) as f32;
    // (speed, its first 100 blocks', silent after): a stopped tape starts
    // only over 2×, a moving one stops only under 1×.
    for (speed, start, silent) in [
        (0.8e-3f32, 0.8e-3f32, true),
        (1.5e-3, 1.5e-3, true),
        (1.5e-3, 4e-3, false),
    ] {
        let mut p = TapeEngineParams {
            machine: TapeMachine::Scrub,
            size: 0.2,
            drag: 0.0,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let mut pos = 0.0f32;
        let mut b = [0.0; BLOCK_SIZE];
        let (mut first_head, mut last_head) = (0, 0);
        for n in 0..3_000 {
            let sp = if n < 100 { start } else { speed };
            pos += sp * B as f32 / span;
            p.position = pos;
            e.render(&mut b, &p, &v, SR);
            if n == 200 {
                first_head = e.probe().head;
            }
            last_head = e.probe().head;
            if n >= 200 {
                if silent {
                    assert!(b.iter().all(|&y| y == 0.0), "{speed}: block {n} sounds");
                } else {
                    assert!(b.iter().any(|&y| y != 0.0), "{speed}: block {n} is silent");
                }
            }
        }
        // The finger moved on underneath: 2 800 blocks at `speed`.
        let moved = (last_head - first_head) as f32;
        let want = speed * 2_799.0 * B as f32;
        assert!(
            (moved / want - 1.0).abs() < 0.05,
            "{speed}: moved {moved}, want {want}"
        );
    }
}

/// A mid-note stop through the voice's DC blocker: the held value leaves
/// over `HUSH_S` without a step above the material's own (a slow sine, so
/// a 1 ms hush's tick would show).
#[test]
fn scrub_mid_note_stop_passes_the_dc_blocker() {
    use chimera_core::dsp::dc_blocker::DcBlocker;
    use chimera_core::dsp::voice::DC_HZ;
    let held = held_plain(&sine(48_000, 2_000.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for stop_at in [50usize, 57, 63, 71] {
        let mut p = TapeEngineParams {
            machine: TapeMachine::Scrub,
            size: 0.5,
            position: 0.3,
            motion: 0.5,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let mut dc = DcBlocker::new(DC_HZ, SR);
        let mut out = Vec::new();
        let mut b = [0.0; BLOCK_SIZE];
        for n in 0..stop_at + 400 {
            if n == stop_at {
                p.motion = 0.0;
            }
            if n == stop_at + 200 {
                p.motion = 0.5;
            }
            e.render(&mut b, &p, &v, SR);
            out.extend(b.iter().map(|&x| dc.process(x)));
        }
        let own = max_step(&out[20 * B..stop_at * B]);
        let worst = max_step(&out[stop_at * B..]);
        assert!(
            worst <= own * 1.05,
            "stop at block {stop_at}: {worst} against {own}"
        );
    }
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
        machine: TapeMachine::Scrub,
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
    let ll = loop_len(0.5, w, SourceRate::MAX);
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
            machine: TapeMachine::Scrub,
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
            machine: TapeMachine::Scrub,
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
        machine: TapeMachine::Scrub,
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
        machine: TapeMachine::Play,
        direction: Direction::Ping,
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
        let ll = loop_len(size, w, SourceRate::MAX);
        // The SIZE whose loop is `shrink` frames shorter.
        let min = 96.0f32;
        let want = (ll - shrink) as f32;
        let smaller = (want / min).ln() / (w as f32 / min).ln();
        let mut p = TapeEngineParams {
            machine: TapeMachine::Play,
            direction: Direction::Ping,
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
            machine: TapeMachine::Play,
            direction: Direction::Ping,
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

/// A loop is at least `MIN_PASS_SAMPLES · inc` frames, so at `inc` 16 on a
/// 64-frame window (which END widens to the floor) a pass, or a turn, comes
/// at most once in 16 output samples: one event a pass at XFADE 0. Folded
/// at XFADE ½ a pass is half the loop: a fade's start and its wrap in 8.
#[test]
fn pass_rate_is_bounded() {
    let held = held_plain(&sine(48_000, 100.7, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for m in [
        Mach::fwd(TapeModel::Play),
        Mach::rev(TapeModel::Play),
        Mach::ping(),
        Mach::fwd(TapeModel::Scrub),
    ] {
        let model = m.model;
        for xfade in [0.0, 0.5] {
            let p = TapeEngineParams {
                machine: TapeMachine::of(model),
                start: 0.5,
                end: 0.5 + 64.0 / 48_000.0,
                size: 0.0,
                xfade,
                motion: 1.0,
                ..TapeEngineParams::default()
            };
            let p = m.params(p);
            let mut e = fresh();
            e.set_pitch(4_800.0);
            e.note_on(127, &p, &v, SR);
            play(&mut e, &p, &v, 4);
            let before = e.events();
            let blocks = 200;
            play(&mut e, &p, &v, blocks);
            let per = (e.events() - before) as f32 / blocks as f32;
            let most = if xfade == 0.0 {
                B / MIN_PASS_SAMPLES as usize + 1
            } else {
                2 * B / (MIN_PASS_SAMPLES as usize / 2) + 1
            };
            assert!(
                per <= most as f32,
                "{m:?} XFADE {xfade}: {per} events a block, at most {most}"
            );
        }
    }
}

/// `inc` swept from 1 to 16 and back on SIZE 0: the floor rises from 96
/// to 256 frames and falls, each change latched at a wrap or a turn under
/// the crossfades, or followed while the head runs up to its first. A loop
/// this short at this speed has its own seams, so the sweep is held against
/// the same loop played at fixed speeds across the range: the floor's moves
/// add no step of their own. Swept over 60 and 300 blocks the head is still
/// running up from START to POSITION 0.3's loop as `inc` climbs (F1); over
/// 3,000 it is long in the loop.
#[test]
fn inc_sweep_across_the_floor_is_click_free() {
    let held = held_plain(&sine(48_000, 2_000.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let cents = |t: f32| 4_800.0 * (1.0 - (2.0 * t - 1.0).abs());
    for m in [
        Mach::fwd(TapeModel::Play),
        Mach::rev(TapeModel::Play),
        Mach::ping(),
        Mach::fwd(TapeModel::Scrub),
    ] {
        let model = m.model;
        let p = TapeEngineParams {
            machine: TapeMachine::of(model),
            size: 0.0,
            xfade: 0.5,
            position: 0.3,
            motion: 0.5,
            ..TapeEngineParams::default()
        };
        let p = m.params(p);
        let run = |blocks: usize, pitch: &dyn Fn(usize) -> f32| {
            let mut e = fresh();
            e.set_pitch(pitch(0));
            e.note_on(60, &p, &v, SR);
            let mut out = Vec::new();
            let mut b = [0.0; BLOCK_SIZE];
            for n in 0..blocks {
                e.set_pitch(pitch(n));
                e.render(&mut b, &p, &v, SR);
                out.extend_from_slice(&b);
            }
            max_step(&out[4 * B..])
        };
        // Every 200 cents: a loop's seams vary with its length.
        let fixed = (0..=24)
            .map(|k| {
                let c = 200.0 * k as f32;
                run(3_000, &|_| c)
            })
            .fold(0.0f32, f32::max);
        for blocks in [60, 300, 3_000] {
            let swept = run(blocks, &|n| cents(n as f32 / blocks as f32));
            assert!(
                swept <= 1.25 * fixed,
                "{m:?} over {blocks} blocks: swept {swept} against fixed {fixed}"
            );
        }
    }
}

/// The pitch dropping four octaves at once, again and again: the floor
/// falls from 256 frames to 96, but the pass in play still ends where it
/// was latched, under its fade. Applied at once, the shorter loop's end can
/// fall behind the head mid-pass, which splices. A ramp source shows any
/// jump; each block at the low pitch is held against that pitch's own run.
#[test]
fn pitch_drops_latch_the_floor_at_a_wrap() {
    let ramp: Vec<f32> = (0..48_000).map(|i| i as f32 / 24_000.0 - 1.0).collect();
    let held = held_plain(&ramp, 48_000);
    let v = SourceView::strict(&held).unwrap();
    let square = |n: usize| {
        if (n / 13).is_multiple_of(2) {
            4_800.0
        } else {
            0.0
        }
    };
    for m in [
        Mach::fwd(TapeModel::Play),
        Mach::rev(TapeModel::Play),
        Mach::ping(),
        Mach::fwd(TapeModel::Scrub),
    ] {
        let model = m.model;
        let p = TapeEngineParams {
            machine: TapeMachine::of(model),
            size: 0.0,
            xfade: 0.1,
            position: 0.3,
            motion: if model == TapeModel::Scrub { 0.5 } else { 0.0 },
            ..TapeEngineParams::default()
        };
        let p = m.params(p);
        let run = |pitch: &dyn Fn(usize) -> f32| {
            let mut e = fresh();
            e.set_pitch(pitch(0));
            e.note_on(60, &p, &v, SR);
            let mut out = Vec::new();
            let mut b = [0.0; BLOCK_SIZE];
            for n in 0..1_200 {
                e.set_pitch(pitch(n));
                e.render(&mut b, &p, &v, SR);
                out.extend_from_slice(&b);
            }
            out
        };
        let low = max_step(&run(&|_| 0.0)[4 * B..]);
        let out = run(&square);
        // Blocks wholly at the low pitch: the drop's own block ramps.
        let worst = (4..1_200)
            .filter(|&n| square(n) == 0.0 && square(n - 1) == 0.0)
            .map(|n| max_step(&out[n * B - 1..(n + 1) * B]))
            .fold(0.0f32, f32::max);
        assert!(
            worst <= 1.5 * low,
            "{m:?}: {worst} after a drop, against {low}"
        );
    }
}

/// The pitch jumping four octaves and back, again and again: a turn sized
/// for the slow head is carried far past the edge by the fast one, and the
/// head that reflects there reads where it never played, so the output
/// fades to it as a clamp's does. Every block is held against the loop's
/// own runs at the two pitches, walked by MOTION and still, at two
/// POSITIONs, and no block passes the sine's peak: a fade retargeted again
/// and again carries the output it left, never more.
#[test]
fn ping_pong_pitch_jumps_are_click_free() {
    let held = held_plain(&sine(48_000, 2_000.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (motion, position) in [(0.0, 0.3), (0.5, 0.3), (0.0, 0.0)] {
        let p = TapeEngineParams {
            machine: TapeMachine::Play,
            direction: Direction::Ping,
            size: 0.0,
            position,
            motion,
            ..TapeEngineParams::default()
        };
        let run = |pitch: &dyn Fn(usize) -> f32| {
            let mut e = fresh();
            e.set_pitch(pitch(0));
            e.note_on(60, &p, &v, SR);
            let mut out = Vec::new();
            let mut b = [0.0; BLOCK_SIZE];
            for n in 0..1_200 {
                e.set_pitch(pitch(n));
                e.render(&mut b, &p, &v, SR);
                out.extend_from_slice(&b);
            }
            (max_step(&out[4 * B..]), peak(&out))
        };
        let fixed = run(&|_| 0.0).0.max(run(&|_| 4_800.0).0);
        for half in [7, 13, 29] {
            let (jumps, top) = run(&|n| if (n / half) % 2 == 0 { 4_800.0 } else { 0.0 });
            assert!(
                jumps <= 1.25 * fixed,
                "MOTION {motion}, POSITION {position}, every {half} blocks: {jumps} against {fixed}"
            );
            assert!(
                top <= PEAK_BOUND,
                "MOTION {motion}, POSITION {position}, every {half} blocks: peak {top}"
            );
        }
    }
}

/// PING-PONG under a far POSITION square wave clamps all the time, and a
/// pitch square wave on top makes turns land late and heads land outside
/// while a clamp's fade still runs: those jumps retarget the fade (R1), so
/// the worst step stays within the fixed pitches' own under the same wave.
#[test]
fn ping_pong_jumps_under_a_running_clamp_are_click_free() {
    let held = held_plain(&sine(48_000, 2_000.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut loud = Vec::new();
    for size in [0.0, 0.2] {
        for motion in [0.0, 0.5] {
            let run = |pitch: &dyn Fn(usize) -> f32| {
                let p = |n: usize| TapeEngineParams {
                    machine: TapeMachine::Play,
                    direction: Direction::Ping,
                    size,
                    position: if (n / 3).is_multiple_of(2) { 0.1 } else { 0.9 },
                    motion,
                    ..TapeEngineParams::default()
                };
                let mut e = fresh();
                e.set_pitch(pitch(0));
                e.note_on(60, &p(0), &v, SR);
                let mut out = Vec::new();
                let mut b = [0.0; BLOCK_SIZE];
                for n in 0..3_000 {
                    e.set_pitch(pitch(n));
                    e.render(&mut b, &p(n), &v, SR);
                    out.extend_from_slice(&b);
                }
                (max_step(&out[4 * B..]), peak(&out))
            };
            let fixed = run(&|_| 0.0).0.max(run(&|_| 4_800.0).0);
            for half in [2, 3, 7] {
                let (jumps, top) = run(&|n| if (n / half) % 2 == 0 { 4_800.0 } else { 0.0 });
                if jumps > 1.25 * fixed || top > PEAK_BOUND {
                    loud.push(format!(
                        "SIZE {size}, MOTION {motion}, every {half} blocks: {jumps} against {fixed}, peak {top}"
                    ));
                }
            }
        }
    }
    assert!(loud.is_empty(), "{loud:#?}");
}

/// A clamp's fade holds PING-PONG's bounds, but `inc` rising under it still
/// lengthens them to the floor (F2): a 96-frame loop at `inc` 1 clamps under
/// a POSITION jump, then the pitch rises four octaves, and every block's
/// turns stay within the floor's one pass in 16 samples.
#[test]
fn ping_pong_clamp_keeps_the_floor() {
    let held = held_plain(&sine(48_000, 2_000.0, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let mut p = TapeEngineParams {
        machine: TapeMachine::Play,
        direction: Direction::Ping,
        size: 0.0,
        ..TapeEngineParams::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _) = play(&mut e, &p, &v, 20);
    p.position = 1.0;
    let mut n = 0;
    while e.probe().other.is_none() {
        out.extend_from_slice(&play(&mut e, &p, &v, 1).0);
        n += 1;
        assert!(n < 100, "no clamp");
    }
    e.set_pitch(4_800.0);
    let most = B as u32 / MIN_PASS_SAMPLES + 1;
    for k in 0..40 {
        let before = e.events();
        out.extend_from_slice(&play(&mut e, &p, &v, 1).0);
        let per = e.events() - before;
        assert!(per <= most, "block {k}: {per} events, at most {most}");
    }
    // Lengthened round the head, the held bounds move nothing.
    let steady = 16.0 * max_step(&sine(48_000, 2_000.0, 4_800));
    let worst = max_step(&out[B..]);
    assert!(worst <= 2.0 * steady, "{worst} against {steady}");
}

/// Host time a sample of `blocks` blocks of `p(block)` on `v`, at `note`
/// and `pitch`: the best of seven.
fn host_time(
    p: &dyn Fn(usize) -> TapeEngineParams,
    v: &SourceView<'_>,
    (note, pitch): (u8, f32),
    blocks: usize,
) -> f64 {
    use std::time::Instant;
    let mut best = f64::MAX;
    for _ in 0..7 {
        let mut e = fresh();
        e.set_pitch(pitch);
        e.note_on(note, &p(0), v, SR);
        let mut b = [0.0; BLOCK_SIZE];
        let t = Instant::now();
        for n in 0..blocks {
            e.render(&mut b, &p(n), v, SR);
            std::hint::black_box(&b);
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    best
}

/// The worst cases for events and per-call work (H2, N2), the `TAPE TINY`
/// bench rows, in host time against LOOP's fade path (a long loop at
/// neutral XFADE ½, where every pass fades):
/// - each model at SIZE 0 and `inc` 16 on the shortest source (64 frames);
/// - each model in its shortest legal loop at `inc` 16 (SIZE 0 on 8 kHz
///   sources: 256 frames, `MIN_PASS_SAMPLES · inc`), XFADE ½; PING-PONG
///   clamping there under a POSITION square wave.
///
/// Each is held under its model's bill: host time over LOOP's fade path,
/// times that path's 217 instructions, is an M7 estimate, billed at
/// × 1.46 × 1.1 + 12 a head; 5 % is left for the host's noise (in release;
/// `cargo test --release` measured the bills). The bench rows replace
/// these.
///
/// Host timing is load-sensitive, so it doesn't run by default: run it
/// alone on a quiet machine with `cargo test --release -p chimera-core
/// --test tape_models_test tiny_loops -- --ignored`.
#[test]
#[ignore = "load-sensitive host timing; the bench rows replace it"]
fn tiny_loops_cost_within_their_bills() {
    let tiny = held_plain(
        &[0.9, -0.7, 0.4, -0.2, 0.6, -0.1, 0.3, -0.8].repeat(8),
        48_000,
    );
    let tv = SourceView::strict(&tiny).unwrap();
    // At 8 kHz, SIZE 0's 2 ms is 16 frames: the loop is MIN_LOOP_FRAMES.
    let slow = held_plain(&sine(8_000, 100.7, 80_000), 8_000);
    let sv = SourceView::strict(&slow).unwrap();
    let long = held_plain(&sine(48_000, 100.7, 480_000), 48_000);
    let lv = SourceView::strict(&long).unwrap();
    let blocks = 20_000;
    let fade = host_time(
        &|_| TapeEngineParams {
            xfade: 0.5,
            ..TapeEngineParams::default()
        },
        &lv,
        (60, 0.0),
        blocks,
    );
    let at_clamp = (127, 4_800.0);
    let bill = |m: (TapeModel, Direction)| match m {
        (TapeModel::Play, Direction::Ping) => PING_PONG_BILL,
        (TapeModel::Play, _) => LOOP_BILL,
        _ => SCRUB_BILL,
    };
    let mut rows: Vec<(String, (TapeModel, Direction), f64)> = Vec::new();
    for m in [
        Mach::fwd(TapeModel::Play),
        Mach::rev(TapeModel::Play),
        Mach::ping(),
        Mach::fwd(TapeModel::Scrub),
    ] {
        let model = m.model;
        let p = TapeEngineParams {
            machine: TapeMachine::of(model),
            size: 0.0,
            xfade: 0.5,
            motion: 1.0,
            ..TapeEngineParams::default()
        };
        let p = m.params(p);
        let t = host_time(&|_| p, &tv, at_clamp, blocks);
        rows.push((format!("{m:?}, 64 frames"), (model, m.mode), t));
    }
    for m in [Mach::fwd(TapeModel::Play), Mach::rev(TapeModel::Play)] {
        let model = m.model;
        let p = TapeEngineParams {
            machine: TapeMachine::of(model),
            size: 0.0,
            xfade: 0.5,
            ..TapeEngineParams::default()
        };
        let p = m.params(p);
        let t = host_time(&|_| p, &sv, at_clamp, blocks);
        rows.push((format!("{m:?}, shortest loop"), (model, m.mode), t));
    }
    let square = |n: usize| TapeEngineParams {
        machine: TapeMachine::Play,
        direction: Direction::Ping,
        size: 0.0,
        position: if (n / 8).is_multiple_of(2) { 0.0 } else { 1.0 },
        ..TapeEngineParams::default()
    };
    let t = host_time(&square, &sv, at_clamp, blocks);
    rows.push((
        "PingPong, shortest loop, clamping".into(),
        (TapeModel::Play, Direction::Ping),
        t,
    ));
    let scrub = TapeEngineParams {
        machine: TapeMachine::Scrub,
        size: 0.0,
        xfade: 0.5,
        motion: 1.0,
        ..TapeEngineParams::default()
    };
    let t = host_time(&|_| scrub, &sv, at_clamp, blocks);
    rows.push((
        "Scrub, shortest loop".into(),
        (TapeModel::Scrub, Direction::Loop),
        t,
    ));
    for (name, model, t) in rows {
        let ratio = t / fade;
        let cycles = ratio * 217.0 * 1.46 * 1.1 + 24.0;
        eprintln!("{name}: {ratio:.2} × LOOP's fade, about {cycles:.0} cycles");
        // A debug build's code is not the firmware's: there, only a gross
        // regression (an event costing a block's work) is caught.
        let slack = if cfg!(debug_assertions) { 1.5 } else { 1.05 };
        assert!(
            cycles <= slack * bill(model),
            "{name}: about {cycles:.0} cycles, billed {}",
            bill(model)
        );
    }
}

/// TRANSWAVE's bills (spec § 5), in host time against LOOP's fade path as
/// `tiny_loops_cost_within_their_bills` measures, the block's own work
/// included: a 32-frame table at top 1 024 scanned at MOTION 1, SIZE 1,
/// two heads; and a pitch route stepping up 12 st a block through six
/// levels (a drop each block) and back at once (held a block, then a rise
/// of six), so seven blocks in eight change level and read four heads, the
/// eighth taken out at the steady path's time.
///
/// Host timing is load-sensitive: run it alone, `cargo test --release -p
/// chimera-core --test tape_models_test transwave_costs -- --ignored`.
#[test]
#[ignore = "load-sensitive host timing; the bench rows replace it"]
fn transwave_costs_within_its_bills() {
    let long = held_plain(&sine(48_000, 100.7, 480_000), 48_000);
    let lv = SourceView::strict(&long).unwrap();
    let held = held_transwave(1_024, 32, |f, h| (f + 1) as f32 * 0.01 / h as f32);
    let wv = SourceView::strict(&held).unwrap();
    let blocks = 24_000;
    let fade = host_time(
        &|_| TapeEngineParams {
            xfade: 0.5,
            ..TapeEngineParams::default()
        },
        &lv,
        (60, 0.0),
        blocks,
    );
    let scan = |_| TapeEngineParams {
        machine: TapeMachine::Wave,
        position: 0.5,
        motion: 1.0,
        size: 1.0,
        ..TapeEngineParams::default()
    };
    let steady = host_time(&scan, &wv, (24, 0.0), blocks) / fade;
    // Seven changes in eight blocks; `host_time` only sets the pitch at the
    // note-on, so the pattern runs here, best of seven.
    let mut best = f64::MAX;
    for _ in 0..7 {
        let mut e = fresh();
        e.note_on(24, &scan(0), &wv, SR);
        let mut b = [0.0; BLOCK_SIZE];
        let t = std::time::Instant::now();
        for n in 0..blocks {
            let k = n % 8;
            e.set_pitch(if k < 7 { 1_200.0 * k as f32 } else { 0.0 });
            e.render(&mut b, &scan(n), &wv, SR);
            std::hint::black_box(&b);
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    let mixed = best / fade;
    let change = (8.0 * mixed - steady) / 7.0;
    let slack = if cfg!(debug_assertions) { 1.5 } else { 1.05 };
    for (name, ratio, heads, bill) in [
        ("steady", steady, 2.0, WAVE_BILL),
        ("level change", change, 4.0, WAVE_CHANGE_BILL),
    ] {
        let cycles = ratio * 217.0 * 1.46 * 1.1 + 12.0 * heads;
        eprintln!("TRANSWAVE {name}: {ratio:.2} × LOOP's fade, about {cycles:.0} cycles");
        assert!(
            cycles <= slack * bill,
            "{name}: about {cycles:.0}, billed {bill}"
        );
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
    let ll = loop_len(0.5, w, SourceRate::MAX) as i64;
    let mut p = TapeEngineParams {
        machine: TapeMachine::Scrub,
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

/// A fresh note at MOTION about 0, MOTION then running backward (#383):
/// the head turns at its loop's start, so the turn's block has an event at
/// once, in the new frame (it once wrapped as LOOP's flip, dropping the
/// finger, and read past the source's end). At SCRUB's defaults (LENGTH 1,
/// XFADE 0.1, DRAG 0.3, FINGER 0) and at a placed loop, on a C4 sine: no
/// step past the sine's own at the fastest speed it reaches.
#[test]
fn scrub_fresh_note_turns_without_a_step() {
    let period = 48_000.0 / 261.63;
    let held = held_plain(&sine(48_000, period, 48_000), 48_000);
    let v = SourceView::strict(&held).unwrap();
    for (size, position, at) in [
        (1.0, 0.0, 1e-4f32),
        (0.3, 0.4, 1e-4),
        (1.0, 0.0, 0.0),
        (1.0, 0.0, 0.02),
        (0.3, 0.4, 0.02),
    ] {
        let mut p = TapeEngineParams {
            machine: TapeMachine::Scrub,
            size,
            xfade: 0.1,
            drag: 0.3,
            position,
            motion: at,
            ..TapeEngineParams::default()
        };
        let mut e = fresh();
        e.hold();
        e.note_on(60, &p, &v, SR);
        let mut out = Vec::new();
        let mut b = [0.0; BLOCK_SIZE];
        // MOTION falls 0.4 a second, as the QA's ramp (1.2 over 3 s), to −0.6.
        for n in 0..1_200 {
            p.motion = (at - n as f32 * 0.4 * B as f32 / SR as f32).max(-0.6);
            e.render(&mut b, &p, &v, SR);
            out.extend_from_slice(&b);
        }
        assert!(out.iter().all(|y| y.is_finite()));
        // At 1.2× the sine's own speed its step is 1.2×; 2× leaves room
        // for the fades' sum.
        let steady = 1.2 * max_step(&sine(48_000, period, 4_800));
        let worst = max_step(&out);
        assert!(
            worst <= 2.0 * steady,
            "SIZE {size} FINGER {position} MOTION {at}: {worst} against {steady}"
        );
    }
}
