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
//!
//! And back (#1038): `read` is the correctly rounded parse `ed set`
//! takes, so the unit reads its own text bit for bit.

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

    /// A tenth of it, whole; what was left over comes out. Sixteen bits
    /// at a time: no 64-bit divide.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.tenth"))]
    fn tenth(&mut self) -> u8 {
        let mut rem = 0u32;
        for l in self.0.iter_mut().rev() {
            let hi = rem << 16 | *l >> 16;
            let lo = (hi % 10) << 16 | *l & 0xFFFF;
            *l = (hi / 10) << 16 | lo / 10;
            rem = lo % 10;
        }
        rem as u8
    }
}

/// A whole part's most digits: `f32::MAX`'s.
pub const WHOLE_MAX: usize = 39;

/// m · 2^e past `u32`, whole: every digit of it (#1039).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.huge"))]
fn huge(w: &mut dyn Write, neg: bool, m: u32, e: u32) -> fmt::Result {
    let mut n = Wide::pow2(e);
    n.times(m);
    let mut digits = [0u8; WHOLE_MAX];
    let mut at = WHOLE_MAX;
    while at > 0 && !n.is_zero() {
        at -= 1;
        digits[at] = b'0' + n.tenth();
    }
    if neg {
        w.write_char('-')?;
    }
    for &d in &digits[at..] {
        w.write_char(char::from(d))?;
    }
    Ok(())
}

/// `v` in the fewest decimals that read back as `v`, a whole part past
/// `u32` in every digit. A value that is no number is `nan`, `inf` or
/// `-inf`, never a number in its place (#1039): a corrupt stored value
/// shows, and `ed set` takes none of the three back.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.stored"))]
pub(super) fn stored(w: &mut dyn Write, v: f32) -> fmt::Result {
    let bits = v.to_bits();
    let (exp, man) = ((bits >> 23 & 0xFF) as i32, bits & 0x7F_FFFF);
    if exp == 0xFF {
        return w.write_str(match (man, v < 0.0) {
            (0, false) => "inf",
            (0, true) => "-inf",
            _ => "nan",
        });
    }
    // v = m · 2^e: a denormal's exponent is the smallest normal's.
    let (m, e) = if exp == 0 {
        (man, -149)
    } else {
        (man | 0x80_0000, exp - 150)
    };
    // A normal's 24 bits shifted past bit 31.
    if e > 8 {
        return huge(w, v < 0.0, m, e as u32);
    }
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
                        whole += 1;
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
    write!(w, "{whole}")?;
    if len != 0 {
        w.write_char('.')?;
        for &d in &digits[..len] {
            w.write_char(char::from(b'0' + d))?;
        }
    }
    Ok(())
}

/// A fraction's decimals kept: a float and the midpoint of two are whole
/// multiples of 2^-150, so of 10^-150, and what a longer tail adds is
/// only that it is over.
const READ_DECIMALS: usize = 150;

/// A decimal fraction under one, a digit a byte, doubled in place a bit
/// at a time.
struct Fraction {
    digits: [u8; READ_DECIMALS],
    len: usize,
}

impl Fraction {
    /// Twice the fraction: its whole part (the next binary place) comes
    /// out, the rest stays.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.twice"))]
    fn twice(&mut self) -> bool {
        let mut carry = 0;
        for d in self.digits[..self.len].iter_mut().rev() {
            let n = *d * 2 + carry;
            (*d, carry) = if n >= 10 { (n - 10, 1) } else { (n, 0) };
        }
        self.trim();
        carry == 1
    }

    /// Zeros at its end are no part of it: zero is no digits.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.trim"))]
    fn trim(&mut self) {
        while self.len > 0 && self.digits[self.len - 1] == 0 {
            self.len -= 1;
        }
    }
}

/// `stored` in reverse (#1038): the `f32` nearest a plain decimal, ties
/// to even, exactly; so what `ed get` printed, `ed set` stores bit for
/// bit. `chimera_theory::tuning::decimal`'s grammar (`-12.5`, `440.`,
/// `.5`, up to nine whole digits), whose float sum lands an ulp off.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.exact.read"))]
pub(super) fn read(w: &[u8]) -> Option<f32> {
    let (neg, w) = match w.split_first() {
        Some((b'-', r)) => (true, r),
        Some((b'+', r)) => (false, r),
        _ => (false, w),
    };
    let (int, frac) = match w.iter().position(|&b| b == b'.') {
        Some(i) => (&w[..i], &w[i + 1..]),
        None => (w, &[][..]),
    };
    if (int.is_empty() && frac.is_empty()) || !frac.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let whole = if int.is_empty() {
        0
    } else {
        chimera_theory::tuning::uint(int)?
    };
    let mut f = Fraction {
        digits: [0; READ_DECIMALS],
        len: frac.len().min(READ_DECIMALS),
    };
    for (d, &b) in f.digits.iter_mut().zip(frac) {
        *d = b - b'0';
    }
    f.trim();
    // Something under the last place read: the tail past it, and below,
    // the bits and decimals no mantissa holds.
    let mut over = frac.iter().skip(READ_DECIMALS).any(|&b| b != b'0');
    // v = m · 2^e and what is over: 25 bits of it, a mantissa and the
    // half place under it, or as far down as a denormal's half place.
    let (mut m, mut e) = (whole, 0i32);
    while m >> 25 != 0 {
        over |= m & 1 != 0;
        m >>= 1;
        e += 1;
    }
    while m >> 24 == 0 && e > -150 {
        m = m << 1 | u32::from(f.twice());
        e -= 1;
    }
    over |= f.len != 0;
    // To the nearest, a tie to even; the carry may make a 25th bit.
    let (mut man, half) = (m >> 1, m & 1 != 0);
    e += 1;
    if half && (over || man & 1 != 0) {
        man += 1;
    }
    if man >> 24 != 0 {
        man >>= 1;
        e += 1;
    }
    // Under 2^23 only at a denormal's exponent, where the bits are the
    // mantissa; 2^23 there is the smallest normal, either way.
    let bits = if man >> 23 == 0 {
        man
    } else {
        ((e + 150) as u32) << 23 | man & 0x7F_FFFF
    };
    Some(f32::from_bits(bits | u32::from(neg) << 31))
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

    /// #1039: a value that is no number says so, and one past `u32`
    /// prints whole: nothing is held at a top that hides it.
    #[test]
    fn nothing_prints_as_another_number() {
        assert_eq!(text(f32::NAN), "nan");
        assert_eq!(text(-f32::NAN), "nan");
        assert_eq!(text(f32::INFINITY), "inf");
        assert_eq!(text(f32::NEG_INFINITY), "-inf");
        assert_eq!(text(4294967296.0), "4294967296");
        assert_eq!(text(-4294967296.0), "-4294967296");
        assert_eq!(text(4294967040.0), "4294967040");
        assert_eq!(text(1.0e10), "10000000000");
        assert_eq!(text(f32::MAX), "340282346638528859811704183484516925440");
        assert_eq!(text(f32::MIN), "-340282346638528859811704183484516925440");
        // `ed set` takes none of the words back.
        for t in ["nan", "inf", "-inf"] {
            assert_eq!(read(t.as_bytes()), None);
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

    /// Every finite value reads back as itself, bit for bit: a sweep of random bits, each binade's edges, the denormals.
    #[test]
    fn every_value_reads_back_as_itself() {
        let check = |bits: u32| {
            let v = f32::from_bits(bits);
            if !v.is_finite() {
                return;
            }
            let t = text(v);
            let back: f32 = t.parse().unwrap();
            assert!(back == v, "{v:e} ({bits:#x}) printed {t}, read {back:e}");
            assert!(t.len() <= 1 + WHOLE_MAX + 1 + DECIMALS_MAX, "{t}");
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

    fn bits(t: &str) -> Option<u32> {
        read(t.as_bytes()).map(f32::to_bits)
    }

    /// #1038: what `stored` printed, `read` takes back bit for bit, over
    /// the sweep `stored` is held to.
    #[test]
    fn what_it_printed_it_reads() {
        let check = |b: u32| {
            let v = f32::from_bits(b);
            if !v.is_finite() || v.abs() >= 1.0e9 {
                return;
            }
            let t = text(v);
            // `-0` prints `0`.
            let want = if v == 0.0 { 0 } else { b };
            assert_eq!(bits(&t), Some(want), "{v:e} ({b:#x}) printed {t}");
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
        // The knobs' grids, where the float parse missed 122 of 5,124.
        for k in 0..=1280 {
            for span in [1.0f32, 2.0, 127.0, 100.0] {
                check((k as f32 / 1280.0 * span).to_bits());
            }
        }
    }

    /// Correctly rounded, as Rust's parse: any decimal of the grammar,
    /// ties and long tails among them.
    #[test]
    fn it_rounds_as_a_correct_parse_does() {
        let same = |t: &str| {
            let want: f32 = t.parse().unwrap();
            assert_eq!(bits(t), Some(want.to_bits()), "{t}");
        };
        for t in [
            "0", "-0", "0.", ".5", "+.5", "440.", "-12.5", "1", "0.1", "0.3",
            // 2^24 + 1 and + 3: ties, to even.
            "16777217", "16777219", "16777217.0000000000000000000000001",
            "16777216.9999999999999999999999999",
            // Halfway between 1 and its neighbour above, and a hair each side.
            "1.00000005960464477539062500000",
            "1.00000005960464477539062500001",
            "1.00000005960464477539062499999",
            // The smallest denormal, half of it (a tie to zero) and just over.
            "0.00000000000000000000000000000000000000000000140129846432481707092372958328991613128026194187651577175706828388979108268586060148663818836212158203125",
            "0.000000000000000000000000000000000000000000000700649232162408535461864791644958065640130970938257885878534141944895541342930300743319094181060791015625",
            "0.0000000000000000000000000000000000000000000007006492321624085354618647916449580656401309709382578858785341419448955413429303007433190941810607910156250000001",
            // The largest denormal's edge with the smallest normal.
            "0.000000000000000000000000000000000000011754943508222875",
            "0.000000000000000000000000000000000000011754942807573643",
            "999999999", "999999999.5", "999999967.99999", "000000001.50",
        ] {
            same(t);
        }
        let mut x = 0x1038_1038_1038_1038u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200_000 {
            let mut t = String::new();
            if next() % 4 == 0 {
                t.push('-');
            }
            for _ in 0..next() % 10 {
                t.push(char::from(b'0' + (next() % 10) as u8));
            }
            t.push('.');
            // Leading zeros reach the small binades; tails run past 150.
            for _ in 0..next() % 44 {
                t.push('0');
            }
            for _ in 0..1 + next() % [3, 9, 20, 170][(next() % 4) as usize] {
                t.push(char::from(b'0' + (next() % 10) as u8));
            }
            same(&t);
        }
    }

    /// `chimera_theory::tuning::decimal`'s grammar, no wider.
    #[test]
    fn it_reads_only_a_plain_decimal() {
        for t in [
            "", "-", "+", ".", "-.", "1e3", "1.5e-3", "inf", "nan", "0x10", "1,5", "1.2.3",
            "--1", "1-", " 1", "1 ", "1234567890", "1.5f",
        ] {
            assert_eq!(bits(t), None, "{t:?}");
            assert_eq!(
                chimera_theory::tuning::decimal(t.as_bytes()).is_none(),
                true,
                "{t:?}"
            );
        }
        assert_eq!(bits("-0"), Some((-0.0f32).to_bits()));
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
