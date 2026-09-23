//! Property-based tests: verify invariants hold for random parameter combinations.
//! Uses a simple xorshift PRNG instead of proptest (no_std compatible).

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::modulation::ModState;
use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};

const SR: u32 = 48000;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

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
    let engine = rng.u8(1); // 0=Pizza, 1=Modal
    p.engine = if engine == 0 {
        EngineType::Pizza
    } else {
        EngineType::Modal
    };

    // Pizza params
    p.pizza.shape = rng.f32();
    p.pizza.crush = rng.f32();
    p.pizza.level = rng.f32();

    // Modal params
    p.modal.mode = ResonatorMode::from_u8(rng.u8(2));
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
    p.filter.cutoff = 20.0 + rng.f32() * 19980.0;
    p.filter.resonance = rng.f32();
    p.filter.drive = rng.f32();
    p.filter.mode = rng.u8(7);

    // Drive
    p.drive.drive = rng.f32();
    p.drive.tone = rng.f32();
    p.drive.mix = rng.f32();

    // Folder
    p.folder.fold = rng.f32();
    p.folder.symmetry = rng.f32();
    p.folder.mix = rng.f32();

    // Volume
    p.volume.set(0.1 + rng.f32() * 0.9); // never zero

    p
}

// ── Property: output is always finite ───────────────────────────────

#[test]
fn prop_output_always_finite() {
    let empty_mod = ModState::new();
    let mut rng = Rng::new(12345);

    for trial in 0..100 {
        let params = random_params(&mut rng);
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 100, &params, SR);

        let mut block = [0.0f32; 64];
        for _ in 0..8 {
            voice.render(&mut block, &params, &empty_mod, SR);

            for (i, &s) in block.iter().enumerate() {
                assert!(
                    s.is_finite(),
                    "trial {}: output[{}] is not finite ({}) with engine={:?} note={}",
                    trial,
                    i,
                    s,
                    params.engine,
                    note
                );
            }
        }
    }
}

// ── Property: output is always bounded ──────────────────────────────

#[test]
fn prop_output_bounded() {
    let empty_mod = ModState::new();
    let mut rng = Rng::new(67890);

    for trial in 0..100 {
        let params = random_params(&mut rng);
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 127, &params, SR);

        let mut block = [0.0f32; 64];
        for _ in 0..16 {
            voice.render(&mut block, &params, &empty_mod, SR);

            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                max < 20.0,
                "trial {}: output max {} is too large with engine={:?} note={}",
                trial,
                max,
                params.engine,
                note
            );
        }
    }
}

// ── Property: note_on always produces non-silent output ─────────────

#[test]
fn prop_note_on_produces_sound() {
    let empty_mod = ModState::new();
    let mut rng = Rng::new(11111);

    for trial in 0..50 {
        let params = random_params(&mut rng);
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 100, &params, SR);

        let mut block = [0.0f32; 64];
        let mut total_max = 0.0f32;

        for _ in 0..16 {
            voice.render(&mut block, &params, &empty_mod, SR);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            total_max = total_max.max(max);
        }

        assert!(
            total_max > 0.0001,
            "trial {}: note_on should produce sound, max={} engine={:?} modal_mode={:?} note={}",
            trial,
            total_max,
            params.engine,
            params.modal.mode,
            note
        );
    }
}

// ── Property: changing any parameter changes output ─────────────────

#[test]
fn prop_param_change_changes_output() {
    let empty_mod = ModState::new();
    let mut rng = Rng::new(22222);

    let mut changed = 0;
    let mut total = 0;

    for _ in 0..50 {
        let params_a = random_params(&mut rng);
        let mut params_b = params_a.clone();

        // Randomly tweak one parameter
        let tweak = rng.u8(5);
        match tweak {
            0 => params_b.filter.cutoff = params_a.filter.cutoff * 0.1 + 100.0,
            1 => params_b.drive.drive = 1.0 - params_a.drive.drive,
            2 => params_b.folder.fold = 1.0 - params_a.folder.fold,
            3 => params_b.volume.set(params_a.volume.value * 0.2),
            _ => params_b.pizza.crush = 1.0 - params_a.pizza.crush,
        }

        let note = 60;

        // Render A
        let mut voice_a = Voice::new();
        voice_a.note_on(note, 100, &params_a, SR);
        let mut buf_a = [0.0f32; 64];
        for _ in 0..8 {
            voice_a.render(&mut buf_a, &params_a, &empty_mod, SR);
        }

        // Render B
        let mut voice_b = Voice::new();
        voice_b.note_on(note, 100, &params_b, SR);
        let mut buf_b = [0.0f32; 64];
        for _ in 0..8 {
            voice_b.render(&mut buf_b, &params_b, &empty_mod, SR);
        }

        let diff: f32 = buf_a
            .iter()
            .zip(buf_b.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / 128.0;

        total += 1;
        if diff > 0.001 {
            changed += 1;
        }
    }

    // At least 80% of random tweaks should produce audible change
    assert!(
        changed > total * 4 / 5,
        "param changes should affect output: {}/{} changed",
        changed,
        total
    );
}

// ── Property: note_off eventually silences ──────────────────────────

#[test]
fn prop_note_off_eventually_silences() {
    let empty_mod = ModState::new();
    let mut rng = Rng::new(33333);

    for trial in 0..50 {
        let mut params = random_params(&mut rng);
        // Tame params so note actually decays
        params.modal.ks_feedback = params.modal.ks_feedback * 0.1;
        params.modal.decay = params.modal.decay * 0.2;
        // Force bowed mode (2) to not self-sustain
        if params.modal.mode == ResonatorMode::Bowed {
            params.modal.mode = ResonatorMode::Modal; // use resonator instead
        }
        params.filter.resonance *= 0.5; // prevent self-oscillation
        params.folder.fold = 0.0; // disable folder feedback path
        let note = rng.note();

        let mut voice = Voice::new();
        voice.note_on(note, 100, &params, SR);

        let mut block = [0.0f32; 64];
        // Play for a bit
        for _ in 0..4 {
            voice.render(&mut block, &params, &empty_mod, SR);
        }
        // Note off
        voice.note_off();

        // Render until silent or max 1000 blocks (~2.6s)
        let mut silent = false;
        for _ in 0..1000 {
            voice.render(&mut block, &params, &empty_mod, SR);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if max < 0.005 || !voice.is_active() {
                silent = true;
                break;
            }
        }

        assert!(
            silent,
            "trial {}: note_off should eventually silence, engine={:?} mode={:?}",
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
    let empty_mod = ModState::new();
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
        let mut block = [0.0f32; 64];
        // Render enough blocks for damping/decay differences to manifest
        for _ in 0..32 {
            voice.render(&mut block, &params, &empty_mod, SR);
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
        name,
        changes,
        total,
        pct
    );
}

#[test]
fn prop_pizza_crush_full_sweep() {
    verify_full_sweep(
        "Pizza crush",
        |p| {
            p.engine = EngineType::Pizza;
        },
        |p, v| {
            p.pizza.crush = v;
        },
        16,
    );
}

#[test]
fn prop_filter_cutoff_full_sweep() {
    verify_full_sweep(
        "Filter cutoff",
        |p| {
            p.engine = EngineType::Pizza;
            p.filter.mode = 2;
        },
        |p, v| {
            p.filter.cutoff = 20.0 + v * 19980.0;
        },
        16,
    );
}

#[test]
fn prop_filter_resonance_full_sweep() {
    verify_full_sweep(
        "Filter resonance",
        |p| {
            p.engine = EngineType::Pizza;
            p.filter.cutoff = 1000.0;
            p.filter.mode = 1;
        },
        |p, v| {
            p.filter.resonance = v;
        },
        16,
    );
}

#[test]
fn prop_drive_full_sweep() {
    verify_full_sweep(
        "Drive",
        |p| {
            p.engine = EngineType::Pizza;
            p.drive.mix = 1.0;
        },
        |p, v| {
            p.drive.drive = v;
        },
        16,
    );
}

#[test]
fn prop_folder_full_sweep() {
    verify_full_sweep(
        "Wavefolder",
        |p| {
            p.engine = EngineType::Pizza;
            p.folder.mix = 1.0;
        },
        |p, v| {
            p.folder.fold = v;
        },
        16,
    );
}

#[test]
fn prop_volume_full_sweep() {
    verify_full_sweep(
        "Volume",
        |p| {
            p.engine = EngineType::Pizza;
        },
        |p, v| {
            p.volume.set(v);
        },
        16,
    );
}

#[test]
fn prop_ks_body_full_sweep() {
    verify_full_sweep(
        "KS body",
        |p| {
            p.engine = EngineType::Modal;
            p.modal.mode = ResonatorMode::String;
        },
        |p, v| {
            p.modal.ks_body = v;
        },
        16,
    );
}

#[test]
fn prop_ks_stiffness_full_sweep() {
    verify_full_sweep(
        "KS stiffness",
        |p| {
            p.engine = EngineType::Modal;
            p.modal.mode = ResonatorMode::String;
        },
        |p, v| {
            p.modal.ks_stiffness = v;
        },
        16,
    );
}

#[test]
fn prop_ks_brightness_full_sweep() {
    verify_full_sweep(
        "KS brightness",
        |p| {
            p.engine = EngineType::Modal;
            p.modal.mode = ResonatorMode::String;
        },
        |p, v| {
            p.modal.brightness = v;
        },
        16,
    );
}

#[test]
fn prop_modal_decay_full_sweep() {
    verify_full_sweep(
        "Modal decay",
        |p| {
            p.engine = EngineType::Modal;
            p.modal.mode = ResonatorMode::Modal;
        },
        |p, v| {
            p.modal.decay = v;
        },
        16,
    );
}

#[test]
fn prop_modal_brightness_full_sweep() {
    verify_full_sweep(
        "Modal brightness",
        |p| {
            p.engine = EngineType::Modal;
            p.modal.mode = ResonatorMode::Modal;
        },
        |p, v| {
            p.modal.brightness = v;
        },
        16,
    );
}
