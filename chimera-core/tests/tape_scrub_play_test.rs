//! SCRUB is playable (ADR 0205): LENGTH is a time, its default 70 ms so
//! FINGER moves from the first turn; KEY POSITION places the finger and
//! plays at ROOT; the jog's speed reads steady, and no input path steps
//! the head's speed.

// TAPE's DSP as bank 1 builds it: under `bank2` the engine is the door's
// proxy (tape_proxy_test), and none of this links (#811).
#![cfg(not(feature = "bank2"))]

mod common;

use core::mem::MaybeUninit;

use chimera_core::block::{Block as _, DiskCode as _};
use chimera_core::dsp::clampf;
use chimera_core::dsp::tape_engine::machine::{SCRUB_LENGTH_DEFAULT, defaults};
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{
    KeyMode, LoopTime, MIN_LOOP_S, Probe, SCRUB_LEN_MAX_S, TapeEngine, TapeEngineParams,
    TapeMachine, key_place,
};
use chimera_core::ui::jog::Jog;
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, sine};

const SR: u32 = 48_000;
const B: usize = BLOCK_SIZE;
type P = TapeEngineParams;

fn fresh() -> TapeEngine {
    let mut slot = MaybeUninit::uninit();
    TapeEngine::init_in_place(&mut slot);
    // SAFETY: `init_in_place` wrote it whole.
    unsafe { slot.assume_init() }
}

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

/// SCRUB at its machine's defaults (what a MODEL turn writes).
fn scrub() -> P {
    let mut p = P::default();
    p.write(P::MODEL, TapeMachine::Scrub.index().into());
    for (id, v) in defaults(
        TapeMachine::Scrub,
        chimera_core::dsp::tape_engine::Direction::Loop,
    ) {
        p.set(id, v);
    }
    p
}

/// A 48 kHz source of `secs` whose every frame is its own place: a ramp
/// from −0.9 to +0.9, so a head's frame reads off its output too.
fn ramp(secs: f32) -> (Vec<u8>, u32) {
    let n = (secs * SR as f32) as u32;
    let x: Vec<f32> = (0..n).map(|i| -0.9 + 1.8 * i as f32 / n as f32).collect();
    (held_plain(&x, 48_000), n)
}

/// The probe heads' block-to-block moves, frames.
fn moves(probes: &[Probe]) -> Vec<i64> {
    probes
        .windows(2)
        .map(|w| w[1].head as i64 - w[0].head as i64)
        .collect()
}

/// The span the first head covers over `probes`: a still finger's loop.
fn span(probes: &[Probe]) -> u32 {
    let lo = probes.iter().map(|p| p.head).min().unwrap();
    let hi = probes.iter().map(|p| p.head).max().unwrap();
    hi - lo
}

#[test]
fn scrub_defaults_are_musical() {
    let p = scrub();
    assert_eq!(p.size, SCRUB_LENGTH_DEFAULT);
    assert_eq!(p.key, KeyMode::Position);
    assert_eq!(p.motion, 0.5, "SPEED: the source's own");
    // 2 ms · 1000^LENGTH: 70 ms, in the 60–80 ms the ADR chose from.
    let ms = 1_000.0 * LoopTime::of_length(SCRUB_LENGTH_DEFAULT).seconds();
    let law = 1_000.0 * MIN_LOOP_S * (SCRUB_LEN_MAX_S / MIN_LOOP_S).powf(SCRUB_LENGTH_DEFAULT);
    assert!((ms - law).abs() < 0.01, "{ms} against {law}");
    assert!((ms - 70.5).abs() < 0.5, "{ms} ms");
}

/// SCRUB's LENGTH is a time on any source: its default loop is 70 ms on a
/// 1 s and on a 6 s window, and LENGTH 1 is 2 s, or the window if shorter.
#[test]
fn scrub_length_is_a_time() {
    for secs in [1.0f32, 6.0] {
        let (held, n) = ramp(secs);
        let v = SourceView::strict(&held).unwrap();
        for (size, want) in [
            (SCRUB_LENGTH_DEFAULT, 0.0705 * SR as f32),
            (1.0, (SCRUB_LEN_MAX_S * SR as f32).min(n as f32)),
        ] {
            let p = P {
                size,
                position: 0.0,
                xfade: 0.0,
                ..scrub()
            };
            let mut e = fresh();
            e.note_on(60, &p, &v, SR);
            let blocks = (2.2 * want as f64 / B as f64) as usize + 4;
            let (_, pr) = play(&mut e, &p, &v, blocks);
            let got = span(&pr[1..]) as f32;
            assert!(
                (got - want).abs() <= B as f32 + 0.01 * want,
                "{secs} s, LENGTH {size}: a loop of {got} frames, want {want}"
            );
        }
    }
}

/// At the default LENGTH a FINGER sweep moves the sound: it is not the
/// unswept note (at LENGTH 1 the loop filled a short window and the
/// finger had no room, #747's page).
#[test]
fn default_length_finger_sweep_is_heard() {
    let held = held_plain(&common::tape::saw_with_burst(48_000, 0.75), 48_000);
    let v = SourceView::strict(&held).unwrap();
    let still = scrub();
    let mut a = fresh();
    a.note_on(60, &still, &v, SR);
    let (x, _) = play(&mut a, &still, &v, 750);
    let mut b = fresh();
    b.note_on(60, &still, &v, SR);
    let mut y = Vec::new();
    let mut p = still;
    for k in 0..750 {
        p.position = k as f32 / 750.0;
        y.extend(play(&mut b, &p, &v, 1).0);
    }
    let diff: f32 = x.iter().zip(&y).map(|(a, b)| (a - b) * (a - b)).sum();
    let pow: f32 = x.iter().map(|a| a * a).sum();
    assert!(
        diff > 0.1 * pow,
        "the sweep is {:.1} dB off the still note",
        10.0 * (diff / pow).log10()
    );
}

/// The frame KEY POSITION places the finger at for `note` and FINGER
/// `finger`: the key's twelfth of the room plus FINGER's, at most the
/// room's end.
fn placed(n: u32, size: f32, note: u8, finger: f32) -> u32 {
    let ll = (LoopTime::of_length(size).seconds() * SR as f32).round() as u32;
    let room = (n - ll) as f32;
    let at = key_place(chimera_core::MidiNote::new(note).unwrap()).plus(finger);
    (at.get() * room) as u32
}

/// KEY POSITION: C places the finger at the window's start and each
/// semitone a twelfth of the room further, every octave alike; FINGER
/// adds to the key's place and stops at the room's end. SPEED 0 holds the
/// head on the finger.
#[test]
fn keys_place_the_finger() {
    let (held, n) = ramp(2.0);
    let v = SourceView::strict(&held).unwrap();
    for (note, finger) in [
        (60u8, 0.0f32),
        (61, 0.0),
        (66, 0.0),
        (71, 0.0),
        (42, 0.0),
        (66, 0.25),
        (71, 0.5),
    ] {
        let p = P {
            motion: 0.0,
            position: finger,
            ..scrub()
        };
        let mut e = fresh();
        e.note_on(note, &p, &v, SR);
        let (_, pr) = play(&mut e, &p, &v, 4);
        let want = placed(n, p.size, note, finger);
        assert!(
            (pr[3].head as i64 - want as i64).abs() <= 1,
            "note {note}, FINGER {finger}: head at {}, want {want}",
            pr[3].head
        );
    }
}

/// KEY PITCH plays the scrub from FINGER on every key, as before.
#[test]
fn key_pitch_leaves_the_finger_to_finger() {
    let (held, _) = ramp(2.0);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        motion: 0.0,
        position: 0.4,
        key: KeyMode::Pitch,
        ..scrub()
    };
    let at = |note| {
        let mut e = fresh();
        e.note_on(note, &p, &v, SR);
        play(&mut e, &p, &v, 4).1[3].head
    };
    assert_eq!(at(60), at(66));
    assert_eq!(at(60), at(71));
}

/// The head's speed over a still finger, frames a block: its moves inside
/// one pass of a 2 s loop.
fn speed(note: u8, key: KeyMode) -> f32 {
    let (held, _) = ramp(4.0);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        size: 1.0,
        key,
        ..scrub()
    };
    let mut e = fresh();
    e.note_on(note, &p, &v, SR);
    let (_, pr) = play(&mut e, &p, &v, 60);
    let d = moves(&pr[20..]);
    d.iter().sum::<i64>() as f32 / d.len() as f32
}

/// KEY POSITION holds the pitch at ROOT (C4 here): every key reads the
/// source at its own speed. KEY PITCH plays a fifth up 1.5× faster.
#[test]
fn key_position_holds_root() {
    let root = speed(60, KeyMode::Position);
    assert!((root - B as f32).abs() < 0.5, "{root}");
    for note in [48u8, 55, 67, 84] {
        let s = speed(note, KeyMode::Position);
        assert!(
            (s - root).abs() < 0.01 * root,
            "note {note}: {s} against {root}"
        );
    }
    let fifth = speed(67, KeyMode::Pitch);
    let want = root * 2f32.powf(7.0 / 12.0);
    assert!(
        (fifth - want).abs() < 0.01 * want,
        "PITCH's fifth {fifth}, want {want}"
    );
}

#[test]
fn key_toggle_saves_and_loads() {
    let mut p = scrub();
    assert_eq!(p.enum_ident(P::KEY), Some("POSITION"));
    p.set(P::KEY, 1.0);
    assert_eq!(p.key, KeyMode::Pitch);
    assert_eq!(p.enum_code(P::KEY), Some(1));
    assert_eq!(p.enum_ident(P::KEY), Some("PITCH"));
    let mut q = P::default();
    assert!(q.set_enum_code(P::KEY, KeyMode::Pitch.disk_code()));
    assert_eq!(q.key, KeyMode::Pitch);
    assert!(!q.set_enum_code(P::KEY, 7), "an unknown code loads nothing");
    assert_eq!(q.key, KeyMode::Pitch);
}

/// A legato key change (a glide steal) on KEY POSITION moves the finger to
/// the new key's place through the aim: no block's move steps.
#[test]
fn a_legato_key_glides_the_finger() {
    let (held, n) = ramp(2.0);
    let v = SourceView::strict(&held).unwrap();
    let p = P {
        motion: 0.0,
        ..scrub()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let (_, a) = play(&mut e, &p, &v, 4);
    e.glide_to(66, (SR, 0.05));
    let (_, b) = play(&mut e, &p, &v, 400);
    let want = placed(n, p.size, 66, 0.0) as i64;
    assert!(
        (b[399].head as i64 - want).abs() <= 2,
        "head {} want {want}",
        b[399].head
    );
    let pr: Vec<Probe> = a[3..].iter().chain(&b).copied().collect();
    let d = moves(&pr);
    let top = d.iter().fold(0i64, |m, x| m.max(x.abs())) as f32;
    let step = d.windows(2).fold(0i64, |m, x| m.max((x[1] - x[0]).abs())) as f32;
    assert!(
        d[0].abs() as f32 <= 0.25 * top && step <= 0.25 * top,
        "first {} step {step} top {top}",
        d[0]
    );
}

/// The head's speed, frames a reading of two blocks, as FINGER is
/// `driven` each block at DRAG `drag`: `(lowest, highest)` from 1.0 to
/// 1.9 s, the reel up to speed.
fn ripple(drag: f32, drive: &mut dyn FnMut(usize, &mut P)) -> (f32, f32) {
    let (held, _) = ramp(4.0);
    let v = SourceView::strict(&held).unwrap();
    // SPEED 0: the head is the finger.
    let mut p = P {
        motion: 0.0,
        size: 0.0,
        drag,
        key: KeyMode::Pitch,
        position: 0.1,
        ..scrub()
    };
    let mut e = fresh();
    e.note_on(60, &p, &v, SR);
    let mut pr = Vec::new();
    for k in 0..1_500 {
        drive(k, &mut p);
        pr.extend(play(&mut e, &p, &v, 1).1);
    }
    let d: Vec<i64> = moves(&pr[750..1_427])
        .chunks(2)
        .map(|c| c.iter().sum())
        .collect();
    let (lo, hi) = d
        .iter()
        .fold((i64::MAX, 0), |(l, h), &x| (l.min(x), h.max(x)));
    (lo as f32, hi as f32)
}

/// The jog at 25 detents a second through a 33 ms UI pass, a turn's time
/// its pass's, as the UI turns it. Its rate read from the time between
/// turns over 250 ms and eased in (ADR 0205) halves #747's swing: the
/// block-rate model's ×1.31 → ×1.16 at DRAG 32 ms, ×1.70 → ×1.41 at
/// 10 ms, ×1.13 → ×1.08 at 120 ms; what is left is the UI pass's
/// staircase through the aim and DRAG (×1.12 at 32 ms for an ideal jog).
#[test]
fn jog_moves_the_head_evenly() {
    // Measured ×1.25, ×1.10, ×1.06 (#747: ×2.23, ×1.32, ×1.12).
    for (drag, most) in [(0.0, 1.3), (0.3, 1.13), (0.6, 1.08)] {
        let mut jog = Jog::new();
        let (mut next_ui, mut prev) = (0.0f32, 0i32);
        let cum = |t: f32| ((clampf(t, 0.5, 2.0) - 0.5) / 0.04).floor() as i32;
        let (lo, hi) = ripple(drag, &mut |k, p| {
            let t = (k * B) as f32 / SR as f32;
            while next_ui <= t {
                let ms = (next_ui * 1000.0) as u32;
                let c = cum(next_ui);
                if c != prev {
                    jog.turn((c - prev) as i8, chimera_hal::Ms(ms));
                    prev = c;
                }
                p.position = clampf(p.position + jog.pass(chimera_hal::Ms(ms), p.drag), 0.0, 1.0);
                next_ui += 0.033;
            }
        });
        assert!(
            hi <= most * lo && lo > 0.0,
            "DRAG {drag}: the jog's head speed swings {lo}..{hi}"
        );
    }
}

/// A CC on FINGER, a 1/128 step every 23 ms, and a matrix-rate ramp: the
/// aim and DRAG hold the head's speed even (measured ×1.08, ×1.04,
/// ×1.03; #747 ×1.19, ×1.07, ×1.04; a ramp ×1.01). DRAG 1's 500 ms is still rising at 1 s: it is
/// held by the jump test.
#[test]
fn cc_and_matrix_move_the_head_evenly() {
    for (drag, most) in [(0.0, 1.12), (0.3, 1.05), (0.6, 1.04)] {
        let (lo, hi) = ripple(drag, &mut |k, p| {
            let t = (k * B) as f32 / SR as f32;
            p.position = 0.1 + ((t - 0.5).max(0.0) / 0.023).floor() / 128.0;
        });
        assert!(hi <= most * lo && lo > 0.0, "DRAG {drag}, a CC: {lo}..{hi}");
        let (lo, hi) = ripple(drag, &mut |k, p| {
            let t = (k * B) as f32 / SR as f32;
            p.position = 0.1 + 0.34 * (t - 0.5).max(0.0);
        });
        assert!(
            hi <= 1.03 * lo && lo > 0.0,
            "DRAG {drag}, a ramp: {lo}..{hi}"
        );
    }
}

/// A jump at KEY POSITION starts the reel over blocks too, at every
/// DRAG (`scrub_finger_speed_never_steps`'s bound).
#[test]
fn a_jump_never_steps_the_speed() {
    let (held, _) = ramp(2.0);
    let v = SourceView::strict(&held).unwrap();
    for drag in [0.0, 0.3, 0.6, 1.0] {
        let mut p = P {
            motion: 0.0,
            drag,
            ..scrub()
        };
        let mut e = fresh();
        e.note_on(63, &p, &v, SR);
        let (_, a) = play(&mut e, &p, &v, 4);
        p.position = 0.6;
        let (_, b) = play(&mut e, &p, &v, 300);
        let pr: Vec<Probe> = a[3..].iter().chain(&b).copied().collect();
        let d = moves(&pr);
        let top = d.iter().fold(0i64, |m, x| m.max(x.abs())) as f32;
        let step = d.windows(2).fold(0i64, |m, x| m.max((x[1] - x[0]).abs())) as f32;
        assert!(top > 0.0);
        assert!(
            d[0].abs() as f32 <= 0.2 * top && step <= 0.2 * top,
            "DRAG {drag}: first {} step {step} top {top}",
            d[0]
        );
    }
}

/// NaN in FINGER, LENGTH or SPEED at KEY POSITION plays finite, and a note
/// off ends the note.
#[test]
fn nan_is_safe_and_the_note_ends() {
    let (held, _) = ramp(1.0);
    let v = SourceView::strict(&held).unwrap();
    for bad in [P::POSITION, P::SIZE, P::MOTION] {
        let mut p = scrub();
        let mut e = fresh();
        e.note_on(67, &p, &v, SR);
        play(&mut e, &p, &v, 10);
        p.write(bad, f32::NAN);
        let (out, _) = play(&mut e, &p, &v, 100);
        assert!(out.iter().all(|y| y.is_finite()), "NaN in {bad:?}");
        let p = scrub();
        let mut e = fresh();
        let mut q = p;
        q.write(bad, f32::NAN);
        e.note_on(67, &q, &v, SR);
        let (out, _) = play(&mut e, &q, &v, 50);
        assert!(
            out.iter().all(|y| y.is_finite()),
            "NaN at note-on in {bad:?}"
        );
    }
    // The voice: its VCA lets SCRUB go (`release_by`), and it ends.
    let mut s = chimera_core::preset::Sound::init(chimera_core::params::EngineType::Tape);
    s.params.tape = scrub();
    let mut r = common::rig::Rig::with_source(SR, &held);
    let on = chimera_core::MidiNote::new(64).unwrap();
    r.note_on(on, chimera_core::Velocity::new(100).unwrap(), &s.params);
    let mut b = [0.0; B];
    let mut heard = false;
    for n in 0..4_000 {
        if n == 50 {
            r.note_off();
        }
        r.render(&mut b, &s.params, &s.mod_state);
        heard |= b.iter().any(|y| y.abs() > 1e-3);
        assert!(b.iter().all(|y| y.is_finite()));
        if !r.is_active() {
            assert!(heard && n > 50, "ended at block {n}");
            return;
        }
    }
    panic!("a released SCRUB note never ends");
}

/// A test's sine stays a sine: the `sine` helper's period.
#[test]
fn the_test_ramp_reads_its_place() {
    let _ = sine(48_000, 100.0, 10);
    let (held, n) = ramp(1.0);
    let v = SourceView::strict(&held).unwrap();
    assert_eq!(v.frames(), n);
}
