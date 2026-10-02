use chimera_core::console::{LoopTimer, Report, Served, serial_hex};

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
