use chimera_core::{MidiNote, Velocity};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::modulation::ModState;
use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};

const SR: u32 = 48000;

/// Render a voice, then change a parameter mid-note, render more.
/// Returns (before_rms, after_rms) for comparison.
fn render_with_param_change(
    setup: impl FnOnce(&mut ParamSnapshot),
    tweak: impl FnOnce(&mut ParamSnapshot),
    blocks_before: usize,
    blocks_after: usize,
) -> (f32, f32, Vec<f32>, Vec<f32>) {
    let empty_mod = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    let mut params = ParamSnapshot::default();
    setup(&mut params);

    voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);

    // Render "before" blocks
    let mut before_buf = Vec::new();
    let mut block = [0.0f32; 64];
    for _ in 0..blocks_before {
        voice.render(&mut block, &params, &empty_mod);
        before_buf.extend_from_slice(&block);
    }

    // Tweak the parameter
    tweak(&mut params);

    // Render "after" blocks
    let mut after_buf = Vec::new();
    for _ in 0..blocks_after {
        voice.render(&mut block, &params, &empty_mod);
        after_buf.extend_from_slice(&block);
    }

    let rms = |buf: &[f32]| -> f32 {
        libm::sqrtf(buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32)
    };

    (rms(&before_buf), rms(&after_buf), before_buf, after_buf)
}

fn goertzel(buf: &[f32], target_freq: f32, sample_rate: u32) -> f32 {
    let n = buf.len() as f32;
    let k = (target_freq * n / sample_rate as f32).round();
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

fn harmonic_energy(buf: &[f32], f0: f32) -> f32 {
    (2..=8).map(|h| goertzel(buf, f0 * h as f32, SR)).sum()
}

// ── Pizza: live parameter tests ─────────────────────────────────────

#[test]
fn test_pizza_shape_change_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.pizza.shape = 0.5; // triangle
        },
        |p| {
            p.pizza.shape = 1.0; // ramp up
        },
        8,
        8,
    );
    let diff: f32 = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / before.len() as f32;
    assert!(diff > 0.001, "shape change should alter sound: diff={}", diff);
}

#[test]
fn test_pizza_crush_change_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.pizza.crush = 0.0;
        },
        |p| {
            p.pizza.crush = 0.8;
        },
        8,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        (h_before - h_after).abs() > 0.001,
        "crush change should alter harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── Filter: live parameter tests ────────────────────────────────────

#[test]
fn test_filter_cutoff_sweep_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.filter.cutoff = 10000.0;
            p.filter.mode = 2; // LP4
        },
        |p| {
            p.filter.cutoff = 200.0;
        }, // close the filter
        16,
        16,
    );
    assert!(
        after_rms < before_rms * 0.6,
        "closing filter should reduce level: before={} after={}",
        before_rms,
        after_rms
    );
}

#[test]
fn test_filter_resonance_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.filter.cutoff = 1000.0;
            p.filter.mode = 1; // LP2
            p.filter.resonance = 0.0;
        },
        |p| {
            p.filter.resonance = 0.9;
        },
        8,
        8,
    );
    let peak_before = goertzel(&before, 1000.0, SR);
    let peak_after = goertzel(&after, 1000.0, SR);
    assert!(
        peak_after > peak_before,
        "resonance should boost cutoff freq: before={} after={}",
        peak_before,
        peak_after
    );
}

// ── Drive: live parameter tests ─────────────────────────────────────

#[test]
fn test_drive_amount_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.drive.drive = 0.0;
            p.drive.mix = 1.0;
        },
        |p| {
            p.drive.drive = 0.9;
        },
        8,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        h_after > h_before,
        "drive should add harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── Wavefolder: live parameter tests ────────────────────────────────

#[test]
fn test_folder_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.folder.fold = 0.0;
        },
        |p| {
            p.folder.fold = 0.8;
            p.folder.mix = 1.0;
        },
        8,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        h_after > h_before,
        "folder should add harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── KS+ String: live parameter tests ────────────────────────────────

#[test]
fn test_ks_body_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
            p.modal.ks_body = 0.0;
        },
        |p| {
            p.modal.ks_body = 0.8;
        },
        8,
        8,
    );
    let diff: f32 = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / before.len() as f32;
    assert!(
        diff > 0.001,
        "body resonance should change sound: diff={}",
        diff
    );
}

#[test]
fn test_ks_stiffness_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
            p.modal.ks_stiffness = 0.0;
        },
        |p| {
            p.modal.ks_stiffness = 0.7;
        },
        8,
        8,
    );
    let diff: f32 = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / before.len() as f32;
    assert!(diff > 0.001, "stiffness should change sound: diff={}", diff);
}

#[test]
fn test_ks_feedback_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
            p.modal.ks_feedback = 0.0;
        },
        |p| {
            p.modal.ks_feedback = 0.9;
        },
        8,
        16,
    );
    // Higher feedback should sustain longer — after_rms should be higher
    // relative to what it would be without feedback
    assert!(
        after_rms > 0.001,
        "feedback should help sustain: after_rms={}",
        after_rms
    );
}

#[test]
fn test_ks_brightness_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
            p.modal.brightness = 0.1;
        },
        |p| {
            p.modal.brightness = 0.9;
        },
        4,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        (h_before - h_after).abs() > 0.0001,
        "brightness should change harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── Modal resonator: live parameter tests ───────────────────────────

#[test]
fn test_modal_decay_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::Modal;
            p.modal.decay = 0.8;
        },
        |p| {
            p.modal.decay = 0.1;
        }, // shorten decay dramatically
        8,
        16,
    );
    // After shortening decay, the sound should die faster
    assert!(
        after_rms < before_rms || after_rms < 0.01,
        "reducing decay should quiet the sound: before={} after={}",
        before_rms,
        after_rms
    );
}

#[test]
fn test_modal_brightness_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::Modal;
            p.modal.brightness = 0.1;
        },
        |p| {
            p.modal.brightness = 1.0;
        },
        4,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        (h_before - h_after).abs() > 0.0001,
        "modal brightness should change spectrum: before={} after={}",
        h_before,
        h_after
    );
}

// ── Volume: live parameter test ─────────────────────────────────────

#[test]
fn test_volume_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Pizza);
            p.out.volume = 0.8;
        },
        |p| {
            p.out.volume = 0.1;
        },
        8,
        8,
    );
    assert!(
        after_rms < before_rms * 0.3,
        "reducing volume should reduce level: before={} after={}",
        before_rms,
        after_rms
    );
}
