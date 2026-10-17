//! TAPE in the engine slot (spec § 2 Structure, § 3 neutral and INIT): its
//! size against the slot and D2, its neutral, INIT's ENV 1 into VCA, the
//! slot's kinds, a fresh note's ramp from silence, the source it latches,
//! its bill, and each voice's own strip of tape.

mod common;

use core::mem::size_of;

use chimera_core::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use chimera_core::dsp::modal::{ModalEngine, SymPool};
use chimera_core::dsp::tape_engine::{
    COST_LOOP, COST_ONESHOT, PlayMode, StretchType, TapeEngine, TapeEngineParams, TapeModel, Wow,
};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{MAX_VOICES, VOICE_CHAIN_BYTES, VOICE_RAM_BUDGET};
use chimera_core::modulation::{FixedCol, ModSource, ModState};
use chimera_core::params::{EngineType, ParamSnapshot, Steal};
use chimera_core::preset::Sound;
use chimera_core::project::SourceId;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::rig::Src;
use common::tape::{held_plain, saw_with_burst};
use common::{Rig, SR, step};

/// 1 s of the saw with its burst at 24 kHz.
fn saw() -> Vec<u8> {
    held_plain(&saw_with_burst(24_000, 1.0), 24_000)
}

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

#[test]
fn tape_engine_fits_the_slot() {
    assert!(
        size_of::<TapeEngine>() < size_of::<ModalEngine>(),
        "TapeEngine {} B, ModalEngine {} B",
        size_of::<TapeEngine>(),
        size_of::<ModalEngine>()
    );
}

/// ADR 0013: the voices fit D2 with the played snapshots in them, and a
/// voice is its chain plus one slot (ADR 0051).
#[test]
fn voices_fit_d2() {
    let all = size_of::<[Voice; MAX_VOICES]>();
    assert!(all <= VOICE_RAM_BUDGET, "[Voice; {MAX_VOICES}] = {all} B");
    assert!(size_of::<Voice>() <= VOICE_CHAIN_BYTES + size_of::<EngineSlot>());
}

/// Spec § 3: the frozen base loops the whole source with a folded 10 %
/// crossfade.
#[test]
fn neutral_tape_loops_whole() {
    let s = Sound::neutral(EngineType::Tape);
    assert_eq!(s.engine(), EngineType::Tape);
    assert_eq!(
        s.params.tape,
        TapeEngineParams {
            model: TapeModel::Play,
            mode: PlayMode::Loop,
            start: 0.0,
            end: 1.0,
            position: 0.0,
            motion: 0.0,
            tape: 0.0,
            age: 0.0,
            size: 1.0,
            xfade: 0.1,
            drag: 0.3,
            kind: StretchType::Mono,
            root: 6_000.0,
            period: 0.0,
            slice: chimera_core::dsp::tape_engine::SliceCode::OFF,
            wow: Wow::Capstan,
        }
    );
}

/// Spec § 2: a loop has no end, so INIT routes ENV 1 into VCA and the note
/// ends when ENV 1 lets go: silent, and the voice idle, within 1 s.
#[test]
fn init_routes_env1_to_vca() {
    let s = Sound::init(EngineType::Tape);
    assert_eq!(
        s.mod_state
            .fixed_amount(ModSource::Env1.index(), FixedCol::Vca),
        127
    );
    let (p, m) = (&s.params, &s.mod_state);
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, p);
    let held = play(&mut v, p, m, 375);
    assert!(peak(&held[BLOCK_SIZE * 100..]) > 0.05, "INIT sounds");
    v.note_off();
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    let mut blocks = 0;
    while v.is_active() {
        v.render(&mut b, p, m);
        out.extend_from_slice(&b);
        blocks += 1;
        assert!(blocks <= 750, "still sounding 1 s after ENV 1 let go");
    }
    assert!(peak(&out[out.len() - BLOCK_SIZE..]) < 1e-3);
    assert!(play(&mut v, p, m, 4).iter().all(|&x| x == 0.0));
}

/// Every `match` lists TAPE: four engines, its own slot kind, and a voice
/// that plays ALGO, then TAPE, then MODAL starts MODAL as a fresh voice
/// does.
#[test]
fn every_match_lists_tape() {
    assert_eq!(EngineType::ALL.len(), 4);
    let tape = Sound::init(EngineType::Tape);
    assert_eq!(SlotKind::of(&tape.params), SlotKind::Tape);
    assert_eq!(SlotKind::Tape.engine(), EngineType::Tape);
    assert_eq!(SlotKind::Tape.resting(), SlotKind::Tape);

    let algo = Sound::init(EngineType::Algo);
    let modal = Sound::init(EngineType::Modal);
    let first = |v: &mut Rig| {
        v.note_on(c4(), Velocity::DEFAULT, &modal.params);
        play(v, &modal.params, &modal.mod_state, 8)
    };
    let mut v = Rig::with_source(SR, &saw());
    for s in [&algo, &tape] {
        v.note_on(c4(), Velocity::DEFAULT, &s.params);
        play(&mut v, &s.params, &s.mod_state, 40);
        assert_eq!(v.kind(), SlotKind::of(&s.params));
        v.kill();
        play(&mut v, &s.params, &s.mod_state, 4);
        assert!(!v.is_active());
    }
    let fresh = first(&mut Rig::new(SR));
    assert!(peak(&fresh) > 1e-3);
    assert_eq!(first(&mut v), fresh);
}

/// A TAPE note whose engine went quiet on its own (its source's row gone),
/// the voice idle and never reset: a fresh note's first sample is 0, not
/// the old note's last (`trigger`'s `hold`).
#[test]
fn fresh_note_after_idle_starts_at_zero() {
    let p = &Sound::neutral(EngineType::Tape).params;
    // No route into VCA: the voice passes the engine's first sample on at
    // OUT LEVEL, not from a VCA gain ramping up from 0.
    let m = ModState::new();
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, p);
    let out = play(&mut v, p, &m, 9);
    assert!(out[out.len() - 1].abs() > 1e-3, "the note ends off zero");
    own(&v).clear();
    play(&mut v, p, &m, 1);
    assert!(!v.is_active(), "its source gone, the note ends");
    own(&v).publish();
    v.note_on(c4(), Velocity::DEFAULT, p);
    let out = play(&mut v, p, &m, 1);
    // The filter's state leaves a residue far under a sample's step.
    assert!(out[0].abs() < 1e-6, "{}", out[0]);
    assert!(peak(&out) > 1e-3, "the fresh note sounds");
}

/// TAPE sounds only through its source: with none latched it is silent and
/// its note ends.
#[test]
fn no_source_plays_nothing() {
    let s = Sound::init(EngineType::Tape);
    let mut v = Rig::at(SR, VoiceIdx::ALL[0], Src::None);
    v.note_on(c4(), Velocity::DEFAULT, &s.params);
    assert!(
        play(&mut v, &s.params, &s.mod_state, 4)
            .iter()
            .all(|&x| x == 0.0)
    );
    assert!(!v.is_active());
}

/// No route into VCA: the key holds a TAPE note, as `vca_holds` holds any
/// other source's, and key up ends it through the voice's fade, without a
/// click.
#[test]
fn unrouted_tape_ends_at_key_up() {
    let p = Sound::neutral(EngineType::Tape).params;
    let m = ModState::new();
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, &p);
    let out = play(&mut v, &p, &m, 40);
    assert!(
        peak(&out[out.len() - BLOCK_SIZE..]) > 0.05,
        "held, it sounds"
    );
    // The saw's own edge is the held note's largest step (at TAPE's
    // `out_gain` it passes `clicks`' fixed bar): the fade steps no more.
    let held = step(&out);
    v.note_off();
    let mut fade = vec![out[out.len() - 1]];
    fade.extend(play(&mut v, &p, &m, Voice::FADE as usize / BLOCK_SIZE + 1));
    assert!(!v.is_active(), "key up ends it");
    assert!(step(&fade) <= held, "{} over {held}", step(&fade));
}

/// Each voice its own strip of tape (spec § 2 TAPE): the engine is seeded
/// by its voice's index, so two voices' wows differ.
#[test]
fn voices_are_seeded_apart() {
    let mut s = Sound::neutral(EngineType::Tape);
    s.params.tape.tape = 1.0;
    let m = ModState::new();
    let held = saw();
    let run = |i: usize| {
        let src = Src::Own(common::tape::OneSource::new(&held));
        let mut v = Rig::at(SR, VoiceIdx::ALL[i], src);
        v.note_on(c4(), Velocity::DEFAULT, &s.params);
        play(&mut v, &s.params, &m, 200)
    };
    assert_ne!(run(0), run(1));
}

/// `v`'s own source: what `with_source` plays.
fn own(v: &Rig) -> &common::tape::OneSource {
    let Src::Own(src) = &v.src else {
        unreachable!()
    };
    src
}

/// `f` on `v`'s voice with its own source's view.
fn with_view(v: &mut Rig, f: impl FnOnce(&mut Voice, &mut EngineCtx<'_>)) {
    let Rig {
        voice, pool, src, ..
    } = v;
    let Src::Own(src) = src else { unreachable!() };
    let view = src.view();
    f(
        voice,
        &mut EngineCtx {
            sym: pool,
            streams: None,
            src: &view,
            part: chimera_core::project::PartId::ALL[0],
            tune: 0.0,
        },
    );
}

/// A closed gate moves the source mid-note: the engine reads nothing and
/// the note waits; the gate opens and it plays on (`GateView::bytes`'s
/// contract). TAPE Task 13's `sounding` mask keeps a compaction off every
/// source a voice reads, so in play neither this wait nor a note-on on a
/// moving source (which `start` drops) is reached.
#[test]
fn a_moving_source_holds_the_note() {
    let s = Sound::init(EngineType::Tape);
    let (p, m) = (&s.params, &s.mod_state);
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, p);
    play(&mut v, p, m, 20);
    own(&v).close();
    play(&mut v, p, m, 2);
    assert!(v.is_active(), "it waits");
    own(&v).open();
    assert!(peak(&play(&mut v, p, m, 8)) > 0.05, "and plays on");
}

/// A model that can't play its source (TRANSWAVE on a `Plain` one,
/// `wave::playable`) plays nothing, and the note ends.
#[test]
fn an_unplayable_source_plays_nothing() {
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape.model = TapeModel::Transwave;
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, &s.params);
    assert!(
        play(&mut v, &s.params, &s.mod_state, 4)
            .iter()
            .all(|&x| x == 0.0)
    );
    assert!(!v.is_active());
}

/// A note with no source latches none: it never plays the last note's,
/// even on the engine that played it.
#[test]
fn no_source_unlatches_the_last() {
    let s = Sound::init(EngineType::Tape);
    let (p, m) = (&s.params, &s.mod_state);
    let mut src = common::tape::OneSource::new(&saw());
    let mut sym = SymPool::boxed();
    let mut v = Voice::new(SR);
    let mut b = [0.0f32; BLOCK_SIZE];
    // A block; first, a fresh note on the source or on none.
    let mut block = |v: &mut Voice, on: Option<bool>| {
        let id = src.id;
        let view = src.view();
        let ctx = &mut EngineCtx {
            sym: &mut sym,
            streams: None,
            src: &view,
            part: chimera_core::project::PartId::ALL[0],
            tune: 0.0,
        };
        if let Some(latched) = on {
            v.note_on(c4(), Velocity::DEFAULT, p, latched.then_some(id), ctx);
        }
        v.render(&mut b, p, m, ctx);
        peak(&b)
    };
    block(&mut v, Some(true));
    assert!((0..8).map(|_| block(&mut v, None)).fold(0.0, f32::max) > 0.05);
    // A retrigger on the sounding voice, with no source: it ends.
    block(&mut v, Some(false));
    assert!(!v.is_active());
}

/// A sounding TAPE note is billed the model it latched, `COST_LOOP` here,
/// under an edit to a cheaper one (spec § 5; `cost_test` has each bill).
#[test]
fn a_sounding_note_bills_its_latched_model() {
    let s = Sound::init(EngineType::Tape);
    let mut slot = EngineSlot::new(SlotKind::Tape);
    assert_eq!(slot.playing_cost(), None);
    let mut src = common::tape::OneSource::new(&saw());
    let (id, view) = (src.id, src.view());
    let mut sym = SymPool::boxed();
    let ctx = &mut EngineCtx {
        sym: &mut sym,
        streams: None,
        src: &view,
        part: chimera_core::project::PartId::ALL[0],
        tune: 0.0,
    };
    slot.note_on(c4(), Velocity::DEFAULT, &s.params, SR, Some(id), ctx);
    assert_eq!(slot.playing_cost(), Some(COST_LOOP));
    let mut edit = s.params.clone();
    edit.tape.mode = PlayMode::One;
    assert_eq!(slot.model_cost(&s.params), COST_LOOP);
    assert_eq!(slot.model_cost(&edit), COST_ONESHOT);
    assert_eq!(
        TapeEngine::model_cost(TapeModel::Play, PlayMode::One),
        COST_ONESHOT,
        "the edit's"
    );
}

/// A retune (ORBIT's re-snap) and a glide steal glide a sounding TAPE
/// note's pitch, never stepping it.
#[test]
fn retune_and_glide_steal_glide() {
    let mut s = Sound::init(EngineType::Tape);
    let m = &s.mod_state.clone();
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, &s.params);
    play(&mut v, &s.params, m, 4);
    assert_eq!(v.slide(), 1.0);
    with_view(&mut v, |v, ctx| {
        v.retune(MidiNote::new(72).unwrap(), 4_800, ctx)
    });
    assert!(v.slide() < 0.9, "from an octave down: {}", v.slide());

    s.params.pitch.steal = Steal::Glide;
    let p = &s.params;
    let mut v = Rig::with_source(SR, &saw());
    v.note_on(c4(), Velocity::DEFAULT, p);
    play(&mut v, p, m, 4);
    v.note_on(MidiNote::new(72).unwrap(), Velocity::DEFAULT, p);
    assert!(v.slide() < 0.9, "a steal glides: {}", v.slide());
}

/// A voice's strip of tape runs on from note to note (spec § 2 TAPE): its
/// next note, after the voice went idle and was reset, isn't its last
/// note again.
#[test]
fn a_voice_strip_runs_on() {
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape.tape = 1.0;
    let (p, m) = (&s.params, &s.mod_state);
    let mut v = Rig::with_source(SR, &saw());
    let mut note = || {
        v.note_on(c4(), Velocity::DEFAULT, p);
        let out = play(&mut v, p, m, 200);
        v.note_off();
        while v.is_active() {
            play(&mut v, p, m, 1);
        }
        out
    };
    let first = note();
    assert_ne!(first, note());
}

/// A note that comes mid-fade waits for it, then plays its own source.
#[test]
fn a_note_waiting_a_fade_plays_its_source() {
    let s = Sound::init(EngineType::Tape);
    let (p, m) = (&s.params, &s.mod_state);
    // The second source is silence: the queued note sounding would be the
    // first source's.
    let hush = held_plain(&[0.0; 4_800], 24_000);
    let (mut src, second) = common::tape::OneSource::with(&saw(), &hush);
    let mut sym = SymPool::boxed();
    let mut v = Voice::new(SR);
    let mut b = [0.0f32; BLOCK_SIZE];
    let mut block = |v: &mut Voice, on: Option<_>| {
        let view = src.view();
        let ctx = &mut EngineCtx {
            sym: &mut sym,
            streams: None,
            src: &view,
            part: chimera_core::project::PartId::ALL[0],
            tune: 0.0,
        };
        if let Some(id) = on {
            v.note_on(c4(), Velocity::DEFAULT, p, Some(id), ctx);
        }
        v.render(&mut b, p, m, ctx);
        peak(&b)
    };
    block(&mut v, Some(SourceId::ALL[0]));
    let first = (0..20).map(|_| block(&mut v, None)).fold(0.0, f32::max);
    assert!(first > 0.05);
    v.kill();
    block(&mut v, Some(second));
    for _ in 0..Voice::FADE as usize / BLOCK_SIZE {
        block(&mut v, None);
    }
    let then = (0..20).map(|_| block(&mut v, None)).fold(0.0, f32::max);
    assert!(v.is_active(), "the queued note plays");
    assert!(
        then < 1e-3,
        "on its own source, not the last note's: {then}"
    );
}

/// Slice 1 § 10.3: the deck holds its stream's cursor and still fits
/// inside MODAL's slot, so `EngineSlot` (and a voice) doesn't grow.
#[test]
fn tape_with_its_cursor_stays_under_modal() {
    assert!(
        size_of::<TapeEngine>() < size_of::<ModalEngine>(),
        "TAPE {} B, MODAL {} B",
        size_of::<TapeEngine>(),
        size_of::<ModalEngine>()
    );
}
