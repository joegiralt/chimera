use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{MAX_MOD_DESTS, MAX_MOD_SOURCES, ModState};
use chimera_core::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FolderParams};
use chimera_core::ui::mod_grid::MatrixState;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

/// A ModState with one dest and `n` sources.
fn one_dest(addr: ParamAddr, n: usize) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(addr, *b"TEST\0\0\0\0").unwrap();
    ModState::from_registry(&reg, n)
}

/// Every modulatable address (25).
fn all_modulatable() -> Vec<ParamAddr> {
    BlockRef::ALL
        .iter()
        .flat_map(|&b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|a| a.modulatable())
        .collect()
}

#[test]
fn mod_state_default_is_empty() {
    let ms = ModState::new();
    assert_eq!(ms.num_sources(), 0);
    assert_eq!(ms.num_dests(), 0);
    for si in 0..MAX_MOD_SOURCES {
        for di in 0..MAX_MOD_DESTS {
            assert_eq!(ms.amount(si, di), 0);
        }
    }
}

#[test]
fn mod_state_offset_no_routes() {
    let ms = ModState::new();
    let sources = [0.0f32; MAX_MOD_SOURCES];
    assert_eq!(
        ms.offset_for(
            ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE),
            &sources
        ),
        0.0
    );
}

#[test]
fn mod_state_offset_single_route() {
    let mut ms = one_dest(CUTOFF, 1);
    ms.set_amount(0, 0, 64);
    assert_eq!(ms.dest(0), CUTOFF);

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.offset_for(CUTOFF, &sources);
    let expected = 1.0 * (64.0 / 127.0);
    assert!(
        (offset - expected).abs() < 1e-5,
        "expected {expected}, got {offset}"
    );
    assert_eq!(ms.sum_for(0, &sources), offset);
}

#[test]
fn mod_state_offset_multiple_sources() {
    let mut ms = one_dest(ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE), 2);
    ms.set_amount(0, 0, 50);
    ms.set_amount(1, 0, 100);

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 0.5;
    sources[1] = -0.8;
    let offset = ms.sum_for(0, &sources);
    let expected = 0.5 * (50.0 / 127.0) + (-0.8) * (100.0 / 127.0);
    assert!(
        (offset - expected).abs() < 1e-5,
        "expected {expected}, got {offset}"
    );
}

#[test]
fn mod_state_offset_negative_amount() {
    let mut ms = one_dest(ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY), 1);
    ms.set_amount(0, 0, -80);

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.sum_for(0, &sources);
    assert!((offset - (-80.0 / 127.0)).abs() < 1e-5);
    assert!(offset < 0.0);
}

#[test]
fn mod_state_set_amount_ignores_out_of_range() {
    let mut ms = one_dest(ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE), 2);
    ms.set_amount(5, 0, 99); // no such source
    ms.set_amount(0, 3, 99); // no such dest
    assert_eq!(ms.amount(5, 0), 0);
    assert_eq!(ms.amount(0, 3), 0);
}

#[test]
fn mod_state_sync_from_matrix() {
    let crush = ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH);
    let mut registry = ModDestRegistry::new();
    registry.add(crush, *b"A  p\0\0\0\0").unwrap();
    registry.add(CUTOFF, *b"B  q\0\0\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 2;
    matrix.amounts[0][0] = 42;
    matrix.amounts[1][1] = -99;

    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);

    assert_eq!(ms.num_sources(), 2);
    assert_eq!(ms.num_dests(), 2);
    assert_eq!(ms.amount(0, 0), 42);
    assert_eq!(ms.amount(1, 1), -99);
    assert_eq!(ms.dest(0), crush);
    assert_eq!(ms.dest(1), CUTOFF);
}

/// Review Focus 2: more destinations than fit keep amounts aligned.
#[test]
fn mod_state_truncates_without_misaligning() {
    let mut registry = ModDestRegistry::new();
    for a in all_modulatable() {
        registry.add(a, *b"X\0\0\0\0\0\0\0").unwrap();
    }
    assert_eq!(registry.len(), 25);
    assert_eq!(
        ModState::from_registry(&registry, 2).num_dests(),
        MAX_MOD_DESTS
    );

    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry); // matrix keeps 16 too
    matrix.num_sources = 2;
    matrix.amounts[1][15] = 77;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);
    assert_eq!(ms.num_dests(), MAX_MOD_DESTS);
    assert_eq!(ms.amount(1, 15), 77);
    assert_eq!(Some(ms.dest(15)), matrix.dests[15].map(|d| d.addr));
}

/// Review Focus 2: a matrix with more than MAX_MOD_SOURCES rows is clamped
/// (the old code indexed `amounts[si]` past 8 in the audio thread).
#[test]
fn mod_state_clamps_sources() {
    let mut registry = ModDestRegistry::new();
    registry.add(CUTOFF, *b"X\0\0\0\0\0\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 16;
    matrix.amounts[12][0] = 50;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);
    assert_eq!(ms.num_sources(), MAX_MOD_SOURCES);
    let values = [1.0f32; MAX_MOD_SOURCES];
    assert_eq!(ms.sum_for(0, &values), 0.0); // row 12 was dropped, nothing panicked
}

/// `dest`/`sum_for` are indexed by the audio ISR (`Voice::render`) with `d`
/// derived from `num_dests()`; an out-of-range `d` must never panic (hard
/// fault on the STM32), just return the unused sentinel / zero offset.
#[test]
fn dest_and_sum_for_never_panic_out_of_range() {
    let ms = ModState::new();
    let sentinel = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);
    let sources = [1.0f32; MAX_MOD_SOURCES];
    assert_eq!(ms.dest(16), sentinel);
    assert_eq!(ms.dest(255), sentinel);
    assert_eq!(ms.sum_for(16, &sources), 0.0);
    assert_eq!(ms.sum_for(255, &sources), 0.0);
}

/// The matrix holds addresses, so a route to an FM operator stays on that
/// operator (and the amp envelope is a destination like any other).
#[test]
fn matrix_dests_are_semantic() {
    let op_c = ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK);
    let atk = ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK);
    let mut registry = ModDestRegistry::new();
    registry.add(op_c, *b"O3 FDBK\0").unwrap();
    registry.add(atk, *b"ENVATK\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    assert_eq!(matrix.mod_info_for(op_c), Some(0.0));
    assert_eq!(matrix.mod_info_for(atk), Some(0.0));
    assert_eq!(
        matrix.mod_info_for(ParamAddr::new(BlockRef::FmOp(Op::D), FmOpParams::FEEDBACK)),
        None
    );
}
