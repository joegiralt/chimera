// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `put` for TAPE sources (ADR 0106): 256 KiB chunks, each ended by its
//! CRC and acked, the file checked as it streams, written once, read back
//! and sealed; any failure leaves no file, and a re-run is the retry.
#![cfg(feature = "card-put")]

use chimera_core::console::{
    Console, Out, PUT_CHUNK_BYTES, PUT_CHUNK_DEADLINE_MS, Request, Stalled, put,
};
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Shape};
use chimera_core::name::SoundName;
use chimera_core::project::encode_new_project;
use chimera_core::storage::{
    Card, Crc32, FileKind, Generation, Header, RecordTag, SOURCE_DATA_MAX, SourceHead, SourceRef,
    WholeShape, read_source_into, source_crc, source_name, write_file,
};
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, Dir, FileName, Files, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use std::collections::VecDeque;

/// A `MemStore` that `put` can cap, counting the bytes written.
struct Mem {
    inner: MemStore,
    written: usize,
}

struct Counted<'a> {
    sink: &'a mut dyn ByteSink,
    n: &'a mut usize,
}

impl ByteSink for Counted<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        *self.n += b.len();
        self.sink.put(b)
    }
}

impl Store for Mem {
    fn clear_read_fault(&mut self) {}
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }
    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.inner.list(vol, dir, f)
    }
    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        s: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.inner.read(vol, file, s)
    }
    fn read_at(
        &mut self,
        vol: VolumeId,
        file: FileName,
        offset: u32,
        s: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.inner.read_at(vol, file, offset, s)
    }
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        let n = &mut self.written;
        self.inner.write(vol, file, &mut |sink| {
            body(&mut Counted { sink, n: &mut *n })
        })
    }
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.inner.delete(vol, file)
    }
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(vol, dir)
    }
    fn seal(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.inner.seal(vol, file)
    }
}

impl Files for Mem {
    fn cap(&mut self, _ms: Option<u32>) {}
}

fn mem() -> Mem {
    Mem {
        inner: MemStore::new(7),
        written: 0,
    }
}

/// The host: what it sends after the request line, and a clock in µs that
/// moves `per_byte_us` a byte and the whole wait when nothing comes.
struct Host {
    input: VecDeque<u8>,
    out: Vec<u8>,
    us: u64,
    per_byte_us: u64,
}

impl Out for Host {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let b = self.input.pop_front();
        self.us += match b {
            Some(_) => self.per_byte_us,
            None => u64::from(wait_ms) * 1_000,
        };
        b
    }
    fn now(&self) -> Ms {
        Ms((self.us / 1_000) as u32)
    }
}

/// `line` through the console's parser and `put`: what the unit said, and
/// how many of the host's bytes it left unread.
fn ask(s: &mut Mem, line: &str, data: &[u8], per_byte_us: u64) -> (String, usize) {
    ask_from(s, line, data, per_byte_us, 0)
}

/// `ask` with the host's clock starting at `us`.
fn ask_from(s: &mut Mem, line: &str, data: &[u8], per_byte_us: u64, us: u64) -> (String, usize) {
    let mut host = Host {
        input: data.iter().copied().collect(),
        out: Vec::new(),
        us,
        per_byte_us,
    };
    let mut c = Console::new();
    for &b in line.as_bytes() {
        if let Some(r) = c.push(b) {
            match r {
                Ok(Request::Put(arg)) => put(arg, None, None, s, &mut host).unwrap(),
                other => panic!("{other:?}"),
            }
        }
    }
    (String::from_utf8(host.out).unwrap(), host.input.len())
}

fn crc(b: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(b);
    c.finish()
}

fn base64(b: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for c in b.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &x)| n | u32::from(x) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() {
                A[(n >> (18 - 6 * i) & 63) as usize]
            } else {
                b'='
            });
        }
    }
    out
}

/// One chunk's data as the host tool sends it: 48 bytes a line.
fn lines(b: &[u8]) -> Vec<u8> {
    b.chunks(48)
        .flat_map(|c| [base64(c), b"\n".to_vec()].concat())
        .collect()
}

/// The file as the host sends it: each chunk's lines, then `end <crc>`.
fn chunked(file: &[u8]) -> Vec<u8> {
    file.chunks(PUT_CHUNK_BYTES as usize)
        .flat_map(|c| [lines(c), format!("end {:08x}\n", crc(c)).into_bytes()].concat())
        .collect()
}

/// A plain source of `frames` and its whole file, as the importer writes it.
fn source(seed: u8, frames: u32) -> (SourceHead, Vec<u8>) {
    let data: Vec<u8> = (0..3 * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(29).wrapping_add(u32::from(seed)) as u8)
        .collect();
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_900,
        period_q16: 0,
        name: SoundName::new("TAPE").unwrap(),
    };
    let h = Header {
        kind: FileKind::Source,
        generation: Generation::FIRST,
        name: None,
    };
    let mut file = Vec::new();
    write_file(&mut Vec8(&mut file), &h, &mut |w| {
        w.put(RecordTag::SourceHead, &head.to_bytes())?;
        data.chunks(SOURCE_DATA_MAX)
            .try_for_each(|c| w.put(RecordTag::SourceData, c))
    })
    .unwrap();
    (head, file)
}

struct Vec8<'a>(&'a mut Vec<u8>);

impl ByteSink for Vec8<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

/// About 600 KB: three chunks.
fn big() -> (SourceHead, Vec<u8>) {
    let (h, f) = source(3, 400_000);
    assert!(f.len() > 2 * PUT_CHUNK_BYTES as usize && f.len() < 3 * PUT_CHUNK_BYTES as usize);
    (h, f)
}

fn path(stem: u32) -> String {
    format!("/CHIMERA/TAPES/{stem:08X}.SRC")
}

fn put_line(stem: u32, file: &[u8]) -> String {
    format!("put {} {} {:08x}\n", path(stem), file.len(), crc(file))
}

fn acks(file: &[u8]) -> String {
    (1..=file.len().div_ceil(PUT_CHUNK_BYTES as usize))
        .map(|k| format!("ACK {}\n", (k * PUT_CHUNK_BYTES as usize).min(file.len())))
        .collect()
}

/// The files in TAPES.
fn tapes(s: &mut Mem) -> Vec<(String, u32)> {
    let vol = s.mount().unwrap();
    let mut v = Vec::new();
    let _ = s.list(vol, Dir::Tapes, &mut |f, n| {
        v.push((String::from_utf8(f.stem().to_vec()).unwrap(), n))
    });
    v
}

fn read_file(s: &mut Mem, f: FileName) -> Option<Vec<u8>> {
    struct Sink(Vec<u8>);
    impl ReadSink for Sink {
        fn begin(&mut self, _: u32) -> core::ops::ControlFlow<()> {
            core::ops::ControlFlow::Continue(())
        }
        fn chunk(&mut self, b: &[u8]) -> core::ops::ControlFlow<()> {
            self.0.extend_from_slice(b);
            core::ops::ControlFlow::Continue(())
        }
    }
    let vol = s.mount().unwrap();
    let mut k = Sink(Vec::new());
    s.read(vol, f, &mut k).ok().map(|()| k.0)
}

/// The source loads as a Part's SOURCE would, and the file is sealed.
fn loads_sealed(s: &mut Mem, h: &SourceHead) {
    let mut out = vec![0; DESC_LEN + 3 * h.sref.frames.div_ceil(2) as usize];
    let n = Card::new()
        .run(s, |s, r| read_source_into(s, r, h.sref, &mut out))
        .and_then(|o| o.result)
        .unwrap();
    assert_eq!(n, out.len());
    let vol = s.mount().unwrap();
    assert_eq!(
        s.delete(vol, source_name(h.sref.stem())),
        Err(StoreError::ReadOnly),
        "sealed"
    );
}

#[test]
fn source_put_writes_sealed() {
    let mut s = mem();
    let (h, file) = big();
    let stem = h.sref.crc;
    let (said, unread) = ask(&mut s, &put_line(stem, &file), &chunked(&file), 0);
    assert_eq!(said, format!("READY\n{}OK\n", acks(&file)));
    assert_eq!(unread, 0);
    assert_eq!(acks(&file).lines().count(), 3);
    assert_eq!(
        read_file(&mut s, source_name(chimera_core::storage::Stem::new(stem))).unwrap(),
        file,
        "as sent"
    );
    loads_sealed(&mut s, &h);
}

#[test]
fn present_answers_without_data() {
    let mut s = mem();
    let (h, file) = source(1, 4_000);
    let line = put_line(h.sref.crc, &file);
    let data = chunked(&file);
    assert_eq!(
        ask(&mut s, &line, &data, 0).0,
        format!("READY\n{}OK\n", acks(&file))
    );
    let before = s.written;
    let (said, unread) = ask(&mut s, &line, &data, 0);
    assert_eq!(said, "OK PRESENT\n");
    assert_eq!(unread, data.len(), "no data read");
    assert_eq!(s.written, before, "no block written");
    loads_sealed(&mut s, &h);
}

#[test]
fn damaged_never_rewritten() {
    let mut s = mem();
    let (h, file) = source(1, 4_000);
    let mut bad = file.clone();
    bad[200] ^= 1;
    let (vol, name) = (s.mount().unwrap(), source_name(h.sref.stem()));
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Tapes).unwrap();
    s.write(vol, name, &mut |w| w.put(&bad)).unwrap();
    s.seal(vol, name).unwrap();
    let (said, _) = ask(&mut s, &put_line(h.sref.crc, &file), &chunked(&file), 0);
    assert_eq!(said, "ERR put: damaged\n");
    assert_eq!(read_file(&mut s, name).unwrap(), bad, "unchanged");
}

#[test]
fn bad_chunk_crc_deletes() {
    let mut s = mem();
    let (h, file) = big();
    let c = PUT_CHUNK_BYTES as usize;
    let mut data = lines(&file[..c]);
    data.extend(format!("end {:08x}\n", crc(&file[..c])).bytes());
    data.extend(lines(&file[c..2 * c]));
    data.extend(format!("end {:08x}\n", crc(&file[c..2 * c]) ^ 1).bytes());
    data.extend(lines(&file[2 * c..]));
    data.extend(format!("end {:08x}\n", crc(&file[2 * c..])).bytes());
    let (said, unread) = ask(&mut s, &put_line(h.sref.crc, &file), &data, 0);
    assert_eq!(said, format!("READY\nACK {c}\nERR put: chunk\n"));
    assert_eq!(unread, 0, "drained");
    assert_eq!(tapes(&mut s), []);
}

#[test]
fn stem_mismatch_refused_early() {
    let mut s = mem();
    let (h, file) = big();
    let other = h.sref.crc ^ 0x10;
    let (said, _) = ask(&mut s, &put_line(other, &file), &chunked(&file), 0);
    assert_eq!(said, "READY\nERR put: CRC isn't its name\n");
    assert_eq!(s.written, 0, "refused at the head's record, before a block");
    assert_eq!(tapes(&mut s), []);
}

#[test]
fn broken_put_then_rerun_writes_once() {
    let mut s = mem();
    let (h, file) = big();
    let line = put_line(h.sref.crc, &file);
    let all = chunked(&file);
    let c = PUT_CHUNK_BYTES as usize;
    // Silent mid-chunk 2.
    let cut = lines(&file[..c]).len() + format!("end {:08x}\n", 0).len() + 100 * 65;
    let (said, _) = ask(&mut s, &line, &all[..cut], 0);
    assert_eq!(
        said,
        format!("READY\nACK {c}\nERR put: no block for 500 ms\n")
    );
    assert_eq!(tapes(&mut s), []);
    let (said, _) = ask(&mut s, &line, &all, 0);
    assert_eq!(said, format!("READY\n{}OK\n", acks(&file)));
    assert_eq!(
        tapes(&mut s),
        [(format!("{:08X}", h.sref.crc), file.len() as u32)]
    );
    loads_sealed(&mut s, &h);
}

#[test]
fn a_torn_unsealed_file_is_replaced() {
    let mut s = mem();
    let (h, file) = source(2, 4_000);
    let (vol, name) = (s.mount().unwrap(), source_name(h.sref.stem()));
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Tapes).unwrap();
    s.write(vol, name, &mut |w| w.put(&file[..1_000])).unwrap();
    let (said, _) = ask(&mut s, &put_line(h.sref.crc, &file), &chunked(&file), 0);
    assert_eq!(said, format!("READY\n{}OK\n", acks(&file)));
    loads_sealed(&mut s, &h);
}

/// Each chunk has its own 30 s (`PUT_CHUNK_DEADLINE_MS`): a host that
/// takes longer than that for the whole file at 15 s a chunk lands it; one
/// that takes 35 s for a chunk doesn't.
#[test]
fn each_chunk_has_its_own_deadline() {
    let (h, file) = big();
    let data = chunked(&file);
    let per_chunk = lines(&file[..PUT_CHUNK_BYTES as usize]).len() as u64;
    let line = put_line(h.sref.crc, &file);
    let deadline_us = u64::from(PUT_CHUNK_DEADLINE_MS) * 1000;
    assert_eq!(PUT_CHUNK_DEADLINE_MS, 30_000);
    let us = deadline_us / 2 / per_chunk;
    assert!(data.len() as u64 * us > deadline_us);
    let mut s = mem();
    assert_eq!(
        ask(&mut s, &line, &data, us).0,
        format!("READY\n{}OK\n", acks(&file))
    );
    let mut s = mem();
    let us = (deadline_us + 5_000_000) / per_chunk;
    assert_eq!(ask(&mut s, &line, &data, us).0, "READY\nERR put: timeout\n");
    assert_eq!(tapes(&mut s), []);
}

/// The same across the link clock's u32 wrap: the put starts 10 s before
/// it, so a chunk's 30 s straddles it.
#[test]
fn each_chunk_has_its_own_deadline_across_the_wrap() {
    let (h, file) = big();
    let data = chunked(&file);
    let per_chunk = lines(&file[..PUT_CHUNK_BYTES as usize]).len() as u64;
    let line = put_line(h.sref.crc, &file);
    let deadline_us = u64::from(PUT_CHUNK_DEADLINE_MS) * 1000;
    let start = (u64::from(u32::MAX) - 10_000) * 1000;
    let us = deadline_us / 2 / per_chunk;
    assert!(data.len() as u64 * us > deadline_us);
    let mut s = mem();
    assert_eq!(
        ask_from(&mut s, &line, &data, us, start).0,
        format!("READY\n{}OK\n", acks(&file))
    );
    let mut s = mem();
    let us = (deadline_us + 5_000_000) / per_chunk;
    assert_eq!(
        ask_from(&mut s, &line, &data, us, start).0,
        "READY\nERR put: timeout\n"
    );
    assert_eq!(tapes(&mut s), []);
}

#[test]
fn a_file_that_isnt_a_source_is_refused() {
    let mut s = mem();
    let (h, mut file) = source(1, 4_000);
    file[6] = FileKind::Sound as u8;
    let (said, _) = ask(&mut s, &put_line(h.sref.crc, &file), &chunked(&file), 0);
    assert_eq!(said, "READY\nERR put: WRONG FILE TYPE\n");
    assert_eq!(tapes(&mut s), []);
}

#[test]
fn resume_is_reserved() {
    let mut s = mem();
    let (h, file) = source(1, 4_000);
    let line = format!(
        "put {} {} {:08x} resume\n",
        path(h.sref.crc),
        file.len(),
        crc(&file)
    );
    let (said, unread) = ask(&mut s, &line, &chunked(&file), 0);
    assert_eq!(said, "ERR put: no resume\n");
    assert_eq!(unread, chunked(&file).len());
    assert_eq!(s.written, 0);
}

/// A TAPES path never takes the project protocol: no `end` line, no ACK.
#[test]
fn a_source_without_chunk_ends_fails() {
    let mut s = mem();
    let (h, file) = source(1, 4_000);
    let (said, _) = ask(&mut s, &put_line(h.sref.crc, &file), &lines(&file), 0);
    assert_eq!(said, "READY\nERR put: no block for 500 ms\n");
    assert_eq!(tapes(&mut s), []);
}

#[test]
fn project_put_unchanged() {
    let mut s = mem();
    let h = Header {
        kind: FileKind::Project,
        generation: Generation::FIRST,
        name: Some(chimera_core::name::Name::new("AVRIL").unwrap()),
    };
    let mut file = Vec::new();
    write_file(&mut Vec8(&mut file), &h, &mut |w| encode_new_project(w)).unwrap();
    let line = format!(
        "put /CHIMERA/PROJECTS/P0000014.A {} {:08x}\n",
        file.len(),
        crc(&file)
    );
    assert_eq!(ask(&mut s, &line, &lines(&file), 0).0, "READY\nOK\n");
}
