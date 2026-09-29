//! `MemStore` and the conformance suite every `Store` must pass.
extern crate std;

use crate::store::{ByteSink, CHUNK, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use core::ops::ControlFlow;
use std::collections::{BTreeMap, BTreeSet};
use std::vec::Vec;

/// A `Store` in RAM: the reference behaviour the FAT stores are held to.
pub struct MemStore {
    files: BTreeMap<FileName, Vec<u8>>,
    dirs: BTreeSet<Dir>,
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
            id: volume(serial),
            present: true,
        }
    }

    /// A different card, empty, in the slot.
    pub fn swap(&mut self, serial: u32) {
        *self = MemStore::new(serial);
    }

    pub fn eject(&mut self) {
        self.present = false;
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

    fn has_dir(&self, dir: Dir) -> Result<(), StoreError> {
        if self.dirs.contains(&dir) {
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
        self.open(vol)?;
        let data = self.files.get(&file).ok_or(StoreError::NotFound)?;
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
        let mut data = Vec::new();
        let r = body(&mut Buf(&mut data));
        let n = data.len() as u32;
        // What was written stays on Err, as on FAT.
        self.files.insert(file, data);
        r.map(|()| n)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.open(vol)?;
        self.files
            .remove(&file)
            .map(|_| ())
            .ok_or(StoreError::NotFound)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.open(vol)?;
        if dir != Dir::Chimera {
            self.has_dir(Dir::Chimera)?;
        }
        self.dirs.insert(dir);
        Ok(())
    }
}

struct Collect {
    len: Option<u32>,
    data: Vec<u8>,
    /// Break after this many chunks.
    stop_after: Option<usize>,
    chunks: usize,
}

impl Collect {
    fn new(stop_after: Option<usize>) -> Self {
        Collect {
            len: None,
            data: Vec::new(),
            stop_after,
            chunks: 0,
        }
    }
}

impl ReadSink for Collect {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.len = Some(len);
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        assert!(bytes.len() <= CHUNK);
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

/// Run every `Store` requirement against stores from `make`. `swap` puts a
/// different, empty card in the slot; `after_op` runs after every step.
pub fn store_suite<S: Store>(
    make: &mut dyn FnMut() -> S,
    swap: &mut dyn FnMut(&mut S),
    after_op: &mut dyn FnMut(&S),
) {
    let mut s = make();
    let vol = s.mount().unwrap();
    after_op(&s);
    assert_eq!(s.mount().unwrap(), vol, "mount is stable");
    after_op(&s);

    // A directory that was never made.
    assert_eq!(
        s.list(vol, Dir::Chimera, &mut |_, _| {}),
        Err(StoreError::NotFound)
    );
    after_op(&s);
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    after_op(&s);

    let read_all = |s: &mut S, f: FileName, stop: Option<usize>| {
        let mut c = Collect::new(stop);
        let r = s.read(vol, f, &mut c);
        (r, c)
    };

    // Round trips across the chunk boundaries.
    let mut written = Vec::new();
    for (i, n) in [0usize, 1, 511, 512, 513, 5000].into_iter().enumerate() {
        let f = name(std::format!("F{i}").as_bytes());
        let data = pattern(n);
        let got = s.write(vol, f, &mut |w| w.put(&data)).unwrap();
        assert_eq!(got as usize, n);
        after_op(&s);
        let (r, c) = read_all(&mut s, f, None);
        r.unwrap();
        after_op(&s);
        assert_eq!(c.len, Some(n as u32), "begin gets the exact length");
        assert_eq!(c.data, data, "round trip of {n} bytes");
        written.push((f, n));
    }

    // Overwriting with less truncates.
    let f = written[5].0;
    s.write(vol, f, &mut |w| w.put(&[9; 10])).unwrap();
    after_op(&s);
    let (r, c) = read_all(&mut s, f, None);
    r.unwrap();
    after_op(&s);
    assert_eq!((c.len, c.data), (Some(10), std::vec![9; 10]));
    written[5].1 = 10;

    // list gives each name with its size.
    let mut listed = Vec::new();
    s.list(vol, Dir::Chimera, &mut |n, len| {
        listed.push((n, len as usize))
    })
    .unwrap();
    after_op(&s);
    listed.sort();
    written.sort();
    assert_eq!(listed, written);

    // Missing files.
    let missing = name(b"NOPE");
    assert_eq!(read_all(&mut s, missing, None).0, Err(StoreError::NotFound));
    after_op(&s);
    assert_eq!(s.delete(vol, missing), Err(StoreError::NotFound));
    after_op(&s);
    s.delete(vol, written[0].0).unwrap();
    after_op(&s);
    assert_eq!(s.delete(vol, written[0].0), Err(StoreError::NotFound));
    after_op(&s);

    // A sink that stops early is not an error, and leaves nothing open.
    let big = name(b"F4");
    let (r, c) = read_all(&mut s, big, Some(1));
    assert_eq!(r, Ok(()));
    assert_eq!(c.data.len(), CHUNK);
    after_op(&s);
    let (r, c) = read_all(&mut s, big, None);
    r.unwrap();
    assert_eq!(c.data, pattern(513));
    after_op(&s);

    // A body that fails mid-file propagates, and the store still works.
    let f = name(b"FAILS");
    let r = s.write(vol, f, &mut |w| {
        w.put(&[1; 700])?;
        Err(StoreError::Io)
    });
    assert_eq!(r, Err(StoreError::Io));
    after_op(&s);
    let ok = name(b"AFTER");
    s.write(vol, ok, &mut |w| w.put(b"ok")).unwrap();
    after_op(&s);
    let (r, c) = read_all(&mut s, ok, None);
    r.unwrap();
    assert_eq!(c.data, b"ok");
    after_op(&s);

    // A swapped card: the old id is refused and nothing is written.
    swap(&mut s);
    after_op(&s);
    let stale = name(b"STALE");
    let new = match s.write(vol, stale, &mut |w| w.put(b"x")) {
        Err(StoreError::VolumeChanged(new)) => new,
        other => panic!("expected VolumeChanged, got {other:?}"),
    };
    after_op(&s);
    assert_ne!(new, vol);
    assert_eq!(s.mount().unwrap(), new);
    after_op(&s);
    s.make_dir(new, Dir::Chimera).unwrap();
    let mut seen = 0;
    s.list(new, Dir::Chimera, &mut |_, _| seen += 1).unwrap();
    after_op(&s);
    assert_eq!(seen, 0, "the stale write touched nothing");
}
