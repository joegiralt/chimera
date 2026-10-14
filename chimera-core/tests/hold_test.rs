mod screen;

use chimera_core::orbit::RINGS;
use chimera_core::ui::hold::{HOLD_MS, HoldGate, HoldGates, Press, Presses};
use chimera_hal::{ButtonId, ButtonState, Controls};
use chimera_hal::{Edges, Ms};
use screen::Input;

const FRAME_MS: u32 = 33;

fn press(t: u32) -> Edges {
    Edges {
        down: true,
        pressed_at: Some(Ms(t)),
        released_at: None,
    }
}
fn held() -> Edges {
    Edges {
        down: true,
        ..Edges::default()
    }
}
fn release(t: u32) -> Edges {
    Edges {
        down: false,
        pressed_at: None,
        released_at: Some(Ms(t)),
    }
}

#[test]
fn tap_is_the_release() {
    let mut g = HoldGate::new();
    assert_eq!(g.step(press(0), Ms(0), false), None);
    let mut t = FRAME_MS;
    while t < HOLD_MS {
        assert_eq!(g.step(held(), Ms(t), false), None);
        t += FRAME_MS;
    }
    assert_eq!(
        g.step(release(HOLD_MS - 1), Ms(HOLD_MS - 1), false),
        Some(Press::Tap)
    );
    assert_eq!(g.step(Edges::default(), Ms(HOLD_MS + 40), false), None);
}

#[test]
fn hold_fires_once_at_hold_ms() {
    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), false);
    assert_eq!(g.step(held(), Ms(HOLD_MS - 1), false), None);
    assert_eq!(g.step(held(), Ms(HOLD_MS), false), Some(Press::Hold));
    assert_eq!(g.step(held(), Ms(HOLD_MS + FRAME_MS), false), None);
    assert_eq!(g.step(release(HOLD_MS + 60), Ms(HOLD_MS + 60), false), None);
}

#[test]
fn hold_ms_minus_one_frame_is_a_tap() {
    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), false);
    let t = HOLD_MS - FRAME_MS;
    assert_eq!(g.step(release(t), Ms(t), false), Some(Press::Tap));

    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), false);
    let t = HOLD_MS + FRAME_MS;
    assert_eq!(g.step(held(), Ms(t), false), Some(Press::Hold));
}

#[test]
fn hold_stalled_through_release_fires_one_hold() {
    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), false);
    assert_eq!(g.step(release(850), Ms(900), false), Some(Press::Hold));
    assert_eq!(g.step(Edges::default(), Ms(933), false), None);
}

#[test]
fn tap_inside_a_stalled_frame() {
    let mut g = HoldGate::new();
    let e = Edges {
        down: false,
        pressed_at: Some(Ms(1000)),
        released_at: Some(Ms(1080)),
    };
    assert_eq!(g.step(e, Ms(1400), false), Some(Press::Tap));
}

#[test]
fn release_then_press_in_one_frame() {
    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), false);
    let e = Edges {
        down: true,
        pressed_at: Some(Ms(1100)),
        released_at: Some(Ms(1050)),
    };
    assert_eq!(g.step(e, Ms(1120), false), Some(Press::Hold));
    assert_eq!(g.step(release(1200), Ms(1200), false), Some(Press::Tap));
}

#[test]
fn muted_press_yields_nothing() {
    let mut g = HoldGate::new();
    assert_eq!(g.step(press(0), Ms(0), true), None);
    assert_eq!(g.step(release(100), Ms(100), false), None);

    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), true);
    assert_eq!(g.step(held(), Ms(HOLD_MS + FRAME_MS), false), None);
    assert_eq!(g.step(release(HOLD_MS + 60), Ms(HOLD_MS + 60), false), None);
}

#[test]
fn muted_stalled_tap_yields_nothing() {
    let mut g = HoldGate::new();
    let e = Edges {
        down: false,
        pressed_at: Some(Ms(1000)),
        released_at: Some(Ms(1080)),
    };
    assert_eq!(g.step(e, Ms(1400), true), None);
}

#[test]
fn hold_ms_is_500() {
    assert_eq!(HOLD_MS, 500);
}

#[test]
fn muted_press_does_not_mute_the_next() {
    let mut g = HoldGate::new();
    assert_eq!(g.step(press(0), Ms(0), true), None);
    assert_eq!(g.step(release(100), Ms(100), false), None);
    assert_eq!(g.step(press(200), Ms(200), false), None);
    assert_eq!(g.step(release(300), Ms(300), false), Some(Press::Tap));
}

#[test]
fn release_at_exactly_hold_ms_is_a_hold() {
    let mut g = HoldGate::new();
    g.step(press(0), Ms(0), false);
    assert_eq!(
        g.step(release(HOLD_MS), Ms(HOLD_MS), false),
        Some(Press::Hold)
    );
}

struct Stub {
    now: u32,
    mix: Edges,
    menu: Edges,
    seq: Edges,
}

impl Controls for Stub {
    fn encoder_delta(&self, _: chimera_hal::EncoderId) -> i8 {
        0
    }
    fn button_state(&self, _: ButtonId) -> ButtonState {
        ButtonState::Up
    }
    fn edges(&self, id: ButtonId) -> Edges {
        match id {
            ButtonId::Mix => self.mix,
            ButtonId::Menu => self.menu,
            ButtonId::Seq => self.seq,
            _ => Edges::default(),
        }
    }
    fn now_ms(&self) -> Ms {
        Ms(self.now)
    }
}

fn stub(now: u32, mix: Edges, menu: Edges, seq: Edges) -> Stub {
    Stub {
        now,
        mix,
        menu,
        seq,
    }
}

const NONE: Presses = Presses {
    menu: None,
    seq: None,
    ring: [None; RINGS],
};

#[test]
fn mix_held_mutes_a_menu_tap() {
    let mut g = HoldGates::new();
    assert_eq!(g.step(&stub(0, press(0), press(0), Edges::default())), NONE);
    assert_eq!(
        g.step(&stub(100, held(), release(100), Edges::default())),
        NONE
    );
}

#[test]
fn mix_released_first_still_mutes_the_menu_press() {
    let mut g = HoldGates::new();
    g.step(&stub(0, press(0), press(0), Edges::default()));
    assert_eq!(
        g.step(&stub(50, release(50), held(), Edges::default())),
        NONE
    );
    assert_eq!(
        g.step(&stub(100, Edges::default(), release(100), Edges::default())),
        NONE
    );
}

/// MIX + SEQ is learn's (MIDI map spec § 4): no SEQ tap follows.
#[test]
fn seq_tap_is_muted_by_mix() {
    let mut g = HoldGates::new();
    g.step(&stub(0, press(0), Edges::default(), press(0)));
    let p = g.step(&stub(100, held(), Edges::default(), release(100)));
    assert_eq!(p.seq, None);
    assert_eq!(p.menu, None);
}

// Frames as the harness's `tap` and `hold` feed them.
fn run(frames: &[Input]) -> Vec<Presses> {
    let mut g = HoldGates::new();
    frames.iter().map(|f| g.step(f)).collect()
}

#[test]
fn harness_hold_gives_one_hold_and_tap_one_tap() {
    let b = ButtonId::Menu;
    let h = run(&[
        Input::press(b).at(0),
        Input::held(b).at(HOLD_MS),
        Input::release(b).at(HOLD_MS + 33),
    ]);
    assert_eq!(
        h.iter().filter_map(|p| p.menu).collect::<Vec<_>>(),
        [Press::Hold]
    );
    let t = run(&[Input::press(b).at(0), Input::release(b).at(100)]);
    assert_eq!(
        t.iter().filter_map(|p| p.menu).collect::<Vec<_>>(),
        [Press::Tap]
    );
}

/// B3's gate: a tap on the release under `HOLD_MS`, a hold once at it.
#[test]
fn b_gates_tap_and_hold() {
    let b = ButtonId::B3;
    let ring = |ps: Vec<Presses>| -> Vec<Option<Press>> { ps.iter().map(|p| p.ring[2]).collect() };
    let t = run(&[Input::press(b).at(0), Input::release(b).at(HOLD_MS - 1)]);
    assert_eq!(ring(t.clone()), [None, Some(Press::Tap)]);
    let h = run(&[
        Input::press(b).at(0),
        Input::held(b).at(HOLD_MS),
        Input::release(b).at(HOLD_MS + 33),
    ]);
    assert_eq!(ring(h.clone()), [None, Some(Press::Hold), None]);
    for p in t.iter().chain(&h) {
        assert_eq!(
            (p.menu, p.seq, [p.ring[0], p.ring[1], p.ring[3], p.ring[4]]),
            (None, None, [None; 4])
        );
    }
}

/// MIX + B3 is a mute and only that: MIX let go first, B3 held 600 ms,
/// neither a ring tap nor a cross follows (Review Focus 6).
#[test]
fn mix_b_is_only_a_mute() {
    let b = ButtonId::B3;
    let frames = || {
        [
            Input::chord(ButtonId::Mix, b).at(1000),
            Input::release(ButtonId::Mix).and_held(b).at(1033),
            Input::held(b).at(1600),
            Input::release(b).at(1633),
        ]
    };
    assert!(run(&frames()).iter().all(|p| p.ring == [None; RINGS]));

    #[cfg(feature = "orbit")]
    {
        use chimera_core::orbit::RingIdx;
        use chimera_core::ui::UiState;
        use chimera_core::ui::nav::OrbitKey;
        let mut ui = Box::new(UiState::new());
        screen::hold(&mut ui, ButtonId::Seq);
        let at = ui.location();
        assert!(at.orbit().is_some());
        let keys: Vec<OrbitKey> = frames()
            .iter()
            .flat_map(|f| ui.orbit_frame_for_test(f))
            .flatten()
            .collect();
        assert_eq!(keys, [OrbitKey::Mute(RingIdx::ALL[2])]);
        // Through `handle_input`, the place stays.
        for f in frames() {
            screen::feed(&mut ui, f);
        }
        assert_eq!(ui.location(), at);
    }
}

/// MIX + PLUS and MIX + MINUS are no ORBIT key: they stay free, as on the
/// ladder.
#[cfg(feature = "orbit")]
#[test]
fn mix_plus_and_minus_are_no_orbit_key() {
    use chimera_core::ui::UiState;
    use chimera_core::ui::nav::OrbitKey;
    let mut ui = Box::new(UiState::new());
    screen::hold(&mut ui, ButtonId::Seq);
    assert!(ui.location().orbit().is_some());
    for b in [ButtonId::Plus, ButtonId::Minus] {
        let keys = ui.orbit_frame_for_test(&Input::chord(ButtonId::Mix, b));
        assert_eq!(keys, [None; 11], "MIX + {b:?}");
        ui.orbit_frame_for_test(&Input::release(b));
    }
    let keys = ui.orbit_frame_for_test(&Input::press(ButtonId::Plus));
    assert_eq!(keys[0], Some(OrbitKey::Plus));
}
