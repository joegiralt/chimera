use chimera_core::ui::page::ValFmt;

// ── Icon frame quantization ─────────────────────────────────────────

#[test]
fn test_frame_quantization_16_steps() {
    // 128 MIDI values / 8 = 16 frames (0-15)
    // Normalized 0.0 -> frame 0, 1.0 -> frame 15
    let frame_at = |val: f32| -> u8 { (val * 15.0) as u8 };

    assert_eq!(frame_at(0.0), 0);
    assert_eq!(frame_at(1.0), 15);
    assert_eq!(frame_at(0.5), 7);
    // Each frame spans ~8 MIDI values
    assert_eq!(frame_at(8.0 / 127.0), 0); // MIDI 8 -> still frame 0
    assert_eq!(frame_at(9.0 / 127.0), 1); // MIDI 9 -> frame 1
}

// ── ValFmt consistency ──────────────────────────────────────────────

#[test]
fn test_bipolar_params_use_bipolar_snaps() {
    // Every bipolar encoder should have 5 snap points centered on 0
    let snaps = ValFmt::Bi.snap_points();
    assert_eq!(snaps.len(), 5);

    // Center snap should be at MIDI 64 (normalized ~0.504)
    let center = snaps[2];
    assert!(
        (center - 64.0 / 127.0).abs() < 0.01,
        "bipolar center should be at MIDI 64"
    );
}

#[test]
fn test_unipolar_snaps_in_order() {
    let snaps = ValFmt::Uni.snap_points();
    for i in 1..snaps.len() {
        assert!(snaps[i] > snaps[i - 1], "snap points must be ascending");
    }
}

#[test]
fn test_bipolar_snaps_in_order() {
    let snaps = ValFmt::Bi.snap_points();
    for i in 1..snaps.len() {
        assert!(snaps[i] > snaps[i - 1], "snap points must be ascending");
    }
}

#[test]
fn test_bipolar_snaps_are_symmetric() {
    let snaps = ValFmt::Bi.snap_points();
    let center = snaps[2];
    // Distance from center to -44 should equal distance from center to +43
    let low_dist = center - snaps[1];
    let high_dist = snaps[3] - center;
    assert!(
        (low_dist - high_dist).abs() < 0.02,
        "bipolar snaps should be roughly symmetric: low={} high={}",
        low_dist,
        high_dist
    );
}

// ── fold_wave function ──────────────────────────────────────────────

#[test]
fn test_fold_wave_identity_in_range() {
    use chimera_core::ui::cell::fold_wave;
    // Values in -1..1 should pass through unchanged
    assert!((fold_wave(0.0) - 0.0).abs() < 0.01);
    assert!((fold_wave(0.5) - 0.5).abs() < 0.01);
    assert!((fold_wave(-0.5) - (-0.5)).abs() < 0.01);
}

#[test]
fn test_fold_wave_reflects() {
    use chimera_core::ui::cell::fold_wave;
    // Values beyond 1 should reflect back
    let v = fold_wave(1.5);
    assert!((v - 0.5).abs() < 0.01, "1.5 should fold to 0.5, got {}", v);

    let v = fold_wave(2.0);
    assert!((v - 0.0).abs() < 0.01, "2.0 should fold to 0.0, got {}", v);

    let v = fold_wave(-1.5);
    assert!(
        (v - (-0.5)).abs() < 0.01,
        "-1.5 should fold to -0.5, got {}",
        v
    );
}

#[test]
fn test_fold_wave_stays_bounded() {
    use chimera_core::ui::cell::fold_wave;
    // Any input should produce output in -1..1
    for i in -100..=100 {
        let input = i as f32 * 0.1;
        let output = fold_wave(input);
        assert!(
            output >= -1.0 && output <= 1.0,
            "fold_wave({}) = {} is out of range",
            input,
            output
        );
    }
}
