//! Converting `Instrument::render`'s float output into DMA-ready DAC words,
//! and the pure planning behind the SAI DMA circular-buffer interrupts
//! (instrument-on-chip spec).

use crate::hw::BLOCK_SIZE;
use crate::instrument::DacOut;
use crate::part::DacPair;

pub const DAC_FULL_SCALE: f32 = 8_388_607.0;

// The SAI data register is right-aligned: with 32-bit data the CS4344 reads
// bits 31..8. Only `to_dac` builds one, so the low byte is always zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct DacSample(i32);

impl DacSample {
    pub const ZERO: DacSample = DacSample(0);

    pub const fn get(self) -> i32 {
        self.0
    }

    /// The level the DAC plays, −1..1. The desktop sends it to its speakers,
    /// so it hears the same clamp and 24-bit steps as the hardware (ADR 0050).
    pub fn level(self) -> f32 {
        (self.0 >> 8) as f32 / DAC_FULL_SCALE
    }
}

/// The final conversion: clamped to full scale, a safety the limiter
/// before it keeps from ever engaging (ADR 0050).
pub fn to_dac(x: f32) -> DacSample {
    let steps = libm::roundf(x.clamp(-1.0, 1.0) * DAC_FULL_SCALE) as i32;
    DacSample(steps << 8)
}

pub fn interleave(dac: &DacOut, pair: DacPair, out: &mut [DacSample; BLOCK_SIZE * 2]) {
    for (o, &s) in out.iter_mut().zip(&dac[pair.index()]) {
        *o = to_dac(s);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    First,
    Second,
}

impl Half {
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HalfPlan {
    pub halves: [Option<Half>; 2],
    pub overrun: bool,
}

pub const fn plan_halves(half_done: bool, full_done: bool) -> HalfPlan {
    match (half_done, full_done) {
        (true, true) => HalfPlan {
            halves: [Some(Half::First), Some(Half::Second)],
            overrun: true,
        },
        (true, false) => HalfPlan {
            halves: [Some(Half::First), None],
            overrun: false,
        },
        (false, true) => HalfPlan {
            halves: [Some(Half::Second), None],
            overrun: false,
        },
        (false, false) => HalfPlan {
            halves: [None, None],
            overrun: false,
        },
    }
}

pub const fn desynced(a: u16, b: u16, ring: u16, tolerance: u16) -> bool {
    let d = (a as u32 + ring as u32 - b as u32) % ring as u32;
    let d = if d > ring as u32 - d {
        ring as u32 - d
    } else {
        d
    };
    d > tolerance as u32
}

/// The audio interrupt's liveness as the watchdog kicker sees it: alive
/// when its block count has moved since the last look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Heartbeat(u32);

impl Heartbeat {
    pub const fn new() -> Self {
        Heartbeat(0)
    }

    pub fn advanced(&mut self, blocks: u32) -> bool {
        let moved = blocks != self.0;
        self.0 = blocks;
        moved
    }
}

/// The watchdog timeout in ms: the longest the audio interrupt may starve
/// the controls tick, a hang or a sustained overrun alike, before the unit
/// resets (ADR 0034). The IWDG counts it on the LSI's nominal 32 kHz.
pub const WATCHDOG_TIMEOUT_MS: u32 = 100;
/// The LSI's nominal and fastest rates (STM32H750 datasheet): the fastest
/// gives the shortest real timeout.
pub const LSI_NOMINAL_HZ: u32 = 32_000;
pub const LSI_MAX_HZ: u32 = 33_600;

/// The longest healthy gap between kicks, in µs: one tick may find no new
/// block and slip, and the next can lag an audio block behind, since the
/// tick runs below the audio interrupt.
pub const fn kick_gap_us(controls_hz: u32, block: u32, sample_rate: u32) -> u32 {
    2 * 1_000_000_u32.div_ceil(controls_hz) + (block * 1_000_000).div_ceil(sample_rate)
}

/// The IWDG's counts for `timeout_ms` at its finest prescaler (/4): the
/// reload register holds 12 bits.
pub const fn iwdg_reload_at_div4(timeout_ms: u32) -> u32 {
    timeout_ms * (LSI_NOMINAL_HZ / 1000) / 4
}
