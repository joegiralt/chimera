// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! TRON (spec § 2 Machines): the Mellotron M400 (Streetly Electronics),
//! about 8 s of tape a key. Every key gets SIZE's length of sound at every
//! pitch (owner, 2026-10-05): the tape runs out in output time, and the
//! head's travel scales with the pitch, so END may end a high note first.
//! Key up lifts the pressure pad: the tape fades through FADE's one-pole.

use super::params::drag_s;
use crate::dsp::algo::math::{clampf, exp2};

/// SIZE 1's tape: an M400 strip.
pub const TRON_LEN_MAX_S: f32 = 8.0;
/// The run-out's fall, a one-pole: 2 ms, so a run-out ends within 20 ms.
/// Its first block eases into it (`run_out::onset`).
pub const RUN_OUT_S: f32 = 0.002;
/// Under −80 dB a released note ends.
pub const TRON_FLOOR: f32 = 1e-4;

/// A TRON key's tape, seconds: 0.25 to 8 s, the same at every pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapeSeconds(f32);

impl TapeSeconds {
    /// An M400's whole strip.
    pub const MAX: TapeSeconds = TapeSeconds(TRON_LEN_MAX_S);

    /// LENGTH's tape (`tron_len_s`).
    pub fn of_length(size: f32) -> TapeSeconds {
        TapeSeconds(tron_len_s(size))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    /// The output samples it plays at `sample_rate`.
    pub fn samples(self, sample_rate: u32) -> u32 {
        tron_run_samples(self.0, sample_rate)
    }
}

/// LENGTH: `0.25 · 32^size` seconds, 0.25 to 8 s.
pub fn tron_len_s(size: f32) -> f32 {
    0.25 * exp2(5.0 * clampf(size, 0.0, 1.0))
}

/// LENGTH over a strip of `strip_s` seconds (START to END at ROOT): LENGTH
/// 1 is the whole strip, up to 8 s, so its top never sits past END on a
/// short source (#698); 0.25 s at 0 as before.
pub fn tron_len_s_in(size: f32, strip_s: f32) -> f32 {
    let top = clampf(strip_s, 0.25, TRON_LEN_MAX_S);
    0.25 * libm::powf(top * 4.0, clampf(size, 0.0, 1.0))
}

/// The output samples a note sounds before its tape runs out: the same at
/// every pitch.
pub fn tron_run_samples(len_s: f32, sample_rate: u32) -> u32 {
    libm::roundf(len_s * sample_rate as f32) as u32
}

/// The release's gain a sample: a one-pole of `drag_s(fade)`.
pub fn release_k(fade: f32, sample_rate: u32) -> f32 {
    libm::expf(-1.0 / (drag_s(fade) * sample_rate as f32))
}
