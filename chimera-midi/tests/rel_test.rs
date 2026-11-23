// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_midi::rel::ByteEncoding::{self, *};
use chimera_midi::value::U7;

fn d(e: ByteEncoding, v: u8) -> i8 {
    e.decode(U7::new(v).unwrap())
}

#[test]
fn each_encoding_plus_minus_one() {
    for (e, up, down) in [
        (TwosComplement, 0x01, 0x7F),
        (BinaryOffset, 0x41, 0x3F),
        (SignBitDown, 0x01, 0x41),
        (SignBitUp, 0x41, 0x01),
    ] {
        assert_eq!(d(e, up), 1, "{e:?} +1");
        assert_eq!(d(e, down), -1, "{e:?} -1");
    }
}

#[test]
fn each_encoding_plus_minus_n() {
    for n in [2u8, 15, 63] {
        let ni = n as i8;
        assert_eq!(d(TwosComplement, n), ni);
        assert_eq!(d(TwosComplement, 128 - n), -ni);
        assert_eq!(d(BinaryOffset, 64 + n), ni);
        assert_eq!(d(BinaryOffset, 64 - n), -ni);
        assert_eq!(d(SignBitDown, n), ni);
        assert_eq!(d(SignBitDown, 64 + n), -ni);
        assert_eq!(d(SignBitUp, 64 + n), ni);
        assert_eq!(d(SignBitUp, n), -ni);
    }
}

#[test]
fn edge_bytes_0_63_64_65_127() {
    // bytes:                    0    63   64   65  127
    let table: [(ByteEncoding, [i8; 5]); 4] = [
        (TwosComplement, [0, 63, -64, -63, -1]),
        (BinaryOffset, [-64, -1, 0, 1, 63]),
        (SignBitDown, [0, 63, 0, -1, -63]),
        (SignBitUp, [0, -63, 0, 1, 63]),
    ];
    for (e, want) in table {
        let got = [0u8, 63, 64, 65, 127].map(|v| d(e, v));
        assert_eq!(got, want, "{e:?}");
    }
}

#[test]
fn twos_comp_01_is_plus_one_binary_offset_01_is_minus_63() {
    assert_eq!(d(TwosComplement, 0x01), 1);
    assert_eq!(d(BinaryOffset, 0x01), -63);
}
