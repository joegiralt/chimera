//! A DRUM Part's kit beside its Sound (ADR 0305, #949): saved into the
//! library's file and brought back by its load, and seen by the Part's
//! status, so a replace asks before a kit edit is lost. Under `bank2`
//! the host's own door is opened: the kit's records and marks are bank
//! 2's (`kit_library_closed_test` has the closed door).

use chimera_core::dsp::drum::types::{ChokeGroup, Kit, Machine, PadId, Tonal};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::save_at;
use chimera_core::project::{
    LibFile, LibPage, LibraryPart, LoadLink, PageAt, PartActionKind, PartFrom, PartId, PartSource,
    PartStatus, Project, ProjectNote, ProjectSource, Prompt, ReplaceError, ReplaceGuard, Shelf,
    TemplateCrc, list_library, list_projects, load_project, part_actions, part_status,
    read_library_sound, save_library_part, save_library_sound, sound_status,
};
use chimera_core::storage::{BuildStamp, Card, LibId};
use chimera_hal::store::Store;
use chimera_hal::testkit::MemStore;
use chimera_midi::value::U7;

const ST: BuildStamp = BuildStamp::new("0.4.0", "02e440e2", false);
const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn open() {
    #[cfg(feature = "bank2")]
    chimera_core::bank2::DOOR_CELL.open_host();
}

fn card() -> MemStore {
    let mut st = MemStore::new(1);
    st.mount().unwrap();
    st
}

fn init(p: &mut Project, t: TemplateCrc, part: PartId, e: EngineType) {
    let from = PartFrom::Init(e);
    let c = ReplaceGuard::check(p, t, PartSource { part, from }).expect("no prompt");
    p.replace_part(c).unwrap();
}

/// NEW with Part 1 on INIT DRUM.
fn drum_project() -> (Box<Project>, TemplateCrc) {
    open();
    let (mut p, t) = Project::boxed();
    init(&mut p, t, P1, EngineType::Drum);
    (p, t)
}

/// Every pad of `part`'s kit off its default, and TONAL on pad 5.
fn edit_kit(p: &mut Project, part: PartId) {
    for (i, pad) in PadId::ALL.into_iter().enumerate() {
        let k = p.kit_mut(part).pad_mut(pad);
        k.set_machine(if i % 2 == 0 {
            Machine::MetalJingle
        } else {
            Machine::BodyShaker
        });
        k.set_level(U7::new(9 + i as u8).unwrap());
        k.set_choke(ChokeGroup::C);
    }
    let kit = p.kit_mut(part);
    kit.pad_mut(PadId::ALL[3])
        .sound_mut()
        .filter_mut()
        .set_kind(FilterKind::Ladder);
    kit.set_tonal(Tonal::Pad(PadId::ALL[4]));
}

fn listed(st: &mut MemStore, id: LibId) -> LibFile {
    let mut page = Box::new(LibPage::EMPTY);
    list_library(&mut Card::new(), st, Shelf::User, PageAt::First, &mut page)
        .out
        .unwrap();
    let e = page.entries().iter().flatten().find(|e| e.id == id);
    page.file(e.unwrap()).unwrap()
}

/// `p`'s Part `from` saved to the library on `st`.
fn save_part(st: &mut MemStore, p: &mut Project, from: PartId) -> LibFile {
    let name = SoundName::new("KITTEST").unwrap();
    let id = save_library_part(&mut Card::new(), st, p, from, name, &ST)
        .out
        .expect("saved");
    listed(st, id.into())
}

/// `file` loaded into `q`'s Part `to`, the guard's answer given.
fn load(st: &mut MemStore, q: &mut Project, t: TemplateCrc, to: PartId, file: LibFile) {
    let go = match ReplaceGuard::check(q, t, LibraryPart { part: to, file }) {
        Ok(go) => go,
        Err(n) => n.into_pending().anyway(q),
    };
    read_library_sound(&mut Card::new(), st, q, file)
        .out
        .expect("read");
    q.adopt(go, None).expect("adopt");
}

#[test]
fn a_drum_parts_kit_goes_to_the_library_and_back() {
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    let want: Kit = *p.kit(P1);
    assert_ne!(want, Kit::DEFAULT);
    let mut st = card();
    let file = save_part(&mut st, &mut p, P1);

    // Into another Part of the same project.
    load(&mut st, &mut p, t, P2, file);
    assert_eq!(p.part(P2).sound.engine(), EngineType::Drum);
    assert_eq!(*p.kit(P2), want);
    assert_eq!(part_status(&p, P2), PartStatus::Clean);

    // Into a fresh project.
    let (mut q, t) = Project::boxed();
    load(&mut st, &mut q, t, P1, file);
    assert_eq!(*q.kit(P1), want);
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
}

#[test]
fn a_library_load_replaces_the_kit_that_was_there() {
    let (mut p, _) = drum_project();
    edit_kit(&mut p, P1);
    let want = *p.kit(P1);
    let mut st = card();
    let file = save_part(&mut st, &mut p, P1);

    let (mut q, t) = drum_project();
    q.kit_mut(P1)
        .pad_mut(PadId::ALL[0])
        .set_machine(Machine::PlainImpulse);
    load(&mut st, &mut q, t, P1, file);
    assert_eq!(*q.kit(P1), want);
}

/// A DRUM Sound saved with no kit: a Sound's own save, as every library
/// file was before ADR 0305.
fn kitless(st: &mut MemStore) -> LibFile {
    let mut s = Sound::init(EngineType::Drum);
    s.name = SoundName::new("OLD DRUM").unwrap();
    let id = save_library_sound(&mut Card::new(), st, &s, s.name, &ST)
        .out
        .unwrap();
    listed(st, id.into())
}

#[test]
fn a_file_with_no_kit_loads_the_default_kit_not_a_stale_one() {
    open();
    let mut st = card();
    let file = kitless(&mut st);
    let (mut q, t) = Project::boxed();
    edit_kit(&mut q, P1);
    load(&mut st, &mut q, t, P1, file);
    assert_eq!(q.part(P1).sound.engine(), EngineType::Drum);
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
}

#[test]
fn a_scratch_kit_never_outlives_its_read() {
    let (mut p, _) = drum_project();
    edit_kit(&mut p, P1);
    let mut st = card();
    let with_kit = save_part(&mut st, &mut p, P1);
    let without = kitless(&mut st);

    // The kit of the first read is not the second load's.
    let (mut q, t) = Project::boxed();
    read_library_sound(&mut Card::new(), &mut st, &mut q, with_kit)
        .out
        .unwrap();
    load(&mut st, &mut q, t, P1, without);
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
}

#[test]
fn a_sound_of_another_engine_leaves_the_parts_kit() {
    open();
    let (mut p, _) = Project::boxed();
    let mut st = card();
    let modal = {
        let mut s = Sound::init(EngineType::Modal);
        s.name = SoundName::new("A MODAL").unwrap();
        let id = save_library_sound(&mut Card::new(), &mut st, &s, s.name, &ST)
            .out
            .unwrap();
        listed(&mut st, id.into())
    };
    let (mut q, t) = drum_project();
    edit_kit(&mut q, P1);
    let kit = *q.kit(P1);
    load(&mut st, &mut q, t, P1, modal);
    assert_eq!(q.part(P1).sound.engine(), EngineType::Modal);
    assert_eq!(*q.kit(P1), kit);
    // Out of play, the kit asks nothing.
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
    // A non-DRUM Part's save holds no kit: its load brings the default.
    let file = save_part(&mut st, &mut q, P1);
    load(&mut st, &mut p, t, P2, file);
    assert_eq!(*p.kit(P2), Kit::DEFAULT);
}

#[test]
fn a_kit_edit_alone_makes_the_part_edited_and_the_guard_ask() {
    let (mut p, t) = drum_project();
    assert_eq!(part_status(&p, P1), PartStatus::Clean);
    let pad = PadId::ALL[2];
    let was = p.kit(P1).pad(pad).machine();
    p.kit_mut(P1).pad_mut(pad).set_machine(Machine::PlainSine);
    assert_eq!(part_status(&p, P1), PartStatus::Edited);
    // The Sound itself is untouched.
    assert_eq!(sound_status(p.part(P1), p.pool()), PartStatus::Clean);

    let from = PartFrom::Init(EngineType::Modal);
    let asked = ReplaceGuard::check(&p, t, PartSource { part: P1, from });
    assert_eq!(asked.unwrap_err().prompt(), Prompt::SavePartFirst);
    let mut st = card();
    let file = kitless(&mut st);
    let asked = ReplaceGuard::check(&p, t, LibraryPart { part: P1, file });
    assert_eq!(asked.unwrap_err().prompt(), Prompt::SavePartFirst);

    // Turned back, nothing is at risk.
    p.kit_mut(P1).pad_mut(pad).set_machine(was);
    assert_eq!(part_status(&p, P1), PartStatus::Clean);
    assert!(ReplaceGuard::check(&p, t, PartSource { part: P1, from }).is_ok());
}

#[test]
fn a_kit_edit_after_the_confirmation_refuses_the_replace() {
    let (mut p, t) = drum_project();
    let from = PartFrom::Init(EngineType::Modal);
    let go = ReplaceGuard::check(&p, t, PartSource { part: P1, from }).unwrap();
    p.kit_mut(P1)
        .pad_mut(PadId::ALL[0])
        .set_level(U7::new(1).unwrap());
    assert_eq!(p.replace_part(go), Err(ReplaceError::Changed));
    assert_eq!(p.part(P1).sound.engine(), EngineType::Drum);
}

#[test]
fn a_library_save_holds_the_kit_a_slot_save_does_not() {
    let (mut p, _) = drum_project();
    edit_kit(&mut p, P1);
    assert_eq!(part_status(&p, P1), PartStatus::Edited);

    // To a slot: the Sound is saved, the kit is in no slot, so the Part
    // still asks (#949's follow-up).
    let new_slot = part_actions(&p, P1)
        .iter()
        .find(|a| matches!(a.kind(), PartActionKind::NewSlot(_)))
        .unwrap();
    p.apply_part_action(new_slot).unwrap();
    assert_eq!(sound_status(p.part(P1), p.pool()), PartStatus::Clean);
    assert_eq!(part_status(&p, P1), PartStatus::Edited);

    // To the library: the kit is in the file.
    let mut st = card();
    save_part(&mut st, &mut p, P1);
    assert_eq!(part_status(&p, P1), PartStatus::Clean);
}

#[test]
fn a_slot_or_init_load_leaves_the_kit_on_the_part() {
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    let kit = *p.kit(P1);
    let from = PartFrom::Init(EngineType::Drum);
    let go = match ReplaceGuard::check(&p, t, PartSource { part: P1, from }) {
        Ok(_) => panic!("an edited kit asks"),
        Err(n) => n.into_pending().anyway(&p),
    };
    p.replace_part(go).unwrap();
    assert_eq!(*p.kit(P1), kit);
    assert_eq!(part_status(&p, P1), PartStatus::Edited);
}

#[test]
fn a_project_load_and_new_mark_the_kit_as_loaded() {
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    let kit = *p.kit(P1);
    let mut st = card();
    let mut card = Card::new();
    let note = save_at(&mut card, &mut st, &mut p, 1);
    assert!(matches!(note, ProjectNote::Saved(_)), "{note:?}");

    let (mut q, tq) = Project::boxed();
    let mut found = None;
    let listed = list_projects(&mut card, &mut st, &mut |e| found = Some(e));
    assert!(listed.note.is_none(), "{:?}", listed.note);
    let e = found.unwrap();
    let src = ProjectSource::File {
        id: e.id,
        vol: e.vol,
    };
    let go = ReplaceGuard::check(&q, tq, src).unwrap();
    let out = load_project(&mut card, &mut st, &mut q, go, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    assert_eq!(*q.kit(P1), kit);
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
    q.kit_mut(P1).set_tonal(Tonal::Off);
    assert_eq!(part_status(&q, P1), PartStatus::Edited);

    // + NEW: the default kit, marked so.
    let go = match ReplaceGuard::check(&q, tq, ProjectSource::New) {
        Ok(go) => go,
        Err(n) => n.into_pending().anyway(&q),
    };
    let out = load_project(&mut card, &mut st, &mut q, go, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    init(&mut q, t, P1, EngineType::Drum);
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
}
