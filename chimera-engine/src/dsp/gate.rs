// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The voice's amp gate (ADR 0110): after the filter, open while the key is
//! down; from note-off the voice may be no louder than `REACH` times the
//! engine feeding it, so a filter that self-oscillates dies with the note
//! instead of outliving it, and a normal release is left as it was.

use chimera_hal::BLOCK_SIZE;

/// A released note's block peak holds this much per block, so a low
/// note's waveform troughs don't flutter the gate.
const HOLD: f32 = 0.98;
/// What the filter may add to the engine's level: 12 dB, over the largest
/// resonant peak of a filter that isn't ringing by itself.
const REACH: f32 = 4.0;
/// The gate shuts no faster than this per block (21 ms in all), so a ring
/// that outlasts its engine is faded, not cut.
const FALL: f32 = 1.0 / 16.0;

/// The gate's gain, ramped per sample across each block (never snap).
#[derive(Clone, Copy, Debug)]
pub struct AmpGate {
    /// The engine's block peak since note-off, held up.
    env: f32,
    /// The last block's end gain.
    gain: f32,
}

fn peak(x: &[f32; BLOCK_SIZE]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(libm::fabsf(*s)))
}

impl AmpGate {
    pub const OPEN: Self = Self {
        env: 0.0,
        gain: 1.0,
    };

    /// A note-on: the engine's level starts over.
    pub fn restart(&mut self) {
        self.env = 0.0;
    }

    /// A key held with the gate open has nothing to do.
    pub fn idle(&self, held: bool) -> bool {
        held && self.gain == 1.0
    }

    /// The engine's raw `x` this block, released.
    pub fn feed(&mut self, x: &[f32; BLOCK_SIZE]) {
        self.env = peak(x).max(self.env * HOLD);
    }

    /// `y`, the voice after the filter, scaled by the gate: open while
    /// `held`, else closing to `REACH` times the engine's level.
    pub fn apply(&mut self, y: &mut [f32; BLOCK_SIZE], held: bool) {
        let p = peak(y);
        let to = if held || p <= 0.0 {
            1.0
        } else {
            (REACH * self.env / p).min(1.0)
        };
        let from = self.gain;
        let to = to.max(from - FALL);
        self.gain = to;
        if from == 1.0 && to == 1.0 {
            return;
        }
        let step = (to - from) / BLOCK_SIZE as f32;
        for (i, s) in y.iter_mut().enumerate() {
            *s *= from + step * i as f32;
        }
    }
}
