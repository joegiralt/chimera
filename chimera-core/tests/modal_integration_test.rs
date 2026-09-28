mod common;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use common::SR;

/// Simulate exactly what the desktop runtime does:
/// 1. Set engine type in params
/// 2. Create voice
/// 3. note_on with those params
/// 4. Render blocks
/// 5. Check output
#[test]
fn test_modal_through_voice_produces_sound() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(SR);
    let params = ParamSnapshot::for_engine(EngineType::Modal);

    // Verify engine type is set
    assert_eq!(params.engine(), EngineType::Modal);

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut output = [0.0f32; 64];
    let mut total_max = 0.0f32;

    for i in 0..16 {
        voice.render(&mut output, &params, &empty_mod);
        let block_max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        total_max = total_max.max(block_max);
        eprintln!(
            "Block {}: max={:.6}, active={}",
            i,
            block_max,
            voice.is_active()
        );
    }

    assert!(
        total_max > 0.001,
        "modal through voice should produce sound, max={}",
        total_max
    );
}

#[test]
fn test_modal_string_through_voice() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(SR);
    let mut params = ParamSnapshot::for_engine(EngineType::Modal);
    params.modal.mode = ResonatorMode::Modal;

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut output = [0.0f32; 64];
    let mut total_max = 0.0f32;

    for i in 0..16 {
        voice.render(&mut output, &params, &empty_mod);
        let block_max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        total_max = total_max.max(block_max);
        eprintln!("String block {}: max={:.6}", i, block_max);
    }

    assert!(
        total_max > 0.001,
        "string through voice should produce sound, max={}",
        total_max
    );
}

#[test]
fn test_modal_bowed_through_voice() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(SR);
    let mut params = ParamSnapshot::for_engine(EngineType::Modal);
    params.modal.mode = ResonatorMode::Bowed; // Bowed mode

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut output = [0.0f32; 64];
    let mut total_max = 0.0f32;

    for i in 0..16 {
        voice.render(&mut output, &params, &empty_mod);
        let block_max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        total_max = total_max.max(block_max);
        eprintln!("Bowed block {}: max={:.6}", i, block_max);
    }

    assert!(
        total_max > 0.001,
        "bowed through voice should produce sound, max={}",
        total_max
    );
}

#[test]
fn test_modal_different_from_algo_through_voice() {
    let empty_mod = ModState::new();
    let render = |engine: EngineType| -> Vec<f32> {
        let mut voice = Voice::new(SR);
        let params = ParamSnapshot::for_engine(engine);
        voice.note_on(
            MidiNote::new(60).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );
        let mut all = Vec::new();
        let mut block = [0.0f32; 64];
        for _ in 0..16 {
            voice.render(&mut block, &params, &empty_mod);
            all.extend_from_slice(&block);
        }
        all
    };

    let algo = render(EngineType::Algo);
    let modal = render(EngineType::Modal);

    let algo_max = algo.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let modal_max = modal.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

    eprintln!("Algo max: {}", algo_max);
    eprintln!("Modal max: {}", modal_max);

    // Both should produce sound
    assert!(algo_max > 0.001, "Algo should produce sound");
    assert!(modal_max > 0.001, "Modal should produce sound");

    // They should be different
    let diff: f32 = algo
        .iter()
        .zip(modal.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / algo.len() as f32;

    eprintln!("Avg difference: {}", diff);
    assert!(diff > 0.001, "Algo and Modal should differ, diff={}", diff);
}

#[test]
fn test_modal_signal_chain_affects_output() {
    let empty_mod = ModState::new();
    // Modal through filter should be different from modal without filter
    let mut voice_open = Voice::new(SR);
    let mut voice_closed = Voice::new(SR);

    let mut params_open = ParamSnapshot::for_engine(EngineType::Modal);
    params_open.filter.cutoff = 20000.0;

    let mut params_closed = ParamSnapshot::for_engine(EngineType::Modal);
    params_closed.filter.cutoff = 200.0;
    params_closed.filter.mode = 2; // LP4

    voice_open.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params_open,
    );
    voice_closed.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params_closed,
    );

    let mut out_open = [0.0f32; 64];
    let mut out_closed = [0.0f32; 64];

    for _ in 0..8 {
        voice_open.render(&mut out_open, &params_open, &empty_mod);
        voice_closed.render(&mut out_closed, &params_closed, &empty_mod);
    }

    let energy_open: f32 = out_open.iter().map(|s| s * s).sum();
    let energy_closed: f32 = out_closed.iter().map(|s| s * s).sum();

    eprintln!("Modal open filter energy: {}", energy_open);
    eprintln!("Modal closed filter energy: {}", energy_closed);

    // Closed filter should reduce energy
    assert!(
        energy_open > energy_closed || energy_open < 0.0001,
        "filter should affect modal: open={} closed={}",
        energy_open,
        energy_closed
    );
}
