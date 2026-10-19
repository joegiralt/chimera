//! The seven operators and the sub's mix (spec § Combine). Each sees its
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
    Provenance::Textbook {
        cite: "Chowning, \"The Synthesis of Complex Audio Spectra by Means of Frequency \
               Modulation\", JAES 21(7), 1973 (FM; FM's through-zero linear read rate is \
               from first principles, the Intellijel DPO an instrument reference only)",
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

/// `p·xor16(a/p, b/p)` with `k = 32767/p` set once a block: the engine's
/// loop. The words saturate to i16: only an input past its peak bound
/// reaches −32768, where `xor16` stops at −32767.
#[inline(always)]
pub fn xor16_scaled(a: f32, b: f32, k: f32, p: f32) -> f32 {
    unword(word(a * k) ^ word(b * k)) * p
}

/// `v` as a 16-bit word, truncated and saturated (`as i32`, then clamped
/// to i16), sign-extended: VCVT.S16.F32 on the M7, NaN 0 either way.
#[inline(always)]
fn word(v: f32) -> i32 {
    #[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
    {
        let mut r = v;
        // SAFETY: one register-to-register VFP op (fixed-point VCVT, 0
        // fraction bits); no memory or stack touched. It may set FPSCR's
        // IOC or IXC flags; nothing reads them.
        unsafe {
            core::arch::asm!("vcvt.s16.f32 {r}, {r}, #0", r = inout(sreg) r,
                options(pure, nomem, nostack));
        }
        r.to_bits() as i32
    }
    #[cfg(not(all(target_arch = "arm", target_feature = "vfp2")))]
    {
        (v as i32).clamp(-32768, 32767)
    }
}

/// A word's low 16 bits, sign-extended, over 2¹⁵ (exact): VCVT.F32.S16
/// with 15 fraction bits on the M7.
#[inline(always)]
fn unword(w: i32) -> f32 {
    #[cfg(all(target_arch = "arm", target_feature = "vfp2"))]
    {
        let mut r = f32::from_bits(w as u32);
        // SAFETY: one register-to-register VFP op (fixed-point VCVT, 15
        // fraction bits, exact); no memory or stack touched.
        unsafe {
            core::arch::asm!("vcvt.f32.s16 {r}, {r}, #15", r = inout(sreg) r,
                options(pure, nomem, nostack));
        }
        r
    }
    #[cfg(not(all(target_arch = "arm", target_feature = "vfp2")))]
    {
        (((w << 16) >> 16) as f32) * (1.0 / 32768.0)
    }
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

/// One block's operator as the engine's loop runs it: MODE and its block
/// constants, at `scale` times `combine`'s level (2 for RING and XOR, where
/// `½(a + b)` is then `a + b`; the scalings are exact, so the bits are
/// `combine`'s doubled, as RING shows).
#[derive(Clone, Copy, Debug)]
pub struct Operator {
    pub mode: VaMode,
    /// The output is `scale · combine(…)`.
    pub scale: f32,
    kx: f32,
    p2: f32,
    q: f32,
    liq: f32,
    crush: Crusher,
}

impl Operator {
    /// Once a block, from the oscillators' peak bounds and the crusher.
    #[inline(always)]
    pub fn new(mode: VaMode, pk: Peaks, crush: &Crusher) -> Self {
        let scale = if matches!(mode, VaMode::Ring | VaMode::Xor) {
            2.0
        } else {
            1.0
        };
        let p = pk.a.max(pk.b);
        let q = 0.5 * (pk.a + pk.b);
        Self {
            mode,
            scale,
            kx: 32767.0 / p,
            p2: scale * p,
            q,
            // BITS crushes `½(a + b) / q`: the ½ rides in the levels (exact)
            liq: 0.5 * crush.levels() / q,
            crush: *crush,
        }
    }

    /// `scale · combine(mode, a, b, x, k, …)`, `ks` being RING's `k ·
    /// scale`.
    #[inline(always)]
    pub fn at(&self, a: f32, b: f32, x: f32, ks: f32) -> f32 {
        let sum = a + b;
        match self.mode {
            VaMode::Mix | VaMode::Sync => a + (b - a) * x,
            VaMode::Ring => sum + (ks * a * b - sum) * x,
            VaMode::Xor => sum + (xor16_scaled(a, b, self.kx, self.p2) - sum) * x,
            VaMode::Bits => self.q * self.crush.process_scaled(sum, self.liq),
            // OSC 2 is in OSC 1's phase already
            VaMode::Fm => a,
            VaMode::LoopFm => a + (b - a) * (0.5 * x),
        }
    }
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
        VaMode::Fm => a,
        VaMode::LoopFm => a + (b - a) * (0.5 * x),
    }
}

/// The sub `s` mixed in at level `sub`, level-stable.
#[inline(always)]
pub fn with_sub(c: f32, s: f32, sub: f32) -> f32 {
    (c + sub * s) / sqrt(1.0 + sub * sub)
}
