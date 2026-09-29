//! Simulates the full desktop audio path:
//! UiState → ParamSnapshot → Voice → audio output
//!
//! This tests the same code path as the desktop simulator,
//! minus cpal and the actual AtomicPtr (which can't be tested
//! without threads). It verifies that UiState.params flows
//! correctly through Voice.render().
mod common;
use common::Rig;
use common::{SR, goertzel, rms, tri};

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::ui::UiState;
use chimera_core::{MidiNote, Velocity};

/// Simulate: create UiState, set engine, modify params, render through Voice.
fn sim_render(setup_ui: impl FnOnce(&mut UiState), note: u8, blocks: usize) -> Vec<f32> {
    let empty_mod = ModState::new();
    let mut ui = UiState::new();
    setup_ui(&mut ui);

    let mut voice = Rig::new(chimera_hal::SAMPLE_RATE);
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

// ── Algo through UiState ────────────────────────────────────────────

#[test]
fn test_desktop_algo_produces_sound() {
    let buf = sim_render(
        |ui| {
            *ui.params_mut() = tri();
        },
        60,
        8,
    );
    assert!(
        rms(&buf) > 0.01,
        "Algo through UiState should produce sound"
    );
}

#[test]
fn test_desktop_algo_modulation_works() {
    let clean = sim_render(
        |ui| {
            *ui.params_mut() = tri();
            // Default: modulators at 0
        },
        60,
        16,
    );

    let modulated = sim_render(
        |ui| {
            *ui.params_mut() = tri();
            ui.params_mut().algo.ops[1].level = 70;
        },
        60,
        16,
    );

    let f0 = 261.6;
    let h_clean: f32 = (2..=6).map(|h| goertzel(&clean, f0 * h as f32, SR)).sum();
    let h_mod: f32 = (2..=6)
        .map(|h| goertzel(&modulated, f0 * h as f32, SR))
        .sum();

    assert!(
        h_mod > h_clean,
        "Algo modulation through UiState: clean={} mod={}",
        h_clean,
        h_mod
    );
}

// ── KS+ String through UiState ─────────────────────────────────────

#[test]
fn test_desktop_string_produces_sound() {
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
fn test_desktop_string_body_changes_sound() {
    let no_body = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.body = 0.0;
        },
        60,
        16,
    );

    let with_body = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.body = 0.8;
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
fn test_desktop_string_structure_changes_sound() {
    let no_stiff = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.structure = 0.0;
        },
        60,
        16,
    );

    let with_stiff = sim_render(
        |ui| {
            *ui.params_mut() = ParamSnapshot::for_engine(EngineType::Modal);
            ui.params_mut().modal.mode = ResonatorMode::String;
            ui.params_mut().modal.structure = 0.7;
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
    assert!(diff > 0.001, "structure should change sound: diff={}", diff);
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
            *ui.params_mut() = tri();

            ui.params_mut().filter.cutoff = 15000.0;
        },
        60,
        16,
    );

    let closed = sim_render(
        |ui| {
            *ui.params_mut() = tri();

            ui.params_mut().filter.cutoff = 200.0;
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
            *ui.params_mut() = tri();
        },
        60,
        16,
    );

    let driven = sim_render(
        |ui| {
            *ui.params_mut() = tri();
            ui.params_mut().drive.drive = 0.9;
            ui.params_mut().drive.mix = 1.0;
        },
        60,
        16,
    );

    let f0 = 261.6;
    let h_clean: f32 = (2..=6).map(|h| goertzel(&clean, f0 * h as f32, SR)).sum();
    let h_driven: f32 = (2..=6).map(|h| goertzel(&driven, f0 * h as f32, SR)).sum();
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
    let algo = sim_render(
        |ui| {
            *ui.params_mut() = tri();
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
    assert!(rms(&algo) > 0.01, "Algo should produce sound");
    assert!(rms(&modal) > 0.001, "Modal should produce sound");
    assert!(rms(&ks) > 0.005, "KS should produce sound");

    // All three should be different from each other
    let algo_vs_modal: f32 = algo
        .iter()
        .zip(modal.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / algo.len() as f32;
    let algo_vs_ks: f32 = algo
        .iter()
        .zip(ks.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / algo.len() as f32;

    assert!(
        algo_vs_modal > 0.01,
        "Algo vs Modal should differ: {}",
        algo_vs_modal
    );
    assert!(
        algo_vs_ks > 0.01,
        "Algo vs KS should differ: {}",
        algo_vs_ks
    );
}

// ── Mid-note param change through UiState ───────────────────────────

#[test]
fn test_desktop_mid_note_filter_sweep() {
    let empty_mod = ModState::new();
    let mut ui = UiState::new();
    *ui.params_mut() = tri();

    ui.params_mut().filter.cutoff = 10000.0;

    let mut voice = Rig::new(chimera_hal::SAMPLE_RATE);
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
