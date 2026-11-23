// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Card, time and rate units. Each is its own type, so a block number is
//! never a byte count and a duration is never an instant; a conversion is
//! one of these methods, never an `as` across units.

use core::num::{NonZeroU8, NonZeroU32};

const BLOCK_BYTES: u32 = 512;

fn sat(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// A 512 B device block number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lba(u32);

impl Lba {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn offset(self, blocks: u32) -> Option<Lba> {
        self.0.checked_add(blocks).map(Lba)
    }
}

/// A block index within one file, never a device block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileBlock(u32);

impl FileBlock {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A run of at least one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockCount(u32);

impl BlockCount {
    pub const ONE: BlockCount = BlockCount(1);

    pub const fn new(n: u32) -> Option<Self> {
        if n == 0 { None } else { Some(Self(n)) }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn bytes(self) -> Bytes {
        Bytes(self.0.saturating_mul(BLOCK_BYTES))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bytes(u32);

impl Bytes {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn checked_add(self, o: Bytes) -> Option<Bytes> {
        match self.0.checked_add(o.0) {
            Some(n) => Some(Bytes(n)),
            None => None,
        }
    }

    /// The block this offset falls in, and its offset there.
    pub const fn split_block(self) -> (FileBlock, Bytes) {
        (FileBlock(self.0 / BLOCK_BYTES), Bytes(self.0 % BLOCK_BYTES))
    }

    /// The rate that moves these bytes in `t`; 0 µs counts as 1.
    pub fn per(self, t: Micros) -> BytesPerSec {
        BytesPerSec(sat(u64::from(self.0) * 1_000_000 / u64::from(t.0.max(1))))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Millis(u32);

impl Millis {
    pub const ZERO: Millis = Millis(0);
    pub const MAX: Millis = Millis(u32::MAX);

    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn saturating_add(self, o: Millis) -> Millis {
        Millis(self.0.saturating_add(o.0))
    }

    /// Saturating at `u32::MAX` µs (about 71 minutes).
    pub const fn as_micros(self) -> Micros {
        Micros(self.0.saturating_mul(1_000))
    }
}

/// A duration that is never zero: a period, so a loop over it advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NonZeroMillis(NonZeroU32);

impl NonZeroMillis {
    pub const fn new(n: u32) -> Option<Self> {
        match NonZeroU32::new(n) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    pub const fn get(self) -> Millis {
        Millis(self.0.get())
    }
}

/// A clock rate in hertz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hz(u32);

impl Hz {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// Cycles in a ms, rounded down; `None` under 1 kHz.
    pub const fn per_ms(self) -> Option<NonZeroU32> {
        NonZeroU32::new(self.0 / 1_000)
    }
}

/// A count of CPU cycles, or a reading of the wrapping cycle counter (DWT).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cycles(u32);

impl Cycles {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// This many cycles in ms, rounded down.
    pub const fn as_millis(self, cycles_per_ms: NonZeroU32) -> Millis {
        Millis(self.0 / cycles_per_ms.get())
    }

    /// Cycles from `then` to this reading, wrapping.
    pub const fn since(self, then: Self) -> Self {
        Self(self.0.wrapping_sub(then.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Micros(u32);

impl Micros {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BytesPerSec(u32);

impl BytesPerSec {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// An SPI clock moves one byte per 8 clocks.
    pub const fn spi(hz: u32) -> Self {
        Self(hz / 8)
    }

    /// What this rate moves in `t`, saturating.
    pub fn bytes_in(self, t: Millis) -> Bytes {
        Bytes(sat(u64::from(self.0) * u64::from(t.0) / 1_000))
    }
}

/// A count of live streams, 1 to 8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamCount(NonZeroU8);

impl StreamCount {
    pub const MAX: StreamCount = StreamCount(NonZeroU8::new(8).unwrap());

    pub const fn new(n: u8) -> Option<Self> {
        match NonZeroU8::new(n) {
            Some(nz) if n <= Self::MAX.get() => Some(Self(nz)),
            _ => None,
        }
    }

    pub const fn get(self) -> u8 {
        self.0.get()
    }

    /// One atomic byte: 0 is none.
    pub const fn to_bits(c: Option<StreamCount>) -> u8 {
        match c {
            Some(c) => c.get(),
            None => 0,
        }
    }

    pub const fn from_bits(b: u8) -> Option<StreamCount> {
        Self::new(b)
    }
}
