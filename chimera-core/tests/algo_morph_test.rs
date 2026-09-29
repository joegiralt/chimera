//! Spec § Plan and morph: carrier gains blend with MORPH; the output scale
//! is 1 / sqrt(max(1, sum)); the mip bound counts blended link depth.

use chimera_core::dsp::algo::algorithms::{AlgoId, plan};
use chimera_core::dsp::algo::morph::{Morph, Sounding, carrier_norm, carrier_sum, incoming};

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
    assert_eq!(carrier_sum(&p, Morph::A, Sounding::ALL), 2.0);
    assert_eq!(carrier_sum(&p, Morph::B, Sounding::ALL), 6.0);
    assert!((carrier_sum(&p, Morph::from_param(63.5), Sounding::ALL) - 4.0).abs() < 1e-5);
}

#[test]
fn the_output_scale_is_one_over_root_carriers_and_never_boosts() {
    let a1 = plan(AlgoId::A1, AlgoId::A1);
    assert!((carrier_norm(&a1, Morph::A, Sounding::ALL) - 1.0 / 6f32.sqrt()).abs() < 1e-5);
    assert_eq!(
        carrier_norm(&plan(AlgoId::A17, AlgoId::A17), Morph::A, Sounding::ALL),
        1.0
    );
    let sweep = plan(AlgoId::A17, AlgoId::A1);
    let mut last = carrier_norm(&sweep, Morph::A, Sounding::ALL);
    for i in 1..=127 {
        let n = carrier_norm(&sweep, Morph::from_param(i as f32), Sounding::ALL);
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
fn a_silent_carrier_takes_no_share_of_the_output() {
    let a1 = plan(AlgoId::A1, AlgoId::A1);
    let one = Sounding::from_gains(&[0.8, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(one.has(0) && !one.has(1));
    assert_eq!(carrier_sum(&a1, Morph::A, one), 1.0);
    assert_eq!(carrier_norm(&a1, Morph::A, one), 1.0);
    let two = Sounding::from_gains(&[0.8, 0.0, 0.0, 0.0, 0.0, 0.1]);
    assert!((carrier_norm(&a1, Morph::A, two) - 1.0 / 2f32.sqrt()).abs() < 1e-6);
}
