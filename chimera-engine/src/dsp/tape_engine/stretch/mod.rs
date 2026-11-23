// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! STRETCH (ADR 0204): two TYPEs. CLASSIC (`classic`) is the Akai S950's
//! two-grain splice, MONO or POLY, as it always was: its notes render bit
//! for bit as before. GRAIN (`grain`) plays the source over LENGTH seconds
//! at the key's pitch in four overlapping grains on the shared grain core
//! (`dsp::grain`, through `cloud`). TYPE latches at note-on.

use chimera_hal::BLOCK_SIZE;

pub mod classic;
mod cloud;
pub mod grain;
pub mod period;
pub mod units;

pub use classic::{GrainClock, HALF_SPEED, WRAP_SAMPLES, d_time_s, lock, locks, time_speed};
pub use grain::HOLD_S;

use super::heads::Dir;
use super::params::{StretchType, TapeEngineParams};
use super::read::SourceView;
use super::{Macros, Probe};

/// A STRETCH note: its TYPE's state.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Note {
    Classic(classic::Stretch),
    Grain(grain::Grains),
}

/// `Note`'s idle image, CLASSIC's: in bank 2 with the code that copies it
/// (an anonymous constant would land in bank 1).
#[cfg_attr(feature = "bank2", unsafe(link_section = ".bank2.rodata.stretch_note"))]
pub static IDLE: Note = Note::Classic(classic::Stretch::IDLE);

impl Note {
    /// A note on `v` at `inc` frames a sample in `p`'s TYPE: CLASSIC's from
    /// its macros `m`, GRAIN's from the voice's draw `seed`.
    pub fn on(
        p: &TapeEngineParams,
        v: &SourceView<'_>,
        inc: f32,
        (m, seed): (&Macros, u32),
        sr: u32,
    ) -> Note {
        match p.kind {
            // GRAIN reads a whole row (`row_taps`); STRETCH is RAM's, so
            // a span never comes, and would play nothing.
            StretchType::Grain => Note::Grain(match v.whole_row() {
                Some(row) => grain::Grains::on(p, &row, inc, seed, sr),
                None => grain::Grains::ended(),
            }),
            StretchType::Mono | StretchType::Poly => {
                Note::Classic(classic::Stretch::on(p, v, inc, m, sr))
            }
        }
    }

    /// The TYPE it plays.
    pub fn grains(&self) -> bool {
        matches!(self, Note::Grain(_))
    }

    pub fn probe(&self) -> Probe {
        match self {
            Note::Classic(st) => {
                let (head, other, gain) = st.probe();
                Probe {
                    head,
                    other: Some(other),
                    gain,
                    level: 0,
                    from: None,
                }
            }
            Note::Grain(g) => g.probe(),
        }
    }

    pub fn dirs(&self) -> [Dir; 2] {
        match self {
            Note::Classic(st) => st.dirs(),
            Note::Grain(g) => [g.dir(); 2],
        }
    }

    /// A block at `inc` ramped, the macros `m` eased; false once a GRAIN
    /// note has ended (a CLASSIC one plays while its key is held).
    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        v: &SourceView<'_>,
        inc: (f32, f32),
        m: &Macros,
        p: &TapeEngineParams,
        sr: u32,
    ) -> bool {
        match self {
            Note::Classic(st) => {
                st.render(out, v, inc, (m.position, m.motion, m.size), p.end, sr);
                true
            }
            Note::Grain(g) => match v.whole_row() {
                Some(row) => g.render(out, &row, inc, m.motion, p, sr),
                None => {
                    out.fill(0.0);
                    false
                }
            },
        }
    }

    /// Splices and wraps (CLASSIC) or grains started (GRAIN): for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn events(&self) -> u32 {
        match self {
            Note::Classic(st) => st.events,
            Note::Grain(g) => g.events,
        }
    }

    /// CLASSIC's wraps, lifted and bare: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn wraps(&self) -> [u32; 2] {
        match self {
            Note::Classic(st) => st.wraps,
            Note::Grain(_) => [0; 2],
        }
    }

    /// CLASSIC's grain clock: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clock(&self) -> GrainClock {
        match self {
            Note::Classic(st) => st.clock(),
            Note::Grain(_) => classic::Stretch::IDLE.clock(),
        }
    }
}
