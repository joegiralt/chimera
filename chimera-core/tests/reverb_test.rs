use chimera_core::{MidiNote, Velocity};
use chimera_core::modulation::ModState;
use chimera_core::dsp::reverb::{Reverb, ReverbParams, ReverbType};

fn impulse_block() -> [f32; 64] {
    let mut block = [0.0f32; 64];
    block[0] = 1.0;
    block
}

fn rms(buf: &[f32]) -> f32 {
    libm::sqrtf(buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32)
}

/// Render an impulse through a reverb and return several blocks of output.
fn render_reverb(reverb_type: u8, time: f32, mix: f32, blocks: usize) -> Vec<f32> {
    let mut reverb = Reverb::new();
    let params = ReverbParams {
        reverb_type,
        time,
        damping: 0.3,
        size: 0.5,
        mix,
    };

    let mut all = Vec::new();

    // First block: impulse
    let mut block = impulse_block();
    reverb.process(&mut block, &params);
    all.extend_from_slice(&block);

    // Subsequent blocks: silence input, reverb tail
    for _ in 1..blocks {
        let mut block = [0.0f32; 64];
        reverb.process(&mut block, &params);
        all.extend_from_slice(&block);
    }

    all
}

// ── All three reverbs produce output ────────────────────────────────

#[test]
fn test_plate_produces_tail() {
    let buf = render_reverb(0, 0.7, 1.0, 256);
    // Plate has long delay lines (up to 4782 samples = 100ms)
    // Check for tail after 200ms (9600 samples)
    let late_rms = rms(&buf[9600..]);
    assert!(
        late_rms > 0.001,
        "plate should have reverb tail: late_rms={}",
        late_rms
    );
}

#[test]
fn test_fdn_produces_tail() {
    let buf = render_reverb(1, 0.7, 1.0, 32);
    let late_rms = rms(&buf[1024..]);
    assert!(
        late_rms > 0.001,
        "FDN should have reverb tail: late_rms={}",
        late_rms
    );
}

#[test]
fn test_midiverb_produces_tail() {
    let buf = render_reverb(2, 0.7, 1.0, 32);
    let late_rms = rms(&buf[1024..]);
    assert!(
        late_rms > 0.001,
        "MidiVerb should have reverb tail: late_rms={}",
        late_rms
    );
}

// ── Output is always finite ─────────────────────────────────────────

#[test]
fn test_reverbs_output_finite() {
    for rt in 0..3 {
        let buf = render_reverb(rt, 0.95, 1.0, 32);
        for (i, &s) in buf.iter().enumerate() {
            assert!(
                s.is_finite(),
                "reverb type {} sample {} is not finite: {}",
                rt,
                i,
                s
            );
        }
    }
}

// ── Output is bounded ───────────────────────────────────────────────

#[test]
fn test_reverbs_output_bounded() {
    for rt in 0..3 {
        let buf = render_reverb(rt, 0.95, 1.0, 32);
        let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max < 10.0,
            "reverb type {} output too loud: max={}",
            rt,
            max
        );
    }
}

// ── Longer time = longer tail ───────────────────────────────────────

#[test]
fn test_plate_longer_time_longer_tail() {
    let short = render_reverb(0, 0.3, 1.0, 256);
    let long = render_reverb(0, 0.9, 1.0, 256);

    let short_late = rms(&short[12800..]);
    let long_late = rms(&long[12800..]);

    assert!(
        long_late > short_late,
        "longer time should produce longer tail: short={} long={}",
        short_late,
        long_late
    );
}

#[test]
fn test_fdn_longer_time_longer_tail() {
    let short = render_reverb(1, 0.3, 1.0, 64);
    let long = render_reverb(1, 0.9, 1.0, 64);

    let short_late = rms(&short[2048..]);
    let long_late = rms(&long[2048..]);

    assert!(
        long_late > short_late,
        "longer time should produce longer tail: short={} long={}",
        short_late,
        long_late
    );
}

// ── Mix at 0 = dry passthrough ──────────────────────────────────────

#[test]
fn test_reverb_dry_passthrough() {
    for rt in 0..3 {
        let mut reverb = Reverb::new();
        let params = ReverbParams {
            reverb_type: rt,
            time: 0.5,
            damping: 0.3,
            size: 0.5,
            mix: 0.0, // fully dry
        };

        let mut block = impulse_block();
        let original = block;
        reverb.process(&mut block, &params);

        for (i, (&a, &b)) in original.iter().zip(block.iter()).enumerate() {
            assert!(
                (a - b).abs() < 0.001,
                "reverb {} at mix=0 should passthrough: sample {} orig={} got={}",
                rt,
                i,
                a,
                b
            );
        }
    }
}

// ── Different reverb types sound different ──────────────────────────

#[test]
fn test_reverb_types_differ() {
    let plate = render_reverb(0, 0.7, 1.0, 128);
    let fdn = render_reverb(1, 0.7, 1.0, 128);
    let midiverb = render_reverb(2, 0.7, 1.0, 128);

    let diff_pf: f32 = plate
        .iter()
        .zip(fdn.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / plate.len() as f32;
    let diff_pm: f32 = plate
        .iter()
        .zip(midiverb.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / plate.len() as f32;

    assert!(diff_pf > 0.001, "plate vs FDN should differ: {}", diff_pf);
    assert!(
        diff_pm > 0.001,
        "plate vs MidiVerb should differ: {}",
        diff_pm
    );
}

// ── Reverb time sweep ───────────────────────────────────────────────

#[test]
fn test_reverb_time_full_sweep() {
    for rt in [0, 2] {
        // FDN has short delays — tail differences are subtle
        let mut prev_rms = -1.0f32;
        let mut changes = 0;
        let total = 8;

        for step in 0..=total {
            let time = step as f32 / total as f32;
            let buf = render_reverb(rt, time, 1.0, 256);
            let late = rms(&buf[buf.len() / 2..]);

            if prev_rms >= 0.0 && (late - prev_rms).abs() > 0.00001 {
                changes += 1;
            }
            prev_rms = late;
        }

        assert!(
            changes >= total / 4,
            "reverb {} time sweep: only {}/{} steps changed",
            rt,
            changes,
            total
        );
    }
}

// ── Live parameter change tests ─────────────────────────────────────

fn render_reverb_with_change(
    rt: u8,
    setup: impl FnOnce(&mut ReverbParams),
    tweak: impl FnOnce(&mut ReverbParams),
    blocks_before: usize,
    blocks_after: usize,
) -> (f32, f32) {
    let mut reverb = Reverb::new();
    let mut params = ReverbParams {
        reverb_type: rt,
        time: 0.5,
        damping: 0.3,
        size: 0.5,
        mix: 1.0,
    };
    setup(&mut params);

    // Feed impulse and render "before"
    let mut block = impulse_block();
    reverb.process(&mut block, &params);
    for _ in 1..blocks_before {
        let mut block = [0.0f32; 64];
        reverb.process(&mut block, &params);
    }
    let mut before_block = [0.0f32; 64];
    reverb.process(&mut before_block, &params);
    let before_rms = rms(&before_block);

    // Tweak parameter
    tweak(&mut params);

    // Render "after"
    for _ in 0..blocks_after {
        let mut block = [0.0f32; 64];
        reverb.process(&mut block, &params);
    }
    let mut after_block = [0.0f32; 64];
    reverb.process(&mut after_block, &params);
    let after_rms = rms(&after_block);

    (before_rms, after_rms)
}

#[test]
fn test_plate_time_mid_reverb() {
    let (before, after) = render_reverb_with_change(
        0,
        |p| {
            p.time = 0.9;
        },
        |p| {
            p.time = 0.1;
        },
        64,
        32,
    );
    assert!(
        (before - after).abs() > 0.0001 || after < before,
        "plate time should change tail: before={} after={}",
        before,
        after
    );
}

#[test]
fn test_fdn_time_mid_reverb() {
    let (before, after) = render_reverb_with_change(
        1,
        |p| {
            p.time = 0.9;
        },
        |p| {
            p.time = 0.1;
        },
        32,
        32,
    );
    assert!(
        (before - after).abs() > 0.00001 || after < before,
        "FDN time should change tail: before={} after={}",
        before,
        after
    );
}

#[test]
fn test_midiverb_time_mid_reverb() {
    let (before, after) = render_reverb_with_change(
        2,
        |p| {
            p.time = 0.9;
        },
        |p| {
            p.time = 0.1;
        },
        16,
        16,
    );
    assert!(
        (before - after).abs() > 0.0001 || after < before,
        "MidiVerb time should change tail: before={} after={}",
        before,
        after
    );
}

#[test]
fn test_plate_mix_mid_reverb() {
    let (before, after) = render_reverb_with_change(
        0,
        |p| {
            p.mix = 1.0;
        },
        |p| {
            p.mix = 0.0;
        },
        64,
        8,
    );
    assert!(
        after < before * 0.1 || after < 0.001,
        "plate mix=0 should be dry: before={} after={}",
        before,
        after
    );
}

#[test]
fn test_fdn_damping_mid_reverb() {
    let (before, after) = render_reverb_with_change(
        1,
        |p| {
            p.damping = 0.1;
        },
        |p| {
            p.damping = 0.9;
        },
        16,
        16,
    );
    assert!(
        (before - after).abs() > 0.00001,
        "FDN damping should change character: before={} after={}",
        before,
        after
    );
}

#[test]
fn test_fdn_size_mid_reverb() {
    let (before, after) = render_reverb_with_change(
        1,
        |p| {
            p.size = 0.2;
        },
        |p| {
            p.size = 0.9;
        },
        16,
        16,
    );
    assert!(
        (before - after).abs() > 0.0001,
        "FDN size should change character: before={} after={}",
        before,
        after
    );
}

// ── E2E: reverb through voice chain ─────────────────────────────────

#[test]
fn test_reverb_through_voice_produces_tail() {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::params::{EngineType, ParamSnapshot};

    let empty_mod = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    let mut reverb = Reverb::new();
    let params = ParamSnapshot::for_engine(EngineType::Pizza);
    let mut rv = chimera_core::dsp::fx_bus::FxParams::default().reverb;
    rv.mix = 0.5;
    rv.time = 0.7;

    voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);

    // Render a few blocks with note
    let mut block = [0.0f32; 64];
    for _ in 0..8 {
        voice.render(&mut block, &params, &empty_mod);
        reverb.process(&mut block, &rv);
    }

    // Note off
    voice.note_off();

    // Render more — reverb tail should persist after note ends
    let mut tail_energy = 0.0f32;
    for _ in 0..64 {
        voice.render(&mut block, &params, &empty_mod);
        reverb.process(&mut block, &rv);
        tail_energy += block.iter().map(|s| s * s).sum::<f32>();
    }

    assert!(
        tail_energy > 0.01,
        "reverb should produce tail after note off: energy={}",
        tail_energy
    );
}

#[test]
fn test_reverb_type_switch_e2e() {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::params::{EngineType, ParamSnapshot};

    let empty_mod = ModState::new();
    let render_with_reverb = |rt: u8| -> f32 {
        let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
        let mut reverb = Reverb::new();
        let params = ParamSnapshot::for_engine(EngineType::Pizza);
        let mut rv = chimera_core::dsp::fx_bus::FxParams::default().reverb;
        rv.reverb_type = rt;
        rv.mix = 0.8;
        rv.time = 0.6;

        voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);

        let mut block = [0.0f32; 64];
        let mut total = 0.0f32;
        for _ in 0..32 {
            voice.render(&mut block, &params, &empty_mod);
            reverb.process(&mut block, &rv);
            total += block.iter().map(|s| s * s).sum::<f32>();
        }
        total
    };

    let plate = render_with_reverb(0);
    let fdn = render_with_reverb(1);
    let midiverb = render_with_reverb(2);

    // All should produce energy
    assert!(plate > 0.1, "plate should produce sound: {}", plate);
    assert!(fdn > 0.1, "FDN should produce sound: {}", fdn);
    assert!(
        midiverb > 0.1,
        "MidiVerb should produce sound: {}",
        midiverb
    );

    // They should differ
    assert!((plate - fdn).abs() > 0.01, "plate vs FDN should differ");
    assert!(
        (plate - midiverb).abs() > 0.01,
        "plate vs MidiVerb should differ"
    );
}

// ── MidiVerb II faithful emulation tests ────────────────────────────

#[test]
fn test_midiverb_ii_all_programs_produce_output() {
    use chimera_core::dsp::midiverb::{MidiVerbII, MvProgram};

    for prog_idx in 0..8 {
        let prog = MvProgram::from_u8(prog_idx);
        let mut mv = MidiVerbII::new();
        let mut block = [0.0f32; 64];
        block[0] = 1.0; // impulse

        mv.process(&mut block, prog, 1.0);

        // Render enough blocks for long-delay programs (reverse needs 8000+ samples)
        let mut total_energy = 0.0f32;
        for _ in 0..128 {
            let mut b = [0.0f32; 64];
            mv.process(&mut b, prog, 1.0);
            total_energy += b.iter().map(|s| s * s).sum::<f32>();
        }

        assert!(
            total_energy > 0.0001,
            "MidiVerb II program {:?} should produce output, energy={}",
            prog, total_energy
        );
    }
}

#[test]
fn test_midiverb_ii_programs_sound_different() {
    use chimera_core::dsp::midiverb::{MidiVerbII, MvProgram};

    let render_program = |prog: MvProgram| -> Vec<f32> {
        let mut mv = MidiVerbII::new();
        let mut all = Vec::new();
        let mut block = [0.0f32; 64];
        block[0] = 1.0;
        mv.process(&mut block, prog, 1.0);
        all.extend_from_slice(&block);
        for _ in 0..16 {
            let mut b = [0.0f32; 64];
            mv.process(&mut b, prog, 1.0);
            all.extend_from_slice(&b);
        }
        all
    };

    let small = render_program(MvProgram::SmallBright);
    let large = render_program(MvProgram::LargeBright);

    let diff: f32 = small.iter().zip(large.iter())
        .map(|(a, b)| (a - b).abs()).sum::<f32>() / small.len() as f32;

    assert!(diff > 0.001, "Small vs Large should sound different: diff={}", diff);
}

#[test]
fn test_midiverb_ii_output_bounded() {
    use chimera_core::dsp::midiverb::{MidiVerbII, MvProgram};

    for prog_idx in 0..8 {
        let prog = MvProgram::from_u8(prog_idx);
        let mut mv = MidiVerbII::new();

        for _ in 0..64 {
            let mut block = [0.0f32; 64];
            block[0] = 1.0;
            mv.process(&mut block, prog, 1.0);
            let max = block.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            assert!(max < 20.0, "MidiVerb II {:?} output too hot: max={}", prog, max);
        }
    }
}

#[test]
fn test_midiverb_ii_output_finite() {
    use chimera_core::dsp::midiverb::{MidiVerbII, MvProgram};

    for prog_idx in 0..8 {
        let prog = MvProgram::from_u8(prog_idx);
        let mut mv = MidiVerbII::new();

        for _ in 0..32 {
            let mut block = [0.0f32; 64];
            block[0] = 0.5;
            mv.process(&mut block, prog, 1.0);
            for &s in &block {
                assert!(s.is_finite(), "MidiVerb II {:?} produced non-finite output", prog);
            }
        }
    }
}
