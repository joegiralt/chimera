// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Active Sensing: opt-in after the first FE. The shell times it.

use crate::stream::RealTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensing {
    Off,
    Armed,
}

impl Sensing {
    pub fn heard(&mut self, rt: RealTime) {
        if rt == RealTime::ActiveSensing {
            *self = Sensing::Armed;
        }
    }

    /// True once per silence: the receiver timeout fired while armed.
    pub fn timed_out(&mut self) -> bool {
        let was = *self == Sensing::Armed;
        *self = Sensing::Off;
        was
    }

    /// Inlined: bank 1's ISR reads it through `PipeSlot::sensing_armed`.
    #[inline(always)]
    pub fn armed(&self) -> bool {
        *self == Sensing::Armed
    }
}
