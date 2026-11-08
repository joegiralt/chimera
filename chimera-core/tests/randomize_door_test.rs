//! RANDOMIZE's roll behind bank 2's door (`UiDoor::roll`, ADR 0232), under
//! `bank2` on a host, where the table answers only while the door cell is
//! open. Closed, a turn rolls nothing and the cells read NEEDS UPDATE; open
//! again, it rolls. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::bank2::{Bank2Status, DOOR_CELL, NEEDS_UPDATE, closed_text};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::randomize::kern::roll;
use chimera_core::randomize::{Randomize, Seed};
use chimera_core::storage::sound_crc;
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, to_leaf};

const P1: PartId = PartId::ALL[0];

fn crc(ui: &UiState) -> u32 {
    sound_crc(&ui.project().part(P1).sound)
}

#[test]
fn closed_rolls_nothing_open_rolls() {
    DOOR_CELL.open_host();
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    feed(&mut ui, Input::press(ButtonId::B1));
    to_leaf(&mut ui, &["PART", "RANDOMIZE"]);
    let seed = ParamAddr::new(BlockRef::Random, Randomize::SEED);
    assert_eq!(
        closed_text(seed, 0.0, Some(Bank2Status::Empty)),
        Some(NEEDS_UPDATE)
    );

    DOOR_CELL.close();
    let (before, rev) = (crc(&ui), ui.project().rev());
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(crc(&ui), before, "closed, nothing rolls");
    assert_eq!(
        ui.project().rev(),
        rev,
        "closed, the Sound isn't even written"
    );

    // Closed, the turn never reached SEED either, so the next one open
    // lands on seed 1.
    DOOR_CELL.open_host();
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    let mut want = Sound::init(EngineType::Algo);
    let r = Randomize {
        seed: Seed::new(1),
        ..Randomize::DEFAULT
    };
    roll(&mut want, &r);
    assert_eq!(crc(&ui), sound_crc(&want), "open, it rolls seed 1");
}
