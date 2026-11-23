// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The PreenFM3 main board, from its strap (ADR 0140): pure, no register
//! access. The shell reads PD0..PD3 at boot and wires the HC165 (the
//! encoders and buttons) where `BoardId::hc165` says.

/// A GPIO port the HC165 can sit on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    F,
}

/// Where the HC165 chain is wired: one port, a pin number each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hc165Pins {
    pub port: Port,
    pub data: u8,
    pub load: u8,
    pub clk: u8,
}

/// LQFP176 and LQFP144: PF2 DATA, PF1 LOAD, PF0 CLK.
pub const HC165_PF: Hc165Pins = Hc165Pins {
    port: Port::F,
    data: 2,
    load: 1,
    clk: 0,
};
/// LQFP100 (the "1.6 - 100" PCB), which has no port F: PA0 DATA, PA1
/// LOAD, PA2 CLK, the other way round from port F.
pub const HC165_PA: Hc165Pins = Hc165Pins {
    port: Port::A,
    data: 0,
    load: 1,
    clk: 2,
};

/// The main board by its MCU package, as the 4-bit strap names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardId {
    Lqfp176,
    Lqfp144,
    Lqfp100,
    /// A strap no known board has; wired as `Lqfp176`.
    Unknown(u8),
}

impl BoardId {
    pub const STRAP_LQFP176: u8 = 0b0001;
    pub const STRAP_LQFP144: u8 = 0b0010;
    pub const STRAP_LQFP100: u8 = 0b0011;

    pub const fn from_straps(straps: u8) -> Self {
        match straps {
            Self::STRAP_LQFP176 => BoardId::Lqfp176,
            Self::STRAP_LQFP144 => BoardId::Lqfp144,
            Self::STRAP_LQFP100 => BoardId::Lqfp100,
            other => BoardId::Unknown(other),
        }
    }

    /// The strap from PD0..PD3's levels, high = 1: PD0 is the top bit.
    pub const fn from_pins(pd: [bool; 4]) -> Self {
        Self::from_straps(
            (pd[0] as u8) << 3 | (pd[1] as u8) << 2 | (pd[2] as u8) << 1 | pd[3] as u8,
        )
    }

    /// The strap it was read from.
    pub const fn straps(self) -> u8 {
        match self {
            BoardId::Lqfp176 => Self::STRAP_LQFP176,
            BoardId::Lqfp144 => Self::STRAP_LQFP144,
            BoardId::Lqfp100 => Self::STRAP_LQFP100,
            BoardId::Unknown(s) => s,
        }
    }

    /// Unknown straps fall back to the LQFP176 wiring, the commonest.
    pub const fn hc165(self) -> Hc165Pins {
        match self {
            BoardId::Lqfp100 => HC165_PA,
            BoardId::Lqfp176 | BoardId::Lqfp144 | BoardId::Unknown(_) => HC165_PF,
        }
    }

    /// The package's pin count, `?` after it when the strap is unknown and
    /// the 176's wiring is a guess.
    pub const fn label(self) -> &'static str {
        match self {
            BoardId::Lqfp176 => "176",
            BoardId::Lqfp144 => "144",
            BoardId::Lqfp100 => "100",
            BoardId::Unknown(_) => "176?",
        }
    }

    /// The console's word.
    pub const fn word(self) -> &'static str {
        match self {
            BoardId::Lqfp176 => "lqfp176",
            BoardId::Lqfp144 => "lqfp144",
            BoardId::Lqfp100 => "lqfp100",
            BoardId::Unknown(_) => "unknown",
        }
    }
}
