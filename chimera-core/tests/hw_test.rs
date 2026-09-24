//! Target limits (ADR 0013): the constants both builds enforce.

use chimera_core::hw::{self, Cost};

#[test]
fn audio_timing_matches_the_chip() {
    assert_eq!(hw::SAMPLE_RATE, chimera_hal::SAMPLE_RATE);
    assert_eq!(hw::BLOCK_SIZE, chimera_hal::BLOCK_SIZE);
    assert_eq!(hw::CYCLES_PER_SAMPLE, 10_000);
    assert_eq!(hw::AUDIO_CYCLE_BUDGET, Cost(7_000));
}

#[test]
fn capacity_constants() {
    assert_eq!((hw::MAX_VOICES, hw::MAX_PARTS, hw::DAC_PAIRS), (6, 6, 3));
}

/// STM32H750 memory map (RM0433 §2.3): AXI 512 KB, D2 SRAM1+2+3 = 288 KB, DTCM 128 KB.
#[test]
fn memory_regions_match_the_h750() {
    assert_eq!(hw::AXI_SRAM, 524_288);
    assert_eq!(hw::D2_SRAM, 294_912);
    assert_eq!(hw::DTCM, 131_072);
}

#[test]
fn costs_add_and_compare() {
    assert_eq!(Cost(610) + Cost(1_210), Cost(1_820));
    assert_eq!([Cost(1), Cost(2), Cost(3)].into_iter().sum::<Cost>(), Cost(6));
    assert!(Cost(7_001) > hw::AUDIO_CYCLE_BUDGET);
    assert_eq!(Cost::ZERO, Cost(0));
}
