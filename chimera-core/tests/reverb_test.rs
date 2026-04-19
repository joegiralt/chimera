use chimera_core::dsp::reverb::{Reverb, ReverbParams, ReverbType};

fn impulse_block() -> [f32; 128] {
    let mut block = [0.0f32; 128];
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
        let mut block = [0.0f32; 128];
        reverb.process(&mut block, &params);
        all.extend_from_slice(&block);
    }

    all
}

// ── All three reverbs produce output ────────────────────────────────

#[test]
fn test_plate_produces_tail() {
    let buf = render_reverb(0, 0.7, 1.0, 128);
    // Plate has long delay lines (up to 4782 samples = 100ms)
    // Check for tail after 200ms (9600 samples)
    let late_rms = rms(&buf[9600..]);
    assert!(late_rms > 0.001, "plate should have reverb tail: late_rms={}", late_rms);
}

#[test]
fn test_fdn_produces_tail() {
    let buf = render_reverb(1, 0.7, 1.0, 16);
    let late_rms = rms(&buf[1024..]);
    assert!(late_rms > 0.001, "FDN should have reverb tail: late_rms={}", late_rms);
}

#[test]
fn test_midiverb_produces_tail() {
    let buf = render_reverb(2, 0.7, 1.0, 16);
    let late_rms = rms(&buf[1024..]);
    assert!(late_rms > 0.001, "MidiVerb should have reverb tail: late_rms={}", late_rms);
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
                rt, i, s
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
            rt, max
        );
    }
}

// ── Longer time = longer tail ───────────────────────────────────────

#[test]
fn test_plate_longer_time_longer_tail() {
    let short = render_reverb(0, 0.3, 1.0, 128);
    let long = render_reverb(0, 0.9, 1.0, 128);

    let short_late = rms(&short[12800..]);
    let long_late = rms(&long[12800..]);

    assert!(
        long_late > short_late,
        "longer time should produce longer tail: short={} long={}",
        short_late, long_late
    );
}

#[test]
fn test_fdn_longer_time_longer_tail() {
    let short = render_reverb(1, 0.3, 1.0, 32);
    let long = render_reverb(1, 0.9, 1.0, 32);

    let short_late = rms(&short[2048..]);
    let long_late = rms(&long[2048..]);

    assert!(
        long_late > short_late,
        "longer time should produce longer tail: short={} long={}",
        short_late, long_late
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
                rt, i, a, b
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

    let diff_pf: f32 = plate.iter().zip(fdn.iter())
        .map(|(a, b)| (a - b).abs()).sum::<f32>() / plate.len() as f32;
    let diff_pm: f32 = plate.iter().zip(midiverb.iter())
        .map(|(a, b)| (a - b).abs()).sum::<f32>() / plate.len() as f32;

    assert!(diff_pf > 0.001, "plate vs FDN should differ: {}", diff_pf);
    assert!(diff_pm > 0.001, "plate vs MidiVerb should differ: {}", diff_pm);
}

// ── Reverb time sweep ───────────────────────────────────────────────

#[test]
fn test_reverb_time_full_sweep() {
    for rt in [0, 2] { // FDN has short delays — tail differences are subtle
        let mut prev_rms = -1.0f32;
        let mut changes = 0;
        let total = 8;

        for step in 0..=total {
            let time = step as f32 / total as f32;
            let buf = render_reverb(rt, time, 1.0, 128);
            let late = rms(&buf[buf.len()/2..]);

            if prev_rms >= 0.0 && (late - prev_rms).abs() > 0.0001 {
                changes += 1;
            }
            prev_rms = late;
        }

        assert!(
            changes >= total / 4,
            "reverb {} time sweep: only {}/{} steps changed",
            rt, changes, total
        );
    }
}
