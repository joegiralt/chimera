//! Boot loads SYSTEM's last project (projects spec § Boot); the UI's save
//! and load record it there (§ SYSTEM) and show their notes.

mod common;
mod screen;

use chimera_core::block::Block;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::project::test_support::{
    FlipOnSecondRead, confirm_delete_of, full, same, save_to,
};
use chimera_core::project::{
    LOAD_LINK, Line, LoadLink, PartFrom, PartId, PartSource, Project, ProjectFile, ProjectNote,
    ProjectSource, ProjectStatus, ReplaceGuard, Subject, TemplateCrc, project_crc, project_file,
    project_status, save_project,
};
use chimera_core::storage::Exit;
use chimera_core::storage::{AbFile, Card, FileError, ProjectId, Side, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::{Toast, ToastStep};
use chimera_core::ui::theme_settings::Bright;
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use common::codec_util::encode_project;

fn id(n: u32) -> ProjectId {
    ProjectId::new(n).unwrap()
}

fn at(s: &mut impl Store, n: u32) -> ProjectFile {
    ProjectFile::for_test(id(n), s.mount().unwrap())
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
    let note = save_project(&mut Card::new(), s, p, save_to(p, f)).out;
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
        id: f.id(),
        vol: f.vol(),
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
    let f = at(&mut s, 2);
    b.sync
        .write(&mut b.card, &mut s, &mut b.settings, f)
        .unwrap();

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
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f),
    );
    assert_eq!(
        b.ui.step_toast(0),
        show("SAVED: P2 DIFFERS FROM SLOT 04"),
        "full()'s edited Part"
    );
    s.swap(9);
    b.ui.project_mut().edit_fx().reverb.mix = 0.3;
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f),
    );
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

/// No card: SYSTEM's defaults name no project, and the toast says why.
#[test]
fn boot_without_a_card_says_no_card() {
    let mut s = MemStore::new(1);
    s.eject();
    let mut b = boot_system(&mut s);
    assert_eq!(b.settings.last_project, None);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    b.ui.boot_project(&mut b.card, &mut s, None);
    assert_eq!(b.ui.step_toast(0), show("NO CARD"));
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Pristine
    );
    assert_eq!(b.card, Card::Absent);

    // A card that won't mount: its fault, not NEW PROJECT.
    let mut s = Timeout;
    let mut b = boot_system(&mut s);
    b.ui.boot_project(&mut b.card, &mut s, None);
    let want = ProjectNote::Card {
        err: StoreError::Timeout,
        subject: None,
    };
    assert_eq!(b.ui.step_toast(0), ToastStep::Show(want.line()));
}

/// A card in the slot that times out on every call.
struct Timeout;

impl Store for Timeout {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        Err(StoreError::Timeout)
    }
    fn list(
        &mut self,
        _: VolumeId,
        _: Dir,
        _: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        Err(StoreError::Timeout)
    }
    fn read(&mut self, _: VolumeId, _: FileName, _: &mut dyn ReadSink) -> Result<(), StoreError> {
        Err(StoreError::Timeout)
    }
    fn write(
        &mut self,
        _: VolumeId,
        _: FileName,
        _: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        Err(StoreError::Timeout)
    }
    fn delete(&mut self, _: VolumeId, _: FileName) -> Result<(), StoreError> {
        Err(StoreError::Timeout)
    }
    fn make_dir(&mut self, _: VolumeId, _: Dir) -> Result<(), StoreError> {
        Err(StoreError::Timeout)
    }
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
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f),
    );
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
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f4),
    );
    assert_eq!(b.ui.step_toast(0), show("NO CARD: NEW PROJECT"));
    s.insert();
    assert_eq!(
        (b.settings.last_project, last_on_card(&mut s)),
        (Some(id(3)), Some(id(3)))
    );
}

/// Deleting the project SYSTEM names (possible after `+ NEW`) clears it,
/// so the next boot isn't PROJECT NOT FOUND; deleting another leaves it.
#[test]
fn delete_forgets_the_last_project() {
    let mut s = MemStore::new(1);
    let mut b = boot_system(&mut s);
    let (f3, f4) = (at(&mut s, 3), at(&mut s, 4));
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f3),
    );
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f4),
    );
    assert_eq!(last_on_card(&mut s), Some(id(4)));
    let go = ReplaceGuard::check(b.ui.project(), b.ui.template(), ProjectSource::New)
        .expect("Saved never asks");
    let link = LoadLink::new();
    let _ = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &link,
        settle,
    );

    let c = confirm_delete_of(&mut s, f3);
    b.ui.delete_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, c);
    assert_eq!(
        (b.settings.last_project, last_on_card(&mut s)),
        (Some(id(4)), Some(id(4))),
        "another project: kept"
    );
    let c = confirm_delete_of(&mut s, f4);
    b.ui.delete_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, c);
    assert_eq!(
        (b.settings.last_project, last_on_card(&mut s)),
        (None, None),
        "the last project: cleared"
    );
    let mut again = boot_system(&mut s);
    let last = again.settings.last_project;
    again.ui.boot_project(&mut again.card, &mut s, last);
    assert_eq!(again.ui.step_toast(0), show("NEW PROJECT"));
}

/// `want` as SYSTEM's next generation on `s`, written as it stands.
fn save_system(s: &mut MemStore, want: &SystemSettings) {
    use chimera_core::storage::{SystemCheck, encode_system, save_ab};
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

/// A card SYSTEM wasn't read from: a delete there clears its own last
/// project, and its theme still applies (untouched defaults never go over
/// it).
#[test]
fn delete_forgets_on_the_card_it_deletes_from() {
    let mut s = MemStore::new(1);
    let mut b = boot_system(&mut s);
    let f3 = at(&mut s, 3);
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f3),
    );
    // Its theme, changed outside this UI.
    let mut card_side = SystemSync::boot(&mut Card::new(), &mut s).1;
    card_side.theme.bright = Bright::new(40);
    save_system(&mut s, &card_side);

    let mut b = boot_system(&mut MemStore::new(9));
    let c = confirm_delete_of(&mut s, f3);
    b.ui.delete_project(&mut b.card, &mut s, &mut b.sync, &mut b.settings, c);
    assert_eq!(last_on_card(&mut s), None);
    assert_eq!(
        SystemSync::boot(&mut Card::new(), &mut s).1.theme,
        card_side.theme,
        "the card's theme kept"
    );
}

/// The project lands, SYSTEM's write fails: the toast is the save's.
#[test]
fn a_failed_system_write_keeps_the_saved_toast() {
    let mut s = NoSystemWrites(MemStore::new(1));
    let mut b = boot_system(&mut s);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    let f = at(&mut s, 3);
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f),
    );
    assert_eq!(b.ui.step_toast(0), show("SAVED"));
    assert_eq!(
        project_status(b.ui.project(), b.ui.template()),
        ProjectStatus::Saved
    );
    assert_eq!(last_on_card(&mut s.0), None);
    assert_eq!(b.settings.last_project, None, "RAM mirrors the card");

    // Retried at the next System exit, with nothing else changed.
    let mut s = s.0;
    screen::tap(&mut b.ui, chimera_hal::ButtonId::Menu);
    b.ui.sync_system(&mut b.sync, &mut b.card, &mut s, &mut b.settings);
    screen::tap(&mut b.ui, chimera_hal::ButtonId::Menu);
    b.ui.sync_system(&mut b.sync, &mut b.card, &mut s, &mut b.settings);
    assert_eq!(last_on_card(&mut s), Some(id(3)));
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
    let swap = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &link,
        settle,
    );
    assert!(swap.is_some());
    same(&p, b.ui.project());
    assert_eq!(b.ui.step_toast(0), ToastStep::Idle, "a load says nothing");
    assert_eq!(b.settings.last_project, Some(id(5)));
    assert_eq!(last_on_card(&mut s), Some(id(5)));

    // + NEW leaves it.
    let go = ReplaceGuard::check(b.ui.project(), b.ui.template(), ProjectSource::New)
        .expect("Saved never asks");
    let swap = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &link,
        settle,
    );
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
    let swap = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &link,
        settle,
    );
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
    let swap = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &link,
        settle,
    );
    assert!(swap.is_none());
    assert_eq!(b.ui.step_toast(0), show("PROJECT NOT FOUND: P0000007"));
    assert_eq!(last_on_card(&mut s), Some(id(5)));
}

/// A shell's publish: settle at once (no audio runs here).
fn settle(swap: chimera_core::project::Swap, _: &Project) {
    let _ = swap.settle(&LoadLink::new(), || false);
}

/// Counts the card writes it passes on.
struct CountWrites<S> {
    inner: S,
    writes: std::rc::Rc<std::cell::Cell<u32>>,
}

impl<S: Store> Store for CountWrites<S> {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }
    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.inner.list(vol, dir, f)
    }
    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.inner.read(vol, file, sink)
    }
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.writes.set(self.writes.get() + 1);
        self.inner.write(vol, file, body)
    }
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.writes.set(self.writes.get() + 1);
        self.inner.delete(vol, file)
    }
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.writes.set(self.writes.get() + 1);
        self.inner.make_dir(vol, dir)
    }
}

/// ADR 0046: from the bump to the publish the audio holds the note
/// queues, so a load's SYSTEM write waits until the shell has published.
#[test]
fn a_load_publishes_before_its_system_write() {
    let mut mem = MemStore::new(1);
    let (mut p, _) = full();
    put_project(&mut mem, &mut p, 5);
    let mut b = boot_system(&mut mem);
    let writes = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut s = CountWrites {
        inner: mem,
        writes: writes.clone(),
    };
    let link = LoadLink::new();
    let go = file_go(&b.ui, at(&mut s, 5));
    let at_publish = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &link,
        |swap, _| {
            let _ = swap.settle(&link, || false);
            writes.get()
        },
    );
    assert_eq!(
        at_publish,
        Some(0),
        "no card write between bump and publish"
    );
    assert!(writes.get() > 0, "SYSTEM written after the publish");
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
    assert_ne!(
        b.ui.project().part(b.ui.active_part).sound.engine(),
        EngineType::Modal
    );

    let go = file_go(&b.ui, at(&mut s, 5));
    let swap = b.ui.load_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        go,
        &LoadLink::new(),
        settle,
    );
    assert!(swap.is_some());
    assert_eq!(
        b.ui.project().part(b.ui.active_part).sound.engine(),
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

/// Boot on the loaded project's own Part: its engine, page and values.
#[test]
fn boot_shows_the_loaded_part() {
    let mut s = MemStore::new(1);
    let (mut p, _) = modal_project();
    put_project(&mut s, &mut p, 4);
    let mut b = boot_system(&mut s);
    let f = at(&mut s, 4);
    b.sync
        .write(&mut b.card, &mut s, &mut b.settings, f)
        .unwrap();
    let mut b = boot_system(&mut s);
    b.ui.boot_project(&mut b.card, &mut s, b.settings.last_project);
    assert_eq!(
        b.ui.project().part(b.ui.active_part).sound.engine(),
        EngineType::Modal
    );
    let now = shown(&b.ui);
    for _ in 0..500 {
        b.ui.update();
    }
    let settled = shown(&b.ui);
    for i in 0..6 {
        assert!((now[i] - settled[i]).abs() < 1e-4, "slot {i}");
    }
}

/// A card whose SYSTEM holds the owner's theme (BRIGHT 40), and no last
/// project.
fn owner_card() -> (MemStore, SystemSettings) {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    set.theme.bright = Bright::new(40);
    assert_eq!(
        sync.on_exit(&mut card, &mut s, &mut set, None),
        Ok(Exit::Wrote)
    );
    (s, set)
}

fn system_on(s: &mut impl Store) -> SystemSettings {
    SystemSync::boot(&mut Card::new(), s).1
}

/// Booted with no card, then the owner's card goes in: a save keeps its
/// theme, adds the last project, and the UI takes the theme.
#[test]
fn a_save_after_a_late_card_keeps_its_theme() {
    let (mut s, owner) = owner_card();
    s.eject();
    let mut card = Card::new();
    let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    s.insert();
    let mut ui = Box::new(UiState::new());
    ui.project_mut().edit_fx().delay.mix = 0.8;
    let f = at(&mut s, 3);
    ui.save_project(
        &mut card,
        &mut s,
        &mut sync,
        &mut set,
        save_to(ui.project(), f),
    );
    let now = system_on(&mut s);
    assert_eq!((now.theme, now.last_project), (owner.theme, Some(id(3))));
    assert_eq!(ui.theme(), owner.theme, "the card's theme applies");
    assert_eq!(set, now);
}

/// SYSTEM reads time out until `.1` is set.
struct Flaky(MemStore, bool);

impl Store for Flaky {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.0.mount()
    }

    fn list(
        &mut self,
        v: VolumeId,
        d: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.0.list(v, d, f)
    }

    fn read(
        &mut self,
        v: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if !self.1
            && [Side::A, Side::B]
                .map(|s| AbFile::SYSTEM.side(s))
                .contains(&file)
        {
            return Err(StoreError::Timeout);
        }
        self.0.read(v, file, sink)
    }

    fn write(
        &mut self,
        v: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.0.write(v, file, body)
    }

    fn delete(&mut self, v: VolumeId, f: FileName) -> Result<(), StoreError> {
        self.0.delete(v, f)
    }

    fn make_dir(&mut self, v: VolumeId, d: Dir) -> Result<(), StoreError> {
        self.0.make_dir(v, d)
    }
}

/// SYSTEM unread at boot (a timeout), then a save: the card's theme
/// stays, whether the card reads by then or not.
#[test]
fn a_save_after_an_unread_system_keeps_its_theme() {
    for reads_by_then in [true, false] {
        let (mem, owner) = owner_card();
        let mut s = Flaky(mem, false);
        let mut card = Card::new();
        let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
        s.1 = reads_by_then;
        let mut ui = Box::new(UiState::new());
        ui.project_mut().edit_fx().delay.mix = 0.8;
        let f = at(&mut s, 3);
        ui.save_project(
            &mut card,
            &mut s,
            &mut sync,
            &mut set,
            save_to(ui.project(), f),
        );
        assert_eq!(ui.step_toast(0), show("SAVED"), "the project landed");
        let now = system_on(&mut s.0);
        assert_eq!(now.theme, owner.theme, "reads: {reads_by_then}");
        let last = reads_by_then.then_some(id(3));
        assert_eq!(now.last_project, last, "reads: {reads_by_then}");
    }
}

/// The write failed, then System is entered and left with nothing
/// changed: the card's SYSTEM is taken, not overwritten with defaults,
/// and the last project still lands.
#[test]
fn a_failed_write_then_an_idle_system_exit_keeps_the_theme() {
    let (mem, owner) = owner_card();
    let mut s = Flaky(mem, false);
    let mut card = Card::new();
    let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    let mut ui = Box::new(UiState::new());
    ui.project_mut().edit_fx().delay.mix = 0.8;
    let f = at(&mut s, 3);
    ui.save_project(
        &mut card,
        &mut s,
        &mut sync,
        &mut set,
        save_to(ui.project(), f),
    );
    s.1 = true;
    screen::tap(&mut ui, chimera_hal::ButtonId::Menu);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut set);
    screen::tap(&mut ui, chimera_hal::ButtonId::Menu);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut set);
    let now = system_on(&mut s.0);
    assert_eq!((now.theme, now.last_project), (owner.theme, Some(id(3))));
    assert_eq!(ui.theme(), owner.theme);
}

/// SYSTEM's last project names a project on that card only: P3 saved on
/// card A never becomes card B's last project, so B doesn't boot its own,
/// unrelated P0000003.
#[test]
fn the_last_project_stays_on_its_card() {
    let mut s = MemStore::new(1);
    let mut b = boot_system(&mut s);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    let fa = at(&mut s, 3);
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), fa),
    );
    assert_eq!(system_on(&mut s).last_project, Some(id(3)));

    // Card B: its own P0000003 and a SYSTEM naming P0000007.
    let mut card_b = MemStore::new(2);
    let (mut other, _) = full();
    put_project(&mut card_b, &mut other, 3);
    {
        let mut c = Card::new();
        let (mut sync, mut set, _) = SystemSync::boot(&mut c, &mut card_b);
        let f = at(&mut card_b, 7);
        sync.write(&mut c, &mut card_b, &mut set, f).unwrap();
    }
    let mut a = std::mem::replace(&mut s, card_b);

    // A theme change on B, then leaving System, writes B's SYSTEM.
    screen::tap(&mut b.ui, chimera_hal::ButtonId::Menu);
    b.ui.sync_system(&mut b.sync, &mut b.card, &mut s, &mut b.settings);
    let mut t = b.ui.theme();
    t.bright = Bright::new(55);
    b.ui.set_theme(t);
    screen::tap(&mut b.ui, chimera_hal::ButtonId::Menu);
    b.ui.sync_system(&mut b.sync, &mut b.card, &mut s, &mut b.settings);
    let on_b = system_on(&mut s);
    assert_eq!(on_b.theme, t, "B took the theme");
    assert_eq!(
        on_b.last_project,
        Some(id(7)),
        "B keeps its own last project"
    );

    // A save of A's project while B is in is refused, and SYSTEM's write
    // with it: B's last project is still its own.
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), fa),
    );
    assert_eq!(system_on(&mut s).last_project, Some(id(7)));
    // Bound by hand to A's card, a write on B does nothing, not even
    // RAM's theme.
    b.settings.theme.bright = Bright::new(60);
    assert_eq!(
        b.sync.write(&mut b.card, &mut s, &mut b.settings, fa),
        Ok(Exit::Unchanged)
    );
    assert_eq!(system_on(&mut s), on_b);

    // Back on A, A's SYSTEM still names P3.
    std::mem::swap(&mut s, &mut a);
    assert_eq!(system_on(&mut s).last_project, Some(id(3)));
}

/// A save puts the last project on a card with no SYSTEM; the theme is
/// still the untouched defaults, so a card swapped in after keeps its own
/// theme when System is left.
#[test]
fn the_last_project_is_no_theme_change() {
    let mut s = MemStore::new(5);
    let mut b = boot_system(&mut s);
    b.ui.project_mut().edit_fx().delay.mix = 0.8;
    let f = at(&mut s, 3);
    b.ui.save_project(
        &mut b.card,
        &mut s,
        &mut b.sync,
        &mut b.settings,
        save_to(b.ui.project(), f),
    );
    assert_eq!(b.settings.last_project, Some(id(3)));

    let (mut s, owner) = owner_card();
    screen::tap(&mut b.ui, chimera_hal::ButtonId::Menu);
    b.ui.sync_system(&mut b.sync, &mut b.card, &mut s, &mut b.settings);
    screen::tap(&mut b.ui, chimera_hal::ButtonId::Menu);
    b.ui.sync_system(&mut b.sync, &mut b.card, &mut s, &mut b.settings);
    assert_eq!(system_on(&mut s), owner, "the owner's card is untouched");
    assert_eq!(b.ui.theme(), owner.theme, "and its theme applies");
}

fn at_home(engine: EngineType) -> chimera_core::ui::nav::Location {
    chimera_core::ui::nav::Location::pages(
        PartId::ALL[0],
        chimera_core::ui::nav::chain_def_for(engine).home(),
    )
}

/// From Part 1's home, PLUS to FLT.
fn to_flt(ui: &mut UiState) {
    for _ in 0..3 {
        screen::feed(ui, screen::Input::press(chimera_hal::ButtonId::Plus));
    }
    assert_eq!(
        ui.page_def().id,
        chimera_core::ui::block_registry::FILTER.id
    );
}

/// Load project `n` from the card into `b`'s UI.
fn load_file(b: &mut Booted, s: &mut MemStore, n: u32) {
    let go = file_go(&b.ui, at(s, n));
    let swap = b.ui.load_project(
        &mut b.card,
        s,
        &mut b.sync,
        &mut b.settings,
        go,
        &LoadLink::new(),
        settle,
    );
    assert!(swap.is_some());
}

/// A boot onto a Modal Part 1 lands on Modal's home, RES, not on the
/// Algo page index it booted on (ADR 0066).
#[test]
fn boot_onto_a_modal_part_lands_on_res() {
    let mut s = MemStore::new(1);
    let (mut p, _) = modal_project();
    put_project(&mut s, &mut p, 4);
    let mut b = boot_system(&mut s);
    let f = at(&mut s, 4);
    b.sync
        .write(&mut b.card, &mut s, &mut b.settings, f)
        .unwrap();
    let mut b = boot_system(&mut s);
    b.ui.boot_project(&mut b.card, &mut s, b.settings.last_project);
    assert_eq!(b.ui.location(), at_home(EngineType::Modal));
    assert_eq!(
        b.ui.page_def().id,
        chimera_core::ui::block_registry::MODAL_1.id
    );
}

#[test]
fn a_load_that_changes_the_engine_lands_on_its_home() {
    let mut s = MemStore::new(1);
    let (mut p, _) = modal_project();
    put_project(&mut s, &mut p, 4);
    let mut b = boot_system(&mut s);
    to_flt(&mut b.ui);
    load_file(&mut b, &mut s, 4);
    assert_eq!(b.ui.location(), at_home(EngineType::Modal));
}

#[test]
fn a_same_engine_load_keeps_the_page() {
    let mut s = MemStore::new(1);
    let (mut p, _) = full();
    put_project(&mut s, &mut p, 5);
    let mut b = boot_system(&mut s);
    assert_eq!(
        p.part(PartId::ALL[0]).sound.engine(),
        b.ui.project().part(PartId::ALL[0]).sound.engine()
    );
    to_flt(&mut b.ui);
    let flt = b.ui.location();
    load_file(&mut b, &mut s, 5);
    assert_eq!(b.ui.location(), flt);
}
