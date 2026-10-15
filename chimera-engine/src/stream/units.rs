//! Stream units: audio frames, the audio block clock, source rates and
//! speed, and the small indices a stream's slots and rings use.

use core::num::NonZeroU32;
use core::ops::Add;

use chimera_hal::units::{Bytes, Millis, StreamCount};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use crate::hw::MAX_VOICES;

// One ring a voice.
const _: () = assert!(StreamCount::MAX.get() as usize == MAX_VOICES);

/// How frames sit in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packing {
    /// 12-bit, 3 B per pair of frames.
    Twelve,
}

impl Packing {
    /// Whole pairs only.
    pub const fn frames_in(self, b: Bytes) -> Frames {
        match self {
            Self::Twelve => Frames(b.get() / 3 * 2),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frames(u32);

impl Frames {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn checked_sub(self, o: Frames) -> Option<Frames> {
        self.0.checked_sub(o.0).map(Frames)
    }

    /// An odd count takes a whole pair, as `pack12` pads.
    pub const fn to_bytes(self, p: Packing) -> Bytes {
        match p {
            Packing::Twelve => Bytes::new(self.0.div_ceil(2).saturating_mul(3)),
        }
    }
}

impl Add for Frames {
    type Output = Frames;

    fn add(self, o: Frames) -> Frames {
        Frames(self.0 + o.0)
    }
}

/// The audio block counter: an instant that wraps, compared with `until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioBlocks(u32);

impl AudioBlocks {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn after(self, blocks: u32) -> Self {
        Self(self.0.wrapping_add(blocks))
    }

    /// Blocks to `deadline`; negative is overdue.
    pub const fn until(self, deadline: AudioBlocks) -> i32 {
        deadline.0.wrapping_sub(self.0) as i32
    }

    /// Whole blocks in `t`, rounded down.
    pub const fn blocks_in(t: Millis) -> u32 {
        (t.get() as u64 * SAMPLE_RATE as u64 / (1000 * BLOCK_SIZE as u64)) as u32
    }
}

/// A source file's sample rate, in the range `Descriptor::from_bytes` accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRate(u16);

impl SourceRate {
    pub const fn new(hz: u16) -> Option<Self> {
        if hz >= 4_000 && hz <= 48_000 { Some(Self(hz)) } else { None }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Playback speed in thousandths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio(u32);

impl Ratio {
    pub const ONE: Ratio = Ratio(1_000);

    pub const fn from_milli(m: u32) -> Option<Self> {
        if m >= 1 && m <= 32_000 { Some(Self(m)) } else { None }
    }

    /// Source frames one audio block consumes, times 1000; never 0.
    pub const fn frames_per_block_x1000(self, rate: SourceRate) -> NonZeroU32 {
        let v = BLOCK_SIZE as u64 * rate.0 as u64 * self.0 as u64 / SAMPLE_RATE as u64;
        match NonZeroU32::new(v as u32) {
            Some(n) => n,
            None => NonZeroU32::MIN,
        }
    }
}

/// A slot's fill generation, mod 64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(u8);

impl Generation {
    pub const ZERO: Generation = Generation(0);
    pub const BITS: u32 = 6;
    const MASK: u32 = (1 << Self::BITS) - 1;

    pub const fn next(self) -> Self {
        Self(((self.0 as u32 + 1) & Self::MASK) as u8)
    }

    /// Masks to `BITS`.
    pub const fn from_bits(b: u32) -> Self {
        Self((b & Self::MASK) as u8)
    }

    pub const fn bits(self) -> u32 {
        self.0 as u32
    }
}

/// One of a stream's two slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotIdx {
    A,
    B,
}

impl SlotIdx {
    pub const ALL: [SlotIdx; 2] = [SlotIdx::A, SlotIdx::B];

    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A voice's index, below `MAX_VOICES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceIdx(usize);

impl VoiceIdx {
    pub const fn new(i: usize) -> Option<Self> {
        if i < MAX_VOICES { Some(Self(i)) } else { None }
    }

    pub const fn get(self) -> usize {
        self.0
    }

    pub fn all() -> impl Iterator<Item = VoiceIdx> {
        (0..MAX_VOICES).map(VoiceIdx)
    }
}
