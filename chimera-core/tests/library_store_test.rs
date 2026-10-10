//! Library ids, the header peek and saving a Sound to the card (library
//! spec § Files, § Save).

mod common;

use std::ops::ControlFlow;

use chimera_core::factory::factory_sound;
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::FullOnWrite;
use chimera_core::project::{LibNote, save_library_sound};
use chimera_core::storage::{
    BuildStamp, Card, FileError, FileKind, Generation, Header, ID_WINDOW, LIB_PEEK, LibId, NextId,
    SoundDecoder, encode_library_sound, encode_sound, lib_id, library_file, load_ab, next_user_id,
    peek_library, write_file,
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
    save_library_sound(&mut Card::new(), st, s, SoundName::new(name).unwrap(), &ST).out
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
    file_of(s, name, &mut |w| encode_library_sound(s, &ST, w))
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
        NextId::Id(LibId::new(11).unwrap())
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
    assert_eq!(
        peek_library(&f[..LIB_PEEK], len),
        Err(FileError::NeedsNewerFirmware)
    );
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
