use chimera_midi::sxmap::{MAX_LEN, Shape, carry, learn};
use proptest::prelude::*;

/// A Roland DT1 body: 41 dev 42 12, address, value, checksum.
fn dt1(addr: [u8; 3], v: u8) -> Vec<u8> {
    let mut b = vec![0x41, 0x10, 0x42, 0x12, addr[0], addr[1], addr[2], v];
    let sum: u32 = b[4..].iter().map(|&x| u32::from(x)).sum();
    b.push(((128 - sum % 128) % 128) as u8);
    b
}

#[test]
fn a_checksummed_value_is_found() {
    let s = learn(&dt1([0x40, 0x01, 0x1A], 10), &dt1([0x40, 0x01, 0x1A], 90)).unwrap();
    assert_eq!((s.at(), s.pair(), s.check()), (7, false, true));
    let k = s.key(&dt1([0x40, 0x01, 0x1A], 10)).unwrap();
    assert_eq!(
        s.value(&dt1([0x40, 0x01, 0x1A], 64), k).map(|v| v.get()),
        Some(64 << 7 | 64)
    );
    // Another address is another control.
    assert_eq!(s.value(&dt1([0x40, 0x01, 0x1B], 64), k), None);
}

#[test]
fn a_plain_byte_and_never_a_pair_at_once() {
    // Yamaha DX7 parameter change: F0 43 10 01 pp vv F7.
    let s = learn(&[0x43, 0x10, 0x01, 0x05, 3], &[0x43, 0x10, 0x01, 0x05, 99]).unwrap();
    assert_eq!((s.at(), s.pair(), s.check()), (4, false, false));
    // Two bytes apart from two messages: a pair or an address and a value,
    // which two can't tell (`carry` learns a pair).
    assert_eq!(
        learn(&[0x42, 0x30, 7, 1, 0x7F], &[0x42, 0x30, 7, 2, 0x00]),
        None
    );
}

#[test]
fn two_controls_or_other_lengths_are_not_learned() {
    assert_eq!(learn(&[1, 2, 3], &[1, 2, 3]), None);
    assert_eq!(learn(&[1, 2, 3], &[1, 2, 3, 4]), None);
    assert_eq!(learn(&[1, 2, 3, 4, 5], &[9, 2, 3, 9, 5]), None);
    assert_eq!(learn(&[], &[]), None);
    assert_eq!(learn(&[0; 40], &[1; 40]), None);
}

#[test]
fn shape_bits_round_trip() {
    for at in 0..MAX_LEN as u8 - 1 {
        for (p, c) in [(false, false), (true, false), (false, true), (true, true)] {
            let s = Shape::new(at, p, c).unwrap();
            assert_eq!(Shape::from_bits(s.bits()), Some(s));
        }
    }
    assert_eq!(Shape::from_bits(0x80), None);
    assert_eq!(Shape::new(MAX_LEN as u8, false, false), None);
}

proptest! {
    /// Any two bodies: learn never panics, and what it learns reads the
    /// second's value back from it.
    #[test]
    fn learn_is_safe(a in proptest::collection::vec(0u8..128, 0..40),
                     b in proptest::collection::vec(0u8..128, 0..40)) {
        if let Some(s) = learn(&a, &b) {
            let k = s.key(&a).unwrap();
            prop_assert!(s.value(&b, k).is_some());
        }
    }

    /// Any body and shape bits: `key` and `value` never panic.
    #[test]
    fn match_is_safe(body in proptest::collection::vec(0u8..128, 0..40), bits in 0u8..=255, key in any::<u32>()) {
        if let Some(s) = Shape::from_bits(bits) {
            let _ = s.key(&body);
            let _ = s.value(&body, key);
        }
    }
}

/// DX7 parameter change body: 43 10 01 pp vv.
fn dx7(pp: u8, vv: u8) -> Vec<u8> {
    vec![0x43, 0x10, 0x01, pp, vv]
}

#[test]
fn an_address_byte_beside_the_value_is_never_learned_as_a_pair() {
    // Two knobs, one after the other: the address and the value differ.
    assert_eq!(learn(&dx7(5, 3), &dx7(6, 99)), None);
    assert_eq!(learn(&dx7(5, 100), &dx7(6, 10)), None);
    // One knob: its value byte alone, the address kept in the key.
    let s = learn(&dx7(5, 3), &dx7(5, 99)).unwrap();
    assert_eq!((s.at(), s.pair()), (4, false));
    let k = s.key(&dx7(5, 3)).unwrap();
    assert!(
        s.value(&dx7(6, 64), k).is_none(),
        "another knob never moves it"
    );
}

#[test]
fn a_slow_14_bit_sweep_learns_its_lsb_then_the_pair_at_a_carry() {
    let m = |msb: u8, lsb: u8| vec![0x42, 0x30, 7, msb, lsb];
    let one = learn(&m(3, 10), &m(3, 11)).unwrap();
    assert_eq!((one.at(), one.pair()), (4, false));
    // No carry: an LSB step, or a jump too big to be one.
    assert_eq!(carry(one, &m(3, 11), &m(3, 12)), None);
    assert_eq!(carry(one, &m(3, 100), &m(4, 10)), None);
    // A carry: 3:127 to 4:0, and down again.
    let up = carry(one, &m(3, 127), &m(4, 0)).unwrap();
    let pair = up.pair();
    assert_eq!((pair.at(), pair.pair()), (3, true));
    let down = carry(one, &m(4, 1), &m(3, 126)).unwrap();
    assert_eq!(down.pair(), pair);
    // One carry is not a pair yet: a second, the same way on, is.
    assert!(!up.confirms(down), "the other way");
    assert!(!up.confirms(up), "the same carry again");
    assert!(up.confirms(carry(one, &m(4, 127), &m(5, 0)).unwrap()));
    assert!(down.confirms(carry(one, &m(3, 1), &m(2, 127)).unwrap()));
    let k = pair.key(&m(4, 0)).unwrap();
    assert_eq!(pair.value(&m(5, 2), k).map(|v| v.get()), Some(5 << 7 | 2));
    // A pair is never carried again.
    assert!(carry(pair, &m(3, 127), &m(4, 0)).is_none());
}

#[test]
fn a_neighbouring_knob_never_confirms_a_carry() {
    // DT1: 1A at 120, then the user touches 1B at 2: one carry's shape.
    let one = learn(&dt1([0x40, 0x01, 0x1A], 10), &dt1([0x40, 0x01, 0x1A], 11)).unwrap();
    let first = carry(
        one,
        &dt1([0x40, 0x01, 0x1A], 120),
        &dt1([0x40, 0x01, 0x1B], 2),
    )
    .unwrap();
    // 1B on: no second carry, so no pair.
    assert!(
        carry(
            one,
            &dt1([0x40, 0x01, 0x1B], 2),
            &dt1([0x40, 0x01, 0x1B], 30)
        )
        .is_none()
    );
    // Back to 1A, then 1B again: the same carry, not a second one on.
    let again = carry(
        one,
        &dt1([0x40, 0x01, 0x1A], 125),
        &dt1([0x40, 0x01, 0x1B], 1),
    )
    .unwrap();
    assert!(!first.confirms(again));
}
