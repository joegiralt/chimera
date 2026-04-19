use chimera_core::ui::region::{quantize, quantize_values, RegionData, RegionKind, RegionSet};
use chimera_core::ui::page::{PageId, PageLayout};
use chimera_core::ui::animation::AnimatedValue;

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
    let a = RegionData::header(0, 1, 2, 0);
    let b = RegionData::header(0, 1, 2, 0);
    assert_eq!(a, b);
}

#[test]
fn region_data_diff_is_not_equal() {
    let a = RegionData::header(0, 1, 2, 0);
    let b = RegionData::header(0, 1, 3, 0);
    assert_ne!(a, b);
}

#[test]
fn region_data_params_values_differ() {
    let a = RegionData::params(PageId::Filter, [500; 6]);
    let b = RegionData::params(PageId::Filter, [501, 500, 500, 500, 500, 500]);
    assert_ne!(a, b);
}

#[test]
fn big_viz_has_4_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    assert_eq!(rs.count, 4);
}

#[test]
fn cell_grid_has_3_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    assert_eq!(rs.count, 3);
}

#[test]
fn regions_tile_full_screen_big_viz() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    let regions = rs.active_regions();
    assert_eq!(regions[0].y_start, 0);
    assert_eq!(regions[regions.len() - 1].y_end, 320);
    for i in 1..regions.len() {
        assert_eq!(regions[i].y_start, regions[i - 1].y_end,
            "gap between region {} and {}", i - 1, i);
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
    rs.regions[2].prev_data = RegionData::params(PageId::Filter, [500; 6]);
    rs.set_layout(PageLayout::CellGrid);
    for r in rs.active_regions() {
        match r.prev_data {
            RegionData::Header { chain_idx: 255, .. } => {}
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
    assert_eq!(kinds, vec![RegionKind::Header, RegionKind::Viz, RegionKind::Params, RegionKind::Nav]);
}

#[test]
fn cell_grid_region_kinds() {
    use chimera_core::ui::region::RegionKind;
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    let kinds: Vec<RegionKind> = rs.active_regions().iter().map(|r| r.kind).collect();
    assert_eq!(kinds, vec![RegionKind::Header, RegionKind::Cells, RegionKind::Nav]);
}
