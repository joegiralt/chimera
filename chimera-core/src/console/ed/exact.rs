//! A stored value as text that reads back as itself (#1035): the fewest
//! decimals that a correctly rounded parse (Rust's, JavaScript's) turns
//! back into the same `f32`. `describe::num`'s seven places don't: the
//! editor's mirror, fed what `ed get` printed, sat an ulp off the unit.
//!
//! Exact, in integers: the fraction is `n / 2^s`, its digits come off a
//! 160-bit remainder a decimal at a time, and it stops at the first place
//! where what is left (or, rounding up, what is added) is under half the
//! gap to the neighbour on that side: half an ulp, a quarter below a
//! binade's lower edge, where the neighbour below is half as far. Core's
//! float `Display` is 20 KB of bank 1; this is `ed`'s, in its S4 group.

use core::fmt::{self, Write};

/// A value's most decimals: a denormal's 45 leading zeros and its digits.
/// (2^-149 stops by 10^k > 2^151, k = 46, at a quarter ulp; nothing
/// needs more.)
pub const DECIMALS_MAX: usize = 46;

/// 160 bits, least significant limb first: a remainder under 2^149 times
/// ten, or a power of ten up to 10^46.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Wide([u32; 5]);

impl Wide {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.of"))]
    fn of(n: u32) -> Wide {
        Wide([n, 0, 0, 0, 0])
    }

    /// 2^`bit`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.pow2"))]
    fn pow2(bit: u32) -> Wide {
        let mut w = [0u32; 5];
        w[(bit / 32) as usize] = 1 << (bit % 32);
        Wide(w)
    }

    /// Times `k`; the caller's bounds keep it in 160 bits.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.times"))]
    fn times(&mut self, k: u32) {
        let mut carry = 0u64;
        for l in &mut self.0 {
            let p = u64::from(*l) * u64::from(k) + carry;
            *l = p as u32;
            carry = p >> 32;
        }
    }

    /// The four bits from `bit` up, and those cleared with all above:
    /// a remainder under 2^`bit`, times ten, gives up its digit.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.take"))]
    fn take(&mut self, bit: u32) -> u8 {
        let (i, b) = ((bit / 32) as usize, bit % 32);
        let mut d = self.0[i] >> b;
        if b > 28 && i + 1 < self.0.len() {
            d |= self.0[i + 1] << (32 - b);
        }
        self.0[i] &= (1u32 << b) - 1;
        for l in &mut self.0[i + 1..] {
            *l = 0;
        }
        (d & 0xF) as u8
    }

    /// `self − o`, `o` no more than `self`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.less"))]
    fn less(mut self, o: &Wide) -> Wide {
        let mut borrow = false;
        for (l, r) in self.0.iter_mut().zip(&o.0) {
            let (d, b1) = l.overflowing_sub(*r);
            let (d, b2) = d.overflowing_sub(u32::from(borrow));
            (*l, borrow) = (d, b1 || b2);
        }
        self
    }

    /// `k` times `self` is under `o`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.under"))]
    fn under(mut self, k: u32, o: &Wide) -> bool {
        self.times(k);
        for (l, r) in self.0.iter().zip(&o.0).rev() {
            if l != r {
                return l < r;
            }
        }
        false
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.zero"))]
    fn is_zero(&self) -> bool {
        self.0.iter().all(|&l| l == 0)
    }
}

/// `v`, finite, in the fewest decimals that read back as `v` (a whole
/// part past `u32` is held at its top: no stored value is near it).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.stored"))]
pub(super) fn stored(w: &mut dyn Write, v: f32) -> fmt::Result {
    let bits = v.to_bits();
    let (exp, man) = ((bits >> 23 & 0xFF) as i32, bits & 0x7F_FFFF);
    // v = m · 2^e: a denormal's exponent is the smallest normal's.
    let (m, e) = if exp == 0 {
        (man, -149)
    } else {
        (man | 0x80_0000, exp - 150)
    };
    // The whole part and the fraction n / 2^s.
    let (mut whole, n, s) = if e >= 0 {
        (v.abs() as u32, 0, 0)
    } else if e > -24 {
        let s = (-e) as u32;
        (m >> s, m & ((1 << s) - 1), s)
    } else {
        (0, m, (-e) as u32)
    };
    // Below `v` the next float is half an ulp off at a binade's lower
    // edge (not the lowest binade's): a cut must land twice as near.
    let below = if man == 0 && exp > 1 { 4 } else { 2 };
    let mut digits = [0u8; DECIMALS_MAX];
    let mut len = 0;
    if n != 0 {
        let (mut rem, mut pow, one) = (Wide::of(n), Wide::of(1), Wide::pow2(s));
        while len < DECIMALS_MAX {
            rem.times(10);
            pow.times(10);
            digits[len] = rem.take(s);
            len += 1;
            if rem.is_zero() || rem.under(below, &pow) {
                break;
            }
            if one.less(&rem).under(2, &pow) {
                // Up: the carry runs left, into the whole part at the end.
                let mut i = len;
                loop {
                    if i == 0 {
                        whole = whole.saturating_add(1);
                        break;
                    }
                    i -= 1;
                    if digits[i] < 9 {
                        digits[i] += 1;
                        break;
                    }
                    digits[i] = 0;
                }
                break;
            }
        }
        while len > 0 && digits[len - 1] == 0 {
            len -= 1;
        }
    }
    if v < 0.0 && (whole != 0 || len != 0) {
        w.write_char('-')?;
    }
    super::lit::uint(w, whole)?;
    if len != 0 {
        w.write_char('.')?;
        for &d in &digits[..len] {
            w.write_char(char::from(b'0' + d))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::format;
    use std::string::String;

    fn text(v: f32) -> String {
        let mut s = String::new();
        stored(&mut s, v).unwrap();
        s
    }

    #[test]
    fn plain_values_print_plainly() {
        for (v, t) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (127.0, "127"),
            (-24.0, "-24"),
            (0.5, "0.5"),
            (-0.5, "-0.5"),
            (0.1, "0.1"),
            (0.0078125, "0.0078125"),
            (0.3515625, "0.3515625"),
            (0.542487, "0.542487"),
            (0.716759, "0.716759"),
            (20000.0, "20000"),
            (440.25, "440.25"),
        ] {
            assert_eq!(text(v), t);
        }
    }

    /// The two #1035 found: seven places read back an ulp off.
    #[test]
    fn the_values_seven_places_lost_read_back() {
        for v in [0.5424871f32, 0.7167591, 1.0 / 3.0, 0.1 + 0.2, f32::EPSILON] {
            assert_eq!(
                text(v).parse::<f32>().unwrap().to_bits(),
                v.to_bits(),
                "{v:?}"
            );
        }
    }

    /// Every finite value under `u32`'s top reads back as itself, bit for
    /// bit: a sweep of random bits, each binade's edges, the denormals.
    #[test]
    fn every_value_reads_back_as_itself() {
        let check = |bits: u32| {
            let v = f32::from_bits(bits);
            if !v.is_finite() || v.abs() >= 4.0e9 {
                return;
            }
            let t = text(v);
            let back: f32 = t.parse().unwrap();
            assert!(back == v, "{v:e} ({bits:#x}) printed {t}, read {back:e}");
            assert!(t.len() <= 1 + 10 + 1 + DECIMALS_MAX, "{t}");
        };
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..400_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            check(x as u32);
        }
        for exp in 0..=0xFEu32 {
            for man in [0, 1, 2, 0x40_0000, 0x7F_FFFE, 0x7F_FFFF] {
                check(exp << 23 | man);
                check(1 << 31 | exp << 23 | man);
            }
        }
        for man in 1..2000 {
            check(man);
        }
    }

    /// Never longer than Rust's own shortest text by more than a digit
    /// (a tie it would take), on the knob's grids.
    #[test]
    fn it_is_about_as_short_as_the_shortest() {
        for k in 0..=1280 {
            for span in [1.0f32, 2.0, 127.0, 100.0] {
                let v = k as f32 / 1280.0 * span;
                let (mine, std) = (text(v), format!("{v}"));
                assert!(mine.len() <= std.len() + 1, "{v:?}: {mine} vs {std}");
            }
        }
    }
}
