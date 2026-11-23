// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! STRETCH's quantities (ADR 0204), each its own type, held in its range
//! as it is made: a value the DSP reads can't be out of it, nor NaN.

use crate::dsp::algo::math::clampf;
use crate::dsp::modulator::law::{GRAIN_SIZE, STRETCH_LENGTH};

/// `x` in [0, 1]; NaN reads 0.
#[inline(always)]
fn unit(x: f32) -> f32 {
    clampf(x, 0.0, 1.0)
}

/// Seconds of output, never negative.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Seconds(f32);

impl Seconds {
    pub fn get(self) -> f32 {
        self.0
    }

    /// As output samples at `sr`.
    #[inline(always)]
    pub fn samples(self, sr: u32) -> Samples {
        Samples::new(self.0 * sr as f32)
    }
}

/// Output samples: a length or a count, never negative, never NaN.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Samples(f32);

impl Samples {
    pub const ZERO: Samples = Samples(0.0);

    #[inline(always)]
    pub fn new(x: f32) -> Samples {
        // `max` takes the non-NaN side: NaN reads 0.
        Samples(x.max(0.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// LENGTH: how long the source lasts at SPEED 1×, 0.25 to 8 s
/// (`0.25 · 32^knob`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Length(Seconds);

impl Length {
    pub const MIN_S: f32 = 0.25;
    pub const MAX_S: f32 = 8.0;

    #[inline(always)]
    pub fn of_knob(x: f32) -> Length {
        Length(Seconds(STRETCH_LENGTH.at(unit(x))))
    }

    pub fn seconds(self) -> Seconds {
        self.0
    }
}

/// GRAIN: one grain's length, 30 to 250 ms (`0.03 · (250/30)^knob`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrainLen(Seconds);

impl GrainLen {
    pub const MIN_S: f32 = 0.03;
    pub const MAX_S: f32 = 0.25;

    #[inline(always)]
    pub fn of_knob(x: f32) -> GrainLen {
        GrainLen(Seconds(GRAIN_SIZE.at(unit(x))))
    }

    pub fn seconds(self) -> Seconds {
        self.0
    }
}

/// SPRAY: its share of its reach, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spray(f32);

impl Spray {
    /// The furthest a start moves back, seconds of source.
    pub const REACH_S: f32 = 0.025;
    /// The most an interval is drawn past its quarter GRAIN, a share of it.
    pub const JITTER: f32 = 0.25;

    #[inline(always)]
    pub fn of_knob(x: f32) -> Spray {
        Spray(unit(x))
    }

    pub fn share(self) -> f32 {
        self.0
    }

    /// The furthest back a start moves, seconds of source.
    pub fn reach(self) -> f32 {
        self.0 * Self::REACH_S
    }

    /// The most an interval is drawn past its quarter GRAIN.
    pub fn jitter(self) -> f32 {
        self.0 * Self::JITTER
    }
}

/// END: what STRETCH does at the window's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StretchEnd {
    /// Park on the last GRAIN of source while the key is held, 8 s at most.
    Hold,
    /// Start no more grains: the last fade out.
    Stop,
}

impl StretchEnd {
    /// END's stored index: 0 HOLD, 1 STOP (NaN, HOLD).
    #[inline(always)]
    pub fn of_knob(x: f32) -> StretchEnd {
        if x >= 0.5 {
            StretchEnd::Stop
        } else {
            StretchEnd::Hold
        }
    }
}

/// A place in the latched window, its frames from its start in the way it
/// plays: never negative.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct WinPos(f32);

impl WinPos {
    pub const START: WinPos = WinPos(0.0);

    #[inline(always)]
    pub fn new(x: f32) -> WinPos {
        WinPos(x.max(0.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// Window frames an output sample: a grain's (the key's pitch) or the
/// pointer's (signed: SPEED below 0 walks back). Never NaN.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Speed(f32);

impl Speed {
    /// One window frame a sample.
    pub const ONE: Speed = Speed(1.0);

    #[inline(always)]
    pub fn new(x: f32) -> Speed {
        Speed(if x.is_finite() { x } else { 0.0 })
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

/// A pitched source's period, window frames, at least 1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Period(f32);

impl Period {
    /// `None` unpitched: under a frame, or NaN.
    pub fn new(x: f32) -> Option<Period> {
        (x.is_finite() && x >= 1.0).then_some(Period(x))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}
