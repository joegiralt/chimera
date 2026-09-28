//! The matrix's eight sources (filter-routing spec § 2).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{ModSource, ModState, note_source};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::ui::block_registry::PART_MOD_SOURCES;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

#[test]
fn sources_are_in_spec_order() {
    let tags = ["E1", "LF1", "E2", "E3", "LF2", "LF3", "VEL", "NTE"];
    for (i, s) in ModSource::ALL.iter().enumerate() {
        assert_eq!(s.index(), i);
        assert_eq!(s.tag(), tags[i]);
        assert!(s.tag().len() <= 3, "#15");
    }
    assert_eq!(PART_MOD_SOURCES, tags);
    for ct in EngineType::ALL {
        assert_eq!(chain_def_for(ct).mod_sources, tags, "{ct:?}");
    }
    // A two-source `ModState` still maps 0 → ENV1 and 1 → LFO1.
    assert_eq!((ModSource::Env1.index(), ModSource::Lfo1.index()), (0, 1));
}

#[test]
fn note_source_is_a_tenth_of_an_octave_law() {
    assert_eq!(note_source(MidiNote::new(60).unwrap()), 0.0);
    assert_eq!(note_source(MidiNote::new(72).unwrap()), 0.1);
    assert_eq!(note_source(MidiNote::new(0).unwrap()), -0.5);
}

/// 24 blocks of `note` with one route `source → CUTOFF` at `amount`.
fn render(source: ModSource, amount: i8, note: u8, vel: u8) -> Vec<f32> {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.cutoff = 1000.0;
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF, *b"FLTCUTOF").unwrap();
    let mut ms = ModState::from_registry(&reg, 8);
    ms.set_amount(source.index(), 0, amount);
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(vel).unwrap(),
        &p,
    );
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..24 {
        v.render(&mut b, &p, &ms);
        out.extend_from_slice(&b);
    }
    out
}

#[test]
fn env1_lfo1_vel_and_note_move_their_destination() {
    for (source, note) in [
        (ModSource::Env1, 60),
        (ModSource::Lfo1, 60),
        (ModSource::Vel, 60),
        (ModSource::Note, 72),
    ] {
        assert_ne!(
            render(source, 100, note, 100),
            render(source, 0, note, 100),
            "{source:?}"
        );
    }
    // VEL follows the velocity. The route's effect is measured against the
    // same velocity with the route at 0, so the engine's own velocity
    // response cancels; a harder note opens the filter further. (ENV 1
    // carries none: `envelope.rs`'s unit test until Task 5, then `note_on`'s
    // signature, which takes none.)
    let effect = |vel| {
        let (on, off) = (
            render(ModSource::Vel, 100, 60, vel),
            render(ModSource::Vel, 0, 60, vel),
        );
        let diff: f32 = on.iter().zip(&off).map(|(a, b)| (a - b).abs()).sum();
        diff / off.iter().map(|x| x.abs()).sum::<f32>()
    };
    assert!(
        effect(120) > effect(30),
        "{} vs {}",
        effect(120),
        effect(30)
    );
}

#[test]
fn the_amount_table_is_the_divide() {
    for a in -127i8..=127 {
        assert_eq!(
            chimera_core::modulation::amount_scale(a).to_bits(),
            (a as f32 / 127.0).to_bits(),
            "{a}"
        );
    }
}

/// Spec § Tests "Matrix": a route from each of the eight sources moves its
/// destination (NOTE at note 72, where it isn't 0).
#[test]
fn every_source_moves_cutoff() {
    for s in ModSource::ALL {
        let note = if s == ModSource::Note { 72 } else { 60 };
        assert_ne!(render(s, 127, note, 100), render(s, 0, note, 100), "{s:?}");
    }
}

use chimera_core::modulation::{CUTOFF as CUTOFF_ADDR, MAX_MOD_SOURCES};
use chimera_core::preset::Sound;
use chimera_core::ui::mod_grid::MatrixState;

const DEFAULT_BITS: u8 = 1 << 0 | 1 << 1 | 1 << 7; // ENV1, LFO1, NOTE

#[test]
fn presence_is_apart_from_the_amount() {
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF_ADDR, *b"FLTCUTOF").unwrap();
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    assert_eq!(ms.routes_into(CUTOFF_ADDR), 0);
    ms.set_route(ModSource::Note.index(), 0, 0);
    assert_eq!(ms.routes_into(CUTOFF_ADDR), 1 << 7, "a route at 0 exists");
    ms.set_amount(ModSource::Lfo1.index(), 0, 9);
    assert_eq!(
        ms.routes_into(CUTOFF_ADDR),
        1 << 7 | 1 << 1,
        "set_amount creates"
    );
    let values = [1.0f32; MAX_MOD_SOURCES];
    ms.set_amount(ModSource::Lfo1.index(), 0, 0);
    assert_eq!(
        ms.sum_for(0, &values),
        0.0,
        "a present route at 0 adds nothing"
    );
}

#[test]
fn presence_survives_sync_from_matrix() {
    let sound = Sound::init(EngineType::Algo);
    let mut m = MatrixState::new();
    m.rebuild_sources(&PART_MOD_SOURCES);
    m.rebuild_dests_from_registry(&sound.dest_registry);
    m.load_amounts(&sound.mod_state);
    assert_eq!(m.present[0], DEFAULT_BITS);
    let mut ms = ModState::new();
    ms.sync_from_matrix(&m);
    assert_eq!(ms.present(0), DEFAULT_BITS);
}

/// Spec § Tests "Defaults": exactly the three CUTOFF routes at 0.
#[test]
fn a_new_sound_has_the_default_routes() {
    for ct in EngineType::ALL {
        let s = Sound::init(ct);
        assert_eq!(s.dest_registry.len(), 1, "{ct:?}");
        assert_eq!(s.dest_registry.get(0).unwrap().addr, CUTOFF_ADDR);
        assert_eq!(s.mod_state.num_sources(), MAX_MOD_SOURCES);
        assert_eq!(s.mod_state.num_dests(), 1);
        assert_eq!(s.mod_state.present(0), DEFAULT_BITS, "{ct:?}");
        assert!((0..MAX_MOD_SOURCES).all(|src| s.mod_state.amount(src, 0) == 0));
    }
}

use chimera_core::dsp::modulator::EnvSlot;
use chimera_core::params::EnvParams;

fn env(s: EnvSlot, id: chimera_core::block::ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::Env(s), id)
}

/// 24 blocks at note 72 under `routes` (source, destination, amount).
fn render_with(p: &ParamSnapshot, routes: &[(ModSource, ParamAddr, i8)], vel: u8) -> Vec<f32> {
    let mut reg = ModDestRegistry::new();
    for &(_, a, _) in routes {
        let _ = reg.add(a, *b"TEST\0\0\0\0");
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for &(s, a, amt) in routes {
        let d = ms.find(a).unwrap();
        ms.set_route(s.index(), d, amt);
    }
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(72).unwrap(), Velocity::new(vel).unwrap(), p);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..24 {
        v.render(&mut b, p, &ms);
        out.extend_from_slice(&b);
    }
    out
}

fn plain() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.cutoff = 1000.0;
    p
}

#[test]
fn env_destinations_are_modulatable() {
    for s in EnvSlot::ALL {
        for id in [
            EnvParams::LEVEL,
            EnvParams::TIME,
            EnvParams::RISE,
            EnvParams::FALL,
            EnvParams::SHAPE,
        ] {
            assert!(env(s, id).modulatable(), "{s:?} {id:?}");
        }
        for id in [
            EnvParams::ATTACK,
            EnvParams::SUSTAIN,
            EnvParams::TYPE,
            EnvParams::HOLD,
        ] {
            assert!(!env(s, id).modulatable(), "{s:?} {id:?}");
        }
    }
}

/// VEL → ENV 1 LEVEL at 100 %: ENV 1's peak is the velocity. A LEVEL
/// route at 0 makes the peak 0, so ENV 1 → CUTOFF then does nothing.
#[test]
fn vel_to_level_scales_env1() {
    let e1_cut = (ModSource::Env1, CUTOFF, 127);
    let level = |amt| (ModSource::Vel, env(EnvSlot::Env1, EnvParams::LEVEL), amt);
    // Against the same velocity without the LEVEL route, so the engine's
    // own velocity response cancels: at full velocity the peak is exactly 1
    // (1.0 × 127/127), a soft note lowers it.
    let no_level = |vel| render_with(&plain(), &[e1_cut], vel);
    assert_eq!(
        render_with(&plain(), &[e1_cut, level(127)], 127),
        no_level(127),
        "full velocity: peak 1"
    );
    assert_ne!(
        render_with(&plain(), &[e1_cut, level(127)], 30),
        no_level(30),
        "a soft note: a lower peak"
    );
    assert_eq!(
        render_with(&plain(), &[e1_cut, level(0)], 100),
        render_with(&plain(), &[(ModSource::Env1, CUTOFF, 0), level(0)], 100)
    );
}

/// TIME acts on A; RISE acts on B and not on A.
#[test]
fn time_and_rise_reach_their_slots() {
    let time = (ModSource::Vel, env(EnvSlot::Env1, EnvParams::TIME), 127);
    let e1 = (ModSource::Env1, CUTOFF, 127);
    assert_ne!(
        render_with(&plain(), &[e1, time], 100),
        render_with(&plain(), &[e1], 100)
    );
    let e3 = (ModSource::Env3, CUTOFF, 127); // ENV 3 is B (ENV, AD)
    let rise3 = (ModSource::Vel, env(EnvSlot::Env3, EnvParams::RISE), 127);
    assert_ne!(
        render_with(&plain(), &[e3, rise3], 100),
        render_with(&plain(), &[e3], 100)
    );
    let rise1 = (ModSource::Vel, env(EnvSlot::Env1, EnvParams::RISE), 127);
    assert_eq!(
        render_with(&plain(), &[e1, rise1], 100),
        render_with(&plain(), &[e1], 100)
    );
}
