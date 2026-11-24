// After Tamara Smyth, Loopback Frequency Modulation, DAFx-26 (CC BY 4.0; see THIRD_PARTY.md)
// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! LOOPFM's oscillator: loopback FM in its closed form, one divide a
//! sample. The wave is the sum of harmonics `(1 − b0²)·(−b0)^(k−1)`, a
//! cosine at `b0 = 0` growing to a pulse as `|b0|` nears 1, always at
//! the clock's pitch.

use super::osc::{R, psin};
use super::stability::WrapJitter;
use crate::dsp::provenance::{Patent, PatentLine, Provenance};
use crate::dsp::{clampf, sqrt};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Textbook {
        cite: "Smyth, \"Loopback Frequency Modulation Using a Time-Varying Delay Line\", \
               DAFx-26, paper 36 (CC-BY 4.0): the closed form z(n) = (b0 + e^(jw0 nT)) / \
               (1 + b0 e^(jw0 nT)) (eq. 25); b0 set directly by our own curve; our own Rust",
    },
    Provenance::Original {
        author: "AI-assisted, directed and reviewed by the maintainer",
    },
];

pub const PATENT: &[PatentLine] = &[PatentLine {
    technique: "loopback FM, closed form",
    patent: Patent::NotChecked,
}];

/// log2 of the first alias's ceiling, −45 dB (0.0056).
const LOG2_ALIAS: f32 = -7.4806;

/// log2(100): at the knob's top the −45 dB harmonic is the 101st.
const LOG2_REACH: f32 = 6.643_856;

/// The `|b0|` whose harmonic `n + 1` sits at −45 dB: `|b0|^n = 0.0056`.
fn reach(n: f32) -> f32 {
    crate::dsp::algo::math::exp2(LOG2_ALIAS / n)
}

/// An oscillator's `b0` at BLEND `blend` and its own WAVE, before the cap
/// (#683, ADR 0166): the harmonic at −45 dB rises in octaves, `100^e + 1`
/// with `e = min(1, 1.5·wave·blend)`, so `|b0|` runs 0.0056 → 0.95 on an
/// even curve (WAVE 2/3, the default, is ×1; WAVE 0 never loops). Negative,
/// the paper's root; 0 at `e = 0`.
pub fn target_b0(blend: f32, wave: f32) -> f32 {
    let e = (1.5 * wave * blend).min(1.0);
    if e <= 0.0 {
        0.0
    } else {
        -reach(crate::dsp::algo::math::exp2(e * LOG2_REACH))
    }
}

/// The largest `|b0|` at `inc` cycles a sample: harmonic `k` is
/// `(1 − b0²)·|b0|^(k−1)`, so with `K = 0.5/inc` harmonics under Nyquist
/// the first alias sits at −45 dB when `|b0|^(K−1) = 0.0056`. 0 when
/// `K ≤ 1`.
pub fn b0_cap(inc: f32) -> f32 {
    let k = 0.5 / inc;
    if k <= 1.0 { 0.0 } else { reach(k - 1.0) }
}

/// The scale's peak: twice a saw's at RMS `R`, `2√3·R`, the end of the
/// filter LEVEL clipper's knee at INIT (MIX's saws peak at its start).
pub const PEAK: f32 = 2.0 * 1.732_050_8 * R;
/// The `|b0|` where RMS `R` reaches `PEAK`: `√2·√((1 + a)/(1 − a)) = 2√3`.
pub const LEAN: f32 = 5.0 / 7.0;

/// The output scale for `b0` (`|b0| < 1`): RMS `R`, `R·√2/√(1 − b0²)`, up
/// to `|b0| = LEAN`; past it the peak `scale·(1 + |b0|)` is held at `PEAK`
/// (#683, ADR 0166), so the sharp pulse leans in RMS (−8.1 dB at 0.95)
/// rather than clip.
#[inline(always)]
pub fn scale_of(b0: f32) -> f32 {
    let a = b0.abs();
    if a > LEAN {
        PEAK / (1.0 + a)
    } else {
        R * core::f32::consts::SQRT_2 / sqrt(1.0 - b0 * b0)
    }
}

/// One sample at `phase` (cycles), `|b0| < 1`: `Re z0 − b0`, DC-free,
/// at `scale_of(b0)`.
pub fn lbfm(phase: f32, b0: f32) -> f32 {
    lbfm_at(phase, b0, scale_of(b0))
}

/// `lbfm` with `scale = scale_of(b0)` given (it ramps per block).
#[inline(always)]
fn lbfm_at(phase: f32, b0: f32, scale: f32) -> f32 {
    // cos 2πφ, folded into psin's range
    let c = -psin(0.25 - (phase - 0.5).abs());
    let b2 = 1.0 + b0 * b0;
    let re = (2.0 * b0 + b2 * c) / (b2 + 2.0 * b0 * c);
    (re - b0) * scale
}

/// `N` samples of one oscillator into `out`: the clock advances
/// `inc·jit` a sample and `b0` ramps; both are (start, step a sample).
/// A wrap writes `mk[i] = [t, t]` when `MARKS` (OSC 1: the sub flips at
/// the wrap), `NO_WRAP` otherwise, and draws the next jitter.
#[inline(never)]
pub fn render_loop<const N: usize, const MARKS: bool>(
    phase: &mut f32,
    b0: (f32, f32),
    inc: (f32, f32),
    out: &mut [f32; N],
    mk: &mut [[f32; 2]; N],
    jit: &mut WrapJitter,
    draw: &mut impl FnMut() -> WrapJitter,
) {
    let (mut p, mut j) = (*phase, *jit);
    let ((mut b, db), (mut ic, di)) = (b0, inc);
    // the scale ramps with b0, one sqrt a block
    let mut sc = scale_of(b);
    let dsc = (scale_of(b + db * N as f32) - sc) * (1.0 / N as f32);
    for (o, m) in out.iter_mut().zip(mk.iter_mut()) {
        b += db;
        sc += dsc;
        ic += di;
        let step = ic * j.factor();
        p += step;
        let mut w = super::osc::NO_WRAP;
        if p >= 1.0 {
            p -= 1.0;
            w = clampf(p / step, 0.0, 1.0);
            j = draw();
        }
        if MARKS {
            *m = [w, w];
        }
        *o = lbfm_at(p, b, sc);
    }
    *phase = p;
    *jit = j;
}
