// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_core::clock_plan::SiliconRev;
use chimera_core::hw::BlockBudget;
use chimera_core::note_queue::MAX_NOTE_SOURCES;
use chimera_core::perf::load::{AVG_BLOCKS, AudioStats, Peaks, load_percent};
use chimera_core::perf::stack::{STACK_PAINT, untouched_words};
use chimera_core::reset::ResetCause;

const V: BlockBudget = BlockBudget::for_cpu(480_000_000);

#[test]
fn load_is_a_percentage_of_the_block_deadline() {
    assert_eq!(load_percent(0, V), 0);
    assert_eq!(load_percent(320_000, V), 50);
    assert_eq!(load_percent(480_000, V), 75);
    assert_eq!(load_percent(640_000, V), 100);
}

#[test]
fn a_block_over_its_deadline_reads_over_100_and_saturates() {
    assert_eq!(load_percent(1_280_000, V), 200);
    assert_eq!(load_percent(u32::MAX, V), u16::MAX);
}

#[test]
fn average_is_the_mean_of_the_last_full_window() {
    let mut s = AudioStats::new(
        SiliconRev::V,
        chimera_core::board::BoardId::Lqfp176,
        480_000_000,
        ResetCause::PowerOn,
    );
    for _ in 0..AVG_BLOCKS - 1 {
        s.record(320_000, V, Peaks::Kept);
    }
    assert_eq!(s.load_avg, 0, "no full window yet");
    s.record(320_000, V, Peaks::Kept);
    assert_eq!(s.load_avg, 50);
    for i in 0..AVG_BLOCKS {
        s.record(if i % 2 == 0 { 64_000 } else { 192_000 }, V, Peaks::Kept);
    }
    assert_eq!(s.load_avg, 20);
}

#[test]
fn peak_holds_the_worst_block_since_boot() {
    let mut s = AudioStats::new(
        SiliconRev::Y,
        chimera_core::board::BoardId::Lqfp176,
        400_000_000,
        ResetCause::PowerOn,
    );
    let y = BlockBudget::for_cpu(400_000_000);
    s.record(100_000, y, Peaks::Kept);
    s.record(400_000, y, Peaks::Kept);
    s.record(50_000, y, Peaks::Kept);
    assert_eq!(s.load_peak, 75);
}

#[test]
fn new_stats_carry_the_chip_and_zero_counters() {
    let s = AudioStats::new(
        SiliconRev::V,
        chimera_core::board::BoardId::Lqfp176,
        480_000_000,
        ResetCause::PowerOn,
    );
    assert_eq!((s.rev, s.cpu_hz), (SiliconRev::V, 480_000_000));
    assert_eq!(
        (s.load_avg, s.load_peak, s.overruns, s.desyncs, s.stack_used),
        (0, 0, 0, 0, 0)
    );
    assert_eq!((s.drops, s.sources), ([0; MAX_NOTE_SOURCES], 0));
}

#[test]
fn untouched_words_counts_the_paint_from_the_bottom() {
    let p = STACK_PAINT;
    assert_eq!(untouched_words([p, p, p, 0, p]), 3);
    assert_eq!(untouched_words([p; 8]), 8);
    assert_eq!(untouched_words([1, p, p]), 0);
}

#[test]
fn the_voice_peak_holds_until_restarted() {
    let mut s = AudioStats::new(
        SiliconRev::V,
        chimera_core::board::BoardId::Lqfp176,
        480_000_000,
        ResetCause::PowerOn,
    );
    assert_eq!((s.voices, s.voices_peak, s.cost_pct), (0, 0, 0));
    s.record_voices(5, 60, Peaks::Kept);
    s.record_voices(2, 25, Peaks::Kept);
    assert_eq!((s.voices, s.voices_peak, s.cost_pct), (2, 5, 25));
    s.record_voices(3, 30, Peaks::Restart);
    assert_eq!(
        (s.voices, s.voices_peak),
        (3, 3),
        "the console read it: the peak starts over"
    );
    s.record_voices(1, 10, Peaks::Kept);
    assert_eq!(s.voices_peak, 3);
}

/// #995: the console's peak (`read_peak`, `peak_pct`) starts over at the
/// block after each `stats`; the since-boot peak (the AUDIO page's)
/// holds.
#[test]
fn the_read_peak_restarts_while_the_boot_peak_holds() {
    let mut s = AudioStats::new(
        SiliconRev::Y,
        chimera_core::board::BoardId::Lqfp176,
        400_000_000,
        ResetCause::PowerOn,
    );
    let y = BlockBudget::for_cpu(400_000_000);
    s.record(400_000, y, Peaks::Kept);
    s.record(100_000, y, Peaks::Kept);
    assert_eq!((s.load_peak, s.read_peak), (75, 75));
    s.record(50_000, y, Peaks::Restart);
    assert_eq!((s.load_peak, s.read_peak), (75, 9), "read: it starts over");
    s.record(100_000, y, Peaks::Kept);
    s.record(60_000, y, Peaks::Kept);
    assert_eq!((s.load_peak, s.read_peak), (75, 19));
}
