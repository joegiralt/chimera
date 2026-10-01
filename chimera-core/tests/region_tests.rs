use chimera_core::ui::animation::AnimatedValue;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::components::Look;
use chimera_core::ui::page::{PageId, PageKey, PageLayout};
use chimera_core::ui::region::{RegionData, RegionSet, quantize, quantize_values};

#[test]
fn quantize_zero() {
    assert_eq!(quantize(0.0), 0);
}

#[test]
fn quantize_one() {
    assert_eq!(quantize(1.0), 1000);
}

#[test]
fn quantize_half() {
    assert_eq!(quantize(0.5), 500);
}

#[test]
fn quantize_clamps_negative() {
    assert_eq!(quantize(-1.0), 0);
}

#[test]
fn quantize_stability_tiny_jitter() {
    let a = quantize(0.5);
    let b = quantize(0.5005);
    assert_eq!(a, b);
}

#[test]
fn region_data_same_is_equal() {
    let a = RegionData::header(0, 1, 2, 0, false, 0, 0);
    let b = RegionData::header(0, 1, 2, 0, false, 0, 0);
    assert_eq!(a, b);
}

#[test]
fn region_data_diff_is_not_equal() {
    let a = RegionData::header(0, 1, 2, 0, false, 0, 0);
    let b = RegionData::header(0, 1, 3, 0, false, 0, 0);
    assert_ne!(a, b);
}

#[test]
fn region_data_cell_values_differ() {
    let a = RegionData::cells(
        PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id)),
        [500; 6],
        0,
        0,
        [None; 6],
    );
    let b = RegionData::cells(
        PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id)),
        [501, 500, 500, 500, 500, 500],
        0,
        0,
        [None; 6],
    );
    assert_ne!(a, b);
}

#[test]
fn big_viz_has_4_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    assert_eq!(rs.count, 4);
}

#[test]
fn cell_grid_has_5_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    assert_eq!(rs.count, 5);
}

#[test]
fn regions_tile_full_screen_big_viz() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    let regions = rs.active_regions();
    assert_eq!(regions[0].y_start, 0);
    assert_eq!(regions[regions.len() - 1].y_end, 320);
    for i in 1..regions.len() {
        assert_eq!(
            regions[i].y_start,
            regions[i - 1].y_end,
            "gap between region {} and {}",
            i - 1,
            i
        );
    }
}

#[test]
fn regions_tile_full_screen_cell_grid() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    let regions = rs.active_regions();
    assert_eq!(regions[0].y_start, 0);
    assert_eq!(regions[regions.len() - 1].y_end, 320);
    for i in 1..regions.len() {
        assert_eq!(regions[i].y_start, regions[i - 1].y_end);
    }
}

#[test]
fn layout_change_resets_all_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    rs.regions[2].prev_data = RegionData::cells(
        PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id)),
        [500; 6],
        0,
        0,
        [None; 6],
    );
    rs.set_layout(PageLayout::CellGrid);
    for r in rs.active_regions() {
        match r.prev_data {
            RegionData::Header { chain_idx: 255, .. } => {}
            RegionData::Focus { slot: u8::MAX, .. } => {}
            RegionData::Viz { values, .. } if values == [u16::MAX; 6] => {}
            RegionData::Cells { values, .. } if values == [u16::MAX; 6] => {}
            RegionData::Nav { chain_idx: 255, .. } => {}
            other => panic!("expected sentinel, got {:?}", other),
        }
    }
}

#[test]
fn big_viz_region_kinds() {
    use chimera_core::ui::region::RegionKind;
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    let kinds: Vec<RegionKind> = rs.active_regions().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![
            RegionKind::Header,
            RegionKind::Viz,
            RegionKind::Cells,
            RegionKind::Nav
        ]
    );
}

#[test]
fn cell_grid_region_kinds() {
    use chimera_core::ui::region::RegionKind;
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    let kinds: Vec<RegionKind> = rs.active_regions().iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![
            RegionKind::Header,
            RegionKind::Focus,
            RegionKind::Viz,
            RegionKind::Cells,
            RegionKind::Nav
        ]
    );
}

#[test]
fn encoder_only_dirties_params_not_header() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id));
    let values_a = [500u16; 6];
    let values_b = [501, 500, 500, 500, 500, 500];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0, false, 0, 0);
    rs.regions[1].prev_data = RegionData::viz(page, values_a, 0);
    rs.regions[2].prev_data = RegionData::cells(page, values_a, 0, 0, [None; 6]);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0, 0, 0);

    let current = [
        RegionData::header(0, 0, 0, 0, false, 0, 0),
        RegionData::viz(page, values_b, 0),
        RegionData::cells(page, values_b, 0, 0, [None; 6]),
        RegionData::nav(0, 0, 0, 0, 0),
    ];

    let dirty: Vec<bool> = rs
        .active_regions()
        .iter()
        .zip(current.iter())
        .map(|(r, c)| r.prev_data != *c)
        .collect();

    assert_eq!(dirty, vec![false, true, true, false]);
}

#[test]
fn nav_change_dirties_header_and_nav() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id));
    let values = [500u16; 6];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0, false, 0, 0);
    rs.regions[1].prev_data = RegionData::viz(page, values, 0);
    rs.regions[2].prev_data = RegionData::cells(page, values, 0, 0, [None; 6]);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0, 0, 0);

    let current = [
        RegionData::header(0, 1, 0, 0, false, 0, 0),
        RegionData::viz(page, values, 0),
        RegionData::cells(page, values, 0, 0, [None; 6]),
        RegionData::nav(0, 1, 0, 0, 0),
    ];

    let dirty: Vec<bool> = rs
        .active_regions()
        .iter()
        .zip(current.iter())
        .map(|(r, c)| r.prev_data != *c)
        .collect();

    assert_eq!(dirty, vec![true, false, false, true]);
}

#[test]
fn no_change_means_no_dirty() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id));
    let values = [500u16; 6];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0, false, 0, 0);
    rs.regions[1].prev_data = RegionData::viz(page, values, 0);
    rs.regions[2].prev_data = RegionData::cells(page, values, 0, 0, [None; 6]);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0, 0, 0);

    let current = [
        RegionData::header(0, 0, 0, 0, false, 0, 0),
        RegionData::viz(page, values, 0),
        RegionData::cells(page, values, 0, 0, [None; 6]),
        RegionData::nav(0, 0, 0, 0, 0),
    ];

    let any_dirty = rs
        .active_regions()
        .iter()
        .zip(current.iter())
        .any(|(r, c)| r.prev_data != *c);

    assert!(!any_dirty);
}

#[test]
fn animation_settling_produces_dirty_then_clean() {
    let mut anim = [AnimatedValue::new(0.5); 6];
    anim[0].set_target(0.8);

    anim[0].update();
    let v1 = quantize_values(&anim);

    anim[0].update();
    let v2 = quantize_values(&anim);

    assert_ne!(
        v1, v2,
        "animation should produce different quantized values"
    );

    for _ in 0..100 {
        anim[0].update();
    }
    let settled_a = quantize_values(&anim);

    anim[0].update();
    let settled_b = quantize_values(&anim);

    assert_eq!(
        settled_a, settled_b,
        "settled animation should produce stable values"
    );
}

fn cells_keyed(matrix_rev: u16, looks: u16) -> RegionData {
    RegionData::cells(
        PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id)),
        [500; 6],
        0,
        0,
        [None; 6],
    )
    .keyed(matrix_rev, looks)
}

/// A cell turning absent (or dimmed) alone redraws the cells.
#[test]
fn region_data_cell_looks_differ() {
    assert_eq!(cells_keyed(0, 0), cells_keyed(0, 0));
    assert_ne!(cells_keyed(0, 0), cells_keyed(0, 1 << 8));
}

/// A matrix change alone (a route deleted at 0) redraws the cells.
#[test]
fn region_data_cell_matrix_rev_differs() {
    assert_ne!(cells_keyed(0, 0), cells_keyed(1, 0));
}

fn focus_with(look: Look) -> RegionData {
    RegionData::focus(
        PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id)),
        0,
        500,
        look,
        None,
    )
}

/// The focused slot turning dimmed (or absent) alone redraws the focus
/// band: a look can change without a `matrix_rev` bump (#123).
#[test]
fn region_data_focus_looks_differ() {
    assert_eq!(focus_with(Look::Dimmed), focus_with(Look::Dimmed));
    assert_ne!(focus_with(Look::Live), focus_with(Look::Dimmed));
    assert_ne!(focus_with(Look::Live), focus_with(Look::Absent));
}

/// Mod matrix (#161): the grid under the header, the one-line readout under
/// it, tiling the screen.
#[test]
fn matrix_regions_tile_grid_then_readout() {
    use chimera_core::ui::mod_grid::GRID_BOTTOM;
    use chimera_core::ui::region::RegionKind;
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::Matrix);
    let regions = rs.active_regions();
    let kinds: Vec<RegionKind> = regions.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            RegionKind::Header,
            RegionKind::Grid,
            RegionKind::Focus,
            RegionKind::Nav
        ]
    );
    assert_eq!(regions[1].y_end, GRID_BOTTOM as u16);
    assert_eq!((regions[0].y_start, regions[3].y_end), (0, 320));
    for i in 1..regions.len() {
        assert_eq!(regions[i].y_start, regions[i - 1].y_end);
    }
}

/// The readout's key carries `MatrixState.rev` (a delete changes the route
/// count) and the lerped amount it prints; the grid's the scroll (the
/// cursor can stay put); both the inert columns.
#[test]
fn matrix_keys_carry_rev_amount_and_scroll() {
    let route = |value, rev| RegionData::route(1, 0, 2, value).keyed(rev, 0);
    assert_eq!(route(500, 3), route(500, 3));
    assert_ne!(route(500, 3), route(500, 4));
    assert_ne!(route(500, 3), route(501, 3));
    let grid = |sx, rev| RegionData::grid(1, 0, sx).keyed(rev, 0);
    assert_ne!(grid(0, 3), grid(1, 3));
    assert_ne!(grid(0, 3), grid(0, 4));
    // A column going inert (ALG B set to ALG A) redraws both (#188).
    assert_ne!(
        route(500, 3),
        RegionData::route(1, 0, 2, 500).keyed(3, 0b10)
    );
    assert_ne!(grid(0, 3), RegionData::grid(1, 0, 0).keyed(3, 0b10));
}
