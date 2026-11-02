use chimera_core::console::{
    ANSWER_MS, AnswerClock, LoopTimer, Report, STALL_MS, STORM_LIMIT, Served, StormWindow,
    serial_hex, storm,
};
use chimera_hal::Ms;

#[test]
fn the_timer_averages_and_peaks_then_resets() {
    let mut t = LoopTimer::new();
    assert_eq!(t.take(), (0, 0));
    for us in [800, 900, 4200, 700] {
        t.lap(us, Served::Idle);
    }
    assert_eq!(t.take(), (1650, 4200));
    assert_eq!(t.take(), (0, 0), "reading resets");
}

#[test]
fn an_answered_iteration_is_not_timed() {
    let mut t = LoopTimer::new();
    t.lap(800, Served::Idle);
    t.lap(300_000, Served::Answered);
    t.lap(900, Served::Idle);
    assert_eq!(t.take(), (850, 900));
}

#[test]
fn a_long_run_does_not_overflow() {
    let mut t = LoopTimer::new();
    for _ in 0..10_000_000 {
        t.lap(u32::MAX / 2, Served::Idle);
    }
    assert_eq!(t.take(), (u32::MAX / 2, u32::MAX / 2));
}

#[test]
fn a_report_is_headings_and_lines() {
    let mut r = Report::<64>::new();
    r.heading("VOICES");
    r.line("ALG 1  12 24");
    assert_eq!(r.as_str(), "# VOICES\nALG 1  12 24\n");
}

#[test]
fn a_full_report_ends_in_truncated() {
    let mut r = Report::<40>::new();
    for i in 0..10 {
        r.line(&format!("line {i}"));
    }
    assert!(r.as_str().ends_with("# TRUNCATED\n"), "{}", r.as_str());
    assert!(r.as_str().len() <= 40);
    let before = r.as_str().to_string();
    r.line("more");
    assert_eq!(r.as_str(), before, "nothing after the cut");
    assert_eq!(r.as_str().matches("# TRUNCATED").count(), 1);
}

#[test]
fn a_report_that_fits_exactly_is_not_cut() {
    let mut r = Report::<{ 6 + 12 }>::new();
    r.line("abcde");
    assert_eq!(r.as_str(), "abcde\n");
}

#[test]
fn a_drawn_line_is_recorded_and_drawn() {
    let mut r = Report::<64>::new();
    let mut seen = String::new();
    r.drawn("ALG 1", |t| seen.push_str(t));
    assert_eq!(seen, "ALG 1");
    assert_eq!(r.as_str(), "ALG 1\n");
}

#[test]
fn serial_is_24_uppercase_hex_digits() {
    let uid = [0x00, 0x12, 0x00, 0xAB, 0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 1];
    assert_eq!(serial_hex(&uid).as_str(), "001200ABDEADBEEF00000001");
}

#[test]
fn an_answer_stalls_after_250_ms_without_progress() {
    let mut c = AnswerClock::start(Ms(100));
    assert!(!c.expired(Ms(100 + STALL_MS - 1)));
    assert!(c.expired(Ms(100 + STALL_MS)));
    c.progress(Ms(300));
    assert!(
        !c.expired(Ms(300 + STALL_MS - 1)),
        "progress restarts the stall"
    );
    assert!(c.expired(Ms(300 + STALL_MS)));
}

#[test]
fn a_slow_dribble_still_ends_at_the_answer_deadline() {
    let mut c = AnswerClock::start(Ms(0));
    let mut now = 0;
    while !c.expired(Ms(now)) {
        now += 10;
        if now % 240 == 0 {
            c.progress(Ms(now));
        }
    }
    assert_eq!(now, ANSWER_MS);
}

#[test]
fn the_answer_clock_reads_across_a_wrap() {
    let start = u32::MAX - 100;
    let mut c = AnswerClock::start(Ms(start));
    c.progress(Ms(start.wrapping_add(200)));
    assert!(!c.expired(Ms(start.wrapping_add(400))));
    assert!(c.expired(Ms(start.wrapping_add(ANSWER_MS))));
}

/// 480 MHz: the unit's DWT cycles in a millisecond.
const MS: u32 = 480_000;

#[test]
fn storm_trips_over_limit() {
    let mut w = StormWindow::new();
    for k in 0..STORM_LIMIT {
        assert!(!storm(&mut w, 1_000 + k * 100, 0, MS), "entry {}", k + 1);
    }
    assert!(storm(&mut w, 1_000 + STORM_LIMIT * 100, 0, MS), "the 201st");
}

#[test]
fn window_resets_each_ms() {
    let mut w = StormWindow::new();
    for ms in 0..10 {
        for k in 0..STORM_LIMIT {
            assert!(
                !storm(&mut w, ms * MS + k * 2_000, 0, MS),
                "ms {ms} entry {k}"
            );
        }
    }
}

#[test]
fn wraps_dwt() {
    let mut w = StormWindow::new();
    let start = u32::MAX - MS / 2;
    for k in 0..STORM_LIMIT {
        assert!(!storm(&mut w, start.wrapping_add(k), 0, MS));
    }
    // A millisecond on, past the counter's wrap: a fresh window.
    let next = start.wrapping_add(MS);
    for k in 0..STORM_LIMIT {
        assert!(
            !storm(&mut w, next.wrapping_add(k), 0, MS),
            "entry {k} after the wrap"
        );
    }
    assert!(storm(&mut w, next.wrapping_add(STORM_LIMIT), 0, MS));
}

#[test]
fn a_window_opens_on_its_first_cycle() {
    let mut w = StormWindow::new();
    for k in 0..STORM_LIMIT {
        assert!(!storm(&mut w, k, 0, MS));
    }
    assert!(!storm(&mut w, MS, 0, MS), "cycle MS opens a fresh window");
}

/// Entry `k` of a run that starts one every `every` cycles, each owning
/// `own` cycles, with audio taking `audio` per mille of the CPU.
fn run(w: &mut StormWindow, every: u32, own: u32, audio: u32, k: u32) -> bool {
    let enter = 1_000 + k * every;
    let tripped = storm(w, enter, k * every / 1_000 * audio, MS);
    w.charge(own);
    tripped
}

#[test]
fn slow_entries_over_half_a_window_trip() {
    let mut w = StormWindow::new();
    for k in 0..3 {
        assert!(!run(&mut w, 10_000, MS / 5, 0, k));
    }
    // 60 % of the millisecond in three entries.
    assert!(storm(&mut w, 40_000, 0, MS));
}

#[test]
fn slow_entries_under_half_a_window_do_not() {
    let mut w = StormWindow::new();
    for k in 0..2 {
        assert!(!run(&mut w, 10_000, MS / 5, 0, k));
    }
    assert!(!storm(&mut w, 30_000, 0, MS), "40 % of the millisecond");
    // The next millisecond starts its own count.
    w.charge(MS / 5);
    assert!(!storm(&mut w, MS + 1_000, 0, MS));
}

/// Audio at 70 %: 10 µs entries back to back, 3 µs of each USB's own. Its
/// own time passes half the non-audio time before the entry limit.
#[test]
fn a_storm_under_heavy_audio_trips() {
    let mut w = StormWindow::new();
    let us10 = 10 * MS / 1_000;
    let tripped = (0..STORM_LIMIT)
        .find(|&k| run(&mut w, us10, 3 * us10 / 10, 700, k))
        .expect("trips under the entry limit");
    assert!(tripped < 100, "inside the millisecond: entry {tripped}");
}

/// One entry audio preempted for 0.9 ms of its wall time, 2 µs its own:
/// audio at 67 % is no storm.
#[test]
fn audio_preemption_is_not_a_storm() {
    let mut w = StormWindow::new();
    assert!(!storm(&mut w, 1_000, 0, MS));
    w.charge(2 * MS / 1_000);
    let wall = 9 * MS / 10;
    assert!(!storm(
        &mut w,
        1_000 + wall + 100,
        wall - 2 * MS / 1_000,
        MS
    ));
}

/// An OUT packet's entry, then its IN completion's 1 µs on: no storm.
#[test]
fn a_back_to_back_pair_does_not_trip() {
    let mut w = StormWindow::new();
    let us = MS / 1_000;
    let at = 10 * MS; // opens its own window
    assert!(!storm(&mut w, at, 0, MS));
    w.charge(2 * us);
    assert!(!storm(&mut w, at + 3 * us, 0, MS));
}

/// 40 entries a millisecond at 2 µs each under 70 % audio: never trips.
#[test]
fn quiet_traffic_does_not_trip() {
    let mut w = StormWindow::new();
    for k in 0..400 {
        assert!(!run(&mut w, MS / 40, 2 * MS / 1_000, 700, k), "entry {k}");
    }
}
