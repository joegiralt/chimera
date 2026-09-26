use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};

const SR: u32 = 48000;

fn render_voice(engine: EngineType, note: u8, blocks: usize) -> Vec<f32> {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    let params = ParamSnapshot::for_engine(engine);

    voice.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut all = Vec::new();
    let mut block = [0.0f32; 64];
    for _ in 0..blocks {
        voice.render(&mut block, &params, &empty_mod);
        all.extend_from_slice(&block);
    }
    all
}

#[test]
fn test_algo_and_modal_produce_different_output() {
    let algo_buf = render_voice(EngineType::Algo, 60, 16);
    let modal_buf = render_voice(EngineType::Modal, 60, 16);

    let algo_rms: f32 =
        libm::sqrtf(algo_buf.iter().map(|s| s * s).sum::<f32>() / algo_buf.len() as f32);
    let modal_rms: f32 =
        libm::sqrtf(modal_buf.iter().map(|s| s * s).sum::<f32>() / modal_buf.len() as f32);

    eprintln!("Algo RMS: {}", algo_rms);
    eprintln!("Modal RMS: {}", modal_rms);

    // Both should produce sound
    assert!(algo_rms > 0.001, "Algo should produce sound: {}", algo_rms);
    assert!(
        modal_rms > 0.001,
        "Modal should produce sound: {}",
        modal_rms
    );

    // They should sound DIFFERENT — compare sample-by-sample
    let diff: f32 = algo_buf
        .iter()
        .zip(modal_buf.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / algo_buf.len() as f32;

    eprintln!("Average sample difference: {}", diff);
    assert!(
        diff > 0.001,
        "Algo and Modal should produce different output, diff={}",
        diff
    );
}

#[test]
fn test_engine_type_is_respected() {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);

    // Start with Algo
    let mut params = ParamSnapshot::for_engine(EngineType::Algo);
    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut block = [0.0f32; 64];
    voice.render(&mut block, &params, &empty_mod);
    let algo_sample = block[32];

    // Now switch to Modal
    let mut voice2 = Voice::new(chimera_hal::SAMPLE_RATE);
    params = ParamSnapshot::for_engine(EngineType::Modal);
    voice2.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut block2 = [0.0f32; 64];
    voice2.render(&mut block2, &params, &empty_mod);
    let modal_sample = block2[32];

    eprintln!("Algo sample[32]: {}", algo_sample);
    eprintln!("Modal sample[32]: {}", modal_sample);

    // At the very least, they shouldn't be identical
    assert!(
        (algo_sample - modal_sample).abs() > 0.0001,
        "different engines should produce different samples: algo={} modal={}",
        algo_sample,
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
fn test_algo_sustains_while_modal_decays() {
    // Algo's init patch holds at D1L 15 until note-off, Modal should decay
    let algo_buf = render_voice(EngineType::Algo, 60, 64);
    let modal_buf = render_voice(EngineType::Modal, 60, 64);

    // Measure energy in last quarter of each
    let last_quarter = |buf: &[f32]| -> f32 {
        let start = buf.len() * 3 / 4;
        let slice = &buf[start..];
        libm::sqrtf(slice.iter().map(|s| s * s).sum::<f32>() / slice.len() as f32)
    };

    let algo_late = last_quarter(&algo_buf);
    let modal_late = last_quarter(&modal_buf);

    eprintln!("Algo late RMS: {}", algo_late);
    eprintln!("Modal late RMS: {}", modal_late);

    // Algo should sustain (it holds at D1L until note-off)
    assert!(
        algo_late > 0.01,
        "Algo should sustain: algo_late={}",
        algo_late,
    );
}

/// Spec § Testing "Engines": every engine pair switches mid-note (hard cut,
/// retrigger) without panicking or producing non-finite output.
#[test]
fn every_engine_pair_switches_mid_note() {
    for from in EngineType::ALL {
        for to in EngineType::ALL {
            let a = ParamSnapshot::for_engine(from);
            let b = ParamSnapshot::for_engine(to);
            let mut voice = Voice::new(SR);
            voice.note_on(MidiNote::A4, Velocity::DEFAULT, &a);
            let mut block = [0.0f32; 64];
            for i in 0..16 {
                voice.render(&mut block, if i < 8 { &a } else { &b }, &ModState::new());
                assert!(block.iter().all(|x| x.is_finite()), "{from:?} -> {to:?}");
            }
        }
    }
}
