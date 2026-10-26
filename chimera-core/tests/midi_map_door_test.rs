//! MIDI MAP behind bank 2's door (spec § 8.2): closed, the page reads
//! `NEEDS UPDATE` and its keys change nothing; open again, it lists and
//! edits. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support", feature = "midi-map"))]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::bank2::DOOR_CELL;
use chimera_core::midi_map::{Input, Mappable, MidiMap};
use chimera_core::params::FilterParams;
use chimera_core::project::PartId;
use chimera_core::ui::settings::view::{LIST_TOP, ROW_H};
use chimera_core::ui::settings::view::{ListRow, RowLook, draw_rows};
use chimera_core::ui::theme::TextSize;
use chimera_hal::ButtonId::{B1, Edit, Minus, Mix, Plus};
use chimera_hal::EncoderId;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use screen::{Fb, Input as Key, feed, render_ui};

const P1: PartId = PartId::ALL[0];

/// The list's first row reads `label`, dimmed, and nothing else is lit
/// in the list.
fn first_row_reads(shot: &Fb, label: &str) -> bool {
    let mut want = Fb::new();
    draw_rows(&mut want, 1, usize::MAX, 0, TextSize::Normal, |_, f| {
        f(ListRow {
            label,
            opens: false,
            note: None,
            look: RowLook::Dimmed,
        })
    });
    (LIST_TOP..LIST_TOP + ROW_H).all(|y| {
        (0..240).all(|x| {
            let w = want.at(x, y);
            w == Rgb565::BLACK || shot.at(x, y) == w
        })
    })
}

#[test]
fn closed_reads_needs_update() {
    DOOR_CELL.open_host();
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut m = MidiMap::EMPTY;
    m.learn(Input::cc(74).unwrap(), Mappable::new(cutoff).unwrap())
        .unwrap();
    ui.project_mut().edit_part(P1).sound.map = m;
    feed(&mut ui, Key::press(B1));
    screen::to_leaf(&mut ui, &["PART"]);
    for _ in 0..6 {
        feed(&mut ui, Key::press(Plus));
    }
    feed(&mut ui, Key::press(Edit));
    assert!(ui.map_band().open);
    assert!(!first_row_reads(&render_ui(&ui), "NEEDS UPDATE"));

    DOOR_CELL.close();
    assert!(!ui.map_band().open);
    assert!(first_row_reads(&render_ui(&ui), "NEEDS UPDATE"));
    let rev = ui.project().rev();
    for e in [
        EncoderId::B,
        EncoderId::C,
        EncoderId::D,
        EncoderId::E,
        EncoderId::F,
    ] {
        feed(&mut ui, Key::turn(e, 1));
    }
    feed(&mut ui, Key::chord(Mix, Minus));
    assert!(!ui.prompt_open(), "nothing to delete behind a closed door");
    assert_eq!(ui.project().rev(), rev, "closed: no key edits");
    assert_eq!(ui.legend_for_test(), Some("MENU BACK"));
    assert_eq!(ui.pickup(), None);

    DOOR_CELL.open_host();
    feed(&mut ui, Key::turn(EncoderId::B, 1));
    assert_ne!(ui.project().rev(), rev, "open: B edits");
    assert!(ui.map_band().open);
}
