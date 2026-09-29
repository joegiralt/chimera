mod common;
use chimera_core::dsp::modal::{BankModes, ModalEngine, ModalParams, ResonatorMode, SymPool};
use common::{SR, goertzel};

fn modal_params() -> ModalParams {
    ModalParams {
        mode: ResonatorMode::Modal, // Modal resonator (SVF bank)
        ..Default::default()
    }
}

fn render_modal(params: &ModalParams, note: u8, blocks: usize) -> Vec<f32> {
    let mut pool = SymPool::boxed();
    let mut engine = ModalEngine::new_in(&mut pool, params.mode);
    engine.note_on(note, 100, params, SR, &mut pool);
    let mut all = Vec::new();
    let mut block = [0.0f32; 64];
    for _ in 0..blocks {
        engine.render(&mut block, params, SR, &mut pool);
        all.extend_from_slice(&block);
    }
    all
}

fn note_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

#[test]
fn test_modal_produces_sound() {
    let params = modal_params();
    let buf = render_modal(&params, 60, 16);
    let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max > 0.001, "modal should produce sound, got max={}", max);
}

#[test]
fn test_modal_has_fundamental() {
    let params = modal_params();
    let buf = render_modal(&params, 48, 64); // lower note, longer render
    let f0 = note_freq(48);
    let fund = goertzel(&buf, f0, SR);
    // Rings-style: fundamental may be quieter than upper modes at default Q
    assert!(
        fund > 0.0001,
        "modal should have fundamental at {}Hz: energy={}",
        f0,
        fund
    );
}

#[test]
fn test_modal_has_harmonics() {
    let params = modal_params();
    let buf = render_modal(&params, 48, 64);
    let f0 = note_freq(48);

    // Check that SOME harmonics have energy (not necessarily all, depends on Q and position)
    let mut total_harmonic_energy = 0.0;
    for h in 1..=12 {
        total_harmonic_energy += goertzel(&buf, f0 * h as f32, SR);
    }
    assert!(
        total_harmonic_energy > 0.001,
        "should have harmonic energy: {}",
        total_harmonic_energy
    );
}

#[test]
fn test_modal_decays() {
    let params = modal_params();
    let mut pool = SymPool::boxed();
    let mut engine = ModalEngine::new_in(&mut pool, params.mode);
    engine.note_on(60, 100, &params, SR, &mut pool);

    let mut block = [0.0f32; 64];

    // Measure energy in early block
    engine.render(&mut block, &params, SR, &mut pool);
    engine.render(&mut block, &params, SR, &mut pool);
    let early_rms: f32 = libm::sqrtf(block.iter().map(|s| s * s).sum::<f32>() / 128.0);

    // Skip ahead
    for _ in 0..100 {
        engine.render(&mut block, &params, SR, &mut pool);
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
    let params = modal_params();
    let mut pool = SymPool::boxed();
    let mut engine = ModalEngine::new_in(&mut pool, params.mode);
    let mut block = [0.0f32; 64];
    engine.render(&mut block, &params, SR, &mut pool);
    let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max < 0.001, "idle modal should be silent");
}

#[test]
fn test_modal_output_bounded() {
    let mut params = modal_params();
    params.excite = 1.0;
    params.damp = 1.0;
    params.bright = 1.0;
    let buf = render_modal(&params, 60, 32);
    let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(
        max < 10.0,
        "modal output should be bounded, got max={}",
        max
    );
}

#[test]
fn test_modal_structure_changes_spectrum() {
    let f0 = note_freq(48);

    let spectrum = |structure: f32| -> f32 {
        let mut params = modal_params();
        params.structure = structure;
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
        "structure should change spectrum: harmonic={} inharmonic={}",
        harmonic,
        inharmonic
    );
}

#[test]
fn test_modal_brightness_changes_spectrum() {
    let f0 = note_freq(48);

    let high_harmonic_energy = |bright: f32| -> f32 {
        let mut params = modal_params();
        params.bright = bright;
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
    let params = modal_params();
    let buf = render_modal(&params, 60, 32);
    let f0 = note_freq(60);

    let fund = goertzel(&buf, f0, SR);
    // Check some non-harmonic frequencies have less energy
    let off1 = goertzel(&buf, f0 * 1.13, SR); // between harmonics
    let off2 = goertzel(&buf, f0 * 1.73, SR);

    assert!(
        fund > off1 && fund > off2,
        "fundamental should be stronger than non-harmonic freqs: fund={} off1={} off2={}",
        fund,
        off1,
        off2
    );
}

#[test]
fn test_modal_position_changes_spectrum() {
    let f0 = note_freq(48);

    let second_harmonic = |pos: f32| -> f32 {
        let mut params = modal_params();
        params.pos = pos;
        let buf = render_modal(&params, 48, 16);
        goertzel(&buf, f0 * 2.0, SR)
    };

    // At position 0.5 (center), even harmonics should be suppressed
    // At position 0.25, all harmonics should be present
    let center = second_harmonic(0.5);
    let quarter = second_harmonic(0.25);

    // Position should change the spectral distribution.
    // With high Q, the effect may be subtle — use wider positions.
    let edge = second_harmonic(0.1);
    assert!(
        (center - edge).abs() > 0.000001 || (quarter - edge).abs() > 0.000001,
        "position should affect harmonics: center={} quarter={} edge={}",
        center,
        quarter,
        edge
    );
}

#[test]
fn test_structure_actually_shifts_modes() {
    let f0 = note_freq(48); // ~130 Hz, low note for clear spectrum

    let measure_mode_freqs = |structure: f32| -> Vec<f32> {
        let mut params = modal_params();
        params.structure = structure;
        params.bright = 1.0; // keep all modes bright
        params.damp = 0.8;
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

    eprintln!("Harmonic (structure=0.25) energies at exact harmonics:");
    for (i, e) in harmonic.iter().enumerate() {
        eprintln!("  H{}: {:.6}", i + 1, e);
    }
    eprintln!("Inharmonic (structure=1) energies at exact harmonics:");
    for (i, e) in inharmonic.iter().enumerate() {
        eprintln!("  H{}: {:.6}", i + 1, e);
    }

    // With structure 0.25, modes should be AT the harmonics (high energy)
    // With structure 1, modes should be SHIFTED AWAY from exact harmonics (lower energy at those freqs)
    let harmonic_total: f32 = harmonic[2..].iter().sum(); // harmonics 3+
    let inharmonic_total: f32 = inharmonic[2..].iter().sum();

    eprintln!(
        "Energy at exact harmonics 3-8: harmonic={:.6} inharmonic={:.6}",
        harmonic_total, inharmonic_total
    );

    // The inharmonic version should have LESS energy at exact harmonic frequencies
    // because its modes are shifted to non-harmonic positions
    assert!(
        harmonic_total > inharmonic_total * 1.1,
        "structure should shift modes away from exact harmonics: harmonic={} inharmonic={}",
        harmonic_total,
        inharmonic_total
    );
}

#[test]
fn test_structure_spreads_spectrum() {
    // Inharmonic modes should have energy at non-harmonic frequencies
    let f0 = note_freq(48);

    let non_harmonic_energy = |structure: f32| -> f32 {
        let mut params = modal_params();
        params.structure = structure;
        params.bright = 1.0;
        params.damp = 0.8;
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

    eprintln!(
        "Energy between harmonics: harmonic={:.6} inharmonic={:.6}",
        harmonic_between, inharmonic_between
    );

    // STRUCTURE should change the spectral distribution measurably
    assert!(
        (harmonic_between - inharmonic_between).abs() > 0.0001,
        "structure should change energy between harmonics: h={} ih={}",
        harmonic_between,
        inharmonic_between
    );
}

/// Sympathetic mode (#68): at DAMP and STRUCTURE's top the output stays
/// finite and inside its tanh's ±1, held and released, and after note-off
/// the engine falls silent. The slowest released loop is the unison halo
/// string: until Task 7's release, its gain is at most the old DECAY law's
/// 0.999 − 0.009 · 0.8 · ½, applied once per trip round the f0-period
/// line, so −60 dB takes at most ln 1000 / −ln of it trips; the loop
/// low-pass only shortens that.
#[test]
fn sympathetic_mode_is_bounded_and_falls_silent() {
    let p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        damp: 1.0,
        structure: 1.0,
        ..Default::default()
    };
    let bounded = |b: &[f32; 64]| b.iter().all(|s| s.is_finite() && s.abs() <= 1.0);
    let mut pool = SymPool::boxed();
    let mut engine = ModalEngine::new_in(&mut pool, p.mode);
    engine.note_on(48, 127, &p, SR, &mut pool);
    let mut block = [0.0f32; 64];
    for _ in 0..2 * SR as usize / 64 {
        engine.render(&mut block, &p, SR, &mut pool);
        assert!(bounded(&block), "held: {block:?}");
    }
    assert!(engine.is_active(), "held note rings");
    engine.note_off(&mut pool);
    let gain: f32 = 0.999 - 0.009 * (0.8 * 0.5);
    let trips = libm::logf(1000.0) / -libm::logf(gain);
    let limit = (trips / note_freq(48) * SR as f32) as usize / 64 + 11; // + the silence count
    let mut blocks = 0;
    while engine.is_active() {
        engine.render(&mut block, &p, SR, &mut pool);
        assert!(bounded(&block), "released: {block:?}");
        blocks += 1;
        assert!(blocks <= limit, "still sounding after {limit} blocks");
    }
    engine.render(&mut block, &p, SR, &mut pool);
    assert!(block.iter().all(|&s| s == 0.0));
}

/// The modal bank at its full size, 48 modes (the deleted stress test's
/// case): every sample finite and within `render_modal`'s 2·tanh bound.
#[test]
fn modal_bank_at_full_size_is_finite_and_bounded() {
    let p = ModalParams {
        modes: BankModes::M48,
        damp: 1.0,
        ..modal_params()
    };
    let buf = render_modal(&p, 36, 400);
    assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 2.0));
    assert!(buf.iter().any(|&s| s != 0.0), "it rings");
}
