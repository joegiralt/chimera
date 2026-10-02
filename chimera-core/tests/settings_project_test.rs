//! SETTINGS › PROJECT against `MemStore` (settings spec § The PROJECT
//! branch): LOAD, SAVE AS, quick save, CARD CHANGED, and `card_work`.

mod screen;

use chimera_core::name::ProjectName;
use chimera_core::project::test_support::{FullOnWrite, confirm_delete, confirm_overwrite, damage};
use chimera_core::project::{
    CardOut, LoadLink, Project, ProjectEntry, ProjectFile, ProjectNote, ProjectSource,
    ProjectStatus, ReplaceGuard, SaveTo, clear_project, delete_project, list_projects,
    load_project, new_project_id, project_crc, save_project,
};
use chimera_core::storage::{Card, CardEvent, Generation, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::nav::Location;
use chimera_core::ui::settings::CardCx;
use chimera_core::ui::settings::listing::{DAMAGED, LOADED};
use chimera_core::ui::settings::view::RowLook;
use chimera_hal::store::{Store, VolumeId};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, hold, tap};

struct Rig<S: Store = MemStore> {
    ui: Box<UiState>,
    s: S,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
    /// Loads published.
    swaps: usize,
}

impl<S: Store> Rig<S> {
    fn new(s: S) -> Self {
        let mut s = s;
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        Rig {
            ui: Box::new(UiState::new()),
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
            swaps: 0,
        }
    }

    fn work(&mut self) {
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.s,
            sync: &mut self.sync,
            settings: &mut self.set,
        };
        let r = self.ui.card_work(cx, link, |swap, _| {
            let _ = swap.settle(link, || false);
        });
        self.swaps += r.is_some() as usize;
    }

    fn tap(&mut self, b: ButtonId) {
        tap(&mut self.ui, b);
        self.work();
    }

    fn hold(&mut self, b: ButtonId) {
        hold(&mut self.ui, b);
        self.work();
    }

    fn feed(&mut self, i: Input) {
        feed(&mut self.ui, i);
        self.work();
    }

    fn edit(&mut self) {
        self.feed(Input::press(ButtonId::Edit));
    }

    fn turn(&mut self, d: i8) {
        self.feed(Input::turn(EncoderId::A, d));
    }

    fn toast(&mut self) -> String {
        match self.ui.step_toast(0) {
            ToastStep::Show(t) => t.as_str().to_string(),
            _ => String::new(),
        }
    }

    /// SETTINGS › PROJECT › LOAD, from anywhere outside SETTINGS.
    fn open_load(&mut self) {
        self.tap(ButtonId::Menu);
        self.edit();
        self.edit();
        assert_eq!(self.ui.location(), Location::settings_at(&[0, 0], 0));
    }

    /// LOAD's row labelled `prefix`.
    fn row_of(&self, prefix: &str) -> usize {
        (0..self.ui.listing().load_rows())
            .find(|&i| {
                self.ui
                    .load_row(i)
                    .unwrap()
                    .label
                    .as_str()
                    .starts_with(prefix)
            })
            .unwrap_or_else(|| panic!("no row {prefix}: {:?}", self.labels()))
    }

    /// The bar to row `i` of a Screen.
    fn bar_to(&mut self, i: usize) {
        let row = |r: &Rig<S>| r.ui.location().settings().unwrap().row() as usize;
        while row(self) != i {
            let k = if row(self) < i {
                ButtonId::Plus
            } else {
                ButtonId::Minus
            };
            self.feed(Input::press(k));
        }
    }

    fn labels(&self) -> Vec<String> {
        (0..self.ui.listing().load_rows())
            .map(|i| self.ui.load_row(i).unwrap().label.as_str().to_string())
            .collect()
    }

    fn vol(&mut self) -> VolumeId {
        self.s.mount().unwrap()
    }

    fn question(&self) -> String {
        let (q, _) = self.ui.prompt_words_for_test().expect("a prompt");
        q.as_str().to_string()
    }
}

fn name(s: &str) -> ProjectName {
    ProjectName::new(s).unwrap()
}

/// A project named `n` saved to the card in the slot, at its next id.
fn put(s: &mut MemStore, n: &str) -> ProjectFile {
    let mut card = Card::new();
    let (mut p, _) = Project::boxed();
    p.set_name(name(n));
    let fresh = new_project_id(&mut card, s).out.unwrap();
    let f = fresh.file();
    assert!(matches!(
        save_project(&mut card, s, &mut p, SaveTo::Fresh(fresh)).out,
        ProjectNote::Saved(_)
    ));
    f
}

fn listed(s: &mut MemStore) -> Vec<ProjectEntry> {
    let mut v = Vec::new();
    let _ = list_projects(&mut Card::new(), s, &mut |e| v.push(e));
    v
}

fn generation(s: &mut MemStore, f: ProjectFile) -> Option<Generation> {
    listed(s).into_iter().find(|e| e.id == f.id())?.generation
}

/// The project `f` holds, read back.
fn read_back(s: &mut MemStore, f: ProjectFile) -> Box<Project> {
    let (mut p, t) = Project::boxed();
    let go = ReplaceGuard::check(
        &p,
        t,
        ProjectSource::File {
            id: f.id(),
            vol: f.vol(),
        },
    )
    .unwrap();
    let out = load_project(&mut Card::new(), s, &mut p, go, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    let _ = out.swap;
    p
}

/// A rig with `f` loaded through LOAD, then outside SETTINGS.
fn loaded<S: Store>(r: &mut Rig<S>, f: ProjectFile) {
    r.open_load();
    let i = r.row_of(&format!("{:02}", f.id().get()));
    r.bar_to(i);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.project().meta().file(), Some(f));
    while r.ui.in_settings() {
        r.tap(ButtonId::Menu);
    }
}

/// An edit that leaves the project Modified.
fn modify<S: Store>(r: &mut Rig<S>, n: &str) {
    r.ui.project_mut().set_name(name(n));
    r.ui.update(UiTick::for_test());
    assert_eq!(r.ui.project_status(), ProjectStatus::Modified);
}

#[test]
fn load_lists_marks_the_loaded_and_greys_the_damaged() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let b = put(&mut s, "BETA");
    damage(&mut Card::new(), &mut s, b.id());
    let mut r = Rig::new(s);
    loaded(&mut r, a);
    r.open_load();
    assert_eq!(r.labels(), ["01 ALPHA", "02", "+ CREATE NEW"]);
    let row = |r: &Rig, i| r.ui.load_row(i).unwrap();
    assert_eq!(row(&r, 0).note, Some(LOADED));
    assert_eq!(row(&r, 0).look, RowLook::Normal);
    assert_eq!(row(&r, 1).note, Some(DAMAGED));
    assert_eq!(row(&r, 1).look, RowLook::Dimmed);
    // SEQ on it says why, and loads nothing.
    r.bar_to(1);
    r.tap(ButtonId::Seq);
    assert_eq!(r.toast(), "FILE IS CUT SHORT: P0000002");
    assert_eq!(r.ui.project().meta().file(), Some(a));
    assert_eq!(r.swaps, 1);
}

#[test]
fn no_card_lists_only_no_card() {
    let mut s = MemStore::new(1);
    put(&mut s, "ALPHA");
    s.eject();
    let mut r = Rig::new(s);
    r.open_load();
    assert_eq!(r.labels(), ["NO CARD"]);
    assert_eq!(r.ui.load_row(0).unwrap().look, RowLook::Dimmed);
    r.tap(ButtonId::Seq);
    assert_eq!(r.swaps, 0);
    assert!(!r.ui.prompt_open());
}

#[test]
fn load_over_modified_asks_and_each_answer() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let b = put(&mut s, "BETA");
    let mut r = Rig::new(s);
    loaded(&mut r, a);

    // CANCEL: nothing moves.
    modify(&mut r, "EDITED");
    let crc = project_crc(r.ui.project());
    r.open_load();
    r.bar_to(r.row_of("02"));
    r.tap(ButtonId::Seq);
    assert!(r.ui.prompt_open());
    assert_eq!(r.question(), "LOAD BETA?");
    r.tap(ButtonId::Menu);
    assert!(!r.ui.prompt_open());
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.swaps, 1);

    // LOAD ANYWAY: BETA, Saved.
    r.tap(ButtonId::Seq);
    r.turn(1);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.project().meta().file(), Some(b));
    r.ui.update(UiTick::for_test());
    assert_eq!(r.ui.project_status(), ProjectStatus::Saved);
    assert_eq!(r.swaps, 2);

    // SAVE THEN LOAD: BETA's edit to its own file, then ALPHA.
    modify(&mut r, "BETA2");
    let edited = project_crc(r.ui.project());
    r.bar_to(r.row_of("01"));
    r.tap(ButtonId::Seq);
    assert!(r.ui.prompt_open());
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.project().meta().file(), Some(a));
    assert_eq!(r.swaps, 3);
    assert_eq!(project_crc(&read_back(&mut r.s, b)), edited);
    assert_eq!(r.labels(), ["01 ALPHA", "02 BETA2", "+ CREATE NEW"]);
}

#[test]
fn save_then_load_on_new_names_first_and_cancel_aborts() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    modify(&mut r, "SKETCH");
    let crc = project_crc(r.ui.project());
    r.open_load();
    r.tap(ButtonId::Seq);
    assert!(r.ui.prompt_open());
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "DRIFT-002");
    // Cancelled: no save, no load.
    r.tap(ButtonId::Menu);
    assert!(r.ui.naming().is_none());
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.swaps, 0);
    assert_eq!(listed(&mut r.s).len(), 1);
    assert_eq!(r.ui.location(), Location::settings_at(&[0, 0], 0));

    // Named: saved as id 2, then ALPHA loads.
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.project().meta().file(), Some(a));
    assert_eq!(r.swaps, 1);
    let all = listed(&mut r.s);
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].name, Some(name("DRIFT-002")));
}

#[test]
fn save_as_names_and_saves() {
    let mut r = Rig::new(MemStore::new(1));
    r.tap(ButtonId::Menu);
    r.edit();
    r.feed(Input::press(ButtonId::Plus));
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "ACID-001");
    r.tap(ButtonId::Seq);
    assert_eq!(r.toast(), "SAVED");
    r.ui.update(UiTick::for_test());
    assert_eq!(r.ui.project_status(), ProjectStatus::Saved);
    assert_eq!(r.ui.project().meta().name(), name("ACID-001"));
    r.tap(ButtonId::Menu);
    r.tap(ButtonId::Menu);
    r.open_load();
    assert_eq!(r.labels(), ["01 ACID-001", "+ CREATE NEW"]);
    assert_eq!(r.ui.load_row(0).unwrap().note, Some(LOADED));
}

/// SAVE AS where the card already has `drift-002`, the name proposed.
fn name_taken() -> (Rig, ProjectFile) {
    let mut s = MemStore::new(1);
    let taken = put(&mut s, "drift-002");
    let mut r = Rig::new(s);
    modify(&mut r, "SKETCH");
    r.tap(ButtonId::Menu);
    r.edit();
    r.feed(Input::press(ButtonId::Plus));
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "NAME EXISTS");
    (r, taken)
}

#[test]
fn name_exists_keep_both_and_overwrite() {
    let (mut r, _) = name_taken();
    r.tap(ButtonId::Seq);
    let all = listed(&mut r.s);
    assert_eq!(all.len(), 2, "KEEP BOTH");
    assert_eq!(all[1].name, Some(name("DRIFT-002")));

    let (mut r, taken) = name_taken();
    r.turn(1);
    r.tap(ButtonId::Seq);
    let all = listed(&mut r.s);
    assert_eq!(all.len(), 1, "OVERWRITE");
    assert_eq!(r.ui.project().meta().file(), Some(taken));
    let crc = project_crc(r.ui.project());
    assert_eq!(project_crc(&read_back(&mut r.s, taken)), crc);
    assert_eq!(all[0].name, Some(name("DRIFT-002")));
}

#[test]
fn quick_save_saves_over_own_file() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    loaded(&mut r, a);
    modify(&mut r, "ALPHA2");
    let at = r.ui.location();
    let was = generation(&mut r.s, a);
    r.hold(ButtonId::Menu);
    assert_eq!(r.toast(), "SAVED");
    assert_eq!(r.ui.location(), at);
    r.ui.update(UiTick::for_test());
    assert_eq!(r.ui.project_status(), ProjectStatus::Saved);
    assert_ne!(generation(&mut r.s, a), was);
}

#[test]
fn quick_save_on_new_opens_save_as() {
    let mut r = Rig::new(MemStore::new(1));
    let home = r.ui.location();
    r.hold(ButtonId::Menu);
    assert_ne!(r.ui.location(), home);
    assert_eq!(r.ui.location(), Location::settings_at(&[0], 1));
    assert_eq!(r.ui.naming().unwrap().text(), "ACID-001");
    // Cancelled, it stays on the list.
    r.tap(ButtonId::Menu);
    assert_eq!(r.ui.location(), Location::settings_at(&[0], 1));
    assert!(listed(&mut r.s).is_empty());
}

/// A file saved under NAMING takes its id: the save goes to the id the
/// re-list gives, not the one NAMING opened with.
#[test]
fn save_as_takes_the_id_free_when_named() {
    let mut r = Rig::new(MemStore::new(1));
    r.hold(ButtonId::Menu);
    assert_eq!(r.ui.naming().unwrap().text(), "ACID-001");
    let taken = put(&mut r.s, "BETA");
    r.tap(ButtonId::Seq);
    assert_eq!(r.toast(), "SAVED");
    let f = r.ui.project().meta().file().unwrap();
    assert_eq!(f.id().get(), taken.id().get() + 1);
    assert_eq!(listed(&mut r.s).len(), 2);
}

#[test]
fn quick_save_stalled_release_saves_once() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    loaded(&mut r, a);
    let was = generation(&mut r.s, a).unwrap();
    r.feed(Input::press(ButtonId::Menu).at(0));
    r.feed(Input::released_at(ButtonId::Menu, 900));
    r.feed(Input::default().at(933));
    assert_eq!(generation(&mut r.s, a), Some(was.next()));
    assert!(!r.ui.in_settings());
}

#[test]
fn menu_hold_inside_a_prompt_does_nothing() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    put(&mut s, "BETA");
    let mut r = Rig::new(s);
    loaded(&mut r, a);
    modify(&mut r, "EDITED");
    let crc = project_crc(r.ui.project());
    let was = generation(&mut r.s, a);
    r.open_load();
    r.bar_to(r.row_of("02"));
    r.tap(ButtonId::Seq);
    assert!(r.ui.prompt_open());
    r.hold(ButtonId::Menu);
    assert!(r.ui.prompt_open());
    assert_eq!(generation(&mut r.s, a), was);
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.swaps, 1);
}

#[test]
fn card_changed_then_save_as() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    loaded(&mut r, a);
    r.s.swap(2);
    r.hold(ButtonId::Menu);
    assert_eq!(r.question(), "CARD CHANGED");
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "ACID-001");
    r.tap(ButtonId::Seq);
    let vol = r.vol();
    let file = r.ui.project().meta().file().unwrap();
    assert_eq!(file.vol(), vol);
    assert_eq!(listed(&mut r.s).len(), 1);
    assert_eq!(r.toast(), "SAVED");
}

#[test]
fn swap_while_listed_relists_and_refuses() {
    let mut s = MemStore::new(1);
    put(&mut s, "ALPHA");
    put(&mut s, "BETA");
    let mut r = Rig::new(s);
    r.open_load();
    assert_eq!(r.labels(), ["01 ALPHA", "02 BETA", "+ CREATE NEW"]);
    let crc = project_crc(r.ui.project());
    r.s.swap(2);
    put(&mut r.s, "GAMMA");
    r.tap(ButtonId::Seq);
    assert!(r.toast().starts_with("CARD CHANGED"));
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.swaps, 0);
    assert_eq!(r.labels(), ["01 GAMMA", "+ CREATE NEW"]);
}

#[test]
fn relist_after_every_operation() {
    let mut r = Rig::new(MemStore::new(1));
    modify(&mut r, "SKETCH");
    r.open_load();
    assert_eq!(r.labels(), ["+ CREATE NEW"]);
    // + CREATE NEW over a Modified NEW: SAVE THEN LOAD names it first.
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.location(), Location::settings_at(&[0, 0], 0));
    assert_eq!(r.labels(), ["01 ACID-001", "+ CREATE NEW"]);
}

#[test]
fn every_card_op_returns_its_event() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let fresh = new_project_id(&mut card, &mut s);
    assert_eq!(fresh.event, Some(CardEvent::Mounted));
    let (mut p, _) = Project::boxed();
    let (mut other, _) = Project::boxed();
    let out = save_project(&mut card, &mut s, &mut p, SaveTo::Fresh(fresh.out.unwrap()));
    assert_eq!(out.event, Some(CardEvent::Same));
    let one = put(&mut s, "ONE");
    let mut on_one = listed(&mut s);
    let e1 = on_one.pop().unwrap();
    assert_eq!(e1.file(), one);
    let swapped = |e: Option<CardEvent>| matches!(e, Some(CardEvent::Swapped { .. }));

    s.swap(2);
    let out = save_project(&mut card, &mut s, &mut p, SaveTo::Own);
    assert!(swapped(out.event), "{out:?}");
    assert!(matches!(out.out, ProjectNote::Card { .. }));
    let again = save_project(&mut card, &mut s, &mut p, SaveTo::Own);
    assert_eq!(again.event, Some(CardEvent::Same));

    s.swap(3);
    let out = delete_project(&mut card, &mut s, &other, confirm_delete(&e1));
    assert!(swapped(out.event), "{out:?}");
    let out = delete_project(&mut card, &mut s, &other, confirm_delete(&e1));
    assert_eq!(out.event, Some(CardEvent::Same));
    // Refused before any mount: no event.
    let mine = save_project(&mut card, &mut s, &mut other, SaveTo::Own);
    assert_eq!(mine.event, None);

    s.swap(4);
    let out = clear_project(&mut card, &mut s, &p, confirm_overwrite(&e1));
    assert!(swapped(out.event), "{out:?}");
    let CardOut { event, .. } = clear_project(&mut card, &mut s, &p, confirm_overwrite(&e1));
    assert_eq!(event, Some(CardEvent::Same));
}

#[test]
fn edit_on_load_lists_then_enters() {
    let mut s = MemStore::new(1);
    put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    r.tap(ButtonId::Menu);
    r.edit();
    feed(&mut r.ui, Input::press(ButtonId::Edit));
    assert_eq!(r.ui.location(), Location::settings_at(&[0], 0));
    assert!(r.ui.card_pending());
    r.work();
    assert!(!r.ui.card_pending());
    assert_eq!(r.ui.location(), Location::settings_at(&[0, 0], 0));
    assert_eq!(r.labels(), ["01 ALPHA", "+ CREATE NEW"]);
}

/// SAVE THEN LOAD with another card in the slot: the save is refused, the
/// load waits behind CARD CHANGED, and nothing moves.
#[test]
fn save_then_load_refused_save_keeps_the_project() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    put(&mut s, "BETA");
    let mut r = Rig::new(s);
    loaded(&mut r, a);
    modify(&mut r, "EDITED");
    let crc = project_crc(r.ui.project());
    r.open_load();
    r.bar_to(r.row_of("02"));
    r.tap(ButtonId::Seq);
    r.s.swap(2);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "CARD CHANGED");
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.swaps, 1);
    assert_eq!(r.ui.project().meta().file(), Some(a));

    // SAVE AS on the new card; BETA was listed on the old one, so the
    // load is still refused.
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "ACID-001");
    r.tap(ButtonId::Seq);
    let vol = r.vol();
    assert_eq!(r.ui.project().meta().file().map(|f| f.vol()), Some(vol));
    assert_eq!(r.ui.project().meta().name(), name("ACID-001"));
    assert_eq!(r.swaps, 1);
    assert!(r.toast().starts_with("CARD CHANGED"));
}

/// The same on NEW, the card swapped while NAMING was open: refused, the
/// name not taken.
#[test]
fn save_then_load_on_new_refused_keeps_the_name() {
    let mut s = MemStore::new(1);
    put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    modify(&mut r, "SKETCH");
    let crc = project_crc(r.ui.project());
    r.open_load();
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "DRIFT-002");
    r.s.swap(2);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "CARD CHANGED");
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.ui.project().meta().name(), name("SKETCH"));
    assert_eq!(r.ui.project().meta().file(), None);
    assert_eq!(r.swaps, 0);
    assert!(listed(&mut r.s).is_empty());
}

/// 50 projects: 48 rows, MORE ON CARD, and SAVE AS takes id 51 from the
/// same pass; NAME EXISTS finds a name past the rows.
#[test]
fn a_full_card_lists_48_and_names_past_them() {
    let mut s = MemStore::new(1);
    for i in 1..=50 {
        put(&mut s, if i == 50 { "pulse-051" } else { "X" });
    }
    let mut r = Rig::new(s);
    r.open_load();
    assert_eq!(r.ui.listing().len(), 48);
    let labels = r.labels();
    assert_eq!(labels.len(), 50);
    assert_eq!(labels[47], "48 X");
    assert_eq!(labels[48], "MORE ON CARD");
    assert_eq!(labels[49], "+ CREATE NEW");
    r.tap(ButtonId::Menu);
    r.feed(Input::press(ButtonId::Plus));
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "PULSE-051");
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "NAME EXISTS");
}

#[test]
fn seq_on_no_card_reads_the_card_again() {
    let mut s = MemStore::new(1);
    put(&mut s, "ALPHA");
    s.eject();
    let mut r = Rig::new(s);
    r.open_load();
    assert_eq!(r.labels(), ["NO CARD"]);
    r.s.insert();
    r.tap(ButtonId::Seq);
    assert_eq!(r.labels(), ["01 ALPHA", "+ CREATE NEW"]);
}

/// A swap to a card with fewer projects keeps the bar on the rows.
#[test]
fn a_shorter_card_clamps_the_bar() {
    let mut s = MemStore::new(1);
    for n in ["A", "B", "C"] {
        put(&mut s, n);
    }
    let mut r = Rig::new(s);
    r.open_load();
    r.bar_to(2);
    r.s.swap(2);
    r.tap(ButtonId::Seq);
    assert_eq!(r.labels(), ["+ CREATE NEW"]);
    assert_eq!(r.ui.location(), Location::settings_at(&[0, 0], 0));
}

/// A save that fails on the card in the slot runs no load after it.
#[test]
fn save_then_load_failed_save_loads_nothing() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    put(&mut s, "BETA");
    let mut r = Rig::new(FullOnWrite(s, false));
    loaded(&mut r, a);
    modify(&mut r, "EDITED");
    let crc = project_crc(r.ui.project());
    r.open_load();
    r.bar_to(r.row_of("02"));
    r.tap(ButtonId::Seq);
    r.s.1 = true;
    r.tap(ButtonId::Seq);
    assert!(r.toast().starts_with("CARD FULL"));
    assert!(!r.ui.prompt_open());
    assert_eq!(r.swaps, 1);
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.ui.project().meta().file(), Some(a));
}

/// The card gone while NAMING was open: SAVE AS stops on it, and the load
/// it was inside goes with it.
#[test]
fn a_card_error_after_naming_drops_the_save_and_its_load() {
    let mut s = MemStore::new(1);
    put(&mut s, "ALPHA");
    let mut r = Rig::new(s);
    modify(&mut r, "SKETCH");
    let crc = project_crc(r.ui.project());
    r.open_load();
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert!(r.ui.naming().is_some());
    r.s.eject();
    r.tap(ButtonId::Seq);
    assert_eq!(r.toast(), "NO CARD");
    assert!(!r.ui.prompt_open());
    assert!(r.ui.naming().is_none());
    r.s.insert();
    r.work();
    assert_eq!(r.swaps, 0);
    assert_eq!(project_crc(r.ui.project()), crc);
    assert_eq!(r.ui.project().meta().name(), name("SKETCH"));
    assert_eq!(listed(&mut r.s).len(), 1);
}
