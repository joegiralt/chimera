// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Library ids, the header peek and saving a Sound to the card (library
//! spec § Files, § Save).

mod common;

use std::ops::ControlFlow;

use chimera_core::factory::factory_sound;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::FullOnWrite;
use chimera_core::project::{
    LibNote, LibPage, PROJECT_DEMO_TOP, PageAt, Shelf, list_library, list_projects, new_project_id,
    peek_note, save_library_sound,
};
use chimera_core::storage::{
    BuildStamp, Card, FileError, FileKind, Generation, Header, ID_WINDOW, LIB_PEEK, LibId, NextId,
    Note, SoundDecoder, UserId, encode_library_sound, encode_sound, lib_id, library_file, load_ab,
    next_user_id, peek_library, write_file,
};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use common::codec_util::VecSink;

const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);

fn file_name(s: &str) -> FileName {
    let (stem, ext) = s.split_once('.').unwrap();
    FileName::new(Dir::Sounds, stem.as_bytes(), ext.as_bytes()).unwrap()
}

/// A card with `/CHIMERA/SOUNDS` and a stub file per name.
fn card_with(names: &[&str]) -> MemStore {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    for n in names {
        st.write(v, file_name(n), &mut |s: &mut dyn ByteSink| s.put(b"x"))
            .unwrap();
    }
    st
}

fn listing(st: &mut impl Store, dir: Dir) -> Result<Vec<(String, u32)>, StoreError> {
    let v = st.mount()?;
    let mut out = Vec::new();
    st.list(v, dir, &mut |f, len| {
        let (s, e) = (f.stem(), f.ext());
        out.push((
            format!(
                "{}.{}",
                String::from_utf8_lossy(s),
                String::from_utf8_lossy(e)
            ),
            len,
        ));
    })?;
    out.sort();
    Ok(out)
}

fn len_of(st: &mut MemStore, n: &str) -> u32 {
    listing(st, Dir::Sounds)
        .unwrap()
        .into_iter()
        .find(|(f, _)| f == n)
        .map(|(_, l)| l)
        .unwrap_or_else(|| panic!("{n} listed"))
}

struct Head(Vec<u8>, usize);

impl ReadSink for Head {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        let n = b.len().min(self.1 - self.0.len());
        self.0.extend_from_slice(&b[..n]);
        if self.0.len() == self.1 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// The first `n` bytes of `name`.
fn first(st: &mut MemStore, name: &str, n: usize) -> Vec<u8> {
    let v = st.mount().unwrap();
    let mut h = Head(Vec::new(), n);
    st.read(v, file_name(name), &mut h).unwrap();
    h.0
}

fn save_named<S: Store>(st: &mut S, s: &Sound, name: &str) -> Result<LibId, LibNote> {
    save_library_sound(&mut Card::new(), st, s, SoundName::new(name).unwrap(), &ST)
        .out
        .map(LibId::from)
}

fn save<S: Store>(st: &mut S) -> Result<LibId, LibNote> {
    save_named(st, &factory_sound(0).unwrap(), "BASS-1")
}

fn load_sound(st: &mut MemStore, id: LibId) -> Sound {
    let mut t = Sound::neutral(EngineType::Algo);
    Card::new()
        .run(st, |s, r| {
            load_ab(s, r, library_file(id), &mut SoundDecoder::new(&mut t))
        })
        .unwrap()
        .result
        .unwrap();
    t
}

fn file_of(
    s: &Sound,
    name: &str,
    body: &mut dyn FnMut(&mut chimera_core::storage::RecordWriter<'_>) -> Result<(), StoreError>,
) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(SoundName::new(name).unwrap()),
    };
    let _ = s;
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, body).unwrap();
    sink.0
}

fn stamped_named(s: &Sound, name: &str) -> Vec<u8> {
    file_of(s, name, &mut |w| encode_library_sound(s, &ST, None, w))
}

fn unstamped(s: &Sound) -> Vec<u8> {
    file_of(s, "INIT", &mut |w| encode_sound(s, w))
}

#[test]
fn ids_parse_only_library_names() {
    let n = |d, s: &[u8], e: &[u8]| lib_id(FileName::new(d, s, e).unwrap());
    assert_eq!(n(Dir::Sounds, b"S0000012", b"A").map(LibId::get), Some(12));
    assert_eq!(
        n(Dir::Sounds, b"S9000001", b"B").map(LibId::is_factory),
        Some(true)
    );
    for (s, e) in [
        (&b"S000012"[..], &b"A"[..]),
        (b"BANK1", b"BNK"),
        (b"S0000012", b"C"),
        (b"S9000000", b"A"),
    ] {
        assert_eq!(n(Dir::Sounds, s, e), None);
    }
    assert_eq!(n(Dir::Projects, b"S0000012", b"A"), None);
}

#[test]
fn ids_have_a_stem_and_bounds() {
    assert_eq!(&LibId::new(12).unwrap().stem(), b"S0000012");
    assert_eq!(&LibId::new(9_999_999).unwrap().stem(), b"S9999999");
    for n in [0, 9_000_000, 10_000_000] {
        assert_eq!(LibId::new(n), None);
    }
}

#[test]
fn a_save_takes_top_plus_one_ignoring_factory_and_stock() {
    let mut st = card_with(&["S0000003.A", "S9000001.A", "BANK1.BNK"]);
    assert_eq!(save(&mut st).unwrap().get(), 4);
}

#[test]
fn a_save_past_the_top_takes_the_lowest_free() {
    let mut st = card_with(&["S8999999.A", "S0000001.A"]);
    assert_eq!(save(&mut st).unwrap().get(), 2);
}

#[test]
fn next_user_id_scans_windows_and_runs_out() {
    let full = [0xFF; 512];
    assert_eq!(
        next_user_id(LibId::USER_MAX, 1, &full),
        NextId::Scan(1 + ID_WINDOW)
    );
    let mut t = [0xFF; 512];
    t[1] = 0b1111_1011;
    assert_eq!(
        next_user_id(LibId::USER_MAX, 1, &t),
        NextId::Id(UserId::new(11).unwrap())
    );
    assert_eq!(
        next_user_id(LibId::USER_MAX, LibId::USER_MAX - 7, &full),
        NextId::NoIds
    );
}

#[test]
fn a_save_makes_sounds_only_then_and_loads_back() {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    assert_eq!(listing(&mut st, Dir::Sounds), Err(StoreError::NotFound));
    let mut snd = factory_sound(0).unwrap();
    snd.name = SoundName::new("BASS-1").unwrap();
    let id = save_named(&mut st, &snd, "BASS-1").unwrap();
    let len = len_of(&mut st, "S0000001.A");
    assert_eq!(
        listing(&mut st, Dir::Sounds).unwrap(),
        vec![("S0000001.A".into(), len)]
    );
    let p = peek_library(&first(&mut st, "S0000001.A", LIB_PEEK), len).unwrap();
    assert_eq!(
        (p.generation, p.name.unwrap().as_str(), p.stamp),
        (Generation::FIRST, "BASS-1", Some(ST))
    );
    assert!(load_sound(&mut st, id).bits_eq(&snd));
}

#[test]
fn a_full_card_leaves_the_directory_as_it_was() {
    let mut st = FullOnWrite(card_with(&["S0000001.A"]), false);
    let before = listing(&mut st, Dir::Sounds).unwrap();
    st.1 = true;
    assert_eq!(save(&mut st).unwrap_err(), LibNote::Store(StoreError::Full));
    assert_eq!(listing(&mut st, Dir::Sounds).unwrap(), before);
}

#[test]
fn a_save_past_the_top_scans_past_a_full_window() {
    let mut st = card_with(&["S8999999.A"]);
    // Ids 1..4096 taken beside the top: the first window has one gap.
    let v = st.mount().unwrap();
    for n in 1..ID_WINDOW {
        let name = format!("S{n:07}.A");
        st.write(v, file_name(&name), &mut |s: &mut dyn ByteSink| s.put(b"x"))
            .unwrap();
    }
    // Id 4096 is the one clear bit of window 1.
    assert_eq!(save(&mut st).unwrap().get(), ID_WINDOW);
}

#[test]
fn peek_reads_name_engine_and_stamp() {
    let f = stamped_named(&factory_sound(0).unwrap(), "BASS-1");
    let p = peek_library(&f[..LIB_PEEK], f.len() as u32).unwrap();
    assert_eq!(
        (p.name.unwrap().as_str(), p.engine, p.stamp),
        ("BASS-1", Some(EngineType::Algo), Some(ST))
    );
    let u = unstamped(&Sound::neutral(EngineType::Algo));
    assert_eq!(peek_library(&u, u.len() as u32).unwrap().stamp, None);
    assert_eq!(peek_library(&[], 0), Err(FileError::Truncated));
}

#[test]
fn peek_refuses_other_kinds_and_unknown_engines() {
    let mut f = stamped_named(&factory_sound(0).unwrap(), "BASS-1");
    let len = f.len() as u32;
    f[6] = FileKind::Project as u8;
    assert_eq!(peek_library(&f[..LIB_PEEK], len), Err(FileError::WrongKind));
    f[6] = FileKind::Sound as u8;
    f[32] = 0x7F;
    let p = peek_library(&f[..LIB_PEEK], len).unwrap();
    assert_eq!(
        (p.needs_newer, p.engine, p.name.unwrap().as_str(), p.stamp),
        (true, None, "BASS-1", Some(ST))
    );
}

/// A later build grows `Build` (spec § Files): the stamp still reads.
#[test]
fn peek_reads_the_stamp_of_a_grown_build() {
    use chimera_core::storage::RecordTag;
    let base = stamped_named(&factory_sound(0).unwrap(), "BASS-1");
    let at = 28 + 5;
    assert_eq!(base[at..at + 2], RecordTag::Build.code().to_le_bytes());
    let old_len = usize::from(u16::from_le_bytes([base[at + 2], base[at + 3]]));
    for grown in [30usize, 40] {
        let mut f = base[..at + 4 + old_len].to_vec();
        f[at + 2..at + 4].copy_from_slice(&(grown as u16).to_le_bytes());
        f.resize(at + 4 + grown, 0);
        // Whole-file length with a trailer: only `file_len` matters to a peek.
        let len = (f.len() + 4) as u32;
        let p = peek_library(&f[..f.len().min(LIB_PEEK)], len).unwrap();
        assert_eq!(p.stamp, Some(ST), "{grown} B Build");
    }
}

/// A later build's longer `Build` moves the `Note`: the peek finds it at
/// `Build`'s declared length, not at this build's.
#[test]
fn peek_reads_the_note_after_a_grown_build() {
    use chimera_core::storage::RecordTag;
    let note = Note::new("AFTER A LONGER BUILD").unwrap();
    let base = file_of(&Sound::init(EngineType::Algo), "GROWN", &mut |w| {
        w.put(RecordTag::Engine, &[0])?;
        let mut build = ST.to_bytes().to_vec();
        build.resize(build.len() + 6, 0);
        w.put(RecordTag::Build, &build)?;
        w.put(RecordTag::Note, note.as_str().as_bytes())
    });
    let p = peek_library(&base[..base.len().min(LIB_PEEK)], base.len() as u32).unwrap();
    assert_eq!((p.stamp, p.note), (Some(ST), Some(note)));
}

#[test]
fn peek_never_panics_on_cut_input() {
    let f = stamped_named(&factory_sound(0).unwrap(), "BASS-1");
    for n in 0..f.len() {
        let _ = peek_library(&f[..n], f.len() as u32);
        let _ = peek_library(&f[..n], n as u32);
    }
}

/// A store whose write lands the whole file, then reports `Full`: the file
/// is on the card though the save failed.
struct LandsThenFails(MemStore);

impl Store for LandsThenFails {
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
    fn read(&mut self, v: VolumeId, f: FileName, s: &mut dyn ReadSink) -> Result<(), StoreError> {
        self.0.read(v, f, s)
    }
    fn write(
        &mut self,
        v: VolumeId,
        f: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.0.write(v, f, body)?;
        Err(StoreError::Full)
    }
    fn delete(&mut self, v: VolumeId, f: FileName) -> Result<(), StoreError> {
        self.0.delete(v, f)
    }
    fn make_dir(&mut self, v: VolumeId, d: Dir) -> Result<(), StoreError> {
        self.0.make_dir(v, d)
    }
}

#[test]
fn a_file_that_landed_before_the_error_is_deleted() {
    let mut st = LandsThenFails(card_with(&["S0000001.A"]));
    let before = listing(&mut st, Dir::Sounds).unwrap();
    assert_eq!(save(&mut st).unwrap_err(), LibNote::Store(StoreError::Full));
    assert_eq!(listing(&mut st, Dir::Sounds).unwrap(), before);
}

fn put<S: Store>(st: &mut S, name: &str, bytes: &[u8]) {
    let v = st.mount().unwrap();
    st.write(v, file_name(name), &mut |s: &mut dyn ByteSink| s.put(bytes))
        .unwrap();
}

/// Real library files for ids `r`, `.A` only.
fn card_with_sounds(r: std::ops::RangeInclusive<u32>) -> MemStore {
    let mut st = card_with(&[]);
    let snd = factory_sound(0).unwrap();
    for _ in r {
        save_named(&mut st, &snd, "BASS-1").unwrap();
    }
    st
}

fn chimera_only() -> MemStore {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st
}

fn list<S: Store>(st: &mut S, at: PageAt) -> Box<LibPage> {
    let mut p = Box::new(LibPage::EMPTY);
    list_library(&mut Card::new(), st, Shelf::User, at, &mut p)
        .out
        .unwrap();
    p
}

fn list_err<S: Store>(st: &mut S) -> LibNote {
    let mut p = Box::new(LibPage::EMPTY);
    list_library(&mut Card::new(), st, Shelf::User, PageAt::First, &mut p)
        .out
        .unwrap_err()
}

fn ids(p: &LibPage) -> Vec<u32> {
    p.entries().iter().flatten().map(|e| e.id.get()).collect()
}

fn entry(p: &LibPage, id: u32) -> chimera_core::project::LibEntry {
    p.entries()
        .iter()
        .flatten()
        .find(|e| e.id.get() == id)
        .copied()
        .unwrap()
}

fn rewrite(st: &mut MemStore, name: &str, f: impl FnOnce(Vec<u8>) -> Vec<u8>) {
    let n = len_of(st, name) as usize;
    let b = f(first(st, name, n));
    put(st, name, &b);
}

fn truncate(st: &mut MemStore, name: &str, n: usize) {
    rewrite(st, name, |mut b| {
        b.truncate(n);
        b
    });
}

fn set_version(st: &mut MemStore, name: &str, v: u8) {
    rewrite(st, name, |mut b| {
        b[4] = v;
        b
    });
}

/// A store that records the name of each `read`.
struct Counting(MemStore, std::cell::RefCell<Vec<String>>);

impl Counting {
    fn new(st: MemStore) -> Self {
        Counting(st, Default::default())
    }
    fn reads(&self) -> Vec<String> {
        let mut r = self.1.borrow().clone();
        r.sort();
        r
    }
}

impl Store for Counting {
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
    fn read(&mut self, v: VolumeId, f: FileName, s: &mut dyn ReadSink) -> Result<(), StoreError> {
        self.1.borrow_mut().push(format!(
            "{}.{}",
            String::from_utf8_lossy(f.stem()),
            String::from_utf8_lossy(f.ext())
        ));
        self.0.read(v, f, s)
    }
    fn write(
        &mut self,
        v: VolumeId,
        f: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.0.write(v, f, body)
    }
    fn delete(&mut self, v: VolumeId, f: FileName) -> Result<(), StoreError> {
        self.0.delete(v, f)
    }
    fn make_dir(&mut self, v: VolumeId, d: Dir) -> Result<(), StoreError> {
        self.0.make_dir(v, d)
    }
}

#[test]
fn a_hundred_sounds_page_48_48_4_newest_first() {
    let mut st = card_with_sounds(1..=100);
    let p1 = list(&mut st, PageAt::First);
    assert_eq!(
        (ids(&p1)[..3].to_vec(), ids(&p1).len(), p1.prev, p1.next),
        (vec![100, 99, 98], 48, false, true)
    );
    let p2 = list(&mut st, PageAt::After(LibId::new(53).unwrap()));
    assert_eq!(
        (ids(&p2)[0], ids(&p2).len(), p2.prev, p2.next),
        (52, 48, true, true)
    );
    let p3 = list(&mut st, PageAt::After(LibId::new(5).unwrap()));
    assert_eq!((ids(&p3), p3.next), (vec![4, 3, 2, 1], false));
    let up = list(&mut st, PageAt::Before(LibId::new(52).unwrap()));
    assert_eq!((ids(&up)[47], ids(&up)[0], up.next), (53, 100, true));
    assert!(!up.prev);
    assert_eq!(up.position(LibId::new(100).unwrap()), Some(0));
}

#[test]
fn factory_and_foreign_names_are_never_listed_or_opened() {
    let mut st = card_with_sounds(1..=1);
    for n in ["S9000001.A", "BANK1.BNK", "S000001.A"] {
        put(&mut st, n, &[0; 40]);
    }
    let mut st = Counting::new(st);
    let p = list(&mut st, PageAt::First);
    assert_eq!(ids(&p), vec![1]);
    assert_eq!(st.reads(), vec!["S0000001.A"]);
}

#[test]
fn only_seen_sides_are_read() {
    let mut st = Counting::new(card_with_sounds(1..=10));
    list(&mut st, PageAt::First);
    assert_eq!(st.reads().len(), 10);
    assert!(st.reads().iter().all(|n| n.ends_with(".A")));
}

#[test]
fn bad_sides_list_with_their_errors() {
    let mut st = card_with_sounds(1..=2);
    truncate(&mut st, "S0000001.A", 0);
    set_version(&mut st, "S0000002.A", 2);
    let p = list(&mut st, PageAt::First);
    assert_eq!(entry(&p, 1).err, Some(FileError::Truncated));
    assert_eq!(entry(&p, 1).seen.name, None);
    assert_eq!(entry(&p, 2).err, Some(FileError::NeedsNewerFirmware));
}

#[test]
fn no_sounds_dir_is_an_empty_page() {
    assert!(ids(&list(&mut chimera_only(), PageAt::First)).is_empty());
}

#[test]
fn no_card_is_its_store_error() {
    let mut st = MemStore::new(1);
    st.eject();
    assert_eq!(list_err(&mut st), LibNote::Store(StoreError::NoCard));
}

#[test]
fn an_unknown_engine_lists_greyed_with_its_name_and_stamp() {
    let mut st = card_with_sounds(1..=1);
    rewrite(&mut st, "S0000001.A", |mut b| {
        b[32] = 0x7F;
        b
    });
    let p = list(&mut st, PageAt::First);
    let e = entry(&p, 1);
    assert_eq!(e.err, Some(FileError::NeedsNewerFirmware));
    assert_eq!(e.seen.name.unwrap().as_str(), "BASS-1");
    assert_eq!((e.engine, e.stamp()), (None, Some(ST)));
}

/// A project pair stub: the allocators read names only.
fn project_stub(st: &mut MemStore, n: u32) {
    let v = st.mount().unwrap();
    let _ = st.make_dir(v, Dir::Chimera);
    let _ = st.make_dir(v, Dir::Projects);
    let name = format!("P{n:07}");
    let f = FileName::new(Dir::Projects, name.as_bytes(), b"A").unwrap();
    st.write(v, f, &mut |s: &mut dyn ByteSink| s.put(b"x"))
        .unwrap();
}

/// What both allocation sites hand a new project, given the ids on the card.
fn next_project(present: &[u32]) -> (u32, u32) {
    let mut st = MemStore::new(1);
    for &n in present {
        project_stub(&mut st, n);
    }
    let by_new = new_project_id(&mut Card::new(), &mut st).out.unwrap();
    let by_list = list_projects(&mut Card::new(), &mut st, &mut |_| {})
        .fresh
        .unwrap();
    (by_new.file().id().get(), by_list.file().id().get())
}

#[test]
fn save_on_empty_card_writes_p0000010() {
    assert_eq!(PROJECT_DEMO_TOP, 9);
    assert_eq!(next_project(&[]), (10, 10));
}

#[test]
fn save_with_p0000003_present_writes_p0000010() {
    assert_eq!(next_project(&[3]), (10, 10));
}

#[test]
fn save_with_p0000012_present_writes_p0000013() {
    assert_eq!(next_project(&[12]), (13, 13));
}

#[test]
fn lib_id_parse_splits_user_from_factory() {
    use chimera_core::storage::LibKind;
    assert!(matches!(
        LibId::parse("S0000012").map(LibId::kind),
        Some(LibKind::User(_))
    ));
    assert!(matches!(
        LibId::parse("S9100001").map(LibId::kind),
        Some(LibKind::Factory(_))
    ));
    for bad in [
        "S9000000",
        "S0000000",
        "P0000012",
        "S000012",
        "S00000123",
        "s0000012",
        "S000001x",
    ] {
        assert_eq!(LibId::parse(bad), None, "{bad}");
    }
    assert_eq!(UserId::new(9_000_001), None);
    assert!(chimera_core::storage::FactoryId::new(8_999_999).is_none());
    let f = chimera_core::storage::FactoryId::new(9_100_001).unwrap();
    assert_eq!(f.to_string(), "S9100001");
}

/// Review Focus 3: a factory Sound loaded then saved is a user file, no Note.
#[test]
fn factory_load_then_save_is_user_without_note() {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    let mut snd = factory_sound(0).unwrap();
    snd.name = SoundName::new("SHOWCASE").unwrap();
    let note = Note::new("A DESCRIBED SOUND").unwrap();
    let bytes = file_of(&snd, "SHOWCASE", &mut |w| {
        encode_library_sound(&snd, &ST, Some(&note), w)
    });
    let fid = LibId::parse("S9100001").unwrap();
    assert!(fid.is_factory());
    let name = FileName::new(Dir::Sounds, &fid.stem(), b"A").unwrap();
    st.write(v, name, &mut |s: &mut dyn ByteSink| s.put(&bytes))
        .unwrap();
    let len = len_of(&mut st, "S9100001.A");
    let p = peek_library(&first(&mut st, "S9100001.A", LIB_PEEK), len).unwrap();
    assert_eq!(p.note, Some(note), "the factory file carries its Note");

    let loaded = load_sound(&mut st, fid);
    let id = save_library_sound(&mut Card::new(), &mut st, &loaded, loaded.name, &ST)
        .out
        .unwrap();
    assert_eq!(id.get(), 1);
    assert!(!LibId::from(id).is_factory());
    let n = format!("S{:07}.A", id.get());
    let len = len_of(&mut st, &n);
    let p = peek_library(&first(&mut st, &n, LIB_PEEK), len).unwrap();
    assert_eq!(p.note, None);
}

/// A factory row keeps neither its stamp nor its description: the page
/// keeps one set stamp, its first described row's, and `peek_note` reads
/// one row's description on its own. A user row keeps its stamp.
#[test]
fn factory_rows_keep_the_page_one_set_stamp_and_peek_one_note() {
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    let snd = factory_sound(0).unwrap();
    let other = BuildStamp::new("0.0.9", "aaaaaaaa", false);
    let note = Note::new("A DESCRIBED SOUND").unwrap();
    for (id, stamp, note) in [
        ("S9000001", other, None),
        ("S9100001", ST, Some(&note)),
        ("S0000001", ST, None),
    ] {
        let bytes = file_of(&snd, "SOUND", &mut |w| {
            encode_library_sound(&snd, &stamp, note, w)
        });
        let name = FileName::new(Dir::Sounds, id.as_bytes(), b"A").unwrap();
        st.write(v, name, &mut |s: &mut dyn ByteSink| s.put(&bytes))
            .unwrap();
    }
    let mut p = Box::new(LibPage::EMPTY);
    list_library(
        &mut Card::new(),
        &mut st,
        Shelf::Factory,
        PageAt::First,
        &mut p,
    )
    .out
    .unwrap();
    let rows: Vec<_> = p.entries().iter().flatten().map(|e| e.stamp()).collect();
    assert_eq!(rows, vec![None, None]);
    let peek = |st: &mut MemStore, i: usize| {
        let e = p.entries()[i].unwrap();
        peek_note(&mut Card::new(), st, p.file(&e).unwrap()).out
    };
    assert_eq!(peek(&mut st, 0), Ok(None), "the stray file has none");
    assert_eq!(peek(&mut st, 1), Ok(Some(note)));
    // A file gone since the listing is refused, as a load is.
    let gone = FileName::new(Dir::Sounds, b"S9100001", b"A").unwrap();
    st.delete(v, gone).unwrap();
    assert_eq!(peek(&mut st, 1), Err(LibNote::Gone));
    assert_eq!(
        p.set_stamp,
        Some(ST),
        "the described row's, not the stray file's"
    );
    let user = list(&mut st, PageAt::First);
    assert_eq!(entry(&user, 1).stamp(), Some(ST));
}
