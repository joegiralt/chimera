//! LFO DEPTH as a matrix destination (#876): INIT's wheel vibrato is a
//! route, WHEEL → LFO 1 DEPTH at +127, not a CC 1 map entry.

mod common;
use common::InstRig;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::midi_map::{Input, default_for};
use chimera_core::modulation::ModSource;
use chimera_core::note_queue::{Expr, ExprEvent};
use chimera_core::params::{EngineType, PitchParams};
use chimera_core::preset::Sound;
use chimera_midi::value::U7;

const DEPTH1: ParamAddr = ParamAddr::new(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH);
const PITCH: ParamAddr = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
const ENGINES: [EngineType; 4] = [
    EngineType::Algo,
    EngineType::Modal,
    EngineType::Tape,
    EngineType::Va,
];

#[test]
fn lfo_depth_is_a_destination() {
    for s in LfoSlot::ALL {
        assert!(ParamAddr::new(BlockRef::Lfo(s), LfoParams::DEPTH).modulatable());
        assert!(!ParamAddr::new(BlockRef::Lfo(s), LfoParams::RATE).modulatable());
    }
}

#[test]
fn init_routes_the_wheel_to_lfo1_depth_and_cc1_is_unmapped() {
    let cc1 = Input::cc(1).unwrap();
    for e in ENGINES {
        let s = Sound::init(e);
        assert!(s.dest_registry.is_primed(DEPTH1), "{e:?}");
        let d = s.mod_state.find(DEPTH1).expect("an LFO 1 DEPTH column");
        assert_eq!(
            s.mod_state.amount(ModSource::Wheel.index(), d),
            127,
            "{e:?}"
        );
        assert!(
            default_for(e).entries().iter().all(|b| b.input != cc1),
            "{e:?}: CC 1 is the wheel, not a map entry"
        );
    }
}

/// INIT's PITCH as voice `v` plays it, each block for `blocks` blocks.
fn pitches(rig: &mut InstRig, v: usize, blocks: usize) -> Vec<f32> {
    (0..blocks)
        .map(|_| {
            rig.render(1);
            rig.inst.played(PITCH)[v]
        })
        .collect()
}

fn spread(xs: &[f32]) -> f32 {
    let (lo, hi) = xs
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    hi - lo
}

/// At rest the wheel leaves INIT's pitch still; at full it opens LFO 1
/// DEPTH to 1: LFO 1 → PITCH at 2 swings ±2/127 of PITCH's 24 semitones.
#[test]
fn the_wheel_opens_inits_vibrato() {
    let s = Sound::init(EngineType::Algo);
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *rig.shared.parts[0].params = s.params.clone();
    *rig.shared.parts[0].mod_state = s.mod_state.clone();
    rig.note_on(0, 60);
    rig.render(4);
    let slots = rig.inst.allocator().slots();
    let v = (0..slots.len()).find(|&v| slots[v].held()).unwrap();
    // LFO 1 at 1 Hz: 750 blocks a cycle.
    assert_eq!(spread(&pitches(&mut rig, v, 800)), 0.0, "still at rest");
    rig.inst.expr(ExprEvent {
        channel: MidiChannel::new(0).unwrap(),
        expr: Expr::Wheel(U7::new(127).unwrap()),
    });
    rig.render(100);
    let swing = spread(&pitches(&mut rig, v, 800)) / 2.0;
    let want = 24.0 * 2.0 / 127.0;
    assert!(
        (swing - want).abs() < 0.02 * want,
        "±{swing} st, want ±{want}"
    );
}

/// INIT ALGO's rig with `mods` (INIT's own when `None`), its LFO 1 at
/// `depth`.
fn init_rig(mods: Option<chimera_core::modulation::ModState>, depth: f32) -> InstRig {
    let s = Sound::init(EngineType::Algo);
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *rig.shared.parts[0].params = s.params.clone();
    rig.shared.parts[0].params.lfos[0].depth = depth;
    *rig.shared.parts[0].mod_state = mods.unwrap_or(s.mod_state);
    rig
}

fn wheel(rig: &mut InstRig, v: u8) {
    rig.inst.expr(ExprEvent {
        channel: MidiChannel::new(0).unwrap(),
        expr: Expr::Wheel(U7::new(v).unwrap()),
    });
}

fn held_voice(rig: &InstRig) -> usize {
    let slots = rig.inst.allocator().slots();
    (0..slots.len()).find(|&v| slots[v].held()).unwrap()
}

/// A negative route closes DEPTH to 0, never past it.
#[test]
fn a_negative_route_clamps_depth_at_zero() {
    use chimera_core::modulation::{MAX_MOD_SOURCES, ModState};
    let mut m = ModState::with_sources(MAX_MOD_SOURCES);
    assert!(m.set_route_addr(ModSource::Wheel.index(), DEPTH1, -127));
    let mut rig = init_rig(Some(m), 0.5);
    rig.note_on(0, 60);
    rig.render(4);
    let v = held_voice(&rig);
    assert_eq!(rig.inst.played(DEPTH1)[v], 0.5);
    wheel(&mut rig, 127);
    rig.render(400);
    assert_eq!(rig.inst.played(DEPTH1)[v], 0.0);
}

/// A note struck with the wheel up plays its depth from its first block
/// (the retrigger's sums take the wheel now, as VEL and NOTE).
#[test]
fn a_new_note_hears_the_wheel_in_its_first_block() {
    let mut rig = init_rig(None, 0.0);
    wheel(&mut rig, 127);
    rig.render(400);
    rig.note_on(0, 60);
    rig.render(1);
    let v = held_voice(&rig);
    let d = rig.inst.played(DEPTH1)[v];
    assert!((d - 1.0).abs() < 1e-3, "{d}");
}

/// A fading voice keeps the depth it last played: the wheel going down
/// mid-fade moves nothing.
#[test]
fn a_fading_voice_keeps_its_depth() {
    use chimera_core::modulation::{MAX_MOD_SOURCES, ModState};
    // Half way, so a sum laid twice would show.
    let mut m = ModState::with_sources(MAX_MOD_SOURCES);
    assert!(m.set_route_addr(ModSource::Wheel.index(), DEPTH1, 64));
    let mut rig = init_rig(Some(m), 0.0);
    rig.note_on(0, 60);
    wheel(&mut rig, 127);
    rig.render(400);
    let v = held_voice(&rig);
    let up = rig.inst.played(DEPTH1)[v];
    assert!((up - 64.0 / 127.0).abs() < 1e-3, "{up}");
    // Another engine: the held voice fades the old one out first (two
    // blocks, `Voice::FADE`).
    *rig.shared.parts[0].params = {
        let mut p = Sound::init(EngineType::Va).params;
        p.lfos[0].depth = 0.0;
        p
    };
    wheel(&mut rig, 0);
    let mut faded = 0;
    for _ in 0..40 {
        rig.render(1);
        if !rig.inst.voice(v).fading() {
            break;
        }
        faded += 1;
        assert_eq!(rig.inst.played(DEPTH1)[v], up, "block {faded}");
    }
    assert!(faded >= 1, "it faded");
}
