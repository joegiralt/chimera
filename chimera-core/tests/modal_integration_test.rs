use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::modulation::ModState;
use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};

const SR: u32 = 48000;

/// Simulate exactly what the desktop runtime does:
/// 1. Set engine type in params
/// 2. Create voice
/// 3. note_on with those params
/// 4. Render blocks
/// 5. Check output
#[test]
fn test_modal_through_voice_produces_sound() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Modal;

    // Verify engine type is set
    assert_eq!(params.engine, EngineType::Modal);

    voice.note_on(60, 100, &params, SR);

    let mut output = [0.0f32; 64];
    let mut total_max = 0.0f32;

    for i in 0..16 {
        voice.render(&mut output, &params, &empty_mod, SR);
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
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Modal;
    params.modal.mode = ResonatorMode::Modal; // String mode

    voice.note_on(60, 100, &params, SR);

    let mut output = [0.0f32; 64];
    let mut total_max = 0.0f32;

    for i in 0..16 {
        voice.render(&mut output, &params, &empty_mod, SR);
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
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Modal;
    params.modal.mode = ResonatorMode::Bowed; // Bowed mode

    voice.note_on(60, 100, &params, SR);

    let mut output = [0.0f32; 64];
    let mut total_max = 0.0f32;

    for i in 0..16 {
        voice.render(&mut output, &params, &empty_mod, SR);
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
fn test_modal_different_from_pizza_through_voice() {
    let empty_mod = ModState::new();
    let render = |engine: EngineType| -> Vec<f32> {
        let mut voice = Voice::new();
        let mut params = ParamSnapshot::default();
        params.engine = engine;
        voice.note_on(60, 100, &params, SR);
        let mut all = Vec::new();
        let mut block = [0.0f32; 64];
        for _ in 0..16 {
            voice.render(&mut block, &params, &empty_mod, SR);
            all.extend_from_slice(&block);
        }
        all
    };

    let pizza = render(EngineType::Pizza);
    let modal = render(EngineType::Modal);

    let pizza_max = pizza.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let modal_max = modal.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

    eprintln!("Pizza max: {}", pizza_max);
    eprintln!("Modal max: {}", modal_max);

    // Both should produce sound
    assert!(pizza_max > 0.001, "Pizza should produce sound");
    assert!(modal_max > 0.001, "Modal should produce sound");

    // They should be different
    let diff: f32 = pizza
        .iter()
        .zip(modal.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / pizza.len() as f32;

    eprintln!("Avg difference: {}", diff);
    assert!(diff > 0.001, "Pizza and Modal should differ, diff={}", diff);
}

#[test]
fn test_modal_signal_chain_affects_output() {
    let empty_mod = ModState::new();
    // Modal through filter should be different from modal without filter
    let mut voice_open = Voice::new();
    let mut voice_closed = Voice::new();

    let mut params_open = ParamSnapshot::default();
    params_open.engine = EngineType::Modal;
    params_open.filter.cutoff = 20000.0;

    let mut params_closed = ParamSnapshot::default();
    params_closed.engine = EngineType::Modal;
    params_closed.filter.cutoff = 200.0;
    params_closed.filter.mode = 2; // LP4

    voice_open.note_on(60, 100, &params_open, SR);
    voice_closed.note_on(60, 100, &params_closed, SR);

    let mut out_open = [0.0f32; 64];
    let mut out_closed = [0.0f32; 64];

    for _ in 0..8 {
        voice_open.render(&mut out_open, &params_open, &empty_mod, SR);
        voice_closed.render(&mut out_closed, &params_closed, &empty_mod, SR);
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
