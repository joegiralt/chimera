//! #515, review of #891: a build without the MIDI map (the bench's) hears
//! no program change: the door's `hear` is `deaf`, so PROGRAM IN on, a
//! change still loads nothing and the pipes are never told to hold.

#![cfg(not(feature = "midi-map"))]

use std::sync::atomic::Ordering;

use chimera_core::midi_in::{CtrlEvent, PROGRAM_IN, Port};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, SlotId};
use chimera_core::ui::UiState;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_hal::Ms;
use chimera_midi::ctl::Ctl;
use chimera_midi::value::U7;

#[test]
fn no_map_no_program_change() {
    let mut ui = Box::new(UiState::new());
    ui.project_mut()
        .pool_store(SlotId::ALL[0], &Sound::init(EngineType::Va))
        .unwrap();
    ui.set_midi(MidiSettings::DEFAULT.with_program_in(true));
    let p = PartId::ALL[0];
    let (origin, loads) = (ui.project().part(p).origin(), ui.project().part(p).loads());
    let e = CtrlEvent::Ctl(
        ui.project().part(p).mix.channel,
        Ctl::Program {
            bank: None,
            program: U7::new(0).unwrap(),
        },
    );
    for _ in 0..3 {
        ui.hear_ccs([(Port::Din, e)].into_iter(), Ms(1000), true);
    }
    assert_eq!(ui.project().part(p).origin(), origin);
    assert_eq!(ui.project().part(p).loads(), loads);
    assert!(!PROGRAM_IN.load(Ordering::Relaxed), "the pipes never hold");
}
