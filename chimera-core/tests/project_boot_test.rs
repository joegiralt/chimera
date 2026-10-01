//! Boot loads SYSTEM's last project (projects spec § Boot); the UI's save
//! and load record it there (§ SYSTEM) and show their notes.

mod common;

use chimera_core::block::Block;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::test_support::{FlipOnSecondRead, full, same};
use chimera_core::project::{
    LOAD_LINK, Line, LoadLink, PartFrom, PartId, PartSource, Project, ProjectFile, ProjectNote,
    ProjectSource, ProjectStatus, ReplaceGuard, Subject, TemplateCrc, project_crc, project_file,
    project_status, save_project,
};
use chimera_core::storage::{AbFile, Card, FileError, ProjectId, Side, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::{Toast, ToastStep};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use common::codec_util::encode_project;

fn id(n: u32) -> ProjectId {
    ProjectId::new(n).unwrap()
}

fn at(s: &mut impl Store, n: u32) -> ProjectFile {
    ProjectFile {
        id: id(n),
        vol: s.mount().unwrap(),
    }
}

fn show(s: &str) -> ToastStep {
    ToastStep::Show(Line::new(s))
}

/// SYSTEM's last project, as the next boot reads it.
fn last_on_card(s: &mut impl Store) -> Option<ProjectId> {
    SystemSync::boot(&mut Card::new(), s).1.last_project
}

/// `p` saved as `P000000n`, outside any UI.
fn put_project(s: &mut impl Store, p: &mut Project, n: u32) {
    let f = at(s, n);
    let note = save_project(&mut Card::new(), s, p, f);
    assert!(matches!(note, ProjectNote::Saved(_)), "{note:?}");
}

/// A fresh UI after SYSTEM's boot, as the shells build it.
struct Booted {
    ui: Box<UiState>,
    card: Card,
    sync: SystemSync,
    settings: SystemSettings,
}

fn boot_system(s: &mut impl Store) -> Booted {
    let mut card = Card::new();
    let (sync, settings, _) = SystemSync::boot(&mut card, s);
    let mut ui = Box::new(UiState::new());
    ui.set_theme(settings.theme);
    Booted {
        ui,
        card,
        sync,
        settings,
    }
}

fn file_go(ui: &UiState, f: ProjectFile) -> chimera_core::project::Confirmed<ProjectSource> {
    let src = ProjectSource::File {
        id: f.id,
        vol: f.vol,
    };
    ReplaceGuard::check(ui.project(), ui.template(), src)
        .unwrap_or_else(|n| n.into_pending().anyway(ui.project()))
}

#[test]
fn boot_loads_the_last_project() {
    let mut s = MemStore::new(1);
    let (mut p, _) = full();
    put_project(&mut s, &mut p, 2);
    let mut b = boot_system(&mut s);
    b.settings.last_project = Some(id(2));
    b.sync.write(&mut b.card, &mut s, &b.settings).unwrap();

    let mut b = boot_system(&mut s);
    assert_eq!(b.settings.last_project, Some(id(2)));
    b.ui.boot_project(&mut b.card, &mut s, b.settings.last_project);
    same(&p, b.ui.project());
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Saved
    );
    assert_eq!(b.ui.project().meta().file(), Some(at(&mut s, 2)));
    assert_eq!(b.ui.step_toast(0), ToastStep::Idle, "a load says nothing");
    assert_eq!(LOAD_LINK.epoch(), 0, "boot never bumps");

    // Bound to this card: SAVE lands here, and a swapped card refuses it.
    b.ui.project_mut().edit_fx().reverb.mix = 0.2;
    let f = b.ui.project().meta().file().unwrap();
    b.ui.save_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, f);
    assert_eq!(
        b.ui.step_toast(0),
        show("SAVED: P2 DIFFERS FROM SLOT 04"),
        "full()'s edited Part"
    );
    s.swap(9);
    b.ui.project_mut().edit_fx().reverb.mix = 0.3;
    b.ui.save_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, f);
    assert_eq!(b.ui.step_toast(0), show("CARD CHANGED: FULL"));
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Modified
    );
    assert_eq!(
        last_on_card(&mut s),
        None,
        "nothing written to the new card"
    );
}

#[test]
fn boot_without_a_last_id_is_new_and_says_so() {
    let mut s = MemStore::new(1);
    let mut b = boot_system(&mut s);
    assert_eq!(b.settings.last_project, None);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    b.ui.boot_project(&mut b.card, &mut s, None);
    assert_eq!(b.ui.step_toast(0), show("NEW PROJECT"));
    assert_eq!(
        b.ui.step_toast(Toast::ERROR_MS - 1),
        show("NEW PROJECT"),
        "an error's time"
    );
    assert_eq!(b.ui.step_toast(1), ToastStep::Ended);
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Pristine
    );
    assert_eq!(b.ui.project().meta().file(), None);
}

/// Each reason boot falls back to NEW: the note, and NEW whatever the
/// project held before.
#[test]
fn boot_reasons() {
    let (mut src, _) = full();
    let good = encode_project(&src);
    let mut bad = good.clone();
    bad[good.len() / 2] ^= 0x40;
    let corrupt = |s: &mut MemStore| {
        let v = s.mount().unwrap();
        s.make_dir(v, Dir::Chimera).unwrap();
        s.make_dir(v, Dir::Projects).unwrap();
        for side in [Side::A, Side::B] {
            s.write(v, project_file(id(3)).side(side), &mut |w| w.put(&bad))
                .unwrap();
        }
    };

    let check = |ui: &UiState, want: ProjectNote| {
        assert_eq!(project_crc(ui.project()), ui.template().get(), "{want:?}");
        assert_eq!(ui.project().meta().file(), None, "{want:?}");
        assert_eq!(LOAD_LINK.epoch(), 0, "{want:?}: no bump");
    };
    let dirty = |ui: &mut UiState| ui.project_mut().edit_fx().chorus.mode = 2;

    // No card.
    let mut s = MemStore::new(1);
    s.eject();
    let mut b = boot_system(&mut s);
    dirty(&mut b.ui);
    b.ui.boot_project(&mut b.card, &mut s, Some(id(3)));
    let want = ProjectNote::Card {
        err: StoreError::NoCard,
        subject: None,
    };
    assert_eq!(b.ui.step_toast(0), ToastStep::Show(want.line()));
    assert_eq!(want.line().as_str(), "NO CARD");
    check(&b.ui, want);

    // Missing.
    let mut s = MemStore::new(1);
    let mut b = boot_system(&mut s);
    dirty(&mut b.ui);
    b.ui.boot_project(&mut b.card, &mut s, Some(id(3)));
    let want = ProjectNote::Missing(Subject::File(id(3)));
    assert_eq!(b.ui.step_toast(0), ToastStep::Show(want.line()));
    assert_eq!(want.line().as_str(), "PROJECT NOT FOUND: P0000003");
    check(&b.ui, want);

    // Corrupt: pass 1 fails.
    let mut s = MemStore::new(1);
    corrupt(&mut s);
    let mut b = boot_system(&mut s);
    dirty(&mut b.ui);
    b.ui.boot_project(&mut b.card, &mut s, Some(id(3)));
    let want = ProjectNote::File {
        err: FileError::BadCrc,
        subject: Subject::File(id(3)),
    };
    assert_eq!(b.ui.step_toast(0), ToastStep::Show(want.line()));
    check(&b.ui, want);

    // Clobbered: pass 2 fails after writing the project.
    let mut mem = MemStore::new(1);
    put_project(&mut mem, &mut src, 3);
    let mut s = FlipOnSecondRead::new(mem, project_file(id(3)).side(Side::A));
    let mut b = boot_system(&mut s);
    b.ui.boot_project(&mut b.card, &mut s, Some(id(3)));
    assert_eq!(s.reads(), 2, "pass 2 ran");
    let want = ProjectNote::LoadFailed(Subject::File(id(3)));
    assert_eq!(b.ui.step_toast(0), ToastStep::Show(want.line()));
    check(&b.ui, want);
}

#[test]
fn save_writes_the_last_project() {
    let mut s = MemStore::new(1);
    let mut b = boot_system(&mut s);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    let f = at(&mut s, 3);
    b.ui.save_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, f);
    assert_eq!(b.settings.last_project, Some(id(3)));
    assert_eq!(last_on_card(&mut s), Some(id(3)));
    assert_eq!(b.ui.step_toast(0), show("SAVED"));
    assert_eq!(b.ui.step_toast(Toast::SAVED_MS - 1), show("SAVED"));
    assert_eq!(b.ui.step_toast(1), ToastStep::Ended, "a save's time");
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Saved
    );

    // A failed save leaves SYSTEM.
    let f4 = at(&mut s, 4);
    s.eject();
    b.ui.save_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, f4);
    assert_eq!(b.ui.step_toast(0), show("NO CARD: NEW PROJECT"));
    s.insert();
    assert_eq!(
        (b.settings.last_project, last_on_card(&mut s)),
        (Some(id(3)), Some(id(3)))
    );
}

/// The project lands, SYSTEM's write fails: the toast is the save's.
#[test]
fn a_failed_system_write_keeps_the_saved_toast() {
    let mut s = NoSystemWrites(MemStore::new(1));
    let mut b = boot_system(&mut s);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    let f = at(&mut s, 3);
    b.ui.save_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, f);
    assert_eq!(b.ui.step_toast(0), show("SAVED"));
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Saved
    );
    assert_eq!(last_on_card(&mut s.0), None);
    assert_eq!(b.settings.last_project, Some(id(3)), "retried next time");
}

#[test]
fn load_writes_the_last_project() {
    let mut s = MemStore::new(1);
    let (mut p, _) = full();
    put_project(&mut s, &mut p, 5);
    let (mut other, _) = full();
    put_project(&mut s, &mut other, 6);
    let link = LoadLink::new();
    let mut b = boot_system(&mut s);

    let go = file_go(&b.ui, at(&mut s, 5));
    let swap =
        b.ui.load_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, go, &link);
    assert!(swap.is_some());
    same(&p, b.ui.project());
    assert_eq!(b.ui.step_toast(0), ToastStep::Idle, "a load says nothing");
    assert_eq!(b.settings.last_project, Some(id(5)));
    assert_eq!(last_on_card(&mut s), Some(id(5)));

    // + NEW leaves it.
    let go = ReplaceGuard::check(b.ui.project(), b.ui.template(), ProjectSource::New)
        .expect("Saved never asks");
    let swap =
        b.ui.load_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, go, &link);
    assert!(swap.is_some());
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Pristine
    );
    assert_eq!(
        (b.settings.last_project, last_on_card(&mut s)),
        (Some(id(5)), Some(id(5)))
    );

    // So does a fallback to NEW.
    let go = file_go(&b.ui, at(&mut s, 6));
    let mut s = FlipOnSecondRead::new(s, project_file(id(6)).side(Side::A));
    let swap =
        b.ui.load_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, go, &link);
    assert!(swap.is_some(), "a failed pass 2 still swaps");
    assert_eq!(b.ui.step_toast(0), show("LOAD FAILED: P0000006"));
    assert_eq!(b.ui.project().meta().file(), None);
    assert_eq!(
        (b.settings.last_project, last_on_card(&mut s.inner)),
        (Some(id(5)), Some(id(5)))
    );

    // A pass 1 failure swaps nothing and leaves it.
    let mut s = s.inner;
    let go = file_go(&b.ui, at(&mut s, 7));
    let swap =
        b.ui.load_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, go, &link);
    assert!(swap.is_none());
    assert_eq!(b.ui.step_toast(0), show("PROJECT NOT FOUND: P0000007"));
    assert_eq!(last_on_card(&mut s), Some(id(5)));
}

/// A project whose Part 1 is a Modal INIT, edited, on another page and
/// engine than NEW's.
fn modal_project() -> (Box<Project>, TemplateCrc) {
    let (mut p, t) = Project::boxed();
    let src = PartSource {
        part: PartId::ALL[0],
        from: PartFrom::Init(EngineType::Modal),
    };
    let c = ReplaceGuard::check(&p, t, src).expect("a Clean Part");
    p.replace_part(c).unwrap();
    p.edit_part(PartId::ALL[0])
        .sound
        .params
        .filter
        .set(FilterParams::CUTOFF, 300.0);
    (p, t)
}

fn shown(ui: &UiState) -> [f32; 6] {
    ui.renderer.anim.each_ref().map(|a| a.current())
}

#[test]
fn replaced_ui_snaps() {
    let mut s = MemStore::new(1);
    let (mut p, _) = modal_project();
    put_project(&mut s, &mut p, 5);
    let mut b = boot_system(&mut s);
    b.ui.update();
    let before = shown(&b.ui);
    assert_ne!(b.ui.nav.engine, EngineType::Modal);

    let go = file_go(&b.ui, at(&mut s, 5));
    let swap = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &LoadLink::new(),
    );
    assert!(swap.is_some());
    assert_eq!(
        b.ui.nav.engine,
        EngineType::Modal,
        "the active Part's engine"
    );
    let now = shown(&b.ui);
    // Settled: as many frames as any lerp takes.
    for _ in 0..500 {
        b.ui.update();
    }
    let settled = shown(&b.ui);
    assert_ne!(before, settled, "the page shows other values");
    for i in 0..6 {
        assert!(
            (now[i] - settled[i]).abs() < 1e-4,
            "slot {i}: {} then {}",
            now[i],
            settled[i]
        );
    }
}

/// Every write but SYSTEM's lands.
struct NoSystemWrites(MemStore);

impl Store for NoSystemWrites {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.0.mount()
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.0.list(vol, dir, f)
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.0.read(vol, file, sink)
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        if [Side::A, Side::B]
            .map(|s| AbFile::SYSTEM.side(s))
            .contains(&file)
        {
            return Err(StoreError::Full);
        }
        self.0.write(vol, file, body)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.0.delete(vol, file)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.0.make_dir(vol, dir)
    }
}
