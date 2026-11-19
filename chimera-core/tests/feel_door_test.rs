//! The encoders' feel behind bank 2's door (`UiDoor::feel`, ADR 0279),
//! under `bank2` on a host, where the table answers only while the door
//! cell is open. Closed: a SEQ + turn is a plain, accelerated turn, the
//! mixer band says its PART jump, CLASSIC RATE reads two places. Open: the
//! fine step and its hint. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

mod screen;

use chimera_core::bank2::DOOR_CELL;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_hal::{Accel, BurstPos, ButtonId, ButtonState, Controls, Detents, EncoderId, Ms, Turn};

/// LEVEL turned 3 detents deep in a burst, SEQ held.
struct SeqFast;

impl Controls for SeqFast {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        Accel::Param.ticks(self.turn(id)).get()
    }
    fn turn(&self, id: EncoderId) -> Turn {
        if id == EncoderId::D {
            Turn::at(Detents::new(-3), BurstPos::new(8))
        } else {
            Turn::default()
        }
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        if id == ButtonId::Seq {
            ButtonState::Held
        } else {
            ButtonState::Up
        }
    }
    fn now_ms(&self) -> Ms {
        Ms(0)
    }
}

/// Part 1's mixer PART page, LEVEL 0.8.
fn mixer() -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    ui.set_midi(chimera_core::ui::midi_settings::MidiSettings::DEFAULT.with_push(false));
    screen::feed(&mut ui, screen::Input::chord(ButtonId::Mix, ButtonId::B1));
    screen::feed(&mut ui, screen::Input::press(ButtonId::Minus));
    ui
}

fn level(ui: &UiState) -> f32 {
    ui.project().part(PartId::ALL[0]).mix.level
}

fn says(ui: &UiState, keys: &str) -> bool {
    ui.focus_hint_for_test().iter().any(|g| g.0 == keys)
}

#[test]
fn closed_plain_open_fine() {
    use chimera_core::block::ValFmt;
    use chimera_core::dsp::modulator::law::Law;
    use chimera_core::ui::fmt::{FmtBuf, fmt_val};
    let rate = || {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, 0.0, ValFmt::Law(Law::LfoHz));
        b.as_str().to_owned()
    };

    DOOR_CELL.close();
    let mut ui = mixer();
    ui.handle_input(&SeqFast);
    assert!(level(&ui) < 0.8 - 20.0 / 128.0, "closed: accelerated");
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
    assert!(says(&ui, "MIX + B1-6"), "{:?}", ui.focus_hint_for_test());
    assert_eq!(rate(), "0.01 Hz");

    DOOR_CELL.open_host();
    let mut ui = mixer();
    ui.handle_input(&SeqFast);
    assert!(level(&ui) > 0.8 - 4.0 / 128.0, "open: fine");
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
    assert!(says(&ui, "SEQ+TURN"), "{:?}", ui.focus_hint_for_test());
    assert_eq!(rate(), "0.0100 Hz");
}
