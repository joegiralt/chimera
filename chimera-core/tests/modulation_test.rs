use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::mod_path::{ModDestRegistry, ParamPath};
use chimera_core::modulation::{ModState, MAX_MOD_DESTS, MAX_MOD_SOURCES};
use chimera_core::params::{DriveParams, FilterParams, FolderParams};
use chimera_core::preset::ChainType;
use chimera_core::ui::mod_grid::MatrixState;

const PIZZA: ChainType = ChainType::PizzaPoly;

/// A ModState with one dest (`path` on the Pizza chain) and `n` sources.
fn one_dest(path: ParamPath, n: usize) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(PIZZA, path, *b"TEST\0\0\0\0").unwrap();
    ModState::from_registry(&reg, PIZZA, n)
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
    let addr = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);
    assert_eq!(ms.offset_for(addr, &sources), 0.0);
}

#[test]
fn mod_state_offset_single_route() {
    let mut ms = one_dest(ParamPath::Block { block: 2, param: 0 }, 1); // filter cutoff
    ms.set_amount(0, 0, 64);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF));

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.offset_for(ms.dest(0), &sources);
    let expected = 1.0 * (64.0 / 127.0);
    assert!((offset - expected).abs() < 1e-5, "expected {expected}, got {offset}");
    assert_eq!(ms.sum_for(0, &sources), offset);
}

#[test]
fn mod_state_offset_multiple_sources() {
    let mut ms = one_dest(ParamPath::Block { block: 1, param: 0 }, 2); // drive amount
    ms.set_amount(0, 0, 50);
    ms.set_amount(1, 0, 100);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE));

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 0.5;
    sources[1] = -0.8;
    let offset = ms.sum_for(0, &sources);
    let expected = 0.5 * (50.0 / 127.0) + (-0.8) * (100.0 / 127.0);
    assert!((offset - expected).abs() < 1e-5, "expected {expected}, got {offset}");
}

#[test]
fn mod_state_offset_negative_amount() {
    let mut ms = one_dest(ParamPath::Block { block: 3, param: 1 }, 1); // folder symmetry
    ms.set_amount(0, 0, -80);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY));

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.sum_for(0, &sources);
    assert!((offset - (-80.0 / 127.0)).abs() < 1e-5);
    assert!(offset < 0.0);
}

#[test]
fn mod_state_set_amount_ignores_out_of_range() {
    let mut ms = one_dest(ParamPath::Block { block: 0, param: 0 }, 2);
    ms.set_amount(5, 0, 99); // no such source
    ms.set_amount(0, 3, 99); // no such dest
    assert_eq!(ms.amount(5, 0), 0);
    assert_eq!(ms.amount(0, 3), 0);
}

#[test]
fn mod_state_sync_from_matrix() {
    let mut matrix = MatrixState::new();
    let mut registry = ModDestRegistry::new();
    registry.add(PIZZA, ParamPath::Block { block: 0, param: 1 }, *b"A  p\0\0\0\0").unwrap();
    registry.add(PIZZA, ParamPath::Block { block: 2, param: 0 }, *b"B  q\0\0\0\0").unwrap();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 2;
    matrix.amounts[0][0] = 42;
    matrix.amounts[1][1] = -99;

    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix, PIZZA);

    assert_eq!(ms.num_sources(), 2);
    assert_eq!(ms.num_dests(), 2);
    assert_eq!(ms.amount(0, 0), 42);
    assert_eq!(ms.amount(1, 1), -99);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH));
    assert_eq!(ms.dest(1), ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF));
}

/// Review Focus 2: more destinations than fit keep amounts aligned.
#[test]
fn mod_state_truncates_without_misaligning() {
    let mut registry = ModDestRegistry::new();
    // 16 modulatable Block paths on the Pizza chain + 4 FM op levels = 20.
    for (block, params) in [(0u8, 0..3u8), (1, 0..3), (2, 0..3), (3, 0..3), (4, 0..4)] {
        for param in params {
            registry.add(PIZZA, ParamPath::Block { block, param }, *b"X\0\0\0\0\0\0\0").unwrap();
        }
    }
    for op in 0..4u8 {
        registry.add(PIZZA, ParamPath::FmOp { op, param: 2 }, *b"X\0\0\0\0\0\0\0").unwrap();
    }
    assert_eq!(registry.len(), 20);
    assert_eq!(ModState::from_registry(&registry, PIZZA, 2).num_dests(), MAX_MOD_DESTS);

    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry); // matrix keeps 16 too
    matrix.num_sources = 2;
    matrix.amounts[1][15] = 77;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix, PIZZA);
    assert_eq!(ms.num_dests(), MAX_MOD_DESTS);
    assert_eq!(ms.amount(1, 15), 77);
}

/// Review Focus 2: a matrix with more than MAX_MOD_SOURCES rows is clamped
/// (the old code indexed `amounts[si]` past 8 in the audio thread).
#[test]
fn mod_state_clamps_sources() {
    let mut registry = ModDestRegistry::new();
    registry.add(PIZZA, ParamPath::Block { block: 2, param: 0 }, *b"X\0\0\0\0\0\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 16;
    matrix.amounts[12][0] = 50;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix, PIZZA);
    assert_eq!(ms.num_sources(), MAX_MOD_SOURCES);
    let values = [1.0f32; MAX_MOD_SOURCES];
    assert_eq!(ms.sum_for(0, &values), 0.0); // row 12 was dropped, nothing panicked
}
