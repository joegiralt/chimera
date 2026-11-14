//! DRUM slice 2 (ADR 0269): each pad's FILTER and AMP reach its hits,
//! latched at the hit. The pad's FILTER is laid over the voice's played
//! Sound; its AMP envelope ends the hit, ONE-SHOT past note-off, GATED
//! released by it.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

mod common;

use std::sync::{LazyLock, Mutex, MutexGuard};

use chimera_core::MidiChannel;
use chimera_core::dsp::Unit;
use chimera_core::dsp::drum::sound::{AmpMode, EnvDecay, EnvPos, PadEnv, PadFilter, PadSound};
use chimera_core::dsp::drum::types::{Hz, Kit, Knob, KnobValue, Machine, PadId};
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::modulator::EnvSpeed;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;
use chimera_midi::value::U7;
use common::{InstRig, SR};

static WRITER: LazyLock<Mutex<KitWriter>> =
    LazyLock::new(|| Mutex::new(KitWriter::take().expect("the kit writer, once")));

const P1: PartId = PartId::ALL[0];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).expect("pads 1–12")
}

fn u7(v: u8) -> U7 {
    U7::new(v).expect("0..=127")
}

/// Part 1's Sound: DRUM INIT, CLEAN at 1000 Hz.
fn sound() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Drum);
    assert_eq!(p.filter.kind(), FilterKind::Svf);
    p.filter.cutoff = 1000.0;
    p
}

/// `Kit::DEFAULT` with every pad on Part 1's Sound's FILTER and AMP.
fn kit() -> Kit {
    let mut k = Kit::DEFAULT;
    let s = PadSound::from_sound(&sound());
    for p in PadId::ALL {
        *k.pad_mut(p).sound_mut() = s;
    }
    k
}

/// The writer, every other Part on `Kit::DEFAULT`, Part 1 (channel 1,
/// `sound()`) on `kit`; the other Parts on channel 16.
fn rig(kit: &Kit) -> (MutexGuard<'static, KitWriter>, InstRig) {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    for p in PartId::ALL {
        w.publish(p, &Kit::DEFAULT);
        let _ = w.take_hit(p);
    }
    w.publish(P1, kit);
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(if i == 0 { 0 } else { 15 }).unwrap();
    }
    *r.shared.parts[0].params = sound();
    (w, r)
}

/// The active voice sounding `p`'s hit.
fn voice_of(r: &InstRig, p: PadId) -> usize {
    (0..MAX_VOICES)
        .find(|&v| {
            let voice = r.inst.voice(v);
            voice.is_active()
                && matches!(voice.slot(), EngineSlot::Drum(d) if d.hit().is_some_and(|h| h.pad() == p))
        })
        .expect("a voice sounds the pad")
}

fn sounding(r: &InstRig) -> bool {
    (0..MAX_VOICES).any(|v| r.inst.voice(v).is_active())
}

fn ladder_200(k: &mut Kit) {
    let f = k.pad_mut(pad(1)).sound_mut().filter_mut();
    f.set_kind(FilterKind::Ladder);
    f.set_cutoff(Hz::new(200.0));
}

#[test]
fn pad_filter_reaches_the_voice() {
    let mut k = kit();
    ladder_200(&mut k);
    let (_w, mut r) = rig(&k);
    r.note_on(0, 0);
    r.render(1);
    let f = r.inst.voice(voice_of(&r, pad(1))).played_filter();
    assert_eq!(f.kind(), FilterKind::Ladder);
    assert_eq!(f.cutoff, 200.0);
    r.note_on(0, 1);
    r.render(1);
    let f = r.inst.voice(voice_of(&r, pad(2))).played_filter();
    assert_eq!(PadFilter::of(&f), PadFilter::of(&sound().filter));
}

/// TYPE latches at the hit; CUTOFF follows, glided (drum_live_test).
#[test]
fn latched_at_the_hit() {
    let mut k = kit();
    ladder_200(&mut k);
    let (mut w, mut r) = rig(&k);
    r.note_on(0, 0);
    r.render(1);
    let v = voice_of(&r, pad(1));
    let f = k.pad_mut(pad(1)).sound_mut().filter_mut();
    f.set_kind(FilterKind::Sem);
    f.set_cutoff(Hz::new(5000.0));
    w.publish(P1, &k);
    r.render(4);
    let f = r.inst.voice(v).played_filter();
    assert_eq!(f.kind(), FilterKind::Ladder);
}

/// Pad 1 on PLAIN SINE, DECAY 127 (the kernel rings long), its AMP `env`
/// in `mode`.
fn sine_kit(env: PadEnv, mode: AmpMode) -> Kit {
    let mut k = kit();
    let p = k.pad_mut(pad(1));
    p.set_machine(Machine::PlainSine);
    p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
    let amp = p.sound_mut().amp_mut();
    *amp.env_mut() = env;
    amp.set_mode(mode);
    k
}

/// A 0, H 0, D 10 on FAST.
fn short() -> PadEnv {
    PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(u7(10))),
        EnvSpeed::Fast,
    )
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Part 1's bus over `ms` after a hit on pad 1, a note-off at `off` ms,
/// and whether a voice still sounds at the end.
fn play(k: &Kit, ms: usize, off: Option<usize>) -> (Vec<f32>, bool) {
    let (_w, mut r) = rig(k);
    r.note_on(0, 0);
    let blocks = ms * SR as usize / 1000 / BLOCK_SIZE;
    let off = off.map(|ms| ms * SR as usize / 1000 / BLOCK_SIZE);
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    for b in 0..blocks {
        if Some(b) == off {
            r.note_off(0, 0);
        }
        r.render(1);
        out.extend_from_slice(r.inst.part_bus(0));
    }
    (out, sounding(&r))
}

fn block_at(x: &[f32], ms: usize) -> &[f32] {
    let b = ms * SR as usize / 1000 / BLOCK_SIZE;
    &x[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE]
}

#[test]
fn amp_env_ends_the_hit() {
    let (out, on) = play(&sine_kit(short(), AmpMode::OneShot), 300, None);
    assert!(!on, "still sounding at 300 ms");
    let (first, late) = (peak(block_at(&out, 0)), peak(block_at(&out, 200)));
    assert!(first > 1e-3, "silent: {first}");
    assert!(late < first * 1e-3, "200 ms {late} vs {first}");
    let (_, on) = play(&sine_kit(PadEnv::OPEN, AmpMode::OneShot), 300, None);
    assert!(on, "an OPEN AMP ended the hit");
}

/// ONE-SHOT: no Part VCA route, so note-off changes nothing.
#[test]
fn amp_note_off_is_ignored() {
    let env = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(u7(40))),
        EnvSpeed::Fast,
    );
    let k = sine_kit(env, AmpMode::OneShot);
    let (held, _) = play(&k, 200, None);
    let (off, _) = play(&k, 200, Some(10));
    assert!(peak(&held) > 1e-3);
    assert_eq!(held, off);
}

#[test]
fn amp_gated_note_off_releases() {
    let k = sine_kit(short(), AmpMode::Gated);
    let (out, _) = play(&k, 50, None);
    let (first, at_40) = (peak(block_at(&out, 0)), peak(block_at(&out, 40)));
    assert!(first > 1e-3, "silent: {first}");
    assert!(at_40 > 0.8 * first, "40 ms {at_40} vs {first}");
    let (_, on) = play(&k, 50 + 300, Some(50));
    assert!(!on, "still sounding 300 ms after note-off");
    let (_, on) = play(&k, 300, None);
    assert!(on, "a held GATED hit ended");
}

/// GATED with an infinite decay never releases, as ONE-SHOT's.
#[test]
fn amp_gated_infinite_holds_past_note_off() {
    let env = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Infinite,
        EnvSpeed::Fast,
    );
    let (_, on) = play(&sine_kit(env, AmpMode::Gated), 300, Some(10));
    assert!(on, "an infinite GATED hit released");
}

/// GATED, note-off with the note-on, before a block: attack and hold
/// still play, then the decay releases it.
#[test]
fn amp_gated_short_gate_plays_attack_hold() {
    let env = PadEnv::new(
        EnvPos::new(u7(20)),
        EnvPos::new(u7(40)),
        EnvDecay::Time(EnvPos::new(u7(10))),
        EnvSpeed::Fast,
    );
    let k = sine_kit(env, AmpMode::Gated);
    let (_w, mut r) = rig(&k);
    r.note_on(0, 0);
    r.note_off(0, 0);
    let mut out = Vec::new();
    for _ in 0..(SR as usize / 2 / BLOCK_SIZE) {
        r.render(1);
        out.extend_from_slice(r.inst.part_bus(0));
    }
    let first = peak(&out[..10 * SR as usize / 1000]);
    assert!(first > 1e-3, "silent through attack and hold: {first}");
    assert!(!sounding(&r), "still sounding at 500 ms");
}

// ---------------------------------------------------------------------------
// VEL: the pad's own amp (ADR 0269, owner ruling 2026-11-14)
// ---------------------------------------------------------------------------

/// DRUM INIT's kit with pad 1 (TONE KICK, which ignores velocity) on
/// VEL `vel`.
fn init_kit(vel: f32) -> Kit {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1))
        .sound_mut()
        .amp_mut()
        .set_vel(Unit::new(vel));
    k
}

/// Part 1's peak over 100 ms after a hit on pad 1 at `vel`.
fn hit_peak(k: &Kit, vel: u8) -> f32 {
    let (_w, mut r) = rig(k);
    r.note_on_vel(0, 0, vel);
    let mut p = 0.0f32;
    for _ in 0..(SR as usize / 10 / BLOCK_SIZE) {
        r.render(1);
        p = p.max(peak(r.inst.part_bus(0)));
    }
    p
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

#[test]
fn pad_vel_scales_the_hit() {
    let vel = Kit::DEFAULT.pad(pad(1)).sound().amp().vel().get();
    assert_eq!(vel, 1.0, "DRUM INIT's VEL: its Sound's");
    let k = init_kit(vel);
    let (hard, soft) = (hit_peak(&k, 127), hit_peak(&k, 32));
    assert!(hard > 1e-3, "silent: {hard}");
    let want = 1.0 - vel * (1.0 - 32.0 / 127.0);
    let err = db(soft / hard) - db(want);
    assert!(err.abs() < 0.5, "{} dB vs {} dB", db(soft / hard), db(want));
    // Half VEL, half the way.
    let k = init_kit(0.5);
    let (hard, soft) = (hit_peak(&k, 127), hit_peak(&k, 32));
    let want = 1.0 - 0.5 * (1.0 - 32.0 / 127.0);
    assert!((db(soft / hard) - db(want)).abs() < 0.5);
}

#[test]
fn vel_zero_ignores_velocity() {
    let k = init_kit(0.0);
    let (hard, soft) = (hit_peak(&k, 127), hit_peak(&k, 32));
    assert!(hard > 1e-3, "silent: {hard}");
    // Equal but for the kick's seed, which moves from rig to rig.
    assert!(db(soft / hard).abs() < 0.001, "{} dB", db(soft / hard));
}

/// The law is the kernel's level, before the filter: a soft hit drives a
/// resonant LADDER less, so it comes out louder than the hard hit scaled
/// by the law (the hard one saturates), and still quieter than the hard.
/// Through CLEAN at RES 0, linear, the two agree.
#[test]
fn a_soft_hit_drives_the_ladder_less() {
    let over = |kind: FilterKind, res: f32| {
        let mut k = init_kit(1.0);
        let f = k.pad_mut(pad(1)).sound_mut().filter_mut();
        f.set_kind(kind);
        f.set_cutoff(Hz::new(5000.0));
        f.set_resonance(Unit::new(res));
        let (hard, soft) = (hit_peak(&k, 127), hit_peak(&k, 32));
        assert!(soft < hard, "{kind:?}: {soft} vs {hard}");
        db(soft / hard) - db(32.0 / 127.0)
    };
    let (ladder, clean) = (over(FilterKind::Ladder, 0.95), over(FilterKind::Svf, 0.0));
    assert!(
        clean.abs() < 0.1,
        "CLEAN at RES 0 is {clean} dB off the law"
    );
    assert!(
        ladder > 0.3,
        "the soft hit is only {ladder} dB over the hard one scaled"
    );
}
