// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MPE's zones through SYSTEM with bank 2's door closed (ADR 0252): a
//! load stores each cell as saved and a save writes it back, so a boot
//! with the door closed never drops the user's zones (as #815's maps).
//! One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_core::bank2::DOOR_CELL;
use chimera_core::mpe::Zones;
use chimera_core::storage::{
    AbFile, Card, SystemCheck, SystemSettings, SystemSync, encode_system, save_ab,
};
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_hal::store::{Dir, Store};
use chimera_hal::testkit::MemStore;

fn save<S: Store>(s: &mut S, want: &SystemSettings) {
    Card::new()
        .run(s, |st, r| {
            st.make_dir(r.volume(), Dir::Chimera)?;
            save_ab(
                st,
                r,
                AbFile::SYSTEM,
                &mut SystemCheck::new(),
                None,
                &mut |w| encode_system(want, w),
            )
        })
        .and_then(|o| o.result)
        .unwrap();
}

fn boot<S: Store>(s: &mut S) -> SystemSettings {
    SystemSync::boot(&mut Card::new(), s).1
}

#[test]
fn a_closed_door_keeps_the_zones_through_load_and_save() {
    // Overlapping as stored: closed, no rule reshapes them.
    let zones = Zones::new(9, 4, 24, 96);
    let want = SystemSettings {
        midi: MidiSettings {
            mpe: zones,
            ..MidiSettings::DEFAULT
        },
        ..SystemSettings::DEFAULT
    };
    DOOR_CELL.close();
    let mut s = MemStore::new(1);
    save(&mut s, &want);
    let got = boot(&mut s);
    assert_eq!(got.midi.mpe, zones, "loaded closed");
    let mut s2 = MemStore::new(1);
    save(&mut s2, &got);
    assert_eq!(boot(&mut s2).midi.mpe, zones, "saved back closed");
    DOOR_CELL.open_host();
    assert_eq!(boot(&mut s).midi.mpe, zones, "and open");
}
