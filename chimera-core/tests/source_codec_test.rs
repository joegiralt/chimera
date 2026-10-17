//! Sources as card files and project records (ADR 0087, ADR 0093): the
//! `SourceHead`, the source CRC that names the file, `PoolSource` refs in a
//! project, and a load over the budget. 12-bit packing is `twelve_test`'s.

mod common;

use chimera_core::dsp::tape_engine::TapeModel;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::name::SoundName;
use chimera_core::project::Absence;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::{full_dense_slots, save_at};
use chimera_core::project::{
    LoadLink, PartId, Project, ProjectDecoder, ProjectNote, ProjectSource, ReplaceGuard, SlotId,
    SourceRef, load_project, project_file,
};
use chimera_core::source_table::AudioToken;
use chimera_core::storage::{
    Card, DecodeInPlace, Event, FileError, FileKind, Framer, Generation, Header, LoadError,
    ProjectId, ReadTag, RecordTag, RecordWriter, SaveError, Side, SourceHead, WholeShape, Wrote,
    read_source_into, source_crc, source_file, source_file_min_len, write_file, write_source,
};
use chimera_core::ui::view::SourceState;
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
            crc: source_crc(WholeShape::Plain, 1, &data),
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
        heads: 0,
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
    assert_eq!(source_crc(WholeShape::Plain, b.count, &data), a.sref.crc);
    assert_ne!(
        source_crc(WholeShape::Transwave, 1, &data),
        a.sref.crc,
        "shape"
    );
    assert_ne!(source_crc(WholeShape::Plain, 2, &data), a.sref.crc, "count");
    let mut one = data.clone();
    one[300] ^= 1;
    assert_ne!(source_crc(WholeShape::Plain, 1, &one), a.sref.crc, "a byte");
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
    // In slot order, after `Fx` and its blocks, before the first `Slot`.
    let want: Vec<Vec<u8>> = [(0x0300, 900), (0x0100, 64), (0x0200, 2_001)]
        .map(|(c, f)| sref(c, f).to_bytes().to_vec())
        .into();
    assert_eq!(refs, want);
    let tags: Vec<ReadTag> = records(&bytes).into_iter().map(|(t, _)| t).collect();
    let at = |t: RecordTag| tags.iter().position(|&x| x == ReadTag::Known(t)).unwrap();
    let (fx, first, slot) = (
        at(RecordTag::Fx),
        at(RecordTag::PoolSource),
        at(RecordTag::Slot),
    );
    assert!(fx < first && first + 3 <= slot);
    assert!(
        tags[fx + 1..first]
            .iter()
            .all(|&t| t == ReadTag::Known(RecordTag::Block))
    );
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
    for (slot, (c, f)) in [(0x0300, 900), (0x0100, 64), (0x0200, 2_001)]
        .into_iter()
        .enumerate()
    {
        let id = q.pool().find_source(sref(c, f)).expect("the ref");
        assert_eq!(id.index(), slot, "the same slot");
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
    p.edit_part(PartId::ALL[0]).sound.source = Some(part_src.sref.into());
    p.edit_part(PartId::ALL[3]).sound.source = Some(part_src.sref.into());
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
    p.edit_part(PartId::ALL[1]).sound.source = Some(gone.sref.into());
    let (mut q, note) = round_trip(&mut s, &mut p);
    let tok = &mut q.pool().table().reader().expect("one reader");
    assert_eq!(
        note,
        Some(ProjectNote::SourcesMissing(1)),
        "named, not a budget note"
    );
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
            crc: source_crc(WholeShape::Plain, 1, cut),
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
        assert_eq!(r, Err(LoadError::File(FileError::Corrupt).into()), "{what}");
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
            crc: source_crc(WholeShape::Plain, 2, &data),
            ..h.sref
        },
        ..h
    };
    let zones = zone_set(&h, &data);
    let cases: [(&str, SourceHead, &[u8]); 6] = [
        ("crc", bad_crc, &data),
        ("62 frames", short, &short_data),
        ("rate", slow, &data),
        ("a Plain of 2", two, &data),
        ("length", h, &data[..data.len() - 3]),
        ("a ZoneSet head alone, no zones", zones, &data),
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
    let mut refs = Vec::new();
    for seed in 0..3 {
        let (h, d) = source(20 + seed, 600, "SRC");
        put_source(&mut s.inner, &h, &d);
        p.source_absent(h.sref).unwrap();
        refs.push(h.sref);
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
    // Reported as the card's fault, not as missing (#372).
    for r in refs {
        let id = q.pool().find_source(r).unwrap();
        assert_eq!(q.pool().absence(id), Some(Absence::ReadFailed));
        let state = SourceState::of(TapeModel::Play, Some(r), q.pool());
        assert_eq!(state, SourceState::Absent(Absence::ReadFailed));
    }
}

/// An absent source says why (#279, #372): no file on the card is
/// SOURCE NOT ON CARD; a damaged one, SOURCE MISSING.
#[test]
fn absent_sources_say_why() {
    let mut s = odd();
    let (mut p, _) = Project::boxed();
    let (on, d) = source(30, 600, "ON");
    let (off, _) = source(31, 600, "OFF");
    put_source(&mut s.inner, &on, &d);
    s.garble = true;
    let (bad, bd) = source(32, 600, "BAD");
    let _ = Card::new().run(&mut s, |s, r| write_source(s, r, &bad, &bd));
    s.garble = false;
    for r in [on.sref, off.sref, bad.sref] {
        p.source_absent(r).unwrap();
    }
    assert!(matches!(
        save_at(&mut Card::new(), &mut s, &mut p, 1),
        ProjectNote::Saved(_)
    ));
    let (mut q, t) = Project::boxed();
    let vol = s.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let _ = load_project(&mut Card::new(), &mut s, &mut q, go, &LoadLink::new());
    let why = |r| q.pool().absence(q.pool().find_source(r).unwrap());
    assert_eq!(why(on.sref), None, "held");
    assert_eq!(why(off.sref), Some(Absence::NotOnCard));
    assert_eq!(why(bad.sref), Some(Absence::Missing));
    let text = |r| {
        let mut b = chimera_core::ui::fmt::FmtBuf::new();
        SourceState::of(TapeModel::Play, Some(r), q.pool()).text(&mut b);
        b.as_str().to_string()
    };
    assert_eq!(text(off.sref), "SOURCE NOT ON CARD");
    assert_eq!(text(bad.sref), "SOURCE MISSING");
}

/// `PoolSource` rules: only between `Fx`'s blocks and the first `Slot`; a
/// 9-byte payload is Bounds, a ref twice is Corrupt, a 33rd is Bounds, and
/// one before `Fx` or past the Parts is Corrupt.
#[test]
fn pool_source_rules() {
    let (p, _) = Project::boxed();
    let body = records(&encode_project(&p));
    // `p`'s records with `at` before `Fx`, `mid` after `Fx`'s blocks and
    // `end` after ORBIT's.
    let file = |at: &[Vec<u8>], mid: &[Vec<u8>], end: &[Vec<u8>]| {
        let h = Header {
            kind: FileKind::Project,
            generation: Generation::FIRST,
            name: Some(p.meta().name()),
        };
        let pool = |w: &mut RecordWriter<'_>, rs: &[Vec<u8>]| {
            rs.iter().try_for_each(|r| w.put(RecordTag::PoolSource, r))
        };
        let slot = body
            .iter()
            .position(|(t, _)| *t == ReadTag::Known(RecordTag::Slot))
            .unwrap();
        let mut sink = VecSink(Vec::new());
        write_file(&mut sink, &h, &mut |w| {
            pool(w, at)?;
            for (i, (t, payload)) in body.iter().enumerate() {
                if i == slot {
                    pool(w, mid)?;
                }
                let ReadTag::Known(t) = *t else {
                    unreachable!()
                };
                w.put(t, payload)?;
            }
            pool(w, end)
        })
        .unwrap();
        sink.0
    };
    let r = |i: u32| sref(i, 64).to_bytes().to_vec();
    let decode = |bytes: Vec<u8>| decode_project(&bytes, &mut Project::boxed().0);
    assert_eq!(decode(file(&[], &[], &[])), Ok(()));
    assert_eq!(
        decode(file(&[], &[r(1)[..9].to_vec()], &[])),
        Err(FileError::Bounds)
    );
    assert_eq!(
        decode(file(&[], &[r(1), r(1)], &[])),
        Err(FileError::Corrupt)
    );
    let all: Vec<Vec<u8>> = (0..32).map(r).collect();
    assert_eq!(decode(file(&[], &all, &[])), Ok(()));
    let over: Vec<Vec<u8>> = (0..33).map(r).collect();
    assert_eq!(decode(file(&[], &over, &[])), Err(FileError::Bounds));
    assert_eq!(
        decode(file(&[r(1)], &[], &[])),
        Err(FileError::Corrupt),
        "before Fx"
    );
    assert_eq!(
        decode(file(&[], &[], &[r(1)])),
        Err(FileError::Corrupt),
        "past ORBIT"
    );
}

/// The CRC the pool once gave a zone set: `[ZoneSet, count]` and the
/// samples, no layout. `source_crc` no longer takes a `ZoneSet`.
fn layoutless_crc(count: u8, packed: &[u8]) -> u32 {
    let mut c = chimera_core::storage::Crc32::new();
    c.update(&[Shape::ZoneSet as u8, count]);
    c.update(packed);
    c.finish()
}

/// `h`'s samples as a 4-zone `ZoneSet` head, its CRC with no layout
/// folded in (`zone_set_crc` folds it).
fn zone_set(h: &SourceHead, data: &[u8]) -> SourceHead {
    SourceHead {
        shape: Shape::ZoneSet,
        count: 4,
        sref: SourceRef {
            crc: layoutless_crc(4, data),
            ..h.sref
        },
        ..*h
    }
}

/// A framed file whose CRC and length match its head, but whose head TAPE
/// can't play (too short), or a `ZoneSet` whose samples come with no
/// `SourceZones` record, reads as missing: nothing enters the arena.
#[test]
fn an_unplayable_head_reads_missing() {
    let (short, short_data) = source(30, 62, "SHORT");
    let (h, data) = source(31, 1_000, "ZONES");
    let zones = zone_set(&h, &data);
    for (what, head, body, err) in [
        ("62 frames", short, &short_data, FileError::Bounds),
        ("a ZoneSet with no zones", zones, &data, FileError::Corrupt),
    ] {
        let mut s = MemStore::new(1);
        put_raw(&mut s, &head, body);
        let mut out = vec![0; DESC_LEN + body.len()];
        let r = Card::new()
            .run(&mut s, |s, r| read_source_into(s, r, head.sref, &mut out))
            .unwrap()
            .result;
        assert_eq!(r, Err(LoadError::File(err).into()), "{what}");
    }
}

/// A project's zone set loads whole through pass 2: the arena holds its
/// descriptor, its layout and its samples, and costs what they take.
#[test]
fn a_zone_set_loads_with_its_layout() {
    use chimera_core::storage::{SourceLayout, SourceZones, Zone, write_descriptor, zone_set_crc};
    let zones: Vec<Zone> = (0..3)
        .map(|i| Zone {
            start: 500 * i,
            frames: 500,
            key: 48 + 12 * i as u8,
            root: 4_800 + 1_200 * i as u16,
            period_q16: 0,
        })
        .collect();
    let z = SourceZones::new(&zones).unwrap();
    let (plain, data) = source(38, 1_500, "TRON");
    let h = SourceHead {
        shape: Shape::ZoneSet,
        count: 3,
        sref: SourceRef {
            crc: zone_set_crc(&z, &data),
            ..plain.sref
        },
        ..plain
    };
    let mut s = MemStore::new(1);
    let w = Card::new().run(&mut s, |s, r| {
        chimera_core::storage::write_zone_set(s, r, &h, &z, &data)
    });
    assert_eq!(w.unwrap().result, Ok(Wrote::New));
    let l = SourceLayout::zoned(h, z).unwrap();
    let mut want = vec![0; l.held_len()];
    let at = write_descriptor(&l, &mut want);
    want[at..].copy_from_slice(&data);

    let (mut p, _) = Project::boxed();
    p.source_absent(h.sref).unwrap();
    let (q, note) = round_trip(&mut s, &mut p);
    assert_eq!(note, None);
    let tok = &mut q.pool().table().reader().expect("one reader");
    assert_eq!(arena_bytes(&q, tok, h.sref), Some(want));
    let id = q.pool().find_source(h.sref).unwrap();
    assert_eq!(
        q.pool().source(id).unwrap().cost(),
        Cost::of(16 + l.held_len())
    );
}

/// An unsealed file that verifies (a write cut at its seal, or a host copy
/// that dropped the read-only attribute) is sealed as it is: Present,
/// never deleted and written again.
#[test]
fn a_whole_unsealed_source_is_sealed_not_rewritten() {
    let (h, data) = source(32, 1_000, "COPY");
    let mut s = MemStore::new(1);
    put_raw(&mut s, &h, &data);
    let vol = s.mount().unwrap();
    // Unsealed: a raw write goes through.
    s.write(vol, source_file(h.sref), &mut |w| w.put(b"x"))
        .unwrap();
    put_raw(&mut s, &h, &data);
    let r = Card::new()
        .run(&mut s, |s, r| write_source(s, r, &h, &data))
        .unwrap()
        .result;
    assert_eq!(r, Ok(Wrote::Present));
    let raw = s.write(vol, source_file(h.sref), &mut |w| w.put(b"x"));
    assert_eq!(raw, Err(StoreError::ReadOnly), "sealed now");
}

/// A `PoolSource` that finds no free source slot in pass 2 is lost from
/// the project, so the load is lossy and counts it. (Pass 2 drains the
/// old sources first, so a load proper never gets here.)
#[test]
fn a_ref_with_no_slot_is_lossy() {
    let (mut p, _) = Project::boxed();
    for crc in 0..32 {
        p.source_store(sref(0x100 + crc, 64), &[1; 40]).unwrap();
    }
    let mut d = ProjectDecoder::new_for_test(&mut p);
    let fx = Event::Record(ReadTag::Known(RecordTag::Fx), &[]);
    assert_eq!(d.apply(fx), Ok(()));
    let r = sref(0x999, 64).to_bytes();
    let e = Event::Record(ReadTag::Known(RecordTag::PoolSource), &r);
    assert_eq!(d.apply(e), Ok(()));
    assert!(d.lossy(), "a save would drop the ref");
}

/// Pass 1 plans only the refs `/CHIMERA/TAPES` lists: a Part's source
/// missing from the card takes no room, so no Sound gives way to it.
#[test]
fn a_missing_part_source_pushes_no_sound_out() {
    let (mut p, _) = full_dense_slots(48);
    let sounds = p.pool().sound_count();
    let left = ARENA_BYTES as u32 - p.pool().used().bytes();
    // Would take the room left and 3 KB more.
    let (gone, _) = source(33, 2 * (left + 3_000) / 3, "GONE");
    p.source_absent(gone.sref).unwrap();
    p.edit_part(PartId::ALL[0]).sound.source = Some(gone.sref.into());
    let mut s = MemStore::new(1);
    let (q, note) = round_trip(&mut s, &mut p);
    assert_eq!(
        note,
        Some(ProjectNote::SourcesMissing(1)),
        "nothing didn't fit"
    );
    assert_eq!(q.pool().sound_count(), sounds);
    let id = q.pool().find_source(gone.sref).expect("the ref kept");
    assert!(q.pool().source(id).unwrap().absent());
    assert!(q.meta().saved_crc().is_some());
}

/// A source file is exactly `source_file_min_len` long as `write_source`
/// writes it, whatever its last record holds.
#[test]
fn a_written_source_is_its_min_len() {
    for frames in [64, 336, 337, 1_001, 20_000] {
        let (h, data) = source(35, frames, "LEN");
        let mut s = MemStore::new(1);
        put_source(&mut s, &h, &data);
        let vol = s.mount().unwrap();
        let mut sizes = Vec::new();
        s.list(vol, Dir::Tapes, &mut |_, n| sizes.push(n)).unwrap();
        assert_eq!(sizes, [source_file_min_len(h.sref).unwrap()], "{frames}");
    }
}

/// A torn file listed in `/CHIMERA/TAPES` (shorter than a whole one) is
/// planned as missing: a Part's source cut short pushes no Sound out.
#[test]
fn a_torn_part_source_pushes_no_sound_out() {
    let (mut p, _) = full_dense_slots(48);
    let sounds = p.pool().sound_count();
    let left = ARENA_BYTES as u32 - p.pool().used().bytes();
    let (torn, torn_data) = source(36, 2 * (left + 3_000) / 3, "TORN");
    let mut s = MemStore::new(1);
    put_raw(&mut s, &torn, &torn_data);
    let vol = s.mount().unwrap();
    let mut whole = Bytes(Vec::new());
    s.read(vol, source_file(torn.sref), &mut whole).unwrap();
    let cut = whole.0.len() / 2;
    s.write(vol, source_file(torn.sref), &mut |w| w.put(&whole.0[..cut]))
        .unwrap();
    p.source_absent(torn.sref).unwrap();
    p.edit_part(PartId::ALL[0]).sound.source = Some(torn.sref.into());
    let (q, note) = round_trip(&mut s, &mut p);
    assert_eq!(note, Some(ProjectNote::SourcesMissing(1)));
    assert_eq!(q.pool().sound_count(), sounds, "every Sound");
    let id = q.pool().find_source(torn.sref).expect("the ref kept");
    assert!(q.pool().source(id).unwrap().absent());
}

/// A source whose cost is exactly the room the slots leave is taken.
#[test]
fn a_source_that_fits_exactly_loads() {
    let (mut p, _) = full_dense_slots(8);
    let left = ARENA_BYTES as u32 - p.pool().used().bytes();
    let pairs = (left - 16 - DESC_LEN as u32) / 3;
    let (h, data) = source(34, 2 * pairs, "EXACT");
    let cost = Cost::of(16 + DESC_LEN + data.len()).bytes();
    assert_eq!(cost, left, "fills the arena to the byte");
    let mut s = MemStore::new(1);
    put_source(&mut s, &h, &data);
    p.source_absent(h.sref).unwrap();
    let (q, note) = round_trip(&mut s, &mut p);
    let tok = &mut q.pool().table().reader().expect("one reader");
    assert_eq!(note, None);
    assert_eq!(arena_bytes(&q, tok, h.sref), Some(held(&h, &data)));
    assert_eq!(q.pool().free(), Cost::default());
}

/// A listed file of a whole file's length that fails its check (other
/// bytes) reads as missing, and the note counts it.
#[test]
fn a_damaged_source_is_named_missing() {
    let (mut p, _) = Project::boxed();
    let (h, data) = source(37, 1_000, "DAMAGED");
    let mut flipped = data.clone();
    flipped[10] ^= 1;
    let mut s = MemStore::new(1);
    put_raw(&mut s, &h, &flipped);
    p.source_absent(h.sref).unwrap();
    let (q, note) = round_trip(&mut s, &mut p);
    assert_eq!(note, Some(ProjectNote::SourcesMissing(1)));
    let id = q.pool().find_source(h.sref).expect("the ref kept");
    assert!(q.pool().source(id).unwrap().absent());
}

/// The plan costs a zone set at its listed size, layout included: in an
/// arena a zone-set Part source fills to the byte, a small source after
/// it plans as not fitting, and the load is the plan.
#[test]
fn a_zone_set_plans_at_its_listed_size() {
    use chimera_core::storage::{SourceLayout, SourceZones, Zone, write_descriptor, zone_set_crc};
    let (mut p, _) = full_dense_slots(8);
    let left = ARENA_BYTES as u32 - p.pool().used().bytes();
    // 24 zones, the layout 216 B: the frames that fill `left` with it.
    let room = left as usize - 16 - DESC_LEN - 24 * 9;
    let total = 2 * (room / 3) as u32;
    let zones: Vec<Zone> = (0..24u32)
        .map(|i| Zone {
            start: 64 * i,
            frames: if i == 23 { total - 64 * 23 } else { 64 },
            key: 24 + 4 * i as u8,
            root: 3_600,
            period_q16: 0,
        })
        .collect();
    let z = SourceZones::new(&zones).unwrap();
    let (plain, data) = source(39, total, "TRON");
    let h = SourceHead {
        shape: Shape::ZoneSet,
        count: 24,
        sref: SourceRef {
            crc: zone_set_crc(&z, &data),
            ..plain.sref
        },
        ..plain
    };
    let l = SourceLayout::zoned(h, z).unwrap();
    assert_eq!(Cost::of(16 + l.held_len()).bytes(), left, "fills the arena");
    // Fits only beside a zone set costed without its layout.
    let (small, small_data) = source(40, 64, "SMALL");
    let mut s = MemStore::new(1);
    let w = Card::new().run(&mut s, |s, r| {
        chimera_core::storage::write_zone_set(s, r, &h, &z, &data)
    });
    assert_eq!(w.unwrap().result, Ok(Wrote::New));
    put_source(&mut s, &small, &small_data);
    // The small source takes the first id, so it reads first: only the
    // plan keeps it from the zone set's room.
    p.source_absent(small.sref).unwrap();
    p.source_absent(h.sref).unwrap();
    p.edit_part(PartId::ALL[0]).sound.source = Some(h.sref.into());
    let (q, note) = round_trip(&mut s, &mut p);
    assert_eq!(
        note,
        Some(ProjectNote::DidntFit {
            sounds: 0,
            sources: 1
        })
    );
    let mut want = vec![0; l.held_len()];
    let at = write_descriptor(&l, &mut want);
    want[at..].copy_from_slice(&data);
    let tok = &mut q.pool().table().reader().expect("one reader");
    assert_eq!(
        arena_bytes(&q, tok, h.sref),
        Some(want),
        "the Part's zone set"
    );
    let id = q.pool().find_source(small.sref).expect("the ref kept");
    assert!(q.pool().source(id).unwrap().absent());
    assert_eq!(q.pool().free(), Cost::default());
}

/// `[Plain, 1]` and 96 zero bytes; zlib's CRC-32 agrees.
const PLAIN_ZEROS: u32 = 0x3930_09AA;
/// `[Transwave, 2]` and the bytes 0..96; zlib's CRC-32 agrees.
const TRANSWAVE_RAMP: u32 = 0x2617_6512;

/// The stems are an on-card contract: a `Plain` and a `Transwave` CRC,
/// pinned. A change to the prefix, its order, or the hash renames every
/// `.SRC` on every card.
#[test]
fn source_crcs_are_pinned() {
    let ramp: Vec<u8> = (0..96u8).collect();
    assert_eq!(source_crc(WholeShape::Plain, 1, &[0; 96]), PLAIN_ZEROS);
    assert_eq!(source_crc(WholeShape::Transwave, 2, &ramp), TRANSWAVE_RAMP);
}
