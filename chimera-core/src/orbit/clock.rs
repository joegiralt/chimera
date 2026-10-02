//! ORBIT's clock types.

use super::euclid::Steps;
use core::num::NonZeroU32;

/// A position within a bar, in 2³² ths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Phase(u32);

impl Phase {
    pub const ZERO: Phase = Phase(0);

    pub const fn new(p: u32) -> Phase {
        Phase(p)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A tempo in whole BPM, 30..=300.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bpm(u16);

impl Bpm {
    pub const MIN: u16 = 30;
    pub const MAX: u16 = 300;
    pub const DEFAULT: Bpm = Bpm(120);

    pub const fn new(b: u16) -> Option<Bpm> {
        if b >= Self::MIN && b <= Self::MAX {
            Some(Bpm(b))
        } else {
            None
        }
    }

    pub const fn clamped(b: u16) -> Bpm {
        Bpm(if b < Self::MIN {
            Self::MIN
        } else if b > Self::MAX {
            Self::MAX
        } else {
            b
        })
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A point on the song's timeline: whole bars, then a phase within the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub bar: u32,
    pub phase: Phase,
}

impl Position {
    pub const ZERO: Position = Position { bar: 0, phase: Phase::ZERO };
}

/// Half a block (§ 6.2).
pub const WINDOW: u64 = 32;
/// The fastest external clock the advance will follow.
pub const MAX_BPM_EXT: u32 = 1200;

// A window at the cap spans less than one step of the longest ring (48 kHz).
const _: () = assert!(
    (2 * WINDOW) * ((MAX_BPM_EXT as u64) << 32) / (48_000 * 240)
        < step_start(1, Steps::MAX).get() as u64
);

const BAR: u64 = 1 << 32;

/// First phase of step `k` of `steps`: ⌈k · 2³² / n⌉.
pub const fn step_start(k: u8, steps: Steps) -> Phase {
    let n = steps.get() as u64;
    Phase((k as u64 * BAR).div_ceil(n) as u32)
}

/// The step holding `phase`: ⌊phase · n / 2³²⌋.
pub const fn step_at(phase: Phase, steps: Steps) -> u8 {
    ((phase.0 as u64 * steps.get() as u64) >> 32) as u8
}

/// Samples to bars, exact rational: a bar is `sample_rate · 240 / bpm`
/// samples. Whole bars and a remainder, all in `u64`.
#[derive(Clone, Copy, Debug)]
pub struct InternalClock {
    at: u64,
    pos: Position,
    bpm: Bpm,
    sample_rate: NonZeroU32,
}

impl InternalClock {
    pub const fn start(at_sample: u64, bpm: Bpm, sample_rate: NonZeroU32) -> Self {
        InternalClock { at: at_sample, pos: Position::ZERO, bpm, sample_rate }
    }

    pub fn anchor(&self) -> u64 {
        self.at
    }

    pub fn bpm(&self) -> Bpm {
        self.bpm
    }

    fn per_bar(&self) -> u64 {
        self.sample_rate.get() as u64 * 240
    }

    pub fn bar_len(&self) -> u32 {
        let b = self.bpm.0 as u64;
        ((self.per_bar() + b / 2) / b) as u32
    }

    /// Re-anchor: `pos` falls at sample `at`, at the new tempo.
    pub fn set_bpm(&mut self, at: u64, pos: Position, bpm: Bpm) {
        self.at = at;
        self.pos = pos;
        self.bpm = bpm;
    }

    /// Where the song is at `sample`. A bar line lands on the sample
    /// `sample_of` gives it, phase 0 exactly.
    pub fn position(&self, sample: u64) -> Position {
        let n = self.per_bar();
        let e = sample.saturating_sub(self.at) * self.bpm.0 as u64;
        let phase = self.pos.phase.0 as u64 + (e % n * BAR) / n;
        let mut bar = (self.pos.bar as u64 + e / n + (phase >> 32)).min(u32::MAX as u64) as u32;
        let mut phase = phase as u32;
        let line = |bar: u32| Position { bar, phase: Phase::ZERO };
        if bar < u32::MAX && self.sample_of(line(bar + 1)) <= sample {
            bar += 1;
            phase = 0;
        } else if bar > self.pos.bar && self.sample_of(line(bar)) == sample {
            phase = 0;
        }
        Position { bar, phase: Phase(phase) }
    }

    /// The sample `pos` falls on, rounded to nearest; saturates at the anchor.
    pub fn sample_of(&self, pos: Position) -> u64 {
        if pos <= self.pos {
            return self.at;
        }
        let mut db = (pos.bar - self.pos.bar) as u64;
        let dp = if pos.phase >= self.pos.phase {
            (pos.phase.0 - self.pos.phase.0) as u64
        } else {
            db -= 1;
            BAR - (self.pos.phase.0 - pos.phase.0) as u64
        };
        let (n, b) = (self.per_bar(), self.bpm.0 as u64);
        let (q, r) = (db * n / b, db * n % b);
        let (num, den) = (r * BAR + dp * n, b * BAR);
        self.at + q + (num + den / 2) / den
    }
}
