//! The five operators and the sub's mix (spec § Combine). Each sees its
//! inputs within their block peak bounds and stays within 0.9; XOR and
//! BITS alias by nature and are exempt from the alias gate.

use super::VaMode;
use super::osc::R;
use crate::dsp::algo::math::exp2;
use crate::dsp::provenance::{Patent, PatentLine, Provenance};
use crate::dsp::ring::rint;
use crate::dsp::{clampf, sqrt};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Textbook {
        cite: "Kleimola, \"Audio Synthesis by Bitwise Logical Modulation\", DAFx-08, 2008 (XOR)",
    },
    Provenance::Original {
        author: "AI-assisted, owner-directed and reviewed",
    },
];

pub const PATENT: &[PatentLine] = &[PatentLine {
    technique: "XOR of two oscillators' sample words",
    patent: Patent::NotChecked,
}];

/// Each input as a 16-bit signed word (clamped to ±1, truncated), the
/// words XORed, and back.
#[inline(always)]
pub fn xor16(a: f32, b: f32) -> f32 {
    let w = |v: f32| (clampf(v, -1.0, 1.0) * 32767.0) as i16;
    (w(a) ^ w(b)) as f32 / 32768.0
}

/// `p·xor16(a/p, b/p)` with `k = 32767/p` and `g = p/32768` set once a
/// block: the engine's loop (the 16-bit XOR on the low halves of the
/// words, sign-extended). The words saturate to i16 (one SSAT): only an
/// input past its peak bound reaches −32768, where `xor16` stops at −32767.
#[inline(always)]
pub fn xor16_scaled(a: f32, b: f32, k: f32, g: f32) -> f32 {
    let w = |v: f32| ((v * k) as i32).clamp(-32768, 32767);
    ((((w(a) ^ w(b)) << 16) >> 16) as f32) * g
}

/// BITS' depth at AMOUNT `x`: `16 · 8^(−x)`, 16 → 2 bits on a log curve.
#[inline]
pub fn bits(x: f32) -> f32 {
    16.0 * exp2(-3.0 * x)
}

/// The bit crusher at one block's depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crusher {
    levels: f32,
    inv: f32,
}

impl Crusher {
    /// 16 bits.
    pub const NEW: Self = Self {
        levels: 32768.0,
        inv: 1.0 / 32768.0,
    };

    /// Once a block, at AMOUNT `x`.
    pub fn set(&mut self, x: f32) {
        self.levels = exp2(bits(clampf(x, 0.0, 1.0)) - 1.0);
        self.inv = 1.0 / self.levels;
    }

    /// `v` (within ±1) on the grid of `levels` steps a unit, within ±1: a
    /// non-integer `levels` would round its top step past 1.
    #[inline(always)]
    pub fn process(&self, v: f32) -> f32 {
        clampf(rint(v * self.levels) * self.inv, -1.0, 1.0)
    }

    /// `process` of `v / q`, `iq = 1/q` folded into the levels: once a
    /// block (BITS' engine loop).
    #[inline(always)]
    pub fn process_scaled(&self, v: f32, levels_iq: f32) -> f32 {
        clampf(rint(v * levels_iq) * self.inv, -1.0, 1.0)
    }

    pub fn levels(&self) -> f32 {
        self.levels
    }
}

/// The block's peak bounds of OSC 1 and OSC 2 (`osc::peak_bound`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peaks {
    pub a: f32,
    pub b: f32,
}

/// RING's gain: the ring term at the oscillators' level, capped so the
/// product stays within 0.9.
#[inline]
pub fn ring_gain(pk: Peaks) -> f32 {
    (1.0 / R).min(0.9 / (pk.a * pk.b))
}

/// One sample of `mode` at AMOUNT `x`. `k` is RING's gain, ramped by the
/// caller; XOR and BITS scale their inputs to unit peak by `pk`.
#[inline(always)]
pub fn combine(mode: VaMode, a: f32, b: f32, x: f32, k: f32, pk: Peaks, crush: &Crusher) -> f32 {
    let half = 0.5 * (a + b);
    match mode {
        VaMode::Mix | VaMode::Sync => a + (b - a) * x,
        VaMode::Ring => half + (k * a * b - half) * x,
        VaMode::Xor => {
            let p = pk.a.max(pk.b);
            half + (p * xor16(a / p, b / p) - half) * x
        }
        VaMode::Bits => {
            let q = 0.5 * (pk.a + pk.b);
            q * crush.process(half / q)
        }
    }
}

/// The sub `s` mixed in at level `sub`, level-stable.
#[inline(always)]
pub fn with_sub(c: f32, s: f32, sub: f32) -> f32 {
    (c + sub * s) / sqrt(1.0 + sub * sub)
}
