//! Every parameter the Part's knobs glide is smooth, never steppy (ADR
//! 0112, #419): each of `KNOBS`, the registry's own list, turned through
//! the real path (the panel's acceleration, `Block::nudge`, a publish, the
//! Instrument), held under a sounding note.
//!
//! What is measured is the value the voice hands its engine each block,
//! routes applied (`Instrument::played`), as a place on the param's 0..1
//! scale: CUTOFF in octaves, the rest linear in their units. That is the
//! engine boundary, so a note's decay can't mask it (the output of a
//! plucked MODAL falls whatever the knob does), and whatever the engine
//! does with it after (MODAL's and TAPE's 0.3-a-block ease, a filter's
//! coefficient ramp) only smooths further. It is the voice's copy, which
//! the engines read today; a reader of the stored values would pass
//! unseen. The stage gates and the bill do read those: a gate fades over
//! its own 20 ms, and an op still running is billed (`held_ops_extra`).

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use chimera_core::block::{ParamSpec, Smoothing};
use chimera_core::dsp::knobs::{KNOBS, Knob, coeff};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_hal::SAMPLE_RATE;
use common::part_rig::Rig;
use common::turn::{fast_turn, slow_turn};

/// Where each turn starts, on the param's 0..1 scale: room to go up.
const START: f32 = 0.25;

/// The held Sound a knob is turned on: MODAL's on a BOWED note (it sustains
/// under the bow), the rest on ALGO INIT, sustained.
fn sound(k: &Knob) -> Sound {
    let mut s = if k.addr.block == BlockRef::Modal {
        let mut s = Sound::init(EngineType::Modal);
        s.params.modal.mode = ResonatorMode::Bowed;
        s
    } else {
        Sound::init(EngineType::Algo)
    };
    s.params.envelopes[0].sustain = 1.0;
    let v = k.spec.quantize(k.spec.denormalize(START));
    s.params
        .block_mut(k.addr.block)
        .unwrap()
        .set(k.addr.param, v);
    s
}

/// One knob turned by `ticks`, a block each: per block, the place the
/// voice played and the place stored. The Project is edited only on a
/// tick, so between them the Part glides on an unmoved stamp, as on the
/// unit (the shell publishes every pass; the stamp moves on an edit).
fn turn(k: &Knob, ticks: &[i8]) -> Vec<(f32, f32)> {
    let mut r = Rig::new([sound(k), Sound::init(EngineType::Algo)]);
    r.note(0, true);
    for _ in 0..200 {
        r.render();
    }
    ticks
        .iter()
        .map(|&d| {
            if d != 0 {
                r.sound(0)
                    .params
                    .block_mut(k.addr.block)
                    .unwrap()
                    .nudge(k.addr.param, d);
            }
            r.render();
            (k.spec.normalize(played(&r, k.addr)), stored(&r, k.addr))
        })
        .collect()
}

/// `a`'s stored place, read without an edit.
fn stored(r: &Rig, a: ParamAddr) -> f32 {
    let p = &r.p.part(PartId::ALL[0]).sound.params;
    let s = a.spec().unwrap();
    s.normalize(p.block(a.block).unwrap().get(a.param))
}

/// Part 1's voice's value of `addr` last block.
fn played(r: &Rig, a: ParamAddr) -> f32 {
    let busy = r.inst.allocator().slots().map(|s| !s.is_free());
    let v = busy.iter().position(|&b| b).expect("a voice sounds");
    r.inst.played(a)[v]
}

/// The largest block-to-block move of the played place, and of the stored.
fn steps(t: &[(f32, f32)]) -> (f32, f32) {
    t.windows(2).fold((0.0, 0.0), |(p, s), w| {
        (
            p.max((w[1].0 - w[0].0).abs()),
            s.max((w[1].1 - w[0].1).abs()),
        )
    })
}

/// `1 − k`: the share of a lone jump the glide moves in a block.
fn share(k: &Knob) -> f32 {
    1.0 - coeff(k.tau, SAMPLE_RATE)
}

fn name(k: &Knob) -> String {
    format!("{:?} {}", k.addr.block, k.spec.label)
}

/// Every knob is in the registry as a knob glide, and the list is the
/// registry's: a param that declares it is here, none that doesn't.
#[test]
fn every_knob_is_declared() {
    assert!(KNOBS.len() > 60, "{}", KNOBS.len());
    for b in BlockRef::ALL.into_iter().filter(|b| b.in_sound()) {
        for s in b.specs() {
            let here = KNOBS.iter().any(|k| k.addr == ParamAddr::new(b, s.id));
            assert_eq!(here, s.motion().knob().is_some(), "{b:?} {}", s.label);
        }
    }
}

/// A fast turn (10 ticks a detent, one every 30 ms, then back down slowly)
/// steps the stored value up to a tenth of its range in one block. Played,
/// no block moves more than twice what a lone tick's glide moves in its
/// first block, × the largest jump: never the staircase (which moves the
/// whole jump in one block, 11–23× this).
#[test]
fn a_fast_turn_of_every_knob_glides() {
    let ticks = fast_turn();
    for k in &KNOBS {
        let t = turn(k, &ticks);
        let (played, stored) = steps(&t);
        assert!(stored > 0.0, "{}: the turn moved nothing", name(k));
        let bound = 2.0 * share(k) * stored;
        assert!(
            played <= bound,
            "{}: {played} a block, bound {bound} (stored jumps {stored})",
            name(k)
        );
    }
}

/// A slow turn (a detent every 200 ms, no acceleration): each tick alone,
/// spread over ~3τ, never more than its glide's first block (and 1 % for the last tick's tail).
#[test]
fn a_slow_turn_of_every_knob_glides() {
    let ticks = slow_turn();
    for k in &KNOBS {
        let t = turn(k, &ticks);
        let (played, stored) = steps(&t);
        let bound = share(k) * stored * 1.01;
        assert!(
            played <= bound,
            "{}: {played} a block, bound {bound} (a tick {stored})",
            name(k)
        );
    }
}

/// Settled, every knob hands its engine the stored value bit for bit:
/// a patch at rest sounds as it did before it could glide.
#[test]
fn every_knob_lands_on_the_stored_value() {
    let ticks = fast_turn();
    for k in &KNOBS {
        let mut r = Rig::new([sound(k), Sound::init(EngineType::Algo)]);
        r.note(0, true);
        for &d in &ticks {
            if d != 0 {
                r.sound(0)
                    .params
                    .block_mut(k.addr.block)
                    .unwrap()
                    .nudge(k.addr.param, d);
            }
            r.render();
        }
        for _ in 0..400 {
            r.render();
        }
        let stored = r
            .sound(0)
            .params
            .block_mut(k.addr.block)
            .unwrap()
            .get(k.addr.param);
        assert_eq!(
            played(&r, k.addr).to_bits(),
            stored.to_bits(),
            "{}",
            name(k)
        );
        assert_eq!(
            r.inst.knob(0, k.addr).map(|p| p.1),
            Some(false),
            "{}",
            name(k)
        );
    }
}

/// The declarations #419 asked for, by name: the steppy knobs it found glide
/// at the knobs' 30 ms; CRSE and TRNSP step.
#[test]
fn the_419_knobs_are_declared() {
    use chimera_core::block::{Change, Motion};
    use chimera_core::dsp::algo::params::{ALGO_OP_SPECS, ALGO_SPECS};
    use chimera_core::dsp::modal::MODAL_SPECS;
    let by = |specs: &'static [ParamSpec], l: &str| {
        specs.iter().find(|s| s.label == l).unwrap().motion()
    };
    for l in ["LEVEL", "FDBK", "FINE", "DETUN"] {
        assert_eq!(
            by(&ALGO_OP_SPECS, l),
            Motion::Continuous(Smoothing::KNOB),
            "{l}"
        );
    }
    for l in ["STRUCT", "BRIGHT", "DAMP", "POS", "FORCE", "SPEED"] {
        assert_eq!(
            by(&MODAL_SPECS, l),
            Motion::Continuous(Smoothing::KNOB),
            "{l}"
        );
    }
    for l in ["EXCITE", "BODY", "COUPLE", "HALO", "COLOR", "BURST"] {
        assert_eq!(
            by(&MODAL_SPECS, l),
            Motion::Continuous(Smoothing::NoteOn),
            "{l}"
        );
    }
    assert_eq!(by(&ALGO_OP_SPECS, "CRSE"), Motion::Stepped(Change::Snap));
    assert_eq!(by(&ALGO_SPECS, "TRNSP"), Motion::Stepped(Change::Snap));
}

/// An op LEVEL turned to 0 under a held note keeps sounding while it glides
/// (ADR 0112): the allocator bills it until the kernel stops running it,
/// then drops it (#424).
#[test]
fn a_level_gliding_to_zero_is_billed_until_it_lands() {
    use chimera_core::addr::Op;
    use chimera_core::dsp::algo::params::AlgoOpParams;
    for op in Op::ALL {
        let mut s = Sound::init(EngineType::Algo);
        s.params.envelopes[0].sustain = 1.0;
        for o in &mut s.params.algo.ops {
            o.level = 99;
        }
        let mut r = Rig::new([s, Sound::init(EngineType::Algo)]);
        r.note(0, true);
        for _ in 0..50 {
            r.render();
        }
        let full = r.inst.allocator().sounding_cost();
        r.sound(0).params.algo.ops[op.index()].level = 0;
        let a = ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL);
        let mut blocks = 0;
        while r.inst.knob(0, a).is_none_or(|k| k.1) || blocks == 0 {
            r.render();
            assert_eq!(
                r.inst.allocator().sounding_cost(),
                full,
                "{op:?} block {blocks}"
            );
            blocks += 1;
            assert!(blocks < 1000, "{op:?} never lands");
        }
        assert!(blocks > 100, "{op:?} landed in {blocks} blocks");
        r.render();
        r.render();
        assert!(
            r.inst.allocator().sounding_cost() < full,
            "{op:?} still billed"
        );
    }
}

/// SCRUB's POSITION is its jog, moved by the UI with DRAG's momentum: the
/// knobs hand each step on at once (ADR 0112). On every other machine it
/// glides.
#[test]
fn scrub_position_follows_the_jog_directly() {
    use chimera_core::dsp::tape_engine::{TapeEngineParams, TapeModel};
    let a = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::POSITION);
    for (model, direct) in [(TapeModel::Scrub, true), (TapeModel::Play, false)] {
        let mut s = Sound::init(EngineType::Algo);
        s.params.envelopes[0].sustain = 1.0;
        s.params.tape.machine =
            TapeMachine::of(model);
        let mut r = Rig::new([s, Sound::init(EngineType::Algo)]);
        r.note(0, true);
        for _ in 0..50 {
            r.render();
        }
        for x in [0.6, 0.62, 0.9, 0.1] {
            r.sound(0).params.tape.position = x;
            r.render();
            assert_eq!(played(&r, a) == x, direct, "{model:?} at {x}");
        }
    }
}

/// A sounding Part's knobs are billed once, on one of its voices, however
/// many it plays (ADR 0112): the allocator's bill is the voices' own plus
/// `Knobs::COST` a Part.
#[test]
fn a_sounding_part_is_billed_its_knobs_once() {
    use chimera_core::dsp::knobs::Knobs;
    let mut s = Sound::init(EngineType::Algo);
    s.params.envelopes[0].sustain = 1.0;
    let mut r = Rig::new([s, Sound::init(EngineType::Algo)]);
    r.key(0, 57, true);
    r.render();
    let one = r.inst.allocator().sounding_cost().0;
    r.key(0, 60, true);
    r.key(0, 64, true);
    r.render();
    let three = r.inst.allocator().sounding_cost().0;
    let voice = one - Knobs::COST.0;
    assert_eq!(three, 3 * voice + Knobs::COST.0);
    r.key(1, 57, true);
    r.render();
    let two_parts = r.inst.allocator().sounding_cost().0;
    assert_eq!(two_parts, three + voice + Knobs::COST.0);
}

/// A knob gliding is billed what it costs while it glides: each voice of
/// the Part its filter's moving path (CUTOFF, RES), its FX slot's ramp
/// (P1 under FOLD and SOFT), over the steady bill, until it lands; the bill is
/// the steady one again after.
#[test]
fn a_gliding_knob_is_billed_its_moving_path_until_it_lands() {
    use chimera_core::dsp::filter::FilterKind;
    let kinds = [
        FilterKind::Svf,
        FilterKind::K2040,
        FilterKind::Dirty,
        FilterKind::Ladder,
    ];
    use chimera_core::dsp::voice_fx::params::FxType;
    type Turn = (&'static str, FxType, fn(&mut Sound));
    let p1 = |s: &mut Sound| s.params.fx.pots[0] = 0.9;
    let turns: [Turn; 4] = [
        ("CUTOFF", FxType::Soft, |s| s.params.filter.cutoff = 0.2),
        ("RES", FxType::Fold, |s| s.params.filter.resonance = 0.8),
        ("FOLD", FxType::Fold, p1),
        ("SOFT", FxType::Soft, p1),
    ];
    for kind in kinds {
        for (name, ty, set) in turns {
            let mut s = Sound::init(EngineType::Algo);
            s.params.envelopes[0].sustain = 1.0;
            s.params.filter.set_kind(kind);
            s.params.fx.set_type(ty);
            s.params.fx.pots[0] = 0.3;
            let mut r = Rig::new([s, Sound::init(EngineType::Algo)]);
            for k in [57, 60, 64] {
                r.key(0, k, true);
            }
            for _ in 0..200 {
                r.render();
            }
            let steady = r.inst.allocator().sounding_cost().0;
            set(r.sound(0));
            r.render();
            r.render();
            let moving = r.inst.allocator().sounding_cost().0;
            let extra = if name == "FOLD" || name == "SOFT" {
                ty.glide_cost().0
            } else {
                kind.glide_cost().0
            };
            assert_eq!(moving, steady + 3 * extra, "{kind:?} {name} gliding");
            for _ in 0..400 {
                r.render();
            }
            assert_eq!(
                r.inst.allocator().sounding_cost().0,
                steady,
                "{kind:?} {name} landed"
            );
        }
    }
}
