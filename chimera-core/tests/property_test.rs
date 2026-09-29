//! Property-based tests: verify invariants hold for random parameter combinations.
//! Uses a simple xorshift PRNG instead of proptest (no_std compatible).

mod common;
use common::Rig;
use common::{SR, tri};

use chimera_core::dsp::modal::{BankModes, ResonatorMode};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use common::expects_sound;

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
    // Engine type: every engine (spec § Testing "Engines")
    let mut p = ParamSnapshot::for_engine(
        EngineType::ALL[rng.u8(EngineType::ALL.len() as u8 - 1) as usize],
    );

    // Modal params
    p.modal.mode = ResonatorMode::from_u8(rng.u8(3)); // all four models
    p.modal.excite = rng.f32();
    p.modal.structure = rng.f32();
    p.modal.bright = rng.f32();
    p.modal.damp = rng.f32();
    p.modal.pos = rng.f32();
    p.modal.body = rng.f32();
    p.modal.ens_depth = rng.f32();
    p.modal.ens_rate = rng.f32();
    p.modal.ens_mix = rng.f32();
    p.modal.couple = rng.f32();
    p.modal.halo = rng.f32();
    p.modal.modes = BankModes::from_index(rng.u8(3));

    // Algo params (release 8–15 so a note ends inside the note-off property's window)
    p.algo.alg_a = rng.u8(31);
    p.algo.alg_b = rng.u8(31);
    p.algo.morph = rng.u8(127);
    for op in p.algo.ops.iter_mut() {
        op.wave = rng.u8(15);
        op.coarse = rng.u8(63);
        op.fine = rng.u8(15);
        op.level = rng.u8(99);
        op.feedback = rng.u8(7);
        op.ar = 20 + rng.u8(11);
        op.d1r = rng.u8(31);
        op.d1l = rng.u8(15);
        op.d2r = rng.u8(31);
        op.rr = 8 + rng.u8(7);
    }

    // Filter
    p.filter.cutoff = 20.0 + rng.f32() * 19980.0;
    p.filter.resonance = rng.f32();
    p.filter.drive = rng.f32();
    p.filter
        .set_mode(chimera_core::dsp::filter::FilterMode::ALL[rng.u8(7) as usize]);

    // Drive
    p.drive.drive = rng.f32();
    p.drive.tone = rng.f32();
    p.drive.mix = rng.f32();

    // Folder
    p.folder.fold = rng.f32();
    p.folder.symmetry = rng.f32();
    p.folder.mix = rng.f32();

    // Volume
    p.out.volume = 0.1 + rng.f32() * 0.9; // never zero

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

        let mut voice = Rig::new(SR);
        voice.note_on(
            MidiNote::new(note).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );

        let mut block = [0.0f32; 64];
        for _ in 0..8 {
            voice.render(&mut block, &params, &empty_mod);

            for (i, &s) in block.iter().enumerate() {
                assert!(
                    s.is_finite(),
                    "trial {}: output[{}] is not finite ({}) with engine={:?} note={}",
                    trial,
                    i,
                    s,
                    params.engine(),
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

        let mut voice = Rig::new(SR);
        voice.note_on(
            MidiNote::new(note).unwrap(),
            Velocity::new(127).unwrap(),
            &params,
        );

        let mut block = [0.0f32; 64];
        for _ in 0..16 {
            voice.render(&mut block, &params, &empty_mod);

            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(
                max < 20.0,
                "trial {}: output max {} is too large with engine={:?} note={}",
                trial,
                max,
                params.engine(),
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
        if !expects_sound(params.engine()) {
            continue;
        }

        let mut voice = Rig::new(SR);
        voice.note_on(
            MidiNote::new(note).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );

        let mut block = [0.0f32; 64];
        let mut total_max = 0.0f32;

        for _ in 0..16 {
            voice.render(&mut block, &params, &empty_mod);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            total_max = total_max.max(max);
        }

        assert!(
            total_max > 0.0001,
            "trial {}: note_on should produce sound, max={} engine={:?} modal_mode={:?} note={}",
            trial,
            total_max,
            params.engine(),
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
            3 => params_b.out.volume = params_a.out.volume * 0.2,
            // The engine's own parameter, where it has one.
            _ => match params_a.engine() {
                EngineType::Algo => params_b.algo.ops[0].level = 99 - params_a.algo.ops[0].level,
                EngineType::Modal => params_b.out.volume = params_a.out.volume * 0.2,
            },
        }

        let note = 60;
        if !expects_sound(params_a.engine()) {
            continue;
        }

        // Render A
        let mut voice_a = Rig::new(SR);
        voice_a.note_on(
            MidiNote::new(note).unwrap(),
            Velocity::new(100).unwrap(),
            &params_a,
        );
        let mut buf_a = [0.0f32; 64];
        for _ in 0..8 {
            voice_a.render(&mut buf_a, &params_a, &empty_mod);
        }

        // Render B
        let mut voice_b = Rig::new(SR);
        voice_b.note_on(
            MidiNote::new(note).unwrap(),
            Velocity::new(100).unwrap(),
            &params_b,
        );
        let mut buf_b = [0.0f32; 64];
        for _ in 0..8 {
            voice_b.render(&mut buf_b, &params_b, &empty_mod);
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
        params.modal.damp *= 0.2;
        // Force bowed mode (2) to not self-sustain
        if params.modal.mode == ResonatorMode::Bowed {
            params.modal.mode = ResonatorMode::Modal; // use resonator instead
        }
        params.filter.resonance *= 0.5; // prevent self-oscillation
        params.folder.fold = 0.0; // disable folder feedback path
        let note = rng.note();

        let mut voice = Rig::new(SR);
        voice.note_on(
            MidiNote::new(note).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );

        let mut block = [0.0f32; 64];
        // Play for a bit
        for _ in 0..4 {
            voice.render(&mut block, &params, &empty_mod);
        }
        // Note off
        voice.note_off();

        // Render until silent or max 1000 blocks (~2.6s)
        let mut silent = false;
        for _ in 0..1000 {
            voice.render(&mut block, &params, &empty_mod);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if max < 0.005 || !voice.is_active() {
                silent = true;
                break;
            }
        }

        assert!(
            silent,
            "trial {}: note_off should eventually silence, engine={:?} mode={:?}",
            trial,
            params.engine(),
            params.modal.mode
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

        let mut voice = Rig::new(SR);
        voice.note_on(
            MidiNote::new(60).unwrap(),
            Velocity::new(100).unwrap(),
            &params,
        );
        let mut block = [0.0f32; 64];
        // Render enough blocks for damping/decay differences to manifest
        for _ in 0..32 {
            voice.render(&mut block, &params, &empty_mod);
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
fn prop_algo_level_full_sweep() {
    verify_full_sweep(
        "Algo operator 1 LEVEL",
        |p| *p = ParamSnapshot::for_engine(EngineType::Algo),
        |p, v| p.algo.ops[0].level = (v * 99.0) as u8,
        16,
    );
}

#[test]
fn prop_filter_cutoff_full_sweep() {
    verify_full_sweep(
        "Filter cutoff",
        |p| {
            *p = tri();
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
            *p = tri();
            p.filter.cutoff = 1000.0;
            p.filter
                .set_mode(chimera_core::dsp::filter::FilterMode::Lp12);
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
            *p = tri();
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
            *p = tri();
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
            *p = tri();
        },
        |p, v| {
            p.out.volume = v;
        },
        16,
    );
}

#[test]
fn prop_string_body_full_sweep() {
    verify_full_sweep(
        "STRING BODY",
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
        },
        |p, v| {
            p.modal.body = v;
        },
        16,
    );
}

#[test]
fn prop_string_structure_full_sweep() {
    verify_full_sweep(
        "STRING STRUCTURE",
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
        },
        |p, v| {
            p.modal.structure = v;
        },
        16,
    );
}

#[test]
fn prop_string_bright_full_sweep() {
    verify_full_sweep(
        "STRING BRIGHT",
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
        },
        |p, v| {
            p.modal.bright = v;
        },
        16,
    );
}

#[test]
fn prop_bank_damp_full_sweep() {
    verify_full_sweep(
        "BANK DAMP",
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::Modal;
        },
        |p, v| {
            p.modal.damp = v;
        },
        16,
    );
}

#[test]
fn prop_bank_bright_full_sweep() {
    verify_full_sweep(
        "BANK BRIGHT",
        |p| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::Modal;
        },
        |p, v| {
            p.modal.bright = v;
        },
        16,
    );
}
