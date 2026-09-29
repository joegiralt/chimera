//! Spec § Plan and morph: carrier gains blend with MORPH; the output scale
//! is 1 / sqrt(max(1, Σ c·g²)); the mip bound counts blended link depth.

use chimera_core::dsp::algo::algorithms::{AlgoId, plan};
use chimera_core::dsp::algo::morph::{Morph, carrier_norm, carrier_power, incoming};

const UNIT: [f32; 6] = [1.0; 6];

#[test]
fn morph_maps_the_stored_range_and_clamps() {
    assert_eq!(Morph::from_param(0.0), Morph::A);
    assert_eq!(Morph::from_param(127.0), Morph::B);
    assert_eq!(Morph::from_param(-5.0), Morph::A);
    assert_eq!(Morph::from_param(300.0), Morph::B);
    assert!((Morph::from_param(63.5).get() - 0.5).abs() < 1e-6);
}

#[test]
fn carrier_gains_blend_between_the_ends() {
    let p = plan(AlgoId::T1, AlgoId::A1);
    assert_eq!(carrier_power(&p, Morph::A, &UNIT), 2.0);
    assert_eq!(carrier_power(&p, Morph::B, &UNIT), 6.0);
    assert!((carrier_power(&p, Morph::from_param(63.5), &UNIT) - 4.0).abs() < 1e-5);
}

#[test]
fn the_output_scale_is_one_over_root_carriers_and_never_boosts() {
    let a1 = plan(AlgoId::A1, AlgoId::A1);
    assert!((carrier_norm(&a1, Morph::A, &UNIT) - 1.0 / 6f32.sqrt()).abs() < 1e-5);
    assert_eq!(
        carrier_norm(&plan(AlgoId::A17, AlgoId::A17), Morph::A, &UNIT),
        1.0
    );
    let sweep = plan(AlgoId::A17, AlgoId::A1);
    let mut last = carrier_norm(&sweep, Morph::A, &UNIT);
    for i in 1..=127 {
        let n = carrier_norm(&sweep, Morph::from_param(i as f32), &UNIT);
        assert!(n <= last && last - n < 0.02, "step {i}: {last} → {n}");
        last = n;
    }
}

#[test]
fn incoming_depth_sums_blended_links_times_source_gain() {
    let p = plan(AlgoId::A18, AlgoId::A1);
    let gain = [1.0, 0.5, 0.5, 0.5, 0.5, 0.5];
    assert_eq!(incoming(&p, Morph::A, 0, &gain), 2.5);
    assert_eq!(incoming(&p, Morph::B, 0, &gain), 0.0);
    assert!((incoming(&p, Morph::from_param(63.5), 0, &gain) - 1.25).abs() < 1e-5);
    assert_eq!(incoming(&p, Morph::A, 3, &gain), 0.0);
}

#[test]
fn the_power_weighs_each_carrier_by_its_gain_squared() {
    let a1 = plan(AlgoId::A1, AlgoId::A1);
    let g = [1.0, 0.5, 0.0, 0.0, 0.0, 0.25];
    assert_eq!(carrier_power(&a1, Morph::A, &g), 1.0 + 0.25 + 0.0625);
    assert!((carrier_norm(&a1, Morph::A, &g) - 1.3125f32.sqrt().recip()).abs() < 1e-6);
    // Below a power of 1 it never boosts.
    assert_eq!(carrier_norm(&a1, Morph::A, &[0.4; 6]), 1.0);
    // Continuous as a carrier's gain falls to 0.
    let mut last = carrier_norm(&a1, Morph::A, &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    for k in (0..100).rev() {
        let n = carrier_norm(&a1, Morph::A, &[1.0, k as f32 / 100.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(n >= last && n - last < 0.01, "{k}: {last} → {n}");
        last = n;
    }
    assert_eq!(last, 1.0);
}
