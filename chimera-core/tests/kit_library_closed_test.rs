//! A DRUM Part's kit with bank 2's door closed (ADR 0305, `KitDoor`),
//! under `bank2` on a host: a library save writes no kit, a load reads
//! none and leaves the Part's, and a kit edit is not seen, as before the
//! ADR. Open again, the same file's load brings the default kit. One
//! test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_core::bank2::DOOR_CELL;
use chimera_core::dsp::drum::types::{Kit, Machine, PadId};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::project::{
    LibPage, LibraryPart, PageAt, PartFrom, PartId, PartSource, PartStatus, Project, ReplaceGuard,
    Shelf, list_library, part_status, read_library_sound, save_library_part,
};
use chimera_core::storage::{BuildStamp, Card};
use chimera_hal::store::Store;
use chimera_hal::testkit::MemStore;

const P1: PartId = PartId::ALL[0];

#[test]
fn closed_no_kit_is_written_read_or_seen() {
    DOOR_CELL.close();
    let (mut p, t) = Project::boxed();
    let from = PartFrom::Init(EngineType::Drum);
    let c = ReplaceGuard::check(&p, t, PartSource { part: P1, from }).unwrap();
    p.replace_part(c).unwrap();
    p.kit_mut(P1)
        .pad_mut(PadId::ALL[0])
        .set_machine(Machine::PlainSine);
    let edited = *p.kit(P1);
    assert_eq!(part_status(&p, P1), PartStatus::Clean);

    let mut st = MemStore::new(1);
    st.mount().unwrap();
    let stamp = BuildStamp::new("0.4.0", "02e440e2", false);
    let name = SoundName::new("CLOSED").unwrap();
    let id = save_library_part(&mut Card::new(), &mut st, &p, P1, name, &stamp)
        .out
        .unwrap();
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

    // Closed: the load leaves the target's kit as it was.
    let mut load = |p: &mut Project, part| {
        let go = ReplaceGuard::check(p, t, LibraryPart { part, file }).unwrap();
        read_library_sound(&mut Card::new(), &mut st, p, file)
            .out
            .unwrap();
        p.adopt(go, None).unwrap();
    };
    load(&mut p, P1);
    assert_eq!(*p.kit(P1), edited);

    // Open: the file holds no kit, so the default lands. A project built
    // open: one built closed holds its Sounds as a closed door wrote them.
    DOOR_CELL.open_host();
    let (mut q, _) = Project::boxed();
    q.kit_mut(P1)
        .pad_mut(PadId::ALL[0])
        .set_machine(Machine::PlainSine);
    load(&mut q, P1);
    assert_eq!(q.part(P1).sound.engine(), EngineType::Drum);
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
}
