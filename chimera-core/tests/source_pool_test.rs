//! Sources in the pool (ADR 0087): their table, the gate a compaction
//! closes, retirement, purge, and pass 1's limit less the sources.

mod common;

use std::collections::BTreeMap;

use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::{full_dense_slots, save_part_to};
use chimera_core::project::{
    EncodedSound, LoadLink, PartFrom, PartId, PartSource, Project, ProjectNote, ProjectSource,
    Purged, ReplaceGuard, SlotId, SourceId, SourceRef, Subject, TemplateCrc, load_project,
    project_crc, project_file, source_users,
};
use chimera_core::source_table::{GateView, SourceTable};
use chimera_core::storage::{Card, FileError, ProjectId, Side};
use chimera_core::ui::browser;
use chimera_hal::store::{Dir, Store};
use chimera_hal::testkit::MemStore;
use common::codec_util::encode_project;

/// CHOIR: 27 900 frames at 24 kHz, 41 850 B held, 41K on the meter.
const CHOIR: SourceRef = SourceRef {
    crc: 0xC401_0001,
    frames: 27_900,
    rate: 24_000,
};

fn sref(crc: u32, frames: u32) -> SourceRef {
    SourceRef {
        crc,
        frames,
        rate: 24_000,
    }
}

/// `r`'s 12-bit pairs, a pattern of its own.
fn audio(r: SourceRef) -> Vec<u8> {
    let len = 3 * r.frames.div_ceil(2) as usize;
    (0..len)
        .map(|i| (i as u32).wrapping_mul(31).wrapping_add(r.crc) as u8)
        .collect()
}

fn store(p: &mut Project, r: SourceRef) -> SourceId {
    p.source_store(r, &audio(r)).expect("fits")
}

/// A Sound named `name` on source `r`, in the first free slot.
fn tape_sound(p: &mut Project, name: &str, r: SourceRef) -> SlotId {
    let s = p.pool().first_free().expect("a free slot");
    let mut x = Sound::init(EngineType::Algo);
    x.name = SoundName::new(name).unwrap();
    x.source = Some(r);
    p.pool_store(s, &x).unwrap();
    s
}

fn load(p: &mut Project, t: TemplateCrc, part: PartId, s: SlotId) {
    let src = PartSource {
        part,
        from: PartFrom::Slot(s),
    };
    let c = ReplaceGuard::check(p, t, src).expect("a Clean Part");
    p.replace_part(c).unwrap();
}

fn pending(p: &Project) -> Cost {
    p.pool().split().2
}

/// UI passes with an audio block after each, `sounding` reported: until
/// nothing is pending, at most `n` rounds.
fn settle(p: &mut Project, sounding: u32, n: usize) {
    let table = p.pool().table();
    for _ in 0..n {
        p.sources_pass();
        let _ = table.begin_block();
        table.store_sounding(sounding);
    }
}

#[test]
fn source_ref_round_trips() {
    let r = sref(0xDEAD_BEEF, 123_457);
    let b = r.to_bytes();
    assert_eq!(b.len(), 10);
    assert_eq!(SourceRef::from_bytes(&b), Some(r));
    assert_eq!(SourceRef::from_bytes(&b[..9]), None);
    assert_eq!(SourceRef::from_bytes(&[0; 11]), None);

    // Through the pool and through a Sound file.
    let (mut p, _) = Project::boxed();
    let s = tape_sound(&mut p, "CHOIR", r);
    let got = p.pool().get(s).unwrap();
    assert_eq!(got.source_ref(), Some(r));
    let mut x = Sound::init(EngineType::Modal);
    x.source = Some(r);
    assert!(got.to_sound().source == Some(r));
    let back = common::codec_util::decode(&common::codec_util::encode(&x)).unwrap();
    assert!(back.bits_eq(&x));
    assert!(
        !back.bits_eq(&Sound::init(EngineType::Modal)),
        "the ref counts"
    );

    // A 9-byte payload is Bounds.
    let mut bytes = SoundName::new("BAD").unwrap().padded().to_vec();
    bytes.extend_from_slice(&[0x02, 0x80, 1, 0, 0]);
    bytes.extend_from_slice(&[0x16, 0x00, 9, 0]);
    bytes.extend_from_slice(&[0; 9]);
    let mut target = Sound::init(EngineType::Algo);
    assert_eq!(
        EncodedSound::new(&bytes).decode_into(&mut target),
        Err(FileError::Bounds)
    );
}

#[test]
fn store_sounding_is_read() {
    let t = SourceTable::new();
    assert_eq!(t.sounding(), 0);
    for m in [1, 0x8000_0001, u32::MAX, 0] {
        t.store_sounding(m);
        assert_eq!(t.sounding(), m);
    }
}

#[test]
fn gate_view_bytes_none_while_moving() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(1, 1000), sref(2, 2000));
    let (ia, ib) = (store(&mut p, a), store(&mut p, b));
    let table = p.pool().table();
    let open = table.begin_block();
    assert_eq!(open.bytes(ia), Some(&audio(a)[..]));
    assert_eq!(open.bytes(ib), Some(&audio(b)[..]));
    let e = table.close_gate(ia.bit());
    assert!(!table.gate_acked(e));
    let v = table.begin_block();
    assert!(table.gate_acked(e));
    assert_eq!(v.bytes(ia), None);
    assert_eq!(v.bytes(ib), Some(&audio(b)[..]));
    table.open_gate();
    assert_eq!(table.begin_block().bytes(ia), Some(&audio(a)[..]));
    // An empty row reads nothing.
    assert_eq!(table.begin_block().bytes(SourceId::ALL[31]), None);
}

#[test]
fn choir_orphan() {
    let (mut p, t) = Project::boxed();
    let choir = store(&mut p, CHOIR);
    let cost = p.pool().source(choir).unwrap().cost();
    assert_eq!(cost.kib_label().as_str(), "41K");
    let slot = tape_sound(&mut p, "CHOIR", CHOIR);
    let p2 = PartId::ALL[1];
    load(&mut p, t, p2, slot);
    assert_eq!(p.part(p2).sound.source, Some(CHOIR));
    load(&mut p, t, p2, SlotId::ALL[0]);

    // Before the clear, the slot uses it: PURGE frees nothing.
    assert_eq!(p.purge_unused_sources(0), Purged::default());
    assert_eq!(p.orphaned_by_clear(slot), Some(cost));
    let name = p.pool().get(slot).unwrap().name();
    let words = browser::orphan_words(name, cost);
    assert_eq!(words.as_str(), "CHOIR 41K UNUSED");

    let used = p.pool().used();
    p.pool_clear(slot).unwrap();
    assert!(source_users(&p, choir).is_empty());
    assert!(p.pool().used() < used, "the Sound's bytes");
    let gap = p.pool().free();
    assert_eq!(
        p.purge_unused_sources(0),
        Purged {
            freed: cost,
            retired: 0
        }
    );
    settle(&mut p, 0, 4);
    assert_eq!(
        p.pool().free(),
        Cost::of((gap.bytes() + cost.bytes()) as usize)
    );
    assert_eq!(pending(&p), Cost::default());

    // Part 2 still on CHOIR: the clear frees the Sound only.
    let (mut p, t) = Project::boxed();
    let choir = store(&mut p, CHOIR);
    let slot = tape_sound(&mut p, "CHOIR", CHOIR);
    load(&mut p, t, p2, slot);
    let other = p.pool().first_free().unwrap();
    save_part_to(&mut p, p2, other);
    assert_eq!(p.orphaned_by_clear(slot), None);
    let sources = p.pool().source_used();
    p.pool_clear(slot).unwrap();
    assert_eq!(p.purge_unused_sources(0), Purged::default());
    assert_eq!(p.pool().source_used(), sources);
    let u = source_users(&p, choir);
    assert!(u.parts.contains(p2) && u.has_sound(other));
}

/// 500 seeded sequences of stores, clears, Part loads and purges: each
/// purge frees exactly what a model says nothing references then.
#[test]
fn purge_recounts() {
    let (mut p, t) = Project::boxed();
    let refs: Vec<SourceRef> = (0..6).map(|i| sref(0x5000 + i, 400 + 300 * i)).collect();
    let cost = |r: &SourceRef| Cost::of(16 + audio(*r).len());
    let slots: Vec<SlotId> = SlotId::ALL[60..72].to_vec();
    // The model: the refs each written slot and each Part holds, and the
    // sources the pool holds.
    let mut in_slot: BTreeMap<SlotId, Option<SourceRef>> = BTreeMap::new();
    let mut in_part: [Option<SourceRef>; 6] = [None; 6];
    let mut held: Vec<SourceRef> = Vec::new();
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut rnd = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for _ in 0..500 {
        for _ in 0..1 + rnd(10) {
            match rnd(5) {
                0 => {
                    let r = refs[rnd(6) as usize];
                    if !held.contains(&r) {
                        store(&mut p, r);
                        held.push(r);
                    }
                }
                1 => {
                    let s = slots[rnd(slots.len() as u64) as usize];
                    if p.users(s).is_empty() {
                        let r = (rnd(4) > 0).then(|| refs[rnd(6) as usize]);
                        let mut x = Sound::init(EngineType::Algo);
                        x.source = r;
                        p.pool_store(s, &x).unwrap();
                        in_slot.insert(s, r);
                    }
                }
                2 => {
                    let s = slots[rnd(slots.len() as u64) as usize];
                    if p.pool_clear(s).is_ok() {
                        in_slot.remove(&s);
                    }
                }
                3 => {
                    let s = slots[rnd(slots.len() as u64) as usize];
                    let part = PartId::ALL[rnd(6) as usize];
                    let src = PartSource {
                        part,
                        from: PartFrom::Slot(s),
                    };
                    if p.pool().get(s).is_some()
                        && let Ok(c) = ReplaceGuard::check(&p, t, src)
                    {
                        p.replace_part(c).unwrap();
                        in_part[part.index()] = in_slot[&s];
                    }
                }
                _ => settle(&mut p, 0, 2),
            }
        }
        let used: Vec<SourceRef> = in_slot
            .values()
            .chain(in_part.iter())
            .flatten()
            .copied()
            .collect();
        let want = held
            .iter()
            .filter(|r| !used.contains(r))
            .fold(Cost::default(), |a, r| a + cost(r));
        assert_eq!(p.unused_sources().cost, want);
        assert_eq!(p.purge_unused_sources(0).freed, want);
        held.retain(|r| used.contains(r));
        for r in &held {
            assert!(p.pool().find_source(*r).is_some());
        }
        settle(&mut p, 0, 2);
        assert_eq!(pending(&p), Cost::default());
    }
}

/// Review Focus 3: a purged source still sounding comes back by itself
/// when its note ends.
#[test]
fn pending_hole_resolves_when_note_ends() {
    let (mut p, _) = Project::boxed();
    let r = sref(7, 9000);
    let id = store(&mut p, r);
    let cost = p.pool().source(id).unwrap().cost();
    let table = p.pool().table();
    table.store_sounding(id.bit());
    let gap = p.pool().free();
    let purged = p.purge_unused_sources(table.sounding());
    assert_eq!(
        purged,
        Purged {
            freed: cost,
            retired: 1
        }
    );
    settle(&mut p, id.bit(), 8);
    assert_eq!(pending(&p), cost, "on the meter while it sounds");
    assert_eq!(p.pool().free(), gap);
    assert_eq!(
        table.begin_block().bytes(id),
        Some(&audio(r)[..]),
        "never cut"
    );

    table.store_sounding(0);
    settle(&mut p, 0, 4);
    assert_eq!(pending(&p), Cost::default());
    assert_eq!(
        p.pool().free(),
        Cost::of((gap.bytes() + cost.bytes()) as usize)
    );
    assert_eq!(p.unused_sources().count, 0, "no second purge");
    assert_eq!(table.begin_block().bytes(id), None);
}

#[test]
fn retire_never_cuts() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(10, 3000), sref(11, 5000));
    let ia = store(&mut p, a);
    let ib = store(&mut p, b);
    tape_sound(&mut p, "KEEP B", b);
    let table = p.pool().table();
    table.store_sounding(ia.bit());
    assert_eq!(p.purge_unused_sources(ia.bit()).retired, 1);
    assert!(p.pool().source(ia).is_none(), "out of the index at once");
    for _ in 0..16 {
        settle(&mut p, ia.bit(), 1);
        let v = table.begin_block();
        assert_eq!(v.bytes(ia), Some(&audio(a)[..]));
        assert_eq!(v.bytes(ib), Some(&audio(b)[..]));
    }
    settle(&mut p, 0, 4);
    let v = table.begin_block();
    assert_eq!(v.bytes(ia), None);
    assert_eq!(v.bytes(ib), Some(&audio(b)[..]), "moved, unchanged");
}

/// A stub TAPE voice: latched to a source at note-on, it plays its bytes
/// a block at a time through the view, and waits while the gate moves it.
struct Voice {
    id: SourceId,
    pos: usize,
    len: usize,
    out: Vec<u8>,
    /// The blocks it played in.
    played: Vec<usize>,
}

const BLOCK: usize = 64;

impl Voice {
    fn on(id: SourceId, len: usize) -> Voice {
        Voice {
            id,
            pos: 0,
            len,
            out: Vec::new(),
            played: Vec::new(),
        }
    }

    fn block(&mut self, v: &GateView<'_>, n: usize) {
        if self.pos == self.len {
            return;
        }
        if let Some(b) = v.bytes(self.id) {
            let end = (self.pos + BLOCK).min(self.len);
            self.out.extend_from_slice(&b[self.pos..end]);
            self.pos = end;
            self.played.push(n);
        }
    }

    /// Started and not ended: its bit is in `sounding`.
    fn sounding(&self) -> u32 {
        if self.pos > 0 && self.pos < self.len {
            self.id.bit()
        } else {
            0
        }
    }
}

/// Retires the lowest source: the hole below `above`.
fn hole_below(p: &mut Project, low: SourceRef, above: &[SourceRef]) {
    store(p, low);
    for &r in above {
        store(p, r);
        tape_sound(p, "KEEP", r);
    }
    assert_eq!(p.purge_unused_sources(0).retired, 0);
    assert!(pending(p) > Cost::default());
}

#[test]
fn gate_recheck_catches_late_note_on() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(20, 2000), sref(21, 4000));
    hole_below(&mut p, a, &[b]);
    let ib = p.pool().find_source(b).unwrap();
    let table = p.pool().table();
    let (gap, held) = (p.pool().free(), pending(&p));

    // The block before the close began with the gate open.
    let open = table.begin_block();
    assert!(!p.sources_pass(), "closes the gate");
    let mut voice = Voice::on(ib, audio(b).len());
    voice.block(&open, 0);
    table.store_sounding(voice.sounding());

    // The ack, then the re-check sees the late note: open, nothing moved.
    let v = table.begin_block();
    voice.block(&v, 1);
    assert_eq!(v.bytes(ib), None, "the gate holds it this block");
    table.store_sounding(voice.sounding());
    assert!(!p.sources_pass());
    assert_eq!((p.pool().free(), pending(&p)), (gap, held));
    assert!(table.begin_block().bytes(ib).is_some(), "reopened");

    // It plays on; the hole waits for its end, then closes.
    let mut n = 2;
    while voice.sounding() != 0 {
        let v = table.begin_block();
        voice.block(&v, n);
        table.store_sounding(voice.sounding());
        n += 1;
        p.sources_pass();
        assert_eq!(pending(&p), held, "nothing moves under the voice");
    }
    settle(&mut p, 0, 4);
    assert_eq!(pending(&p), Cost::default());
    assert_eq!(voice.out, audio(b), "the unmoved source, throughout");
    assert_eq!(table.begin_block().bytes(ib), Some(&audio(b)[..]));
}

#[test]
fn gate_holds_note_on() {
    let (mut p, _) = Project::boxed();
    let (c, a, b) = (sref(30, 1500), sref(31, 2000), sref(32, 3000));
    let ic = store(&mut p, c);
    tape_sound(&mut p, "KEEP C", c);
    hole_below(&mut p, a, &[b]);
    let ib = p.pool().find_source(b).unwrap();
    let table = p.pool().table();

    assert!(!p.sources_pass(), "closes the gate");
    let v = table.begin_block();
    let mut moving = Voice::on(ib, audio(b).len());
    let mut other = Voice::on(ic, audio(c).len());
    moving.block(&v, 0);
    other.block(&v, 0);
    table.store_sounding(moving.sounding() | other.sounding());
    assert_eq!(other.played, [0], "another source starts at once");
    assert!(moving.played.is_empty(), "held for the block");

    assert!(p.sources_pass(), "moves, then opens");
    let v = table.begin_block();
    moving.block(&v, 1);
    assert_eq!(moving.played, [1], "the first block after open");
    assert_eq!(moving.out, audio(b)[..BLOCK], "first sample unchanged");
}

/// `n`'s file on both sides of the card.
fn put_project(s: &mut MemStore, n: u32, bytes: &[u8]) {
    let v = s.mount().unwrap();
    s.make_dir(v, Dir::Chimera).unwrap();
    s.make_dir(v, Dir::Projects).unwrap();
    let id = ProjectId::new(n).unwrap();
    for side in [Side::A, Side::B] {
        s.write(v, project_file(id).side(side), &mut |w| w.put(bytes))
            .unwrap();
    }
}

fn load_file(s: &mut MemStore, q: &mut Project, t: TemplateCrc) -> (Option<ProjectNote>, bool) {
    let mut card = Card::new();
    let link = LoadLink::new();
    let vol = s.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let go = ReplaceGuard::check(q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let out = load_project(&mut card, s, q, go, &link);
    (out.note, out.swap.is_some())
}

/// Pass 1's limit is the arena less the sources: a project that only fits
/// the whole arena is refused before anything is written.
#[test]
fn pass_one_counts_the_sources() {
    let (src, _) = full_dense_slots(48);
    let slots = src.pool().used().bytes() as usize;
    let file = encode_project(&src);
    let room = ARENA_BYTES - slots;
    for (extra, fits) in [(16, false), (0, true)] {
        let mut s = MemStore::new(1);
        put_project(&mut s, 1, &file);
        let (mut q, t) = Project::boxed();
        // Sources holding S: the slots cost ARENA_BYTES − S + extra.
        q.source_store(sref(99, 0), &vec![7; room + extra - 16])
            .unwrap();
        assert_eq!(q.pool().source_used().bytes() as usize, room + extra);
        q.mark_saved_for_test();
        let before = project_crc(&q);
        let (note, swapped) = load_file(&mut s, &mut q, t);
        if fits {
            assert_eq!(note, None, "loaded, no note");
            assert!(swapped);
            assert_eq!(q.pool().free(), Cost::default());
            assert_eq!(q.meta().name(), src.meta().name());
        } else {
            assert_eq!(
                note,
                Some(ProjectNote::File {
                    err: FileError::Bounds,
                    subject: Subject::File(ProjectId::new(1).unwrap()),
                })
            );
            assert!(!swapped, "no NEW fallback");
            assert_eq!(project_crc(&q), before, "nothing written");
            assert_eq!(q.meta().saved_crc(), Some(before));
        }
    }
}
