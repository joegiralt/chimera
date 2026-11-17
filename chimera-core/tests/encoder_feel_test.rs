//! Encoder feel (ADR 0279, #1005): lists and menus accelerate gentler than
//! a parameter, and SEQ + turn steps fine with no push.

mod common;
mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::hold::{HoldGates, Press};
use chimera_core::ui::{UiState, hint, theme, view};
use chimera_hal::{Accel, ButtonId, ButtonState, Controls, EncoderId, Turn, encoder_accel};

/// The panel's acceleration, as it was: 1, 2, 4, 5, 10, then 10.
#[test]
fn a_param_accelerates_as_before() {
    for burst in 0..20u8 {
        for detents in 1..=6i8 {
            let want: i32 = (0..detents as u8)
                .map(|k| i32::from(encoder_accel(burst.saturating_add(k))))
                .sum();
            let t = Turn {
                detents,
                burst: Some(burst),
            };
            assert_eq!(i32::from(Accel::Param.ticks(t)), want.min(127));
            let back = Turn {
                detents: -detents,
                burst: Some(burst),
            };
            assert_eq!(Accel::Param.ticks(back), -Accel::Param.ticks(t));
        }
    }
}

/// A list moves one row a detent for its first four, never more than
/// three a detent, and never further than a parameter would.
#[test]
fn a_list_accelerates_gentler() {
    for burst in 0..4u8 {
        assert_eq!(
            Accel::List.ticks(Turn {
                detents: 1,
                burst: Some(burst)
            }),
            1
        );
    }
    for burst in 0..40u8 {
        let t = Turn {
            detents: 1,
            burst: Some(burst),
        };
        assert!(Accel::List.ticks(t) <= 3);
        assert!(Accel::List.ticks(t) <= Accel::Param.ticks(t));
    }
    assert_eq!(
        Accel::List.ticks(Turn {
            detents: 0,
            burst: Some(9)
        }),
        0
    );
}

/// A shell that does not accelerate (the desktop, a test's hand) reports
/// ticks as they are, under either curve.
#[test]
fn a_steady_turn_is_its_ticks() {
    for d in [-9i8, -1, 0, 1, 5, 12] {
        let t = Turn {
            detents: d,
            burst: None,
        };
        assert_eq!((Accel::Param.ticks(t), Accel::List.ticks(t)), (d, d));
    }
}

/// One frame: turns (detents at a burst position) and held buttons.
#[derive(Default)]
struct Frame {
    turns: Vec<(EncoderId, Turn)>,
    held: Vec<ButtonId>,
    now: u32,
}

impl Controls for Frame {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        Accel::Param.ticks(self.turn(id))
    }
    fn turn(&self, id: EncoderId) -> Turn {
        self.turns
            .iter()
            .find(|t| t.0 == id)
            .map_or(Turn::default(), |t| t.1)
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        if self.held.contains(&id) {
            ButtonState::Held
        } else {
            ButtonState::Up
        }
    }
    fn now_ms(&self) -> chimera_hal::Ms {
        chimera_hal::Ms(self.now)
    }
}

fn level(ui: &UiState) -> f32 {
    ui.project().part(PartId::ALL[0]).mix.level
}

/// Part 1's mixer PART page: D is LEVEL, 0.8, a step 1/128.
fn part_page() -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    screen::feed(&mut ui, screen::Input::chord(ButtonId::Mix, ButtonId::B1));
    screen::feed(&mut ui, screen::Input::press(ButtonId::Minus));
    ui
}

/// Deep in a fast burst, a plain turn accelerates; SEQ held, each detent
/// is one fine step (a tenth with pushes built in, else a whole one).
#[test]
fn seq_and_a_turn_step_fine() {
    let fast = Turn {
        detents: 3,
        burst: Some(8),
    };
    let mut plain = part_page();
    plain.handle_input(&Frame {
        turns: vec![(
            EncoderId::D,
            Turn {
                detents: -3,
                ..fast
            },
        )],
        ..Frame::default()
    });
    assert!(level(&plain) < 0.8 - 20.0 / 128.0, "{}", level(&plain));

    let mut ui = part_page();
    ui.handle_input(&Frame {
        turns: vec![(
            EncoderId::D,
            Turn {
                detents: -3,
                ..fast
            },
        )],
        held: vec![ButtonId::Seq],
        ..Frame::default()
    });
    let step = if cfg!(feature = "push-encoders") {
        0.1 / 128.0
    } else {
        1.0 / 128.0
    };
    assert!(
        (level(&ui) - (0.8 - 3.0 * step)).abs() < 1e-6,
        "{}",
        level(&ui)
    );
}

/// SEQ held through a turn is a fine turn, not a key: its release is no
/// tap (no move up) and holding on never opens ORBIT.
#[test]
fn a_turn_mutes_seq() {
    let mut g = HoldGates::new();
    // Edges by hand: pressed at 0, turned at 100, held to 900, let go.
    let frames = [
        chimera_hal::Edges {
            down: true,
            pressed_at: Some(chimera_hal::Ms(0)),
            released_at: None,
        },
        chimera_hal::Edges {
            down: true,
            pressed_at: None,
            released_at: None,
        },
        chimera_hal::Edges {
            down: true,
            pressed_at: None,
            released_at: None,
        },
        chimera_hal::Edges {
            down: false,
            pressed_at: None,
            released_at: Some(chimera_hal::Ms(900)),
        },
    ];
    let turned = [false, true, false, false];
    let times = [0, 100, 800, 900];
    for ((e, t), now) in frames.into_iter().zip(turned).zip(times) {
        let f = SeqFrame {
            seq: e,
            turned: t,
            now,
        };
        let p = g.step(&f);
        assert_eq!(p.seq, None::<Press>, "at {now} ms");
    }
}

/// SEQ's edges and one encoder's turn.
struct SeqFrame {
    seq: chimera_hal::Edges,
    turned: bool,
    now: u32,
}

impl Controls for SeqFrame {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        i8::from(self.turned && id == EncoderId::C)
    }
    fn button_state(&self, _: ButtonId) -> ButtonState {
        ButtonState::Up
    }
    fn edges(&self, id: ButtonId) -> chimera_hal::Edges {
        if id == ButtonId::Seq {
            self.seq
        } else {
            chimera_hal::Edges::default()
        }
    }
    fn now_ms(&self) -> chimera_hal::Ms {
        chimera_hal::Ms(self.now)
    }
}

/// A value cell's band names the fine turn, and the line fits.
#[test]
fn a_value_cell_hints_seq_fine() {
    assert!(view::FINE_LEARN_HINT.iter().any(|g| g.1 == view::FINE.1));
    assert!(hint::width(view::FINE_LEARN_HINT) <= theme::CONTENT_W);
    assert!(hint::width(view::FINE_HINT) <= theme::CONTENT_W);
}

/// A pick from names turns as a list; a level as a parameter.
#[test]
fn a_choice_turns_as_a_list() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ValFmt;
    use chimera_core::params::OutParams;
    use chimera_core::ui::view::View;
    let level = ParamAddr::new(BlockRef::Out, OutParams::VOLUME);
    let cell = |fmt| View::Param {
        addr: level,
        label: "X",
        fmt,
        short: None,
    };
    assert_eq!(cell(ValFmt::Uni).accel(), Accel::Param);
    assert_eq!(cell(ValFmt::Names(&["A", "B"])).accel(), Accel::List);
    assert_eq!(View::SelectPad.accel(), Accel::List);
}

/// A SEQ hold with no turn still opens ORBIT at 500 ms, as before.
#[test]
fn an_untouched_seq_hold_still_holds() {
    let mut g = HoldGates::new();
    let down = |at| chimera_hal::Edges {
        down: true,
        pressed_at: at,
        released_at: None,
    };
    let f = |seq, now| SeqFrame {
        seq,
        turned: false,
        now,
    };
    assert_eq!(g.step(&f(down(Some(chimera_hal::Ms(0))), 0)).seq, None);
    assert_eq!(g.step(&f(down(None), 499)).seq, None);
    assert_eq!(g.step(&f(down(None), 500)).seq, Some(Press::Hold));
}

/// The focus band's hint line: INK pixels in it, column by column.
fn hint_line(ui: &UiState) -> Vec<bool> {
    let fb = screen::render_ui(ui);
    (theme::MARGIN_X..theme::SCREEN_W - theme::MARGIN_X)
        .flat_map(|x| (hint::FOCUS_Y - 7..=hint::FOCUS_Y).map(move |y| (x, y)))
        .map(|(x, y)| fb.at(x, y) == theme::INK)
        .collect()
}

/// On a mixer cell the band hints the PART jump; SEQ held there, it says
/// the fine turn in its place, and let go, the PART jump again.
#[test]
fn seq_held_on_a_mixer_cell_hints_fine() {
    let mut ui = part_page();
    ui.set_midi(chimera_core::ui::midi_settings::MidiSettings::DEFAULT.with_push(false));
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
    let jump = hint_line(&ui);
    ui.handle_input(&Frame {
        held: vec![ButtonId::Seq],
        ..Frame::default()
    });
    let fine = hint_line(&ui);
    assert_ne!(fine, jump, "SEQ held: the fine turn's hint");
    assert!(fine.iter().any(|&p| p));
    ui.handle_input(&Frame::default());
    assert_eq!(hint_line(&ui), jump, "let go: the PART jump again");
}
