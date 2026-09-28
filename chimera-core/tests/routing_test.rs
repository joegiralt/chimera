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
