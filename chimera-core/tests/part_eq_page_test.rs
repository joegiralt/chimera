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
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
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
    // Step 5 of 64 to the shelves' 10 dB: 0.78 dB.
    assert!(screen::cell_reads(&shot, 0, "+0.8 dB"), "TILT reads its dB");
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

#[test]
fn tilt_reads_its_db_flat_0_and_the_ends_10() {
    let mut ui = eq_page();
    let mut reads = |turn: i8, want: &str| {
        for _ in 0..3 {
            feed(&mut ui, Input::turn(EncoderId::A, turn));
        }
        screen::settle(&mut ui);
        let shot = screen::render_ui(&ui);
        assert!(screen::cell_reads(&shot, 0, want), "{want}");
    };
    reads(0, "0 dB");
    reads(-100, "-10.0 dB");
    reads(100, "+9.8 dB");
}
