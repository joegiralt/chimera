//! Snap points of the value formats (shift + encoder).

use chimera_core::ui::page::ValFmt;

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
