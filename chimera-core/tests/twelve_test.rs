//! 12-bit packing (ADR 0087, pool constraint): two samples in three bytes.

mod common;

use chimera_core::storage::twelve::{pack12, unpack12};
use common::XorShift;

fn round_trip(a: i16, b: i16) {
    let mut out = [0u8; 3];
    assert_eq!(pack12(&[a, b], &mut out), 3);
    assert_eq!(unpack12(&out, 0), (a, b), "{a}, {b}");
}

#[test]
fn pack_unpack_all_values() {
    if cfg!(debug_assertions) {
        let mut rng = XorShift(0x7A9E_0002);
        for _ in 0..100_000 {
            let mut s = || (rng.next_u32() & 0xFFF) as i16;
            let (a, b) = (s(), s());
            round_trip((a << 4) >> 4, (b << 4) >> 4);
        }
    } else {
        for a in -2048..2048 {
            for b in -2048..2048 {
                round_trip(a, b);
            }
        }
    }
}

#[test]
fn odd_tail_pads() {
    let mut out = [0xFFu8; 8];
    assert_eq!(pack12(&[-2048, 2047, -1], &mut out), 6);
    assert_eq!(unpack12(&out, 0), (-2048, 2047));
    assert_eq!(unpack12(&out, 1), (-1, 0));
    assert_eq!(out[6..], [0xFF, 0xFF], "nothing past the last pair");
}
