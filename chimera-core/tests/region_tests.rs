use chimera_core::ui::region::{quantize, quantize_values, RegionData, RegionKind, RegionSet};
use chimera_core::ui::page::{PageId, PageKey, PageLayout};
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
    let a = RegionData::header(0, 1, 2, 0, false);
    let b = RegionData::header(0, 1, 2, 0, false);
    assert_eq!(a, b);
}

#[test]
fn region_data_diff_is_not_equal() {
    let a = RegionData::header(0, 1, 2, 0, false);
    let b = RegionData::header(0, 1, 3, 0, false);
    assert_ne!(a, b);
}

#[test]
fn region_data_params_values_differ() {
    let a = RegionData::params(PageKey::Legacy(PageId::DemoWaves), [500; 6]);
    let b = RegionData::params(PageKey::Legacy(PageId::DemoWaves), [501, 500, 500, 500, 500, 500]);
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
    rs.regions[2].prev_data = RegionData::params(PageKey::Legacy(PageId::DemoWaves), [500; 6]);
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

#[test]
fn encoder_only_dirties_params_not_header() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageKey::Legacy(PageId::DemoWaves);
    let values_a = [500u16; 6];
    let values_b = [501, 500, 500, 500, 500, 500];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0, false);
    rs.regions[1].prev_data = RegionData::viz(page, values_a);
    rs.regions[2].prev_data = RegionData::params(page, values_a);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0, 0);

    let current = [
        RegionData::header(0, 0, 0, 0, false),
        RegionData::viz(page, values_b),
        RegionData::params(page, values_b),
        RegionData::nav(0, 0, 0, 0),
    ];

    let dirty: Vec<bool> = rs.active_regions().iter().zip(current.iter())
        .map(|(r, c)| r.prev_data != *c)
        .collect();

    assert_eq!(dirty, vec![false, true, true, false]);
}

#[test]
fn nav_change_dirties_header_and_nav() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageKey::Legacy(PageId::DemoWaves);
    let values = [500u16; 6];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0, false);
    rs.regions[1].prev_data = RegionData::viz(page, values);
    rs.regions[2].prev_data = RegionData::params(page, values);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0, 0);

    let current = [
        RegionData::header(0, 1, 0, 0, false),
        RegionData::viz(page, values),
        RegionData::params(page, values),
        RegionData::nav(0, 1, 0, 0),
    ];

    let dirty: Vec<bool> = rs.active_regions().iter().zip(current.iter())
        .map(|(r, c)| r.prev_data != *c)
        .collect();

    assert_eq!(dirty, vec![true, false, false, true]);
}

#[test]
fn no_change_means_no_dirty() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageKey::Legacy(PageId::DemoWaves);
    let values = [500u16; 6];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0, false);
    rs.regions[1].prev_data = RegionData::viz(page, values);
    rs.regions[2].prev_data = RegionData::params(page, values);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0, 0);

    let current = [
        RegionData::header(0, 0, 0, 0, false),
        RegionData::viz(page, values),
        RegionData::params(page, values),
        RegionData::nav(0, 0, 0, 0),
    ];

    let any_dirty = rs.active_regions().iter().zip(current.iter())
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

    assert_ne!(v1, v2, "animation should produce different quantized values");

    for _ in 0..100 {
        anim[0].update();
    }
    let settled_a = quantize_values(&anim);

    anim[0].update();
    let settled_b = quantize_values(&anim);

    assert_eq!(settled_a, settled_b, "settled animation should produce stable values");
}
