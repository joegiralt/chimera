//! The STM32H750's limits as constants, shared by the desktop and firmware
//! builds (ADR 0013): a design that cannot run on the chip fails on both.

use core::iter::Sum;
use core::ops::Add;

pub use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

pub const MAX_VOICES: usize = 6;
pub const MAX_PARTS: usize = 6;
pub const DAC_PAIRS: usize = 3;

pub const CPU_HZ: u32 = 480_000_000;
pub const CYCLES_PER_SAMPLE: u32 = CPU_HZ / SAMPLE_RATE; // 10_000
/// 30% is left for UI, MIDI and interrupt overhead.
pub const AUDIO_CYCLE_BUDGET: Cost = Cost(CYCLES_PER_SAMPLE * 70 / 100); // 7_000

/// Memory regions, in bytes (STM32H750 map, RM0433 §2.3).
pub const AXI_SRAM: usize = 512 * 1024; // D1: framebuffer, UI, Performance, FX bus
pub const D2_SRAM: usize = 288 * 1024; // SRAM1+2+3 at 0x3000_0000: voices, DMA buffers
pub const DTCM: usize = 128 * 1024; // tables, audio stack

/// D2 kept free for audio DMA (3 SAI × 2 halves × 64 frames × 2 ch × 4 B =
/// 3 KB; today one 512 B buffer) and MIDI buffers.
pub const D2_DMA_RESERVE: usize = 8 * 1024;
/// `[Voice; MAX_VOICES]` lives in D2 beside the DMA buffers.
pub const VOICE_RAM_BUDGET: usize = D2_SRAM - D2_DMA_RESERVE; // 286_720

/// CPU cycles per sample (per voice for engines). Values are estimates
/// until measured on hardware with the DWT cycle counter (ADR 0013).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cost(pub u32);

impl Cost {
    pub const ZERO: Cost = Cost(0);
}

impl Add for Cost {
    type Output = Cost;

    fn add(self, rhs: Cost) -> Cost {
        Cost(self.0 + rhs.0)
    }
}

impl Sum for Cost {
    fn sum<I: Iterator<Item = Cost>>(iter: I) -> Cost {
        iter.fold(Cost::ZERO, Add::add)
    }
}
