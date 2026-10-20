//! Every continuous parameter's declaration is true (ADR 0112): the ones
//! the Part's knobs glide are `param_motion_test`'s; here the rest.
//! `Unheard` leaves a held note's output bit for bit; `NoteOn` too, until
//! the next note, which hears it; `Own` is a reviewed list, each smoothed by
//! its DSP, so a change to it is a visible diff.

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use chimera_core::block::{Motion, ParamSpec, Smoothing};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::modal::{self, ResonatorMode};
use chimera_core::dsp::tape_engine::{self as tape, TapeEngineParams, TapeModel};
use chimera_core::modulation::ModSource;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use common::part_rig::Rig;
use common::tape::{held_plain, saw_with_burst};

/// Every Sound param declared `Continuous(s)`.
fn declared(s: Smoothing) -> Vec<(ParamAddr, &'static ParamSpec)> {
    BlockRef::ALL
        .into_iter()
        .filter(|b| b.in_sound())
        .flat_map(|b| b.specs().iter().map(move |p| (ParamAddr::new(b, p.id), p)))
        .filter(|(_, p)| p.motion() == Motion::Continuous(s))
        .collect()
}

fn name(a: ParamAddr, s: &ParamSpec) -> String {
    format!("{:?} {}", a.block, s.label)
}

/// A held Sound where `a` is read, sustained: the engine it belongs to, in
/// a mode or model that reads it; an LFO synced and routed to MORPH. With
/// TAPE, the source it plays.
fn sound(a: ParamAddr) -> (Sound, Option<Vec<u8>>) {
    let engine = match a.block {
        BlockRef::Modal => EngineType::Modal,
        BlockRef::TapeEngine => EngineType::Tape,
        BlockRef::Va => EngineType::Va,
        _ => EngineType::Algo,
    };
    let mut s = Sound::plain_init(engine);
    s.params.envelopes[0].sustain = 1.0;
    let mut src = None;
    match a.block {
        BlockRef::Modal => {
            use ResonatorMode::*;
            let m = [String, Sympathetic, Modal, Bowed];
            s.params.modal.mode = *m.iter().find(|&&m| modal::reads(m, a.param)).unwrap();
            (s.params.modal.ens_mix, s.params.modal.ens_depth) = (0.5, 0.5);
        }
        BlockRef::TapeEngine => {
            use TapeModel::*;
            // PERIOD is STRETCH MONO's lock, DRAG TRON's release (FADE); the
            // rest, the first machine that reads it. A quarter-second
            // source: LOOP wraps within a note.
            let m = [Play, Stretch, Transwave, Tron, Scrub];
            s.params.tape.model = match a.param {
                TapeEngineParams::PERIOD => Stretch,
                TapeEngineParams::DRAG => Tron,
                // SLICE is retired: no machine has a cell for it.
                TapeEngineParams::SLICE => Play,
                id => *m
                    .iter()
                    .find(|&&m| {
                        let k = chimera_core::dsp::tape_engine::TapeMachine::of(m, s.params.tape.mode);
                        tape::reads(k, id)
                    })
                    .unwrap(),
            };
            // START, END and SLICE latch at note-on in ONE; LOOP reads END live.
            if matches!(
                a.param,
                TapeEngineParams::START | TapeEngineParams::END | TapeEngineParams::SLICE
            ) {
                s.params.tape.mode = chimera_core::dsp::tape_engine::PlayMode::One;
            }
            src = Some(held_plain(&saw_with_burst(48_000, 0.25), 48_000));
            // TRON's fade heard past ENV 1's release on the VCA.
            s.params.envelopes[0].release = 1.0;
        }
        BlockRef::Lfo(l) => {
            let i = l as usize;
            s.params.lfos[i].sync = 1;
            let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
            let src = [ModSource::Lfo1, ModSource::Lfo2, ModSource::Lfo3][i];
            assert!(s.mod_state.set_route_addr(src.index(), morph, 127));
        }
        _ => {}
    }
    (s, src)
}

/// `a` set to place `x` (0..1) on Part 1.
fn set(r: &mut Rig, a: ParamAddr, s: &ParamSpec, x: f32) {
    let v = s.quantize(s.denormalize(x));
    r.sound(0)
        .params
        .block_mut(a.block)
        .unwrap()
        .set(a.param, v);
}

/// `blocks` of Part 1's mix from `s`, note 57 held from the start; `at`
/// says, each block, what to do first.
fn run(
    (s, src): (Sound, Option<Vec<u8>>),
    blocks: usize,
    mut at: impl FnMut(&mut Rig, usize),
) -> Vec<Vec<u32>> {
    let other = Sound::plain_init(EngineType::Algo);
    let mut r = match src {
        Some(h) => Rig::with_source([s, other], &h),
        None => Rig::new([s, other]),
    };
    r.note(0, true);
    (0..blocks)
        .map(|b| {
            at(&mut r, b);
            r.mix_bits()
        })
        .collect()
}

/// An `Unheard` param turned end to end under a held note changes nothing.
#[test]
fn an_unheard_param_never_moves_a_held_note() {
    let all = declared(Smoothing::Unheard);
    assert!(!all.is_empty());
    for (a, s) in all {
        let still = run(sound(a), 120, |r, b| {
            if b == 20 || b == 60 {
                r.sound(0);
            }
        });
        let turned = run(sound(a), 120, |r, b| match b {
            20 => set(r, a, s, 1.0),
            60 => set(r, a, s, 0.0),
            _ => {}
        });
        let loud = still[100].iter().any(|&b| f32::from_bits(b).abs() > 1e-4);
        assert!(loud, "{}: the note is silent", name(a, s));
        assert!(still == turned, "{} is heard", name(a, s));
    }
}

/// The next note, 60 blocks after the turn (under a TAPE source's first
/// wrap); it is let go `HELD` blocks on, inside the source (TRON's DRAG is
/// its release), and heard for `AFTER` blocks (LOOP's XFADE at a wrap).
const NEXT: usize = 80;
const HELD: usize = 100;
const AFTER: usize = 400;

/// A `NoteOn` param turned under a held note changes nothing until the next
/// note, which plays it: whether each one was heard after.
fn note_on_runs() -> Vec<(String, bool)> {
    let all = declared(Smoothing::NoteOn);
    assert!(!all.is_empty());
    all.into_iter()
        .map(|(a, s)| {
            let base = sound(a);
            let from = s.normalize(
                BlockRead::block(&base.0.params, a.block)
                    .unwrap()
                    .get(a.param),
            );
            // PERIOD locks only under half the window: 131 frames.
            let to = match (a.block, a.param) {
                (BlockRef::TapeEngine, TapeEngineParams::PERIOD) => 0.002,
                _ if from < 0.5 => 0.9,
                _ => 0.1,
            };
            let next = |r: &mut Rig, b: usize| {
                if b == NEXT {
                    r.key(0, 57, false);
                    r.key(0, 64, true);
                }
                if b == NEXT + HELD {
                    r.key(0, 64, false);
                }
            };
            let still = run(base.clone(), NEXT + AFTER, |r, b| {
                if b == 20 {
                    r.sound(0);
                }
                next(r, b);
            });
            let turned = run(base, NEXT + AFTER, |r, b| {
                if b == 20 {
                    set(r, a, s, to);
                }
                next(r, b);
            });
            assert!(
                still[..NEXT] == turned[..NEXT],
                "{} moves a held note",
                name(a, s)
            );
            (name(a, s), still[NEXT..] != turned[NEXT..])
        })
        .collect()
}

#[test]
fn a_note_on_param_waits_for_the_next_note() {
    let unheard: Vec<String> = note_on_runs()
        .into_iter()
        .filter(|(_, heard)| !heard)
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        unheard, NOT_HEARD_HERE,
        "note-on params the next note didn't hear"
    );
}

/// Note-on params this test's patches can't make heard, and why: each is
/// covered where its feature is set up.
const NOT_HEARD_HERE: &[&str] = &[
    // Read by the formant kernels (Task 10): silent until then.
    "AlgoOp(A) KEY",
    "AlgoOp(A) RES",
    "AlgoOp(B) KEY",
    "AlgoOp(B) RES",
    "AlgoOp(C) KEY",
    "AlgoOp(C) RES",
    "AlgoOp(D) KEY",
    "AlgoOp(D) RES",
    "AlgoOp(E) KEY",
    "AlgoOp(E) RES",
    "AlgoOp(F) KEY",
    "AlgoOp(F) RES",
    // At a steal under STEAL GLIDE: steal_glide_test.
    "Pitch TIME",
];

/// Every Sound param its own DSP smooths: reviewed (ADR 0112 point 1). An
/// op envelope's or an ENV's time, rate or level, or an LFO's rate or
/// slopes: the envelope or oscillator travels to it, never jumps.
#[test]
fn the_params_their_dsp_smooths_are_the_reviewed_list() {
    let got: Vec<String> = declared(Smoothing::Own)
        .into_iter()
        .map(|(a, s)| name(a, s))
        .collect();
    assert_eq!(got, OWN_IN_SOUND);
    let fx: Vec<String> = BlockRef::ALL
        .into_iter()
        .filter(|b| !b.in_sound())
        .flat_map(|b| {
            b.specs()
                .iter()
                .filter(|p| p.motion() == Motion::Continuous(Smoothing::Own))
                .map(move |p| format!("{b:?} {}", p.label))
        })
        .collect();
    assert_eq!(fx, OWN_OUTSIDE);
}

const OWN_IN_SOUND: &[&str] = &[
    // The op envelope: a rate bends the stage it is in (`set_coefs`), D1L
    // moves DECAY 1's target, RS rescales the rates.
    "AlgoOp(A) AR",
    "AlgoOp(A) D1R",
    "AlgoOp(A) D1L",
    "AlgoOp(A) D2R",
    "AlgoOp(A) RR",
    "AlgoOp(A) RS",
    "AlgoOp(B) AR",
    "AlgoOp(B) D1R",
    "AlgoOp(B) D1L",
    "AlgoOp(B) D2R",
    "AlgoOp(B) RR",
    "AlgoOp(B) RS",
    "AlgoOp(C) AR",
    "AlgoOp(C) D1R",
    "AlgoOp(C) D1L",
    "AlgoOp(C) D2R",
    "AlgoOp(C) RR",
    "AlgoOp(C) RS",
    "AlgoOp(D) AR",
    "AlgoOp(D) D1R",
    "AlgoOp(D) D1L",
    "AlgoOp(D) D2R",
    "AlgoOp(D) RR",
    "AlgoOp(D) RS",
    "AlgoOp(E) AR",
    "AlgoOp(E) D1R",
    "AlgoOp(E) D1L",
    "AlgoOp(E) D2R",
    "AlgoOp(E) RR",
    "AlgoOp(E) RS",
    "AlgoOp(F) AR",
    "AlgoOp(F) D1R",
    "AlgoOp(F) D1L",
    "AlgoOp(F) D2R",
    "AlgoOp(F) RR",
    "AlgoOp(F) RS",
    // ENV: a time bends the stage in progress, never its level.
    "Env(Env1) ATK",
    "Env(Env1) DEC",
    "Env(Env1) REL",
    "Env(Env1) H",
    "Env(Env1) RISE",
    "Env(Env1) FALL",
    "Env(Env2) ATK",
    "Env(Env2) DEC",
    "Env(Env2) REL",
    "Env(Env2) H",
    "Env(Env2) RISE",
    "Env(Env2) FALL",
    "Env(Env3) ATK",
    "Env(Env3) DEC",
    "Env(Env3) REL",
    "Env(Env3) H",
    "Env(Env3) RISE",
    "Env(Env3) FALL",
    // LFO: RATE moves the phase's speed, RISE and FALL its slopes.
    "Lfo(Lfo1) RATE",
    "Lfo(Lfo1) RISE",
    "Lfo(Lfo1) FALL",
    "Lfo(Lfo2) RATE",
    "Lfo(Lfo2) RISE",
    "Lfo(Lfo2) FALL",
    "Lfo(Lfo3) RATE",
    "Lfo(Lfo3) RISE",
    "Lfo(Lfo3) FALL",
    // OUT's sends: the mixer's ease (`MixState`), as the Part's gains.
    "Out CHORUS",
    "Out DELAY",
    "Out REVERB",
];

/// The bus, mixer and master: each eases its own settings (ADR 0061).
const OWN_OUTSIDE: &[&str] = &[
    // Chorus: DEPTH and MIX `Ease`; RATE moves its LFO's speed.
    "Chorus RATE",
    "Chorus DEPTH",
    "Chorus MIX",
    // Delay: TIME crossfades two heads over 20 ms; the rest `Ease`.
    "Delay TIME",
    "Delay FDBK",
    "Delay MECHANICS",
    "Delay SAT",
    "Delay TONE",
    "Delay MIX",
    "Delay REV",
    // Reverb: each TYPE eases its gains and filters (τ 20-50 ms), crossfades
    // its read heads (30 ms), and MIX `Ease`s; TYPE and SYNC are choices.
    "Reverb TIME",
    "Reverb DAMP",
    "Reverb SIZE",
    "Reverb MIX",
    "Reverb TONE",
    "Reverb DIFFUSE",
    "Reverb PRE-DLY",
    "Reverb SHAPE",
    "Reverb SMEAR",
    "Reverb RATE",
    "Reverb DENSITY",
    // The master tape ramps its smoothed controls across each block.
    "Tape DRIVE",
    "Tape TONE",
    "Tape WOW",
    "Tape MIX",
    // The compressor's per-block one-poles.
    "Comp THRESH",
    "Comp ATK",
    "Comp REL",
    "Comp MAKEUP",
    "Comp MIX",
    // The master VOL and PAN, `Ease`d after the limiter (`MasterOut`).
    "Comp VOL",
    "Comp PAN",
    // The Part's pan × LEVEL gains, `Ease`.
    "Part LEVEL",
    "Part PAN",
    // The wheel's reach, glided as a knob (`Instrument::ease_bend`).
    "Part BEND",
];
