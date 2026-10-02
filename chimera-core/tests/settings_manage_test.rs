//! SETTINGS › PROJECT › MANAGE PROJECTS against `MemStore` (settings spec
//! § MANAGE PROJECTS): each command, behind its prompt.

mod screen;

use chimera_core::block::Block;
use chimera_core::name::ProjectName;
use chimera_core::part::PartParams;
use chimera_core::project::{
    LoadLink, Project, ProjectEntry, ProjectFile, ProjectNote, ProjectSource, ProjectStatus,
    ReplaceGuard, SaveTo, list_projects, load_project, new_project_id, project_crc, save_project,
};
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::nav::{Column, Location};
use chimera_core::ui::settings::manage::{Command, Note, Off, Whose};
use chimera_core::ui::settings::view::RowLook;
use chimera_core::ui::settings::{CardCx, Screen, screen_path};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, tap};

struct Rig {
    ui: Box<UiState>,
    s: MemStore,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl Rig {
    fn new(mut s: MemStore) -> Self {
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        Rig {
            ui: Box::new(UiState::new()),
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
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
        let _ = self.ui.card_work(cx, link, |swap, _| {
            let _ = swap.settle(link, || false);
        });
    }

    fn tap(&mut self, b: ButtonId) {
        tap(&mut self.ui, b);
        self.work();
    }

    fn feed(&mut self, i: Input) {
        feed(&mut self.ui, i);
        self.work();
    }

    fn column(&self) -> Option<Column> {
        self.ui.location().settings().and_then(|s| s.column())
    }

    /// SETTINGS › PROJECT › MANAGE, from outside SETTINGS.
    fn open_manage(&mut self) {
        self.tap(ButtonId::Menu);
        self.feed(Input::press(ButtonId::Edit));
        self.feed(Input::press(ButtonId::Plus));
        self.feed(Input::press(ButtonId::Plus));
        self.feed(Input::press(ButtonId::Edit));
        assert_eq!(self.column(), Some(Column::Projects));
    }

    /// The bar on `f`'s row, then `c` in its commands.
    fn command(&mut self, f: ProjectFile, c: Command) {
        let at = |r: &Rig| r.ui.location().settings().unwrap().row() as usize;
        let i = (0..self.ui.listing().len())
            .find(|&i| self.ui.listing().entry(i).unwrap().file() == f)
            .expect("listed");
        while at(self) != i {
            self.feed(Input::press(ButtonId::Plus));
        }
        self.feed(Input::press(ButtonId::Edit));
        for _ in 0..c as usize {
            self.feed(Input::press(ButtonId::Plus));
        }
        assert_eq!(self.column(), Some(Column::Command(c as u8)));
    }

    fn question(&self) -> String {
        let (q, _) = self.ui.prompt_words_for_test().expect("a prompt");
        q.as_str().to_string()
    }

    fn files(&mut self) -> Vec<ProjectFile> {
        listed(&mut self.s).iter().map(|e| e.file()).collect()
    }

    /// Outside SETTINGS.
    fn leave(&mut self) {
        while self.ui.in_settings() {
            self.tap(ButtonId::Menu);
        }
    }
}

fn name(s: &str) -> ProjectName {
    ProjectName::new(s).unwrap()
}

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

fn read_back(s: &mut MemStore, f: ProjectFile) -> Box<Project> {
    let (mut p, t) = Project::boxed();
    let src = ProjectSource::File {
        id: f.id(),
        vol: f.vol(),
    };
    let go = ReplaceGuard::check(&p, t, src).unwrap();
    let out = load_project(&mut Card::new(), s, &mut p, go, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    let _ = out.swap;
    p
}

/// A rig with ALPHA and BETA on the card and ALPHA loaded through MANAGE.
fn two() -> (Rig, ProjectFile, ProjectFile) {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    let b = put(&mut s, "BETA");
    let mut r = Rig::new(s);
    r.open_manage();
    r.command(a, Command::LoadFrom);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.project().meta().file(), Some(a));
    r.ui.update();
    (r, a, b)
}

fn modify(r: &mut Rig, n: &str) {
    r.ui.project_mut().set_name(name(n));
    r.ui.update();
    assert_eq!(r.ui.project_status(), ProjectStatus::Modified);
}

#[test]
fn delete_is_dimmed_on_the_loaded_project() {
    let (mut r, a, b) = two();
    r.tap(ButtonId::Menu);
    r.command(a, Command::Delete);
    let row = r.ui.manage_command_row(Command::Delete).unwrap();
    assert_eq!(row.look, RowLook::Dimmed);
    r.tap(ButtonId::Seq);
    assert!(!r.ui.prompt_open());
    assert_eq!(r.files(), [a, b]);

    let rename = r.ui.manage_command_row(Command::Rename).unwrap();
    assert_eq!(rename.look, RowLook::Normal);
    let protect = r.ui.manage_command_row(Command::Protect).unwrap();
    assert_eq!(protect.look, RowLook::Later);

    // Another project: DELETE applies, RENAME waits on a load.
    r.tap(ButtonId::Menu);
    r.command(b, Command::Delete);
    assert_eq!(
        r.ui.manage_command_row(Command::Delete).unwrap().look,
        RowLook::Normal
    );
    let rename = r.ui.manage_command_row(Command::Rename).unwrap();
    assert_eq!(rename.look, RowLook::Dimmed);
    assert_eq!(
        Command::Rename.on(Whose::Other),
        Err(Off::Dimmed(Some(Note::LoadToRename)))
    );
    // Dimmed: SEQ opens nothing.
    r.feed(Input::press(ButtonId::Minus));
    r.feed(Input::press(ButtonId::Minus));
    assert_eq!(r.column(), Some(Column::Command(Command::Rename as u8)));
    r.tap(ButtonId::Seq);
    assert!(r.ui.naming().is_none() && !r.ui.prompt_open());
}

#[test]
fn delete_another_after_confirm() {
    let (mut r, a, b) = two();
    r.tap(ButtonId::Menu);
    r.command(b, Command::Delete);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "DELETE BETA?");
    // CANCEL keeps it.
    r.feed(Input::turn(EncoderId::A, 1));
    r.tap(ButtonId::Seq);
    assert!(!r.ui.prompt_open());
    assert_eq!(r.files(), [a, b]);
    // MENU cancels too.
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Menu);
    assert!(!r.ui.prompt_open());
    assert_eq!(r.files(), [a, b]);

    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.files(), [a]);
    assert_eq!(r.ui.listing().len(), 1);
    // Its commands went with it: the bar is back on the list.
    assert_eq!(r.column(), Some(Column::Projects));
    assert_eq!(r.ui.project().meta().file(), Some(a));
}

#[test]
fn delete_the_project_system_names_clears_last_project() {
    let (mut r, a, b) = two();
    assert_eq!(r.set.last_project, Some(a.id()));
    // + NEW leaves SYSTEM naming ALPHA, which can now be deleted.
    r.leave();
    r.tap(ButtonId::Menu);
    r.feed(Input::press(ButtonId::Edit));
    r.feed(Input::press(ButtonId::Edit));
    let new = r.ui.listing().load_rows() - 1;
    for _ in 0..new {
        r.feed(Input::press(ButtonId::Plus));
    }
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.project().meta().file(), None);
    assert_eq!(r.set.last_project, Some(a.id()));

    r.leave();
    r.open_manage();
    r.command(a, Command::Delete);
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.files(), [b]);
    assert_eq!(r.set.last_project, None);
}

#[test]
fn save_to_overwrites_after_confirm() {
    let (mut r, a, b) = two();
    modify(&mut r, "GAMMA");
    let crc = project_crc(r.ui.project());
    r.tap(ButtonId::Menu);
    r.command(b, Command::SaveTo);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "SAVE OVER BETA?");
    // CANCEL: BETA stays BETA.
    r.tap(ButtonId::Menu);
    assert_eq!(listed(&mut r.s)[1].name, Some(name("BETA")));

    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(project_crc(&read_back(&mut r.s, b)), crc);
    assert_eq!(listed(&mut r.s)[1].name, Some(name("GAMMA")));
    assert_eq!(r.ui.project().meta().file(), Some(b));
    r.ui.update();
    assert_eq!(r.ui.project_status(), ProjectStatus::Saved);
    assert_eq!(listed(&mut r.s)[0].file(), a);
    assert_eq!(listed(&mut r.s)[0].name, Some(name("ALPHA")));
}

#[test]
fn clear_other_and_clear_own() {
    let (mut r, a, b) = two();
    let fresh = project_crc(&Project::boxed().0);

    // Another: its file becomes NEW; RAM is untouched.
    r.tap(ButtonId::Menu);
    r.command(b, Command::Clear);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "CLEAR BETA?");
    r.tap(ButtonId::Seq);
    assert_eq!(listed(&mut r.s)[1].name, Some(name("NEW PROJECT")));
    assert_eq!(project_crc(&read_back(&mut r.s, b)), fresh);
    assert_eq!(r.ui.project().meta().name(), name("ALPHA"));
    assert_eq!(r.ui.project().meta().file(), Some(a));

    // Your own, Modified: CLEAR, then the load's prompt, LOAD ANYWAY.
    modify(&mut r, "EDITED");
    r.tap(ButtonId::Menu);
    r.command(a, Command::Clear);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "CLEAR ALPHA?");
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "START A NEW PROJECT?");
    r.feed(Input::turn(EncoderId::A, 1));
    r.tap(ButtonId::Seq);
    assert!(!r.ui.prompt_open());
    r.ui.update();
    assert_eq!(project_crc(r.ui.project()), fresh);
    assert_eq!(r.ui.project().meta().file(), Some(a));
    assert_eq!(r.ui.project().meta().saved_crc(), Some(fresh));
    // Saved, but NEW reads Pristine first (project_marks_test).
    assert_eq!(r.ui.project_status(), ProjectStatus::Pristine);
    assert_eq!(project_crc(&read_back(&mut r.s, a)), fresh);
    assert_eq!(listed(&mut r.s)[0].name, Some(name("NEW PROJECT")));
}

#[test]
fn rename_loaded_marks_modified() {
    let (mut r, a, _) = two();
    assert_eq!(r.ui.project_status(), ProjectStatus::Saved);
    r.tap(ButtonId::Menu);
    r.command(a, Command::Rename);
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "ALPHA");
    r.feed(Input::turn(EncoderId::E, -1));
    r.tap(ButtonId::Seq);
    assert!(r.ui.naming().is_none());
    assert_eq!(r.ui.project().meta().name(), name("ALPH"));
    r.ui.update();
    assert_eq!(r.ui.project_status(), ProjectStatus::Modified);
    // Back on MANAGE's commands; the card is untouched.
    assert_eq!(r.column(), Some(Column::Command(Command::Rename as u8)));
    assert_eq!(listed(&mut r.s)[0].name, Some(name("ALPHA")));
}

#[test]
fn menu_from_commands_returns_to_the_list() {
    let (mut r, _, b) = two();
    r.tap(ButtonId::Menu);
    r.command(b, Command::Clear);
    r.tap(ButtonId::Menu);
    assert_eq!(r.column(), Some(Column::Projects));
    assert_eq!(r.ui.location().settings().unwrap().row(), 1);
    // SEQ on the list runs nothing.
    r.tap(ButtonId::Seq);
    assert!(!r.ui.prompt_open());
    r.tap(ButtonId::Menu);
    let m = screen_path(Screen::ManageProjects);
    assert_eq!(
        r.ui.location(),
        Location::settings_at(&m[..m.len() - 1], m[m.len() - 1])
    );
}

#[test]
fn an_empty_card_never_enters_manage() {
    let mut r = Rig::new(MemStore::new(1));
    r.tap(ButtonId::Menu);
    r.feed(Input::press(ButtonId::Edit));
    r.feed(Input::press(ButtonId::Plus));
    r.feed(Input::press(ButtonId::Plus));
    let at = r.ui.location();
    r.feed(Input::press(ButtonId::Edit));
    assert_eq!(r.ui.location(), at);
    assert_eq!(r.column(), None);
}

#[test]
fn every_command_fits_its_column() {
    use chimera_core::ui::settings::MANAGE_COMMANDS;
    use chimera_core::ui::settings::view::COMMAND_W;
    use chimera_core::ui::{draw, theme};
    for w in [Whose::Loaded, Whose::Other] {
        for c in MANAGE_COMMANDS {
            let row = c.row(Some(w));
            let label = draw::text_width(&theme::FONT_LABEL_BOLD, row.label, theme::LABEL_TRACKING);
            assert!(label <= COMMAND_W, "{c:?}: {label} px");
            let note = match row.look {
                RowLook::Later => Some("LATER"),
                _ => row.note,
            };
            if let Some(n) = note {
                let nw = draw::text_width(&theme::FONT_LABEL, n, 0);
                assert!(nw <= COMMAND_W, "{c:?} {n}: {nw} px");
            }
        }
    }
}

#[test]
fn clear_own_when_saved() {
    let (mut r, a, _) = two();
    assert_eq!(r.ui.project_status(), ProjectStatus::Saved);
    let fresh = project_crc(&Project::boxed().0);
    r.tap(ButtonId::Menu);
    r.command(a, Command::Clear);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "CLEAR ALPHA?");
    r.tap(ButtonId::Seq);
    // Saved: no load prompt.
    assert!(!r.ui.prompt_open());
    assert_eq!(project_crc(&read_back(&mut r.s, a)), fresh);
    assert_eq!(listed(&mut r.s)[0].name, Some(name("NEW PROJECT")));
    assert_eq!(r.ui.project().meta().file(), Some(a));
}

/// The card swapped under CLEAR's prompt: refused before NEW loads, so
/// RAM keeps the project and its file.
#[test]
fn clear_own_on_a_swapped_card_leaves_ram() {
    let (mut r, a, _) = two();
    let kept = project_crc(r.ui.project());
    r.tap(ButtonId::Menu);
    r.command(a, Command::Clear);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "CLEAR ALPHA?");
    r.s.swap(2);
    r.tap(ButtonId::Seq);
    assert_eq!(project_crc(r.ui.project()), kept);
    assert!(!r.ui.prompt_open());
    let toast = match r.ui.step_toast(0) {
        ToastStep::Show(t) => t.as_str().to_string(),
        _ => String::new(),
    };
    assert!(toast.starts_with("CARD CHANGED"), "{toast}");
    assert_eq!(r.ui.project().meta().file(), Some(a));
    assert_eq!(r.ui.project().meta().name(), name("ALPHA"));
}

#[test]
fn clear_own_save_then_load_keeps_the_edits_elsewhere() {
    let (mut r, a, b) = two();
    let fresh = project_crc(&Project::boxed().0);
    let p1 = chimera_core::project::PartId::ALL[0];
    let level = |p: &Project| p.part(p1).mix.get(PartParams::LEVEL);
    r.ui.project_mut()
        .edit_part(p1)
        .mix
        .set(PartParams::LEVEL, 0.25);
    r.ui.update();
    assert_eq!(r.ui.project_status(), ProjectStatus::Modified);
    let edited = level(r.ui.project());
    r.tap(ButtonId::Menu);
    r.command(a, Command::Clear);
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert_eq!(r.question(), "START A NEW PROJECT?");
    // SAVE THEN LOAD: SAVE AS, never over the file being cleared.
    r.tap(ButtonId::Seq);
    assert_eq!(r.ui.naming().unwrap().text(), "PULSE-003");
    r.tap(ButtonId::Seq);
    assert!(r.ui.naming().is_none() && !r.ui.prompt_open());

    let all = listed(&mut r.s);
    assert_eq!(all.len(), 3);
    let c = all[2].file();
    assert_eq!(all[2].name, Some(name("PULSE-003")));
    assert_eq!(level(&read_back(&mut r.s, c)), edited);
    assert_ne!(level(&read_back(&mut r.s, a)), edited);
    assert_eq!(project_crc(&read_back(&mut r.s, a)), fresh);
    assert_eq!(all[0].name, Some(name("NEW PROJECT")));
    assert_eq!(all[1].file(), b);
    assert_eq!(project_crc(r.ui.project()), fresh);
    assert_eq!(r.ui.project().meta().file(), Some(a));
}

#[test]
fn manage_never_sits_empty() {
    let mut s = MemStore::new(1);
    let a = put(&mut s, "ALPHA");
    // NEW, no file: the only one on the card can go.
    let mut r = Rig::new(s);
    assert_eq!(r.ui.project().meta().file(), None);
    r.open_manage();
    r.command(a, Command::Delete);
    r.tap(ButtonId::Seq);
    r.tap(ButtonId::Seq);
    assert!(r.files().is_empty());
    let m = screen_path(Screen::ManageProjects);
    assert_eq!(
        r.ui.location(),
        Location::settings_at(&m[..m.len() - 1], m[m.len() - 1])
    );
}
