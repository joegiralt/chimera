use chimera_core::dsp::fm::{
    FmEngine, FmOperator, FmParams, HARMONIC_RATIOS, Waveform, ratio_from_normalized,
};
use chimera_core::params::EnvParams;

// ── Waveforms ───────────────────────────────────────────────────────

#[test]
fn test_waveform_from_index_wraps() {
    assert_eq!(Waveform::from_index(0), Waveform::Sine);
    assert_eq!(Waveform::from_index(7), Waveform::ResPulse2);
    assert_eq!(Waveform::from_index(8), Waveform::Sine); // wraps
    assert_eq!(Waveform::from_index(255), Waveform::ResPulse2);
}

// ── Harmonic ratios ─────────────────────────────────────────────────

#[test]
fn test_harmonic_ratios_are_sorted() {
    for i in 1..HARMONIC_RATIOS.len() {
        assert!(
            HARMONIC_RATIOS[i] > HARMONIC_RATIOS[i - 1],
            "ratios must be ascending"
        );
    }
}

#[test]
fn test_ratio_from_normalized_bounds() {
    let r0 = ratio_from_normalized(0.0);
    assert!((r0 - HARMONIC_RATIOS[0]).abs() < 0.01);

    let r1 = ratio_from_normalized(1.0);
    assert!((r1 - *HARMONIC_RATIOS.last().unwrap()).abs() < 0.01);
}

#[test]
fn test_ratio_includes_fundamental() {
    assert!(HARMONIC_RATIOS.contains(&1.0));
}

#[test]
fn test_ratio_includes_octave() {
    assert!(HARMONIC_RATIOS.contains(&2.0));
}

// ── Operator ────────────────────────────────────────────────────────

#[test]
fn test_operator_silent_when_idle() {
    let mut op = FmOperator::new();
    let env = EnvParams::default();
    // Without note_on, envelope is idle -> output should be 0
    let out = op.tick(0.0, 0.0, &env, 48000);
    assert!((out).abs() < 0.001);
}

#[test]
fn test_operator_produces_sound_after_note_on() {
    let mut op = FmOperator::new();
    op.set_frequency(440.0, 48000);
    op.note_on(1.0);

    let env = EnvParams::default();
    // Advance a few samples to let attack develop
    let mut max = 0.0f32;
    for _ in 0..480 {
        let out = op.tick(0.0, 0.0, &env, 48000);
        max = max.max(out.abs());
    }
    assert!(max > 0.01, "operator should produce sound after note_on");
}

#[test]
fn test_operator_goes_silent_after_note_off() {
    let mut op = FmOperator::new();
    op.set_frequency(440.0, 48000);
    op.note_on(1.0);

    let mut env = EnvParams::default();
    env.release.set(0.001); // very fast release

    // Play for a bit
    for _ in 0..1000 {
        op.tick(0.0, 0.0, &env, 48000);
    }

    op.note_off();

    // Let release finish
    for _ in 0..500 {
        op.tick(0.0, 0.0, &env, 48000);
    }

    assert!(!op.is_active(), "operator should be idle after release");
}

// ── Engine ──────────────────────────────────────────────────────────

#[test]
fn test_engine_silent_when_no_note() {
    let mut engine = FmEngine::new();
    let mut output = [0.0f32; 128];
    let env = [EnvParams::default(); 4];
    engine.render(&mut output, &env, 48000);

    let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max < 0.001, "engine should be silent with no note");
}

#[test]
fn test_engine_produces_sound() {
    let mut engine = FmEngine::new();
    engine.note_on(60, 100, 48000); // Middle C

    let mut output = [0.0f32; 128];
    let env = [EnvParams::default(); 4];

    // Render a few blocks to let attack develop
    for _ in 0..4 {
        engine.render(&mut output, &env, 48000);
    }

    let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(
        max > 0.01,
        "engine should produce sound after note_on, got max={}",
        max
    );
}

#[test]
fn test_engine_algorithm_8_is_additive() {
    let mut engine = FmEngine::new();
    engine.algorithm = 7; // Algo 8 (0-indexed)
    engine.note_on(60, 100, 48000);

    let mut output = [0.0f32; 128];
    let env = [EnvParams::default(); 4];
    for _ in 0..4 {
        engine.render(&mut output, &env, 48000);
    }

    // All 4 ops are carriers in algo 8 — should produce sound
    let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max > 0.01, "algo 8 (additive) should produce sound");
}

#[test]
fn test_default_is_clean_sine() {
    // Default params: all modulators at level 0 -> pure sine from carrier
    let mut engine = FmEngine::new();
    let params = FmParams::default();
    engine.update_params(&params, 48000);
    engine.note_on(60, 100, 48000);

    let mut output = [0.0f32; 128];
    // Let attack develop
    for _ in 0..8 {
        engine.render(&mut output, &params.op_env, 48000);
    }

    // Should be a clean sine — check that there are zero crossings
    // and the waveform is smooth (no high-frequency FM artifacts)
    let mut zero_crossings = 0;
    for i in 1..128 {
        if (output[i] >= 0.0) != (output[i - 1] >= 0.0) {
            zero_crossings += 1;
        }
    }
    // Middle C at 48kHz: ~261 Hz, 128 samples ≈ 2.67ms ≈ 0.7 cycles ≈ 1-2 zero crossings
    assert!(
        zero_crossings <= 4,
        "default should be clean sine, got {} zero crossings",
        zero_crossings
    );
}

#[test]
fn test_modulation_depth_changes_timbre() {
    let mut engine_quiet = FmEngine::new();
    let mut engine_loud = FmEngine::new();

    let mut params_quiet = FmParams::default();
    let mut params_loud = FmParams::default();

    // Quiet: all modulators off -> clean sine
    params_quiet.op_level = [1.0, 0.0, 0.0, 0.0];
    // Loud: full modulation chain active -> heavy FM
    params_loud.op_level = [1.0, 0.8, 0.8, 0.8];

    engine_quiet.update_params(&params_quiet, 48000);
    engine_loud.update_params(&params_loud, 48000);
    engine_quiet.note_on(60, 100, 48000);
    engine_loud.note_on(60, 100, 48000);

    let mut out_quiet = [0.0f32; 128];
    let mut out_loud = [0.0f32; 128];

    for _ in 0..8 {
        engine_quiet.render(&mut out_quiet, &params_quiet.op_env, 48000);
        engine_loud.render(&mut out_loud, &params_loud.op_env, 48000);
    }

    // Count zero crossings — FM should have more (higher harmonics)
    let zc = |buf: &[f32]| -> usize {
        buf.windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count()
    };

    let zc_quiet = zc(&out_quiet);
    let zc_loud = zc(&out_loud);
    assert!(
        zc_loud > zc_quiet,
        "high modulation should produce more harmonics: quiet={} loud={}",
        zc_quiet,
        zc_loud
    );
}

#[test]
fn test_algorithms_sound_different() {
    // With modulation active, different algorithms should produce different output
    let mut params = FmParams::default();
    params.op_level = [1.0, 0.5, 0.5, 0.8]; // modulators active

    let rms = |algo: u8| -> f32 {
        let mut engine = FmEngine::new();
        let mut p = params;
        p.algorithm = algo;
        engine.update_params(&p, 48000);
        engine.note_on(60, 100, 48000);
        let mut output = [0.0f32; 128];
        for _ in 0..8 {
            engine.render(&mut output, &p.op_env, 48000);
        }
        let sum_sq: f32 = output.iter().map(|s| s * s).sum();
        libm::sqrtf(sum_sq / 128.0)
    };

    // Algo 0 (serial) vs Algo 7 (all carriers/additive) should differ
    let rms_serial = rms(0);
    let rms_additive = rms(7);
    assert!(
        (rms_serial - rms_additive).abs() > 0.01,
        "different algorithms should produce different RMS: serial={} additive={}",
        rms_serial,
        rms_additive
    );
}

#[test]
fn test_different_ratios_change_pitch() {
    let mut params = FmParams::default();

    let mut peak_freq = |ratio_norm: f32| -> usize {
        let mut engine = FmEngine::new();
        params.op_ratio[3] = ratio_norm; // op4 is the carrier
        engine.update_params(&params, 48000);
        engine.note_on(60, 100, 48000);
        let mut output = [0.0f32; 128];
        for _ in 0..8 {
            engine.render(&mut output, &params.op_env, 48000);
        }
        // Count zero crossings as proxy for frequency
        output
            .windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count()
    };

    let zc_low = peak_freq(0.2); // ratio 1.0 (fundamental)
    let zc_high = peak_freq(0.4); // ratio ~3.0 (higher)
    assert!(
        zc_high > zc_low,
        "higher ratio should produce higher frequency: low={} high={}",
        zc_low,
        zc_high
    );
}

#[test]
fn test_engine_output_bounded() {
    let mut engine = FmEngine::new();
    engine.feedback = 0.8; // high feedback
    engine.note_on(60, 127, 48000);

    let env = [EnvParams::default(); 4];
    let mut output = [0.0f32; 128];

    for _ in 0..20 {
        engine.render(&mut output, &env, 48000);
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max < 10.0,
            "output should stay bounded even with high feedback, got {}",
            max
        );
    }
}

#[test]
fn test_engine_update_params() {
    let mut engine = FmEngine::new();
    let mut params = FmParams::default();
    params.algorithm = 5;
    params.feedback = 0.3;
    params.op_ratio[0] = 0.5; // should map to some harmonic ratio

    engine.update_params(&params, 48000);
    assert_eq!(engine.algorithm, 5);
    assert!((engine.feedback - 0.3).abs() < 0.001);
}

#[test]
fn test_fm_params_default_is_sane() {
    let p = FmParams::default();
    assert_eq!(p.algorithm, 0);
    assert_eq!(p.feedback, 0.0);
    assert_eq!(p.op_waveform, [0, 0, 0, 0]);
    assert!(
        p.op_level[3] > 0.0,
        "carrier (op4) should have nonzero level"
    );
    assert_eq!(p.op_level[0], 0.0, "modulator (op1) should start silent");
}
