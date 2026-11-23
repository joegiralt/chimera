// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SETTINGS › AUDIO ROUTING › USB AUDIO (ADR 0260): USB IN, the step the
//! host's playback joins the main out at, 0 dB until turned.
#![cfg(feature = "usb-audio")]

mod common;
mod screen;

use chimera_core::ui::midi_settings::MidiSettings;
use chimera_hal::EncoderId;
use screen::{Input, feed};

#[test]
fn usb_in_turns_down_from_unity_and_off() {
    let mut ui = Box::new(screen::stack_ui());
    assert_eq!(ui.midi().usb_in(), 7);
    screen::to_leaf(&mut ui, &["AUDIO ROUTING", "USB AUDIO"]);
    assert_eq!(screen::page_def_id(&ui), 115);
    feed(&mut ui, Input::turn(EncoderId::A, -2));
    assert_eq!(ui.midi().usb_in(), 5, "-6 dB");
    feed(&mut ui, Input::turn(EncoderId::A, -20));
    assert_eq!(ui.midi().usb_in(), 0, "OFF");
    assert_eq!(MidiSettings::DEFAULT.usb_in(), 7);
}
