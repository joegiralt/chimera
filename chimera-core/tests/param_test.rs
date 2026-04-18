use chimera_core::params::Param;

#[test]
fn test_nudge_clamps_to_range() {
    let mut p = Param::new(0.0, 1.0, 0.5);
    p.nudge(10.0);
    assert_eq!(p.value, 1.0);
    p.nudge(-20.0);
    assert_eq!(p.value, 0.0);
}

#[test]
fn test_normalized() {
    let p = Param::new(20.0, 20000.0, 1000.0);
    let n = p.normalized();
    assert!((n - (1000.0 - 20.0) / (20000.0 - 20.0)).abs() < 0.001);
}

#[test]
fn test_set_normalized_roundtrip() {
    let mut p = Param::new(-1.0, 1.0, 0.0);
    p.set_normalized(0.75);
    assert!((p.normalized() - 0.75).abs() < 0.001);
}

#[test]
fn test_snap_to_unipolar_ascending() {
    // Snap points: 0, 100/127, 1.0
    let snaps: &[f32] = &[0.0, 100.0 / 127.0, 1.0];
    let mut p = Param::new(0.0, 1.0, 0.0);

    // From 0, snap up -> 100/127
    p.snap_to(1, snaps);
    assert!((p.normalized() - 100.0 / 127.0).abs() < 0.01);

    // From 100/127, snap up -> 1.0
    p.snap_to(1, snaps);
    assert!((p.normalized() - 1.0).abs() < 0.01);

    // Already at max, snap up stays at 1.0
    p.snap_to(1, snaps);
    assert!((p.normalized() - 1.0).abs() < 0.01);
}

#[test]
fn test_snap_to_unipolar_descending() {
    let snaps: &[f32] = &[0.0, 100.0 / 127.0, 1.0];
    let mut p = Param::new(0.0, 1.0, 1.0);

    // From 127, snap down -> 100/127
    p.snap_to(-1, snaps);
    assert!((p.normalized() - 100.0 / 127.0).abs() < 0.01);

    // From 100/127, snap down -> 0
    p.snap_to(-1, snaps);
    assert!((p.normalized() - 0.0).abs() < 0.01);

    // Already at min, snap down stays at 0
    p.snap_to(-1, snaps);
    assert!((p.normalized() - 0.0).abs() < 0.01);
}

#[test]
fn test_snap_to_bipolar() {
    // -64, -44, 0, +43, +63 → normalized 0, 20/127, 64/127, 107/127, 1.0
    let snaps: &[f32] = &[0.0, 20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0];
    let mut p = Param::new(0.0, 1.0, 0.0); // start at -64

    p.snap_to(1, snaps); // -> -44
    assert!((p.normalized() - 20.0 / 127.0).abs() < 0.01);

    p.snap_to(1, snaps); // -> 0
    assert!((p.normalized() - 64.0 / 127.0).abs() < 0.01);

    p.snap_to(1, snaps); // -> +43
    assert!((p.normalized() - 107.0 / 127.0).abs() < 0.01);

    p.snap_to(1, snaps); // -> +63
    assert!((p.normalized() - 1.0).abs() < 0.01);

    // Now descend all the way back
    p.snap_to(-1, snaps); // -> +43
    assert!((p.normalized() - 107.0 / 127.0).abs() < 0.01);

    p.snap_to(-1, snaps); // -> 0
    assert!((p.normalized() - 64.0 / 127.0).abs() < 0.01);

    p.snap_to(-1, snaps); // -> -44
    assert!((p.normalized() - 20.0 / 127.0).abs() < 0.01);

    p.snap_to(-1, snaps); // -> -64
    assert!((p.normalized() - 0.0).abs() < 0.01);
}

#[test]
fn test_snap_from_midpoint() {
    // Starting between snap points should jump to nearest in direction
    let snaps: &[f32] = &[0.0, 100.0 / 127.0, 1.0];
    let mut p = Param::new(0.0, 1.0, 0.5); // midpoint, between 0 and 100/127

    p.snap_to(1, snaps); // should jump to 100/127
    assert!((p.normalized() - 100.0 / 127.0).abs() < 0.01);

    p.set_normalized(0.5);
    p.snap_to(-1, snaps); // should jump to 0
    assert!((p.normalized() - 0.0).abs() < 0.01);
}
