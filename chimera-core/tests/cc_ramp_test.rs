//! The pure CC core (MIDI map spec §§ 5-6): the inter-arrival ramp, jitter,
//! stepped binning with hysteresis and the round-robin MOVES budget.

use chimera_core::midi_map::ramp::{HYST, RAMP_MIN_MS, Ramp, bin, budget};
use chimera_hal::Ms;

/// Steps `r` by `dt` ms until rest; returns the ms taken and the places seen.
fn run(r: &mut Ramp, dt: u32) -> (u32, Vec<f32>) {
    let (mut t, mut seen) = (0, Vec::new());
    while let Some(p) = r.step(dt) {
        t += dt;
        seen.push(p);
        assert!(t <= 10_000, "ramp never rests");
    }
    (t, seen)
}

/// Ms a 0.0 -> 1.0 ramp takes when its CC follows the last by `dt`.
fn cross_ms(dt: u32) -> u32 {
    let mut r = Ramp::rest(0.0);
    assert!(r.hear(10, 0.0, Ms(1_000)));
    assert!(r.hear(20, 1.0, Ms(1_000 + dt)));
    let (t, seen) = run(&mut r, 1);
    assert_eq!(seen.last(), Some(&1.0));
    t
}

#[test]
fn ramp_crosses_in_inter_arrival() {
    let mut r = Ramp::rest(0.0);
    assert!(r.hear(0, 0.0, Ms(0)));
    assert!(r.hear(64, 0.5, Ms(80)));
    let (t, seen) = run(&mut r, 10);
    assert!((70..=90).contains(&t), "crossed in {t} ms");
    assert_eq!(seen.last(), Some(&0.5));
    assert!(
        seen.windows(2).all(|w| w[0] <= w[1]),
        "not monotonic: {seen:?}"
    );
    assert!(
        seen.iter().all(|&p| (0.0..=0.5).contains(&p)),
        "overshoot: {seen:?}"
    );
    assert_eq!(r.step(10), None);
}

#[test]
fn ramp_clamps() {
    assert_eq!(cross_ms(5), RAMP_MIN_MS);
    assert_eq!(cross_ms(100), 100);
    assert_eq!(cross_ms(1_000), RAMP_MIN_MS);
}

#[test]
fn ramp_first_cc_uses_the_floor() {
    let mut r = Ramp::rest(0.0);
    assert!(r.hear(127, 1.0, Ms(5_000)));
    assert_eq!(run(&mut r, 1).0, RAMP_MIN_MS);
}

#[test]
fn ramp_retargets_from_where_it_is() {
    let mut r = Ramp::rest(0.0);
    r.hear(0, 0.0, Ms(0));
    r.hear(127, 1.0, Ms(100));
    let mid = r.step(50).unwrap();
    assert!(mid > 0.0 && mid < 1.0);
    // A new CC turns it round from `mid`, never jumping.
    assert!(r.hear(10, 0.0, Ms(150)));
    let (t, seen) = run(&mut r, 5);
    assert_eq!(t, 50);
    assert!(seen[0] < mid && seen[0] > 0.0);
    assert!(seen.windows(2).all(|w| w[0] >= w[1]));
    assert_eq!(seen.last(), Some(&0.0));
}

#[test]
fn ramp_handles_the_clock_wrap() {
    let mut r = Ramp::rest(0.0);
    r.hear(0, 0.0, Ms(u32::MAX - 39));
    r.hear(100, 1.0, Ms(40));
    assert_eq!(run(&mut r, 1).0, 80);
}

#[test]
fn ramp_cancel_rests_at_the_hand() {
    let mut r = Ramp::rest(0.0);
    r.hear(127, 1.0, Ms(0));
    r.step(3);
    r.cancel(0.25);
    assert_eq!(r.step(10), None);
    assert!(r.hear(0, 0.25, Ms(500)));
    assert_eq!(r.step(10), None, "a CC at the place it rests is no move");
}

#[test]
fn ramp_ignores_jitter() {
    let mut r = Ramp::rest(0.0);
    assert!(r.hear(64, 0.5, Ms(0)));
    assert!(!r.hear(64, 0.5, Ms(30)));

    let mut r = Ramp::rest(0.0);
    assert!(r.hear(64, 0.5, Ms(0)));
    assert!(r.hear(65, 0.51, Ms(20)));
    assert!(!r.hear(64, 0.5, Ms(40)));

    let mut r = Ramp::rest(0.0);
    assert!(r.hear(64, 0.5, Ms(0)));
    assert!(r.hear(65, 0.51, Ms(60)));
    assert!(r.hear(64, 0.5, Ms(120)));
}

#[test]
fn bin_hysteresis() {
    let b = |v, cur| bin(v, 4, 0, 255, cur);
    let mut cur = 1;
    for v in [63, 64, 63, 64, 65] {
        cur = b(v, cur);
        assert_eq!(cur, 1, "v {v}");
    }
    cur = b(64 + HYST, cur);
    assert_eq!(cur, 2);
    cur = b(63, cur);
    assert_eq!(cur, 2);
    cur = b(64 - 1 - HYST, cur);
    assert_eq!(cur, 1);
}

#[test]
fn bin_reaches_the_ends_and_inverts() {
    // Fine steps narrower than the hysteresis still reach both ends.
    assert_eq!(bin(0, 100, 0, 255, 1), 0);
    assert_eq!(bin(127, 100, 0, 255, 98), 99);
    // `min > max` inverts.
    assert_eq!(bin(0, 4, 255, 0, 0), 3);
    assert_eq!(bin(127, 4, 255, 0, 3), 0);
    // A mapped half-range only spans its half of the steps.
    assert_eq!(bin(127, 4, 0, 127, 0), 1);
    // Degenerate step counts never divide by zero.
    assert_eq!(bin(64, 0, 0, 255, 0), 0);
    assert_eq!(bin(64, 1, 0, 255, 0), 0);
}

#[test]
fn budget_round_robin() {
    let due = [true; 8];
    let mut cursor = 0;
    let picked = |p: [bool; 8]| (0..8).filter(|&i| p[i]).collect::<Vec<_>>();
    assert_eq!(picked(budget(&due, &mut cursor, 5)), [0, 1, 2, 3, 4]);
    assert_eq!(picked(budget(&due, &mut cursor, 5)), [0, 1, 5, 6, 7]);
    let mut served = [0; 8];
    for _ in 0..3 {
        let p = budget(&due, &mut cursor, 5);
        assert!(p.iter().filter(|&&x| x).count() <= 5);
        (0..8).filter(|&i| p[i]).for_each(|i| served[i] += 1);
    }
    assert!(served.iter().all(|&n| n >= 1), "starved: {served:?}");
}

#[test]
fn budget_skips_what_is_not_due() {
    let mut due = [false; 16];
    due[3] = true;
    due[9] = true;
    let mut cursor = 5;
    let p = budget(&due, &mut cursor, 5);
    assert!(p[3] && p[9]);
    assert_eq!(p.iter().filter(|&&x| x).count(), 2);
    assert_eq!(budget(&[false; 16], &mut cursor, 5), [false; 16]);
}

#[test]
fn ramp_stays_small() {
    // 6 Parts × 16 entries of these live in RAM.
    assert_eq!(core::mem::size_of::<Ramp>(), 16);
}
