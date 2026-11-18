//! MIXER › PRT › EQ (ADR 0284): TILT and LO CUT, under VOICES; the
//! Part's mix, never its Sound.

mod common;
mod screen;

use chimera_core::part_eq::{LoCut, PartEq, Tilt};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry as reg;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed};

/// MIX + B1, back to PRT, EDIT down twice: EQ.
fn eq_page() -> UiState {
    let mut ui = screen::stack_ui();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::press(ButtonId::Minus));
    assert_eq!(ui.page_def().id, reg::PART.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, reg::PART_VOICES.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, reg::PART_EQ.id);
    ui.step_toast(60_000);
    ui
}

#[test]
fn the_eq_page_turns_the_part_s_tilt_and_lo_cut() {
    let mut ui = eq_page();
    let p = PartId::ALL[0];
    let sound = format!("{:?}", ui.project().part(p).sound.params);
    assert_eq!(ui.project().part(p).mix.eq, PartEq::FLAT);
    feed(&mut ui, Input::turn(EncoderId::A, 5));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(
        ui.project().part(p).mix.eq,
        PartEq::new(Tilt::at(5), LoCut::On)
    );
    assert_eq!(
        format!("{:?}", ui.project().part(p).sound.params),
        sound,
        "the Sound untouched"
    );
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    assert!(screen::cell_reads(&shot, 0, "+5"), "TILT reads its step");
    assert!(screen::cell_reads(&shot, 1, "ON"));
}

#[test]
fn up_from_eq_is_voices_and_down_the_sound() {
    let mut ui = eq_page();
    screen::tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.page_def().id, reg::PART_VOICES.id);
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_ne!(ui.page_def().id, reg::PART_EQ.id, "on to the Sound");
}
