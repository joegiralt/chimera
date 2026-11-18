//! DRUM's voice groups behind bank 2's door (`UiDoor::groups`, ADR 0280),
//! under `bank2` on a host, where the table answers only while the door
//! cell is open. Closed, no count is shown and a CHOKE edit onto a 9th
//! group lands as a plain edit; open again, one that raises the count is
//! refused; a held CHOKE's reason is looked up through the door, the
//! kit and selected pad read bank-2 side. One test: the door is a
//! process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::bank2::DOOR_CELL;
use chimera_core::dsp::drum::params::PadParams;
use chimera_core::dsp::drum::types::{ChokeGroup, PadId};
use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::ui::held::Reason;
use chimera_core::ui::{UiState, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, load_init, tap};

const P1: PartId = PartId::ALL[0];

#[test]
fn closed_lands_open_refuses() {
    let choke = ParamAddr::new(BlockRef::Pad, PadParams::CHOKE);
    let [p3, p5, p6] = [2, 4, 5].map(|i| PadId::new(i).unwrap());
    DOOR_CELL.open_host();
    let mut ui = Box::new(UiState::new());
    load_init(&mut ui, EngineType::Drum);

    DOOR_CELL.close();
    assert!(view::groups_note(choke, ui.project().kit(P1)).is_none());
    // Pad 3 off A (shared with pad 4): a 9th group, but closed it lands.
    ui.select_pad(P1, p3);
    assert!(ui.edit_to(P1, choke, 0.0));
    assert_eq!(ui.project().kit(P1).pad(p3).choke(), ChokeGroup::Off);

    // Open: the count shows 9 over 8. Pad 6 (TOM, B with pad 7) to OFF
    // would make 10: refused, nothing written. Pad 5 (TOM, OFF) to A
    // brings it to 8: lands.
    DOOR_CELL.open_host();
    let note = view::groups_note(choke, ui.project().kit(P1)).unwrap();
    assert_eq!(note.as_str(), "GROUPS 9/8");
    ui.select_pad(P1, p6);
    let rev = ui.project().rev();
    assert!(!ui.edit_to(P1, choke, 0.0));
    assert_eq!(ui.project().kit(P1).pad(p6).choke(), ChokeGroup::B);
    assert_eq!(ui.project().rev(), rev);
    ui.select_pad(P1, p5);
    assert!(ui.edit_to(P1, choke, 1.0));
    assert_eq!(ui.project().kit(P1).pad(p5).choke(), ChokeGroup::A);

    // A held CHOKE: Part 1 on INIT DRUM's KEYS page, pad 3 (A with pad 4)
    // turned to OFF, a 9th group.
    let mut ui = Box::new(UiState::new());
    screen::stack_map(&mut ui);
    load_init(&mut ui, EngineType::Drum);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.selected_pad(P1), p3);
    tap(&mut ui, ButtonId::Edit);
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(ui.project().kit(P1).pad(p3).choke(), ChokeGroup::A);
    let why = ui.held_choice().expect("OFF held").why;
    assert!(matches!(why, Reason::Groups(g) if g.get() == 9));
    // Closed, the reason isn't looked up: nothing shows held.
    DOOR_CELL.close();
    assert!(ui.held_choice().is_none(), "the lookup is bank 2's");
    DOOR_CELL.open_host();
    assert!(ui.held_choice().is_some());
    // The selected pad is read where the reason is worked out: pad 1
    // (OFF already) selected under the hold, OFF fits it.
    ui.project_mut().select_pad(P1, PadId::new(0).unwrap());
    assert!(ui.held_choice().is_none(), "judged on the pad selected now");
}
