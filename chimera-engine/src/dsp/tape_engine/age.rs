// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! AGE, the wear (spec 2026-10-04 § 2): a one-pole low-pass whose corner
//! falls from the source's band edge (`AGE_TOP_HZ` at most) by `(1/8)^age`, white hiss up to −42 dBFS RMS, and above
//! ½ dropouts: the gain dipping `(age − ½)·2·9` dB at random for 20–80 ms,
//! eased a block at a time. Each voice has its own hiss and dropouts (a
//! Mellotron's strip of tape a note).

use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use crate::dsp::algo::math::exp2;
use crate::dsp::xorshift32;

/// The corner at AGE 0; it falls by `AGE_FALL` at AGE 1 (2.5 kHz).
pub const AGE_TOP_HZ: f32 = 20_000.0;
pub const AGE_FALL: f32 = 0.125;
/// The hiss at AGE 1, RMS: −42 dBFS.
pub const HISS_RMS: f32 = 0.007_943_282;
/// Dropouts start above this AGE, one each `DROPOUT_EVERY_S / ((age − ½)·2)`
/// on average, `(age − ½)·2·DROPOUT_DB` deep.
pub const DROPOUT_FROM: f32 = 0.5;
pub const DROPOUT_EVERY_S: f32 = 2.0;
pub const DROPOUT_DB: f32 = 9.0;
/// A dropout's length, blocks: 20 to 80 ms at 48 kHz.
const DROPOUT_MIN: u32 = 15;
const DROPOUT_SPAN: u32 = 46;
const _: () = assert!(DROPOUT_MIN as usize * BLOCK_SIZE == 960);
const _: () = assert!((DROPOUT_MIN + DROPOUT_SPAN - 1) as usize * BLOCK_SIZE == 3_840);
/// The gain's share of the way to its target each block: the macros' ease.
const EASE: f32 = 0.3;
/// Within this of 1 the gain is 1 (−87 dB): the stage can go quiet.
const SETTLE: f32 = 4e-5;
/// The one-pole opens to a wire below this AGE (its `a` blended to 1), so
/// AGE just above 0 sounds as AGE off.
pub const AGE_OPEN: f32 = 0.05;

/// AGE runs, and is billed, whenever it can be heard: stored above 0, or
/// routed (`Voice::fx_runs`' rule).
pub fn age_runs(stored: f32, routed: bool) -> bool {
    stored > 0.0 || routed
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Age {
    /// The one-pole's state: the last sample out of it.
    y: f32,
    /// The dropout gain, and where it eases to.
    g: f32,
    target: f32,
    /// Blocks of the dropout left; 0 when none.
    left: u32,
    rng: u32,
    /// The corner at AGE 0: the source's own band edge (`set_band`).
    top: f32,
}

impl Default for Age {
    fn default() -> Self {
        Self::new()
    }
}

impl Age {
    pub const fn new() -> Self {
        Self {
            y: 0.0,
            g: 1.0,
            target: 1.0,
            left: 0,
            rng: 0x6A09_E667,
            top: AGE_TOP_HZ,
        }
    }

    /// The note's source plays at `rate`: AGE's corner starts at its band
    /// edge, `rate / 2` (`AGE_TOP_HZ` at most), so AGE darkens over its
    /// whole travel on a low-rate source too (#698).
    pub fn set_band(&mut self, rate: u16) {
        self.top = (0.5 * f32::from(rate)).min(AGE_TOP_HZ);
    }

    /// Its own hiss and dropouts, from `s`.
    pub fn seed(&mut self, s: u32) {
        self.rng = (s.wrapping_mul(0x9E37_79B9) ^ 0x6A09_E667) | 1;
    }

    /// The dropout gain: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn gain(&self) -> f32 {
        self.g
    }

    /// A fresh note: the filter starts from silence, with no dropout.
    pub fn hold(&mut self) {
        (self.y, self.g, self.target, self.left) = (0.0, 1.0, 1.0, 0);
    }

    /// The block `x` worn by `age` (0 to 1). At 0, with no dropout easing
    /// back, it passes `x` untouched, the filter following it so the wear
    /// comes back without a step.
    #[inline(always)]
    pub fn process(&mut self, x: &mut [f32; BLOCK_SIZE], age: f32) {
        if age <= 0.0 && self.g == 1.0 {
            self.y = x[BLOCK_SIZE - 1];
        } else {
            self.wear(x, age);
        }
    }

    #[inline(never)]
    fn wear(&mut self, x: &mut [f32; BLOCK_SIZE], age: f32) {
        // 1 − e^(−2π·corner/rate), the corner `top · 2^(−3·age)`.
        const K: f32 = core::f32::consts::TAU / SAMPLE_RATE as f32 * core::f32::consts::LOG2_E;
        let shut = exp2(-K * self.top * exp2(-3.0 * age));
        let a = 1.0 - shut * (age * (1.0 / AGE_OPEN)).min(1.0);
        self.dropout(age);
        let g0 = self.g;
        let mut g1 = g0 + EASE * (self.target - g0);
        if self.target == 1.0 && 1.0 - g1 < SETTLE {
            g1 = 1.0;
        }
        self.g = g1;
        let dg = (g1 - g0) * (1.0 / BLOCK_SIZE as f32);
        // Uniform on ±2³¹ has an RMS of 2³¹/√3.
        let h = age * HISS_RMS * (1.732_050_8 / 2_147_483_648.0);
        let (mut y, mut g, mut rng) = (self.y, g0, self.rng);
        // Four a turn: the loop's own count and branch, a quarter each.
        for q in x.as_chunks_mut::<4>().0 {
            for s in q {
                y += a * (*s - y);
                g += dg;
                *s = y * g + xorshift32(&mut rng) as i32 as f32 * h;
            }
        }
        (self.y, self.rng) = (y, rng);
    }

    /// A block of the dropouts: one in progress counts down, else one may
    /// start, its depth and length drawn as it does. AGE at ½ or under ends
    /// one at once: the gain eases back, so none outlives its AGE.
    fn dropout(&mut self, age: f32) {
        let over = (age - DROPOUT_FROM) * 2.0;
        if over <= 0.0 {
            if self.left > 0 {
                (self.left, self.target) = (0, 1.0);
            }
            return;
        }
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.target = 1.0;
            }
            return;
        }
        // A block's chance: the mean rate over the block rate, in 2⁻³².
        let chance = over / DROPOUT_EVERY_S * (BLOCK_SIZE as f32 / SAMPLE_RATE as f32);
        if xorshift32(&mut self.rng) >= (chance * 4_294_967_296.0) as u32 {
            return;
        }
        let r = xorshift32(&mut self.rng) >> 16;
        self.left = DROPOUT_MIN + ((r * DROPOUT_SPAN) >> 16);
        // 10^(−dB/20).
        self.target = exp2(-over * DROPOUT_DB * (core::f32::consts::LOG2_10 / 20.0));
    }
}
