use chimera_core::dsp::envelope::Envelope;
use chimera_core::params::EnvParams;

#[test]
fn test_envelope_attack_decay_sustain() {
    let params = EnvParams::default();
    let mut env = Envelope::new();
    let sample_rate = 48000;

    env.note_on(1.0);

    // Run through attack phase (0.01s = 480 samples)
    let mut last = 0.0;
    for _ in 0..480 {
        last = env.process(&params, sample_rate);
    }
    assert!(
        last > 0.8,
        "after attack, level should be near 1.0, got {}",
        last
    );

    // Run through decay into sustain (0.3s = 14400 samples)
    for _ in 0..14400 {
        last = env.process(&params, sample_rate);
    }
    assert!(
        (last - 0.7).abs() < 0.05,
        "at sustain, level should be ~0.7, got {}",
        last
    );
}

#[test]
fn test_envelope_release() {
    let params = EnvParams::default();
    let mut env = Envelope::new();
    let sample_rate = 48000;

    env.note_on(1.0);
    for _ in 0..20000 {
        env.process(&params, sample_rate);
    }

    env.note_off();

    let mut last = 0.0;
    for _ in 0..14400 {
        last = env.process(&params, sample_rate);
    }
    assert!(
        last < 0.05,
        "after release, level should be near 0, got {}",
        last
    );
}

#[test]
fn test_envelope_idle_is_zero() {
    let params = EnvParams::default();
    let mut env = Envelope::new();
    let val = env.process(&params, 48000);
    assert_eq!(val, 0.0);
}
