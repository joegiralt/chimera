use chimera_core::orbit::clock::{Bpm, InternalClock, Phase, Position, step_at, step_start};
use chimera_core::orbit::euclid::Steps;
use core::num::NonZeroU32;

const SR: NonZeroU32 = match NonZeroU32::new(48_000) {
    Some(n) => n,
    None => panic!(),
};

#[test]
fn step_index_is_exact_for_any_count() {
    for n in 1..=32u8 {
        let steps = Steps::new(n).unwrap();
        for k in 0..n {
            let s = step_start(k, steps);
            assert_eq!(step_at(s, steps), k, "n={n} k={k}");
            if k > 0 {
                assert_eq!(
                    step_at(Phase::new(s.get() - 1), steps),
                    k - 1,
                    "n={n} k={k}"
                );
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
        let at = Position {
            bar: n as u32,
            phase: Phase::ZERO,
        };
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

fn key(p: Position) -> u64 {
    (p.bar as u64) << 32 | p.phase.get() as u64
}

#[test]
fn position_is_monotonic_across_bar_lines() {
    let clock = InternalClock::start(0, Bpm::new(97).unwrap(), SR);
    // bars 1..=3 and 100..=102: both rounding directions
    for line in [1u32, 2, 3, 100, 101, 102] {
        let at = clock.sample_of(Position {
            bar: line,
            phase: Phase::ZERO,
        });
        let mut prev = clock.position(at - 400);
        for s in at - 399..=at + 400 {
            let p = clock.position(s);
            assert!(p >= prev, "backwards at {s}");
            // one sample is ~2^32 / 23.7k = ~181k phase; allow a rounding sample
            assert!(key(p) - key(prev) < 400_000, "jump at {s}");
            assert_eq!(
                p.phase == Phase::ZERO,
                s == at,
                "phase 0 only on the line, {s}"
            );
            prev = p;
        }
    }
}

#[test]
fn mid_bar_bpm_change_round_trips() {
    let mut clock = InternalClock::start(0, Bpm::new(97).unwrap(), SR);
    let p = clock.position(300_001);
    assert!(p.phase != Phase::ZERO && p.bar > 0);
    clock.set_bpm(300_001, p, Bpm::new(143).unwrap());
    assert_eq!(clock.position(300_001), p);
    assert_eq!(clock.sample_of(p), 300_001);
    let next = Position {
        bar: p.bar + 1,
        phase: Phase::ZERO,
    };
    let line = clock.sample_of(next);
    assert_eq!(clock.position(line), next);
    assert!(clock.position(line - 1) < next);
    // the rest of the bar takes (1 - phase) of a bar at 143 BPM
    let want = (1.0 - p.phase.get() as f64 / 4294967296.0) * 48_000.0 * 240.0 / 143.0;
    assert!(((line - 300_001) as f64 - want).abs() <= 1.0);
}

/// A leader that keeps sending Continue, and no Clock, for over an hour:
/// the run past the last tick is clamped, never overflows, and stops
/// short of the next tick, in debug as in release.
#[test]
fn midi_clock_hour_without_clock_holds_before_the_next_tick() {
    use chimera_core::orbit::clock::MidiClock;
    let sr = 48_000u64;
    let mut m = MidiClock::new(NonZeroU32::new(sr as u32).unwrap());
    assert!(m.start(0, 0));
    for k in 1..=25 {
        assert!(m.tick(0, k * 500));
    }
    let last = 25 * 500;
    // A tick's interval past it: held just short of the next.
    let held = m.position(last + 500).unwrap();
    assert!(held > m.position(last + 499).unwrap());
    let mut now = last;
    while now < last + 70 * 60 * sr {
        now += sr / 4;
        assert!(m.resume(0, now));
    }
    assert_eq!(m.position(now), Some(held), "held short of the next tick");
}

/// One Clock lost from a steady 120 BPM, drained a block at a time: the
/// doubled interval is no tempo change, and the held beat stays within a
/// tenth of a BPM through it (#1046's review), never 4 % slow for a beat.
#[test]
fn a_dropped_clock_never_moves_the_held_tempo() {
    use chimera_core::orbit::clock::MidiClock;
    let sr = 48_000u64;
    let tick = sr * 60 / (120 * 24);
    let mut worst = 0.0f64;
    // The lost Clock at each place in a beat's window.
    for lost in 24 * 20..24 * 21 {
        let mut m = MidiClock::new(NonZeroU32::new(sr as u32).unwrap());
        for n in 0..24 * 60u64 {
            if n == lost {
                continue;
            }
            let now = n * tick / 64 * 64;
            assert!(m.tick(0, now));
            if n > 24 * 8
                && let Some(b) = m.beat()
            {
                worst = worst.max((60.0 * sr as f64 / f64::from(b) - 120.0).abs());
            }
        }
    }
    assert!(worst < 0.1, "held {worst} BPM off");
}
