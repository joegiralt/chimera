//! Target limits (ADR 0013): the budgets derived from the chip's constants.

use chimera_core::hw::{self, BlockBudget, Cost, SampleBudget};

/// 70 % of each clock's cycles per 48 kHz sample.
#[test]
fn sample_budgets_follow_the_cpu_clock() {
    assert_eq!(
        SampleBudget::for_cpu(hw::CPU_HZ_REV_V).as_cost(),
        Cost(7_000)
    );
    assert_eq!(
        SampleBudget::for_cpu(hw::CPU_HZ_REV_Y).as_cost(),
        Cost(5_833)
    );
}

/// Cycles per 64-sample block, and their 70 % share.
#[test]
fn block_budgets_follow_the_cpu_clock() {
    let v = BlockBudget::for_cpu(hw::CPU_HZ_REV_V);
    assert_eq!((v.block_cycles(), v.budget_cycles()), (640_000, 448_000));
    let y = BlockBudget::for_cpu(hw::CPU_HZ_REV_Y);
    assert_eq!((y.block_cycles(), y.budget_cycles()), (533_333, 373_333));
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
