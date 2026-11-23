// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! USB's Part cables (#840): each pass the UI hands USB's poll the Parts'
//! channels, so a Part cable plays on its Part's channel as it is now.
//! One test: `PART_CHANNELS` is a process-wide static.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::MidiChannel;
use chimera_core::midi_in::{CtrlEvent, PART_CHANNELS, Port};
use chimera_core::project::PartId;
use chimera_hal::Ms;
use chimera_midi::usb::unpack;

#[test]
fn a_pass_hands_usb_the_parts_channels() {
    let mut ui = Box::new(screen::stack_ui());
    let p3 = PartId::ALL[2];
    let nine = MidiChannel::new(9).unwrap();
    ui.project_mut().edit_part(p3).mix.channel = nine;
    let none = core::iter::empty::<(Port, CtrlEvent)>();
    ui.hear_ccs(none, Ms(1_000), true);
    let cable3 = unpack([0x39, 0x90, 60, 100]).unwrap().0;
    assert_eq!(PART_CHANNELS.of_cable(cable3), Some(nine));
}
