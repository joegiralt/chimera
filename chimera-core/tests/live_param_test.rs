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
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    setup(&mut params);

    voice.note_on(60, 100, &params, SR);

    // Render "before" blocks
    let mut before_buf = Vec::new();
    let mut block = [0.0f32; 128];
    for _ in 0..blocks_before {
        voice.render(&mut block, &params, SR);
        before_buf.extend_from_slice(&block);
    }

    // Tweak the parameter
    tweak(&mut params);

    // Render "after" blocks
    let mut after_buf = Vec::new();
    for _ in 0..blocks_after {
        voice.render(&mut block, &params, SR);
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

// ── FM: live parameter tests ────────────────────────────────────────

#[test]
fn test_fm_algo_change_mid_note() {
    let (before_rms, after_rms, before, after) = render_with_param_change(
        |p| {
            p.engine = EngineType::Fm;
            p.fm.op_level = [0.5, 0.3, 0.3, 1.0];
        },
        |p| {
            p.fm.algorithm = 7;
        }, // switch to additive
        8,
        8,
    );
    let diff: f32 = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / before.len() as f32;
    assert!(diff > 0.01, "algo change should alter sound: diff={}", diff);
}

#[test]
fn test_fm_feedback_change_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            p.engine = EngineType::Fm;
        },
        |p| {
            p.fm.feedback = 0.8;
        },
        8,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        (h_before - h_after).abs() > 0.001,
        "feedback change should alter harmonics: before={} after={}",
        h_before,
        h_after
    );
}

#[test]
fn test_fm_modulator_depth_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            p.engine = EngineType::Fm;
            p.fm.algorithm = 4;
            p.fm.op_level[1] = 1.0;
        },
        |p| {
            p.fm.op_level[0] = 0.8;
        }, // crank modulator
        8,
        8,
    );
    let f0 = 261.6;
    let h_before = harmonic_energy(&before, f0);
    let h_after = harmonic_energy(&after, f0);
    assert!(
        h_after > h_before,
        "increasing modulator should add harmonics: before={} after={}",
        h_before,
        h_after
    );
}

// ── Filter: live parameter tests ────────────────────────────────────

#[test]
fn test_filter_cutoff_sweep_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| {
            p.engine = EngineType::Fm;
            p.fm.op_level = [0.5, 0.0, 0.0, 1.0];
            p.filter.cutoff.set(10000.0);
            p.filter.mode = 2; // LP4
        },
        |p| {
            p.filter.cutoff.set(200.0);
        }, // close the filter
        8,
        8,
    );
    assert!(
        after_rms < before_rms * 0.5,
        "closing filter should reduce level: before={} after={}",
        before_rms,
        after_rms
    );
}

#[test]
fn test_filter_resonance_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| {
            p.engine = EngineType::Fm;
            p.fm.op_level = [0.5, 0.0, 0.0, 1.0];
            p.filter.cutoff.set(1000.0);
            p.filter.mode = 1; // LP2
            p.filter.resonance.set(0.0);
        },
        |p| {
            p.filter.resonance.set(0.9);
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
            p.engine = EngineType::Fm;
            p.drive.drive.set(0.0);
            p.drive.mix.set(1.0);
        },
        |p| {
            p.drive.drive.set(0.9);
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
            p.engine = EngineType::Fm;
            p.folder.fold.set(0.0);
        },
        |p| {
            p.folder.fold.set(0.8);
            p.folder.mix.set(1.0);
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
            p.engine = EngineType::Modal;
            p.modal.mode = 0;
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
            p.engine = EngineType::Modal;
            p.modal.mode = 0;
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
            p.engine = EngineType::Modal;
            p.modal.mode = 0;
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
            p.engine = EngineType::Modal;
            p.modal.mode = 0;
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
            p.engine = EngineType::Modal;
            p.modal.mode = 1;
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
            p.engine = EngineType::Modal;
            p.modal.mode = 1;
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
            p.engine = EngineType::Fm;
            p.volume.set(0.8);
        },
        |p| {
            p.volume.set(0.1);
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
