// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

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
    // Clamped after the saturating cast, so NaN plays silence.
    const TOP: i32 = DAC_FULL_SCALE as i32;
    // `round` is roundTiesToAway, libm's `roundf` bit for bit (all 2³² f32
    // checked, `to_dac_matches_libm`): one `vrinta`, not libm's software round.
    let steps = (core::f32::math::round(x * DAC_FULL_SCALE) as i32).clamp(-TOP, TOP);
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

/// What one trailing stream's flags mean this half: which counter moves.
/// A transfer error clears the stream's EN and freezes its NDTR, so drift is
/// not judged on a stream that faulted or still waits to be re-armed. FIFO
/// errors in the first `WARMUP_BLOCKS` are the start-up's own (the streams
/// are enabled before the SAI clocks), not counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrailVerdict {
    pub transfer_error: bool,
    pub fifo_error: bool,
    pub drift: bool,
}

pub const WARMUP_BLOCKS: u32 = 64;

pub const fn trail_verdict(
    te: bool,
    fe: bool,
    apart: bool,
    pending: bool,
    warm: bool,
) -> TrailVerdict {
    TrailVerdict {
        transfer_error: te,
        fifo_error: fe && warm,
        drift: apart && !te && !pending,
    }
}

/// Whether a halted trailing stream restarts now. Only at stream 0's wrap
/// (its TC): a circular stream reloads NDTR and M0AR from what was
/// programmed at enable, so it must be programmed with the whole ring from
/// its base, which is where stream 0 is at that instant. A mid-ring restart
/// would loop a partial ring forever.
pub const fn rearm_due(pending: bool, full_done: bool) -> bool {
    pending && full_done
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

/// Whether the watchdog's kicks are live: the audio block count and the
/// controls tick have each moved at least `LIVE_STEPS` times since `start`.
/// Arming before that leaves the start-up's own gaps on the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveCheck {
    blocks: u32,
    ticks: u32,
}

/// Two, so one move isn't a counter caught mid-start.
pub const LIVE_STEPS: u32 = 2;

impl LiveCheck {
    pub const fn start(blocks: u32, ticks: u32) -> Self {
        LiveCheck { blocks, ticks }
    }

    pub const fn live(&self, blocks: u32, ticks: u32) -> bool {
        blocks.wrapping_sub(self.blocks) >= LIVE_STEPS
            && ticks.wrapping_sub(self.ticks) >= LIVE_STEPS
    }
}

/// How long start-up waits for `LiveCheck` before it resets instead: many
/// blocks and ticks, so only dead audio or a dead tick reaches it.
pub const LIVE_WAIT_MS: u32 = 500;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// `to_dac` as it was, with libm's `roundf`.
    fn libm_dac(x: f32) -> DacSample {
        const TOP: i32 = DAC_FULL_SCALE as i32;
        DacSample(((libm::roundf(x * DAC_FULL_SCALE) as i32).clamp(-TOP, TOP)) << 8)
    }

    fn check(bits: u32) {
        let x = f32::from_bits(bits);
        let y = x * DAC_FULL_SCALE;
        assert_eq!(
            core::f32::math::round(y).to_bits(),
            libm::roundf(y).to_bits(),
            "{bits:#x}"
        );
        assert_eq!(to_dac(x), libm_dac(x), "{bits:#x}");
    }

    /// Every 61st f32, and every input whose scaled value lands within a few
    /// ulps of a half step anywhere the DAC can play.
    #[test]
    fn to_dac_matches_libm() {
        for bits in (0..=u32::MAX).step_by(61) {
            check(bits);
        }
        for n in -8_388_608i32..=8_388_608 {
            let h = (n as f32 + 0.5) / DAC_FULL_SCALE;
            for d in -3i32..=3 {
                check(h.to_bits().wrapping_add_signed(d));
            }
        }
    }

    /// All 2³² f32: `cargo test --release -p chimera-engine -- --ignored to_dac_matches_libm_everywhere`.
    #[test]
    #[ignore]
    fn to_dac_matches_libm_everywhere() {
        for bits in 0..=u32::MAX {
            check(bits);
        }
    }
}
