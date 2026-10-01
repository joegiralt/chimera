use chimera_core::ui::hold::{HOLD_MS, HoldGate, Press};
use chimera_hal::{Edges, Ms};

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
