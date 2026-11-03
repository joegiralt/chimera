use chimera_core::console::{
    self, ANSWER_MS, AnswerClock, LoopTimer, Report, RxHead, RxPark, STALL_MS, STORM_LIMIT, Served,
    Storm, StormKinds, StormWindow, park, serial_hex,
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
    starved_by(now, tick_at, alive, per_ms).is_some()
}

fn starved_by(now: u32, tick_at: u32, alive: Option<u32>, per_ms: u32) -> Option<Storm> {
    let c = Cycles::new;
    console::starved(c(now), c(tick_at), alive.map(c), c(per_ms))
}

fn trip(w: &mut StormWindow, now: u32, tick_at: u32, alive: Option<u32>) -> Option<Storm> {
    let c = Cycles::new;
    console::trip(w, c(now), c(tick_at), alive.map(c), c(MS))
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
    assert_eq!(
        starved_by(t + 7 * MS, t, Some(t), MS),
        Some(Storm::Tick),
        "7 ms"
    );
    assert!(!starved(t + 5 * MS, t, Some(t), MS), "5 ms");
}

/// SysTick steps every 2 ms: 6 ms is three steps missed, never one late.
#[test]
fn the_tick_test_sits_three_steps_out() {
    assert_eq!(console::TICK_STALL_MS, 6);
    let t = 5 * MS;
    assert!(!starved(t + 2 * MS + MS / 2, t, Some(t), MS), "a late step");
}

#[test]
fn a_stalled_service_starves() {
    let s = 5 * MS;
    let tick = s + 248 * MS; // SysTick still moving
    assert_eq!(
        starved_by(s + 251 * MS, tick, Some(s), MS),
        Some(Storm::Ui),
        "251 ms"
    );
    assert!(!starved(s + 249 * MS, tick, Some(s), MS), "249 ms");
}

/// Both stalled: the tick's test is the one named.
#[test]
fn a_stalled_tick_names_the_tick_first() {
    let s = 5 * MS;
    assert_eq!(starved_by(s + 251 * MS, s, Some(s), MS), Some(Storm::Tick));
}

#[test]
fn trip_names_each_test() {
    let mut w = StormWindow::new();
    let t = 5 * MS;
    for k in 0..STORM_LIMIT {
        assert_eq!(trip(&mut w, t + k, t, Some(t)), None, "entry {k}");
    }
    // The entries' test first, even with the tick stalled.
    assert_eq!(
        trip(&mut w, t + STORM_LIMIT, t, Some(t)),
        Some(Storm::Entries)
    );
    let mut w = StormWindow::new();
    assert_eq!(trip(&mut w, t + 7 * MS, t, Some(t)), Some(Storm::Tick));
    let mut w = StormWindow::new();
    let now = t + 251 * MS;
    assert_eq!(trip(&mut w, now, now, Some(t)), Some(Storm::Ui));
}

/// The most entries one millisecond held, for § 13.5's 4x rule (#848).
#[test]
fn storm_kinds_keep_the_busiest_millisecond() {
    let mut w = StormWindow::new();
    let mut k = StormKinds::new();
    for (ms, n) in [(1, 3), (2, 9), (3, 5)] {
        for e in 0..n {
            storm(&mut w, ms * MS + e, MS);
            k.saw(&w);
        }
    }
    assert_eq!(k.peak, 9);
    assert_eq!(k.total(), 0, "a peak is no trip");
}

#[test]
fn storm_kinds_count_each_and_total() {
    let mut k = StormKinds::new();
    for s in [
        Storm::Entries,
        Storm::Tick,
        Storm::Tick,
        Storm::Ui,
        Storm::Ui,
        Storm::Ui,
    ] {
        k.add(s);
    }
    assert_eq!((k.entries, k.tick, k.ui, k.total()), (1, 2, 3, 6));
    let mut k = StormKinds {
        entries: u32::MAX,
        tick: 1,
        ui: 0,
        ..StormKinds::new()
    };
    k.add(Storm::Entries);
    assert_eq!(
        (k.entries, k.total()),
        (0, 1),
        "wrapping, as the counters do"
    );
}

/// One handler entry's RX FIFO head, before its poll and after.
#[test]
fn parks_only_a_head_the_poll_left() {
    // GRXSTSR: EPNUM 0..3, BCNT 4..14, PKTSTS 17..20.
    let full = Some(RxHead(0x0004_0402)); // OUT data (2), 64 bytes, EP2
    let done = Some(RxHead(0x0006_0002)); // its OUT-complete (3) behind it
    assert!(park(full, full), "nobody took it: a full console");
    assert!(!park(full, done), "read: the next entry is the poll's");
    assert!(!park(full, None), "the FIFO emptied");
    assert!(!park(None, done), "a packet came in during the poll");
    assert!(!park(None, None));
}

/// Only an OUT data entry parks: a SETUP the poll left (a host's identical
/// retry, or a suspend served first) or an unchanged complete entry
/// re-enters as before.
#[test]
fn parks_only_out_data() {
    let setup = Some(RxHead(0x000C_0080)); // SETUP data (6), 8 bytes, EP0
    let done = Some(RxHead(0x0006_0002));
    assert!(!park(setup, setup));
    assert!(!park(done, done));
}

/// The park's steps as the handler and `unlock` take them; each `Some`
/// is the RXFLVLM write the board makes (`Some(true)`: cleared, parked).
#[test]
fn the_park_parks_on_a_stuck_head_and_every_hold_lets_it_back() {
    let full = Some(RxHead(0x0004_0402));
    let done = Some(RxHead(0x0006_0002));
    let mut r = RxPark::new();
    assert_eq!(r.unlock(), None, "nothing parked, no write");
    assert_eq!(r.polled(full, done), None, "progress, unparked already");
    assert_eq!(r.polled(full, full), Some(true), "a full console parks");
    assert!(r.parked());
    assert_eq!(r.polled(full, full), None, "parked once");
    assert_eq!(r.unlock(), Some(false), "the hold lets it back");
    assert!(!r.parked());
    assert_eq!(r.unlock(), None, "once");
    assert_eq!(r.polled(full, full), Some(true), "still full: parked again");
    assert_eq!(r.polled(full, None), Some(false), "a bus reset flushed it");
    assert_eq!(r.polled(full, full), Some(true));
    assert_eq!(r.polled(full, done), Some(false), "read meanwhile");
}

/// Parked if and only if RXFLVLM is clear, over every step sequence up to
/// eight long.
#[test]
fn parked_is_rxflvlm_clear() {
    let full = Some(RxHead(0x0004_0402));
    let done = Some(RxHead(0x0006_0002));
    let setup = Some(RxHead(0x000C_0080));
    let steps = [
        (full, full),
        (full, done),
        (full, None),
        (setup, setup),
        (None, None),
    ];
    // Step 5: a UI hold's unlock.
    for seq in 0..6u32.pow(8) {
        let (mut r, mut masked_in) = (RxPark::new(), true);
        let mut s = seq;
        for _ in 0..8 {
            let w = match (s % 6) as usize {
                5 => r.unlock(),
                i => r.polled(steps[i].0, steps[i].1),
            };
            if let Some(p) = w {
                assert_ne!(masked_in, !p, "a write that changes nothing");
                masked_in = !p;
            }
            assert_eq!(r.parked(), !masked_in, "{seq}");
            s /= 6;
        }
    }
}

#[test]
fn starved_reads_across_the_dwt_wrap() {
    let at = u32::MAX - 5 * MS;
    assert!(starved(at.wrapping_add(7 * MS), at, Some(at), MS));
    assert!(!starved(at.wrapping_add(5 * MS), at, Some(at), MS));
    assert!(starved(
        at.wrapping_add(251 * MS),
        at.wrapping_add(248 * MS),
        Some(at),
        MS
    ));
    assert!(!starved(
        at.wrapping_add(249 * MS),
        at.wrapping_add(248 * MS),
        Some(at),
        MS
    ));
}

/// Before the UI's first `service` only the tick can starve.
#[test]
fn no_service_yet_is_not_starved() {
    let tick = 2 * MS;
    assert!(!starved(tick + 5 * MS, tick, None, MS));
    assert!(!starved(1_000 * MS, 999 * MS, None, MS), "1 s from boot");
    assert!(starved(tick + 7 * MS, tick, None, MS));
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

use chimera_core::console::{PerMille, WallClock};

/// The unit's USB flood under a heavy chord: each 22 ms lap the handler
/// storms until the tick test trips at 20 ms of DWT (its limit then), SysTick (the shell's
/// `Ms`) stalled all the while, and the handler gets a third of it (audio
/// takes the rest). Shares over SysTick's ms read the stalled clock: the
/// storm's cycles over 2 ms a lap, pinned at 100 %. Over the DWT wall
/// clock, a third of 20 ms in 22.
#[test]
fn a_storm_that_stalls_systick_keeps_the_share_true() {
    let (lap, stalled, own_lap) = (22 * MS, 20 * MS, 20 * MS / 3);
    let (mut dwt, mut ms) = (u32::MAX - 3 * MS, Ms(1_000));
    let mut wall = WallClock::new(Cycles::new(dwt), ms);
    let (start_ms, mut own) = (ms, 0u64);
    for _ in 0..100 {
        dwt = dwt.wrapping_add(lap);
        ms = Ms(ms.0 + (lap - stalled) / MS);
        own += u64::from(own_lap);
        wall.fold(Cycles::new(dwt), ms, Cycles::new(MS));
    }
    let by_ms = u64::from(ms.since(start_ms)) * u64::from(MS);
    assert_eq!(
        PerMille::of(own, by_ms),
        PerMille::of(1, 1),
        "the stalled clock"
    );
    assert_eq!(PerMille::of(own, wall.total()), PerMille::of(20, 66));
}

/// A lap longer than the cycle counter's 8.9 s wrap (an install): SysTick,
/// stalled at most `TICK_STALL_MS` a trip, says how many wraps passed.
#[test]
fn a_lap_past_the_dwt_wrap_counts_whole() {
    let mut wall = WallClock::new(Cycles::new(5), Ms(0));
    let ten_s = 10_000u64 * u64::from(MS);
    let dwt = Cycles::new(5u32.wrapping_add(ten_s as u32));
    assert_eq!(wall.fold(dwt, Ms(10_000), Cycles::new(MS)), ten_s);
    // 20 ms of SysTick lost in the next 9 s: still one wrap, by DWT.
    let nine_s = 9_000u64 * u64::from(MS);
    let dwt = Cycles::new(dwt.get().wrapping_add(nine_s as u32));
    assert_eq!(wall.fold(dwt, Ms(18_980), Cycles::new(MS)), ten_s + nine_s);
}
