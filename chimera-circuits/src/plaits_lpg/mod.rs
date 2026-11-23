// Copyright 2014-2016 Emilie Gillet (Mutable Instruments Plaits and stmlib, MIT; notice below)
// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later AND MIT

//! Plaits' low-pass gate, ported as a behaviour reference for the 292B
//! (lpg-circuit lab, `lpg_gate.rs`): `plaits/dsp/fx/low_pass_gate.h` and
//! `plaits/dsp/envelope.h` (LPGEnvelope), with the parameter mapping of
//! `plaits/dsp/voice.cc` and stmlib's `Svf` (FREQUENCY_DIRTY).
//! Copyright 2014–2016 Emilie Gillet, MIT licence:
//!
//!   Permission is hereby granted, free of charge, to any person obtaining a
//!   copy of this software and associated documentation files (the
//!   "Software"), to deal in the Software without restriction, including
//!   without limitation the rights to use, copy, modify, merge, publish,
//!   distribute, sublicense, and/or sell copies of the Software, and to
//!   permit persons to whom the Software is furnished to do so, subject to
//!   the following conditions: The above copyright notice and this
//!   permission notice shall be included in all copies or substantial
//!   portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT
//!   WARRANTY OF ANY KIND.
//!
//! The licence's disclaimer in full: THE SOFTWARE IS PROVIDED "AS IS",
//! WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
//! LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
//! PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
//! HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
//! AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
//! CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//!
//! One state does three jobs: the gain (a true VCA), the cutoff
//! (0.003 + 0.3·state⁴ + 0.04·colour of fs) and, through its own level,
//! the fall's rate. As a voice: a gate rise strikes LEVEL with the gate
//! checks' 1 ms/10 ms ping × velocity; CUTOFF sets a resting LEVEL (the one
//! whose settled cutoff is CUTOFF; below 0.023·fs it rests shut); RES is
//! ignored; DECAY and COLOUR at ½, the lab's setting.

#[cfg(test)]
mod tests;

use crate::{Controls, Entry, FS, Voice};

/// voice.cc's kBlockSize: the envelope and filter update once a block.
pub const P_BLOCK: usize = 12;
/// The lab's hidden LPG settings: DECAY ½, COLOUR ½ (mid).
pub const DECAY: f64 = 0.5;
pub const COLOUR: f64 = 0.5;

fn semis(x: f64) -> f64 {
    2f64.powf(x / 12.0)
}

/// LPGEnvelope's two decay rates at `decay` and `colour` (voice.cc).
fn rates(decay: f64, colour: f64) -> (f64, f64) {
    let short = (200.0 * P_BLOCK as f64) / FS * semis(-96.0 * decay);
    let tail = (20.0 * P_BLOCK as f64) / FS * semis(-72.0 * decay + 12.0 * colour) - short;
    (short, tail)
}

/// One block of LPGEnvelope (ProcessLP): `state` toward the compressed
/// level `l`. Returns the new state.
fn envelope(state: f64, l: f64, short: f64, tail: f64) -> f64 {
    // voice.cc: compressed_level = 1.3 l / (0.3 + |l|), once a block.
    let input = 1.3 * l / (0.3 + l.abs());
    let err = input - state;
    let coef = if err > 0.0 {
        0.6
    } else {
        short + (1.0 - state.powi(4)) * tail
    };
    state + coef * err
}

/// Plaits' LPGEnvelope + LowPassGate, the level input patched (ProcessLP),
/// DECAY and COLOUR as given (0..1). `level` a sample, read at each block's
/// first.
pub fn plaits(x: &[f64], level: &[f64], decay: f64, colour: f64) -> Vec<f64> {
    let (short, tail) = rates(decay, colour);
    let hf = colour;
    let mut state = 0.0f64;
    let mut prev_gain = 0.0f64;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    let mut y = vec![0.0; x.len()];
    for b in 0..x.len().div_ceil(P_BLOCK) {
        let i0 = b * P_BLOCK;
        let end = (i0 + P_BLOCK).min(x.len());
        // The bleed's tail is the state before this block's update.
        let tail2 = (1.0 - state) * (1.0 - state);
        state = envelope(state, level[i0], short, tail);
        let gain = state;
        let freq = 0.003 + 0.3 * state.powi(4) + hf * 0.04;
        let bleed = (tail2 + (1.0 - tail2) * hf) * hf * hf;
        // Svf::set_f_q<FREQUENCY_DIRTY>(freq, 0.4)
        let a = 3.736e-01 * std::f64::consts::PI.powi(3);
        let g = freq * (std::f64::consts::PI + a * freq * freq);
        let r = 1.0 / 0.4;
        let h = 1.0 / (1.0 + r * g + g * g);
        let n = (end - i0) as f64;
        for (k, i) in (i0..end).enumerate() {
            let gm = prev_gain + (gain - prev_gain) * (k + 1) as f64 / n;
            let s = x[i] * gm;
            let hp = (s - r * s1 - g * s1 - s2) * h;
            let bp = g * hp + s1;
            s1 = g * hp + bp;
            let lp = g * bp + s2;
            s2 = g * bp + lp;
            y[i] = lp + (s - lp) * bleed;
        }
        prev_gain = gain;
    }
    y
}

/// The state (= gain) and cutoff (Hz) a block, for a level input.
pub fn trace(level: &[f64], decay: f64, colour: f64) -> Vec<(f64, f64)> {
    let (short, tail) = rates(decay, colour);
    let mut state = 0.0f64;
    (0..level.len() / P_BLOCK)
        .map(|b| {
            state = envelope(state, level[b * P_BLOCK], short, tail);
            let freq = 0.003 + 0.3 * state.powi(4) + colour * 0.04;
            (state, freq * FS)
        })
        .collect()
}

/// The resting LEVEL whose settled cutoff is `hz` (0 below the floor).
pub fn rest_level(hz: f64, colour: f64) -> f64 {
    let s = ((hz / FS - 0.003 - 0.04 * colour) / 0.3)
        .max(0.0)
        .powf(0.25);
    let s = s.min(1.0);
    // invert 1.3 l / (0.3 + l); s → 1.3 is l → ∞, so 1 is l = 1.
    0.3 * s / (1.3 - s)
}

struct Port;

impl Voice for Port {
    fn render(&mut self, x: &[f64], c: &[Controls]) -> Vec<f64> {
        let level: Vec<f64> = crate::buchla292b::stim::strikes(c)
            .into_iter()
            .zip(c)
            .map(|(s, c)| s + rest_level(c.cutoff, COLOUR))
            .collect();
        plaits(x, &level, DECAY, COLOUR)
    }
}

pub fn models() -> Vec<Entry> {
    vec![Entry {
        name: "plaits_lpg/port",
        about: "Plaits' LPG (MIT, Emilie Gillet), DECAY and COLOUR 1/2: the behaviour reference the 292B was checked against",
        make: || Box::new(Port),
    }]
}
