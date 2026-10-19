//! LOOPFM's oscillator: loopback FM in its closed form, one divide a
//! sample. The wave is the sum of harmonics `(1 − b0²)·(−b0)^(k−1)`, a
//! cosine at `b0 = 0` growing to a pulse as `|b0|` nears 1, always at
//! the clock's pitch.

use super::osc::{R, psin};
use crate::dsp::provenance::{Patent, PatentLine, Provenance};
use crate::dsp::{clampf, sqrt};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Textbook {
        cite: "Smyth, \"Loopback Frequency Modulation Using a Time-Varying Delay Line\", \
               DAFx-26, paper 36 (CC-BY 4.0): the closed form z(n) = (b0 + e^(jw0 nT)) / \
               (1 + b0 e^(jw0 nT)) and the b0/w0 mapping (eq. 26); our own Rust",
    },
    Provenance::Original {
        author: "AI-assisted, owner-directed and reviewed",
    },
];

pub const PATENT: &[PatentLine] = &[PatentLine {
    technique: "loopback FM, closed form",
    patent: Patent::NotChecked,
}];

/// log2 of the first alias's ceiling, −45 dB (0.0056).
const LOG2_ALIAS: f32 = -7.4806;

/// An oscillator's feedback coefficient `B` at BLEND `blend` and its own
/// WAVE: `0.95·blend²`, scaled by `1.5·wave` (WAVE 2/3, the default, is
/// ×1; WAVE 0 never loops), capped at 0.95.
pub fn loop_b(blend: f32, wave: f32) -> f32 {
    (0.95 * blend * blend * 1.5 * wave).min(0.95)
}

/// The closed form's coefficient for feedback `b` (paper eq. 26),
/// `(√(1 − b²) − 1)/b`: negative for `b > 0`, `−b/2` near 0.
pub fn b0_of(b: f32) -> f32 {
    if b.abs() < 1e-4 {
        -0.5 * b
    } else {
        (sqrt(1.0 - b * b) - 1.0) / b
    }
}

/// The largest `|b0|` at `inc` cycles a sample: harmonic `k` is
/// `(1 − b0²)·|b0|^(k−1)`, so with `K = 0.5/inc` harmonics under Nyquist
/// the first alias sits at −45 dB when `|b0|^(K−1) = 0.0056`. 0 when
/// `K ≤ 1`.
pub fn b0_cap(inc: f32) -> f32 {
    let k = 0.5 / inc;
    if k <= 1.0 {
        0.0
    } else {
        crate::dsp::algo::math::exp2(LOG2_ALIAS / (k - 1.0))
    }
}

/// One sample at `phase` (cycles): `Re z0 − b0`, DC-free, scaled to RMS
/// `R`.
#[inline(always)]
pub fn lbfm(phase: f32, b0: f32) -> f32 {
    // cos 2πφ, folded into psin's range
    let c = -psin(0.25 - (phase - 0.5).abs());
    let b2 = 1.0 + b0 * b0;
    let re = (2.0 * b0 + b2 * c) / (b2 + 2.0 * b0 * c);
    (re - b0) * (R * core::f32::consts::SQRT_2 / sqrt(1.0 - b0 * b0))
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
    jit: &mut f32,
    draw: &mut impl FnMut() -> f32,
) {
    let (mut p, mut j) = (*phase, *jit);
    let ((mut b, db), (mut ic, di)) = (b0, inc);
    for (o, m) in out.iter_mut().zip(mk.iter_mut()) {
        b += db;
        ic += di;
        let step = ic * j;
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
        *o = lbfm(p, b);
    }
    *phase = p;
    *jit = j;
}
