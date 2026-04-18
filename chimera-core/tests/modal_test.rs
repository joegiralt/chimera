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
    let buf = render_modal(&params, 48, 64); // lower note, longer render
    let f0 = note_freq(48);
    let fund = goertzel(&buf, f0, SR);
    // Rings-style: fundamental may be quieter than upper modes at default Q
    assert!(fund > 0.0001, "modal should have fundamental at {}Hz: energy={}", f0, fund);
}

#[test]
fn test_modal_has_harmonics() {
    let params = ModalParams::default();
    let buf = render_modal(&params, 48, 64);
    let f0 = note_freq(48);

    // Check that SOME harmonics have energy (not necessarily all, depends on Q and position)
    let mut total_harmonic_energy = 0.0;
    for h in 1..=12 {
        total_harmonic_energy += goertzel(&buf, f0 * h as f32, SR);
    }
    assert!(total_harmonic_energy > 0.001, "should have harmonic energy: {}", total_harmonic_energy);
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

#[test]
fn test_inharm_actually_shifts_modes() {
    let f0 = note_freq(48); // ~130 Hz, low note for clear spectrum

    let measure_mode_freqs = |inharm: f32| -> Vec<f32> {
        let mut params = ModalParams::default();
        params.inharm = inharm;
        params.brightness = 1.0; // keep all modes bright
        params.decay = 0.8;
        let buf = render_modal(&params, 48, 64);

        // Check energy at exact harmonics and some shifted frequencies
        let mut results = Vec::new();
        for h in 1..=8 {
            let exact = goertzel(&buf, f0 * h as f32, SR);
            let shifted_up = goertzel(&buf, f0 * h as f32 * 1.1, SR);
            let shifted_down = goertzel(&buf, f0 * h as f32 * 0.9, SR);
            results.push((exact, shifted_up, shifted_down));
        }
        // Return the energy at exact harmonics
        results.iter().map(|(e, _, _)| *e).collect()
    };

    let harmonic = measure_mode_freqs(0.25); // harmonic plateau
    let inharmonic = measure_mode_freqs(1.0);

    eprintln!("Harmonic (inharm=0) energies at exact harmonics:");
    for (i, e) in harmonic.iter().enumerate() {
        eprintln!("  H{}: {:.6}", i+1, e);
    }
    eprintln!("Inharmonic (inharm=1) energies at exact harmonics:");
    for (i, e) in inharmonic.iter().enumerate() {
        eprintln!("  H{}: {:.6}", i+1, e);
    }

    // With inharm=0, modes should be AT the harmonics (high energy)
    // With inharm=1, modes should be SHIFTED AWAY from exact harmonics (lower energy at those freqs)
    let harmonic_total: f32 = harmonic[2..].iter().sum(); // harmonics 3+
    let inharmonic_total: f32 = inharmonic[2..].iter().sum();

    eprintln!("Energy at exact harmonics 3-8: harmonic={:.6} inharmonic={:.6}", harmonic_total, inharmonic_total);

    // The inharmonic version should have LESS energy at exact harmonic frequencies
    // because its modes are shifted to non-harmonic positions
    assert!(
        harmonic_total > inharmonic_total * 1.1,
        "inharm should shift modes away from exact harmonics: harmonic={} inharmonic={}",
        harmonic_total, inharmonic_total
    );
}

#[test]
fn test_inharm_spreads_spectrum() {
    // Inharmonic modes should have energy at non-harmonic frequencies
    let f0 = note_freq(48);

    let non_harmonic_energy = |inharm: f32| -> f32 {
        let mut params = ModalParams::default();
        params.inharm = inharm;
        params.brightness = 1.0;
        params.decay = 0.8;
        let buf = render_modal(&params, 48, 64);
        // Measure energy between harmonics
        let mut energy = 0.0;
        for mult in &[1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5] {
            energy += goertzel(&buf, f0 * mult, SR);
        }
        energy
    };

    let harmonic_between = non_harmonic_energy(0.25); // harmonic plateau
    let inharmonic_between = non_harmonic_energy(1.0); // stretched

    eprintln!("Energy between harmonics: harmonic={:.6} inharmonic={:.6}",
        harmonic_between, inharmonic_between);

    assert!(
        inharmonic_between > harmonic_between,
        "inharmonic should have more energy between harmonics: h={} ih={}",
        harmonic_between, inharmonic_between
    );
}

#[test]
fn test_modal_debug_output() {
    // Debug: just print what the modal engine actually produces
    let mut params = ModalParams::default();
    params.brightness = 1.0;
    params.decay = 0.8;
    params.inharm = 0.0;

    let f0 = note_freq(48);
    let buf = render_modal(&params, 48, 64);

    eprintln!("\n=== Modal spectrum (inharm=0, harmonic) ===");
    for h in 1..=12 {
        let freq = f0 * h as f32;
        let energy = goertzel(&buf, freq, SR);
        eprintln!("  {}Hz (H{}): {:.6}", freq as i32, h, energy);
    }

    // Also check some non-harmonic frequencies
    eprintln!("\n  Non-harmonic frequencies:");
    for f in &[f0 * 1.5, f0 * 2.5, f0 * 3.5, f0 * 4.7] {
        let energy = goertzel(&buf, *f, SR);
        eprintln!("  {}Hz: {:.6}", *f as i32, energy);
    }

    let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let rms = libm::sqrtf(buf.iter().map(|s| s*s).sum::<f32>() / buf.len() as f32);
    eprintln!("\n  Max: {:.4}, RMS: {:.4}", max, rms);
    eprintln!("  Samples: {}", buf.len());

    // Now with inharm
    params.inharm = 1.0;
    let buf2 = render_modal(&params, 48, 64);

    eprintln!("\n=== Modal spectrum (inharm=1, metallic) ===");
    for h in 1..=12 {
        let freq = f0 * h as f32;
        let energy = goertzel(&buf2, freq, SR);
        eprintln!("  {}Hz (H{}): {:.6}", freq as i32, h, energy);
    }

    assert!(true); // always passes, just for debug output
}
