// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relative encoder bytes: one CC value as a signed detent count.

use crate::value::U7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteEncoding {
    /// +1 = 01, -1 = 7F: EC4 CCr1, BCR relative-1.
    TwosComplement,
    /// +1 = 41, -1 = 3F: EC4 CCr2, BCR relative-2, LCXL 3, Arturia.
    BinaryOffset,
    /// +1 = 01, -1 = 41: Mackie V-Pot, BCR relative-3.
    SignBitDown,
    /// +1 = 41, -1 = 01: Ableton "Signed Bit".
    SignBitUp,
}

impl ByteEncoding {
    pub const ALL: [ByteEncoding; 4] = [
        Self::TwosComplement,
        Self::BinaryOffset,
        Self::SignBitDown,
        Self::SignBitUp,
    ];

    /// Detents, -64..=63. The sign-bit encodings read both 0 and 64 as 0.
    pub const fn decode(self, v: U7) -> i8 {
        let v = v.get() as i8;
        let mag = v & 63;
        match self {
            Self::TwosComplement => (v << 1) >> 1,
            Self::BinaryOffset => v - 64,
            Self::SignBitDown => {
                if v & 64 != 0 {
                    -mag
                } else {
                    mag
                }
            }
            Self::SignBitUp => {
                if v & 64 != 0 {
                    mag
                } else {
                    -mag
                }
            }
        }
    }
}
