//! The Parts' meters through bank 2's door (ADR 0293): open, a played
//! Part reads; closed, nothing is measured (`part_meter_test` holds that
//! measuring never changes a sample).
#![cfg(feature = "bank2")]

mod common;
use common::InstRig;

use chimera_core::bank2::DOOR_CELL;
use chimera_core::hw::SampleBudget;
use chimera_core::part_peaks::{self, PART_PEAKS, Watch};

fn play() -> f32 {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.note_on(0, 48);
    rig.render(20);
    PART_PEAKS[0].take().get()
}

/// One test: the door and the meters are process-wide.
#[test]
fn the_door_measures_open_and_nothing_closed() {
    part_peaks::watch(Watch::Up);
    DOOR_CELL.open_host();
    assert!(play() > 0.0, "open: the Part reads");
    DOOR_CELL.close();
    assert_eq!(play(), 0.0, "closed: nothing measured");
    part_peaks::watch(Watch::Down);
}
