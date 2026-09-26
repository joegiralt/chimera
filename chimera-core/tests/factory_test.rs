//! Spec § Replacing the old engines: a bank of eight Algo Sounds that plays.

use chimera_core::dsp::voice::Voice;
use chimera_core::factory::{FACTORY_LEN, factory_sound, load_factory};
use chimera_core::preset::{ChainType, SoundPool};
use chimera_core::ui::UiState;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// One second held, then three seconds of release.
fn play(i: usize) -> (Vec<f32>, Vec<f32>) {
    let s = factory_sound(i).unwrap();
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &s.params,
    );
    let (mut held, mut tail) = (Vec::new(), Vec::new());
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..750 {
        v.render(&mut b, &s.params, &s.mod_state);
        held.extend_from_slice(&b);
    }
    v.note_off();
    for _ in 0..2250 {
        v.render(&mut b, &s.params, &s.mod_state);
        tail.extend_from_slice(&b);
    }
    (held, tail)
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

#[test]
fn eight_algo_sounds_with_distinct_names() {
    let names: Vec<String> = (0..FACTORY_LEN)
        .map(|i| {
            let s = factory_sound(i).unwrap();
            assert_eq!(s.chain_type, ChainType::Algo);
            s.name_str().to_string()
        })
        .collect();
    for (i, n) in names.iter().enumerate() {
        assert!(!n.is_empty() && !names[..i].contains(n), "{n}");
    }
    assert!(factory_sound(FACTORY_LEN).is_none());
}

#[test]
fn every_factory_sound_is_audible_finite_bounded_and_ends() {
    for i in 0..FACTORY_LEN {
        let name = factory_sound(i).unwrap().name_str().to_string();
        let (held, tail) = play(i);
        assert!(held.iter().chain(&tail).all(|x| x.is_finite()), "{name}");
        assert!(
            peak(&held) > 1e-2 && peak(&held) <= 1.0,
            "{name}: peak {}",
            peak(&held)
        );
        assert!(
            peak(&tail[tail.len() - 10 * BLOCK_SIZE..]) < 1e-4,
            "{name} rings on"
        );
    }
}

#[test]
fn the_morph_showcases_morph() {
    for i in [6, 7] {
        let s = factory_sound(i).unwrap();
        assert_ne!(s.params.algo.alg_a, s.params.algo.alg_b, "{}", s.name_str());
    }
    let pad = factory_sound(6).unwrap();
    assert_eq!(pad.mod_state.num_dests(), 1);
    assert_ne!(pad.mod_state.amount(1, 0), 0, "the LFO moves MORPH");
}

#[test]
fn the_ui_starts_with_the_bank_in_the_pool() {
    let ui = UiState::new();
    for i in 0..FACTORY_LEN {
        let want = factory_sound(i).unwrap();
        assert_eq!(ui.pool.get(i).unwrap().name_str(), want.name_str());
    }
    assert!(ui.pool.get(FACTORY_LEN).is_none());
    let mut pool = SoundPool::new();
    load_factory(&mut pool);
    assert!(pool.get(FACTORY_LEN - 1).is_some() && pool.get(FACTORY_LEN).is_none());
}
