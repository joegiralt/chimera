// The chord table follows Mutable Instruments Rings' `rings/dsp/part.cc`
// (ADR 0032):
//
// Copyright 2015 Emilie Gillet.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

//! SYMP's STRUCTURE: the halo's seven strings step through Rings' chords
//! (ADR 0056).

use super::string::MAX_STRING_DELAY;

pub const CHORD_COUNT: usize = 11;
/// A chord change's glide (`Glide`), seconds to 63 %: 90 % in 184 ms,
/// as a Prophet's glide (the owner's UAT, 2026-09-30).
pub const CHORD_GLIDE_TAU: f32 = 0.08;

/// Rings' single-voice chords (part.cc, `chords[0]`), the 0.0 the main
/// string plays dropped. Pairs 0.01 apart are Rings' detuned chorus.
pub static CHORDS: [[f32; 7]; CHORD_COUNT] = [
    [-12.0, 0.01, 0.02, 0.03, 11.98, 11.99, 12.0],
    [-12.0, 3.0, 3.01, 7.0, 9.99, 10.0, 19.0],
    [-12.0, 3.0, 3.01, 7.0, 11.99, 12.0, 19.0],
    [-12.0, 3.0, 3.01, 7.0, 13.99, 14.0, 19.0],
    [-12.0, 3.0, 3.01, 7.0, 16.99, 17.0, 19.0],
    [-12.0, 6.98, 6.99, 7.0, 12.0, 18.99, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 16.99, 17.0, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 13.99, 14.0, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 11.99, 12.0, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 10.99, 11.0, 19.0],
    [-12.0, 4.99, 5.0, 7.0, 11.99, 12.0, 17.0],
];

/// STRUCTURE's chord.
pub fn chord_of(structure: f32) -> usize {
    ((structure * CHORD_COUNT as f32) as usize).min(CHORD_COUNT - 1)
}

/// Chord `c`'s periods over the main string's.
pub fn period_ratios(c: usize) -> [f32; 7] {
    CHORDS[c].map(|st| libm::exp2f(-st / 12.0))
}

/// The longest period whose line fits: `split` gives
/// `floor(period − 0.5) <= MAX_STRING_DELAY − 2`.
const FITS: f32 = (MAX_STRING_DELAY - 2) as f32 + 1.5;

/// `period` raised by octaves (halved) until its line fits.
pub fn fold(period: f32) -> f32 {
    let mut p = period;
    while p >= FITS {
        p *= 0.5;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::modal::loop_parts::split;

    #[test]
    fn every_chord_has_seven_distinct_intervals() {
        for (c, row) in CHORDS.iter().enumerate() {
            for i in 0..row.len() {
                for j in i + 1..row.len() {
                    assert_ne!(row[i], row[j], "chord {c}");
                }
                assert_ne!(row[i], 0.0, "chord {c}: the main string's unison");
            }
        }
    }

    #[test]
    fn chord_of_spans_the_table() {
        assert_eq!(chord_of(0.0), 0);
        assert_eq!(chord_of(0.1), 1);
        assert_eq!(chord_of(0.3), 3);
        assert_eq!(chord_of(1.0), CHORD_COUNT - 1);
        assert_eq!(chord_of(-1.0), 0);
        assert_eq!(chord_of(f32::NAN), 0);
    }

    /// The least octave up that fits: the line in bounds, and one octave
    /// lower would not be.
    #[test]
    fn fold_fits_by_the_least_octave() {
        for period in [10.0, 979.6, 980.49, 980.5, 1959.2, 4000.0, 1e6] {
            let f = fold(period);
            assert!(split(f, 0.0, 0.1).0 <= MAX_STRING_DELAY - 2, "{period}");
            if f != period {
                assert!(
                    split(2.0 * f, 0.0, 0.1).0 > MAX_STRING_DELAY - 2,
                    "{period}"
                );
            }
        }
        assert_eq!(fold(979.6), 979.6);
    }
}
