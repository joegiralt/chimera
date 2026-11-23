// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SysEx parameter messages as controls (#846): an old box that edits a
//! synth by SysEx sends one fixed message a parameter with its value in
//! one byte (or two, MSB first), often followed by a checksum. Learn
//! diffs two messages of one control at different values to find where
//! the value sits (`learn`); a binding keeps that `Shape` and a hash of
//! the message's fixed bytes (`Shape::key`), and a later message matches
//! when its own key is the same (`Shape::value`). Pure, never panics.

use crate::value::U14;

/// The longest body learned: a parameter message, not a dump.
pub const MAX_LEN: usize = 32;

/// Where a learned message's value sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape(u8);

impl Shape {
    const PAIR: u8 = 0x20;
    const CHECK: u8 = 0x40;

    /// The value at byte `at` (from the byte after F0), one byte or a pair
    /// MSB first, with or without a trailing checksum. `None` past
    /// `MAX_LEN`.
    pub const fn new(at: u8, pair: bool, check: bool) -> Option<Shape> {
        if at as usize + pair as usize >= MAX_LEN {
            return None;
        }
        Some(Shape(
            at | if pair { Self::PAIR } else { 0 } | if check { Self::CHECK } else { 0 },
        ))
    }

    /// From its stored byte, if it is one.
    pub const fn from_bits(b: u8) -> Option<Shape> {
        if b & 0x80 != 0 {
            return None;
        }
        Shape::new(b & 0x1F, b & Self::PAIR != 0, b & Self::CHECK != 0)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn at(self) -> usize {
        (self.0 & 0x1F) as usize
    }

    pub const fn pair(self) -> bool {
        self.0 & Self::PAIR != 0
    }

    pub const fn check(self) -> bool {
        self.0 & Self::CHECK != 0
    }

    /// Byte `i` of a `len`-byte body is the value's or the checksum's.
    fn masked(self, i: usize, len: usize) -> bool {
        i == self.at() || (self.pair() && i == self.at() + 1) || (self.check() && i + 1 == len)
    }

    /// `body`'s fixed bytes as one word: FNV-1a over its length and every
    /// byte, the value's and checksum's as zero. `None` if the value
    /// doesn't fit in it.
    pub fn key(self, body: &[u8]) -> Option<u32> {
        let len = body.len();
        let last = self.at() + usize::from(self.pair());
        if len > MAX_LEN || last >= len || (self.check() && last + 1 >= len) {
            return None;
        }
        let mut h: u32 = 0x811C_9DC5;
        let mut eat = |b: u8| {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        };
        eat(len as u8);
        for (i, &b) in body.iter().enumerate() {
            eat(if self.masked(i, len) { 0 } else { b });
        }
        Some(h)
    }

    /// `body`'s value as 14 bits, if `body` is the message `key` names: a
    /// lone byte is spread over 14 (`v << 7 | v`), as a 7-bit CC is.
    pub fn value(self, body: &[u8], key: u32) -> Option<U14> {
        if self.key(body)? != key {
            return None;
        }
        let hi = u16::from(body[self.at()] & 0x7F);
        let lo = if self.pair() {
            u16::from(body[self.at() + 1] & 0x7F)
        } else {
            hi
        };
        U14::new(hi << 7 | lo)
    }
}

/// A Roland-style checksum ends `m`: from some byte on, the sum with it
/// is a multiple of 128.
fn summed(m: &[u8], from: usize) -> bool {
    m[from..]
        .iter()
        .map(|&x| u32::from(x))
        .sum::<u32>()
        .is_multiple_of(128)
}

/// Where `a` and `b`, one control's messages at two values, keep the
/// value: the one byte they differ in, past a last byte that checksums
/// both. Never a pair: two bytes side by side that differ are as likely an
/// address and a value (a DX7's `pp vv`, two knobs) as an MSB and LSB, and
/// a pair's key would hear every address. A pair is learned from a carry
/// later (`carry`). `None`: not one control's (other lengths, nothing or
/// more than one byte differs).
pub fn learn(a: &[u8], b: &[u8]) -> Option<Shape> {
    let len = a.len();
    if len != b.len() || len == 0 || len > MAX_LEN {
        return None;
    }
    let check =
        len > 2 && a[len - 1] != b[len - 1] && (1..len - 1).any(|k| summed(a, k) && summed(b, k));
    [check, false].into_iter().find_map(|check| {
        let n = len - usize::from(check);
        let mut diff = (0..n).filter(|&i| a[i] != b[i]);
        let at = diff.next()?;
        let shape = match diff.next() {
            None => Shape::new(at as u8, false, check),
            Some(_) => None,
        }?;
        // The two must be the same message at two values.
        (shape.key(a)? == shape.key(b)?).then_some(shape)
    })
}

/// The most a carry steps the 14-bit value: a sweep's step, not a jump.
const CARRY_STEP: u16 = 16;

/// One carry heard (`carry`): the pair it suggests, which way it went and
/// the MSB it reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Carry {
    pair: Shape,
    up: bool,
    msb: u8,
}

impl Carry {
    pub const fn pair(self) -> Shape {
        self.pair
    }

    /// `next` is a second carry the same way on from this one (MSB m to
    /// m+1, then m+1 to m+2): a 14-bit sweep. Two knobs side by side can
    /// show one carry, never two in a row.
    pub fn confirms(self, next: Carry) -> bool {
        let on = if self.up {
            self.msb.checked_add(1)
        } else {
            self.msb.checked_sub(1)
        };
        next.pair == self.pair && next.up == self.up && Some(next.msb) == on
    }
}

/// A single-byte `shape`'s value as the LSB of a pair, if `last` to `next`
/// looks like its carry: the byte before it moves by one while the value
/// byte wraps (127 to 0 up, 0 to 127 down), the 14-bit value stepping at
/// most `CARRY_STEP`, every other byte kept. One is not proof: a pair is
/// learned once a second confirms it (`Carry::confirms`).
pub fn carry(shape: Shape, last: &[u8], next: &[u8]) -> Option<Carry> {
    let at = shape.at();
    if shape.pair() || at == 0 || last.len() != next.len() {
        return None;
    }
    let pair = Shape::new(at as u8 - 1, true, shape.check())?;
    if pair.key(last)? != pair.key(next)? {
        return None;
    }
    let (m0, l0, m1, l1) = (last[at - 1], last[at], next[at - 1], next[at]);
    let up = m1 == m0.wrapping_add(1) && l1 < l0;
    let down = m0 == m1.wrapping_add(1) && l1 > l0;
    let v = |m: u8, l: u8| u16::from(m & 0x7F) << 7 | u16::from(l & 0x7F);
    ((up || down) && v(m0, l0).abs_diff(v(m1, l1)) <= CARRY_STEP).then_some(Carry {
        pair,
        up,
        msb: m1,
    })
}
