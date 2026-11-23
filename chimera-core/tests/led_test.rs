// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The front-panel LED: ORBIT's beat, longer on the downbeat (#307).

use chimera_core::ui::led::{BEAT_TICKS, DOWNBEAT_TICKS, Led, Pulse};

fn lit_for(mut led: Led, first: Option<Pulse>) -> usize {
    led = led.step(first);
    let mut n = 0;
    while led.on() {
        n += 1;
        led = led.step(None);
    }
    n
}

#[test]
fn pulse_of_counts_bars() {
    for b in [0, 4, 8] {
        assert_eq!(Pulse::of(b), Pulse::Downbeat, "beat {b}");
    }
    for b in [1, 2, 3, 5] {
        assert_eq!(Pulse::of(b), Pulse::Beat, "beat {b}");
    }
}

#[test]
fn beat_is_short_and_downbeat_long() {
    assert_eq!(lit_for(Led::OFF, Some(Pulse::Beat)), BEAT_TICKS as usize);
    assert_eq!(
        lit_for(Led::OFF, Some(Pulse::Downbeat)),
        DOWNBEAT_TICKS as usize
    );
}

#[test]
fn a_pulse_never_shortens() {
    // Downbeat tick 1, then tick 2 with a Beat: the Downbeat still ends
    // on its third tick.
    let mut led = Led::OFF.step(Some(Pulse::Downbeat));
    led = led.step(Some(Pulse::Beat));
    assert!(led.on());
    led = led.step(None);
    assert!(led.on(), "the third tick");
    assert!(!led.step(None).on(), "and no longer");
}

#[cfg(feature = "orbit")]
mod through_ui {
    use chimera_core::orbit::clock::Phase;
    use chimera_core::orbit::runner::OrbitView;
    use chimera_core::ui::UiState;
    use chimera_core::ui::animation::UiTick;

    fn view(running: bool, beat: u32) -> OrbitView {
        let mut v = OrbitView {
            running,
            ..Default::default()
        };
        v.pos.bar = beat / 4;
        v.pos.phase = Phase::new((beat % 4) << 30);
        v
    }

    /// One UiTick on `v`: the LED after it.
    fn tick(ui: &mut UiState, v: &OrbitView) -> bool {
        ui.set_orbit_view(v);
        ui.update(UiTick::for_test());
        ui.led()
    }

    #[test]
    fn led_follows_the_beat() {
        let mut ui = Box::new(UiState::new());
        assert!(!ui.led());
        for b in 0..=8u32 {
            let v = view(true, b);
            let long = b % 4 == 0;
            assert!(tick(&mut ui, &v), "beat {b} lights on its first tick");
            assert_eq!(tick(&mut ui, &v), long, "beat {b}, tick 2");
            assert_eq!(tick(&mut ui, &v), long, "beat {b}, tick 3");
            assert!(!tick(&mut ui, &v), "beat {b}, tick 4");
        }
        // A stalled frame skips beat 6: one pulse for the newest, beat 7.
        assert!(tick(&mut ui, &view(true, 5)));
        assert!(!tick(&mut ui, &view(true, 5)));
        assert!(tick(&mut ui, &view(true, 7)));
        assert!(!tick(&mut ui, &view(true, 7)));
        // A downbeat then stop: off at the very next tick, and it stays.
        assert!(tick(&mut ui, &view(true, 8)));
        let stopped = view(false, 8);
        for _ in 0..5 {
            assert!(!tick(&mut ui, &stopped));
        }
    }
}
