use chimera_core::orbit::clock::{step_at, step_start, Bpm, InternalClock, Phase, Position};
use chimera_core::orbit::euclid::Steps;

const SR: u32 = 48_000;

#[test]
fn step_index_is_exact_for_any_count() {
    for n in 1..=32u8 {
        let steps = Steps::new(n).unwrap();
        for k in 0..n {
            let s = step_start(k, steps);
            assert_eq!(step_at(s, steps), k, "n={n} k={k}");
            if k > 0 {
                assert_eq!(step_at(Phase::new(s.get() - 1), steps), k - 1, "n={n} k={k}");
            }
        }
        assert_eq!(step_at(Phase::new(u32::MAX), steps), n - 1);
    }
}

#[test]
fn internal_clock_never_drifts() {
    let clock = InternalClock::start(0, Bpm::new(97).unwrap(), SR);
    let bars = 97 * 60 / 4 + 1; // one hour
    for n in 0..bars as u64 {
        let want = (n * 48_000 * 240 * 2 + 97) / (97 * 2);
        let at = Position { bar: n as u32, phase: Phase::ZERO };
        assert_eq!(clock.sample_of(at), want, "bar {n}");
        assert_eq!(clock.position(want), at, "bar {n}");
    }
}

#[test]
fn bpm_change_keeps_the_position() {
    let mut clock = InternalClock::start(0, Bpm::new(120).unwrap(), SR);
    let p = clock.position(99_968);
    clock.set_bpm(99_968, p, Bpm::new(60).unwrap());
    assert_eq!(clock.position(99_968), p);
    let bar = clock.bar_len() as u64;
    assert_eq!(bar, 192_000);
    let q = clock.position(99_968 + bar / 2);
    let moved = (q.bar as u64) << 32 | q.phase.get() as u64;
    let from = (p.bar as u64) << 32 | p.phase.get() as u64;
    let half = (moved - from) as i64;
    assert!((half - (1i64 << 31)).abs() <= 1, "half a bar later: {half}");
}

#[test]
fn position_before_the_anchor_saturates() {
    let clock = InternalClock::start(1000, Bpm::DEFAULT, SR);
    assert_eq!(clock.position(0), Position::ZERO);
}

#[test]
fn bpm_is_bounded() {
    assert!(Bpm::new(29).is_none() && Bpm::new(301).is_none());
    assert_eq!(Bpm::clamped(5).get(), 30);
    assert_eq!(Bpm::clamped(999).get(), 300);
}
