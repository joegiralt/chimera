// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! WOW WARBLE (spec § 2 Machines): CHOMPI's Warble, ideas only (ADR 0093).
//! A random target within ±33 cents at TAPE 1, redrawn every 0.1–0.6 s,
//! glided to by a one-pole of 0.2 s, once a block. It replaces the
//! capstan's transport, so it fits TAPE's bill. Cut it by removing this
//! module and WOW's arm.

use crate::dsp::xorshift32;

/// The deviation's reach at TAPE 1, cents: the capstan's wow and flutter
/// summed.
pub const WARBLE_CENTS: f32 = 33.0;
/// A target holds 0.1 s to 0.6 s.
pub const HOLD_MIN_S: f32 = 0.1;
pub const HOLD_SPAN_S: f32 = 0.5;
/// The glide's one-pole time constant.
pub const GLIDE_S: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Warble {
    rng: u32,
    target: f32,
    value: f32,
    /// Blocks left on this target.
    left: u32,
}

impl Default for Warble {
    fn default() -> Self {
        Self::new()
    }
}

impl Warble {
    pub const fn new() -> Self {
        Warble {
            rng: 0x2545_F491,
            target: 0.0,
            value: 0.0,
            left: 0,
        }
    }

    /// Each voice its own draws, from `s` (the voice's index).
    pub fn seed(&mut self, s: u32) {
        *self = Warble {
            rng: (s.wrapping_add(1).wrapping_mul(0x85EB_CA6B) ^ 0x2545_F491) | 1,
            ..Warble::new()
        };
    }

    /// A draw in [0, 1).
    fn unit(&mut self) -> f32 {
        (xorshift32(&mut self.rng) >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// The block's speed deviation, cents, at depth `tape` (0–1), its
    /// clock `block_rate` blocks a second.
    pub fn cents(&mut self, tape: f32, block_rate: f32) -> f32 {
        if self.left == 0 {
            self.target = WARBLE_CENTS * (2.0 * self.unit() - 1.0);
            self.left = ((HOLD_MIN_S + HOLD_SPAN_S * self.unit()) * block_rate) as u32;
            self.left = self.left.max(1);
        }
        self.left -= 1;
        self.value += (self.target - self.value) * (1.0 / (GLIDE_S * block_rate));
        self.value * tape
    }
}
