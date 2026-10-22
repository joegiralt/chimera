//! `MemStore` and the conformance suite every `Store` must pass.
extern crate std;

use crate::store::{
    ByteSink, CHUNK, Dir, FileName, FileRuns, ReadSink, RunsError, Seal, Store, StoreError,
    VolumeId, synthetic_runs,
};
use crate::units::{BlockCount, Bytes, FileBlock};
use core::ops::ControlFlow;
use std::collections::{BTreeMap, BTreeSet};
use std::vec::Vec;

/// A `Store` in RAM: the reference behaviour the FAT stores are held to.
pub struct MemStore {
    files: BTreeMap<FileName, Vec<u8>>,
    dirs: BTreeSet<Dir>,
    sealed: BTreeSet<FileName>,
    id: VolumeId,
    present: bool,
}

fn volume(serial: u32) -> VolumeId {
    VolumeId {
        serial,
        label: *b"MEMSTORE   ",
    }
}

impl MemStore {
    pub fn new(serial: u32) -> Self {
        MemStore {
            files: BTreeMap::new(),
            dirs: BTreeSet::new(),
            sealed: BTreeSet::new(),
            id: volume(serial),
            present: true,
        }
    }

    /// A different card, empty, in the slot.
    pub fn swap(&mut self, serial: u32) {
        *self = MemStore::new(serial);
    }

    /// `bytes` as `file`, past `write`: as a host drops a file in the root.
    pub fn plant(&mut self, file: FileName, bytes: &[u8]) {
        self.files.insert(file, bytes.to_vec());
    }

    /// `file`'s seal gone: as a `put` cut before its read-back leaves it.
    pub fn unseal(&mut self, file: FileName) {
        self.sealed.remove(&file);
    }

    pub fn eject(&mut self) {
        self.present = false;
    }

    /// The ejected card back in the slot, files intact.
    pub fn insert(&mut self) {
        self.present = true;
    }

    fn open(&self, vol: VolumeId) -> Result<(), StoreError> {
        if !self.present {
            Err(StoreError::NoCard)
        } else if vol != self.id {
            Err(StoreError::VolumeChanged(self.id))
        } else {
            Ok(())
        }
    }

    fn writable(&self, file: FileName) -> Result<(), StoreError> {
        if file.dir() == Dir::Root || self.sealed.contains(&file) {
            Err(StoreError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn has_dir(&self, dir: Dir) -> Result<(), StoreError> {
        if dir == Dir::Root || self.dirs.contains(&dir) {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }
}

struct Buf<'a>(&'a mut Vec<u8>);

impl ByteSink for Buf<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

impl Store for MemStore {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        if self.present {
            Ok(self.id)
        } else {
            Err(StoreError::NoCard)
        }
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.open(vol)?;
        self.has_dir(dir)?;
        for (name, data) in self.files.iter().filter(|(n, _)| n.dir() == dir) {
            f(*name, data.len() as u32);
        }
        Ok(())
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.read_at(vol, file, 0, sink)
    }

    fn runs(&mut self, vol: VolumeId, file: FileName) -> Result<FileRuns, RunsError> {
        self.open(vol)?;
        let data = self.files.get(&file).ok_or(StoreError::NotFound)?;
        let seal = if self.sealed.contains(&file) {
            Seal::Sealed
        } else {
            Seal::Open
        };
        synthetic_runs(file, Bytes::new(data.len() as u32), seal)
    }

    fn read_at(
        &mut self,
        vol: VolumeId,
        file: FileName,
        offset: u32,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.open(vol)?;
        let data = self.files.get(&file).ok_or(StoreError::NotFound)?;
        let data = data.get(offset as usize..).unwrap_or_default();
        if sink.begin(data.len() as u32).is_break() {
            return Ok(());
        }
        for c in data.chunks(CHUNK) {
            if sink.chunk(c).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.open(vol)?;
        self.has_dir(file.dir())?;
        self.writable(file)?;
        let mut data = Vec::new();
        let r = body(&mut Buf(&mut data));
        let n = data.len() as u32;
        // What was written stays on Err, as on FAT.
        self.files.insert(file, data);
        r.map(|()| n)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.open(vol)?;
        self.writable(file)?;
        self.files
            .remove(&file)
            .map(|_| ())
            .ok_or(StoreError::NotFound)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.open(vol)?;
        if dir == Dir::Root {
            return Ok(());
        }
        if dir != Dir::Chimera {
            self.has_dir(Dir::Chimera)?;
        }
        self.dirs.insert(dir);
        Ok(())
    }

    fn seal(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.open(vol)?;
        if file.dir() != Dir::Tapes {
            return Err(StoreError::Io);
        }
        if !self.files.contains_key(&file) {
            return Err(StoreError::NotFound);
        }
        self.sealed.insert(file);
        Ok(())
    }
}

struct Collect {
    len: Option<u32>,
    data: Vec<u8>,
    /// Break after this many chunks.
    stop_after: Option<usize>,
    break_in_begin: bool,
    chunks: usize,
}

impl Collect {
    fn new(stop_after: Option<usize>) -> Self {
        Collect {
            len: None,
            data: Vec::new(),
            stop_after,
            break_in_begin: false,
            chunks: 0,
        }
    }
}

impl ReadSink for Collect {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.len = Some(len);
        if self.break_in_begin {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        assert!(bytes.len() <= CHUNK);
        if self.chunks > 0 {
            assert_eq!(
                self.data.len(),
                self.chunks * CHUNK,
                "every chunk but the last is full"
            );
        }
        self.data.extend_from_slice(bytes);
        self.chunks += 1;
        if self.stop_after == Some(self.chunks) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 251) as u8).collect()
}

fn name(stem: &[u8]) -> FileName {
    FileName::new(Dir::Chimera, stem, b"BIN").unwrap()
}

fn read_all<S: Store>(
    s: &mut S,
    vol: VolumeId,
    f: FileName,
    stop: Option<usize>,
) -> (Result<(), StoreError>, Collect) {
    let mut c = Collect::new(stop);
    let r = s.read(vol, f, &mut c);
    (r, c)
}

fn listing<S: Store>(
    s: &mut S,
    vol: VolumeId,
    dir: Dir,
) -> Result<Vec<(FileName, usize)>, StoreError> {
    let mut v = Vec::new();
    s.list(vol, dir, &mut |n, len| v.push((n, len as usize)))?;
    v.sort();
    Ok(v)
}

/// Run every `Store` requirement against stores from `make`. `swap` puts a
/// different, empty card in the slot, `eject` takes the card out, and
/// `after_op` runs after every step.
///
/// `Full` and `Corrupt` need a real medium (a full card, a damaged chain), so
/// Task 4 pins them in the FAT stores' own tests, not here.
pub fn store_suite<S: Store>(
    make: &mut dyn FnMut() -> S,
    swap: &mut dyn FnMut(&mut S),
    eject: &mut dyn FnMut(&mut S),
    after_op: &mut dyn FnMut(&S),
) {
    let mut s = make();
    let vol = s.mount().unwrap();
    after_op(&s);
    assert_eq!(s.mount().unwrap(), vol, "mount is stable");
    after_op(&s);

    // Directories must exist: a write into an unmade one, or a child made
    // before its parent, is NotFound.
    assert_eq!(
        listing(&mut s, vol, Dir::Chimera),
        Err(StoreError::NotFound)
    );
    after_op(&s);
    let r = s.write(vol, name(b"EARLY"), &mut |w| w.put(b"x"));
    assert_eq!(r, Err(StoreError::NotFound));
    after_op(&s);
    assert_eq!(s.make_dir(vol, Dir::Projects), Err(StoreError::NotFound));
    after_op(&s);
    s.make_dir(vol, Dir::Chimera).unwrap();
    after_op(&s);

    // The root is listed and read, never written: a host drops files there.
    let dropped = FileName::new(Dir::Root, b"CHIMER~1", b"BK2").unwrap();
    assert_eq!(s.make_dir(vol, Dir::Root), Ok(()));
    after_op(&s);
    assert!(listing(&mut s, vol, Dir::Root).is_ok());
    after_op(&s);
    let r = s.write(vol, dropped, &mut |w| w.put(b"x"));
    assert_eq!(r, Err(StoreError::ReadOnly));
    after_op(&s);
    assert_eq!(s.delete(vol, dropped), Err(StoreError::ReadOnly));
    after_op(&s);
    assert_eq!(s.seal(vol, dropped), Err(StoreError::Io));
    after_op(&s);
    s.make_dir(vol, Dir::Projects).unwrap();
    after_op(&s);
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Projects).unwrap();
    after_op(&s);
    assert_eq!(
        listing(&mut s, vol, Dir::Chimera),
        Ok(Vec::new()),
        "the failed write left nothing"
    );
    assert_eq!(
        listing(&mut s, vol, Dir::Projects),
        Ok(Vec::new()),
        "an empty directory lists"
    );
    after_op(&s);
    assert_eq!(listing(&mut s, vol, Dir::Sounds), Err(StoreError::NotFound));
    after_op(&s);

    // Round trips across the chunk boundaries.
    let mut written = Vec::new();
    for (i, n) in [0usize, 1, 511, 512, 513, 5000].into_iter().enumerate() {
        let f = name(std::format!("F{i}").as_bytes());
        let data = pattern(n);
        let got = s.write(vol, f, &mut |w| w.put(&data)).unwrap();
        assert_eq!(got as usize, n);
        after_op(&s);
        let (r, c) = read_all(&mut s, vol, f, None);
        r.unwrap();
        after_op(&s);
        assert_eq!(c.len, Some(n as u32), "begin gets the exact length");
        assert_eq!(c.data, data, "round trip of {n} bytes");
        written.push((f, n));
    }

    // runs_cover_the_file: a written file's runs span its length.
    let (f, n) = written[5];
    let r = s.runs(vol, f).unwrap();
    assert_eq!(r.len.get() as usize, n);
    let max = BlockCount::new(u32::MAX).unwrap();
    let blocks: u32 = r
        .ext
        .transfers(FileBlock::new(0), u32::MAX, max)
        .map(|(_, b)| b.get())
        .sum();
    assert!(blocks as usize * CHUNK >= n);
    assert!(matches!(
        s.runs(vol, name(b"NORUNS")),
        Err(RunsError::Store(StoreError::NotFound))
    ));
    after_op(&s);

    // Overwriting with less truncates.
    let f = written[5].0;
    s.write(vol, f, &mut |w| w.put(&[9; 10])).unwrap();
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, f, None);
    r.unwrap();
    after_op(&s);
    assert_eq!((c.len, c.data), (Some(10), std::vec![9; 10]));
    written[5].1 = 10;

    // list gives each name with its size.
    written.sort();
    assert_eq!(listing(&mut s, vol, Dir::Chimera), Ok(written.clone()));
    after_op(&s);

    // The same name in another directory is another file.
    let other = FileName::new(Dir::Projects, b"F1", b"BIN").unwrap();
    s.write(vol, other, &mut |w| w.put(b"projects")).unwrap();
    after_op(&s);
    assert_eq!(
        listing(&mut s, vol, Dir::Projects),
        Ok(std::vec![(other, 8)])
    );
    assert_eq!(listing(&mut s, vol, Dir::Chimera), Ok(written.clone()));
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, name(b"F1"), None);
    r.unwrap();
    assert_eq!(c.data, pattern(1));
    after_op(&s);
    s.delete(vol, other).unwrap();
    after_op(&s);

    // Missing files.
    let missing = name(b"NOPE");
    assert_eq!(
        read_all(&mut s, vol, missing, None).0,
        Err(StoreError::NotFound)
    );
    after_op(&s);
    assert_eq!(s.delete(vol, missing), Err(StoreError::NotFound));
    after_op(&s);
    s.delete(vol, written[0].0).unwrap();
    after_op(&s);
    assert_eq!(s.delete(vol, written[0].0), Err(StoreError::NotFound));
    after_op(&s);

    // A sink that stops early, in begin or in a chunk, is not an error and
    // leaves nothing open.
    let big = name(b"F4");
    let (r, c) = read_all(&mut s, vol, big, Some(1));
    assert_eq!(r, Ok(()));
    assert_eq!(c.data.len(), CHUNK);
    after_op(&s);
    let mut c = Collect::new(None);
    c.break_in_begin = true;
    assert_eq!(s.read(vol, big, &mut c), Ok(()));
    assert_eq!((c.len, c.chunks), (Some(513), 0));
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, big, None);
    r.unwrap();
    assert_eq!(c.data, pattern(513));
    after_op(&s);

    // What a failing body wrote stays, over a new file and over an old one,
    // and the store still works.
    let fails = name(b"FAILS");
    let r = s.write(vol, fails, &mut |w| {
        w.put(&[1; 700])?;
        Err(StoreError::Io)
    });
    assert_eq!(r, Err(StoreError::Io));
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, fails, None);
    r.unwrap();
    after_op(&s);
    assert_eq!((c.len, c.data), (Some(700), std::vec![1; 700]));
    let r = s.write(vol, fails, &mut |w| {
        w.put(&[2; 5])?;
        Err(StoreError::Io)
    });
    assert_eq!(r, Err(StoreError::Io));
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, fails, None);
    r.unwrap();
    after_op(&s);
    assert_eq!((c.len, c.data), (Some(5), std::vec![2; 5]));
    let ok = name(b"AFTER");
    s.write(vol, ok, &mut |w| w.put(b"ok")).unwrap();
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, ok, None);
    r.unwrap();
    assert_eq!(c.data, b"ok");
    after_op(&s);
    // Only a source seals: any other file stays writable.
    assert_eq!(s.seal(vol, ok), Err(StoreError::Io));
    after_op(&s);
    s.write(vol, ok, &mut |w| w.put(b"ok")).unwrap();
    after_op(&s);

    // A sealed file reads as it was; nothing rewrites or deletes it.
    s.make_dir(vol, Dir::Tapes).unwrap();
    after_op(&s);
    let tape = FileName::new(Dir::Tapes, b"0A1B2C3D", b"SRC").unwrap();
    assert_eq!(s.seal(vol, tape), Err(StoreError::NotFound));
    after_op(&s);
    s.write(vol, tape, &mut |w| w.put(&pattern(700))).unwrap();
    after_op(&s);
    s.seal(vol, tape).unwrap();
    after_op(&s);
    s.seal(vol, tape).unwrap();
    after_op(&s);
    let r = s.write(vol, tape, &mut |w| w.put(b"over"));
    assert_eq!(r, Err(StoreError::ReadOnly));
    after_op(&s);
    assert_eq!(s.delete(vol, tape), Err(StoreError::ReadOnly));
    after_op(&s);
    let (r, c) = read_all(&mut s, vol, tape, None);
    r.unwrap();
    assert_eq!(c.data, pattern(700), "the sealed file unchanged");
    assert_eq!(listing(&mut s, vol, Dir::Tapes), Ok(std::vec![(tape, 700)]));
    after_op(&s);

    // A swapped card: every op refuses the old id and touches nothing.
    swap(&mut s);
    after_op(&s);
    let stale = name(b"STALE");
    let new = match s.write(vol, stale, &mut |w| w.put(b"x")) {
        Err(StoreError::VolumeChanged(new)) => new,
        other => panic!("expected VolumeChanged, got {other:?}"),
    };
    after_op(&s);
    assert_ne!(new, vol);
    let changed = Err(StoreError::VolumeChanged(new));
    assert_eq!(s.list(vol, Dir::Chimera, &mut |_, _| {}), changed);
    after_op(&s);
    assert_eq!(read_all(&mut s, vol, ok, None).0, changed);
    after_op(&s);
    assert_eq!(s.delete(vol, ok), changed);
    after_op(&s);
    assert_eq!(s.make_dir(vol, Dir::Chimera), changed);
    after_op(&s);
    assert_eq!(s.seal(vol, ok), changed);
    after_op(&s);
    assert_eq!(s.mount().unwrap(), new);
    after_op(&s);
    s.make_dir(new, Dir::Chimera).unwrap();
    after_op(&s);
    assert_eq!(
        listing(&mut s, new, Dir::Chimera),
        Ok(Vec::new()),
        "the stale ops touched nothing"
    );
    after_op(&s);

    // No card: every op says so.
    let mut s = make();
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.write(vol, ok, &mut |w| w.put(b"ok")).unwrap();
    eject(&mut s);
    after_op(&s);
    let no_card = Some(StoreError::NoCard);
    assert_eq!(s.mount().err(), no_card);
    assert_eq!(s.list(vol, Dir::Chimera, &mut |_, _| {}).err(), no_card);
    assert_eq!(read_all(&mut s, vol, ok, None).0.err(), no_card);
    assert_eq!(s.write(vol, ok, &mut |w| w.put(b"x")).err(), no_card);
    assert_eq!(s.delete(vol, ok).err(), no_card);
    assert_eq!(s.make_dir(vol, Dir::Chimera).err(), no_card);
    after_op(&s);
}
