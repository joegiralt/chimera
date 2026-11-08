use chimera_midi::value::{Bend, Channel, Key, U7, U14, Velocity};

#[test]
fn u7_rejects_128() {
    assert_eq!(U7::new(127), Some(U7::MAX));
    assert_eq!(U7::new(128), None);
}

#[test]
fn u14_round_trips_parts() {
    for v in [0u16, 1, 127, 128, 8192, 16383] {
        let u = U14::new(v).unwrap();
        assert_eq!(U14::from_parts(u.msb(), u.lsb()), u);
        assert_eq!(u.get(), v);
    }
    assert_eq!(U14::new(16384), None);
}

#[test]
fn velocity_zero_unrepresentable() {
    assert_eq!(Velocity::new(0), None);
    assert_eq!(Velocity::new(128), None);
    assert_eq!(Velocity::new(1).map(Velocity::get), Some(1));
    assert_eq!(Velocity::DEFAULT.get(), 100);
}

#[test]
fn channel_and_key_limits() {
    assert!(Channel::new(15).is_some() && Channel::new(16).is_none());
    assert_eq!(Channel::clamped(40).get(), 15);
    assert!(Key::new(127).is_some() && Key::new(128).is_none());
    assert_eq!(Key::clamped(200).get(), 127);
    assert_eq!(Key::A4.get(), 69);
}

fn bend(lsb: u8, msb: u8) -> Bend {
    Bend::from_u14(U14::from_parts(
        U7::new(msb).unwrap(),
        U7::new(lsb).unwrap(),
    ))
}

#[test]
fn bend_ends_and_centre() {
    assert_eq!(bend(0x00, 0x00).get(), -8192);
    assert_eq!(bend(0x00, 0x40).get(), 0);
    assert_eq!(bend(0x7F, 0x7F).get(), 8191);
}

#[test]
fn bend_between() {
    assert_eq!(bend(0x00, 0x20).get(), -4096);
    assert_eq!(bend(0x7F, 0x3F).get(), -1);
    assert_eq!(bend(0x01, 0x40).get(), 1);
    assert_eq!(bend(0x00, 0x20).unit(), -0.5);
}

#[test]
fn bend_unit_hits_plus_and_minus_one() {
    assert_eq!(bend(0, 0).unit(), -1.0);
    assert_eq!(bend(0, 0x40).unit(), 0.0);
    assert_eq!(bend(0x7F, 0x7F).unit(), 1.0);
}
