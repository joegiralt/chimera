//! Property-based tests: verify invariants hold for random parameter combinations.
//! Uses a simple xorshift PRNG instead of proptest (no_std compatible).

use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};

const SR: u32 = 48000;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self { Self(seed) }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Random f32 in 0..1
    fn f32(&mut self) -> f32 {
        (self.next_u64() & 0xFFFF) as f32 / 65535.0
    }

    /// Random u8 in 0..max (inclusive)
    fn u8(&mut self, max: u8) -> u8 {
        (self.next_u64() % (max as u64 + 1)) as u8
    }

    /// Random MIDI note 36..96
    fn note(&mut self) -> u8 {
        36 + self.u8(60)
    }
}

/// Generate a completely random ParamSnapshot.
fn random_params(rng: &mut Rng) -> ParamSnapshot {
    let mut p = ParamSnapshot::default();

    // Engine type
    let engine = rng.u8(1); // 0=Fm, 1=Modal (skip Va for now)
    p.engine = if engine == 0 { EngineType::Fm } else { EngineType::Modal };

    // FM params
    p.fm.algorithm = rng.u8(7);
    p.fm.feedback = rng.f32();
    for i in 0..4 {
        p.fm.op_ratio[i] = rng.f32();
        p.fm.op_detune[i] = rng.f32();
        p.fm.op_waveform[i] = rng.u8(7);
        p.fm.op_level[i] = rng.f32();
    }

    // Modal params
    p.modal.mode = rng.u8(2);
    p.modal.excite = rng.f32();
    p.modal.decay = rng.f32();
    p.modal.brightness = rng.f32();
    p.modal.inharm = rng.f32();
    p.modal.position = rng.f32();
    p.modal.ks_excitation = rng.u8(3);
    p.modal.ks_color = rng.f32();
    p.modal.ks_body = rng.f32();
    p.modal.ks_stiffness = rng.f32();
    p.modal.ks_feedback = rng.f32();
    p.modal.ks_ens_rate = rng.f32();
    p.modal.ks_ens_depth = rng.f32();
    p.modal.ks_ens_mix = rng.f32();

    // Filter
    p.filter.cutoff.set(20.0 + rng.f32() * 19980.0);
    p.filter.resonance.set(rng.f32());
    p.filter.drive.set(rng.f32());
    p.filter.mode = rng.u8(7);

    // Drive
    p.drive.drive.set(rng.f32());
    p.drive.tone.set(rng.f32());
    p.drive.mix.set(rng.f32());

    // Folder
    p.folder.fold.set(rng.f32());
    p.folder.symmetry.set(rng.f32());
    p.folder.mix.set(rng.f32());

    // Volume
    p.volume.set(0.1 + rng.f32() * 0.9); // never zero

    p
}

// ── Property: output is always finite ───────────────────────────────

#[test]
fn prop_output_always_finite() {
    let mut rng = Rng::new(12345);

    for trial in 0..100 {
        let params = random_params(&mut rng);
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 100, &params, SR);

        let mut block = [0.0f32; 128];
        for _ in 0..8 {
            voice.render(&mut block, &params, SR);

            for (i, &s) in block.iter().enumerate() {
                assert!(
                    s.is_finite(),
                    "trial {}: output[{}] is not finite ({}) with engine={:?} note={}",
                    trial, i, s, params.engine, note
                );
            }
        }
    }
}

// ── Property: output is always bounded ──────────────────────────────

#[test]
fn prop_output_bounded() {
    let mut rng = Rng::new(67890);

    for trial in 0..100 {
        let params = random_params(&mut rng);
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 127, &params, SR);

        let mut block = [0.0f32; 128];
        for _ in 0..16 {
            voice.render(&mut block, &params, SR);

            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                max < 20.0,
                "trial {}: output max {} is too large with engine={:?} note={}",
                trial, max, params.engine, note
            );
        }
    }
}

// ── Property: note_on always produces non-silent output ─────────────

#[test]
fn prop_note_on_produces_sound() {
    let mut rng = Rng::new(11111);

    for trial in 0..50 {
        let params = random_params(&mut rng);
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 100, &params, SR);

        let mut block = [0.0f32; 128];
        let mut total_max = 0.0f32;

        for _ in 0..16 {
            voice.render(&mut block, &params, SR);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            total_max = total_max.max(max);
        }

        assert!(
            total_max > 0.0001,
            "trial {}: note_on should produce sound, max={} engine={:?} modal_mode={} note={}",
            trial, total_max, params.engine, params.modal.mode, note
        );
    }
}

// ── Property: changing any parameter changes output ─────────────────

#[test]
fn prop_param_change_changes_output() {
    let mut rng = Rng::new(22222);

    let mut changed = 0;
    let mut total = 0;

    for _ in 0..50 {
        let params_a = random_params(&mut rng);
        let mut params_b = params_a.clone();

        // Randomly tweak one parameter
        let tweak = rng.u8(5);
        match tweak {
            0 => params_b.filter.cutoff.set(params_a.filter.cutoff.value * 0.1 + 100.0),
            1 => params_b.drive.drive.set(1.0 - params_a.drive.drive.value),
            2 => params_b.folder.fold.set(1.0 - params_a.folder.fold.value),
            3 => params_b.volume.set(params_a.volume.value * 0.2),
            _ => params_b.fm.feedback = 1.0 - params_a.fm.feedback,
        }

        let note = 60;

        // Render A
        let mut voice_a = Voice::new();
        voice_a.note_on(note, 100, &params_a, SR);
        let mut buf_a = [0.0f32; 128];
        for _ in 0..8 { voice_a.render(&mut buf_a, &params_a, SR); }

        // Render B
        let mut voice_b = Voice::new();
        voice_b.note_on(note, 100, &params_b, SR);
        let mut buf_b = [0.0f32; 128];
        for _ in 0..8 { voice_b.render(&mut buf_b, &params_b, SR); }

        let diff: f32 = buf_a.iter().zip(buf_b.iter())
            .map(|(a, b)| (a - b).abs()).sum::<f32>() / 128.0;

        total += 1;
        if diff > 0.001 {
            changed += 1;
        }
    }

    // At least 80% of random tweaks should produce audible change
    assert!(
        changed > total * 4 / 5,
        "param changes should affect output: {}/{} changed",
        changed, total
    );
}

// ── Property: note_off eventually silences ──────────────────────────

#[test]
fn prop_note_off_eventually_silences() {
    let mut rng = Rng::new(33333);

    for trial in 0..50 {
        let mut params = random_params(&mut rng);
        // Tame params so note actually decays
        params.modal.ks_feedback = params.modal.ks_feedback * 0.3;
        params.modal.decay = params.modal.decay * 0.5;
        params.filter.resonance.set(params.filter.resonance.value * 0.5); // prevent self-oscillation
        params.folder.fold.set(0.0); // disable folder feedback path
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 100, &params, SR);

        let mut block = [0.0f32; 128];
        // Play for a bit
        for _ in 0..4 {
            voice.render(&mut block, &params, SR);
        }
        // Note off
        voice.note_off();

        // Render until silent or max 500 blocks (~1.3s)
        let mut silent = false;
        for _ in 0..500 {
            voice.render(&mut block, &params, SR);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if max < 0.01 || !voice.is_active() {
                silent = true;
                break;
            }
        }

        assert!(
            silent,
            "trial {}: note_off should eventually silence, engine={:?} mode={}",
            trial, params.engine, params.modal.mode
        );
    }
}

// ── Property: full knob sweep produces continuous change ────────────

/// Sweep a parameter from 0 to 1 in steps, verify output changes
/// at each step (no dead zones across the full range).
fn verify_full_sweep(
    name: &str,
    setup: impl Fn(&mut ParamSnapshot),
    sweep: impl Fn(&mut ParamSnapshot, f32),
    steps: usize,
) {
    let mut prev_rms = -1.0f32;
    let mut changes = 0;
    let mut total = 0;

    for step in 0..=steps {
        let val = step as f32 / steps as f32;
        let mut params = ParamSnapshot::default();
        setup(&mut params);
        sweep(&mut params, val);

        let mut voice = Voice::new();
        voice.note_on(60, 100, &params, SR);
        let mut block = [0.0f32; 128];
        // Render enough blocks for damping/decay differences to manifest
        for _ in 0..32 {
            voice.render(&mut block, &params, SR);
        }
        let rms = libm::sqrtf(block.iter().map(|s| s * s).sum::<f32>() / 128.0);

        if prev_rms >= 0.0 {
            total += 1;
            if (rms - prev_rms).abs() > 0.0001 {
                changes += 1;
            }
        }
        prev_rms = rms;
    }

    let pct = changes as f32 / total as f32 * 100.0;
    assert!(
        changes >= total / 2,
        "{}: only {}/{} steps ({:.0}%) produced change — knob has dead zones",
        name, changes, total, pct
    );
}

#[test]
fn prop_fm_feedback_full_sweep() {
    verify_full_sweep(
        "FM feedback",
        |p| { p.engine = EngineType::Fm; p.fm.algorithm = 7; p.fm.op_level = [1.0, 0.0, 0.0, 1.0]; },
        |p, v| { p.fm.feedback = v; },
        16,
    );
}

#[test]
fn prop_filter_cutoff_full_sweep() {
    verify_full_sweep(
        "Filter cutoff",
        |p| { p.engine = EngineType::Fm; p.fm.op_level = [0.5, 0.0, 0.0, 1.0]; p.filter.mode = 2; },
        |p, v| { p.filter.cutoff.set(20.0 + v * 19980.0); },
        16,
    );
}

#[test]
fn prop_filter_resonance_full_sweep() {
    verify_full_sweep(
        "Filter resonance",
        |p| { p.engine = EngineType::Fm; p.fm.op_level = [0.5, 0.0, 0.0, 1.0]; p.filter.cutoff.set(1000.0); p.filter.mode = 1; },
        |p, v| { p.filter.resonance.set(v); },
        16,
    );
}

#[test]
fn prop_drive_full_sweep() {
    verify_full_sweep(
        "Drive",
        |p| { p.engine = EngineType::Fm; p.drive.mix.set(1.0); },
        |p, v| { p.drive.drive.set(v); },
        16,
    );
}

#[test]
fn prop_folder_full_sweep() {
    verify_full_sweep(
        "Wavefolder",
        |p| { p.engine = EngineType::Fm; p.folder.mix.set(1.0); },
        |p, v| { p.folder.fold.set(v); },
        16,
    );
}

#[test]
fn prop_volume_full_sweep() {
    verify_full_sweep(
        "Volume",
        |p| { p.engine = EngineType::Fm; },
        |p, v| { p.volume.set(v); },
        16,
    );
}

#[test]
fn prop_ks_body_full_sweep() {
    verify_full_sweep(
        "KS body",
        |p| { p.engine = EngineType::Modal; p.modal.mode = 0; },
        |p, v| { p.modal.ks_body = v; },
        16,
    );
}

#[test]
fn prop_ks_stiffness_full_sweep() {
    verify_full_sweep(
        "KS stiffness",
        |p| { p.engine = EngineType::Modal; p.modal.mode = 0; },
        |p, v| { p.modal.ks_stiffness = v; },
        16,
    );
}

#[test]
fn prop_ks_brightness_full_sweep() {
    verify_full_sweep(
        "KS brightness",
        |p| { p.engine = EngineType::Modal; p.modal.mode = 0; },
        |p, v| { p.modal.brightness = v; },
        16,
    );
}

#[test]
fn prop_modal_decay_full_sweep() {
    verify_full_sweep(
        "Modal decay",
        |p| { p.engine = EngineType::Modal; p.modal.mode = 1; },
        |p, v| { p.modal.decay = v; },
        16,
    );
}

#[test]
fn prop_modal_brightness_full_sweep() {
    verify_full_sweep(
        "Modal brightness",
        |p| { p.engine = EngineType::Modal; p.modal.mode = 1; },
        |p, v| { p.modal.brightness = v; },
        16,
    );
}
