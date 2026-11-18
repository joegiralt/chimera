//! The per-Part EQ's settings (ADR 0284): TILT and LO CUT in one byte.

use chimera_engine::part_eq::{LoCut, PartEq, Tilt};

#[test]
fn the_setting_is_one_byte_and_every_byte_round_trips() {
    assert_eq!(core::mem::size_of::<PartEq>(), 1);
    for b in 0..=u8::MAX {
        assert_eq!(PartEq::from_raw(b).raw(), b);
    }
}

#[test]
fn flat_and_off_is_the_default_and_the_bypass() {
    assert_eq!(PartEq::default(), PartEq::FLAT);
    assert_eq!(PartEq::FLAT.tilt(), Tilt::FLAT);
    assert_eq!(PartEq::FLAT.lo_cut(), LoCut::Off);
    assert!(PartEq::FLAT.is_flat());
    assert!(!PartEq::new(Tilt::FLAT, LoCut::On).is_flat());
    assert!(!PartEq::new(Tilt::at(1), LoCut::Off).is_flat());
}

#[test]
fn every_tilt_and_lo_cut_round_trip() {
    for s in Tilt::MIN_STEP..=Tilt::MAX_STEP {
        for c in [LoCut::Off, LoCut::On] {
            let e = PartEq::new(Tilt::at(s), c);
            assert_eq!((e.tilt().step(), e.lo_cut()), (s, c));
        }
    }
}

#[test]
fn flat_lands_exactly_on_0_db_and_the_ends_on_10() {
    assert_eq!(Tilt::FLAT.db(), 0.0);
    assert_eq!(Tilt::FLAT.step(), 0);
    assert_eq!(Tilt::at(Tilt::MIN_STEP).db(), -10.0);
    assert_eq!(Tilt::at(Tilt::MAX_STEP).db(), 10.0 * 63.0 / 64.0);
    // A step is 1/128 of the 20 dB travel.
    assert_eq!(Tilt::at(1).db(), 20.0 / 128.0);
}

#[test]
fn a_tilt_clamps_at_its_ends() {
    assert_eq!(Tilt::at(-100).step(), Tilt::MIN_STEP);
    assert_eq!(Tilt::at(100).step(), Tilt::MAX_STEP);
    assert_eq!(Tilt::from_f32(-0.4).step(), 0);
    assert_eq!(Tilt::from_f32(12.6).step(), 13);
    assert_eq!(Tilt::from_f32(f32::NAN), Tilt::FLAT);
    assert_eq!(Tilt::from_f32(1e9).step(), Tilt::MAX_STEP);
}

#[test]
fn a_tilt_s_code_is_its_cc_value_64_flat() {
    assert_eq!(Tilt::FLAT.code(), 64);
    for c in 0..=127u8 {
        assert_eq!(Tilt::from_code(c).map(Tilt::code), Some(c));
    }
    assert_eq!(Tilt::from_code(128), None);
}
