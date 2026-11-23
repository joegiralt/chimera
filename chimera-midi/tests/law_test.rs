// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_midi::ctl::DataValue;
use chimera_midi::law::*;
use chimera_midi::take::Place;
use chimera_midi::value::{U7, U14};

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}
fn coarse(v: u8) -> DataValue {
    DataValue::Coarse(u7(v))
}
fn fine(m: u8, l: u8) -> DataValue {
    DataValue::Fine(U14::from_parts(u7(m), u7(l)))
}
fn none(_: U7) -> Option<u16> {
    None
}
fn at(law: ValueLaw, v: DataValue, steps: u16) -> Option<u16> {
    place_of(law, v, steps, none).map(Place::get)
}

#[test]
fn place14_coarse_127_is_top() {
    assert_eq!(at(ValueLaw::Place14, coarse(127), 0), Some(16383));
    assert_eq!(at(ValueLaw::Place14, coarse(0), 0), Some(0));
    assert_eq!(at(ValueLaw::Place14, coarse(64), 0), Some(64 << 7 | 64));
}

#[test]
fn place14_fine_exact() {
    assert_eq!(at(ValueLaw::Place14, fine(64, 0), 0), Some(8192));
    assert_eq!(at(ValueLaw::Place14, fine(1, 5), 0), Some(133));
    assert_eq!(at(ValueLaw::Place14, fine(127, 127), 0), Some(16383));
}

#[test]
fn index7_reads_msb() {
    // 8 values, index 0..=7: index i sits at round(i * 16383 / 7)
    assert_eq!(at(ValueLaw::Index7, coarse(3), 7), Some(7021));
    assert_eq!(
        at(ValueLaw::Index7, fine(3, 99), 7),
        Some(7021),
        "LSB ignored"
    );
    assert_eq!(at(ValueLaw::Index7, coarse(0), 7), Some(0));
    assert_eq!(at(ValueLaw::Index7, coarse(7), 7), Some(16383));
    assert_eq!(
        at(ValueLaw::Index7, coarse(100), 7),
        Some(16383),
        "past the top clamps"
    );
    assert_eq!(
        at(ValueLaw::Index7, coarse(5), 0),
        Some(0),
        "a one-value param"
    );
}

#[test]
fn index14_reads_both() {
    assert_eq!(at(ValueLaw::Index14, fine(1, 5), 200), Some(10895)); // round(133 * 16383 / 200)
    assert_eq!(at(ValueLaw::Index14, coarse(1), 200), Some(10485)); // 128 steps: round(128 * 16383 / 200)
    assert_eq!(at(ValueLaw::Index14, fine(127, 127), 200), Some(16383));
}

/// A stub frozen-code table: codes 10, 20, 30 at indices 0, 1, 2.
fn stub(code: U7) -> Option<u16> {
    match code.get() {
        10 => Some(0),
        20 => Some(1),
        30 => Some(2),
        _ => None,
    }
}

#[test]
fn code7_maps_frozen_code() {
    let at = |v| place_of(ValueLaw::Code7, v, 2, stub).map(Place::get);
    assert_eq!(at(coarse(10)), Some(0));
    assert_eq!(at(coarse(20)), Some(8192)); // round(16383 / 2)
    assert_eq!(at(fine(30, 77)), Some(16383), "LSB ignored");
}

#[test]
fn code7_unknown_code_is_none() {
    assert_eq!(place_of(ValueLaw::Code7, coarse(11), 2, stub), None);
    assert_eq!(place_of(ValueLaw::Code7, coarse(1), 2, stub), None);
}

#[test]
fn other_laws_ignore_the_table() {
    let boom = |_: U7| -> Option<u16> { panic!("not a Code7 law") };
    assert!(place_of(ValueLaw::Place14, coarse(1), 0, boom).is_some());
    assert!(place_of(ValueLaw::Index7, coarse(1), 3, boom).is_some());
    assert!(place_of(ValueLaw::Index14, coarse(1), 3, boom).is_some());
}
