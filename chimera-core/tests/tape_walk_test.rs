//! WALK (ADR 0203): one machine for what ONE, LOOP, PING-PONG and TRON
//! were. Each DIRECTION, FADE without clicks, POSITION and DRIFT (its
//! bounds, wrap, both signs and spread), TRON's run-out and pad, the old
//! machines' codes, NaN safety and where a voice ends.

mod common;

use core::mem::MaybeUninit;

use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::tape_engine::drift::{self, DriftRng, Place};
use chimera_core::dsp::tape_engine::heads::loop_len;
use chimera_core::dsp::tape_engine::machine::{defaults, direction_defaults, env_defaults};
use chimera_core::dsp::tape_engine::params::{Drift, RetiredMode};
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::tron::TRON_LEN_MAX_S;
use chimera_core::dsp::tape_engine::{
    Direction, FADE_HOLD, RetiredModel, TapeEngine, TapeEngineParams, TapeMachine, TapeModel, Tron,
    Way, fade_default, needs_ram,
};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{MIGRATIONS, TRANSLATIONS, decode_block};
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;
type P = TapeEngineParams;

fn fresh(seed: u32) -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    let mut e = unsafe { slot.assume_init() };
    e.seed(seed);
    e
}

/// A rising ramp, −0.9 to 0.9 over 1 s at 48 kHz: the output's slope is
/// the way it plays, its value where.
fn ramp() -> Vec<u8> {
    let n = 48_000;
    let x: Vec<f32> = (0..n).map(|i| -0.9 + 1.8 * i as f32 / n as f32).collect();
    held_plain(&x, 48_000)
}

fn sine_src() -> Vec<u8> {
    held_plain(&sine(48_000, 100.7, 48_000), 48_000)
}

/// WALK in `d` at ROOT C4, LENGTH 1, `d`'s FADE.
fn play(d: Direction) -> P {
    P {
        machine: TapeMachine::Walk,
        direction: d,
        size: 1.0,
        drag: fade_default(d),
        root: 6_000.0,
        ..P::default()
    }
}

/// `blocks` of `e` on `p` from a note-on of `note`, the key let go at
/// block `off` (if any).
fn run(
    e: &mut TapeEngine,
    p: &P,
    v: &SourceView<'_>,
    note: u8,
    blocks: usize,
    off: Option<usize>,
) -> Vec<f32> {
    e.hold();
    e.note_on(note, p, v, SR);
    let mut out = Vec::new();
    let mut b = [0.0; B];
    for k in 0..blocks {
        if Some(k) == off {
            e.note_off();
        }
        e.render(&mut b, p, v, SR);
        out.extend_from_slice(&b);
    }
    out
}

fn blocks(secs: f32) -> usize {
    (secs * SR as f32) as usize / B
}

fn sounding(out: &[f32]) -> usize {
    out.iter().rposition(|&y| y != 0.0).map_or(0, |i| i + 1)
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

fn at(x: &[f32], s: f32) -> f32 {
    x[(s * SR as f32) as usize]
}

// ── DIRECTION ────────────────────────────────────────────────────────

#[test]
fn forward_plays_the_window_once_and_ends() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let mut e = fresh(0);
    let out = run(&mut e, &play(Direction::Forward), &v, 60, blocks(1.5), None);
    assert!(
        at(&out, 0.1) < at(&out, 0.5) && at(&out, 0.5) < at(&out, 0.9),
        "rising"
    );
    assert!(
        (at(&out, 0.1) + 0.72).abs() < 0.02,
        "from START: {}",
        at(&out, 0.1)
    );
    let end = sounding(&out) as f32 / SR as f32;
    assert!((0.99..1.01).contains(&end), "ends at END: {end}");
    assert!(!e.is_active());
}

#[test]
fn reverse_plays_it_backward_and_ends() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let mut e = fresh(0);
    let out = run(&mut e, &play(Direction::Reverse), &v, 60, blocks(1.5), None);
    assert!(
        at(&out, 0.1) > at(&out, 0.5) && at(&out, 0.5) > at(&out, 0.9),
        "falling"
    );
    assert!(
        (at(&out, 0.1) - 0.72).abs() < 0.02,
        "from END: {}",
        at(&out, 0.1)
    );
    let end = sounding(&out) as f32 / SR as f32;
    assert!((0.99..1.01).contains(&end), "ends at START: {end}");
    assert!(!e.is_active());
}

/// REVERSE is FORWARD with END before START, bit for bit: one path.
#[test]
fn reverse_is_end_before_start() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let rev = run(
        &mut fresh(0),
        &play(Direction::Reverse),
        &v,
        60,
        blocks(1.2),
        None,
    );
    let swapped = P {
        start: 1.0,
        end: 0.0,
        ..play(Direction::Forward)
    };
    let fwd = run(&mut fresh(0), &swapped, &v, 60, blocks(1.2), None);
    assert_eq!(rev, fwd);
    // And REVERSE on END before START plays forward again.
    let back = P {
        start: 1.0,
        end: 0.0,
        ..play(Direction::Reverse)
    };
    let again = run(&mut fresh(0), &back, &v, 60, blocks(1.2), None);
    assert!(at(&again, 0.1) < at(&again, 0.5));
}

#[test]
fn loop_keeps_going_past_the_window() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let mut e = fresh(0);
    let out = run(&mut e, &play(Direction::Loop), &v, 60, blocks(3.0), None);
    assert!(e.is_active());
    assert!(
        out[(2.5 * SR as f32) as usize..]
            .iter()
            .any(|y| y.abs() > 0.1),
        "still sounding"
    );
}

#[test]
fn ping_pong_turns_at_the_edges() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let mut e = fresh(0);
    let out = run(&mut e, &play(Direction::Ping), &v, 60, blocks(2.5), None);
    assert!(e.is_active());
    // Up to the window's end, then back down: no jump at the turn.
    assert!(at(&out, 0.5) < at(&out, 0.95));
    assert!(at(&out, 1.5) < at(&out, 1.05));
    assert!(max_step(&out[B..]) < 0.01, "{}", max_step(&out[B..]));
}

// ── FADE ─────────────────────────────────────────────────────────────

/// A loop's seam, a key's release, TRON's pad and its run-out: no sample
/// steps further than the source's own (a 100.7-frame sine: 0.062).
#[test]
fn fade_never_clicks() {
    let h = sine_src();
    let v = SourceView::new(&h).unwrap();
    let own = 0.0625 * 1.05;
    let lp = P {
        size: 0.6,
        position: 0.3,
        xfade: 0.2,
        ..play(Direction::Loop)
    };
    let seam = run(&mut fresh(0), &lp, &v, 60, blocks(3.0), Some(blocks(2.0)));
    let fwd = P {
        drag: 0.3,
        ..play(Direction::Forward)
    };
    let release = run(&mut fresh(0), &fwd, &v, 60, blocks(1.0), Some(blocks(0.4)));
    let tron = P {
        tron: Tron::On,
        drag: 0.0,
        size: 0.0,
        ..play(Direction::Forward)
    };
    let pad = run(&mut fresh(0), &tron, &v, 60, blocks(1.0), Some(blocks(0.1)));
    let run_out = run(&mut fresh(0), &tron, &v, 60, blocks(1.0), None);
    for (name, x) in [
        ("seam", &seam),
        ("release", &release),
        ("pad", &pad),
        ("run-out", &run_out),
    ] {
        assert!(max_step(x) <= own, "{name}: {}", max_step(x));
    }
}

// ── POSITION and LENGTH ──────────────────────────────────────────────

#[test]
fn position_starts_there_and_length_takes_the_rest() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let p = P {
        position: 0.5,
        ..play(Direction::Forward)
    };
    let out = run(&mut fresh(0), &p, &v, 60, blocks(1.5), None);
    assert!(
        at(&out, 0.01).abs() < 0.03,
        "from the middle: {}",
        at(&out, 0.01)
    );
    let end = sounding(&out) as f32 / SR as f32;
    assert!(
        (0.49..0.51).contains(&end),
        "LENGTH 1 plays the rest: {end}"
    );
    // REVERSE counts POSITION from END.
    let r = P {
        position: 0.25,
        ..play(Direction::Reverse)
    };
    let out = run(&mut fresh(0), &r, &v, 60, blocks(1.5), None);
    assert!((at(&out, 0.01) - 0.45).abs() < 0.03, "{}", at(&out, 0.01));
    // LENGTH is LOOP's law over what is left.
    let short = P {
        size: 0.5,
        ..play(Direction::Forward)
    };
    let out = run(&mut fresh(0), &short, &v, 60, blocks(1.5), None);
    let want = loop_len(
        0.5,
        48_000,
        chimera_core::stream::units::SourceRate::new(48_000).unwrap(),
    );
    let got = sounding(&out) as u32;
    assert!(got.abs_diff(want) <= 2 * B as u32, "{got} vs {want}");
}

// ── DRIFT ─────────────────────────────────────────────────────────────

/// `n` steps from 0 at DRIFT `w`: each step's signed move (wrapped to
/// ±½) and each place.
fn walks(w: Drift, n: usize, seed: u32) -> (Vec<f32>, Vec<f32>) {
    let mut rng = DriftRng::seed(seed);
    let (mut at, mut d, mut places) = (Place::ZERO, Vec::new(), Vec::new());
    for _ in 0..n {
        let next = drift::step(at, w, &mut rng);
        let (next, at_f) = (next, at.get());
        let mut m = next.get() - at_f;
        m -= m.round();
        d.push(m);
        places.push(next.get());
        at = next;
    }
    (d, places)
}

#[test]
fn drift_bounds_hold_and_it_wraps_both_ways() {
    for w in [0.01, 0.1, 0.3, 0.5, 0.75, 1.0] {
        let (d, places) = walks(Drift::of(w), 20_000, 3);
        let w = Drift::of(w).get();
        assert!(
            places.iter().all(|&x| (0.0..1.0).contains(&x)),
            "{w}: in the window"
        );
        if w <= 0.5 {
            assert!(
                d.iter().all(|m| m.abs() <= w + 1e-6),
                "{w}: steps at most WALK"
            );
        }
        assert!(
            d.iter().any(|&m| m < 0.0) && d.iter().any(|&m| m > 0.0),
            "{w}: both signs"
        );
    }
    // Over the ends: a step back from 0.05 by more than 0.05 lands near 1,
    // one forward from 0.95 near 0.
    let (_, places) = walks(Drift::of(0.3), 2_000, 5);
    let pairs = places.windows(2);
    let mut down = false;
    let mut up = false;
    for p in pairs {
        down |= p[0] < 0.1 && p[1] > 0.8;
        up |= p[0] > 0.9 && p[1] < 0.2;
    }
    assert!(down && up, "wraps at both ends");
}

/// The steps are uniform in ±DRIFT: mean 0, |step| DRIFT/2 on average,
/// RMS DRIFT/√3; at full DRIFT the places cover the window evenly.
#[test]
fn drift_spreads_as_it_should() {
    for w in [0.1, 0.25, 0.5] {
        let (d, _) = walks(Drift::of(w), 50_000, 11);
        let w = Drift::of(w).get();
        let n = d.len() as f32;
        let mean = d.iter().sum::<f32>() / n;
        let abs = d.iter().map(|m| m.abs()).sum::<f32>() / n;
        let rms = (d.iter().map(|m| m * m).sum::<f32>() / n).sqrt();
        assert!(mean.abs() < 0.01 * w, "{w}: mean {mean}");
        assert!(
            (abs / (w / 2.0) - 1.0).abs() < 0.02,
            "{w}: mean |step| {abs}"
        );
        assert!(
            (rms / (w / 3f32.sqrt()) - 1.0).abs() < 0.02,
            "{w}: RMS {rms}"
        );
    }
    let (_, places) = walks(Drift::of(1.0), 50_000, 13);
    let mut bins = [0u32; 10];
    for x in places {
        bins[(x * 10.0) as usize] += 1;
    }
    assert!(
        bins.iter().all(|&b| (4_500..5_500).contains(&b)),
        "{bins:?}"
    );
}

/// DRIFT 0 draws nothing and rests at POSITION; a NaN is DRIFT 0.
#[test]
fn drift_off_rests() {
    let mut rng = DriftRng::seed(1);
    let before = rng;
    let at = Place::wrapped(0.4);
    assert_eq!(drift::step(at, Drift::OFF, &mut rng), Place::ZERO);
    assert_eq!(drift::step(at, Drift::of(f32::NAN), &mut rng), Place::ZERO);
    assert_eq!(rng, before);
    assert_eq!(drift::place(0.7, Place::ZERO), 0.7);
    assert_eq!(drift::place(1.0, Place::ZERO), 1.0);
    assert_eq!(Place::wrapped(f32::NAN), Place::ZERO);
    assert_eq!(Place::wrapped(-0.25).get(), 0.75);
    assert_eq!(Drift::of(f32::NAN), Drift::OFF);
}

/// DRIFT 0 renders bit for bit as a Sound that never set it, note after
/// note, on every direction.
#[test]
fn drift_off_is_bit_identical_to_no_walk() {
    let h = sine_src();
    let v = SourceView::new(&h).unwrap();
    for d in Direction::ALL {
        let plain = P {
            size: 0.7,
            position: 0.3,
            ..play(d)
        };
        let mut set = plain;
        set.set(P::DRIFT, 0.0);
        let (mut a, mut b) = (fresh(4), fresh(4));
        for _ in 0..4 {
            assert_eq!(
                run(&mut a, &plain, &v, 60, blocks(0.3), Some(blocks(0.2))),
                run(&mut b, &set, &v, 60, blocks(0.3), Some(blocks(0.2))),
                "{d:?}"
            );
        }
    }
}

/// Each note starts where the walk has wandered, inside the window, and a
/// voice's seed repeats it.
#[test]
fn drift_moves_each_note_repeatably() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let p = P {
        drift: Drift::of(0.5),
        ..play(Direction::Forward)
    };
    let starts = |seed: u32| {
        let mut e = fresh(seed);
        (0..8)
            .map(|_| run(&mut e, &p, &v, 60, 4, None)[B])
            .collect::<Vec<f32>>()
    };
    let a = starts(2);
    assert_eq!(a, starts(2), "a seed repeats");
    assert_ne!(a, starts(3), "voices differ");
    assert!(a.iter().all(|&x| (-0.91..0.91).contains(&x)));
    assert!(a.windows(2).all(|w| w[0] != w[1]), "{a:?}");
    // LOOP's place walks too: two notes' first passes differ.
    let lp = P {
        drift: Drift::of(0.5),
        size: 0.3,
        ..play(Direction::Loop)
    };
    let mut e = fresh(2);
    let one = run(&mut e, &lp, &v, 60, blocks(0.5), None);
    let two = run(&mut e, &lp, &v, 60, blocks(0.5), None);
    assert_ne!(one, two);
}

// ── TRON ─────────────────────────────────────────────────────────────

/// ON: the tape runs out after LENGTH's seconds at every pitch, while the
/// key is held. OFF, held, it plays to END (slower at a low key).
#[test]
fn tron_runs_out_held() {
    let h = held_plain(&sine(48_000, 100.7, 96_000), 48_000);
    let v = SourceView::new(&h).unwrap();
    let on = P {
        tron: Tron::On,
        size: 0.0,
        drag: 0.3,
        ..play(Direction::Forward)
    };
    for note in [48, 60, 72] {
        let out = run(&mut fresh(0), &on, &v, note, blocks(1.5), None);
        let end = sounding(&out) as f32 / SR as f32;
        assert!(
            (0.24..0.27).contains(&end),
            "{note}: runs out at 0.25 s, {end}"
        );
    }
    let off = P {
        tron: Tron::Off,
        size: 1.0,
        ..on
    };
    let held = run(&mut fresh(0), &off, &v, 48, blocks(2.5), None);
    assert!(sounding(&held) as f32 / SR as f32 > 2.4, "OFF plays on");
}

/// ON: key up lifts the pad through FADE (32 ms, TRON's), and even at
/// FADE's top, 500 ms, where OFF reads HOLD; OFF at HOLD the key up
/// changes nothing.
#[test]
fn tron_pad_stops_at_key_up() {
    let h = held_plain(&sine(48_000, 100.7, 96_000), 48_000);
    let v = SourceView::new(&h).unwrap();
    let off = play(Direction::Forward);
    assert_eq!(off.drag, FADE_HOLD);
    let on = P {
        tron: Tron::On,
        drag: 0.3,
        ..off
    };
    let up = blocks(0.5);
    let tron = run(&mut fresh(0), &on, &v, 60, blocks(2.0), Some(up));
    let end = sounding(&tron) as f32 / SR as f32;
    assert!((0.5..0.85).contains(&end), "the pad lifts: {end}");
    let top = run(
        &mut fresh(0),
        &P {
            drag: FADE_HOLD,
            ..on
        },
        &v,
        60,
        blocks(2.0),
        Some(up),
    );
    let level = |x: &[f32], s: f32| {
        x[(s * SR as f32) as usize..][..4_800]
            .iter()
            .map(|y| y * y)
            .sum::<f32>()
    };
    assert!(
        level(&top, 1.5) < 0.05 * level(&top, 0.3),
        "at the top it still lifts"
    );
    let held = run(&mut fresh(0), &off, &v, 60, blocks(2.0), Some(up));
    assert!(
        sounding(&held) as f32 / SR as f32 > 1.95,
        "HOLD plays to END"
    );
    // Around a loop the tape runs out at 8 s.
    let lp = P {
        tron: Tron::On,
        ..play(Direction::Loop)
    };
    let mut e = fresh(0);
    let out = run(&mut e, &lp, &v, 60, blocks(8.5), None);
    let end = sounding(&out) as f32 / SR as f32;
    assert!(
        (TRON_LEN_MAX_S..TRON_LEN_MAX_S + 0.1).contains(&end),
        "{end}"
    );
    assert!(!e.is_active());
    let mut e = fresh(0);
    run(
        &mut e,
        &P {
            tron: Tron::Off,
            ..lp
        },
        &v,
        60,
        blocks(8.5),
        None,
    );
    assert!(e.is_active(), "OFF loops on");
}

// ── The old machines ─────────────────────────────────────────────────

fn decoded(entries: &[(u8, u32)]) -> P {
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape.position = 0.4;
    s.params.tape.size = 0.5;
    let mut payload = vec![34u8];
    for &(id, code) in entries {
        payload.push(id);
        payload.extend_from_slice(&code.to_le_bytes());
    }
    decode_block(&payload, MIGRATIONS, TRANSLATIONS, Some(&mut s.params)).unwrap();
    s.params.tape
}

#[test]
fn old_machines_load_as_play() {
    const MODEL: u8 = 0;
    const MODE: u8 = 15;
    // MODE ONE: FORWARD from the window's start, whole, as ONE read
    // neither POSITION nor LENGTH.
    let t = decoded(&[(MODEL, 0), (MODE, u32::from(RetiredMode::One.code()))]);
    assert_eq!(
        (t.way(), t.direction, t.position, t.size),
        (Way::One, Direction::Forward, 0.0, 1.0)
    );
    // MODE LOOP and PING keep their codes and their reads.
    let t = decoded(&[(MODEL, 0), (MODE, 1)]);
    assert_eq!(
        (t.direction, t.position, t.size),
        (Direction::Loop, 0.4, 0.5)
    );
    let t = decoded(&[(MODEL, 0), (MODE, 2)]);
    assert_eq!(t.direction, Direction::Ping);
    // TRON (6), whatever MODE it kept: FORWARD, TRON on, from START, its
    // LENGTH kept.
    for mode in [0, 1, 2] {
        let t = decoded(&[(MODEL, 6), (MODE, mode)]);
        assert_eq!(
            (t.machine, t.direction, t.tron, t.position, t.size),
            (TapeMachine::Walk, Direction::Forward, Tron::On, 0.0, 0.5),
            "MODE {mode}"
        );
    }
    // ONESHOT (7), PING-PONG (2) and REVERSE (1), retired before WALK.
    let t = decoded(&[(MODEL, 7)]);
    assert_eq!(
        (t.way(), t.drag, t.position, t.size),
        (Way::One, FADE_HOLD, 0.0, 1.0)
    );
    assert_eq!(decoded(&[(MODEL, 2)]).way(), Way::Ping);
    let t = decoded(&[(MODEL, 1)]);
    assert_eq!((t.way(), t.start, t.end), (Way::Loop, 1.0, 0.0));
}

/// Codes are append-only: no live code is a retired one, and WALK saves
/// DIRECTION's new codes.
#[test]
fn old_codes_are_never_reused() {
    for m in TapeModel::ALL {
        assert!(RetiredModel::from_code(m.disk_code()).is_none(), "{m:?}");
    }
    for d in Direction::ALL {
        assert_ne!(d.disk_code(), RetiredMode::One.code(), "{d:?}");
        assert_eq!(Direction::from_disk_code(d.disk_code()), Some(d));
    }
    assert_eq!(Direction::from_disk_code(RetiredMode::One.code()), None);
    let mut t = P::default();
    t.set_way(Way::One);
    assert_eq!(t.enum_code(P::MODE), Some(3));
    t.set_way(Way::Tron);
    assert_eq!(
        (t.enum_code(P::MODEL), t.enum_code(P::TRON)),
        (Some(0), Some(1))
    );
}

// ── The machine's cells ──────────────────────────────────────────────

/// A MODEL turn onto WALK writes its cells, DIRECTION kept and TRON off;
/// a DIRECTION turn across once and loop writes LENGTH and FADE.
#[test]
fn play_defaults_follow_direction() {
    let d: Vec<_> = defaults(TapeMachine::Walk, Direction::Forward).collect();
    assert!(d.contains(&(P::SIZE, 1.0)) && d.contains(&(P::DRAG, FADE_HOLD)));
    assert!(d.contains(&(P::TRON, 0.0)) && d.contains(&(P::DRIFT, 0.0)));
    assert!(!d.iter().any(|&(id, _)| id == P::MODE));
    let d: Vec<_> = defaults(TapeMachine::Walk, Direction::Ping).collect();
    assert!(d.contains(&(P::DRAG, 0.3)));
    assert!(direction_defaults(Direction::Forward, Direction::Reverse).is_empty());
    assert!(direction_defaults(Direction::Loop, Direction::Ping).is_empty());
    assert_eq!(
        direction_defaults(Direction::Loop, Direction::Forward)[1],
        (P::DRAG, FADE_HOLD)
    );
    assert!(!env_defaults(TapeMachine::Walk, Direction::Forward).is_empty());
    assert!(env_defaults(TapeMachine::Walk, Direction::Loop).is_empty());
}

/// Streams read forward: REVERSE and PING-PONG need RAM, TRON doesn't.
#[test]
fn what_streams() {
    for (d, tron, ram) in [
        (Direction::Forward, Tron::Off, false),
        (Direction::Forward, Tron::On, false),
        (Direction::Loop, Tron::On, false),
        (Direction::Reverse, Tron::Off, true),
        (Direction::Ping, Tron::Off, true),
    ] {
        let p = P { tron, ..play(d) };
        assert_eq!(needs_ram(&p).is_some(), ram, "{d:?} {tron:?}");
    }
}

// ── NaN and the voice's end ──────────────────────────────────────────

/// NaN in any of WALK's reals never reaches the output or hangs a note.
#[test]
fn nan_is_safe() {
    let h = sine_src();
    let v = SourceView::new(&h).unwrap();
    for d in Direction::ALL {
        for tron in [Tron::Off, Tron::On] {
            for k in 0..6 {
                let mut p = P { tron, ..play(d) };
                match k {
                    0 => p.position = f32::NAN,
                    1 => p.size = f32::NAN,
                    2 => p.set(P::DRIFT, f32::NAN),
                    3 => p.drag = f32::NAN,
                    4 => (p.start, p.end) = (f32::NAN, f32::NAN),
                    _ => p.xfade = f32::NAN,
                }
                let mut e = fresh(1);
                for _ in 0..3 {
                    let out = run(&mut e, &p, &v, 60, blocks(0.3), Some(blocks(0.1)));
                    assert!(out.iter().all(|y| y.is_finite()), "{d:?} {tron:?} {k}");
                }
            }
        }
    }
}

/// FORWARD and REVERSE end at the window's end, TRON at its run-out or
/// after its pad; LOOP and PING-PONG end only through FADE after key up.
#[test]
fn voices_end() {
    let h = sine_src();
    let v = SourceView::new(&h).unwrap();
    for (d, tron) in [
        (Direction::Forward, Tron::Off),
        (Direction::Reverse, Tron::Off),
        (Direction::Forward, Tron::On),
        (Direction::Reverse, Tron::On),
    ] {
        // TRON's tape at LENGTH 0: 0.25 s.
        let size = if tron == Tron::On { 0.0 } else { 1.0 };
        let mut e = fresh(0);
        run(
            &mut e,
            &P {
                tron,
                size,
                ..play(d)
            },
            &v,
            60,
            blocks(1.2),
            None,
        );
        assert!(!e.is_active(), "{d:?} {tron:?}");
    }
    for d in [Direction::Loop, Direction::Ping] {
        let mut e = fresh(0);
        run(&mut e, &play(d), &v, 60, blocks(1.5), None);
        assert!(e.is_active(), "{d:?} held");
        let mut e = fresh(0);
        run(&mut e, &play(d), &v, 60, blocks(1.5), Some(blocks(0.5)));
        assert!(!e.is_active(), "{d:?} released");
    }
}

/// TRON REVERSE plays the whole window backward, as TRON FORWARD with END
/// before START does, bit for bit; and REVERSE on END before START plays
/// it forward.
#[test]
fn tron_reverse_is_end_before_start() {
    let h = ramp();
    let v = SourceView::new(&h).unwrap();
    let tron = |d: Direction, start: f32, end: f32| P {
        tron: Tron::On,
        drag: 0.3,
        start,
        end,
        ..play(d)
    };
    let rev = run(
        &mut fresh(0),
        &tron(Direction::Reverse, 0.0, 1.0),
        &v,
        60,
        blocks(1.2),
        None,
    );
    assert!(
        at(&rev, 0.1) > at(&rev, 0.5) && at(&rev, 0.5) > at(&rev, 0.9),
        "falling: {} {} {}",
        at(&rev, 0.1),
        at(&rev, 0.5),
        at(&rev, 0.9)
    );
    assert!(
        (at(&rev, 0.1) - 0.72).abs() < 0.02,
        "from END: {}",
        at(&rev, 0.1)
    );
    let fwd = run(
        &mut fresh(0),
        &tron(Direction::Forward, 1.0, 0.0),
        &v,
        60,
        blocks(1.2),
        None,
    );
    assert_eq!(rev, fwd);
    let back = run(
        &mut fresh(0),
        &tron(Direction::Reverse, 1.0, 0.0),
        &v,
        60,
        blocks(1.2),
        None,
    );
    assert!(at(&back, 0.1) < at(&back, 0.5) && at(&back, 0.5) < at(&back, 0.9));
}
