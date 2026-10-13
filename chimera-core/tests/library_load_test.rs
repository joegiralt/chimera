//! Loading a library Sound (library spec § Load): the re-peek, the scratch
//! read, the budget and `Project::adopt`, which commits the source, the
//! slot and the Part in one step or changes nothing.

mod common;

use std::ops::ControlFlow;
use std::sync::OnceLock;

use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Shape};
use chimera_core::factory;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::Cost;
use chimera_core::project::test_support::{scratch_of, set_free_to};
use chimera_core::project::{
    Absence, AdoptError, Adopted, Confirmed, LibFile, LibNote, LibPage, LibRead, LibraryPart,
    PageAt, PartId, PartStatus, Project, ReplaceError, ReplaceGuard, SourceNeed, SourceRef, Staged,
    TemplateCrc, list_library, part_status, read_library_sound, save_library_sound,
};
use chimera_core::storage::{
    BuildStamp, Card, FileError, FileKind, Generation, HEADER_LEN, Header, LibId, RecordTag,
    SourceHead, TRAILER_LEN, WholeShape, encode_library_sound, encode_sound, source_crc,
    source_file, write_file, write_source,
};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store};
use chimera_hal::testkit::MemStore;
use common::codec_util::{VecSink, fix_crc};
use common::tape::{held_plain, sine};

const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);
const P1: PartId = PartId::ALL[0];

/// Factory Sound `i` renamed, so NEW's copy of it doesn't dedupe.
fn sound(i: usize) -> Sound {
    let mut s = factory::factory_sound(i).unwrap();
    s.name = SoundName::new(&format!("LIB {i}")).unwrap();
    s
}

/// NEW's template: a Part's guard never reads it.
fn template() -> TemplateCrc {
    static T: OnceLock<TemplateCrc> = OnceLock::new();
    *T.get_or_init(|| Project::boxed().1)
}

fn file_name(id: LibId, ext: &str) -> FileName {
    FileName::new(Dir::Sounds, &id.stem(), ext.as_bytes()).unwrap()
}

fn card() -> MemStore {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    st
}

fn save_named<S: Store>(st: &mut S, s: &Sound, name: &str) -> LibId {
    save_library_sound(&mut Card::new(), st, s, SoundName::new(name).unwrap(), &ST)
        .out
        .unwrap()
}

/// The listed row of `id`.
fn listed(st: &mut MemStore, id: LibId) -> LibFile {
    let mut p = Box::new(LibPage::EMPTY);
    list_library(&mut Card::new(), st, PageAt::Newest, &mut p)
        .out
        .unwrap();
    let e = p.entries().iter().flatten().find(|e| e.id == id).unwrap();
    p.file(e).unwrap()
}

/// `s` saved and listed, beside a NEW project.
fn saved(s: Sound) -> (Box<Project>, MemStore, LibFile) {
    let mut st = card();
    let id = save_named(&mut st, &s, s.name.as_str());
    let file = listed(&mut st, id);
    (Project::boxed().0, st, file)
}

fn saved_with_full_arena(s: Sound) -> (Box<Project>, MemStore, LibFile) {
    let (mut p, st, file) = saved(s);
    set_free_to(&mut p, Cost::default());
    (p, st, file)
}

/// `bytes` as `S0000001.A`, listed: the project, the card, the row, the
/// file's length.
fn saved_bytes(bytes: Vec<u8>) -> (Box<Project>, MemStore, LibFile, u32) {
    let mut st = card();
    let id = LibId::new(1).unwrap();
    put(&mut st, file_name(id, "A"), &bytes);
    let file = listed(&mut st, id);
    (Project::boxed().0, st, file, bytes.len() as u32)
}

fn read(st: &mut MemStore, p: &mut Project, file: LibFile) -> Result<LibRead, LibNote> {
    read_library_sound(&mut Card::new(), st, p, file).out
}

/// The guard on a Clean Part, no prompt.
fn confirmed(p: &Project, part: PartId, file: LibFile) -> Confirmed<LibraryPart> {
    ReplaceGuard::check(p, template(), LibraryPart { part, file }).unwrap()
}

fn adopt_twice(s: Sound) -> (Adopted, Adopted) {
    let (mut p, mut st, file) = saved(s);
    read(&mut st, &mut p, file).unwrap();
    let first = p.adopt(confirmed(&p, P1, file), None).unwrap();
    read(&mut st, &mut p, file).unwrap();
    let second = p.adopt(confirmed(&p, P1, file), None).unwrap();
    (first, second)
}

const FRAMES: u32 = 9_600;

/// A Plain sine source's head and samples.
fn source() -> (SourceHead, Vec<u8>) {
    let data = held_plain(&sine(48_000, 100.0, FRAMES), 48_000)[DESC_LEN..].to_vec();
    let h = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames: FRAMES,
            rate: 48_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("SINE").unwrap(),
    };
    (h, data)
}

/// A TAPE Sound on `source()` saved, its `.SRC` on the card when `src`.
fn tape_saved(src: bool) -> (Box<Project>, MemStore, LibFile) {
    let (h, data) = source();
    let mut s = Sound::init(EngineType::Tape);
    s.pick_source(&h, None);
    let mut st = card();
    if src {
        Card::new()
            .run(&mut st, |s, r| write_source(s, r, &h, &data))
            .unwrap()
            .result
            .unwrap();
    }
    let id = save_named(&mut st, &s, "TAPE");
    let file = listed(&mut st, id);
    (Project::boxed().0, st, file)
}

fn tape_library() -> (Box<Project>, MemStore, LibFile) {
    tape_saved(true)
}

fn tape_library_without_src() -> (Box<Project>, MemStore, LibFile) {
    tape_saved(false)
}

/// `tape_library` beside a pool whose 32 source slots are all held.
fn tape_library_with_32_sources_held() -> (Box<Project>, MemStore, LibFile) {
    let (mut p, st, file) = tape_library();
    hold_32_sources(&mut p);
    (p, st, file)
}

fn hold_32_sources(p: &mut Project) {
    for i in 0..32 {
        let r = SourceRef {
            crc: 0x5EED_0000 + i,
            frames: 16,
            rate: 48_000,
        };
        p.source_write(r, 64, |b| b.fill(0)).unwrap();
    }
}

/// The pool's source and Sound counts and its rev.
fn counts(p: &Project) -> (usize, usize, u32) {
    (p.pool().source_count(), p.pool().sound_count(), p.rev())
}

/// The source's bytes staged as the card-row read leaves them: unpublished.
fn stage_source(p: &mut Project, _st: &mut MemStore, need: SourceNeed) {
    let held = held_plain(&sine(48_000, 100.0, FRAMES), 48_000);
    assert_eq!(held.len(), need.held);
    let (_, to) = p.source_staging(need.held).unwrap();
    to[..need.held].copy_from_slice(&held);
}

fn put(st: &mut MemStore, f: FileName, bytes: &[u8]) {
    let v = st.mount().unwrap();
    st.write(v, f, &mut |s: &mut dyn ByteSink| s.put(bytes))
        .unwrap();
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

fn bytes_of(st: &mut MemStore, f: FileName) -> Vec<u8> {
    let v = st.mount().unwrap();
    let mut a = All(Vec::new());
    st.read(v, f, &mut a).unwrap();
    a.0
}

fn rm_pair(st: &mut MemStore, id: LibId) {
    let v = st.mount().unwrap();
    for ext in ["A", "B"] {
        let _ = st.delete(v, file_name(id, ext));
    }
}

fn flip_byte(st: &mut MemStore, id: LibId, at: usize) {
    let f = file_name(id, "A");
    let mut b = bytes_of(st, f);
    b[at] ^= 0x01;
    put(st, f, &b);
}

/// An unstamped Sound file whose body is `Engine` (Modal), a Modal block
/// of `entries`, then a neutral Modal Sound's other records.
fn modal_file(entries: &[(u8, [u8; 4])]) -> Vec<u8> {
    let s = Sound::neutral(EngineType::Modal);
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(SoundName::new("V1 BANK").unwrap()),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_sound(&s, w)).unwrap();
    let mut f = sink.0;
    let mut block = vec![1u8];
    for (id, raw) in entries {
        block.push(*id);
        block.extend(raw);
    }
    let mut rec = RecordTag::Block.code().to_le_bytes().to_vec();
    rec.extend((block.len() as u16).to_le_bytes());
    rec.extend(block);
    let at = HEADER_LEN + 5;
    f.splice(at..at, rec);
    fix_crc(&mut f);
    f
}

/// A v1 Modal BANK file: MODE, EXCITE ≠ BURST's neutral and a retired
/// DECAY, which translate to DAMP and an added BURST entry; `pad` neutral
/// Modal entries no translation touches keep the estimate's granule below
/// the exact cost's.
fn translated_modal_file() -> Vec<u8> {
    let mut e = vec![
        (0, u32::from(ResonatorMode::Modal as u8).to_le_bytes()),
        (1, 0.6f32.to_le_bytes()),
        (2, 0.2f32.to_le_bytes()),
    ];
    // ENS DEPTH, ENS RATE, ENS MIX off neutral.
    for (id, v) in [(9u8, 0.1f32), (10, 0.2), (11, 0.3)].into_iter().take(PAD) {
        e.push((id, v.to_le_bytes()));
    }
    modal_file(&e)
}

/// Entries that put the translated file's estimate just under a granule.
const PAD: usize = 2;

#[test]
fn adopt_stores_the_slot_and_the_part_reads_clean() {
    let (mut p, mut st, file) = saved(sound(0));
    read(&mut st, &mut p, file).unwrap();
    let loads = p.part(P1).loads();
    let a = p.adopt(confirmed(&p, P1, file), None).unwrap();
    assert!(!a.deduped);
    assert_eq!(part_status(p.part(P1), p.pool()), PartStatus::Clean);
    assert!(p.pool().get(a.slot).unwrap().matches(&sound(0)));
    assert_ne!(p.part(P1).loads(), loads);
}

#[test]
fn a_read_counts_no_skips_and_needs_no_source() {
    let (mut p, mut st, file) = saved(sound(0));
    assert_eq!(
        read(&mut st, &mut p, file).unwrap(),
        LibRead {
            skips: 0,
            over: false,
            source: None
        }
    );
}

#[test]
fn loading_the_same_sound_twice_reuses_its_slot() {
    let (first, second) = adopt_twice(sound(0));
    assert!(second.deduped);
    assert_eq!(second.slot, first.slot);
}

#[test]
fn over_budget_changes_nothing() {
    let (mut p, mut st, file) = saved_with_full_arena(sound(0));
    read(&mut st, &mut p, file).unwrap();
    let (rev, used, n) = (p.rev(), p.pool().used(), p.pool().sound_count());
    let e = p.adopt(confirmed(&p, P1, file), None).unwrap_err();
    assert!(matches!(e, AdoptError::Full(Some(f)) if f.needs.bytes() > f.free.bytes()));
    assert_eq!(
        (p.rev(), p.pool().used(), p.pool().sound_count()),
        (rev, used, n)
    );
}

#[test]
fn a_full_source_table_refuses_up_front() {
    let (mut p, mut st, file) = tape_library_with_32_sources_held();
    let r = read(&mut st, &mut p, file).unwrap();
    assert!(r.source.is_some_and(|s| s.on_card));
    assert_eq!(p.library_needs(r.source), Err(None), "no source slot");
    assert_eq!(p.pool().source_count(), 32);
}

#[test]
fn a_moved_witness_leaves_no_source_and_no_slot() {
    let (mut p, mut st, file) = tape_library();
    let r = read(&mut st, &mut p, file).unwrap();
    let c = confirmed(&p, P1, file);
    stage_source(&mut p, &mut st, r.source.unwrap());
    let (srcs, snds) = (p.pool().source_count(), p.pool().sound_count());
    p.edit_part(P1).sound.params.out.volume = 0.1;
    assert_eq!(
        p.adopt(c, Some(Staged::Read(r.source.unwrap())))
            .unwrap_err(),
        AdoptError::Changed
    );
    assert_eq!(
        (p.pool().source_count(), p.pool().sound_count()),
        (srcs, snds)
    );
    assert_eq!(
        ReplaceError::from(AdoptError::Changed),
        ReplaceError::Changed
    );
}

#[test]
fn a_staged_source_commits_with_the_slot() {
    let (mut p, mut st, file) = tape_library();
    let r = read(&mut st, &mut p, file).unwrap();
    let need = r.source.unwrap();
    assert!(need.on_card && need.held > 0);
    let c = confirmed(&p, P1, file);
    stage_source(&mut p, &mut st, need);
    let srcs = p.pool().source_count();
    p.adopt(c, Some(Staged::Read(need))).unwrap();
    let id = p.pool().find_source(need.r).unwrap();
    assert_eq!(p.pool().absence(id), None);
    assert_eq!(p.pool().source(id).unwrap().held as usize, need.held);
    assert_eq!(p.pool().source_count(), srcs + 1);
    // Held now: a second read needs no source.
    assert_eq!(read(&mut st, &mut p, file).unwrap().source, None);
}

#[test]
fn a_missing_source_costs_nothing_and_loads_absent() {
    let (mut p, mut st, file) = tape_library_without_src();
    let r = read(&mut st, &mut p, file).unwrap();
    let need = r.source.unwrap();
    assert!(!need.on_card && need.held == 0);
    let exact = p
        .pool()
        .needs(p.pool().first_free().unwrap(), scratch_of(&p));
    set_free_to(&mut p, exact);
    let a = p
        .adopt(confirmed(&p, P1, file), Some(Staged::Missing(need.r)))
        .unwrap();
    let id = p.pool().find_source(need.r).unwrap();
    assert_eq!(p.pool().absence(id), Some(Absence::NotOnCard));
    assert!(!a.deduped);
}

#[test]
fn a_file_changed_or_gone_since_listed_refuses() {
    let (mut p, mut st, file) = saved(sound(0));
    rm_pair(&mut st, file.id);
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::Gone);
    save_named(&mut st, &sound(1), "OTHER"); // reuses id 1
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::Changed);
}

#[test]
fn a_file_resaved_since_listed_refuses() {
    let (mut p, mut st, file) = saved(sound(0));
    // A newer generation on the B side, same name.
    let b = bytes_of(&mut st, file_name(file.id, "A"));
    let mut b2 = b.clone();
    b2[8..12].copy_from_slice(&2u32.to_le_bytes());
    fix_crc(&mut b2);
    put(&mut st, file_name(file.id, "B"), &b2);
    assert_ne!(listed(&mut st, file.id).seen, file.seen, "the B side newer");
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::Changed);
}

#[test]
fn another_card_refuses() {
    let (mut p, mut st, file) = saved(sound(0));
    let other = LibFile {
        vol: MemStore::new(7).mount().unwrap(),
        ..file
    };
    assert_ne!(other.vol, file.vol);
    assert!(matches!(
        read(&mut st, &mut p, other).unwrap_err(),
        LibNote::Store(chimera_hal::store::StoreError::VolumeChanged(_))
    ));
}

#[test]
fn a_corrupt_file_refuses_and_leaves_the_part() {
    let (mut p, mut st, file) = saved(sound(0));
    flip_byte(&mut st, file.id, 40);
    let before = p.part(P1).sound.clone();
    assert_eq!(
        read(&mut st, &mut p, file).unwrap_err(),
        LibNote::File(FileError::BadCrc)
    );
    assert!(p.part(P1).sound.bits_eq(&before));
}

#[test]
fn an_unknown_engine_refuses_never_loads() {
    let (_, mut st, file) = saved(sound(0));
    let f = file_name(file.id, "A");
    let mut b = bytes_of(&mut st, f);
    b[HEADER_LEN + 4] = 0xEE; // the Engine record's code
    fix_crc(&mut b);
    put(&mut st, f, &b);
    let file = listed(&mut st, file.id);
    let mut p = Project::boxed().0;
    assert_eq!(
        read(&mut st, &mut p, file).unwrap_err(),
        LibNote::File(FileError::NeedsNewerFirmware)
    );
}

#[test]
fn a_translated_file_bigger_than_its_estimate_refuses_cleanly() {
    let (mut p, mut st, file, len) = saved_bytes(translated_modal_file());
    read(&mut st, &mut p, file).unwrap();
    let estimate = Cost::of(len as usize - HEADER_LEN - TRAILER_LEN + 16);
    let exact = p
        .pool()
        .needs(p.pool().first_free().unwrap(), scratch_of(&p));
    assert!(
        exact.bytes() > estimate.bytes(),
        "{} vs {}",
        exact.bytes(),
        estimate.bytes()
    );
    set_free_to(&mut p, estimate);
    assert!(matches!(
        p.adopt(confirmed(&p, P1, file), None),
        Err(AdoptError::Full(Some(_)))
    ));
}

#[test]
fn a_newer_side_failing_its_crc_never_loads_the_older() {
    let (mut p, mut st, file) = saved(sound(0));
    // B: another Sound at generation 2, its body damaged after the CRC.
    let other = sound(1);
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST.next(),
        name: Some(other.name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_library_sound(&other, &ST, w)).unwrap();
    let mut b = sink.0;
    let at = b.len() - 8;
    b[at] ^= 0x01;
    put(&mut st, file_name(file.id, "B"), &b);
    let file = listed(&mut st, file.id);
    assert_eq!(file.seen.name, Some(other.name), "the listing peeks B");
    let before = (p.part(P1).sound.clone(), counts(&p));
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::Changed);
    assert!(p.part(P1).sound.bits_eq(&before.0));
    assert_eq!(counts(&p), before.1);
}

#[test]
fn a_staged_sources_cost_over_budget_changes_nothing() {
    let (mut p, mut st, file) = tape_library();
    let need = read(&mut st, &mut p, file).unwrap().source.unwrap();
    let exact = p
        .pool()
        .needs(p.pool().first_free().unwrap(), scratch_of(&p));
    set_free_to(&mut p, exact);
    let before = counts(&p);
    let e = p
        .adopt(confirmed(&p, P1, file), Some(Staged::Read(need)))
        .unwrap_err();
    assert!(matches!(e, AdoptError::Full(Some(f)) if f.needs.bytes() > f.free.bytes()));
    assert_eq!(counts(&p), before);
}

#[test]
fn a_missing_source_with_every_source_slot_taken_changes_nothing() {
    let (mut p, mut st, file) = tape_library_without_src();
    hold_32_sources(&mut p);
    let need = read(&mut st, &mut p, file).unwrap().source.unwrap();
    let before = counts(&p);
    assert_eq!(
        p.adopt(confirmed(&p, P1, file), Some(Staged::Missing(need.r)))
            .unwrap_err(),
        AdoptError::Full(None)
    );
    assert_eq!(counts(&p), before);
}

#[test]
fn a_damaged_src_loads_absent_as_missing() {
    let (mut p, mut st, file) = tape_library_without_src();
    let (h, _) = source();
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Tapes).unwrap();
    st.write(v, source_file(h.sref), &mut |s: &mut dyn ByteSink| {
        s.put(&[0; 7])
    })
    .unwrap();
    let need = read(&mut st, &mut p, file).unwrap().source.unwrap();
    assert!(need.on_card && need.held == 0 && !need.readable());
    p.adopt(confirmed(&p, P1, file), Some(Staged::Read(need)))
        .unwrap();
    let id = p.pool().find_source(h.sref).unwrap();
    assert_eq!(p.pool().absence(id), Some(Absence::Missing));
}

#[test]
fn a_read_not_on_the_card_stores_absent_never_empty() {
    let (mut p, mut st, file) = tape_library_without_src();
    let need = read(&mut st, &mut p, file).unwrap().source.unwrap();
    p.adopt(confirmed(&p, P1, file), Some(Staged::Read(need)))
        .unwrap();
    let id = p.pool().find_source(need.r).unwrap();
    assert_eq!(p.pool().absence(id), Some(Absence::NotOnCard));
}

/// A Sound legal whole but over the floor in operator order reads whole.
#[test]
fn a_sound_that_fits_whole_reads_whole() {
    let mut s = common::formant::frmt_last();
    s.name = SoundName::new("FRMT LAST").unwrap();
    let (mut p, mut st, file) = saved(s);
    assert_eq!(
        read(&mut st, &mut p, file).unwrap(),
        LibRead {
            skips: 0,
            over: false,
            source: None
        }
    );
    assert_eq!(
        scratch_of(&p).params.algo.kinds().all(),
        common::formant::FRMT_LAST
    );
}
