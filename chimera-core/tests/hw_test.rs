//! Target limits (ADR 0013): the constants both builds enforce.

use chimera_core::hw::{self, BlockBudget, Cost, SampleBudget};

#[test]
fn audio_timing_matches_the_chip() {
    assert_eq!(hw::SAMPLE_RATE, chimera_hal::SAMPLE_RATE);
    assert_eq!(hw::BLOCK_SIZE, chimera_hal::BLOCK_SIZE);
    assert_eq!(
        SampleBudget::for_cpu(hw::CPU_HZ_REV_V).as_cost(),
        Cost(7_000)
    );
}

#[test]
fn budgets_follow_the_cpu_clock() {
    assert_eq!(hw::CPU_HZ_REV_V, 480_000_000);
    assert_eq!(hw::CPU_HZ_REV_Y, 400_000_000);
    assert_eq!(hw::AUDIO_BUDGET_PERCENT, 70);
    assert_eq!(
        SampleBudget::for_cpu(hw::CPU_HZ_REV_Y).as_cost(),
        Cost(5_833)
    );
    let v = BlockBudget::for_cpu(hw::CPU_HZ_REV_V);
    assert_eq!((v.block_cycles(), v.budget_cycles()), (640_000, 448_000));
    let y = BlockBudget::for_cpu(hw::CPU_HZ_REV_Y);
    assert_eq!((y.block_cycles(), y.budget_cycles()), (533_333, 373_333));
}

#[test]
fn capacity_constants() {
    assert_eq!((hw::MAX_VOICES, hw::MAX_PARTS, hw::DAC_PAIRS), (6, 6, 3));
}

#[test]
fn memory_regions_match_the_h750() {
    assert_eq!(hw::AXI_SRAM, 524_288);
    assert_eq!(hw::D2_SRAM, 294_912);
    assert_eq!(hw::DTCM, 131_072);
}

#[test]
fn costs_add_and_compare() {
    assert_eq!(Cost(610) + Cost(1_210), Cost(1_820));
    assert_eq!(
        [Cost(1), Cost(2), Cost(3)].into_iter().sum::<Cost>(),
        Cost(6)
    );
    assert!(Cost(7_001) > SampleBudget::for_cpu(hw::CPU_HZ_REV_V).as_cost());
    assert_eq!(Cost::ZERO, Cost(0));
}
