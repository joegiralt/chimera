// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The STM32H750's limits as constants, shared by the desktop and firmware
//! builds (ADR 0013): a design that cannot run on the chip fails on both.

use core::iter::Sum;
use core::ops::Add;

pub use chimera_hal::units::Cycles;
pub use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

pub const MAX_VOICES: usize = 8;
pub const MAX_PARTS: usize = 6;
pub const DAC_PAIRS: usize = 3;
/// Voices the heaviest Sound must still allow (ADR 0026).
pub const FLOOR_VOICES: u32 = 4;
/// Voices a Sound with a formant operator must still allow (owner,
/// 2026-10-12).
pub const FLOOR_VOICES_FORMANT: u32 = 3;

pub const CPU_HZ_REV_V: u32 = 480_000_000;
pub const CPU_HZ_REV_Y: u32 = 400_000_000;

/// 25% is left for UI, MIDI and interrupt overhead (ADR 0124).
pub const AUDIO_BUDGET_PERCENT: u32 = 75;

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

    /// The render span past which a block sheds a voice (ADR 0273):
    /// `LOAD_SHED_PERCENT` of its deadline, rounded down, so a span over
    /// it is over the share.
    pub const fn shed_at(self) -> Cycles {
        Cycles::new((self.0 as u64 * LOAD_SHED_PERCENT / 100) as u32)
    }
}

/// The share of a block's deadline, in percent, past which a rendered
/// block sheds a voice (ADR 0273): above ADR 0262's worst billed block,
/// short of the overrun. Six Parts read about 89 % on rev V and about
/// 92 % on rev Y (the same overhead cycles over a smaller deadline):
/// about 6 points of margin on rev V, 3 on rev Y. Read through
/// `BlockBudget::shed_at`.
const LOAD_SHED_PERCENT: u64 = 95;

/// Memory regions, in bytes (STM32H750 map, RM0433 §2.3).
pub const AXI_SRAM: usize = 512 * 1024; // D1: framebuffer, UI, Performance, FX bus
pub const D2_SRAM: usize = 288 * 1024; // SRAM1+2+3 at 0x3000_0000: voices, DMA buffers
pub const DTCM: usize = 128 * 1024; // tables, audio stack
include!("itcm_geometry.rs");
const _: () = assert!(ITCM_ORIGIN + ITCM_STATE == 64 * 1024 && ITCM_MARGIN < ITCM_STATE);

/// D2 kept free, as one non-cacheable MPU region (16 KB), for the audio DMA
/// rings (3 SAI × 2 halves × 64 frames × 2 ch × 4 B = 3 KB,
/// `chimera-stm32/src/audio/dma.rs`), MIDI buffers and the card's staging.
pub const D2_DMA_RESERVE: usize = 16 * 1024;
/// D3 SRAM4 at 0x3800_0000: the stream rings, but for their third slots.
pub const SRAM4: usize = 64 * 1024;
/// The UI's knob publisher, kept in D2 beside the voices on the board, not
/// in AXI, which the bench builds hold within 8 KiB of full (ADR 0115).
pub const D2_KNOBS: usize = core::mem::size_of::<crate::dsp::knobs::KnobPublisher>();
/// The voices' formant views to the OP pages, a triple buffer in D2 beside
/// the knobs, for the same reason.
pub const D2_FORMANT: usize =
    core::mem::size_of::<crate::triple::TripleBuffer<crate::dsp::algo::engine::FormantViews>>();
/// The stream rings' third slots, which SRAM4 has no room for: in D2.
pub const D2_THIRD_SLOTS: usize = core::mem::size_of::<crate::stream::ring::ThirdSlots>();
/// `[Voice; MAX_VOICES]` lives in D2 beside the DMA buffers, the knob
/// publisher, the formant views and the third slots.
pub const VOICE_RAM_BUDGET: usize =
    D2_SRAM - D2_DMA_RESERVE - D2_KNOBS - D2_FORMANT - D2_THIRD_SLOTS;
/// A `Voice` less its `EngineSlot`: the chain, modulators and bookkeeping
/// (ADR 0051, 2,048 B; ADR 0101 raises it to 2,112 for the 2040's state;
/// 2,176 for the formant operators' 13 glided settings in `AlgoLive`, which
/// ADR 0131 records; 2,240 for the played Sound's FX block, ADR 0155;
/// 2,336 for FUZZ's memory, one a place, 2 × 44 B, ADR 0156; 2,344 for
/// the played Sound's TAPE block, WALK's DIRECTION, TRON and DRIFT beside
/// SCRUB's KEY, ADR 0203; 2,360 with STRETCH LENGTH, GRAIN, SPRAY and END,
/// ADR 0204; 2,440 for the VCA's low-pass gate, 68 B and its padding, ADR
/// 0195, measured on the merged tree; 2,448 for the WHEEL and PRESSURE
/// sources' values and the voice's own pressure, #514; 2,464 for TIMBRE's
/// value and the voice's own MPE bend and timbre, ADR 0252; 2,472 for the
/// played Sound's WIDTH, 4 B and its padding, ADR 0294; 2,480 for the
/// last block's OUT PAN and BASE, kept apart from what a retrigger
/// overwrites, ADR 0294).
pub const VOICE_CHAIN_BYTES: usize = 2480;

/// Framebuffer: 240 × 320 RGB565, one static in AXI (`chimera-board/src/display.rs`).
pub const FB_BYTES: usize = chimera_hal::FB_SIZE * 2; // 153_600
/// AXI for the card's `SdStore` (`chimera-board/src/sd.rs`, which asserts
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

    /// The costlier of the two (`Ord::max`, in a const).
    pub const fn max(self, other: Cost) -> Cost {
        if other.0 > self.0 { other } else { self }
    }
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
