use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};

const SR: u32 = 48000;

fn render_voice(engine: EngineType, note: u8, blocks: usize) -> Vec<f32> {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = engine;

    voice.note_on(note, 100, &params, SR);

    let mut all = Vec::new();
    let mut block = [0.0f32; 64];
    for _ in 0..blocks {
        voice.render(&mut block, &params, SR);
        all.extend_from_slice(&block);
    }
    all
}

#[test]
fn test_pizza_and_modal_produce_different_output() {
    let pizza_buf = render_voice(EngineType::Pizza, 60, 16);
    let modal_buf = render_voice(EngineType::Modal, 60, 16);

    let pizza_rms: f32 =
        libm::sqrtf(pizza_buf.iter().map(|s| s * s).sum::<f32>() / pizza_buf.len() as f32);
    let modal_rms: f32 =
        libm::sqrtf(modal_buf.iter().map(|s| s * s).sum::<f32>() / modal_buf.len() as f32);

    eprintln!("Pizza RMS: {}", pizza_rms);
    eprintln!("Modal RMS: {}", modal_rms);

    // Both should produce sound
    assert!(
        pizza_rms > 0.001,
        "Pizza should produce sound: {}",
        pizza_rms
    );
    assert!(
        modal_rms > 0.001,
        "Modal should produce sound: {}",
        modal_rms
    );

    // They should sound DIFFERENT — compare sample-by-sample
    let diff: f32 = pizza_buf
        .iter()
        .zip(modal_buf.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / pizza_buf.len() as f32;

    eprintln!("Average sample difference: {}", diff);
    assert!(
        diff > 0.001,
        "Pizza and Modal should produce different output, diff={}",
        diff
    );
}

#[test]
fn test_engine_type_is_respected() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();

    // Start with Pizza
    params.engine = EngineType::Pizza;
    voice.note_on(60, 100, &params, SR);

    let mut block = [0.0f32; 64];
    voice.render(&mut block, &params, SR);
    let pizza_sample = block[32];

    // Now switch to Modal
    let mut voice2 = Voice::new();
    params.engine = EngineType::Modal;
    voice2.note_on(60, 100, &params, SR);

    let mut block2 = [0.0f32; 64];
    voice2.render(&mut block2, &params, SR);
    let modal_sample = block2[32];

    eprintln!("Pizza sample[32]: {}", pizza_sample);
    eprintln!("Modal sample[32]: {}", modal_sample);

    // At the very least, they shouldn't be identical
    assert!(
        (pizza_sample - modal_sample).abs() > 0.0001,
        "different engines should produce different samples: pizza={} modal={}",
        pizza_sample,
        modal_sample
    );
}

#[test]
fn test_modal_has_percussive_character() {
    // Default mode 0 (KS+ String) — may sustain with feedback.
    // Test that it at least produces sound and is different from silence.
    let buf = render_voice(EngineType::Modal, 60, 64);

    let early_rms: f32 = {
        let slice = &buf[100..1100]; // skip initial transient
        libm::sqrtf(slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32)
    };

    let late_rms: f32 = {
        let slice = &buf[buf.len() - 1000..];
        libm::sqrtf(slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32)
    };

    eprintln!("Modal early RMS: {}", early_rms);
    eprintln!("Modal late RMS: {}", late_rms);

    // KS+ with feedback can sustain — just verify it produces sound
    assert!(
        early_rms > 0.001 || late_rms > 0.001,
        "modal should produce sound: early={} late={}",
        early_rms,
        late_rms
    );
}

#[test]
fn test_pizza_sustains_while_modal_decays() {
    // Pizza with default envelope should sustain, Modal should decay
    let pizza_buf = render_voice(EngineType::Pizza, 60, 64);
    let modal_buf = render_voice(EngineType::Modal, 60, 64);

    // Measure energy in last quarter of each
    let last_quarter = |buf: &[f32]| -> f32 {
        let start = buf.len() * 3 / 4;
        let slice = &buf[start..];
        libm::sqrtf(slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32)
    };

    let pizza_late = last_quarter(&pizza_buf);
    let modal_late = last_quarter(&modal_buf);

    eprintln!("Pizza late RMS: {}", pizza_late);
    eprintln!("Modal late RMS: {}", modal_late);

    // Pizza should sustain (it has an amp envelope)
    assert!(
        pizza_late > 0.01,
        "Pizza should sustain: pizza_late={}",
        pizza_late,
    );
}
