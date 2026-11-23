// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each Part's tuning (#217, ADR 0240): a key's offset from its Part's
//! table reaches the voice at note-on; all zero plays the untuned bits.
//! `TUNINGS` is global: every test here holds `LOCK`, and no other test
//! binary writes it.

#![cfg(feature = "tuning")]

mod common;

use std::sync::Mutex;

use chimera_core::tuning::TUNINGS;
use common::InstRig;

const SR: u32 = 48_000;
static LOCK: Mutex<()> = Mutex::new(());

fn rig() -> InstRig {
    InstRig::with_budget(chimera_core::hw::SampleBudget::for_cpu(
        chimera_core::hw::CPU_HZ_REV_V,
    ))
}

/// Part 1's note `key` (channel 1), its strongest partial in cents above
/// `hz`, after it settles.
fn played(key: u8, hz: f32) -> f64 {
    let mut rig = rig();
    rig.note_on(0, key);
    rig.render(100);
    let mut x = Vec::new();
    while x.len() < 1 << 17 {
        rig.render(1);
        x.extend(rig.out()[0].chunks(2).map(|s| s[0]));
    }
    x.truncate(1 << 17);
    1200.0 * (common::spectrum::peak_hz(&x, SR as f32, hz) / f64::from(hz)).log2()
}

fn bits(key: u8) -> Vec<u32> {
    let mut rig = rig();
    rig.note_on(0, key);
    rig.render(40);
    rig.out()[0].iter().map(|s| s.to_bits()).collect()
}

#[test]
fn a_parts_table_moves_its_key_at_note_on() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let base = played(52, 164.81);
    let mut t = [0.0; 128];
    t[52] = -0.137;
    TUNINGS.set(0, &t);
    let moved = played(52, 164.81) - base;
    TUNINGS.set(0, &[0.0; 128]);
    assert!((moved + 13.7).abs() < 3.0, "moved {moved:.1} cents");
}

#[test]
fn a_part_set_to_mts_plays_the_mts_table() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let base = played(52, 164.81);
    TUNINGS.set_mts(52, 0.25);
    TUNINGS.follow_mts(0, true);
    let moved = played(52, 164.81) - base;
    TUNINGS.follow_mts(0, false);
    TUNINGS.set_mts(52, 0.0);
    assert!((moved - 25.0).abs() < 3.0, "moved {moved:.1} cents");
}

#[test]
fn twelve_tet_is_the_untuned_bits() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let before = bits(60);
    TUNINGS.set(0, &[0.0; 128]);
    assert_eq!(before, bits(60));
    let mut t = [0.0; 128];
    t[60] = 0.01;
    TUNINGS.set(0, &t);
    let tuned = bits(60);
    TUNINGS.set(0, &[0.0; 128]);
    assert_ne!(before, tuned);
}

#[test]
fn a_silent_key_refuses_its_note() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let loud = |key| bits(key).iter().any(|&b| f32::from_bits(b) != 0.0);
    assert!(loud(61));
    TUNINGS.silence(0, 1 << 61);
    let (silent, other) = (loud(61), loud(60));
    // The MTS table has no silent keys.
    TUNINGS.follow_mts(0, true);
    let on_mts = loud(61);
    TUNINGS.follow_mts(0, false);
    TUNINGS.silence(0, 0);
    assert!(!silent, "key 61 refused");
    assert!(other && on_mts);
}

/// A refused key's note-off finds nothing: no voice, nothing held, and
/// the next key plays and releases as usual (ADR 0259).
#[test]
fn a_refused_keys_note_off_leaves_nothing_behind() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut rig = rig();
    TUNINGS.silence(0, 1 << 61);
    rig.note_on(0, 61);
    rig.render(4);
    let on = rig.inst.sounding();
    rig.note_off(0, 61);
    rig.render(4);
    let off = rig.inst.sounding();
    rig.note_on(0, 60);
    rig.render(4);
    let next = rig.inst.sounding();
    rig.note_off(0, 60);
    rig.render(2_000);
    let after = rig.inst.sounding();
    TUNINGS.silence(0, 0);
    assert_eq!((on, off), (0, 0), "the refused key never sounds");
    assert_eq!(next, 1, "the next key plays");
    assert_eq!(after, 0, "and releases: nothing stuck");
}

mod ui {
    use super::LOCK;
    use chimera_core::part::{PartParams, TUNING_NAMES, TUNING_SCALA};
    use chimera_core::project::{LoadLink, PartId};
    use chimera_core::storage::{Card, SystemSettings, SystemSync};
    use chimera_core::tuning::{ScalaName, TUNINGS};
    use chimera_core::ui::UiState;
    use chimera_core::ui::busy::ToastStep;
    use chimera_core::ui::settings::CardCx;
    use chimera_hal::store::{Dir, FileName};
    use chimera_hal::testkit::MemStore;

    struct Rig {
        ui: Box<UiState>,
        st: MemStore,
        card: Card,
        sync: SystemSync,
        set: SystemSettings,
    }

    impl Rig {
        fn new() -> Rig {
            let mut st = MemStore::new(1);
            let mut card = Card::new();
            let (sync, set, _) = SystemSync::boot(&mut card, &mut st);
            Rig {
                ui: Box::new(UiState::new()),
                st,
                card,
                sync,
                set,
            }
        }

        /// Part `p`'s TUNING to `name`, then a pass of card work.
        fn pick(&mut self, p: usize, name: &str) {
            // A name not built in is a Scala file's (ADR 0259).
            let v = TUNING_NAMES.iter().position(|&n| n == name);
            let file = v
                .is_none()
                .then(|| ScalaName::new(name.as_bytes()).unwrap());
            let v = v.map_or(TUNING_SCALA, |v| v as u8);
            self.ui.project_mut().set_scala(PartId::ALL[p], file);
            self.ui.project_mut().edit_part(PartId::ALL[p]).mix.tuning = v;
            let cx = CardCx {
                card: &mut self.card,
                store: &mut self.st,
                sync: &mut self.sync,
                settings: &mut self.set,
            };
            let link = LoadLink::new();
            let _ = self
                .ui
                .card_work(chimera_core::errand::Errand::HOST, cx, &link, |_, _| ());
        }

        /// A pass of card work, no edit.
        fn pass(&mut self) {
            let cx = CardCx {
                card: &mut self.card,
                store: &mut self.st,
                sync: &mut self.sync,
                settings: &mut self.set,
            };
            let link = LoadLink::new();
            let _ = self
                .ui
                .card_work(chimera_core::errand::Errand::HOST, cx, &link, |_, _| ());
        }

        fn toast(&mut self) -> Option<String> {
            match self.ui.step_toast(0) {
                ToastStep::Show(l) => Some(l.as_str().into()),
                _ => None,
            }
        }
    }

    fn table(p: usize) -> Vec<f32> {
        (0..128).map(|k| TUNINGS.offset(p, k)).collect()
    }

    fn file(ext: &[u8]) -> FileName {
        FileName::new(Dir::Tunings, b"TUNE2", ext).unwrap()
    }

    #[test]
    fn the_cell_is_saved_with_the_part() {
        assert_eq!(PartParams::TUNING.0, 11);
    }

    #[test]
    fn a_named_temperament_lands_and_twelve_tet_clears_it() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        r.pick(1, "JUST");
        // E, a just major third: 13.7 cents flat.
        assert!((TUNINGS.offset(1, 64) + 0.13686).abs() < 1e-4);
        assert_eq!(TUNINGS.offset(1, 60), 0.0);
        assert!(table(0).iter().all(|&o| o == 0.0), "Part 1 untouched");
        r.pick(1, "12-TET");
        assert!(table(1).iter().all(|&o| o == 0.0));
    }

    #[test]
    fn mts_follows_the_mts_table() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        TUNINGS.set_mts(70, 0.3);
        r.pick(2, "MTS");
        assert_eq!(TUNINGS.offset(2, 70), 0.3);
        r.pick(2, "12-TET");
        assert_eq!(TUNINGS.offset(2, 70), 0.0);
        TUNINGS.set_mts(70, 0.0);
    }

    #[test]
    fn a_card_file_with_its_map_lands() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        r.st.plant(file(b"SCL"), b"! q\nquarter tones\n24\n50.\n100.\n150.\n200.\n250.\n300.\n350.\n400.\n450.\n500.\n550.\n600.\n650.\n700.\n750.\n800.\n850.\n900.\n950.\n1000.\n1050.\n1100.\n1150.\n2/1\n");
        r.pick(3, "TUNE2");
        // Every key a quarter tone on from middle C.
        assert_eq!(TUNINGS.offset(3, 61), -0.5);
        assert_eq!(TUNINGS.offset(3, 62), -1.0);
        assert_eq!(r.toast(), None);
        // With a map: A4 at 432 Hz, keys in turn: middle C 4.5 semitones
        // under it.
        r.st.plant(file(b"KBM"), b"0\n0\n127\n60\n69\n432.0\n24\n");
        r.pick(3, "12-TET");
        r.pick(3, "TUNE2");
        let a = 12.0 * (432.0f32 / 440.0).log2();
        assert!((TUNINGS.offset(3, 69) - a).abs() < 1e-4);
        assert!((TUNINGS.offset(3, 60) - (69.0 + a - 4.5 - 60.0)).abs() < 1e-4);
        r.pick(3, "12-TET");
    }

    #[test]
    fn a_missing_or_bad_file_plays_twelve_tet_and_says_why() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        r.pick(4, "TUNE2");
        assert_eq!(r.toast().as_deref(), Some("TUNE2.SCL NOT FOUND"));
        assert!(table(4).iter().all(|&o| o == 0.0));
        r.st.plant(file(b"SCL"), b"desc\n3\n100.0\n");
        r.pick(4, "12-TET");
        r.pick(4, "TUNE2");
        assert_eq!(r.toast().as_deref(), Some("TUNE2.SCL NOT SCALA"));
        r.st.plant(file(b"SCL"), &[b'!'; 5000]);
        r.pick(4, "12-TET");
        r.pick(4, "TUNE2");
        assert_eq!(r.toast().as_deref(), Some("TUNE2.SCL TOO BIG"));
        assert!(table(4).iter().all(|&o| o == 0.0));
        r.pick(4, "12-TET");
    }

    #[test]
    fn a_file_put_on_the_card_is_read_again() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        r.pick(5, "TUNE2");
        assert_eq!(r.toast().as_deref(), Some("TUNE2.SCL NOT FOUND"));
        // The console's `put` writes it: the Part on TUNE2 reads it.
        r.st.plant(file(b"SCL"), b"q\n2\n50.\n2/1\n");
        chimera_core::tuning::file_put();
        r.pick(5, "TUNE2");
        assert_eq!(TUNINGS.offset(5, 61), -0.5);
        r.pick(5, "12-TET");
    }

    #[test]
    fn a_file_removed_from_the_card_plays_twelve_tet() {
        use chimera_hal::store::Store;
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        r.st.plant(file(b"SCL"), b"q\n2\n50.\n2/1\n");
        r.pick(5, "TUNE2");
        assert_eq!(TUNINGS.offset(5, 61), -0.5);
        // The console's `rm` deletes it (#906): the Part on TUNE2 reads
        // again, finds nothing and plays 12-TET.
        let vol = r.st.mount().unwrap();
        r.st.delete(vol, file(b"SCL")).unwrap();
        chimera_core::tuning::file_put();
        r.pick(5, "TUNE2");
        assert_eq!(r.toast().as_deref(), Some("TUNE2.SCL NOT FOUND"));
        assert!(table(5).iter().all(|&o| o == 0.0));
        r.pick(5, "12-TET");
    }

    /// A Part on a file the card lacks plays 12-TET; once the console puts
    /// the file, the next pass plays it with no edit (ADR 0259).
    #[test]
    fn a_missing_file_plays_once_put_with_no_edit() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut r = Rig::new();
        r.pick(3, "PELOG");
        assert_eq!(r.toast().as_deref(), Some("PELOG.SCL NOT FOUND"));
        assert!(table(3).iter().all(|&o| o == 0.0));
        let pelog = FileName::new(Dir::Tunings, b"PELOG", b"SCL").unwrap();
        r.st.plant(pelog, b"q\n2\n50.\n2/1\n");
        r.pass();
        assert!(
            table(3).iter().all(|&o| o == 0.0),
            "not until a put says so"
        );
        chimera_core::tuning::file_put();
        r.pass();
        assert_eq!(TUNINGS.offset(3, 61), -0.5);
        r.pick(3, "12-TET");
    }
}
