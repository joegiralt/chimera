//! VA in the voice (spec § Voice and engine, § Stability): its note ends
//! through the VCA or, with no route, at key-up through the fade; its
//! oscillators and walks run on across the idle slot's rebuild below
//! STABILITY 1 and replay at 1; each (Part, voice slot) recalls its
//! character; its disk code, neutral and INIT.

mod common;

use chimera_core::block::DiskCode;
use chimera_core::dsp::engines::{EngineSlot, SlotKind};
use chimera_core::dsp::filter::k2040::K2040Voice;
use chimera_core::dsp::filter::{FilterKind, FilterMode};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::va::stability::{Character, Seed};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::SampleBudget;
use chimera_core::modulation::{FixedCol, ModSource, ModState};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::rig::Src;
use common::{InstRig, Rig, SR};

fn c4() -> MidiNote {
    MidiNote::new(60).unwrap()
}

/// `blocks` blocks of `v` under `p` and `m`.
fn play(v: &mut Rig, p: &ParamSnapshot, m: &ModState, blocks: usize) -> Vec<f32> {
    let mut b = [0.0; BLOCK_SIZE];
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    for _ in 0..blocks {
        v.render(&mut b, p, m);
        out.extend_from_slice(&b);
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

/// The VA engine a voice holds.
fn va(v: &Voice) -> &chimera_core::dsp::va::VaEngine {
    match v.slot() {
        EngineSlot::Va(e) => e,
        _ => panic!("not VA: {:?}", v.kind()),
    }
}

/// Key-up, then blocks until the voice is idle: how many.
fn release(v: &mut Rig, p: &ParamSnapshot, m: &ModState, most: usize) -> (usize, Vec<f32>) {
    v.note_off();
    let mut out = Vec::new();
    let mut blocks = 0;
    while v.is_active() {
        out.extend(play(v, p, m, 1));
        blocks += 1;
        assert!(
            blocks <= most,
            "still sounding {blocks} blocks after key-up"
        );
    }
    (blocks, out)
}

/// ENV 1 (R 300 ms at MED) ends the note through the VCA: silent and
/// idle within its release and the fade, and silent after.
#[test]
fn va_row_lives_until_its_vca_ends() {
    let s = Sound::init(EngineType::Va);
    let (p, m) = (&s.params, &s.mod_state);
    let mut v = Rig::new(SR);
    v.note_on(c4(), Velocity::DEFAULT, p);
    let held = play(&mut v, p, m, 200);
    assert!(peak(&held[BLOCK_SIZE * 100..]) > 0.05, "INIT sounds");
    assert!(v.is_active());
    // 300 ms of release and the fade, in blocks, with room.
    let most = (SR as usize * 6 / 10 + Voice::FADE as usize) / BLOCK_SIZE;
    let (_, out) = release(&mut v, p, m, most);
    assert!(peak(&out[out.len() - BLOCK_SIZE..]) < 1e-4);
    assert!(play(&mut v, p, m, 4).iter().all(|&x| x == 0.0));
}

/// ENV 1 → VCA deleted: VA never ends itself, so its key ends it,
/// through the voice fade (Review Focus 5).
#[test]
fn va_with_no_vca_route_ends_at_key_up() {
    let s = Sound::init(EngineType::Va);
    let p = &s.params;
    let m = &ModState::new();
    let mut v = Rig::new(SR);
    v.note_on(c4(), Velocity::DEFAULT, p);
    assert!(peak(&play(&mut v, p, m, 50)) > 0.05);
    let fade = Voice::FADE as usize / BLOCK_SIZE;
    let (blocks, out) = release(&mut v, p, m, fade + 1);
    assert!(blocks <= fade + 1, "{blocks} blocks");
    assert_eq!(out.last(), Some(&0.0), "the fade lands on silence");
    assert!(play(&mut v, p, m, 4).iter().all(|&x| x == 0.0));
}

/// Two takes of C4 on one voice, each played 200 blocks then let go until
/// idle: their outputs and each one's phases at note-on.
fn two_takes(stability: f32) -> [(Vec<f32>, [f32; 2]); 2] {
    let mut s = Sound::init(EngineType::Va);
    s.params.va.stability = stability;
    let (p, m) = (&s.params, &s.mod_state);
    let mut v = Rig::new(SR);
    let mut take = || {
        let before = v.rebuilds();
        v.note_on(c4(), Velocity::DEFAULT, p);
        let phase = va(&v).osc_phase();
        let out = play(&mut v, p, m, 200);
        release(&mut v, p, m, 1_000);
        assert!(v.rebuilds() != before, "the idle voice rebuilt its slot");
        (out, phase)
    };
    [take(), take()]
}

/// Below STABILITY 1 the oscillators and the walks run on across the idle
/// voice's rebuild (review finding B1): the next take starts where the
/// last left off, and differs.
#[test]
fn a_voice_va_runs_on() {
    let [(y1, p1), (y2, p2)] = two_takes(0.5);
    assert_eq!(p1, [0.0, 0.0], "a fresh voice starts at 0");
    assert_ne!(p2, [0.0, 0.0], "the second take runs on");
    let d: Vec<f32> = y1.iter().zip(&y2).map(|(a, b)| a - b).collect();
    assert!(rms(&d) >= 0.1 * rms(&y1), "{} vs {}", rms(&d), rms(&y1));
}

/// At STABILITY 1 an idle voice's note starts at phase 0 with no drift:
/// the takes are bit-identical.
#[test]
fn stab_one_replays_on_an_idle_voice() {
    let [(y1, _), (y2, p2)] = two_takes(1.0);
    assert_eq!(p2, [0.0, 0.0]);
    let bits = |y: &[f32]| y.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert!(bits(&y1) == bits(&y2));
}

/// `blocks` blocks of `v` with its knobs gliding (ADR 0112).
fn play_glided(v: &mut Rig, s: &Sound, blocks: usize) -> Vec<f32> {
    let mut b = [0.0; BLOCK_SIZE];
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    for _ in 0..blocks {
        v.render_glided(&mut b, &s.params, &s.mod_state);
        out.extend_from_slice(&b);
    }
    out
}

/// STABILITY gliding to 1.0 through the knob ease lands on exactly 1.0:
/// depth is exactly 0 and the note is bit-identical to a fresh STAB-1
/// voice's. Mid-glide (one block in) it is not.
#[test]
fn stab_glided_to_one_is_a_fresh_stab_one() {
    let bits = |y: &[f32]| y.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    let take = |idle_blocks: usize| {
        let mut s = Sound::init(EngineType::Va);
        s.params.va.stability = 0.5;
        let mut v = Rig::new(SR);
        play_glided(&mut v, &s, 1); // primed at 0.5
        s.params.va.stability = 1.0;
        play_glided(&mut v, &s, idle_blocks);
        v.note_on(c4(), Velocity::DEFAULT, &s.params);
        play_glided(&mut v, &s, 200)
    };
    let mut s = Sound::init(EngineType::Va);
    s.params.va.stability = 1.0;
    let mut fresh = Rig::new(SR);
    fresh.note_on(c4(), Velocity::DEFAULT, &s.params);
    let want = bits(&play_glided(&mut fresh, &s, 200));
    assert!(bits(&take(1_000)) == want, "landed knob is exactly 1.0");
    assert!(bits(&take(1)) != want, "one block in, it is still gliding");
}

/// A note on `p` for Part `part`, played till the voice is idle.
fn idle_note(v: &mut Rig, s: &Sound, part: usize) {
    v.part = PartId::ALL[part];
    v.note_on(c4(), Velocity::DEFAULT, &s.params);
    assert_eq!(v.kind(), SlotKind::of(&s.params));
    play(v, &s.params, &s.mod_state, 20);
    release(v, &s.params, &s.mod_state, 3_000);
}

/// Each (Part, voice slot) recalls its own character (Review Focus 4):
/// a slot that played VA on Part 1, then TAPE and MODAL, plays VA on Part
/// 2 with Part 2's; a fresh Instrument playing Part 1 has Part 1's.
#[test]
fn a_voice_moving_parts_takes_the_parts_character() {
    const SLOT: usize = 3;
    let mut vs = Sound::init(EngineType::Va);
    vs.params.va.stability = 0.5;
    let mut modal = Sound::init(EngineType::Modal);
    (modal.params.modal.mode, modal.params.modal.damp) = (ResonatorMode::String, 0.0);
    let mut v = Rig::at(SR, VoiceIdx::ALL[SLOT], Src::Saw);
    idle_note(&mut v, &vs, 0);
    assert_eq!(
        va(&v).character(),
        Some(Character::of(Seed::of(0, SLOT as u32)))
    );
    idle_note(&mut v, &Sound::init(EngineType::Tape), 0);
    idle_note(&mut v, &modal, 0);
    idle_note(&mut v, &vs, 1);
    assert_eq!(
        va(&v).character(),
        Some(Character::of(Seed::of(1, SLOT as u32)))
    );

    let mut r = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    *r.shared.parts[0].params = vs.params.clone();
    *r.shared.parts[0].mod_state = vs.mod_state.clone();
    r.note_on(0, 60);
    r.render(1);
    let slot = (r.inst.slot_kinds().iter())
        .position(|&k| k == SlotKind::Va)
        .expect("a VA voice");
    assert_eq!(
        va(r.inst.voice(slot)).character(),
        Some(Character::of(Seed::of(0, slot as u32)))
    );
}

#[test]
fn va_init_pins_engine_code_3() {
    assert_eq!(EngineType::Va.disk_code(), 3);
    assert_eq!(EngineType::Va.disk_ident(), "VA");
    assert_eq!(EngineType::from_disk_code(3), Some(EngineType::Va));
    assert_eq!(EngineType::Va.label(), "VA");
}

/// INIT is neutral plus three values (spec § Voice and engine): ENV 1 →
/// VCA 127, OSC 2 FINE +6, STABILITY 0.7; both on the 2040's LP4, REV2.
#[test]
fn init_is_neutral_plus_three() {
    let s = Sound::init(EngineType::Va);
    assert_eq!(
        s.mod_state
            .fixed_amount(ModSource::Env1.index(), FixedCol::Vca),
        127
    );
    assert_eq!(s.params.va.fine, [0.0, 6.0]);
    assert_eq!(s.params.va.stability, 0.7);
    let n = Sound::neutral(EngineType::Va);
    assert_eq!(n.params.va.fine, [0.0, 0.0]);
    assert_eq!(n.params.va.stability, 1.0);
    for p in [&s.params, &n.params] {
        assert_eq!(p.filter.kind(), FilterKind::K2040);
        assert_eq!(p.filter.mode(), FilterMode::Lp24);
        assert_eq!(p.filter.voice, K2040Voice::Rev2);
    }
}
