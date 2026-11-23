// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Seeded xorshift32, one per ring: the same seed plays the same music.

use super::RingIdx;

/// A percentage, 0..=100: CHANCE, MUTATE, GATE.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pct(u8);

impl Pct {
    pub const ZERO: Pct = Pct(0);
    pub const FULL: Pct = Pct(100);

    pub const fn new(p: u8) -> Option<Pct> {
        if p <= 100 { Some(Pct(p)) } else { None }
    }

    pub const fn clamped(p: u8) -> Pct {
        Pct(if p > 100 { 100 } else { p })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rng(u32);

impl Rng {
    pub const fn new(seed: u32) -> Rng {
        // Zero is xorshift's fixed point.
        Rng(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    /// Ring `r`'s own stream: SplitMix32 of the scene seed and the ring, so
    /// editing one ring never reshuffles another's dice.
    pub const fn for_ring(seed: u32, r: RingIdx) -> Rng {
        let mut z = seed.wrapping_add((r.get() as u32 + 1).wrapping_mul(0x9E37_79B9));
        z = (z ^ (z >> 16)).wrapping_mul(0x85EB_CA6B);
        z = (z ^ (z >> 13)).wrapping_mul(0xC2B2_AE35);
        Rng::new(z ^ (z >> 16))
    }

    pub fn next_u32(&mut self) -> u32 {
        crate::dsp::xorshift32(&mut self.0)
    }

    /// 0..n; 0 when n is 0.
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// True with probability `p`: never at 0, always at 100. One draw
    /// either way.
    pub fn chance(&mut self, p: Pct) -> bool {
        self.below(100) < p.0 as u32
    }
}
