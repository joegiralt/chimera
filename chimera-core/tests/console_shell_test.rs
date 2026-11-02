use chimera_core::console::{
    self, ANSWER_MS, AnswerClock, LoopTimer, Report, STALL_MS, STORM_LIMIT, Served, StormWindow,
    serial_hex,
};
use chimera_hal::Ms;
use chimera_hal::units::Cycles;

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

/// The guard's tests, read in raw DWT counts.
fn storm(w: &mut StormWindow, now: u32, per_ms: u32) -> bool {
    console::storm(w, Cycles::new(now), Cycles::new(per_ms))
}

fn starved(now: u32, tick_at: u32, alive: Option<u32>, per_ms: u32) -> bool {
    let c = Cycles::new;
    console::starved(c(now), c(tick_at), alive.map(c), c(per_ms))
}

#[test]
fn storm_trips_over_limit() {
    let mut w = StormWindow::new();
    for k in 0..STORM_LIMIT {
        assert!(!storm(&mut w, 1_000 + k * 100, MS), "entry {}", k + 1);
    }
    assert!(storm(&mut w, 1_000 + STORM_LIMIT * 100, MS), "the 201st");
}

#[test]
fn window_resets_each_ms() {
    let mut w = StormWindow::new();
    for ms in 0..10 {
        for k in 0..STORM_LIMIT {
            assert!(!storm(&mut w, ms * MS + k * 2_000, MS), "ms {ms} entry {k}");
        }
    }
}

#[test]
fn wraps_dwt() {
    let mut w = StormWindow::new();
    let start = u32::MAX - MS / 2;
    for k in 0..STORM_LIMIT {
        assert!(!storm(&mut w, start.wrapping_add(k), MS));
    }
    // A millisecond on, past the counter's wrap: a fresh window.
    let next = start.wrapping_add(MS);
    for k in 0..STORM_LIMIT {
        assert!(
            !storm(&mut w, next.wrapping_add(k), MS),
            "entry {k} after the wrap"
        );
    }
    assert!(storm(&mut w, next.wrapping_add(STORM_LIMIT), MS));
}

#[test]
fn a_window_opens_on_its_first_cycle() {
    let mut w = StormWindow::new();
    for k in 0..STORM_LIMIT {
        assert!(!storm(&mut w, k, MS));
    }
    assert!(!storm(&mut w, MS, MS), "cycle MS opens a fresh window");
}

#[test]
fn a_stalled_tick_starves() {
    let t = 5 * MS;
    assert!(starved(t + 21 * MS, t, Some(t), MS), "21 ms");
    assert!(!starved(t + 19 * MS, t, Some(t), MS), "19 ms");
}

#[test]
fn a_stalled_service_starves() {
    let s = 5 * MS;
    let tick = s + 240 * MS; // SysTick still moving
    assert!(starved(s + 251 * MS, tick, Some(s), MS), "251 ms");
    assert!(!starved(s + 249 * MS, tick, Some(s), MS), "249 ms");
}

#[test]
fn starved_reads_across_the_dwt_wrap() {
    let at = u32::MAX - 5 * MS;
    assert!(starved(at.wrapping_add(21 * MS), at, Some(at), MS));
    assert!(!starved(at.wrapping_add(19 * MS), at, Some(at), MS));
    assert!(starved(
        at.wrapping_add(251 * MS),
        at.wrapping_add(240 * MS),
        Some(at),
        MS
    ));
    assert!(!starved(
        at.wrapping_add(249 * MS),
        at.wrapping_add(240 * MS),
        Some(at),
        MS
    ));
}

/// Before the UI's first `service` only the tick can starve.
#[test]
fn no_service_yet_is_not_starved() {
    let tick = 2 * MS;
    assert!(!starved(tick + 19 * MS, tick, None, MS));
    assert!(!starved(1_000 * MS, 999 * MS, None, MS), "1 s from boot");
    assert!(starved(tick + 21 * MS, tick, None, MS));
}

/// A long answer: the UI holds the cell every 100 ms inside one `service`
/// for a second while SysTick runs. Never starved.
#[test]
fn a_long_answer_pumping_is_not_starved() {
    let t = 5 * MS;
    for k in 1..=10 {
        let held = t + (k - 1) * 100 * MS;
        for ms in (0..100).step_by(2) {
            let now = held + ms * MS;
            assert!(!starved(now, now, Some(held), MS), "{k} {ms}");
        }
    }
}
