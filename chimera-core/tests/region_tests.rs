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
