// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI quantities as checked newtypes: an out-of-range value cannot be built.

/// 7-bit data byte, 0..=127.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U7(u8);

impl U7 {
    pub const ZERO: U7 = U7(0);
    pub const MAX: U7 = U7(127);

    pub const fn new(v: u8) -> Option<Self> {
        if v <= 127 { Some(Self(v)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// 14-bit value, 0..=16383, sent as MSB then LSB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U14(u16);

impl U14 {
    pub const fn new(v: u16) -> Option<Self> {
        if v <= 16383 { Some(Self(v)) } else { None }
    }

    pub const fn from_parts(msb: U7, lsb: U7) -> Self {
        Self(((msb.0 as u16) << 7) | lsb.0 as u16)
    }

    pub const fn msb(self) -> U7 {
        U7((self.0 >> 7) as u8)
    }

    pub const fn lsb(self) -> U7 {
        U7((self.0 & 127) as u8)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// MIDI channel, 0..=15 (the low nibble of the status byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Channel(u8);

impl Channel {
    pub const fn new(c: u8) -> Option<Self> {
        if c <= 15 { Some(Self(c)) } else { None }
    }

    /// `c` limited to 15: for values already clamped by a param spec.
    pub const fn clamped(c: u8) -> Self {
        Self(if c > 15 { 15 } else { c })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// MIDI note number, 0..=127. Built at the MIDI trust boundary (the parser,
/// the desktop keyboard), so the audio path only ever sees valid notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(u8);

impl Key {
    /// A4 (440 Hz).
    pub const A4: Key = Key(69);

    pub const fn new(n: u8) -> Option<Self> {
        if n <= 127 { Some(Self(n)) } else { None }
    }

    /// `n` limited to 127: for a note already taken from a `Key`.
    pub const fn clamped(n: u8) -> Self {
        Self(if n > 127 { 127 } else { n })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Note-on velocity, 1..=127. Zero means note-off and is not representable.
// The 1..=127 invariant is held by the constructor, not NonZeroU8: that niche costs ~100 B of bank 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(u8);

impl Velocity {
    pub const MAX: Velocity = Velocity(127);
    /// Velocity of the desktop keyboard and the firmware test note.
    pub const DEFAULT: Velocity = Velocity(100);

    pub const fn new(v: u8) -> Option<Self> {
        if v >= 1 && v <= 127 {
            Some(Self(v))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Velocity as 0..1, exactly `v as f32 / 127.0` (what the engines used).
    pub fn unit(self) -> f32 {
        self.0 as f32 / 127.0
    }
}

/// Pitch bend, -8192..=8191 around the centre 0x2000.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bend(i16);

impl Bend {
    pub const fn from_u14(v: U14) -> Self {
        Self(v.0 as i16 - 8192)
    }

    pub const fn get(self) -> i16 {
        self.0
    }

    /// -1..=1, both ends reached exactly.
    pub fn unit(self) -> f32 {
        if self.0 < 0 {
            self.0 as f32 / 8192.0
        } else {
            self.0 as f32 / 8191.0
        }
    }
}
