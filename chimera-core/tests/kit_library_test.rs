//! A DRUM Part's kit beside its Sound (ADR 0305, #949): saved into the
//! library's file and brought back by its load, and weighed by a library
//! load's guard, the one replace that takes a kit. Under `bank2` the
//! host's own door is opened: the kit's records and marks are bank 2's
//! (`kit_library_closed_test` has the closed door).

mod common;

use std::ops::ControlFlow;

use chimera_core::dsp::drum::sound::PadSound;
use chimera_core::dsp::drum::types::{ChokeGroup, Kit, Machine, PadId, Tonal};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::save_at;
use chimera_core::project::{
    AdoptError, Confirmed, Edits, FileKit, LibFile, LibNote, LibPage, LibraryKit, LibraryPart,
    LoadLink, PageAt, PartActionKind, PartFrom, PartId, PartSource, PartStatus, Project,
    ProjectNote, ProjectSource, Prompt, ReadKit, ReadPart, ReplaceGuard, Shelf, SoundStatus,
    TemplateCrc, list_library, list_projects, load_project, part_actions, part_status,
    read_library_sound, save_library_part, save_library_sound, sound_status,
};
use chimera_core::storage::{
    BuildStamp, Card, FileError, FileKind, Generation, Header, LibId, RecordTag,
    encode_library_sound, write_file,
};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store};
use chimera_hal::testkit::MemStore;
use chimera_midi::value::U7;
use common::codec_util::{VecSink, record_offsets};

const ST: BuildStamp = BuildStamp::new("0.4.0", "02e440e2", false);
const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const OTHERS: [EngineType; 4] = [
    EngineType::Algo,
    EngineType::Modal,
    EngineType::Tape,
    EngineType::Va,
];

fn open() {
    #[cfg(feature = "bank2")]
    chimera_core::bank2::DOOR_CELL.open_host();
}

fn card() -> MemStore {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    st
}

/// Part `part` to INIT `e`, whatever the guard asks.
fn init(p: &mut Project, t: TemplateCrc, part: PartId, e: EngineType) {
    let src = PartSource {
        part,
        from: PartFrom::Init(e),
    };
    let c = match ReplaceGuard::check(p, t, src) {
        Ok(c) => c,
        Err(n) => n.into_pending().anyway(p),
    };
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

/// `p`'s Part `from` saved to the library on `st`, and the Part told, as
/// the UI does.
fn save_part(st: &mut MemStore, p: &mut Project, from: PartId) -> LibFile {
    let name = SoundName::new("KITTEST").unwrap();
    let saved = save_library_part(&mut Card::new(), st, p, from, name, &ST)
        .out
        .expect("saved");
    let id = saved.id();
    p.library_saved(saved);
    listed(st, id.into())
}

/// `e`'s INIT Sound saved alone, with no kit: as every library file was
/// before ADR 0305.
fn sound_file(st: &mut MemStore, e: EngineType) -> LibFile {
    let mut s = Sound::init(e);
    s.name = SoundName::new("A SOUND").unwrap();
    let id = save_library_sound(&mut Card::new(), st, &s, s.name, &ST)
        .out
        .unwrap();
    listed(st, id.into())
}

/// Both questions of a load of `file` into `to`: the confirmation, or
/// the first prompt.
fn ask(
    q: &Project,
    t: TemplateCrc,
    to: PartId,
    file: LibFile,
) -> Result<Confirmed<LibraryKit>, Prompt> {
    let c = ReplaceGuard::check(q, t, LibraryPart { part: to, file }).map_err(|n| n.prompt())?;
    ReplaceGuard::kit(q, t, c).map_err(|n| n.prompt())
}

/// Both questions, each answered REPLACE if asked.
fn go(q: &Project, t: TemplateCrc, to: PartId, file: LibFile) -> Confirmed<LibraryKit> {
    let c = match ReplaceGuard::check(q, t, LibraryPart { part: to, file }) {
        Ok(c) => c,
        Err(n) => n.into_pending().anyway(q),
    };
    match ReplaceGuard::kit(q, t, c) {
        Ok(c) => c,
        Err(n) => n.into_pending().anyway(q),
    }
}

fn read(
    st: &mut MemStore,
    q: &mut Project,
    t: TemplateCrc,
    to: PartId,
    file: LibFile,
) -> Result<ReadPart, LibNote> {
    let c = go(q, t, to, file);
    read_library_sound(&mut Card::new(), st, q, c).out
}

/// `file` loaded into `q`'s Part `to`, every question answered REPLACE.
fn load(st: &mut MemStore, q: &mut Project, t: TemplateCrc, to: PartId, file: LibFile) {
    let r = read(st, q, t, to, file).expect("read");
    q.adopt(r, None).expect("adopt");
}

struct All(Vec<u8>);

impl ReadSink for All {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(b);
        ControlFlow::Continue(())
    }
}

/// The record tags of `file`'s A side, in order.
fn tags(st: &mut MemStore, file: LibFile) -> Vec<u16> {
    let v = st.mount().unwrap();
    let f = FileName::new(Dir::Sounds, &file.id.stem(), b"A").unwrap();
    let mut all = All(Vec::new());
    st.read(v, f, &mut all).unwrap();
    record_offsets(&all.0)
        .into_iter()
        .map(|o| u16::from_le_bytes([all.0[o], all.0[o + 1]]))
        .collect()
}

const KIT: u16 = 0x0024;
const KIT_SOUND: u16 = 0x0025;

#[test]
fn a_drum_parts_kit_goes_to_the_library_and_back() {
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    let want: Kit = *p.kit(P1);
    assert_ne!(want, Kit::DEFAULT);
    let mut st = card();
    let file = save_part(&mut st, &mut p, P1);
    let tags = tags(&mut st, file);
    assert_eq!(tags.iter().filter(|&&t| t == KIT).count(), 1);
    assert_eq!(tags.iter().filter(|&&t| t == KIT_SOUND).count(), 12);

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
    assert_eq!(ask(&q, t, P1, file).unwrap_err(), Prompt::KitUnsaved);
    load(&mut st, &mut q, t, P1, file);
    assert_eq!(*q.kit(P1), want);
}

#[test]
fn a_drum_file_with_no_kit_loads_the_default_kit_not_a_stale_one() {
    open();
    let mut st = card();
    let file = sound_file(&mut st, EngineType::Drum);
    let (mut q, t) = Project::boxed();
    edit_kit(&mut q, P1);
    let r = read(&mut st, &mut q, t, P1, file).unwrap();
    assert!(matches!(r.kit(), ReadKit::Found(f) if f.kit() == FileKit::Absent));
    q.adopt(r, None).unwrap();
    assert_eq!(q.part(P1).sound.engine(), EngineType::Drum);
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
    assert_eq!(part_status(&q, P1), PartStatus::Clean);
}

/// T3: a read is adopted once, and only while the scratch is its own.
#[test]
fn an_earlier_reads_kit_never_lands() {
    let (mut p, _) = drum_project();
    edit_kit(&mut p, P1);
    let mut st = card();
    let with_kit = save_part(&mut st, &mut p, P1);
    let without = sound_file(&mut st, EngineType::Drum);

    let (mut q, t) = Project::boxed();
    let first = read(&mut st, &mut q, t, P1, with_kit).unwrap();
    let second = read(&mut st, &mut q, t, P1, without).unwrap();
    // The scratch is the second read's now.
    assert_eq!(q.adopt(first, None), Err(AdoptError::Changed));
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
    q.adopt(second, None).unwrap();
    assert_eq!(*q.kit(P1), Kit::DEFAULT);
}

#[test]
fn a_sound_of_any_other_engine_leaves_the_parts_kit_and_asks_nothing() {
    for e in OTHERS {
        let mut st = card();
        let file = sound_file(&mut st, e);
        let (mut q, t) = drum_project();
        edit_kit(&mut q, P1);
        let kit = *q.kit(P1);
        // The listing knows the file is not DRUM's: the kit is not at risk.
        assert!(ask(&q, t, P1, file).is_ok(), "{e:?}");
        load(&mut st, &mut q, t, P1, file);
        assert_eq!(q.part(P1).sound.engine(), e);
        assert_eq!(*q.kit(P1), kit, "{e:?}");
        // Out of play, the kit is no edit of the Part's.
        assert_eq!(part_status(&q, P1), PartStatus::Clean, "{e:?}");
        // The Part's own save, on this engine, writes no kit record.
        let own = save_part(&mut st, &mut q, P1);
        let tags = tags(&mut st, own);
        assert!(!tags.contains(&KIT) && !tags.contains(&KIT_SOUND), "{e:?}");
    }
}

/// C-b: a kit edited on DRUM, the Part then on another engine (the kit
/// kept, dormant): a DRUM library load would replace it, so it asks, and
/// an edit after the answer refuses the adopt.
#[test]
fn a_dormant_kit_asks_before_a_drum_library_load() {
    for e in OTHERS {
        let (mut src, _) = drum_project();
        let mut st = card();
        let file = save_part(&mut st, &mut src, P1);
        let (mut p, t) = drum_project();
        edit_kit(&mut p, P1);
        let kit = *p.kit(P1);
        init(&mut p, t, P1, e);
        assert_eq!(*p.kit(P1), kit, "{e:?}: the switch keeps the kit");
        assert_eq!(
            ask(&p, t, P1, file).unwrap_err(),
            Prompt::KitUnsaved,
            "{e:?}"
        );

        let r = read(&mut st, &mut p, t, P1, file).unwrap();
        p.kit_mut(P1)
            .pad_mut(PadId::ALL[0])
            .set_level(U7::new(1).unwrap());
        assert_eq!(p.adopt(r, None), Err(AdoptError::Changed), "{e:?}");
        assert_eq!(p.part(P1).sound.engine(), e);
    }
}

/// The witness covers the kit in every state: a default kit edited after
/// the confirmation refuses too.
#[test]
fn a_kit_edit_after_the_confirmation_refuses_the_adopt() {
    let (mut src, _) = drum_project();
    let mut st = card();
    let file = save_part(&mut st, &mut src, P1);
    let (mut p, t) = drum_project();
    assert!(ask(&p, t, P1, file).is_ok(), "a default kit asks nothing");
    let r = read(&mut st, &mut p, t, P1, file).unwrap();
    p.kit_mut(P1)
        .pad_mut(PadId::ALL[0])
        .set_machine(Machine::PlainSine);
    let kit = *p.kit(P1);
    assert_eq!(p.adopt(r, None), Err(AdoptError::Changed));
    assert_eq!(*p.kit(P1), kit);
}

/// I-b: a kit edited, the project saved and loaded again: the project
/// file is its only copy, so a DRUM library load still asks.
#[test]
fn a_kit_from_a_project_load_asks_before_a_drum_library_load() {
    let (mut src, _) = drum_project();
    let mut st = card();
    let file = save_part(&mut st, &mut src, P1);
    let (mut p, _) = drum_project();
    edit_kit(&mut p, P1);
    let kit = *p.kit(P1);
    let mut card = Card::new();
    let note = save_at(&mut card, &mut st, &mut p, 1);
    assert!(matches!(note, ProjectNote::Saved(_)), "{note:?}");
    let (mut q, tq) = Project::boxed();
    let mut found = None;
    let listed = list_projects(&mut card, &mut st, &mut |e| found = Some(e));
    assert!(listed.note.is_none(), "{:?}", listed.note);
    let e = found.unwrap();
    let from = ProjectSource::File {
        id: e.id,
        vol: e.vol,
    };
    let c = ReplaceGuard::check(&q, tq, from).unwrap();
    let out = load_project(&mut card, &mut st, &mut q, c, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    assert_eq!(*q.kit(P1), kit);
    assert_eq!(part_status(&q, P1), PartStatus::Edited(Edits::Kit));
    assert_eq!(ask(&q, tq, P1, file).unwrap_err(), Prompt::KitUnsaved);
}

/// The kit is weighed by a library load alone: a slot or INIT load, a
/// reload and a program change leave it on the Part, so they never ask
/// about it (and SAVE PART FIRST never saves a slot for it: I-a).
#[test]
fn a_kit_edit_never_asks_before_a_slot_or_init_load() {
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    let kit = *p.kit(P1);
    assert_eq!(sound_status(p.part(P1), p.pool()), SoundStatus::Clean);
    assert_eq!(part_status(&p, P1), PartStatus::Edited(Edits::Kit));
    for e in OTHERS.into_iter().chain([EngineType::Drum]) {
        let src = PartSource {
            part: P1,
            from: PartFrom::Init(e),
        };
        let c = ReplaceGuard::check(&p, t, src).expect("no prompt");
        // Nor does a kit edit after the confirmation refuse it.
        p.kit_mut(P1)
            .pad_mut(PadId::ALL[0])
            .set_choke(ChokeGroup::A);
        p.kit_mut(P1)
            .pad_mut(PadId::ALL[0])
            .set_choke(ChokeGroup::C);
        p.replace_part(c).unwrap();
        assert_eq!(*p.kit(P1), kit, "{e:?}");
    }
}

/// T2: the one total match.
#[test]
fn a_parts_status_names_what_is_edited() {
    use chimera_core::project::KitState::{Edited, Kept};
    use chimera_core::project::SlotId;
    let s = SlotId::ALL[3];
    let of = PartStatus::of;
    assert_eq!(of(SoundStatus::Clean, Kept), PartStatus::Clean);
    assert_eq!(of(SoundStatus::Stale(s), Kept), PartStatus::Stale(s));
    assert_eq!(
        of(SoundStatus::Edited, Kept),
        PartStatus::Edited(Edits::Sound)
    );
    assert_eq!(
        of(SoundStatus::Clean, Edited),
        PartStatus::Edited(Edits::Kit)
    );
    assert_eq!(
        of(SoundStatus::Stale(s), Edited),
        PartStatus::Edited(Edits::Kit)
    );
    assert_eq!(
        of(SoundStatus::Edited, Edited),
        PartStatus::Edited(Edits::Both)
    );

    let (mut p, _) = drum_project();
    assert_eq!(part_status(&p, P1), PartStatus::Clean);
    let pad = PadId::ALL[2];
    let was = p.kit(P1).pad(pad).machine();
    p.kit_mut(P1).pad_mut(pad).set_machine(Machine::PlainSine);
    assert_eq!(part_status(&p, P1), PartStatus::Edited(Edits::Kit));
    p.edit_part(P1).sound.params.out.volume = 0.1;
    assert_eq!(part_status(&p, P1), PartStatus::Edited(Edits::Both));
    // Turned back to the default, the kit is no edit.
    p.kit_mut(P1).pad_mut(pad).set_machine(was);
    assert_eq!(part_status(&p, P1), PartStatus::Edited(Edits::Sound));
}

/// T7: only a save's `SavedPart` marks the kit as the library's, and a
/// slot save never does.
#[test]
fn only_a_library_save_holds_the_kit() {
    let (mut src, _) = drum_project();
    let mut st = card();
    let file = save_part(&mut st, &mut src, P1);
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    assert_eq!(ask(&p, t, P1, file).unwrap_err(), Prompt::KitUnsaved);

    // To a slot: the Sound's status only.
    let new_slot = part_actions(&p, P1)
        .iter()
        .find(|a| matches!(a.kind(), PartActionKind::NewSlot(_)))
        .unwrap();
    p.apply_part_action(new_slot).unwrap();
    assert_eq!(ask(&p, t, P1, file).unwrap_err(), Prompt::KitUnsaved);

    // To the library: saved, and still asking until the Part is told.
    let name = SoundName::new("MINE").unwrap();
    let saved = save_library_part(&mut Card::new(), &mut st, &p, P1, name, &ST)
        .out
        .unwrap();
    assert!(saved.kit().is_some());
    assert_eq!(ask(&p, t, P1, file).unwrap_err(), Prompt::KitUnsaved);
    p.library_saved(saved);
    assert!(ask(&p, t, P1, file).is_ok());
    assert_eq!(part_status(&p, P1), PartStatus::Clean);
    // Edited again, it is in no file again.
    p.kit_mut(P1).set_tonal(Tonal::Off);
    assert_eq!(ask(&p, t, P1, file).unwrap_err(), Prompt::KitUnsaved);
}

/// The Sound's question comes first, then the kit's.
#[test]
fn an_edited_sound_asks_first_then_the_kit() {
    let (mut src, _) = drum_project();
    let mut st = card();
    let file = save_part(&mut st, &mut src, P1);
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    p.edit_part(P1).sound.params.out.volume = 0.1;
    let n = ReplaceGuard::check(&p, t, LibraryPart { part: P1, file }).unwrap_err();
    assert_eq!(n.prompt(), Prompt::SavePartFirst);
    let c = n.into_pending().anyway(&p);
    let n = ReplaceGuard::kit(&p, t, c).unwrap_err();
    assert_eq!(n.prompt(), Prompt::KitUnsaved);
    // A Sound edit between the two answers refuses the adopt.
    let c = n.into_pending().anyway(&p);
    let r = read_library_sound(&mut Card::new(), &mut st, &mut p, c)
        .out
        .unwrap();
    p.edit_part(P1).sound.params.out.volume = 0.2;
    assert_eq!(p.adopt(r, None), Err(AdoptError::Changed));
}

#[test]
fn new_brings_the_default_kit() {
    let (mut p, t) = drum_project();
    edit_kit(&mut p, P1);
    let mut st = card();
    let mut card = Card::new();
    let c = match ReplaceGuard::check(&p, t, ProjectSource::New) {
        Ok(c) => c,
        Err(n) => n.into_pending().anyway(&p),
    };
    let out = load_project(&mut card, &mut st, &mut p, c, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    init(&mut p, t, P1, EngineType::Drum);
    assert_eq!(*p.kit(P1), Kit::DEFAULT);
    assert_eq!(part_status(&p, P1), PartStatus::Clean);
}

/// A library file of `s`'s records, then `extra`, as `S0000001.A`, listed.
fn raw(st: &mut MemStore, s: &Sound, extra: &[(RecordTag, Vec<u8>)], first: bool) -> LibFile {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| {
        if first {
            for (t, p) in extra {
                w.put(*t, p)?;
            }
        }
        encode_library_sound(s, &ST, None, w)?;
        if !first {
            for (t, p) in extra {
                w.put(*t, p)?;
            }
        }
        Ok(())
    })
    .unwrap();
    let id = LibId::new(1).unwrap();
    let v = st.mount().unwrap();
    let f = FileName::new(Dir::Sounds, &id.stem(), b"A").unwrap();
    st.write(v, f, &mut |b: &mut dyn ByteSink| b.put(&sink.0))
        .unwrap();
    listed(st, id)
}

fn kit_rec() -> (RecordTag, Vec<u8>) {
    (RecordTag::Kit, vec![0; 157])
}

fn pad_rec(pad: u8) -> (RecordTag, Vec<u8>) {
    let mut b = vec![0; 58];
    b[0] = pad;
    (RecordTag::KitSound, b)
}

/// What a read of `s` with `extra` records says.
fn read_raw(s: &Sound, extra: &[(RecordTag, Vec<u8>)], first: bool) -> Result<ReadPart, LibNote> {
    open();
    let mut st = card();
    let file = raw(&mut st, s, extra, first);
    let (mut q, t) = Project::boxed();
    edit_kit(&mut q, P1);
    let kit = *q.kit(P1);
    let r = read(&mut st, &mut q, t, P1, file);
    assert_eq!(*q.kit(P1), kit, "a read lands nothing");
    r
}

fn named(e: EngineType) -> Sound {
    let mut s = Sound::init(e);
    s.name = SoundName::new("RAW").unwrap();
    s
}

/// T5, T6: every kit record is checked in pass 1, by the project's rules.
#[test]
fn a_malformed_kit_refuses_the_file() {
    let drum = named(EngineType::Drum);
    let refused =
        |extra: &[(RecordTag, Vec<u8>)], first| read_raw(&drum, extra, first).unwrap_err();
    let (bounds, corrupt) = (
        LibNote::File(FileError::Bounds),
        LibNote::File(FileError::Corrupt),
    );
    // The wrong length.
    assert_eq!(refused(&[(RecordTag::Kit, vec![0; 10])], false), bounds);
    assert_eq!(
        refused(&[kit_rec(), (RecordTag::KitSound, vec![0; 9])], false),
        bounds
    );
    // A pad there is none of.
    assert_eq!(refused(&[kit_rec(), pad_rec(12)], false), bounds);
    // A second `Kit`, a pad's second `KitSound`.
    assert_eq!(refused(&[kit_rec(), kit_rec()], false), corrupt);
    assert_eq!(
        refused(&[kit_rec(), pad_rec(3), pad_rec(3)], false),
        corrupt
    );
    // A kit in part: pad sounds with no `Kit`.
    assert_eq!(refused(&[pad_rec(0), pad_rec(1)], false), corrupt);
    // Before `Engine`.
    assert_eq!(refused(&[kit_rec()], true), corrupt);
    // In a file of any other engine.
    for e in OTHERS {
        let s = named(e);
        assert_eq!(
            read_raw(&s, &[kit_rec()], false).unwrap_err(),
            corrupt,
            "{e:?}"
        );
        assert_eq!(
            read_raw(&s, &[pad_rec(0)], false).unwrap_err(),
            corrupt,
            "{e:?}"
        );
    }
}

/// ADR 0269: a `Kit` with some pads' sounds missing loads, and those
/// pads take the Sound's FILTER and AMP. Its coded values this firmware
/// reads as others count as a skip.
#[test]
fn a_kit_with_pad_sounds_missing_takes_the_sounds() {
    let drum = named(EngineType::Drum);
    open();
    let mut st = card();
    let file = raw(&mut st, &drum, &[kit_rec(), pad_rec(2)], false);
    let (mut q, t) = Project::boxed();
    let r = read(&mut st, &mut q, t, P1, file).unwrap();
    let ReadKit::Found(found) = r.kit() else {
        panic!("the door is open");
    };
    assert!(matches!(found.kit(), FileKit::Whole(own) if own.contains(PadId::ALL[2])));
    assert!(found.lossy(), "all-zero pads are not what a save writes");
    assert!(r.read().skips > 0);
    q.adopt(r, None).unwrap();
    let from_sound = PadSound::from_sound(&q.part(P1).sound.params);
    assert_eq!(*q.kit(P1).pad(PadId::ALL[0]).sound(), from_sound);
    assert_ne!(*q.kit(P1).pad(PadId::ALL[2]).sound(), from_sound);
    // The library holds this kit: it asks nothing more.
    assert!(ask(&q, t, P1, file).is_ok());
}
