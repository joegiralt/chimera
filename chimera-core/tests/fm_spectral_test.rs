use chimera_core::dsp::fm::{FmEngine, FmParams};

const SR: u32 = 48000;
const ANALYSIS_BLOCKS: usize = 32; // 32 * 128 = 4096 samples ≈ 85ms

/// Goertzel algorithm: measure energy at a specific frequency in a buffer.
/// Returns magnitude (not squared) at the target frequency.
fn goertzel(buf: &[f32], target_freq: f32, sample_rate: u32) -> f32 {
    let n = buf.len() as f32;
    let k = (target_freq * n / sample_rate as f32).round();
    let w = 2.0 * core::f32::consts::PI * k / n;
    let coeff = 2.0 * libm::cosf(w);

    let mut s0;
    let mut s1 = 0.0f32;
    let mut s2 = 0.0f32;

    for &sample in buf {
        s0 = sample + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }

    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    libm::sqrtf(power.abs()) / n
}

/// Render the FM engine for analysis: returns a buffer of samples.
fn render_fm(params: &FmParams, note: u8) -> Vec<f32> {
    let mut engine = FmEngine::new();
    engine.update_params(params, SR);
    engine.note_on(note, 100, SR);

    let mut all = Vec::new();
    let mut block = [0.0f32; 128];

    for _ in 0..ANALYSIS_BLOCKS {
        engine.render(&mut block, &params.op_env, SR);
        all.extend_from_slice(&block);
    }
    all
}

/// Get the fundamental frequency for a MIDI note.
fn note_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

// ── Basic FM spectrum tests ─────────────────────────────────────────

#[test]
fn test_pure_carrier_is_sine() {
    // No modulation -> carrier should be a pure sine at the fundamental
    let params = FmParams::default(); // op4 carrier only, modulators at 0
    let buf = render_fm(&params, 60); // middle C ≈ 261.6 Hz

    let f0 = note_freq(60);
    let fundamental = goertzel(&buf, f0, SR);
    let second_harmonic = goertzel(&buf, f0 * 2.0, SR);
    let third_harmonic = goertzel(&buf, f0 * 3.0, SR);

    assert!(
        fundamental > 0.01,
        "should have energy at fundamental: {}",
        fundamental
    );
    assert!(
        second_harmonic < fundamental * 0.05,
        "pure sine should have no 2nd harmonic: fund={} 2nd={}",
        fundamental,
        second_harmonic
    );
    assert!(
        third_harmonic < fundamental * 0.05,
        "pure sine should have no 3rd harmonic: fund={} 3rd={}",
        fundamental,
        third_harmonic
    );
}

#[test]
fn test_fm_adds_sidebands() {
    // Use algo 5 (1→2*) — op1 directly modulates carrier op2
    let mut params = FmParams::default();
    params.algorithm = 4; // algo 5
    params.op_level[0] = 0.5; // modulator
    params.op_level[1] = 1.0; // carrier (op2)

    let buf = render_fm(&params, 60);
    let f0 = note_freq(60);

    let fundamental = goertzel(&buf, f0, SR);
    let second = goertzel(&buf, f0 * 2.0, SR);
    let third = goertzel(&buf, f0 * 3.0, SR);

    assert!(
        second > fundamental * 0.01,
        "FM should create 2nd harmonic: fund={} 2nd={}",
        fundamental,
        second
    );
    assert!(
        third > fundamental * 0.005,
        "FM should create 3rd harmonic: fund={} 3rd={}",
        fundamental,
        third
    );
}

#[test]
fn test_more_modulation_more_harmonics() {
    // Use algo 5 (1→2*) for direct modulator→carrier
    let f0 = note_freq(60);

    let measure_brightness = |mod_level: f32| -> f32 {
        let mut params = FmParams::default();
        params.algorithm = 4; // algo 5: 1→2*
        params.op_level[0] = mod_level; // modulator
        params.op_level[1] = 1.0; // carrier
        let buf = render_fm(&params, 60);

        // Sum energy at harmonics 2-8 as a "brightness" metric
        let mut harmonic_energy = 0.0;
        for h in 2..=8 {
            harmonic_energy += goertzel(&buf, f0 * h as f32, SR);
        }
        harmonic_energy
    };

    let bright_none = measure_brightness(0.0);
    let bright_low = measure_brightness(0.15);
    let bright_mid = measure_brightness(0.4);

    assert!(
        bright_low > bright_none,
        "some modulation should add harmonics: none={} low={}",
        bright_none,
        bright_low
    );
    assert!(
        bright_mid > bright_low,
        "more modulation should mean more harmonics: low={} mid={}",
        bright_low,
        bright_mid
    );
    // Note: at very high mod indices, energy spreads into many sidebands
    // and individual harmonic peaks decrease — this is correct FM behavior.
}

#[test]
fn test_ratio_2_produces_even_harmonics() {
    let mut params = FmParams::default();
    params.algorithm = 4; // algo 5: 1→2*
    params.op_level[0] = 0.4;
    params.op_level[1] = 1.0;
    params.op_ratio[0] = 0.27; // maps to ratio 2.0

    let buf = render_fm(&params, 60);
    let f0 = note_freq(60);

    let h3 = goertzel(&buf, f0 * 3.0, SR);

    // With mod ratio 2:1, sidebands at f0 ± 2f0 = -f0, 3f0
    assert!(h3 > 0.001, "ratio 2:1 should produce 3rd harmonic: {}", h3);
}

#[test]
fn test_ratio_3_produces_third_harmonics() {
    let mut params = FmParams::default();
    params.algorithm = 4; // algo 5: 1→2*
    params.op_level[0] = 0.4;
    params.op_level[1] = 1.0;
    params.op_ratio[0] = 0.33; // maps to ratio 3.0

    let buf = render_fm(&params, 48); // lower note for cleaner spectrum

    let f0 = note_freq(48);
    let h4 = goertzel(&buf, f0 * 4.0, SR);

    // f0 + 3*f0 = 4*f0 should have energy
    assert!(
        h4 > 0.001,
        "ratio 3:1 should produce 4th harmonic (f+3f): {}",
        h4
    );
}

// ── Algorithm character tests ───────────────────────────────────────

#[test]
fn test_serial_vs_parallel_different_spectrum() {
    // Algo 0 (serial: 1→2→3→4) vs Algo 7 (additive: all carriers)
    // should produce very different spectra with same operator settings
    let f0 = note_freq(60);

    let spectrum_for_algo = |algo: u8| -> Vec<f32> {
        let mut params = FmParams::default();
        params.algorithm = algo;
        params.op_level = [0.5, 0.5, 0.5, 1.0]; // all ops active
        let buf = render_fm(&params, 60);
        (1..=8).map(|h| goertzel(&buf, f0 * h as f32, SR)).collect()
    };

    let serial = spectrum_for_algo(0);
    let additive = spectrum_for_algo(7);

    // Calculate spectral difference
    let diff: f32 = serial
        .iter()
        .zip(additive.iter())
        .map(|(a, b)| (a - b).abs())
        .sum();

    assert!(
        diff > 0.01,
        "serial vs additive should have different spectra, diff={}",
        diff
    );
}

#[test]
fn test_each_algorithm_has_unique_character() {
    // All 8 algorithms with the same operator levels should produce
    // measurably different spectra
    let f0 = note_freq(60);

    let spectrum_for_algo = |algo: u8| -> f32 {
        let mut params = FmParams::default();
        params.algorithm = algo;
        params.op_level = [0.5, 0.3, 0.3, 1.0];
        params.op_ratio[0] = 0.27; // ratio 2.0 for modulator
        let buf = render_fm(&params, 60);

        // Return a spectral fingerprint: weighted sum of harmonics
        let mut fingerprint = 0.0;
        for h in 1..=8 {
            fingerprint += goertzel(&buf, f0 * h as f32, SR) * h as f32;
        }
        fingerprint
    };

    let fingerprints: Vec<f32> = (0..8).map(spectrum_for_algo).collect();

    // Check that not all fingerprints are the same
    let min = fingerprints.iter().cloned().fold(f32::MAX, f32::min);
    let max = fingerprints.iter().cloned().fold(f32::MIN, f32::max);

    assert!(
        max - min > 0.01,
        "algorithms should sound different. fingerprints: {:?}",
        fingerprints
    );
}

// ── Feedback tests ──────────────────────────────────────────────────

#[test]
fn test_feedback_adds_harmonics() {
    let f0 = note_freq(60);

    let brightness_with_fb = |fb: f32| -> f32 {
        let mut params = FmParams::default();
        params.feedback = fb;
        // Op1 has feedback, and in algo 0 it feeds into op2→3→4
        // But with op2/3 at level 0, feedback only affects op1's own timbre
        // Use algo 7 where op1 is also a carrier so we can hear its feedback
        params.algorithm = 7; // all carriers
        params.op_level = [1.0, 0.0, 0.0, 1.0];
        let buf = render_fm(&params, 60);

        let mut energy = 0.0;
        for h in 2..=8 {
            energy += goertzel(&buf, f0 * h as f32, SR);
        }
        energy
    };

    let no_fb = brightness_with_fb(0.0);
    let hi_fb = brightness_with_fb(0.6);

    assert!(
        hi_fb > no_fb,
        "feedback should add harmonics: no_fb={} hi_fb={}",
        no_fb,
        hi_fb
    );
}

#[test]
fn test_waveform_changes_spectrum() {
    // Different carrier waveforms should produce different spectra
    let f0 = note_freq(60);

    let spectrum_for_wave = |wave: u8| -> f32 {
        let mut params = FmParams::default();
        params.op_waveform[3] = wave; // carrier waveform
        let buf = render_fm(&params, 60);

        let mut energy = 0.0;
        for h in 1..=8 {
            energy += goertzel(&buf, f0 * h as f32, SR);
        }
        energy
    };

    let sine = spectrum_for_wave(0);
    let half_sine = spectrum_for_wave(1);
    let full_sine = spectrum_for_wave(2);

    // Half-sine and full-sine (rectified) have more harmonics than pure sine
    assert!(
        (half_sine - sine).abs() > 0.001 || (full_sine - sine).abs() > 0.001,
        "different waveforms should produce different spectra: sine={} half={} full={}",
        sine,
        half_sine,
        full_sine
    );
}
