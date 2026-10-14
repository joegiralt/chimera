//! The boot's staged file (ADR 0137, Task 9c): `/CHIMERA/SYSTEM/BANK2.BK2`
//! first, then up to eight `*.BK2` in the card root, a dropped
//! `chimera-<v>.bk2` by its short name; the first judged `Good` for this
//! build's seal. Nothing is read when flash already holds it.

use chimera_core::console::{ROOT_BK2_MAX, STAGED, find_staged};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use chimera_image::crc::Crc32;
use chimera_image::{Header, ImageRead, Seal, Verdict, Version};

const SEAL: [u8; 8] = *b"SEAL0001";
const OTHER: [u8; 8] = *b"SEAL0002";

fn seal() -> Seal {
    Seal::new(SEAL).unwrap()
}

/// A committed image of `body` sealed `seal`, named `version`.
fn image(seal: [u8; 8], version: &str, body: &[u8]) -> Vec<u8> {
    let mut c = Crc32::new();
    c.update(body);
    let h = Header {
        body_len: body.len() as u32,
        body_crc: c.finish(),
        door_off: 0x100,
        seal,
        version: Version::new(version),
    };
    [&h.to_bytes(true)[..], body].concat()
}

fn body(seed: u8) -> Vec<u8> {
    (0..1024u32)
        .map(|i| (i as u8).wrapping_mul(7) ^ seed)
        .collect()
}

fn root(stem: &[u8]) -> FileName {
    FileName::new(Dir::Root, stem, b"BK2").unwrap()
}

/// `MemStore`, every call counted and every file read named.
struct Counted {
    s: MemStore,
    calls: usize,
    read: Vec<FileName>,
}

impl Counted {
    fn new() -> (Self, VolumeId) {
        let mut s = MemStore::new(1);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        s.make_dir(vol, Dir::System).unwrap();
        let c = Counted {
            s,
            calls: 0,
            read: Vec::new(),
        };
        (c, vol)
    }

    /// A root file is planted, as a host drops it: the store won't write
    /// the root.
    fn put(&mut self, vol: VolumeId, f: FileName, bytes: &[u8]) {
        if f.dir() == Dir::Root {
            return self.s.plant(f, bytes);
        }
        self.s
            .write(vol, f, &mut |w: &mut dyn ByteSink| w.put(bytes))
            .unwrap();
    }

    /// Files read, each once.
    fn files_read(&self) -> Vec<FileName> {
        let mut v = self.read.clone();
        v.dedup();
        v
    }
}

impl Store for Counted {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.calls += 1;
        self.s.mount()
    }
    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.calls += 1;
        self.s.list(vol, dir, f)
    }
    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.read_at(vol, file, 0, sink)
    }
    fn read_at(
        &mut self,
        vol: VolumeId,
        file: FileName,
        offset: u32,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.calls += 1;
        self.read.push(file);
        self.s.read_at(vol, file, offset, sink)
    }
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.calls += 1;
        self.s.write(vol, file, body)
    }
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.calls += 1;
        self.s.delete(vol, file)
    }
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.calls += 1;
        self.s.make_dir(vol, dir)
    }
}

/// The file the boot would install and its version, if any.
fn found(s: &mut Counted, flash: &Verdict) -> Option<(FileName, String)> {
    let (mut img, h) = find_staged(s, flash, &seal())?;
    let mut magic = [0; 8];
    img.read(0, &mut magic).unwrap();
    assert_eq!(&magic, b"CHIMERA2");
    Some((img.file(), h.version.as_str().to_string()))
}

#[test]
fn a_dropped_short_name_takes_tilde_in_the_root_only() {
    assert!(FileName::new(Dir::Root, b"CHIMER~1", b"BK2").is_some());
    assert!(FileName::new(Dir::System, b"CHIMER~1", b"BK2").is_none());
    assert!(FileName::new(Dir::Root, b"CHIMER~1", b"B~2").is_none());
}

#[test]
fn a_matching_root_bk2_installs() {
    let (mut s, vol) = Counted::new();
    s.put(vol, root(b"CHIMER~1"), &image(SEAL, "0.2.0 aa", &body(1)));
    s.put(
        vol,
        FileName::new(Dir::Root, b"CHIMER~1", b"BIN").unwrap(),
        b"firmware",
    );
    assert_eq!(
        found(&mut s, &Verdict::Empty),
        Some((root(b"CHIMER~1"), "0.2.0 aa".into()))
    );
    assert!(!s.files_read().iter().any(|f| f.ext() == b"BIN"));
}

#[test]
fn a_root_bk2_for_another_build_is_ignored() {
    let (mut s, vol) = Counted::new();
    s.put(vol, root(b"CHIMER~1"), &image(OTHER, "0.3.0 bb", &body(1)));
    s.put(vol, STAGED, &image(OTHER, "0.3.0 bb", &body(1)));
    let stale = Verdict::Stale {
        have: Version::new("0.1.0"),
    };
    assert_eq!(found(&mut s, &stale), None);
    assert_eq!(found(&mut s, &Verdict::Empty), None);
}

#[test]
fn of_several_the_matching_one() {
    let (mut s, vol) = Counted::new();
    s.put(vol, root(b"CHIMER~1"), &image(OTHER, "0.1.0 aa", &body(1)));
    s.put(vol, root(b"CHIMER~2"), &image(SEAL, "0.2.0 bb", &body(2)));
    s.put(vol, root(b"CHIMER~3"), &[0u8; 300]);
    s.put(vol, root(b"CHIMER~4"), &image(OTHER, "0.3.0 cc", &body(3)));
    assert_eq!(
        found(&mut s, &Verdict::Torn),
        Some((root(b"CHIMER~2"), "0.2.0 bb".into()))
    );
}

#[test]
fn the_system_file_comes_first() {
    let (mut s, vol) = Counted::new();
    s.put(vol, root(b"CHIMER~1"), &image(SEAL, "root", &body(1)));
    s.put(vol, STAGED, &image(SEAL, "system", &body(2)));
    assert_eq!(
        found(&mut s, &Verdict::Empty),
        Some((STAGED, "system".into()))
    );
    // A stale SYSTEM file: the root's.
    s.put(vol, STAGED, &image(OTHER, "old", &body(2)));
    assert_eq!(
        found(&mut s, &Verdict::Empty),
        Some((root(b"CHIMER~1"), "root".into()))
    );
}

/// Releases dropped and never deleted pile up ahead of the newest (a host
/// appends entries): it is still found behind 8 and more, up to the bound.
#[test]
fn the_newest_is_found_behind_many_stale_up_to_the_bound() {
    assert_eq!(ROOT_BK2_MAX, 32);
    for stale in [8u8, 12, 31, 32, 33] {
        let (mut s, vol) = Counted::new();
        for i in 1..=stale {
            let stem = format!("CHIME~{i:02}");
            s.put(vol, root(stem.as_bytes()), &image(OTHER, "old", &body(i)));
        }
        // Lists after every CHIME~nn.
        s.put(vol, root(b"ZZ"), &image(SEAL, "new", &body(0)));
        let got = found(&mut s, &Verdict::Empty);
        if usize::from(stale) < ROOT_BK2_MAX {
            assert_eq!(got, Some((root(b"ZZ"), "new".into())), "{stale} stale");
        } else {
            assert_eq!(got, None, "{stale} stale");
        }
        let roots = s
            .files_read()
            .iter()
            .filter(|f| f.dir() == Dir::Root)
            .count();
        assert!(roots <= ROOT_BK2_MAX, "{stale} stale: {roots} judged");
    }
}

#[test]
fn the_root_is_never_written() {
    let (mut s, vol) = Counted::new();
    let f = root(b"CHIMER~1");
    assert_eq!(
        s.write(vol, f, &mut |w: &mut dyn ByteSink| w.put(b"x")),
        Err(StoreError::ReadOnly)
    );
    assert_eq!(s.delete(vol, f), Err(StoreError::ReadOnly));
}

#[test]
fn flash_already_good_reads_nothing() {
    let (mut s, vol) = Counted::new();
    s.put(vol, root(b"CHIMER~1"), &image(SEAL, "root", &body(1)));
    s.put(vol, STAGED, &image(SEAL, "system", &body(2)));
    s.calls = 0;
    let h = match chimera_image::judge(&mut Bytes(image(SEAL, "x", &body(3))), &seal()) {
        Verdict::Good(h) => h,
        v => panic!("{v:?}"),
    };
    assert_eq!(found(&mut s, &Verdict::Good(h)), None);
    assert_eq!(s.calls, 0, "no mount, list or read");
}

#[test]
fn no_card_installs_nothing() {
    let (mut s, _) = Counted::new();
    s.s.eject();
    assert_eq!(found(&mut s, &Verdict::Empty), None);
}

struct Bytes(Vec<u8>);

impl ImageRead for Bytes {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), chimera_image::ReadFail> {
        let o = off as usize;
        let s = self
            .0
            .get(o..o + buf.len())
            .ok_or(chimera_image::ReadFail)?;
        buf.copy_from_slice(s);
        Ok(())
    }
}
