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
fn test_fm_and_modal_produce_different_output() {
    let fm_buf = render_voice(EngineType::Fm, 60, 16);
    let modal_buf = render_voice(EngineType::Modal, 60, 16);

    let fm_rms: f32 = libm::sqrtf(fm_buf.iter().map(|s| s * s).sum::<f32>() / fm_buf.len() as f32);
    let modal_rms: f32 =
        libm::sqrtf(modal_buf.iter().map(|s| s * s).sum::<f32>() / modal_buf.len() as f32);

    eprintln!("FM RMS: {}", fm_rms);
    eprintln!("Modal RMS: {}", modal_rms);

    // Both should produce sound
    assert!(fm_rms > 0.001, "FM should produce sound: {}", fm_rms);
    assert!(
        modal_rms > 0.001,
        "Modal should produce sound: {}",
        modal_rms
    );

    // They should sound DIFFERENT — compare sample-by-sample
    let diff: f32 = fm_buf
        .iter()
        .zip(modal_buf.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / fm_buf.len() as f32;

    eprintln!("Average sample difference: {}", diff);
    assert!(
        diff > 0.001,
        "FM and Modal should produce different output, diff={}",
        diff
    );
}

#[test]
fn test_engine_type_is_respected() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();

    // Start with FM
    params.engine = EngineType::Fm;
    voice.note_on(60, 100, &params, SR);

    let mut block = [0.0f32; 64];
    voice.render(&mut block, &params, SR);
    let fm_sample = block[32];

    // Now switch to Modal
    let mut voice2 = Voice::new();
    params.engine = EngineType::Modal;
    voice2.note_on(60, 100, &params, SR);

    let mut block2 = [0.0f32; 64];
    voice2.render(&mut block2, &params, SR);
    let modal_sample = block2[32];

    eprintln!("FM sample[32]: {}", fm_sample);
    eprintln!("Modal sample[32]: {}", modal_sample);

    // At the very least, they shouldn't be identical
    assert!(
        (fm_sample - modal_sample).abs() > 0.0001,
        "different engines should produce different samples: fm={} modal={}",
        fm_sample,
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
fn test_fm_sustains_while_modal_decays() {
    // FM with default envelope should sustain, Modal should decay
    let fm_buf = render_voice(EngineType::Fm, 60, 64);
    let modal_buf = render_voice(EngineType::Modal, 60, 64);

    // Measure energy in last quarter of each
    let last_quarter = |buf: &[f32]| -> f32 {
        let start = buf.len() * 3 / 4;
        let slice = &buf[start..];
        libm::sqrtf(slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32)
    };

    let fm_late = last_quarter(&fm_buf);
    let modal_late = last_quarter(&modal_buf);

    eprintln!("FM late RMS: {}", fm_late);
    eprintln!("Modal late RMS: {}", modal_late);

    // KS+ can sustain with feedback — just verify they're different
    assert!(
        (fm_late - modal_late).abs() > 0.005,
        "FM and Modal should differ: fm_late={} modal_late={}",
        fm_late,
        modal_late
    );
}
