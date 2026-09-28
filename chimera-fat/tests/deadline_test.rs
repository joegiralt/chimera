use chimera_fat::deadline::{Deadline, Over};

const IDLE: u64 = 1_000;
const CAP: u64 = 5_000;

#[test]
fn wraps_across_u32_max() {
    let mut d = Deadline::arm(u32::MAX - 100, IDLE, CAP);
    // 101 + 800 cycles across the wrap: under the idle limit.
    assert_eq!(d.tick(800, false), Ok(()));
    // 1 000 in all since `arm`: at the limit.
    assert_eq!(d.tick(899, false), Err(Over));
}

#[test]
fn idle_limit_trips() {
    let mut d = Deadline::arm(0, IDLE, CAP);
    assert_eq!(d.tick(999, false), Ok(()));
    assert_eq!(d.tick(1_000, false), Err(Over));
    assert!(d.is_over());
}

#[test]
fn over_is_sticky() {
    let mut d = Deadline::arm(0, IDLE, CAP);
    assert_eq!(d.tick(2_000, false), Err(Over));
    assert_eq!(d.tick(2_001, true), Err(Over));
}

#[test]
fn a_block_restarts_idle() {
    let mut d = Deadline::arm(0, IDLE, CAP);
    for t in [900, 1_800, 2_700, 3_600] {
        assert_eq!(d.tick(t, true), Ok(()));
    }
    assert_eq!(d.tick(4_500, false), Ok(()));
    assert_eq!(d.max_gap(), 900);
}

#[test]
fn cap_limit_trips_and_blocks_do_not_reset_it() {
    let mut d = Deadline::arm(0, IDLE, CAP);
    for t in (500..5_000).step_by(500) {
        assert_eq!(d.tick(t, true), Ok(()));
    }
    assert_eq!(d.tick(5_000, true), Err(Over));
}

#[test]
fn cap_trips_past_u32_cycles() {
    let cap = (1 << 32) + 10;
    let mut d = Deadline::arm(0, u64::MAX, cap);
    for i in 1..=4u32 {
        assert_eq!(d.tick(i << 30, true), Ok(()));
    }
    // 2^32 cycles in all, then 10 more.
    assert_eq!(d.tick(9, true), Ok(()));
    assert_eq!(d.tick(10, true), Err(Over));
}

#[test]
fn transfers_count_ticks_not_time() {
    let mut d = Deadline::arm_transfers(3, 10);
    assert_eq!(d.tick(u32::MAX, false), Ok(()));
    assert_eq!(d.tick(0, false), Ok(()));
    assert_eq!(d.tick(0, false), Err(Over));
}

#[test]
fn transfers_restart_on_a_block_and_cap() {
    let mut d = Deadline::arm_transfers(3, 10);
    for _ in 0..9 {
        assert_eq!(d.tick(0, true), Ok(()));
    }
    assert_eq!(d.tick(0, true), Err(Over));
    assert_eq!(d.max_gap(), 0);
}

#[test]
fn a_block_ending_a_long_gap_does_not_trip() {
    // The gap is caught by the `tick(false)` before the transfer, not by
    // the block that ends it.
    let mut d = Deadline::arm(0, IDLE, CAP);
    assert_eq!(d.tick(1_500, true), Ok(()));
    assert_eq!(d.max_gap(), 1_500);
}
