//! The STM32H750's limits as constants, shared by the desktop and firmware
//! builds (ADR 0013): a design that cannot run on the chip fails on both.

use core::iter::Sum;
use core::ops::Add;

pub use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

pub const MAX_VOICES: usize = 8;
pub const MAX_PARTS: usize = 6;
pub const DAC_PAIRS: usize = 3;

pub const CPU_HZ_REV_V: u32 = 480_000_000;
pub const CPU_HZ_REV_Y: u32 = 400_000_000;

/// 30% is left for UI, MIDI and interrupt overhead.
pub const AUDIO_BUDGET_PERCENT: u32 = 70;

// No `Default`: a rev Y chip must never silently get the 480 MHz budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleBudget(u32);

impl SampleBudget {
    pub const fn for_cpu(cpu_hz: u32) -> Self {
        Self((cpu_hz as u64 * AUDIO_BUDGET_PERCENT as u64 / (100 * SAMPLE_RATE as u64)) as u32)
    }

    pub const fn as_cost(self) -> Cost {
        Cost(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockBudget(u32);

impl BlockBudget {
    pub const fn for_cpu(cpu_hz: u32) -> Self {
        Self((cpu_hz as u64 * BLOCK_SIZE as u64 / SAMPLE_RATE as u64) as u32)
    }

    pub const fn block_cycles(self) -> u32 {
        self.0
    }

    pub const fn budget_cycles(self) -> u32 {
        (self.0 as u64 * AUDIO_BUDGET_PERCENT as u64 / 100) as u32
    }
}

/// Memory regions, in bytes (STM32H750 map, RM0433 §2.3).
pub const AXI_SRAM: usize = 512 * 1024; // D1: framebuffer, UI, Performance, FX bus
pub const D2_SRAM: usize = 288 * 1024; // SRAM1+2+3 at 0x3000_0000: voices, DMA buffers
pub const DTCM: usize = 128 * 1024; // tables, audio stack

/// D2 kept free for the audio DMA rings (3 SAI × 2 halves × 64 frames × 2 ch
/// × 4 B = 3 KB, `chimera-stm32/src/audio/dma.rs`) and MIDI buffers.
pub const D2_DMA_RESERVE: usize = 8 * 1024;
/// `[Voice; MAX_VOICES]` lives in D2 beside the DMA buffers.
pub const VOICE_RAM_BUDGET: usize = D2_SRAM - D2_DMA_RESERVE; // 286_720
/// A `Voice` less its `EngineSlot`: the chain, modulators and bookkeeping
/// (ADR 0051).
pub const VOICE_CHAIN_BYTES: usize = 2048;

/// Framebuffer: 240 × 320 RGB565, one static in AXI (`chimera-stm32/src/display.rs`).
pub const FB_BYTES: usize = chimera_hal::FB_SIZE * 2; // 153_600
/// AXI kept for the UI besides the Performance and SoundPool: renderer and
/// navigation state. The stack is in DTCM (ADR 0025), not here.
pub const UI_RESERVE: usize = 64 * 1024;
/// AXI for the card's `SdStore` (`chimera-stm32/src/sd.rs`, which asserts
/// it fits): a 512 B block buffer, the 512 B FAT sector cache and its tag,
/// the allocator hint and the SD driver's state. No heap, no handle table.
pub const STORE_RESERVE: usize = 2 * 1024;

/// AXI share for the FX bus (ADR 0014). `instrument.rs` asserts the sum of
/// everything placed in AXI.
pub const FX_BUS_BUDGET: usize = 256 * 1024; // 262_144

/// CPU cycles per sample (per voice for engines), measured on the chip with
/// the DWT cycle counter; each figure carries its bench date (ADR 0013).
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
