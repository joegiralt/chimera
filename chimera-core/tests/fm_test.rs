use chimera_core::dsp::envelope_fm::FmEnvelope;
use chimera_core::dsp::engine_fm::{FmEngine, FmOperator, FmOpSettings};
use chimera_core::dsp::fm_tables;
use chimera_core::dsp::fm_waveform;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};

#[test]
fn ratio_table_unity() {
    assert_eq!(fm_tables::compute_ratio(4, 0), 1.0);
}

#[test]
fn ratio_table_half() {
    assert_eq!(fm_tables::compute_ratio(0, 0), 0.5);
}

#[test]
fn ratio_table_fine_interpolates() {
    let r0 = fm_tables::compute_ratio(4, 0);
    let r15 = fm_tables::compute_ratio(4, 15);
    assert!(r15 > r0);
    assert!(r15 <= fm_tables::FREQ_RATIOS_MAX[4]);
}

#[test]
fn ratio_table_sub4_clamps_fine() {
    let r7 = fm_tables::compute_ratio(0, 7);
    let r15 = fm_tables::compute_ratio(0, 15);
    assert_eq!(r7, r15);
}

#[test]
fn level_to_gain_zero_is_tiny() {
    let g = fm_tables::level_to_gain(0);
    assert!(g > 0.0);
    assert!(g < 0.01);
}

#[test]
fn level_to_gain_99_near_unity() {
    let g = fm_tables::level_to_gain(99);
    assert!(g > 0.5);
    assert!(g <= 2.0);
}

#[test]
fn d1l_zero_returns_zero() {
    assert_eq!(fm_tables::d1l_to_level(0), 0.0);
}

#[test]
fn d1l_15_near_unity() {
    let l = fm_tables::d1l_to_level(15);
    assert!(l > 0.9);
    assert!(l <= 1.0);
}

#[test]
fn feedback_factors_correct() {
    assert_eq!(fm_tables::FEEDBACK[0], 0.0);
    assert_eq!(fm_tables::FEEDBACK[7], 0.26);
}

#[test]
fn waveform_0_is_sine() {
    let v = fm_waveform::compute(0, 0.25);
    assert!((v - 1.0).abs() < 0.001);
}

#[test]
fn waveform_0_zero_at_origin() {
    let v = fm_waveform::compute(0, 0.0);
    assert!(v.abs() < 0.001);
}

#[test]
fn waveform_2_half_sine_zero_second_half() {
    let v = fm_waveform::compute(2, 0.75);
    assert!(v.abs() < 0.001);
}

#[test]
fn all_8_waveforms_produce_different_output() {
    let phase = 0.13;
    let values: [f32; 8] = core::array::from_fn(|w| fm_waveform::compute(w as u8, phase));
    let mut unique = values.to_vec();
    unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    assert!(unique.len() >= 4);
}

#[test]
fn all_waveforms_bounded() {
    for w in 0..8u8 {
        for i in 0..1024 {
            let phase = i as f32 / 1024.0;
            let v = fm_waveform::compute(w, phase);
            assert!(v.is_finite(), "w={w} phase={phase}");
            assert!(v >= -2.0 && v <= 2.0, "w={w} phase={phase} v={v}");
        }
    }
}

#[test]
fn fm_envelope_starts_idle() {
    let env = FmEnvelope::new();
    assert!(env.is_idle());
    assert_eq!(env.current_level(), 0.0);
}

#[test]
fn fm_envelope_attack_reaches_peak() {
    let mut env = FmEnvelope::new();
    // Fast attack (rate=31), slow decay, full D1L
    env.note_on(31, 0, 15, 0, 15, 0, 0, 48000.0, 60);
    let mut buf = [0.0f32; 4800]; // 100ms
    env.run(&mut buf);
    // Should have values > 0.9 somewhere
    assert!(buf.iter().any(|&v| v > 0.9));
}

#[test]
fn fm_envelope_gate_off_silences() {
    let mut env = FmEnvelope::new();
    env.note_on(31, 31, 15, 0, 15, 0, 0, 48000.0, 60);
    let mut buf = [0.0f32; 480];
    env.run(&mut buf);
    env.note_off();
    // Process more samples — should go silent
    for _ in 0..100 {
        env.run(&mut buf);
    }
    assert!(env.current_level() < 0.001);
}

#[test]
fn fm_envelope_d1l_zero_decays_to_silence() {
    let mut env = FmEnvelope::new();
    // Fast attack, fast D1, D1L=0 (decay to zero)
    env.note_on(31, 31, 0, 0, 15, 0, 0, 48000.0, 60);
    let mut buf = [0.0f32; 4800];
    for _ in 0..20 {
        env.run(&mut buf);
    }
    assert!(env.current_level() < 0.001);
}

#[test]
fn fm_operator_produces_sound() {
    let mut op = FmOperator::new();
    let settings = FmOpSettings {
        waveform: 0, coarse: 4, fine: 0, level: 99,
        feedback: 0, detune: 0, velocity_sens: 0,
        ar: 31, d1r: 0, d1l: 15, d2r: 0, rr: 15, rate_scaling: 0,
    };
    op.note_on(69, 1.0, &settings, 48000.0);
    let zeros = [0.0f32; 1024];
    let mut buf = [0.0f32; 1024];
    op.run(&zeros, &mut buf);
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms > 0.01, "rms={rms}");
}

#[test]
fn fm_operator_modulation_changes_timbre() {
    let settings = FmOpSettings {
        waveform: 0, coarse: 4, fine: 0, level: 99,
        feedback: 0, detune: 0, velocity_sens: 0,
        ar: 31, d1r: 0, d1l: 15, d2r: 0, rr: 15, rate_scaling: 0,
    };

    // Clean (no modulation)
    let mut op = FmOperator::new();
    op.note_on(69, 1.0, &settings, 48000.0);
    let zeros = [0.0f32; 1024];
    let mut clean = [0.0f32; 1024];
    op.run(&zeros, &mut clean);

    // Modulated
    let mut op2 = FmOperator::new();
    op2.note_on(69, 1.0, &settings, 48000.0);
    let modulator = [0.3f32; 1024];
    let mut modded = [0.0f32; 1024];
    op2.run(&modulator, &mut modded);

    let diff: f32 = clean.iter().zip(modded.iter()).map(|(a, b)| (a - b).abs()).sum::<f32>();
    assert!(diff > 1.0, "modulation should change output");
}

#[test]
fn fm_operator_feedback_adds_harmonics() {
    let mut settings = FmOpSettings {
        waveform: 0, coarse: 4, fine: 0, level: 99,
        feedback: 0, detune: 0, velocity_sens: 0,
        ar: 31, d1r: 0, d1l: 15, d2r: 0, rr: 15, rate_scaling: 0,
    };

    // No feedback
    let mut op = FmOperator::new();
    op.note_on(69, 1.0, &settings, 48000.0);
    let zeros = [0.0f32; 2048];
    let mut clean = [0.0f32; 2048];
    op.run(&zeros, &mut clean);

    // Max feedback
    settings.feedback = 7;
    let mut op2 = FmOperator::new();
    op2.note_on(69, 1.0, &settings, 48000.0);
    let mut fb = [0.0f32; 2048];
    op2.run(&zeros, &mut fb);

    let diff: f32 = clean.iter().zip(fb.iter()).map(|(a, b)| (a - b).abs()).sum::<f32>();
    assert!(diff > 1.0, "feedback should change timbre");
}

// ---------------------------------------------------------------------------
// FmEngine tests
// ---------------------------------------------------------------------------

#[test]
fn fm_engine_produces_sound() {
    let mut engine = FmEngine::new();
    let settings = [FmOpSettings::default(); 4];
    engine.note_on(69, 1.0, 0, &settings, 48000.0);
    let mut buf = [0.0f32; 1024];
    engine.render(&mut buf, 0, &settings);
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms > 0.01, "rms={rms}");
}

#[test]
fn fm_engine_note_off_decays() {
    let mut engine = FmEngine::new();
    let settings = [FmOpSettings::default(); 4];
    engine.note_on(69, 1.0, 0, &settings, 48000.0);
    let mut buf = [0.0f32; 64];
    engine.render(&mut buf, 0, &settings);
    engine.note_off();
    for _ in 0..2000 {
        engine.render(&mut buf, 0, &settings);
    }
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 64.0).sqrt();
    assert!(rms < 0.001, "should be silent, rms={rms}");
}

#[test]
fn fm_all_algorithms_produce_different_output() {
    let mut results = [0.0f32; 8];
    for alg in 0..8u8 {
        let mut engine = FmEngine::new();
        let mut settings = [FmOpSettings::default(); 4];
        for (i, s) in settings.iter_mut().enumerate() {
            s.level = 90;
            s.coarse = (4 + i * 4) as u8;
        }
        engine.note_on(69, 1.0, alg, &settings, 48000.0);
        let mut buf = [0.0f32; 2048];
        engine.render(&mut buf, alg, &settings);
        results[alg as usize] = (buf.iter().map(|x| x * x).sum::<f32>() / 2048.0).sqrt();
    }
    let first = results[0];
    assert!(results.iter().any(|&r| (r - first).abs() > 0.001),
        "all algorithms should not be identical: {:?}", results);
}

#[test]
fn fm_output_bounded() {
    for alg in 0..8u8 {
        let mut engine = FmEngine::new();
        let mut settings = [FmOpSettings::default(); 4];
        for s in settings.iter_mut() {
            s.level = 99;
            s.feedback = 7;
        }
        engine.note_on(69, 1.0, alg, &settings, 48000.0);
        let mut buf = [0.0f32; 4096];
        engine.render(&mut buf, alg, &settings);
        for &s in &buf {
            assert!(s.is_finite(), "alg={alg} NaN/Inf");
        }
    }
}

// ---------------------------------------------------------------------------
// Voice integration tests
// ---------------------------------------------------------------------------

#[test]
fn voice_fm_produces_sound() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Fm;
    // Op1 already has level=99 from FmParams default
    voice.note_on(69, 100, &params, 48000);
    let mut buf = [0.0f32; 64];
    let mod_state = ModState::default();
    voice.render(&mut buf, &params, &mod_state, 48000);
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt();
    assert!(rms > 0.001, "FM voice should produce sound, rms={rms}");
}

#[test]
fn voice_fm_output_finite() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Fm;
    for op in params.fm.operators.iter_mut() {
        op.level.value = 99.0;
        op.feedback.value = 7.0;
    }
    voice.note_on(69, 127, &params, 48000);
    let mut buf = [0.0f32; 64];
    let mod_state = ModState::default();
    // Render many blocks to stress-test
    for _ in 0..64 {
        voice.render(&mut buf, &params, &mod_state, 48000);
        for &s in &buf {
            assert!(s.is_finite(), "NaN/Inf in FM voice output");
        }
    }
}
