use chimera_core::dsp::oscillator::SineOsc;

#[test]
fn test_sine_osc_440hz() {
    let mut osc = SineOsc::new();
    osc.set_frequency(440.0, 48000);

    let mut buf = [0.0f32; 64];
    osc.render(&mut buf);

    let max = buf.iter().copied().fold(0.0f32, f32::max);
    let min = buf.iter().copied().fold(0.0f32, f32::min);
    assert!(
        max > 0.3,
        "sine should have positive peaks, got max {}",
        max
    );
    assert!(
        min < -0.3,
        "sine should have negative peaks, got min {}",
        min
    );

    // 440 Hz at 48000 Hz = ~109 sample period
    // In 64 samples we should see at least 1 zero crossing
    let mut crossings = 0;
    for i in 1..buf.len() {
        if (buf[i - 1] >= 0.0) != (buf[i] >= 0.0) {
            crossings += 1;
        }
    }
    assert!(
        crossings >= 1,
        "should have zero crossings, got {}",
        crossings
    );
}

#[test]
fn test_sine_osc_silence_at_zero_freq() {
    let mut osc = SineOsc::new();
    osc.set_frequency(0.0, 48000);
    let mut buf = [0.0f32; 64];
    osc.render(&mut buf);
    assert!(buf.iter().all(|&s| s == 0.0));
}
