use chimera_core::modulation::{ModState, MAX_MOD_SOURCES};
use chimera_core::ui::mod_grid::MatrixState;

#[test]
fn mod_state_default_is_empty() {
    let ms = ModState::new();
    assert_eq!(ms.num_sources, 0);
    assert_eq!(ms.num_dests, 0);
    for si in 0..MAX_MOD_SOURCES {
        for di in 0..16 {
            assert_eq!(ms.amounts[si][di], 0);
        }
    }
}

#[test]
fn mod_state_compute_offset_no_routes() {
    let ms = ModState::new();
    let sources = [0.0f32; MAX_MOD_SOURCES];
    let offset = ms.compute_offset(&sources, 0, 0);
    assert!((offset - 0.0).abs() < 1e-6, "no routes should return 0.0, got {}", offset);
}

#[test]
fn mod_state_compute_offset_single_route() {
    let mut ms = ModState::new();
    ms.num_sources = 1;
    ms.num_dests = 1;
    ms.dests[0] = (2, 0); // block 2, param 0 (filter cutoff)
    ms.amounts[0][0] = 64; // source 0 -> dest 0 at amount 64

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0; // source 0 fully on

    let offset = ms.compute_offset(&sources, 2, 0);
    let expected = 1.0 * (64.0 / 127.0);
    assert!(
        (offset - expected).abs() < 1e-5,
        "single route offset: expected {}, got {}",
        expected,
        offset
    );
}

#[test]
fn mod_state_compute_offset_multiple_sources() {
    let mut ms = ModState::new();
    ms.num_sources = 2;
    ms.num_dests = 1;
    ms.dests[0] = (1, 0); // drive amount
    ms.amounts[0][0] = 50;  // source 0 -> dest 0
    ms.amounts[1][0] = 100; // source 1 -> dest 0

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 0.5;
    sources[1] = -0.8;

    let offset = ms.compute_offset(&sources, 1, 0);
    let expected = 0.5 * (50.0 / 127.0) + (-0.8) * (100.0 / 127.0);
    assert!(
        (offset - expected).abs() < 1e-5,
        "multiple sources offset: expected {}, got {}",
        expected,
        offset
    );
}

#[test]
fn mod_state_compute_offset_negative_amount() {
    let mut ms = ModState::new();
    ms.num_sources = 1;
    ms.num_dests = 1;
    ms.dests[0] = (3, 1);
    ms.amounts[0][0] = -80;

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;

    let offset = ms.compute_offset(&sources, 3, 1);
    let expected = 1.0 * (-80.0 / 127.0);
    assert!(
        (offset - expected).abs() < 1e-5,
        "negative amount offset: expected {}, got {}",
        expected,
        offset
    );
    assert!(offset < 0.0, "negative amount should produce negative offset");
}

#[test]
fn mod_state_sync_from_matrix() {
    let mut matrix = MatrixState::new();
    // Enable two destinations via set_mod_enabled (which calls rebuild_dests internally)
    matrix.set_mod_enabled(0, 1, true); // block 0, param 1
    matrix.set_mod_enabled(2, 0, true); // block 2, param 0

    // Set up sources
    matrix.num_sources = 2;

    // Set amounts
    matrix.amounts[0][0] = 42;
    matrix.amounts[1][1] = -99;

    // Sync to ModState
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);

    assert_eq!(ms.num_sources, 2);
    assert_eq!(ms.num_dests, matrix.num_dests);
    assert_eq!(ms.mod_enabled, matrix.mod_enabled);

    // Verify amounts copied
    assert_eq!(ms.amounts[0][0], 42);
    assert_eq!(ms.amounts[1][1], -99);

    // Verify dests copied from matrix.dests
    for di in 0..matrix.num_dests {
        if let Some(dest) = &matrix.dests[di] {
            assert_eq!(ms.dests[di], (dest.block_idx, dest.param_idx));
        }
    }
}
