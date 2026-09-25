//! Simulates the full desktop audio path:
//! UiState → ParamSnapshot → Voice → audio output
//!
//! This tests the same code path as the desktop simulator,
//! minus cpal and the actual AtomicPtr (which can't be tested
//! without threads). It verifies that UiState.params flows
//! correctly through Voice.render().

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::ui::UiState;
use chimera_core::{MidiNote, Velocity};

const SR: u32 = 48000;

/// Simulate: create UiState, set engine, modify params, render through Voice.
fn sim_render(setup_ui: impl FnOnce(&mut UiState), note: u8, blocks: usize) -> Vec<f32> {
    let empty_mod = ModState::new();
    let mut ui = UiState::new();
    setup_ui(&mut ui);

    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    // This is what the audio callback does: read params, note_on, render
    voice.note_on(
        MidiNote::new(note).unwrap(),
        Velocity::new(100).unwrap(),
        ui.params(),
    );

    let mut all = Vec::new();
    let mut block = [0.0f32; 64];
    for _ in 0..blocks {
        voice.render(&mut block, ui.params(), &empty_mod);
        all.extend_from_slice(&block);
    }
    all
}

fn rms(buf: &[f32]) -> f32 {
    libm::sqrtf(buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32)
}

fn goertzel(buf: &[f32], target_freq: f32) -> f32 {
    let n = buf.len() as f32;
    let k = (target_freq * n / SR as f32).round();
    let w = 2.0 * core::f32::consts::PI * k / n;
    let coeff = 2.0 * libm::cosf(w);
    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;
    for &sample in buf {
        let s0 = sample + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    libm::sqrtf((s1 * s1 + s2 * s2 - coeff * s1 * s2).abs()) / n
}

// ── FM through UiState ──────────────────────────────────────────────

#[test]
fn test_desktop_fm_produces_sound() {
    let buf = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);
        },
        60,
        8,
    );
    assert!(rms(&buf) > 0.01, "FM through UiState should produce sound");
}

#[test]
fn test_desktop_fm_modulation_works() {
    let clean = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);
            // Default: modulators at 0
        },
        60,
        16,
    );

    let modulated = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);
            ui.params_mut().pizza.crush = 0.7;
        },
        60,
        16,
    );

    let f0 = 261.6;
    let h_clean: f32 = (2..=6).map(|h| goertzel(&clean, f0 * h as f32)).sum();
    let h_mod: f32 = (2..=6).map(|h| goertzel(&modulated, f0 * h as f32)).sum();

    assert!(
        h_mod > h_clean,
        "FM modulation through UiState: clean={} mod={}",
        h_clean,
        h_mod
    );
}

// ── KS+ String through UiState ─────────────────────────────────────

#[test]
fn test_desktop_ks_produces_sound() {
    let buf = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
        },
        60,
        16,
    );
    assert!(
        rms(&buf) > 0.005,
        "KS+ through UiState should produce sound, rms={}",
        rms(&buf)
    );
}

#[test]
fn test_desktop_ks_body_resonance_changes_sound() {
    let no_body = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.ks_body = 0.0;
        },
        60,
        16,
    );

    let with_body = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.ks_body = 0.8;
        },
        60,
        16,
    );

    let diff: f32 = no_body
        .iter()
        .zip(with_body.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / no_body.len() as f32;
    assert!(
        diff > 0.001,
        "body resonance should change sound: diff={}",
        diff
    );
}

#[test]
fn test_desktop_ks_stiffness_changes_sound() {
    let no_stiff = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.ks_stiffness = 0.0;
        },
        60,
        16,
    );

    let with_stiff = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.ks_stiffness = 0.7;
        },
        60,
        16,
    );

    let diff: f32 = no_stiff
        .iter()
        .zip(with_stiff.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / no_stiff.len() as f32;
    assert!(diff > 0.001, "stiffness should change sound: diff={}", diff);
}

#[test]
fn test_desktop_ks_excitation_types_differ() {
    let noise = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.ks_excitation = 0;
        },
        60,
        8,
    );

    let click = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.ks_excitation = 1;
        },
        60,
        8,
    );

    let diff: f32 = noise
        .iter()
        .zip(click.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / noise.len() as f32;
    assert!(
        diff > 0.001,
        "different excitation types should sound different: diff={}",
        diff
    );
}

// ── Modal through UiState ───────────────────────────────────────────

#[test]
fn test_desktop_modal_produces_sound() {
    let buf = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::Modal;
        },
        60,
        16,
    );
    assert!(
        rms(&buf) > 0.001,
        "Modal through UiState should produce sound, rms={}",
        rms(&buf)
    );
}

// ── Signal chain through UiState ────────────────────────────────────

#[test]
fn test_desktop_filter_affects_output() {
    let open = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);

            ui.params_mut().filter.cutoff = 15000.0;
        },
        60,
        16,
    );

    let closed = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);

            ui.params_mut().filter.cutoff = 200.0;
            ui.params_mut().filter.mode = 2;
        },
        60,
        16,
    );

    assert!(
        rms(&open) > rms(&closed) * 1.5,
        "filter should reduce level: open={} closed={}",
        rms(&open),
        rms(&closed)
    );
}

#[test]
fn test_desktop_drive_affects_output() {
    let clean = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);
        },
        60,
        16,
    );

    let driven = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);
            ui.params_mut().drive.drive = 0.9;
            ui.params_mut().drive.mix = 1.0;
        },
        60,
        16,
    );

    let f0 = 261.6;
    let h_clean: f32 = (2..=6).map(|h| goertzel(&clean, f0 * h as f32)).sum();
    let h_driven: f32 = (2..=6).map(|h| goertzel(&driven, f0 * h as f32)).sum();
    assert!(
        h_driven > h_clean,
        "drive should add harmonics: clean={} driven={}",
        h_clean,
        h_driven
    );
}

// ── Engine switching through UiState ────────────────────────────────

#[test]
fn test_desktop_engine_switch() {
    let fm = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);
        },
        60,
        16,
    );

    let modal = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::Modal;
        },
        60,
        16,
    );

    let ks = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
        },
        60,
        16,
    );

    // All three should produce sound
    assert!(rms(&fm) > 0.01, "FM should produce sound");
    assert!(rms(&modal) > 0.001, "Modal should produce sound");
    assert!(rms(&ks) > 0.005, "KS should produce sound");

    // All three should be different from each other
    let fm_vs_modal: f32 = fm
        .iter()
        .zip(modal.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / fm.len() as f32;
    let fm_vs_ks: f32 = fm
        .iter()
        .zip(ks.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / fm.len() as f32;

    assert!(
        fm_vs_modal > 0.01,
        "FM vs Modal should differ: {}",
        fm_vs_modal
    );
    assert!(fm_vs_ks > 0.01, "FM vs KS should differ: {}", fm_vs_ks);
}

// ── Mid-note param change through UiState ───────────────────────────

#[test]
fn test_desktop_mid_note_filter_sweep() {
    let empty_mod = ModState::new();
    let mut ui = UiState::new();
    *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Pizza);

    ui.params_mut().filter.cutoff = 10000.0;
    ui.params_mut().filter.mode = 2;

    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        ui.params(),
    );

    // Render with open filter
    let mut block = [0.0f32; 64];
    let mut before_energy = 0.0f32;
    for _ in 0..16 {
        voice.render(&mut block, ui.params(), &empty_mod);
        before_energy += block.iter().map(|s| s * s).sum::<f32>();
    }

    // Close the filter mid-note
    ui.params_mut().filter.cutoff = 200.0;

    let mut after_energy = 0.0f32;
    for _ in 0..16 {
        voice.render(&mut block, ui.params(), &empty_mod);
        after_energy += block.iter().map(|s| s * s).sum::<f32>();
    }

    assert!(
        after_energy < before_energy * 0.4,
        "closing filter mid-note should reduce energy: before={} after={}",
        before_energy,
        after_energy
    );
}
