// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! WALK's DRIFT (ADR 0203): at each note-on POSITION steps by a random
//! amount of up to DRIFT windows either way, from where the last note's
//! step left it, so repeats wander through the source (Octatrack's random
//! start, carried from note to note). Each voice draws from its own seeded
//! xorshift, so a render repeats. The place wraps at the window's ends.
//! DRIFT 0 rests at POSITION and draws nothing.

use super::heads::Window;
use super::params::Drift;
use crate::dsp::algo::math::clampf;

/// Where DRIFT has wandered, in windows: always in `[0, 1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place(f32);

impl Place {
    pub const ZERO: Place = Place(0.0);

    /// `x` wrapped into `[0, 1)`; a NaN is 0.
    pub fn wrapped(x: f32) -> Place {
        let w = x - libm::floorf(x);
        Place(if (0.0..1.0).contains(&w) { w } else { 0.0 })
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A voice's own draws: a seeded xorshift, never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftRng(u32);

impl DriftRng {
    /// Before its seed.
    pub const IDLE: DriftRng = DriftRng(0x2545_F491);

    /// Voice `s`'s: distinct from its wow's (`TapeDeck::seed`).
    pub const fn seed(s: u32) -> DriftRng {
        DriftRng((s.wrapping_mul(0x85EB_CA6B) ^ Self::IDLE.0) | 1)
    }

    /// A uniform draw in `[−1, 1)`.
    fn draw(&mut self) -> f32 {
        crate::dsp::xorshift32(&mut self.0) as i32 as f32 * (1.0 / 2_147_483_648.0)
    }
}

/// The place after a note-on at `drift` from `at`: `at` plus up to
/// ±DRIFT, wrapped. DRIFT off rests at 0 and draws nothing.
pub fn step(at: Place, drift: Drift, rng: &mut DriftRng) -> Place {
    if drift == Drift::OFF {
        return Place::ZERO;
    }
    Place::wrapped(at.get() + drift.get() * rng.draw())
}

/// POSITION moved by the place, wrapped; POSITION alone (its 1 kept)
/// while the place rests, so DRIFT off changes nothing.
pub fn place(position: f32, at: Place) -> f32 {
    if at == Place::ZERO {
        position
    } else {
        Place::wrapped(position + at.get()).get()
    }
}

/// The frame a once-through note starts at, `place` of `w` counted the way
/// it plays, leaving at least `floor` frames (or the window) to play.
pub fn from(w: Window, place: f32, floor: u32) -> u32 {
    let n = w.frames();
    let room = n.saturating_sub(floor.min(n));
    w.lo + ((clampf(place, 0.0, 1.0) * n as f32) as u32).min(room)
}
