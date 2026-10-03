//! SETTINGS › MIDI CONFIG › SYNC (ADR 0080): the page edits the UI's
//! setting, the publish hands it to ORBIT, and SYSTEM keeps it.
#![cfg(feature = "orbit")]

mod screen;

use chimera_core::orbit::clock::SyncMode;
use chimera_core::orbit::runner::OrbitShared;
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::SYS_SYNC;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn published(ui: &UiState) -> SyncMode {
    let mut out = OrbitShared::default();
    ui.publish_orbit(&mut out);
    out.sync
}

#[test]
fn sync_page_edits_what_orbit_is_sent() {
    let mut ui = UiState::new();
    to_leaf(&mut ui, &["MIDI CONFIG", "SYNC"]);
    assert_eq!(ui.page_def().id, SYS_SYNC.id);
    assert_eq!(ui.midi().sync, SyncMode::Ext, "EXT out of the box");
    assert_eq!(published(&ui), SyncMode::Ext);

    feed(&mut ui, Input::turn(EncoderId::A, -1));
    assert_eq!(ui.midi().sync, SyncMode::Int);
    assert_eq!(published(&ui), SyncMode::Int);
    // Empty slots edit nothing.
    feed(&mut ui, Input::turn(EncoderId::B, 3));
    assert_eq!(ui.midi().sync, SyncMode::Int);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(published(&ui), SyncMode::Ext);

    ui.set_midi(MidiSettings {
        sync: SyncMode::Int,
    });
    assert_eq!(published(&ui), SyncMode::Int);
}

/// Leaving SETTINGS saves SYNC with the theme; a card's SYSTEM brings it.
#[test]
fn sync_is_kept_in_system() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    let mut ui = UiState::new();
    to_leaf(&mut ui, &["MIDI CONFIG", "SYNC"]);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut cur);
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    tap(&mut ui, ButtonId::Menu);
    while ui.in_settings() {
        tap(&mut ui, ButtonId::Menu);
    }
    ui.sync_system(&mut sync, &mut card, &mut s, &mut cur);
    assert_eq!(cur.midi.sync, SyncMode::Int, "the save took the UI's SYNC");

    // Booted with no card: untouched defaults, then that card goes in.
    s.eject();
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    assert_eq!(cur, SystemSettings::DEFAULT);
    s.insert();
    let mut ui = UiState::new();
    tap(&mut ui, ButtonId::Menu);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut cur);
    tap(&mut ui, ButtonId::Menu);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut cur);
    assert_eq!(ui.midi().sync, SyncMode::Int, "loaded and applied");
    assert_eq!(published(&ui), SyncMode::Int);
}

/// SYNC's value is its index in `SyncMode::ALL`, and that index names it:
/// `get`, `write` and the page's names agree for every mode.
#[test]
fn sync_value_names_and_order_agree() {
    use chimera_core::block::{Block, DiskCode, ValFmt};
    use chimera_core::ui::midi_settings::MIDI_SPECS;
    let ValFmt::Names(names) = MIDI_SPECS[0].fmt else {
        panic!("SYNC is a choice");
    };
    assert_eq!(names.len(), SyncMode::ALL.len());
    for (i, &m) in SyncMode::ALL.iter().enumerate() {
        let mut s = MidiSettings { sync: m };
        assert_eq!(s.get(MidiSettings::SYNC), i as f32, "{m:?}");
        assert_eq!(names[i], m.disk_ident(), "{m:?}");
        s.sync = SyncMode::ALL[(i + 1) % SyncMode::ALL.len()];
        s.write(MidiSettings::SYNC, i as f32);
        assert_eq!(s.sync, m);
    }
}
