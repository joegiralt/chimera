// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI CONFIG › CONTROL's routes (ADR 0254): each pass the UI hands the
//! pipes AUTO's channel and the focused Part's, and SEQUENCER's, each only
//! where no Part's own channel takes it. One test: `ROUTES` is a
//! process-wide static.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::MidiChannel;
use chimera_core::midi_in::{CtrlEvent, Port, ROUTES};
use chimera_core::project::PartId;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_hal::{ButtonId, Ms};

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

#[test]
fn a_pass_hands_the_pipes_autos_part_and_the_sequencer() {
    let mut ui = Box::new(screen::stack_ui());
    let pass = |ui: &mut chimera_core::ui::UiState| {
        ui.hear_ccs(core::iter::empty::<(Port, CtrlEvent)>(), Ms(1_000), true)
    };
    pass(&mut ui);
    assert_eq!((ROUTES.auto(), ROUTES.seq()), (None, None), "OFF");

    let mut m = MidiSettings::DEFAULT;
    for (id, ch) in MidiSettings::CONTROL.into_iter().zip([14.0, 15.0, 16.0]) {
        chimera_core::block::Block::write(&mut m, id, ch);
    }
    ui.set_midi(m);
    pass(&mut ui);
    assert_eq!(ROUTES.auto(), Some((ch(15), ch(0))), "AUTO plays Part 1");
    assert_eq!(ROUTES.seq(), Some(ch(14)));

    // A note on SEQUENCER, through DIN's pipe: KEY is its pitch class, and
    // the note queue gets nothing to play.
    {
        use chimera_core::block::Block;
        use chimera_core::midi_in::{DinPipe, Sinks};
        use chimera_core::note_queue::InEvent;
        use chimera_core::orbit::chords::ChordRing;
        #[derive(Default)]
        struct Rec(Vec<InEvent>, Vec<CtrlEvent>);
        impl Sinks for Rec {
            fn note(&mut self, e: InEvent) -> bool {
                self.0.push(e);
                true
            }
            fn ctrl(&mut self, e: CtrlEvent) -> bool {
                self.1.push(e);
                true
            }
        }
        let mut r = Rec::default();
        DinPipe::new().feed(&[0x9E, 62, 100, 0x8E, 62, 0], &mut r);
        assert!(r.0.is_empty(), "nothing plays: {:?}", r.0);
        ui.hear_ccs(r.1.iter().map(|&e| (Port::Din, e)), Ms(1_004), true);
        let key = ui.project().orbit().live().chords.get(ChordRing::KEY);
        assert_eq!(key, 2.0, "D");
    }

    // Part 3 focused, on channel 9.
    ui.project_mut().edit_part(PartId::ALL[2]).mix.channel = ch(8);
    screen::tap(&mut ui, ButtonId::B3);
    assert_eq!(ui.active_part, PartId::ALL[2]);
    pass(&mut ui);
    assert_eq!(ROUTES.auto(), Some((ch(15), ch(8))), "AUTO plays the focus");

    // A Part on 16 and on 15: theirs, not AUTO's or SEQUENCER's.
    ui.project_mut().edit_part(PartId::ALL[3]).mix.channel = ch(15);
    ui.project_mut().edit_part(PartId::ALL[4]).mix.channel = ch(14);
    pass(&mut ui);
    assert_eq!((ROUTES.auto(), ROUTES.seq()), (None, None), "the Parts win");
}
