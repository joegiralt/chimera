//! Sources as card files and project records (ADR 0087, ADR 0093): the
//! `SourceHead`, the source CRC that names the file, `PoolSource` refs in a
//! project, and a load over the budget. 12-bit packing is `twelve_test`'s.

mod common;

use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::name::SoundName;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::{full_dense_slots, save_at};
use chimera_core::project::{
    LoadLink, PartId, Project, ProjectNote, ProjectSource, ReplaceGuard, SlotId, SourceRef,
    load_project, project_file,
};
use chimera_core::source_table::AudioToken;
use chimera_core::storage::{
    Card, Event, FileError, FileKind, Framer, Generation, Header, LoadError, ProjectId, ReadTag,
    RecordTag, SaveError, Side, SourceHead, Wrote, read_source_into, source_crc, source_file,
    write_file, write_source,
};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use common::codec_util::{VecSink, decode_project, encode_project};
use core::ops::ControlFlow;

/// A Plain source of `frames` frames, its bytes a pattern of `seed`.
fn source(seed: u8, frames: u32, name: &str) -> (SourceHead, Vec<u8>) {
    let data: Vec<u8> = (0..3 * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(37).wrapping_add(seed as u32) as u8)
        .collect();
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(Shape::Plain, 1, &data),
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new(name).unwrap(),
    };
    (head, data)
}

/// What the arena holds for a source: its descriptor, then its bytes.
fn held(head: &SourceHead, data: &[u8]) -> Vec<u8> {
    let d = Descriptor {
        shape: head.shape,
        count: head.count,
        rate: head.sref.rate,
        head_frames: 0,
    };
    let mut v = d.to_bytes().to_vec();
    v.extend_from_slice(data);
    v
}

#[test]
fn head_round_trips() {
    for (shape, count) in [
        (Shape::Plain, 1),
        (Shape::Transwave, 32),
        (Shape::ZoneSet, 24),
    ] {
        let h = SourceHead {
            sref: SourceRef {
                crc: 0xDEAD_BEEF,
                frames: 123_457,
                rate: 31_250,
            },
            shape,
            count,
            root: 0xBEEF,
            period_q16: 0x0123_4567,
            name: SoundName::new("CHOIR LOOP").unwrap(),
        };
        let b = h.to_bytes();
        assert_eq!(b.len(), 34);
        assert_eq!(SourceHead::from_bytes(&b), Some(h), "{shape:?}");
        assert_eq!(b[10], shape as u8);
        assert_eq!(b[11], count);
        assert_eq!(&b[12..14], &0xBEEFu16.to_le_bytes());
        assert_eq!(&b[14..18], &0x0123_4567u32.to_le_bytes());
        assert_eq!(&b[18..], &h.name.padded());
        assert_eq!(SourceHead::from_bytes(&b[..33]), None, "short");
        let mut bad = b;
        bad[10] = 3;
        assert_eq!(SourceHead::from_bytes(&bad), None, "an unknown shape");
    }
}

#[test]
fn crc_ignores_labels() {
    let (a, data) = source(1, 600, "A");
    let mut b = a;
    b.name = SoundName::new("ANOTHER").unwrap();
    b.root = 4_321;
    b.period_q16 = 99 << 16;
    // The labels aren't in what the CRC covers at all.
    assert_eq!(source_crc(b.shape, b.count, &data), a.sref.crc);
    assert_ne!(source_crc(Shape::Transwave, 1, &data), a.sref.crc, "shape");
    assert_ne!(source_crc(Shape::Plain, 2, &data), a.sref.crc, "count");
    let mut one = data.clone();
    one[300] ^= 1;
    assert_ne!(source_crc(Shape::Plain, 1, &one), a.sref.crc, "a byte");
    assert_eq!(source_file(a.sref).dir(), Dir::Tapes);
    assert_eq!(
        source_file(a.sref).stem(),
        format!("{:08X}", a.sref.crc).as_bytes()
    );
    assert_eq!(source_file(a.sref).ext(), b"SRC");
}

/// Each record of a project file, in order: tag and payload.
fn records(bytes: &[u8]) -> Vec<(ReadTag, Vec<u8>)> {
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    let mut v = Vec::new();
    f.push(bytes, &mut |e| {
        if let Event::Record(t, p) = e {
            v.push((t, p.to_vec()));
        }
        Ok(())
    })
    .unwrap();
    f.finish().unwrap();
    v
}

fn pool_sources(bytes: &[u8]) -> Vec<Vec<u8>> {
    records(bytes)
        .into_iter()
        .filter(|(t, _)| *t == ReadTag::Known(RecordTag::PoolSource))
        .map(|(_, p)| p)
        .collect()
}

fn sref(crc: u32, frames: u32) -> SourceRef {
    SourceRef {
        crc,
        frames,
        rate: 24_000,
    }
}

#[test]
fn pool_source_round_trips() {
    let (mut p, _) = Project::boxed();
    for (crc, frames) in [(0x0300, 900), (0x0100, 64), (0x0200, 2_001)] {
        p.source_store(sref(crc, frames), &[crc as u8; 40]).unwrap();
    }
    let bytes = encode_project(&p);
    let refs = pool_sources(&bytes);
    // Ordered by ref, not by slot: one canonical file.
    let want: Vec<Vec<u8>> = [(0x0100, 64), (0x0200, 2_001), (0x0300, 900)]
        .map(|(c, f)| sref(c, f).to_bytes().to_vec())
        .into();
    assert_eq!(refs, want);
    assert!(
        records(&bytes)
            .iter()
            .all(|(t, _)| *t != ReadTag::Known(RecordTag::SourceData)),
        "no audio in a project"
    );
    let (mut q, _) = Project::boxed();
    decode_project(&bytes, &mut q).unwrap();
    assert_eq!(encode_project(&q), bytes, "byte-equal");
    // No card read here: each ref is an absent entry, costing nothing.
    for (c, f) in [(0x0100, 64), (0x0200, 2_001), (0x0300, 900)] {
        let id = q.pool().find_source(sref(c, f)).expect("the ref");
        assert!(q.pool().source(id).unwrap().absent());
    }
    assert_eq!(q.pool().source_used(), Cost::default());
}

/// `p` saved as P0000001 on `s`, then loaded into a fresh project.
fn round_trip(s: &mut MemStore, p: &mut Project) -> (Box<Project>, Option<ProjectNote>) {
    let mut card = Card::new();
    assert!(matches!(save_at(&mut card, s, p, 1), ProjectNote::Saved(_)));
    let (mut q, t) = Project::boxed();
    let vol = s.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let out = load_project(&mut card, s, &mut q, go, &LoadLink::new());
    assert!(out.swap.is_some(), "{:?}", out.note);
    (q, out.note)
}

/// Writes `(head, data)` as a source file on `s`.
fn put_source(s: &mut MemStore, head: &SourceHead, data: &[u8]) {
    let mut card = Card::new();
    let wrote = card
        .run(s, |s, r| write_source(s, r, head, data))
        .unwrap()
        .result
        .unwrap();
    assert_eq!(wrote, Wrote::New);
}

/// The bytes the audio reads for source `r`, if the arena holds them.
fn arena_bytes(p: &Project, tok: &mut AudioToken, r: SourceRef) -> Option<Vec<u8>> {
    let id = p.pool().find_source(r)?;
    tok.begin_block().bytes(id).map(<[u8]>::to_vec)
}

#[test]
fn over_budget_loads_in_order() {
    // 48 dense slots, most of the arena between them.
    let (mut p, _) = full_dense_slots(48);
    let slots: Vec<(SlotId, u32)> = SlotId::ALL
        .into_iter()
        .filter_map(|s| p.pool().get(s).map(|e| (s, e.cost().bytes())))
        .collect();
    let all_slots: u32 = slots.iter().map(|x| x.1).sum();
    let cost = |frames: u32| Cost::of(16 + DESC_LEN + 3 * frames.div_ceil(2) as usize).bytes();
    // Part 1's source takes the room left and 3 KB more: the last slots
    // don't fit, then a small source might, and a large one won't.
    let left_after = ARENA_BYTES as u32 - all_slots;
    let big = 2 * (left_after + 3_000) / 3;
    let (part_src, part_data) = source(1, big, "PART");
    let (small, small_data) = source(2, 64, "SMALL");
    let (other, other_data) = source(3, 20_000, "OTHER");

    let mut s = MemStore::new(1);
    for (h, d) in [
        (&part_src, &part_data),
        (&small, &small_data),
        (&other, &other_data),
    ] {
        put_source(&mut s, h, d);
    }
    // In RAM only the refs: the arena couldn't hold all three.
    for h in [&other, &small, &part_src] {
        p.source_absent(h.sref).unwrap();
    }
    p.edit_part(PartId::ALL[0]).sound.source = Some(part_src.sref);
    p.edit_part(PartId::ALL[3]).sound.source = Some(part_src.sref);
    // An older project on both sides first: the load picks this one's
    // side, A, after checking B too, and plans by A's pass 1, not B's.
    let (mut older, _) = full_dense_slots(48);
    for _ in 0..2 {
        let note = save_at(&mut Card::new(), &mut s, &mut older, 1);
        assert!(matches!(note, ProjectNote::Saved(_)), "{note:?}");
    }

    let (q, note) = round_trip(&mut s, &mut p);
    let tok = &mut q.pool().table().reader().expect("one reader");

    // The model: Part 1's source, then the slots by index, then the rest,
    // each taken if it still fits.
    let mut left = ARENA_BYTES as u32 - cost(big);
    let mut kept = Vec::new();
    for &(slot, c) in &slots {
        if c <= left {
            left -= c;
            kept.push(slot);
        }
    }
    let dropped_slots = slots.len() - kept.len();
    assert!(dropped_slots > 0, "some slots don't fit");
    let mut dropped_src = 0;
    for (h, d) in [(&small, &small_data), (&other, &other_data)] {
        let want = held(h, d);
        if cost(h.sref.frames) <= left {
            left -= cost(h.sref.frames);
            assert_eq!(arena_bytes(&q, tok, h.sref), Some(want), "{h:?}");
        } else {
            dropped_src += 1;
            let id = q.pool().find_source(h.sref).expect("the ref kept");
            assert!(q.pool().source(id).unwrap().absent());
        }
    }
    assert!(dropped_src >= 1, "OTHER didn't fit");
    assert_eq!(
        arena_bytes(&q, tok, part_src.sref),
        Some(held(&part_src, &part_data)),
        "Part 1's source first"
    );
    for s in SlotId::ALL {
        let loaded = q.pool().get(s).map(|e| e.bytes().to_vec());
        let want = kept
            .contains(&s)
            .then(|| p.pool().get(s).unwrap().bytes().to_vec());
        assert_eq!(loaded, want, "slot {s:?}");
    }
    assert!(q.pool().used().bytes() <= ARENA_BYTES as u32);
    let note = note.expect("a note");
    assert_eq!(
        note,
        ProjectNote::DidntFit {
            sounds: dropped_slots as u8,
            sources: dropped_src,
        }
    );
    assert_eq!(
        note.line().as_str(),
        format!("LOADED · {dropped_slots} SOUNDS, {dropped_src} SRC DIDN'T FIT")
    );
    assert_eq!(q.meta().saved_crc(), None, "a save would write less");
}

/// Review Focus 5: a ref whose file is missing loads absent, costs nothing,
/// leaves its Part silent, and a save writes it back as it was.
#[test]
fn missing_source_ref_survives_save() {
    let (mut p, _) = Project::boxed();
    let (gone, _) = source(9, 4_000, "GONE");
    let (here, here_data) = source(8, 4_000, "HERE");
    let mut s = MemStore::new(1);
    put_source(&mut s, &here, &here_data);
    p.source_absent(gone.sref).unwrap();
    p.source_absent(here.sref).unwrap();
    p.edit_part(PartId::ALL[1]).sound.source = Some(gone.sref);
    let (mut q, note) = round_trip(&mut s, &mut p);
    let tok = &mut q.pool().table().reader().expect("one reader");
    assert_eq!(note, None, "a missing file is no budget note");
    let id = q.pool().find_source(gone.sref).expect("the ref kept");
    let e = q.pool().source(id).unwrap();
    assert!(e.absent());
    assert_eq!(e.cost(), Cost::default());
    assert_eq!(arena_bytes(&q, tok, gone.sref), None, "Part 2 silent");
    assert_eq!(
        arena_bytes(&q, tok, here.sref),
        Some(held(&here, &here_data))
    );
    assert_eq!(
        q.pool().source_used(),
        Cost::of(16 + DESC_LEN + here_data.len())
    );

    let file = |s: &mut MemStore, side: Side| {
        let vol = s.mount().unwrap();
        let mut c = Bytes(Vec::new());
        s.read(
            vol,
            project_file(ProjectId::new(1).unwrap()).side(side),
            &mut c,
        )
        .unwrap();
        c.0
    };
    let first = pool_sources(&file(&mut s, Side::A));
    // Saved over its own file: side B.
    assert!(matches!(
        save_at(&mut Card::new(), &mut s, &mut q, 1),
        ProjectNote::Saved(_)
    ));
    let again = pool_sources(&file(&mut s, Side::B));
    assert_eq!(first.len(), 2);
    assert_eq!(again, first, "PoolSource bytes identical");
}

/// Reading a source back: its descriptor, then its bytes, and a ref that
/// disagrees with the head is refused.
#[test]
fn read_source_checks_the_ref() {
    let (h, data) = source(4, 1_001, "ODD");
    let mut s = MemStore::new(1);
    put_source(&mut s, &h, &data);
    let mut out = vec![0; DESC_LEN + data.len()];
    let mut card = Card::new();
    let n = card
        .run(&mut s, |s, r| read_source_into(s, r, h.sref, &mut out))
        .unwrap()
        .result
        .unwrap();
    assert_eq!(n, out.len());
    assert_eq!(out, held(&h, &data));
    for wrong in [
        SourceRef {
            frames: h.sref.frames + 1,
            ..h.sref
        },
        SourceRef {
            rate: 32_000,
            ..h.sref
        },
    ] {
        // The same file (the stem is the CRC), named by a ref that lies
        // about it.
        assert_eq!(source_file(wrong), source_file(h.sref));
        let mut c = Card::new();
        let r = c
            .run(&mut s, |s, r| read_source_into(s, r, wrong, &mut out))
            .unwrap()
            .result;
        assert!(r.is_err(), "{wrong:?}");
    }
    let mut short = vec![0; DESC_LEN + data.len() - 1];
    let r = card
        .run(&mut s, |s, r| read_source_into(s, r, h.sref, &mut short))
        .unwrap()
        .result;
    assert!(r.is_err(), "out too short");
    assert_eq!(FileKind::Source as u8, 6);
}

struct Bytes(Vec<u8>);

impl ReadSink for Bytes {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(b);
        ControlFlow::Continue(())
    }
}

/// `head` and `body` as a source file, framed whole: only the reader's own
/// checks stand between it and the arena.
fn put_raw(s: &mut MemStore, head: &SourceHead, body: &[u8]) {
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Tapes).unwrap();
    let h = Header {
        kind: FileKind::Source,
        generation: Generation::FIRST,
        name: None,
    };
    s.write(vol, source_file(head.sref), &mut |sink| {
        write_file(sink, &h, &mut |w| {
            w.put(RecordTag::SourceHead, &head.to_bytes())?;
            body.chunks(504)
                .try_for_each(|c| w.put(RecordTag::SourceData, c))
        })
    })
    .unwrap();
}

/// A file the framer passes, whose samples stop short of its frames or
/// aren't the bytes its CRC names, reads as missing.
#[test]
fn a_framed_but_wrong_source_reads_missing() {
    let (h, data) = source(5, 1_000, "WRONG");
    let mut flipped = data.clone();
    flipped[10] ^= 1;
    // Short, under a CRC its own bytes match: only the length tells.
    let cut = &data[..data.len() - 3];
    let short = SourceHead {
        sref: SourceRef {
            crc: source_crc(Shape::Plain, 1, cut),
            ..h.sref
        },
        ..h
    };
    for (what, head, body) in [("short", short, cut), ("other bytes", h, &flipped[..])] {
        let mut s = MemStore::new(1);
        put_raw(&mut s, &head, body);
        let mut out = vec![0; DESC_LEN + data.len()];
        let r = Card::new()
            .run(&mut s, |s, r| read_source_into(s, r, head.sref, &mut out))
            .unwrap()
            .result;
        assert_eq!(r, Err(LoadError::File(FileError::Corrupt)), "{what}");
    }
    let mut s = MemStore::new(1);
    put_raw(&mut s, &h, &data);
    let mut out = vec![0; DESC_LEN + data.len()];
    let r = Card::new()
        .run(&mut s, |s, r| read_source_into(s, r, h.sref, &mut out))
        .unwrap()
        .result;
    assert_eq!(r, Ok(out.len()), "the same file whole");
}

/// A head and data TAPE couldn't play, or that disagree, are refused
/// before anything is written.
#[test]
fn write_source_refuses_what_tape_cant_play() {
    let (h, data) = source(6, 1_000, "BAD");
    let bad_crc = SourceHead {
        sref: SourceRef {
            crc: h.sref.crc ^ 1,
            ..h.sref
        },
        ..h
    };
    let (short, short_data) = source(6, 62, "SHORT");
    let slow = SourceHead {
        sref: SourceRef {
            rate: 3_999,
            ..h.sref
        },
        ..h
    };
    let two = SourceHead {
        count: 2,
        sref: SourceRef {
            crc: source_crc(Shape::Plain, 2, &data),
            ..h.sref
        },
        ..h
    };
    let cases: [(&str, SourceHead, &[u8]); 5] = [
        ("crc", bad_crc, &data),
        ("62 frames", short, &short_data),
        ("rate", slow, &data),
        ("a Plain of 2", two, &data),
        ("length", h, &data[..data.len() - 3]),
    ];
    for (what, head, d) in cases {
        let mut s = MemStore::new(1);
        let r = Card::new()
            .run(&mut s, |s, r| write_source(s, r, &head, d))
            .unwrap()
            .result;
        assert_eq!(r, Err(SaveError::File(FileError::Bounds)), "{what}");
        let vol = s.mount().unwrap();
        let none = s.list(vol, Dir::Chimera, &mut |_, _| {});
        assert_eq!(none, Err(StoreError::NotFound), "{what}: nothing written");
    }
}

/// A store that garbles one byte of each source file it writes while
/// `garble`, and fails every source read with `Io` while `fail_reads`,
/// counting them.
struct Odd {
    inner: MemStore,
    garble: bool,
    fail_reads: bool,
    tape_reads: u32,
}

/// Flips the 600th byte put through it.
struct Garbled<'a> {
    inner: &'a mut dyn ByteSink,
    at: usize,
}

impl ByteSink for Garbled<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        let mut v = b.to_vec();
        if (0..v.len()).contains(&(600usize.wrapping_sub(self.at))) {
            v[600 - self.at] ^= 0x08;
        }
        self.at += b.len();
        self.inner.put(&v)
    }
}

impl Store for Odd {
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
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if file.dir() == Dir::Tapes {
            self.tape_reads += 1;
            if self.fail_reads {
                return Err(StoreError::Io);
            }
        }
        self.inner.read(vol, file, sink)
    }
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        if !(self.garble && file.dir() == Dir::Tapes) {
            return self.inner.write(vol, file, body);
        }
        self.inner
            .write(vol, file, &mut |w| body(&mut Garbled { inner: w, at: 0 }))
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

fn odd() -> Odd {
    Odd {
        inner: MemStore::new(1),
        garble: false,
        fail_reads: false,
        tape_reads: 0,
    }
}

/// A write that doesn't read back whole is never sealed: the next write
/// replaces it.
#[test]
fn write_source_verifies_before_sealing() {
    let (h, data) = source(7, 1_000, "GARBLE");
    let mut s = odd();
    s.garble = true;
    let r = Card::new()
        .run(&mut s, |s, r| write_source(s, r, &h, &data))
        .unwrap()
        .result;
    assert_eq!(r, Err(SaveError::File(FileError::BadCrc)));
    s.garble = false;
    let r = Card::new()
        .run(&mut s, |s, r| write_source(s, r, &h, &data))
        .unwrap()
        .result;
    assert_eq!(r, Ok(Wrote::Replaced), "unsealed, so replaced");
}

/// A card fault reading one source ends the load's source reads: the
/// rest stay absent, and the project still loads.
#[test]
fn a_card_fault_ends_the_source_reads() {
    let mut s = odd();
    let (mut p, _) = Project::boxed();
    for seed in 0..3 {
        let (h, d) = source(20 + seed, 600, "SRC");
        put_source(&mut s.inner, &h, &d);
        p.source_absent(h.sref).unwrap();
    }
    assert!(matches!(
        save_at(&mut Card::new(), &mut s, &mut p, 1),
        ProjectNote::Saved(_)
    ));
    s.fail_reads = true;
    let (mut q, t) = Project::boxed();
    let vol = s.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let out = load_project(&mut Card::new(), &mut s, &mut q, go, &LoadLink::new());
    assert!(out.swap.is_some(), "{:?}", out.note);
    assert_eq!(s.tape_reads, 1, "one fault, no more reads");
    assert_eq!(q.pool().source_used(), Cost::default());
}

/// `PoolSource` rules: a 9-byte payload is Bounds, a ref twice is
/// Corrupt, a 33rd is Bounds, and one past the Parts is Corrupt.
#[test]
fn pool_source_rules() {
    let (p, _) = Project::boxed();
    let file = |pre: &[Vec<u8>], post: &[Vec<u8>]| {
        let h = Header {
            kind: FileKind::Project,
            generation: Generation::FIRST,
            name: Some(p.meta().name()),
        };
        let mut sink = VecSink(Vec::new());
        write_file(&mut sink, &h, &mut |w| {
            for r in pre {
                w.put(RecordTag::PoolSource, r)?;
            }
            chimera_core::project::encode_project(&p, w)?;
            for r in post {
                w.put(RecordTag::PoolSource, r)?;
            }
            Ok(())
        })
        .unwrap();
        sink.0
    };
    let r = |i: u32| sref(i, 64).to_bytes().to_vec();
    let decode = |bytes: Vec<u8>| decode_project(&bytes, &mut Project::boxed().0);
    assert_eq!(
        decode(file(&[r(1)[..9].to_vec()], &[])),
        Err(FileError::Bounds)
    );
    assert_eq!(decode(file(&[r(1), r(1)], &[])), Err(FileError::Corrupt));
    let all: Vec<Vec<u8>> = (0..32).map(r).collect();
    assert_eq!(decode(file(&all, &[])), Ok(()));
    let over: Vec<Vec<u8>> = (0..33).map(r).collect();
    assert_eq!(decode(file(&over, &[])), Err(FileError::Bounds));
    assert_eq!(decode(file(&[], &[r(1)])), Err(FileError::Corrupt));
}
