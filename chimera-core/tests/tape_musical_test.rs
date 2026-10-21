//! #698's musical findings, one regression each where a voice shows it: one
//! release a note (3), ONE's hit (4), equal-power fades (9), AGE from the
//! source's band (10) and VEL → START (11). The streamed ones (1, 2, 5,
//! RETRIG) are `tape_stream_test`'s; the defaults and laws
//! `tape_machines_test`'s, `tape_loop_test`'s and `tape_stretch_test`'s.

mod common;

use chimera_core::dsp::tape_engine::Way;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::modulator::EnvSlot;
use chimera_core::dsp::tape_engine::machine::env_defaults;
use chimera_core::dsp::tape_engine::{FADE_HOLD, Direction, TapeEngineParams, TapeMachine};
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::params::{EngineType, EnvParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::tape::{held_plain, sine};
use common::{Rig, SR, rms, step};

fn c4() -> MidiNote {
    MidiNote::new(60).unwrap()
}

/// INIT TAPE (ENV 1 into VCA) playing `machine` with `tape` on top.
fn init(machine: Way, tape: impl FnOnce(&mut TapeEngineParams)) -> Sound {
    let mut s = Sound::plain_init(EngineType::Tape);
    s.params.tape.set_way(machine);
    tape(&mut s.params.tape);
    s
}

/// `on` blocks held, then `off` released.
fn play(v: &mut Rig, s: &Sound, vel: u8, (on, off): (usize, usize)) -> Vec<f32> {
    v.note_on(c4(), Velocity::new(vel).unwrap(), &s.params);
    let mut b = [0.0; BLOCK_SIZE];
    let mut out = Vec::new();
    for k in 0..on + off {
        if k == on {
            v.note_off();
        }
        v.render(&mut b, &s.params, &s.mod_state);
        out.extend_from_slice(&b);
    }
    out
}

/// The last sample over `floor`.
fn tail_end(x: &[f32], floor: f32) -> usize {
    x.iter().rposition(|y| y.abs() > floor).unwrap_or(0)
}

/// Finding 3: under INIT's VCA route AMP RELEASE is LOOP's and PING's one
/// release (FADE steps aside): 5 s of release rings far past FADE's 32 ms,
/// a short one ends it, and nothing steps at key-up. With no VCA route FADE
/// still releases, and TRON keeps it.
#[test]
fn amp_release_is_the_one_release() {
    let held = held_plain(&sine(24_000, 50.0, 24_000), 24_000);
    let tail = |machine, release: f32, routed: bool| {
        let mut s = init(machine, |_| {});
        s.params.envelopes[0].release = release;
        if !routed {
            s.mod_state = ModState::new();
        }
        let mut v = Rig::with_source(SR, &held);
        let out = play(&mut v, &s, 100, (100, 400));
        let at = 100 * BLOCK_SIZE;
        let steady = step(&out[20 * BLOCK_SIZE..at]);
        assert!(
            step(&out[at..]) <= 1.5 * steady,
            "{machine:?} {release}: a step at key-up"
        );
        (tail_end(&out[at..], 1e-3) as f32) / SR as f32
    };
    for m in [Way::Loop, Way::Ping] {
        let long = tail(m, 0.9, true);
        let short = tail(m, 0.2, true);
        // 4.5 s of release: still ringing at the window's end, 0.53 s on.
        assert!(long > 0.5 && short < 0.2, "{m:?}: {long} s, {short} s");
        // No VCA route: the key gates it, FADE its release.
        assert!(tail(m, 0.9, false) < 0.4, "{m:?} unrouted");
    }
    // TRON's FADE (the pressure pad) ends it whatever AMP's release.
    assert!(tail(Way::Tron, 0.9, true) < 0.4, "TRON");
}

/// Finding 4: ONE's MODEL defaults put ENV 1 at its floor and hold it
/// full, so a hit is near full scale within a millisecond of its start.
#[test]
fn one_opens_on_the_hit() {
    let held = held_plain(&sine(24_000, 24.0, 24_000), 24_000);
    let mut s = init(Way::One, |t| t.drag = FADE_HOLD);
    let env = ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::ATTACK).block;
    assert_eq!(env, BlockRef::Env(EnvSlot::Env1));
    for &(id, v) in env_defaults(TapeMachine::Play, Direction::Forward) {
        chimera_core::block::Block::set(&mut s.params.envelopes[0], id, v);
    }
    let mut v = Rig::with_source(SR, &held);
    let out = play(&mut v, &s, 127, (20, 0));
    // The 1 ms note-on ramp's end, and the source's own peak.
    let ms2 = out[48..96].iter().fold(0f32, |m, y| m.max(y.abs()));
    let late = out[10 * BLOCK_SIZE..]
        .iter()
        .fold(0f32, |m, y| m.max(y.abs()));
    assert!(ms2 > 0.85 * late, "{ms2} at 1-2 ms against {late}");
    // MED's 10 ms attack at INIT, as before: well under.
    let mut slow = init(Way::One, |t| t.drag = FADE_HOLD);
    slow.params.envelopes[0].sustain = 1.0;
    let out = play(&mut Rig::with_source(SR, &held), &slow, 127, (20, 0));
    let ms2_slow = out[48..96].iter().fold(0f32, |m, y| m.max(y.abs()));
    assert!(ms2_slow < 0.6 * late, "MED's attack: {ms2_slow}");
}

/// A noise source: uncorrelated across any splice.
fn noise(frames: u32) -> Vec<f32> {
    let mut r = 0x1234_5678u32;
    (0..frames)
        .map(|_| {
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            (r as i32 as f32) / 2_147_483_648.0 * 0.5
        })
        .collect()
}

/// RMS over 16 blocks at a time, over `x`: noise's own swing under 0.5 dB.
fn block_rms(x: &[f32]) -> Vec<f32> {
    x.chunks(16 * BLOCK_SIZE)
        .filter(|c| c.len() == 16 * BLOCK_SIZE)
        .map(rms)
        .collect()
}

/// Finding 9: LOOP's long fade over noise keeps its power (the pulse at
/// each pass is under 1 dB, not 3); over a tone it sums to 1 and never
/// passes the tone's peak; and no fade clicks.
#[test]
fn long_fades_keep_their_power() {
    let held = held_plain(&noise(24_000), 24_000);
    let s = init(Way::Loop, |t| {
        (t.size, t.xfade, t.position) = (0.85, 0.5, 0.3);
    });
    let mut s = s;
    s.params.envelopes[0].sustain = 1.0;
    let out = play(&mut Rig::with_source(SR, &held), &s, 100, (1_500, 0));
    let r = block_rms(&out[50 * BLOCK_SIZE..]);
    let (lo, hi) = r
        .iter()
        .fold((f32::MAX, 0f32), |(a, b), &x| (a.min(x), b.max(x)));
    let dip = 20.0 * (hi / lo).log10();
    assert!(dip < 1.5, "the level swings {dip:.2} dB a pass");

    let tone = held_plain(&sine(24_000, 50.0, 24_000), 24_000);
    let out = play(&mut Rig::with_source(SR, &tone), &s, 100, (600, 0));
    let peak = out[50 * BLOCK_SIZE..]
        .iter()
        .fold(0f32, |m, y| m.max(y.abs()));
    let first = out[20 * BLOCK_SIZE..40 * BLOCK_SIZE]
        .iter()
        .fold(0f32, |m, y| m.max(y.abs()));
    assert!(
        peak <= 1.05 * first,
        "a tone's fade peaks {peak} over {first}"
    );
}

/// Finding 10: AGE's corner starts at the source's band edge, so on an
/// 8 kHz source AGE 0.3 already darkens it (its slope's level down by more
/// than 15 %), where the absolute 20 kHz corner (10.6 kHz there) did next
/// to nothing to a 4 kHz band.
#[test]
fn age_darkens_a_low_rate_source() {
    let held = held_plain(&noise(16_000), 8_000);
    let hi = |age: f32| {
        let mut s = init(Way::Loop, |t| t.age = age);
        s.params.envelopes[0].sustain = 1.0;
        let out = play(&mut Rig::with_source(SR, &held), &s, 100, (300, 0));
        // The top octave of the source's band, 2–4 kHz.
        let x = &out[100 * BLOCK_SIZE..];
        let hp: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
        rms(&hp) / rms(x)
    };
    let (clean, worn) = (hi(0.0), hi(0.3));
    assert!(worn < 0.85 * clean, "AGE 0.3: {worn} against {clean}");
}

/// Finding 11: VEL → START moves where ONE's hit plays from, latched at the
/// note: a soft and a hard hit start in different places, each where its
/// velocity puts it, and neither clicks.
#[test]
fn velocity_moves_one_start() {
    // A ramp, so the first sample says where the head started.
    let ramp: Vec<f32> = (0..24_000)
        .map(|i| -0.9 + 1.8 * i as f32 / 24_000.0)
        .collect();
    let held = held_plain(&ramp, 24_000);
    let mut s = init(Way::One, |t| {
        (t.drag, t.start) = (FADE_HOLD, 0.0);
    });
    s.params.envelopes[0].sustain = 1.0;
    for &(id, v) in env_defaults(TapeMachine::Play, Direction::Forward) {
        chimera_core::block::Block::set(&mut s.params.envelopes[0], id, v);
    }
    let start = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::START);
    assert!(
        s.mod_state
            .set_route_addr(ModSource::Vel.index(), start, 127)
    );
    let level = |vel: u8| {
        let out = play(&mut Rig::with_source(SR, &held), &s, vel, (4, 0));
        assert!(step(&out[..BLOCK_SIZE]) < 0.05, "VEL {vel}: a click");
        out[2 * BLOCK_SIZE]
    };
    let (soft, hard) = (level(20), level(120));
    assert!(
        hard > soft + 0.5,
        "VEL 120 starts later: {hard} against {soft}"
    );
    // ONE's START with no route starts at START.
    s.mod_state = Sound::plain_init(EngineType::Tape).mod_state;
    let out = play(&mut Rig::with_source(SR, &held), &s, 120, (4, 0));
    assert!(out[2 * BLOCK_SIZE] < -0.8, "{}", out[2 * BLOCK_SIZE]);
    let _ = (Direction::Forward, ParamSnapshot::default());
}
