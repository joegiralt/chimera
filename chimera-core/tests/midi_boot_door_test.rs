//! A staged bank 2 installed at boot (spec § 7.3). NEW built while the
//! door was closed decodes empty maps and takes its template so: open
//! later, the boot's NEW reads Modified. So the board builds the UI, and
//! NEW, after the install: that NEW boots with the default CC chart,
//! every Part Clean, the project Pristine. One test: the door is a
//! process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support", feature = "midi-map"))]

mod common;
mod screen;

use chimera_core::bank2::{Bank2Status, DOOR_CELL};
use chimera_core::midi_map::{MidiMap, default_for};
use chimera_core::project::{PartId, PartStatus, ProjectStatus, part_status};
use chimera_core::storage::Card;
use chimera_hal::testkit::MemStore;

#[test]
fn new_is_built_after_the_install_opens() {
    DOOR_CELL.close();
    let mut early = Box::new(screen::stack_ui());
    assert_eq!(
        early.project().part(PartId::ALL[0]).sound.map,
        MidiMap::EMPTY
    );

    // The staged install opens the door.
    DOOR_CELL.open_host();
    early.set_bank2(Bank2Status::Empty);
    early.boot_project(&mut Card::new(), &mut MemStore::new(1), None);
    assert_eq!(
        early.project_status(),
        ProjectStatus::Modified,
        "NEW built before the install: its template is stale"
    );

    let mut ui = Box::new(screen::stack_ui());
    ui.set_bank2(Bank2Status::Empty);
    ui.boot_project(&mut Card::new(), &mut MemStore::new(1), None);
    for id in PartId::ALL {
        let p = ui.project().part(id);
        // A factory Sound with an LFO 1 route keeps CC 1 (ADR 0250).
        let e = p.sound.params.engine();
        let charts = [default_for(e), chimera_core::midi_map::wheel_map_for(e)];
        assert!(charts.contains(&p.sound.map), "{id:?}: the default chart");
        assert_eq!(
            part_status(p, ui.project().pool()),
            PartStatus::Clean,
            "{id:?}"
        );
    }
    assert_eq!(ui.project_status(), ProjectStatus::Pristine);
}
