// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MAX VOICES and RESERVE behind bank 2's door (`UiDoor::voices`, ADR
//! 0281), under `bank2` on a host, where the table answers only while the
//! door cell is open. Closed, both cells say the rule is off and an edit
//! past it lands as a plain edit, nothing held; open again, the RESERVE
//! note counts the saved excess and only an edit that brings it down
//! lands. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_core::bank2::DOOR_CELL;
use chimera_core::project::PartId;
use chimera_core::ui::{UiState, held, view};
use chimera_core::voice_limit::{MaxVoices, Reserve, VoiceCap, VoiceCount};

fn reserve(n: u8) -> Reserve {
    Reserve::new(VoiceCount::new(n).unwrap())
}

#[test]
fn closed_lands_open_refuses() {
    let [p1, p2] = [PartId::ALL[0], PartId::ALL[1]];
    DOOR_CELL.open_host();
    let mut ui = Box::new(UiState::new());
    assert!(ui.edit_to(p2, held::RESERVE, 8.0));

    DOOR_CELL.close();
    for cell in [held::RESERVE, held::MAX_VOICES] {
        let note = view::voices_note(cell, ui.project().perf().parts()).expect("the rule is off");
        assert_eq!(note.as_str(), view::RULE_OFF);
    }
    assert!(ui.edit_to(p1, held::RESERVE, 2.0), "closed: a plain edit");
    assert_eq!(ui.project().part(p1).mix.voices().reserve, reserve(2));
    // A cap under it lands too, unchecked.
    assert!(ui.edit_to(p1, held::MAX_VOICES, 1.0));
    let cap1 = MaxVoices::Cap(VoiceCap::new(1).unwrap());
    assert_eq!(ui.project().part(p1).mix.voices().max, cap1);
    assert!(ui.held_choice().is_none(), "nothing held closed");

    // Open: 10 booked shows; up is refused, down lands.
    DOOR_CELL.open_host();
    let note = view::voices_note(held::RESERVE, ui.project().perf().parts()).unwrap();
    assert_eq!(note.as_str(), "RESERVES 10/8");
    assert!(view::voices_note(held::MAX_VOICES, ui.project().perf().parts()).is_none());
    let rev = ui.project().rev();
    assert!(!ui.edit_to(p1, held::RESERVE, 3.0));
    assert_eq!(ui.project().rev(), rev);
    assert!(ui.edit_to(p1, held::RESERVE, 1.0));
    assert_eq!(ui.project().part(p1).mix.voices().reserve, reserve(1));
}
