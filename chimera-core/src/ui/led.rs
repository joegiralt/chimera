// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The front-panel LED pulses ORBIT's beat: short on a beat, long on the
//! bar's downbeat. Pure; the shell copies `on()` to the pin (#307).

/// A bar is four beats.
pub const BEATS_PER_BAR: u32 = crate::orbit::runner::BEATS_PER_BAR;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pulse {
    Beat,
    Downbeat,
}

impl Pulse {
    pub const fn of(beat: u32) -> Pulse {
        if beat.is_multiple_of(BEATS_PER_BAR) {
            Pulse::Downbeat
        } else {
            Pulse::Beat
        }
    }

    const fn ticks(self) -> u8 {
        match self {
            Pulse::Beat => BEAT_TICKS,
            Pulse::Downbeat => DOWNBEAT_TICKS,
        }
    }
}

/// 50 ms at `UI_FPS` = 20.
pub const BEAT_TICKS: u8 = 1;
/// 150 ms, the header dot's fade.
pub const DOWNBEAT_TICKS: u8 = 3;

/// The `UiTick`s left lit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Led(u8);

impl Led {
    pub const OFF: Led = Led(0);

    /// A pulse lights for the longer of what is left and its length; else
    /// one tick less.
    pub const fn step(self, p: Option<Pulse>) -> Led {
        let left = self.0.saturating_sub(1);
        match p {
            Some(p) if p.ticks() > left => Led(p.ticks()),
            _ => Led(left),
        }
    }

    pub const fn on(self) -> bool {
        self.0 > 0
    }
}
