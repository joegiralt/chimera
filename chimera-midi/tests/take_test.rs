use chimera_midi::ctl::Dir;
use chimera_midi::take::*;
use chimera_midi::value::U7;
use proptest::prelude::*;

fn p(v: u16) -> Place {
    Place::new(v).unwrap()
}
fn u7p(v: u8) -> Place {
    Place::from_u7_replicated(U7::new(v).unwrap())
}

#[test]
fn place_bounds_and_replication() {
    assert!(Place::new(16383).is_some());
    assert!(Place::new(16384).is_none());
    assert_eq!(u7p(0).get(), 0);
    assert_eq!(u7p(127).get(), 16383);
    assert_eq!(u7p(1).get(), 129);
    assert_eq!(TOL_7BIT, 258);
}

#[test]
fn jump_moves_at_once() {
    let mut s = TakeState::new();
    assert_eq!(
        take(Takeover::Jump, &mut s, p(100), p(9000), TOL_7BIT),
        Taken::Move(p(100))
    );
    assert_eq!(
        take(Takeover::Jump, &mut s, p(16000), p(100), TOL_7BIT),
        Taken::Move(p(16000))
    );
}

#[test]
fn pickup_waits_below_then_engages_on_cross_up() {
    let mut s = TakeState::new();
    let param = p(8000);
    for v in [1000, 3000, 6000] {
        assert_eq!(
            take(Takeover::Pickup, &mut s, p(v), param, 10),
            Taken::Waiting { toward: Dir::Up }
        );
    }
    // jumps over the param between two messages
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(9000), param, 10),
        Taken::Move(p(9000))
    );
    // engaged: follows, whatever the param says
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(500), param, 10),
        Taken::Move(p(500))
    );
}

#[test]
fn pickup_waits_above_then_engages_on_cross_down() {
    let mut s = TakeState::new();
    let param = p(8000);
    for v in [16000, 12000] {
        assert_eq!(
            take(Takeover::Pickup, &mut s, p(v), param, 10),
            Taken::Waiting { toward: Dir::Down }
        );
    }
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(7000), param, 10),
        Taken::Move(p(7000))
    );
}

#[test]
fn pickup_engages_within_tolerance() {
    let first = |v| {
        take(
            Takeover::Pickup,
            &mut TakeState::new(),
            p(v),
            p(8000),
            TOL_7BIT,
        )
    };
    assert_eq!(first(8000 - 259), Taken::Waiting { toward: Dir::Up });
    assert_eq!(first(8000 - 258), Taken::Move(p(8000 - 258)));
    assert_eq!(first(8000 + 258), Taken::Move(p(8000 + 258)));
    assert_eq!(first(8000 + 259), Taken::Waiting { toward: Dir::Down });
}

#[test]
fn pickup_rearms_on_reset() {
    let mut s = TakeState::new();
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(8000), p(8000), 10),
        Taken::Move(p(8000))
    );
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(9000), p(8000), 10),
        Taken::Move(p(9000))
    );
    // the panel moved the param elsewhere
    s.reset();
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(9100), p(2000), 10),
        Taken::Waiting { toward: Dir::Down }
    );
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(1900), p(2000), 10),
        Taken::Move(p(1900))
    );
}

#[test]
fn waiting_reports_direction() {
    let mut s = TakeState::new();
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(0), p(16383), 10),
        Taken::Waiting { toward: Dir::Up }
    );
    let mut s = TakeState::new();
    assert_eq!(
        take(Takeover::Pickup, &mut s, p(16383), p(0), 10),
        Taken::Waiting { toward: Dir::Down }
    );
}

fn scaled(last: u16, param: u16, incoming: u16) -> u16 {
    let mut s = TakeState::new();
    take(Takeover::Scale, &mut s, p(last), p(param), 0);
    match take(Takeover::Scale, &mut s, p(incoming), p(param), 0) {
        Taken::Move(m) => m.get(),
        w => panic!("scale never waits: {w:?}"),
    }
}

proptest! {
    #[test]
    fn scale_is_monotonic(last in 0u16..=16383, param in 0u16..=16383, a in 0u16..=16383, b in 0u16..=16383) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(scaled(last, param, lo) <= scaled(last, param, hi));
    }
}

#[test]
fn scale_meets_at_both_ends() {
    for (last, param) in [
        (8000, 2000),
        (2000, 14000),
        (16000, 100),
        (1, 16382),
        (8000, 8000),
    ] {
        assert_eq!(scaled(last, param, 16383), 16383, "{last} {param}");
        assert_eq!(scaled(last, param, 0), 0, "{last} {param}");
        assert_eq!(scaled(last, param, last), param, "no move, no change");
    }
    // halfway up from (c=8000, p=2000): p moves half its headroom
    assert_eq!(
        scaled(8000, 2000, 8000 + (16383 - 8000) / 2),
        2000 + (16383 - 2000) / 2 - 1
    );
}

#[test]
fn seven_bit_sender_engages_fourteen_bit_param() {
    // A param off the 7-bit grid: a slow 7-bit sweep must still engage.
    for param in [1, 64, 8000, 8063, 12345, 16382] {
        let mut s = TakeState::new();
        let engaged = (0..=127u8).any(|v| {
            matches!(
                take(Takeover::Pickup, &mut s, u7p(v), p(param), TOL_7BIT),
                Taken::Move(_)
            )
        });
        assert!(engaged, "param {param}");
        // and the first move lands within the tolerance
        let mut s = TakeState::new();
        let first = (0..=127u8)
            .find_map(
                |v| match take(Takeover::Pickup, &mut s, u7p(v), p(param), TOL_7BIT) {
                    Taken::Move(m) => Some(m.get()),
                    _ => None,
                },
            )
            .unwrap();
        assert!(
            first.abs_diff(param) <= TOL_7BIT,
            "param {param} first {first}"
        );
    }
}
