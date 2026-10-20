//! TAPE's machines (spec § 2 Machines, ADR 0093, #369): PLAY (ONE, LOOP,
//! PING), TRON, STRETCH, TRANSWAVE, SCRUB; END before START plays
//! reversed; SLICE, WOW WARBLE, PIT's SNAP and SCRUB's jog. Precedent:
//! Elektron Digitakt II (machines; § Playback modes), Octatrack manual OS
//! 1.25 § Machines, Mellotron M400, CHOMPI (ideas only).

mod common;

use core::mem::MaybeUninit;

use chimera_core::block::{Block, DiskCode, ParamId};
use chimera_core::dsp::tape_engine::heads::{Dir, Window};
use chimera_core::dsp::tape_engine::machine::{MACHINES, defaults, machine};
use chimera_core::dsp::tape_engine::oneshot::{SLICE_NOTE_BASE, Slice, slice_bounds};
use chimera_core::dsp::tape_engine::params::hold;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape, SourceView};
use chimera_core::dsp::tape_engine::tron::{
    TRON_LEN_MAX_S, tron_len_s, tron_len_s_in, tron_run_samples,
};
use chimera_core::dsp::tape_engine::warble::Warble;
use chimera_core::dsp::tape_engine::{
    CLAMP_FADE_S, END_RAMP, FADE_HOLD, NOTE_ON_RAMP, PlayMode, Probe, StretchType, TapeEngine,
    TapeEngineParams, TapeModel, Wow, drag_s, fade_default, wave,
};
use chimera_core::params::{EngineType, PitchParams, Snap, musical_step};
use chimera_core::preset::Sound;
use chimera_core::storage::{MIGRATIONS, TRANSLATIONS, decode_block};
use chimera_core::ui::jog::Jog;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::rig::Rig;
use common::tape::{held_plain, held_transwave, saw_with_burst, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;
type P = TapeEngineParams;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    unsafe { slot.assume_init() }
}

/// `blocks` blocks of `e` under `p`, with each block's probe.
fn play(e: &mut TapeEngine, p: &P, v: &SourceView<'_>, blocks: usize) -> (Vec<f32>, Vec<Probe>) {
    let (mut out, mut probes) = (Vec::new(), Vec::new());
    let mut b = [0.0; B];
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

/// Samples until the engine went quiet for good: one past the last
/// non-zero sample.
fn sounding(out: &[f32]) -> usize {
    out.iter().rposition(|&y| y != 0.0).map_or(0, |i| i + 1)
}

/// A 48 kHz sine of `frames`, its period 100.7 frames.
fn sine_src(frames: u32) -> Vec<u8> {
    held_plain(&sine(48_000, 100.7, frames), 48_000)
}

#[test]
fn models_keep_their_codes() {
    use TapeModel as M;
    for (m, c) in [
        (M::Play, 0),
        (M::Transwave, 3),
        (M::Scrub, 4),
        (M::Stretch, 5),
        (M::Tron, 6),
    ] {
        assert_eq!(m.disk_code(), c);
        assert_eq!(M::from_disk_code(c), Some(m));
    }
    for c in [1, 2, 7] {
        assert_eq!(M::from_disk_code(c), None, "{c} is retired");
    }
    assert_eq!(
        M::ALL,
        [M::Play, M::Tron, M::Scrub, M::Stretch, M::Transwave]
    );
    // MODEL's value is the machine's place, PLAY's MODE in it; the disk
    // keeps the model's code and MODE's (ADR 0178).
    use chimera_core::dsp::tape_engine::TapeMachine as K;
    for (i, k) in K::ALL.into_iter().enumerate() {
        assert_eq!(k.index() as usize, i);
        assert_eq!(K::from_index(i as u8), k);
        let mut p = P::default();
        p.set(P::MODEL, i as f32);
        assert_eq!(p.get(P::MODEL), i as f32);
        assert_eq!((p.model, p.machine()), (k.model(), k));
        assert_eq!(K::of(k.model(), p.mode), k);
    }
    assert_eq!(K::from_index(200), K::Wave);
    for (m, c) in [(PlayMode::One, 0), (PlayMode::Loop, 1), (PlayMode::Ping, 2)] {
        assert_eq!(m.disk_code(), c);
        assert_eq!(PlayMode::from_disk_code(c), Some(m));
    }
    assert_eq!(PlayMode::from_disk_code(3), None);
}

/// A TAPE block record: `(id, raw)` entries.
fn block(entries: &[(u8, [u8; 4])]) -> Vec<u8> {
    let mut b = vec![34u8];
    for (id, raw) in entries {
        b.push(*id);
        b.extend_from_slice(raw);
    }
    b
}

#[test]
fn reverse_code_loads_as_loop_swapped() {
    let load = |payload: &[u8]| {
        let mut s = Sound::init(EngineType::Tape);
        decode_block(payload, MIGRATIONS, TRANSLATIONS, Some(&mut s.params)).unwrap();
        s.params.tape
    };
    let t = load(&block(&[
        (0, 1u32.to_le_bytes()),
        (1, 0.1f32.to_le_bytes()),
        (2, 0.7f32.to_le_bytes()),
    ]));
    assert_eq!(t.model, TapeModel::Play);
    assert_eq!((t.start, t.end), (0.7, 0.1));
    // Sparse: the base START 0, END 1, swapped after the writes.
    let t = load(&block(&[(0, 1u32.to_le_bytes())]));
    assert_eq!(t.model, TapeModel::Play);
    assert_eq!((t.start, t.end), (1.0, 0.0));
    // A live code is untouched.
    let t = load(&block(&[
        (0, 6u32.to_le_bytes()),
        (1, 0.1f32.to_le_bytes()),
    ]));
    assert_eq!((t.model, t.start, t.end), (TapeModel::Tron, 0.1, 1.0));
}

/// ONESHOT (7), LOOP (0), PING-PONG (2) and REVERSE (1) load as PLAY
/// (ADR 0170): ONE with FADE HOLD, LOOP and PING keeping their DRAG, and
/// LOOP with END before START.
#[test]
fn play_loads_old_codes() {
    let load = |payload: &[u8]| {
        let mut s = Sound::init(EngineType::Tape);
        decode_block(payload, MIGRATIONS, TRANSLATIONS, Some(&mut s.params)).unwrap();
        s.params.tape
    };
    let code = |c: u32| (0, c.to_le_bytes());
    let drag = (9, 0.6f32.to_le_bytes());
    let slice = (13, 1u32.to_le_bytes());
    let t = load(&block(&[code(7), drag, slice]));
    assert_eq!((t.model, t.mode), (TapeModel::Play, PlayMode::One));
    assert_eq!(t.drag, FADE_HOLD);
    assert!(hold(&t));
    assert_eq!(t.slice.code(), 0, "SLICE loads and is ignored");
    let t = load(&block(&[code(0), drag]));
    assert_eq!(
        (t.model, t.mode, t.drag),
        (TapeModel::Play, PlayMode::Loop, 0.6)
    );
    let t = load(&block(&[code(2), drag]));
    assert_eq!(
        (t.model, t.mode, t.drag),
        (TapeModel::Play, PlayMode::Ping, 0.6)
    );
    let t = load(&block(&[code(1), drag]));
    assert_eq!(
        (t.model, t.mode, t.drag),
        (TapeModel::Play, PlayMode::Loop, 0.6)
    );
    assert!(t.end < t.start);
    // A file of today names MODE itself: ONE keeps its own DRAG.
    let t = load(&block(&[code(0), (15, 0u32.to_le_bytes()), drag]));
    assert_eq!((t.mode, t.drag), (PlayMode::One, 0.6));
}

#[test]
fn fade_releases_loop_and_ping() {
    let held = sine_src(48_000);
    let v = SourceView::strict(&held).unwrap();
    for mode in [PlayMode::Loop, PlayMode::Ping] {
        let run = |drag: f32| {
            let p = P {
                mode,
                drag,
                size: 0.5,
                ..P::default()
            };
            let mut e = fresh();
            e.note_on(60, &p, &v, SR);
            let mut out = play(&mut e, &p, &v, 100).0;
            e.note_off();
            out.extend(play(&mut e, &p, &v, 400).0);
            (out, e.is_active())
        };
        let (short, on) = run(0.0);
        let (long, _) = run(0.8);
        assert!(!on, "{mode:?}: the 10 ms fade ends the note");
        let at = 100 * B;
        assert_eq!(short[..at], long[..at], "{mode:?}: before note-off");
        let tail = |x: &[f32]| {
            x[at + 40 * B..at + 44 * B]
                .iter()
                .map(|y| y.abs())
                .sum::<f32>()
        };
        assert!(
            tail(&short) < 0.05 * tail(&long),
            "{mode:?}: FADE sets the tail"
        );
    }
}

/// HOLD is FADE's last step in MODE ONE only: in LOOP and PING no DRAG
/// gives it, and DRAG's top renders its 500 ms.
#[test]
fn hold_only_in_one() {
    let mut p = P::default();
    for mode in [PlayMode::Loop, PlayMode::Ping] {
        p.mode = mode;
        for d in [0.0, 0.3, 0.99, 1.0] {
            p.drag = d;
            assert!(!hold(&p), "{mode:?} at {d}");
        }
        assert!((drag_s(1.0) - 0.5).abs() < 1e-5);
    }
    p.mode = PlayMode::One;
    p.drag = 0.99;
    assert!(!hold(&p));
    p.drag = FADE_HOLD;
    assert!(hold(&p));
    // Only PLAY has MODEs.
    p.model = TapeModel::Tron;
    assert!(!hold(&p));
    assert_eq!(fade_default(PlayMode::One), FADE_HOLD);
    for mode in [PlayMode::Loop, PlayMode::Ping] {
        assert_eq!(fade_default(mode), 0.3);
        assert!((drag_s(fade_default(mode)) - 0.032).abs() < 1e-3);
    }
}

/// Task 6's `reverse_is_loop_mirrored`, ported: LOOP with END before START
/// is LOOP on the reversed source, the window mirrored.
#[test]
fn order_reverses_loop() {
    let x = saw_with_burst(48_000, 0.5);
    let rev: Vec<f32> = x.iter().rev().copied().collect();
    let (held, held_rev) = (held_plain(&x, 48_000), held_plain(&rev, 48_000));
    let (v, v_rev) = (
        SourceView::strict(&held).unwrap(),
        SourceView::strict(&held_rev).unwrap(),
    );
    for (start, end, position, size, xfade, motion) in [
        (1.0, 0.0, 0.0, 1.0, 0.1, 0.0),
        (0.8, 0.2, 0.3, 0.6, 0.2, 0.0),
        (0.9, 0.1, 0.7, 0.4, 0.5, 0.3),
    ] {
        let p = P {
            start,
            end,
            position,
            size,
            xfade,
            motion,
            ..P::default()
        };
        let q = P {
            start: 1.0 - start,
            end: 1.0 - end,
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
        assert_eq!(a.dirs(), [Dir::Rev; 2]);
    }
}

#[test]
fn min_window_both_sides() {
    let e = 1e-5;
    assert_eq!(
        Window::of(0.5, 0.5 + e, 10_000, 96),
        Window {
            lo: 5_000,
            hi: 5_096,
            dir: Dir::Fwd
        }
    );
    assert_eq!(
        Window::of(0.5, 0.5 - e, 10_000, 96),
        Window {
            lo: 4_904,
            hi: 5_000,
            dir: Dir::Rev
        }
    );
    assert_eq!(Window::of(0.5, 0.5, 10_000, 96).dir, Dir::Fwd);
    assert_eq!(Window::of(0.5, 0.5, 10_000, 96).lo, 5_000);
    // At the source's edges the head still starts at START: the window
    // shortens there rather than reach past it (12b review F1).
    assert_eq!(
        Window::of(0.001, 0.0, 10_000, 96),
        Window {
            lo: 0,
            hi: 10,
            dir: Dir::Rev
        }
    );
    assert_eq!(
        Window::of(0.0001, 0.0, 10_000, 96),
        Window {
            lo: 0,
            hi: 1,
            dir: Dir::Rev
        }
    );
    assert_eq!(Window::of(1e-6, 0.0, 10_000, 96).frames(), 1, "never empty");
    assert_eq!(
        Window::of(0.999, 1.0, 10_000, 96),
        Window {
            lo: 9_990,
            hi: 10_000,
            dir: Dir::Fwd
        }
    );
    assert_eq!(Window::of(1.0, 1.0, 10_000, 96).frames(), 1, "never empty");
    // Wide enough, it is the sorted pair.
    assert_eq!(
        Window::of(0.8, 0.2, 10_000, 96),
        Window {
            lo: 2_000,
            hi: 8_000,
            dir: Dir::Rev
        }
    );
}

#[test]
fn flip_lands_at_a_wrap() {
    let held = sine_src(48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        start: 0.5,
        end: 0.9,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    play(&mut e, &p, &v, 40);
    // END stepped across START mid-pass.
    let q = P { end: 0.1, ..p };
    let mut last = e.probe().head;
    let (mut before, mut during, mut after, mut falls) = (0, 0, 0, 0);
    for _ in 0..1_000 {
        let (_, pr) = play(&mut e, &q, &v, 1);
        let pr = pr[0];
        match e.dirs() {
            [Dir::Fwd, Dir::Fwd] => {
                assert_eq!(during + after, 0, "no flip back");
                assert!(pr.head > last, "forward until the wrap");
                before += 1;
            }
            [Dir::Fwd, Dir::Rev] => {
                assert!(pr.other.is_some(), "the flip fades");
                assert!(pr.head > last, "the old head runs on the old way");
                during += 1;
            }
            [Dir::Rev, Dir::Rev] => {
                assert!(during > 0, "only through a fade");
                // Down, but for its own wraps back up.
                if pr.head < last {
                    falls += 1;
                }
                after += 1;
            }
            d => panic!("{d:?}"),
        }
        last = pr.head;
    }
    assert!(before > 50, "{before} blocks before the wrap");
    assert!(during > 10 && after > 100, "{during} fading, {after} after");
    assert!(
        falls * 10 >= after * 9,
        "{falls} of {after} blocks reversed"
    );
}

/// A sine; END swept 1 → 0 → 1 across START 0.5 over 2 s, every block.
fn sweep_end(xfade: f32) -> (Vec<f32>, Vec<[Dir; 2]>) {
    let held = sine_src(48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        start: 0.5,
        xfade,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let blocks = 2 * SR as usize / B;
    let (mut out, mut dirs) = (Vec::new(), Vec::new());
    let mut b = [0.0; B];
    for n in 0..blocks {
        let t = n as f32 / blocks as f32;
        let q = P {
            end: (2.0 * t - 1.0).abs(),
            ..p
        };
        e.render(&mut b, &q, &v, SR);
        out.extend_from_slice(&b);
        dirs.push(e.dirs());
    }
    (out, dirs)
}

#[test]
fn direction_flips_are_click_free() {
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    for xfade in [0.1, 0.5] {
        let (out, dirs) = sweep_end(xfade);
        let flips = dirs.iter().filter(|d| d[0] != d[1]).count();
        assert!(flips > 0, "XFADE {xfade}: no flip");
        let worst = max_step(&out[B..]);
        assert!(
            worst <= 4.0 * steady,
            "XFADE {xfade}: {worst} against {steady}"
        );
    }
    // XFADE 0 splices hard by request, but a flip takes the clamp's 5 ms
    // fade: about 240 samples a flip.
    let (_, dirs) = sweep_end(0.0);
    let len = (CLAMP_FADE_S * SR as f32) as usize;
    let mut runs = Vec::new();
    let mut n = 0;
    for d in &dirs {
        if d[0] != d[1] {
            n += 1;
        } else if n > 0 {
            runs.push(n);
            n = 0;
        }
    }
    assert!(!runs.is_empty());
    for n in runs {
        // 240 samples span 4 or 5 blocks' ends.
        assert!(
            (len / B..=len / B + 1).contains(&n),
            "a flip faded over {n} blocks"
        );
    }
    // A flip at XFADE 0 with no hard splice near it (the loop is the
    // window, its first pass running): no click across it.
    let held = sine_src(48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        start: 0.5,
        end: 0.9,
        xfade: 0.0,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _) = play(&mut e, &p, &v, 40);
    let q = P { end: 0.1, ..p };
    let mut span = None;
    for n in 40..600 {
        out.extend(play(&mut e, &q, &v, 1).0);
        if e.dirs()[0] != e.dirs()[1] && span.is_none() {
            span = Some(n);
        }
    }
    let at = span.expect("a flip");
    let worst = max_step(&out[(at - 2) * B..(at + 8) * B]);
    assert!(worst <= 4.0 * steady, "flip: {worst} against {steady}");
}

#[test]
fn stretch_flip_lands_at_a_splice() {
    let held = sine_src(96_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        model: TapeModel::Stretch,
        start: 0.5,
        end: 0.9,
        motion: 0.5,
        period: 100.7,
        size: 0.3,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _) = play(&mut e, &p, &v, 30);
    let q = P { end: 0.1, ..p };
    let mut prev = e.dirs();
    let mut starts = Vec::new();
    let (mut last, mut down, mut both) = (e.probe(), 0, 0);
    for _ in 0..600 {
        let pos_before = e.grain().pos;
        let (y, pr) = play(&mut e, &q, &v, 1);
        // Both grains reversed and no splice between: both read down.
        if prev == [Dir::Rev; 2] && e.dirs() == [Dir::Rev; 2] && e.grain().pos > pos_before {
            both += 1;
            if pr[0].head < last.head && pr[0].other < last.other {
                down += 1;
            }
        }
        last = pr[0];
        out.extend_from_slice(&y);
        let d = e.dirs();
        let spliced = e.grain().pos < pos_before;
        if d != prev {
            assert!(
                spliced,
                "the grains turned only at a splice: {prev:?} → {d:?}"
            );
        }
        if spliced && d == [Dir::Rev; 2] {
            starts.push(pr[0].other.unwrap());
        }
        let g = pr[0].gain;
        assert!((0.0..=1.0).contains(&g));
        assert!((g + (1.0 - g) - 1.0).abs() < 1e-6);
        prev = d;
    }
    assert_eq!(prev, [Dir::Rev; 2]);
    assert!(
        both > 50 && down * 10 >= both * 9,
        "{down} of {both} blocks read down"
    );
    // Reversed, time walks from START (frame 48 000) down.
    assert!(starts.len() > 10, "{} reversed splices", starts.len());
    let falls = starts.windows(2).filter(|w| w[1] < w[0]).count();
    assert!(falls * 10 >= starts.len() * 8, "{starts:?}");
    assert!(starts[0] <= 48_000 && starts[0] > 40_000, "{}", starts[0]);
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    let worst = max_step(&out[B..]);
    assert!(worst <= 4.0 * steady, "{worst} against {steady}");
}

/// Reversed, STRETCH's two grains move down as one, at the note's speed
/// as it ramps (a pitch moving every block): neither runs off on its own.
#[test]
fn stretch_reversed_grains_move_as_one() {
    let held = sine_src(96_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        model: TapeModel::Stretch,
        start: 0.9,
        end: 0.1,
        motion: 0.5,
        size: 1.0,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    assert_eq!(e.dirs(), [Dir::Rev; 2]);
    let mut last = e.probe();
    let mut checked = 0;
    for n in 0..400 {
        e.set_pitch(if n % 2 == 0 { 600.0 } else { -600.0 });
        let pos_before = e.grain().pos;
        let (_, pr) = play(&mut e, &p, &v, 1);
        let pr = pr[0];
        let d0 = last.head as i64 - pr.head as i64;
        let d1 = last.other.unwrap() as i64 - pr.other.unwrap() as i64;
        // No splice, no wrap: both read down by the same travel.
        if e.grain().pos > pos_before && (1..200).contains(&d0) && (1..200).contains(&d1) {
            assert!((d0 - d1).abs() <= 1, "block {n}: {d0} against {d1}");
            checked += 1;
        }
        last = pr;
    }
    assert!(checked > 100, "{checked} blocks");
}

#[test]
fn sorted_window_models_ignore_order() {
    let held = sine_src(48_000);
    let wave = held_transwave(512, 4, |f, h| if h == 1 + f as u32 { 0.5 } else { 0.0 });
    for (model, mode, motion) in [
        (TapeModel::Play, PlayMode::Ping, 0.2),
        (TapeModel::Scrub, PlayMode::Loop, 0.5),
        (TapeModel::Transwave, PlayMode::Loop, 0.3),
    ] {
        let src = if model == TapeModel::Transwave {
            &wave
        } else {
            &held
        };
        let v = SourceView::strict(src).unwrap();
        let p = P {
            model,
            mode,
            start: 0.2,
            end: 0.8,
            motion,
            size: 0.5,
            position: 0.3,
            ..P::default()
        };
        let q = P {
            start: 0.8,
            end: 0.2,
            ..p
        };
        let (mut a, mut b) = (fresh(), fresh());
        a.note_on(60, &p, &v, SR);
        b.note_on(60, &q, &v, SR);
        let (ya, _) = play(&mut a, &p, &v, 400);
        let (yb, _) = play(&mut b, &q, &v, 400);
        for (i, (s, t)) in ya.iter().zip(&yb).enumerate() {
            assert_eq!(s.to_bits(), t.to_bits(), "{model:?}: sample {i}");
        }
    }
}

/// A ramp from −0.9 to +0.9 over `frames` at 48 kHz.
fn ramp_src(frames: u32) -> Vec<u8> {
    let x: Vec<f32> = (0..frames)
        .map(|i| -0.9 + 1.8 * i as f32 / frames as f32)
        .collect();
    held_plain(&x, 48_000)
}

#[test]
fn oneshot_plays_once() {
    let held = ramp_src(10_000);
    let v = SourceView::strict(&held).unwrap();
    for (start, end, note, inc) in [
        (0.2, 0.6, 60, 1.0f32),
        (0.6, 0.2, 60, 1.0),
        (0.2, 0.6, 72, 2.0),
    ] {
        let p = P {
            model: TapeModel::Play,
            mode: PlayMode::One,
            drag: FADE_HOLD,
            start,
            end,
            ..P::default()
        };
        let mut e = fresh();
        e.note_on(note, &p, &v, SR);
        let (out, probes) = play(&mut e, &p, &v, 100);
        let want = 4_000.0 / inc;
        let n = sounding(&out) as f32;
        assert!(
            (n - want).abs() <= 1.0,
            "{start}..{end}: {n} against {want}"
        );
        assert!(!e.is_active());
        assert!(out[n as usize..].iter().all(|&y| y == 0.0));
        // The head runs START → END, once.
        let heads: Vec<i64> = probes[..(n as usize / B)]
            .iter()
            .map(|p| p.head as i64)
            .collect();
        let up = end > start;
        assert!(heads.windows(2).all(|w| (w[1] > w[0]) == up));
        // It reads the ramp from START: rising forward, falling reversed.
        let mid = &out[1_000..1_100];
        assert_eq!(mid[50] > mid[0], up, "{start}..{end}");
    }
}

/// `blocks` of a TAPE voice on INIT as PLAY in `mode` with FADE `fade`, its
/// key up after `off` blocks (if any): its output, and the block it went
/// idle.
fn voice_run(
    (mode, fade): (PlayMode, f32),
    off: Option<usize>,
    blocks: usize,
) -> (Vec<f32>, Option<usize>) {
    let mut s = Sound::init(EngineType::Tape);
    (s.params.tape.mode, s.params.tape.drag) = (mode, fade);
    let mut r = Rig::new(SR);
    r.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &s.params,
    );
    let (mut out, mut idle) = (Vec::new(), None);
    let mut b = [0.0; B];
    for n in 0..blocks {
        if Some(n) == off {
            r.note_off();
        }
        r.render(&mut b, &s.params, &s.mod_state);
        out.extend_from_slice(&b);
        if idle.is_none() && !r.is_active() {
            idle = Some(n);
        }
    }
    (out, idle)
}

#[test]
fn oneshot_ignores_note_off() {
    // The rig's second of 24 kHz saw at note 60, ROOT C4: 1 s.
    let end_block = SR as usize / B;
    let one = (PlayMode::One, FADE_HOLD);
    let (held, idle) = voice_run(one, None, end_block + 20);
    let (released, idle_r) = voice_run(one, Some(8), end_block + 20);
    assert!(held.iter().any(|&y| y.abs() > 1e-3));
    for (i, (a, b)) in held.iter().zip(&released).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "sample {i}");
    }
    let idle = idle.expect("ends at END");
    assert_eq!(idle_r, Some(idle));
    assert!(
        idle.abs_diff(end_block) <= 1,
        "idle at {idle}, END at {end_block}"
    );
    // LOOP lets go at key up (its INIT envelope's release).
    let (_, idle_loop) = voice_run((PlayMode::Loop, 0.3), Some(8), end_block + 20);
    assert!(idle_loop.is_some_and(|n| n < end_block));
    // ONE under a FADE short of HOLD lets go like TRON: through FADE.
    let (_, idle_fade) = voice_run((PlayMode::One, 0.3), Some(8), end_block + 20);
    assert!(idle_fade.is_some_and(|n| n < end_block));
}

#[test]
fn oneshot_end_is_click_free() {
    let held = sine_src(48_000);
    let v = SourceView::strict(&held).unwrap();
    for (start, end) in [(0.1, 0.5013), (0.5013, 0.1)] {
        let p = P {
            model: TapeModel::Play,
            mode: PlayMode::One,
            drag: FADE_HOLD,
            start,
            end,
            ..P::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (out, _) = play(&mut e, &p, &v, 400);
        let n = sounding(&out);
        // The last 48 samples ramp to 0, a smoothstep.
        for k in 0..END_RAMP as usize {
            let left = END_RAMP as usize - k;
            let x = left as f32 / END_RAMP as f32;
            let y = out[n - left].abs();
            assert!(
                y <= x * x * (3.0 - 2.0 * x) + 1e-3,
                "{left} before the end: {y}"
            );
        }
        let steady = max_step(&sine(48_000, 100.7, 4_800));
        let worst = max_step(&out[B..]);
        assert!(worst <= 4.0 * steady, "{worst} against {steady}");
    }
}

#[test]
fn slices_are_exact() {
    for w in [64u32, 1_000, 1_023, 48_001] {
        for dir in [Dir::Fwd, Dir::Rev] {
            let win = Window {
                lo: 7,
                hi: 7 + w,
                dir,
            };
            let mut seen = Vec::new();
            for k in 0..16u8 {
                let s = slice_bounds(win, Slice::At(k), 0);
                let j = if dir == Dir::Rev { 15 - k } else { k } as u64;
                let edge = |j: u64| 7 + (j * w as u64 / 16) as u32;
                assert_eq!(
                    (s.lo, s.hi, s.dir),
                    (edge(j), edge(j + 1), dir),
                    "W {w} slice {k}"
                );
                seen.push((s.lo, s.hi));
            }
            seen.sort();
            assert_eq!(seen[0].0, 7);
            assert_eq!(seen[15].1, 7 + w);
            assert!(
                seen.windows(2).all(|p| p[0].1 == p[1].0),
                "W {w}: gap or overlap"
            );
            if dir == Dir::Rev {
                assert_eq!(
                    slice_bounds(win, Slice::At(0), 0).hi,
                    7 + w,
                    "slice 1 is the top"
                );
            }
        }
    }
    // A window shorter than 16 frames (START at a source's edge): no
    // slice is empty, and each stays inside it.
    for w in 1..16u32 {
        for dir in [Dir::Fwd, Dir::Rev] {
            let win = Window { lo: 0, hi: w, dir };
            for k in 0..16u8 {
                let s = slice_bounds(win, Slice::At(k), 0);
                assert!(s.lo < s.hi && s.hi <= w, "W {w} slice {k}: {s:?}");
            }
        }
    }
    let win = Window {
        lo: 0,
        hi: 1_600,
        dir: Dir::Fwd,
    };
    assert_eq!(slice_bounds(win, Slice::Off, 60), win);
    for v in 0..18 {
        let s = Slice::of(v as f32);
        assert_eq!(
            s,
            match v {
                0 => Slice::Off,
                1 => Slice::Note,
                k => Slice::At(k - 2),
            }
        );
    }
}

#[test]
fn slice_note_picks_without_transposing() {
    let win = Window {
        lo: 0,
        hi: 1_600,
        dir: Dir::Fwd,
    };
    for (note, k) in [
        (60u8, 0u32),
        (75, 15),
        (76, 0),
        (59, 15),
        (SLICE_NOTE_BASE + 4, 4),
    ] {
        let s = slice_bounds(win, Slice::Note, note);
        assert_eq!((s.lo, s.hi), (k * 100, k * 100 + 100), "note {note}");
    }
    let held = sine_src(96_000);
    let v = SourceView::strict(&held).unwrap();
    let inc_of = |slice: u8, note: u8, pitch: f32| {
        let p = P {
            model: TapeModel::Play,
            mode: PlayMode::One,
            drag: FADE_HOLD,
            slice: chimera_core::dsp::tape_engine::SliceCode::new(slice),
            ..P::default()
        };
        let mut e = fresh();
        e.set_pitch(pitch);
        e.note_on(note, &p, &v, SR);
        play(&mut e, &p, &v, 2);
        e.inc_now().unwrap()
    };
    // SLICE NOTE: every note at ROOT's pitch (C4, 48 kHz: 1), within 0.1
    // cent; PITCH still applies.
    let cent = 2f32.powf(0.1 / 1_200.0) - 1.0;
    for note in [0u8, 36, 59, 60, 61, 75, 76, 127] {
        let i = inc_of(1, note, 0.0);
        assert!((i - 1.0).abs() <= cent, "note {note}: {i}");
        let i = inc_of(1, note, 1_200.0);
        assert!((i - 2.0).abs() <= 2.0 * cent, "note {note} +12: {i}");
    }
    // SLICE 5 (its value 6): the note transposes.
    assert!((inc_of(6, 72, 0.0) - 2.0).abs() <= 2.0 * cent);
    assert!((inc_of(6, 48, 0.0) - 0.5).abs() <= cent);
}

/// Samples a TRON note on a 4 s, 48 kHz sine sounds, held throughout.
fn tron_len(note: u8, tape: f32, size: f32) -> usize {
    let held = sine_src(4 * 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        model: TapeModel::Tron,
        size,
        tape,
        ..P::default()
    };
    let mut e = fresh();
    e.seed(1);
    e.note_on(note, &p, &v, SR);
    let (out, _) = play(&mut e, &p, &v, 5 * SR as usize / B);
    assert!(!e.is_active());
    sounding(&out)
}

#[test]
fn tron_runs_out_after_size_at_every_pitch() {
    assert_eq!(tron_len_s(1.0), TRON_LEN_MAX_S);
    assert!((tron_len_s(0.0) - 0.25).abs() < 1e-6);
    assert!((tron_len_s(0.6) - 2.0).abs() < 1e-4);
    assert_eq!(tron_run_samples(2.0, SR), 96_000);
    // LENGTH runs over the strip (#698): on a 4 s one, 0.25 s to 4 s.
    assert!((tron_len_s_in(1.0, 4.0) - 4.0).abs() < 1e-4);
    assert!((tron_len_s_in(0.0, 4.0) - 0.25).abs() < 1e-6);
    assert!((tron_len_s_in(1.0, 20.0) - TRON_LEN_MAX_S).abs() < 1e-4);
    let run = tron_run_samples(tron_len_s_in(0.6, 4.0), SR) as i64;
    for (note, tape) in [(48u8, 0.0), (60, 0.0), (67, 0.0), (60, 1.0)] {
        let n = tron_len(note, tape, 0.6) as i64;
        assert!((n - run).abs() <= 48, "note {note} TAPE {tape}: {n}");
    }
    // LENGTH's top is never dead: 1 plays the strip to END at ROOT, and
    // 0.9 less of it.
    let (top, under) = (tron_len(60, 0.0, 1.0) as i64, tron_len(60, 0.0, 0.9) as i64);
    assert!(
        (top - 4 * 48_000).abs() <= 48 && under < top - 10_000,
        "{top}, {under}"
    );
    // An octave and a fifth up reads 2^(19/12) of the source a sample:
    // END comes first.
    let want = 4.0 / 2f64.powf(19.0 / 12.0) * 48_000.0;
    let n = tron_len(79, 0.0, 1.0) as f64;
    assert!((n - want).abs() <= 48.0, "{n} against {want}");
}

#[test]
fn tron_never_loops() {
    let held = sine_src(48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        model: TapeModel::Tron,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, probes) = play(&mut e, &p, &v, 3 * SR as usize / B);
    let n = sounding(&out);
    assert!((n as i64 - 48_000).abs() <= 48, "{n}");
    let live = &probes[..n / B];
    assert!(live.windows(2).all(|w| w[1].head > w[0].head), "monotone");
    assert!(probes.iter().all(|p| p.other.is_none()));
    assert!(!e.is_active());
}

#[test]
fn tron_release_fades() {
    // A constant: the output is the release's gain itself.
    let held = held_plain(&vec![0.5; 10 * 48_000], 48_000);
    let v = SourceView::strict(&held).unwrap();
    for fade in [0.3, 0.0, 1.0] {
        let p = P {
            model: TapeModel::Tron,
            drag: fade,
            ..P::default()
        };
        let mut e = fresh();
        e.note_on(60, &p, &v, SR);
        let (mut out, _) = play(&mut e, &p, &v, SR as usize / B);
        let at = out.len();
        e.note_off();
        let (rest, _) = play(&mut e, &p, &v, 6 * SR as usize / B);
        out.extend_from_slice(&rest);
        let tau = drag_s(fade) as f64 * SR as f64;
        for k in (0..(5.0 * tau) as usize).step_by(7) {
            let want = 0.5 * (-(k as f64 + 1.0) / tau).exp();
            let got = out[at + k] as f64;
            assert!(
                (got - want).abs() <= 0.01 * want,
                "FADE {fade}: {k}: {got} against {want}"
            );
        }
        assert!(!e.is_active(), "FADE {fade}");
        let n = sounding(&out) - at;
        let floor = tau * 1e4f64.ln();
        // Ended in the block it fell under −80 dB, the gain's product
        // within 0.1 % of the exponential's.
        assert!(
            (n as f64 - floor).abs() <= B as f64 + 1e-3 * floor,
            "{n} against {floor}"
        );
    }
    // A sine: no click at key up.
    let held = sine_src(4 * 48_000);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        model: TapeModel::Tron,
        drag: 0.0,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (mut out, _) = play(&mut e, &p, &v, 100);
    e.note_off();
    out.extend(play(&mut e, &p, &v, 100).0);
    let steady = max_step(&sine(48_000, 100.7, 4_800));
    assert!(max_step(&out[B..]) <= 4.0 * steady);
}

#[test]
fn tron_refuses_a_zone_set() {
    let d = Descriptor {
        shape: Shape::ZoneSet,
        count: 3,
        rate: 32_000,
        head_frames: 0,
        heads: 0,
    };
    let mut zs = d.to_bytes().to_vec();
    zs.resize(DESC_LEN + 27 + 3 * 33, 0);
    let zs = SourceView::new(&zs).unwrap();
    assert!(!wave::playable(TapeModel::Tron, &zs));
    let plain = sine_src(4_800);
    let wv = held_transwave(512, 2, |_, h| if h == 1 { 0.5 } else { 0.0 });
    for held in [&plain, &wv] {
        assert!(wave::playable(
            TapeModel::Tron,
            &SourceView::new(held).unwrap()
        ));
    }
}

/// `blocks` of a voice's WARBLE at TAPE 1, cents.
fn warble(seed: u32, blocks: usize) -> Vec<f32> {
    let mut w = Warble::new();
    w.seed(seed);
    (0..blocks).map(|_| w.cents(1.0, 750.0)).collect()
}

#[test]
fn warble_bounds() {
    let d = warble(1, 8 * 750);
    let peak = d.iter().fold(0.0f32, |m, c| m.max(c.abs()));
    assert!((20.0..=33.0).contains(&peak), "{peak}");
    // A glide: a block moves at most 1/(0.2 s · 750) of the way to a
    // target at most 66 cents off.
    let most = 66.0 / (0.2 * 750.0);
    let step = d.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(step <= most + 1e-4, "{step} a block");
    assert!(step > 0.3 * most, "it moves: {step}");
    // TAPE scales it.
    let mut w = Warble::new();
    w.seed(1);
    let half: Vec<f32> = (0..100).map(|_| w.cents(0.5, 750.0)).collect();
    for (a, b) in half.iter().zip(&d) {
        assert!((a - 0.5 * b).abs() < 1e-5);
    }
}

fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len()) as f64;
    let (ma, mb) = (
        a.iter().map(|&x| x as f64).sum::<f64>() / n,
        b.iter().map(|&x| x as f64).sum::<f64>() / n,
    );
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (x as f64 - ma, y as f64 - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa * bb).sqrt()
}

#[test]
fn warble_is_not_periodic() {
    let d = warble(3, 120 * 750);
    let ac = |lag: usize| correlation(&d[..d.len() - lag], &d[lag..]);
    // The capstan's wow repeats every 2 s; the warble has no line: past
    // its glide, no lag correlates.
    let lags: Vec<(usize, f64)> = (150..=3_000).step_by(15).map(|l| (l, ac(l))).collect();
    for w in lags.windows(3) {
        let (l, c) = w[1];
        if c > w[0].1 && c > w[2].1 {
            assert!(c <= 0.5, "a peak of {c} at {} s", l as f64 / 750.0);
        }
    }
    assert!(ac(1_500) < 0.5, "{}", ac(1_500));
}

#[test]
fn warble_voices_differ() {
    let r = correlation(&warble(1, 8 * 750), &warble(2, 8 * 750));
    assert!(r.abs() < 0.5, "{r}");
}

/// The table, as spec § 2 Machines lists it, laid out by ADR 0178: DECK
/// the glyph's knobs, the model page the rest.
#[test]
fn machine_table_is_pinned() {
    use chimera_core::dsp::tape_engine::TapeMachine as K;
    type C = (ParamId, &'static str, &'static str, f32);
    type Row = (K, &'static str, &'static [C], &'static [C]);
    const LEN: C = (P::SIZE, "LENGTH", "LEN", 1.0);
    const LOOP_LEN: C = (P::SIZE, "LENGTH", "LEN", 0.8);
    const XFD: C = (P::XFADE, "XFADE", "XFD", 0.1);
    const FADE: C = (P::DRAG, "FADE", "FDE", 0.3);
    const POS: C = (P::POSITION, "POSITION", "POS", 0.0);
    const WALK: C = (P::MOTION, "WALK", "WLK", 0.0);
    let want: [Row; 7] = [
        (
            K::One,
            "ONE",
            &[(P::START, "START", "STA", 0.0), (P::END, "END", "END", 1.0)],
            &[(P::DRAG, "FADE", "FDE", FADE_HOLD)],
        ),
        (K::Loop, "LOOP", &[POS, WALK, LOOP_LEN, XFD], &[FADE]),
        (K::Ping, "PING", &[POS, WALK, LOOP_LEN], &[FADE]),
        (K::Tron, "TRON", &[LEN, FADE], &[]),
        (
            K::Scrub,
            "SCRUB",
            &[
                (P::POSITION, "FINGER", "FNG", 0.0),
                (P::MOTION, "SPEED", "SPD", 0.5),
                LEN,
                XFD,
                (P::DRAG, "DRAG", "DRG", 0.3),
            ],
            &[],
        ),
        (
            K::Stretch,
            "STRCH",
            &[
                (P::MOTION, "SPEED", "SPD", 0.5),
                (P::SIZE, "D-TIME", "DTM", 0.5),
            ],
            &[POS, (P::TYPE, "TYPE", "TYP", 0.0)],
        ),
        (
            K::Wave,
            "WAVE",
            &[
                (P::POSITION, "FRAME", "FRM", 0.0),
                (P::MOTION, "SCAN", "SCN", 0.0),
                (P::SIZE, "RANGE", "RNG", 1.0),
            ],
            &[],
        ),
    ];
    let row =
        |c: &chimera_core::dsp::tape_engine::machine::Cell| (c.id, c.label, c.short, c.default);
    for (i, (k, page, deck, cells)) in want.into_iter().enumerate() {
        let m = &MACHINES[i];
        assert_eq!(m.machine, k);
        assert_eq!(machine(k).page, page);
        assert_eq!(m.deck.iter().map(row).collect::<Vec<_>>(), deck, "{k:?}");
        assert_eq!(m.cells.iter().map(row).collect::<Vec<_>>(), cells, "{k:?}");
        for c in m.deck.iter().chain(m.cells) {
            for word in [c.label, c.short] {
                assert!(word != "TAPE" && word != "REL", "{k:?}: {word}");
            }
            assert!(c.short.len() <= 3, "{k:?}: {}", c.short);
        }
    }
}

/// A machine's cell moved from its default (`busy` keeps every path in
/// play: TAPE on, so WOW is heard; a moving POSITION, so DRAG is): the
/// render changes, on every machine, ONE, LOOP and PING each PLAY in its
/// MODE.
#[test]
fn every_cell_reads_in_the_audio() {
    let held = held_plain(&saw_with_burst(48_000, 1.0), 48_000);
    let wave = held_transwave(512, 4, |f, h| if h <= 1 + f as u32 { 0.5 } else { 0.0 });
    for m in &MACHINES {
        let (model, mode) = (
            m.machine.model(),
            m.machine.mode().unwrap_or(PlayMode::Loop),
        );
        let src = if model == TapeModel::Transwave {
            &wave
        } else {
            &held
        };
        let v = SourceView::strict(src).unwrap();
        let mut base = P {
            model,
            mode,
            tape: 0.6,
            period: 109.1,
            ..P::default()
        };
        for (id, d) in defaults(m.machine) {
            base.set(id, d);
        }
        match model {
            TapeModel::Transwave => base.motion = 0.3,
            // A loop shorter than the window, so POSITION places it.
            TapeModel::Play | TapeModel::Scrub => base.size = 0.6,
            _ => {}
        }
        let render = |p: P| {
            let mut e = fresh();
            e.seed(1);
            e.note_on(60, &p, &v, SR);
            let mut out = Vec::new();
            let mut b = [0.0; B];
            for n in 0..400 {
                // POSITION moves halfway through: SCRUB's DRAG follows it.
                let q = P {
                    position: if n < 200 { p.position } else { 0.6 },
                    ..p
                };
                e.render(&mut b, &q, &v, SR);
                out.extend_from_slice(&b);
                if n == 300 {
                    e.note_off();
                }
            }
            out
        };
        let plain = render(base);
        for c in m.deck.iter().chain(m.cells) {
            let spec = TapeEngineParams::default().spec(c.id).unwrap();
            let mut p = base;
            let moved = if c.id == P::START {
                0.2
            } else if c.id == P::END {
                // Inside the 400 blocks heard.
                0.15
            } else if spec.max <= 1.0 && spec.kind == chimera_core::block::ParamKind::Enum {
                1.0 - c.default
            } else if c.id == P::SLICE {
                6.0
            } else {
                spec.min + 0.1 * (spec.max - spec.min)
            };
            p.set(c.id, moved);
            assert!(
                render(p) != plain,
                "{:?}: {} moved to {moved} changes nothing",
                m.machine,
                c.label
            );
        }
    }
}

#[test]
fn machine_defaults_list() {
    use chimera_core::dsp::tape_engine::TapeMachine as K;
    let stretch: Vec<(ParamId, f32)> = defaults(K::Stretch).collect();
    for want in [
        (P::MOTION, 0.5),
        (P::SIZE, 0.5),
        (P::TYPE, StretchType::Mono as u8 as f32),
    ] {
        assert!(stretch.contains(&want), "{want:?} in {stretch:?}");
    }
    assert!(defaults(K::One).any(|d| d == (P::DRAG, FADE_HOLD)));
    for m in K::ALL {
        for (id, _) in defaults(m) {
            // TRON's wear is its identity (#698): TAPE and WARBLE.
            let wear = m == K::Tron && [P::TAPE, P::WOW].contains(&id);
            assert!(
                wear || ![P::MODEL, P::MODE, P::START, P::END, P::TAPE, P::AGE, P::WOW]
                    .contains(&id),
                "{m:?} sets {id:?}"
            );
        }
    }
    let tron: Vec<(ParamId, f32)> = defaults(K::Tron).collect();
    assert!(tron.contains(&(P::TAPE, 0.3)) && tron.contains(&(P::WOW, Wow::Warble as u8 as f32)));
    // LOOP's are the neutral's but LENGTH, 0.8 so POSITION moves the loop.
    let n = P::default();
    assert_eq!(n.machine(), K::Loop);
    for (id, d) in defaults(K::Loop) {
        let want = if id == P::SIZE { 0.8 } else { n.get(id) };
        assert_eq!(want, d, "{id:?}");
    }
    assert_eq!(n.wow, Wow::Capstan);
}

#[test]
fn musical_snap_steps_fifths_and_fourths() {
    let mut p = PitchParams::default();
    p.set(PitchParams::SNAP, 1.0);
    assert_eq!(p.snap, Snap::Musical);
    let mut up = Vec::new();
    for _ in 0..5 {
        p.nudge(PitchParams::PITCH, 1);
        up.push(p.pitch);
    }
    assert_eq!(up, [7.0, 12.0, 19.0, 24.0, 24.0]);
    p.pitch = 0.0;
    let mut down = Vec::new();
    for _ in 0..4 {
        p.nudge(PitchParams::PITCH, -1);
        down.push(p.pitch);
    }
    assert_eq!(down, [-7.0, -12.0, -19.0, -24.0]);
    assert_eq!(musical_step(5.0, 1), 7.0);
    assert_eq!(musical_step(5.0, -1), 0.0);
    assert_eq!(musical_step(-24.0, -1), -24.0);
    assert_eq!(musical_step(-20.0, 1), -19.0);
    p.pitch = 0.0;
    p.nudge(PitchParams::PITCH, 2);
    assert_eq!(p.pitch, 12.0, "two clicks an octave");
    // FINE steps as ever under MUSICAL; CHROMATIC steps semitones.
    p.nudge(PitchParams::FINE, 3);
    assert_eq!(p.fine, 3.0);
    p.set(PitchParams::SNAP, 0.0);
    p.pitch = 5.0;
    p.nudge(PitchParams::PITCH, 1);
    assert_eq!(p.pitch, 6.0);
    p.nudge(PitchParams::PITCH, -1);
    assert_eq!(p.pitch, 5.0);
    // The snap is the cell's step only: the ratio reads PITCH as it is.
    let mut q = PitchParams {
        pitch: 5.0,
        ..PitchParams::default()
    };
    let r = q.ratio();
    q.snap = Snap::Musical;
    assert_eq!(q.ratio(), r);
}

#[test]
fn jog_speed_from_detents() {
    let mut j = Jog::new();
    j.pass(0, 0.3);
    for t in 0..8u32 {
        j.turn(1, 1 + t * 15);
    }
    let mv = j.pass(125, 0.3);
    assert!((j.speed() - 0.5).abs() < 1e-6, "{}", j.speed());
    assert!((mv - 0.5 * 0.125).abs() < 1e-6);
    // Backward turns run it backward; turns older than 125 ms don't count.
    j.turn(-4, 300);
    j.pass(310, 0.3);
    assert!((j.speed() + 0.25).abs() < 1e-6, "{}", j.speed());
}

#[test]
fn jog_glides_to_stop() {
    for drag in [0.0, 0.3, 1.0] {
        let mut j = Jog::new();
        j.pass(0, drag);
        for t in 0..8u32 {
            j.turn(1, 1 + t * 15);
        }
        let pass_ms = 10u32;
        let mut now = 125;
        j.pass(now, drag);
        let v0 = j.speed();
        let tau_ms = drag_s(drag) * 1_000.0;
        let mut reached = None;
        let mut moved = 0.0;
        while now < 125 + (10.0 * tau_ms) as u32 + 100 {
            now += pass_ms;
            let before = j.speed();
            let mv = j.pass(now, drag);
            // A pass moves no more than its speed allows.
            assert!(mv.abs() <= before * pass_ms as f32 * 1e-3 + 1e-7);
            moved += mv;
            if reached.is_none() && j.speed() <= v0 * (-1.0f32).exp() {
                reached = Some(now - 125);
            }
        }
        let r = reached.unwrap() as f32;
        assert!(
            (r - tau_ms).abs() <= pass_ms as f32 + 0.5,
            "DRAG {drag}: {r} ms against {tau_ms}"
        );
        assert!(j.speed() < 1e-3 * v0);
        // A tape stopping covers its speed × τ.
        assert!((moved - v0 * tau_ms * 1e-3).abs() <= 0.1 * v0 * tau_ms * 1e-3);
    }
}

/// The head reaching END on a block's last sample ends the note there
/// (it once played on past END for ever).
#[test]
fn oneshot_ends_on_a_blocks_last_sample() {
    let held = ramp_src(6_400);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        model: TapeModel::Play,
        mode: PlayMode::One,
        drag: FADE_HOLD,
        ..P::default()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (out, _) = play(&mut e, &p, &v, 100);
    assert!(!e.is_active(), "100 blocks: 6 400 frames at 1 a sample");
    assert!(out[6_399].abs() <= 1.0 / END_RAMP as f32, "the ramp lands");
    let (more, _) = play(&mut e, &p, &v, 10);
    assert!(more.iter().all(|&y| y == 0.0));
}
