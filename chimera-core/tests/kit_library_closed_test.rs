//! A DRUM Part's kit with bank 2's door closed (ADR 0305, `KitDoor`),
//! under `bank2` on a host. Closed is a value its callers handle: a DRUM
//! Part's library save is refused whole (its file would hold no kit),
//! any other Part's goes through, a load reads no kit and leaves the
//! Part's, and the guard reads no kit. Open again, the same file's load
//! brings its kit. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_core::bank2::DOOR_CELL;
use chimera_core::dsp::drum::types::{Kit, Machine, PadId};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{
    LibFile, LibNote, LibPage, LibraryPart, PageAt, PartFrom, PartId, PartSource, Project, ReadKit,
    ReplaceGuard, Shelf, TemplateCrc, list_library, read_library_sound, save_library_part,
    save_library_sound,
};
use chimera_core::storage::{BuildStamp, Card};
use chimera_hal::store::{Dir, Store};
use chimera_hal::testkit::MemStore;

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const ST: BuildStamp = BuildStamp::new("0.4.0", "02e440e2", false);

fn sounds(st: &mut MemStore) -> usize {
    let v = st.mount().unwrap();
    let mut n = 0;
    let _ = st.list(v, Dir::Sounds, &mut |_, _| n += 1);
    n
}

fn load(
    st: &mut MemStore,
    p: &mut Project,
    t: TemplateCrc,
    part: PartId,
    file: LibFile,
) -> ReadKit {
    let c = ReplaceGuard::check(p, t, LibraryPart { part, file }).unwrap();
    let c = ReplaceGuard::kit(p, t, c).expect("closed, no kit is read; open, it is the default");
    let r = read_library_sound(&mut Card::new(), st, p, c).out.unwrap();
    let kit = r.kit();
    p.adopt(r, None).unwrap();
    kit
}

#[test]
fn closed_is_handled_not_silent() {
    // Built open: a project built closed holds its Sounds as a closed
    // door wrote them.
    DOOR_CELL.open_host();
    let (mut p, t) = Project::boxed();
    let from = PartFrom::Init(EngineType::Drum);
    let c = ReplaceGuard::check(&p, t, PartSource { part: P1, from }).unwrap();
    p.replace_part(c).unwrap();
    p.kit_mut(P1)
        .pad_mut(PadId::ALL[0])
        .set_machine(Machine::PlainSine);
    let edited = *p.kit(P1);
    let mut st = MemStore::new(1);
    st.mount().unwrap();
    let name = SoundName::new("CLOSED").unwrap();

    DOOR_CELL.close();
    // A DRUM Part's save: refused, and nothing left on the card.
    let out = save_library_part(&mut Card::new(), &mut st, &p, P1, name, &ST).out;
    assert_eq!(out.unwrap_err(), LibNote::KitClosed);
    assert_eq!(
        LibNote::KitClosed.message(),
        "KIT NOT SAVED \u{b7} NEEDS UPDATE"
    );
    assert_eq!(sounds(&mut st), 0);
    // Another engine's Part: saved, with no kit to mark.
    let saved = save_library_part(&mut Card::new(), &mut st, &p, P2, name, &ST)
        .out
        .unwrap();
    assert!(saved.kit().is_none());

    // A DRUM Sound's file, read closed: no kit read, the Part's kept.
    DOOR_CELL.open_host();
    let (mut src, _) = Project::boxed();
    let c = ReplaceGuard::check(&src, t, PartSource { part: P1, from }).unwrap();
    src.replace_part(c).unwrap();
    let id = save_library_part(&mut Card::new(), &mut st, &src, P1, name, &ST)
        .out
        .unwrap()
        .id();
    let mut page = Box::new(LibPage::EMPTY);
    list_library(
        &mut Card::new(),
        &mut st,
        Shelf::User,
        PageAt::First,
        &mut page,
    )
    .out
    .unwrap();
    let e = page.entries().iter().flatten().find(|e| e.id == id.into());
    let file = page.file(e.unwrap()).unwrap();
    DOOR_CELL.close();
    assert_eq!(load(&mut st, &mut p, t, P1, file), ReadKit::Closed);
    assert_eq!(*p.kit(P1), edited);

    // Open: the file's kit (the default) lands, after its question.
    DOOR_CELL.open_host();
    let c = ReplaceGuard::check(&p, t, LibraryPart { part: P1, file }).unwrap();
    let c = ReplaceGuard::kit(&p, t, c)
        .unwrap_err()
        .into_pending()
        .anyway(&p);
    let r = read_library_sound(&mut Card::new(), &mut st, &mut p, c)
        .out
        .unwrap();
    p.adopt(r, None).unwrap();
    assert_eq!(*p.kit(P1), Kit::DEFAULT);
    let _ = save_library_sound(
        &mut Card::new(),
        &mut st,
        &Sound::init(EngineType::Algo),
        name,
        &ST,
    );
}
