//! SYSTEM's PUSH ENCODERS through the UI (ADR 0140): a push let go with no
//! turn sets the cell to its default, a turn while pushed steps a tenth,
//! MIX wins over the push, cells with no value take nothing, OFF changes
//! nothing, and a value cell's band hints the pushes.

#![cfg(feature = "push-encoders")]

mod common;
mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_core::ui::push::{self, Push, PushStep};
use chimera_core::ui::{UiState, hint, theme};
use chimera_hal::{ButtonId, ButtonState, Controls, Edges, EncoderId, Ms};
use screen::render_ui;

/// One frame: turns, pushes, MIX.
#[derive(Default)]
struct Frame {
    enc: Vec<(EncoderId, i8)>,
    push: Vec<(EncoderId, Edges)>,
    mix: bool,
}

impl Controls for Frame {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.enc.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        if id == ButtonId::Mix && self.mix {
            ButtonState::Held
        } else {
            ButtonState::Up
        }
    }
    fn push(&self, id: EncoderId) -> Edges {
        self.push
            .iter()
            .find(|p| p.0 == id)
            .map_or(Edges::default(), |p| p.1)
    }
}

const DOWN: Edges = Edges {
    down: true,
    pressed_at: Some(Ms(0)),
    released_at: None,
};
const HELD: Edges = Edges {
    down: true,
    pressed_at: None,
    released_at: None,
};
const UP: Edges = Edges {
    down: false,
    pressed_at: None,
    released_at: Some(Ms(0)),
};

fn feed(ui: &mut UiState, f: Frame) {
    ui.handle_input(&f);
}

fn pushed(e: EncoderId, edges: Edges) -> Frame {
    Frame {
        push: vec![(e, edges)],
        ..Frame::default()
    }
}

/// Push `e` down and let it go, no turn.
fn tap(ui: &mut UiState, e: EncoderId) {
    feed(ui, pushed(e, DOWN));
    feed(ui, pushed(e, UP));
}

/// A UI with PUSH ENCODERS `on`, on Part 1's mixer `PART` page (MIX + B1,
/// then MINUS from SENDS), or on SENDS.
fn ui(on: bool, part_page: bool) -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    ui.set_midi(MidiSettings {
        push: on,
        ..MidiSettings::DEFAULT
    });
    screen::feed(&mut ui, screen::Input::chord(ButtonId::Mix, ButtonId::B1));
    if part_page {
        screen::feed(&mut ui, screen::Input::press(ButtonId::Minus));
    }
    ui
}

fn level(ui: &UiState) -> f32 {
    ui.project().part(PartId::ALL[0]).mix.level
}

/// D is LEVEL: 0.8 by default, a step 1/128.
#[test]
fn a_tap_sets_the_default() {
    let mut ui = ui(true, true);
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, -10));
    assert_ne!(level(&ui), 0.8);
    tap(&mut ui, EncoderId::D);
    assert_eq!(level(&ui), 0.8);
}

#[test]
fn a_turn_pushed_is_a_tenth_and_its_release_no_tap() {
    let mut ui = ui(true, true);
    feed(&mut ui, pushed(EncoderId::D, DOWN));
    feed(
        &mut ui,
        Frame {
            enc: vec![(EncoderId::D, 4)],
            push: vec![(EncoderId::D, HELD)],
            ..Frame::default()
        },
    );
    let fine = 0.8 + 1.0 / 1280.0;
    assert!((level(&ui) - fine).abs() < 1e-6, "{}", level(&ui));
    feed(&mut ui, pushed(EncoderId::D, UP));
    assert!((level(&ui) - fine).abs() < 1e-6, "the release keeps it");
}

/// MIX + turn snaps, pushed or not.
#[test]
fn mix_wins_over_the_push() {
    let turn = |on_push: bool| {
        let mut ui = ui(true, true);
        if on_push {
            feed(&mut ui, pushed(EncoderId::D, DOWN));
        }
        feed(
            &mut ui,
            Frame {
                enc: vec![(EncoderId::D, -1)],
                push: if on_push {
                    vec![(EncoderId::D, HELD)]
                } else {
                    vec![]
                },
                mix: true,
            },
        );
        level(&ui)
    };
    assert_eq!(turn(true), turn(false));
    assert!(
        (turn(true) - (0.8 - 0.1 / 128.0)).abs() > 1e-3,
        "no fine step"
    );
}

/// SENDS' D, E and F are empty: a tap there changes nothing.
#[test]
fn a_tap_on_an_empty_cell_does_nothing() {
    let mut ui = ui(true, false);
    let before = format!("{:?}", ui.params());
    for e in [EncoderId::D, EncoderId::E, EncoderId::F] {
        tap(&mut ui, e);
    }
    assert_eq!(format!("{:?}", ui.params()), before);
}

/// OFF: a push is no tap, and a pushed turn is the plain turn.
#[test]
fn off_changes_nothing() {
    let mut ui = ui(false, true);
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, -10));
    let turned = level(&ui);
    tap(&mut ui, EncoderId::D);
    assert_eq!(level(&ui), turned, "no default");
    feed(&mut ui, pushed(EncoderId::D, DOWN));
    feed(
        &mut ui,
        Frame {
            enc: vec![(EncoderId::D, 1)],
            push: vec![(EncoderId::D, HELD)],
            ..Frame::default()
        },
    );
    assert_eq!(level(&ui), turned + 1.0 / 128.0, "a whole step");
}

/// The focus band's hint line: INK pixels in it.
fn hint_ink(ui: &UiState) -> usize {
    let fb = render_ui(ui);
    (theme::MARGIN_X..theme::SCREEN_W - theme::MARGIN_X)
        .flat_map(|x| (hint::FOCUS_Y - 7..=hint::FOCUS_Y).map(move |y| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == theme::INK)
        .count()
}

/// LEVEL touched: ON, its band hints the pushes; OFF, nothing. It fits.
#[test]
fn a_value_cell_hints_the_pushes_when_on() {
    let touched = |on| {
        let mut ui = ui(on, true);
        screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
        hint_ink(&ui)
    };
    assert!(touched(true) > 0);
    assert_eq!(touched(false), 0);
    assert!(hint::width(push::HINT) <= theme::CONTENT_W);
}

fn step(p: &mut Push, e: Edges, turned: bool) -> PushStep {
    p.step(e, turned)
}

#[test]
fn push_steps() {
    let mut p = Push::default();
    let none = PushStep::default();
    let fine = PushStep {
        fine: true,
        tap: false,
    };
    let tap = PushStep {
        fine: false,
        tap: true,
    };
    assert_eq!(step(&mut p, Edges::default(), true), none, "not pushed");
    assert_eq!(step(&mut p, DOWN, false), none);
    assert_eq!(step(&mut p, UP, false), tap, "let go, no turn");
    assert_eq!(step(&mut p, DOWN, true), fine, "pressed and turned at once");
    assert_eq!(step(&mut p, UP, false), none, "turned: no tap");
    let in_frame = Edges {
        down: false,
        pressed_at: Some(Ms(0)),
        released_at: Some(Ms(5)),
    };
    assert_eq!(step(&mut p, in_frame, false), tap, "tapped inside a frame");
    // Let go and pushed again in one frame: the old push taps.
    step(&mut p, DOWN, false);
    let again = Edges {
        down: true,
        pressed_at: Some(Ms(9)),
        released_at: Some(Ms(8)),
    };
    assert_eq!(step(&mut p, again, false), tap);
    assert_eq!(step(&mut p, HELD, true), fine, "the new push is held");
}
