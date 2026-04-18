use chimera_core::dsp::modal::{ModalEngine, ModalParams};

const SR: u32 = 48000;

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
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    libm::sqrtf(power.abs()) / n
}

fn render_modal(params: &ModalParams, note: u8, blocks: usize) -> Vec<f32> {
    let mut engine = ModalEngine::new();
    engine.note_on(note, 100, params, SR);
    let mut all = Vec::new();
    let mut block = [0.0f32; 128];
    for _ in 0..blocks {
        engine.render(&mut block, params, SR);
        all.extend_from_slice(&block);
    }
    all
}

fn note_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

#[test]
fn test_modal_produces_sound() {
    let params = ModalParams::default();
    let buf = render_modal(&params, 60, 16);
    let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max > 0.001, "modal should produce sound, got max={}", max);
}

#[test]
fn test_modal_has_fundamental() {
    let params = ModalParams::default();
    let buf = render_modal(&params, 60, 32);
    let f0 = note_freq(60);
    let fund = goertzel(&buf, f0, SR);
    assert!(fund > 0.001, "modal should have fundamental at {}Hz: energy={}", f0, fund);
}

#[test]
fn test_modal_has_harmonics() {
    let params = ModalParams::default();
    let buf = render_modal(&params, 48, 32); // lower note for cleaner spectrum
    let f0 = note_freq(48);
    let h1 = goertzel(&buf, f0, SR);
    let h2 = goertzel(&buf, f0 * 2.0, SR);
    let h3 = goertzel(&buf, f0 * 3.0, SR);

    assert!(h1 > 0.001, "should have fundamental: {}", h1);
    assert!(h2 > 0.0001, "should have 2nd harmonic: {}", h2);
    assert!(h3 > 0.0001, "should have 3rd harmonic: {}", h3);
}

#[test]
fn test_modal_decays() {
    let params = ModalParams::default();
    let mut engine = ModalEngine::new();
    engine.note_on(60, 100, &params, SR);

    let mut block = [0.0f32; 128];

    // Measure energy in early block
    engine.render(&mut block, &params, SR);
    engine.render(&mut block, &params, SR);
    let early_rms: f32 = libm::sqrtf(block.iter().map(|s| s * s).sum::<f32>() / 128.0);

    // Skip ahead
    for _ in 0..100 {
        engine.render(&mut block, &params, SR);
    }
    let late_rms: f32 = libm::sqrtf(block.iter().map(|s| s * s).sum::<f32>() / 128.0);

    assert!(
        late_rms < early_rms,
        "modal should decay over time: early={} late={}",
        early_rms,
        late_rms
    );
}

#[test]
fn test_modal_silent_when_idle() {
    let params = ModalParams::default();
    let mut engine = ModalEngine::new();
    let mut block = [0.0f32; 128];
    engine.render(&mut block, &params, SR);
    let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max < 0.001, "idle modal should be silent");
}

#[test]
fn test_modal_output_bounded() {
    let mut params = ModalParams::default();
    params.excite = 1.0;
    params.decay = 1.0;
    params.brightness = 1.0;
    let buf = render_modal(&params, 60, 32);
    let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max < 10.0, "modal output should be bounded, got max={}", max);
}

#[test]
fn test_modal_inharm_changes_spectrum() {
    let f0 = note_freq(48);

    let spectrum = |inharm: f32| -> f32 {
        let mut params = ModalParams::default();
        params.inharm = inharm;
        let buf = render_modal(&params, 48, 32);
        // Measure energy at exact harmonics — inharmonic modes will miss these
        let mut energy = 0.0;
        for h in 1..=8 {
            energy += goertzel(&buf, f0 * h as f32, SR);
        }
        energy
    };

    let harmonic = spectrum(0.0);
    let inharmonic = spectrum(1.0);

    assert!(
        (harmonic - inharmonic).abs() > 0.0001,
        "inharm should change spectrum: harmonic={} inharmonic={}",
        harmonic,
        inharmonic
    );
}

#[test]
fn test_modal_brightness_changes_spectrum() {
    let f0 = note_freq(48);

    let high_harmonic_energy = |brightness: f32| -> f32 {
        let mut params = ModalParams::default();
        params.brightness = brightness;
        let buf = render_modal(&params, 48, 16);
        // Energy in harmonics 4-8 only (high partials)
        (4..=8).map(|h| goertzel(&buf, f0 * h as f32, SR)).sum()
    };

    let dark = high_harmonic_energy(0.0);
    let bright = high_harmonic_energy(1.0);

    assert!(
        bright > dark,
        "higher brightness should have more high harmonics: dark={} bright={}",
        dark,
        bright
    );
}

#[test]
fn test_modal_resonator_rings_at_pitch() {
    // The strongest spectral peak should be near the fundamental
    let params = ModalParams::default();
    let buf = render_modal(&params, 60, 32);
    let f0 = note_freq(60);

    let fund = goertzel(&buf, f0, SR);
    // Check some non-harmonic frequencies have less energy
    let off1 = goertzel(&buf, f0 * 1.13, SR); // between harmonics
    let off2 = goertzel(&buf, f0 * 1.73, SR);

    assert!(
        fund > off1 && fund > off2,
        "fundamental should be stronger than non-harmonic freqs: fund={} off1={} off2={}",
        fund, off1, off2
    );
}

#[test]
fn test_modal_position_changes_spectrum() {
    let f0 = note_freq(48);

    let second_harmonic = |pos: f32| -> f32 {
        let mut params = ModalParams::default();
        params.position = pos;
        let buf = render_modal(&params, 48, 16);
        goertzel(&buf, f0 * 2.0, SR)
    };

    // At position 0.5 (center), even harmonics should be suppressed
    // At position 0.25, all harmonics should be present
    let center = second_harmonic(0.5);
    let quarter = second_harmonic(0.25);

    // Position should at least change the 2nd harmonic level
    assert!(
        (center - quarter).abs() > 0.00001,
        "position should affect harmonics: center={} quarter={}",
        center,
        quarter
    );
}
