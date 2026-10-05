//! The pool arena (ADR 0087): Sounds down from the top, sources up from 0,
//! one gap; no source byte moves under a sounding voice.

use chimera_core::project::arena::{
    ARENA_BYTES, ArenaWriter, Cost, GRANULE, GateAcked, Hole, MAX_SOURCES, Refused, Shift,
};
use chimera_core::source_table::SourceTable;
use chimera_core::storage::{Crc32, MAX_SOUND_COST};

fn arena() -> ArenaWriter {
    ArenaWriter::new(
        Box::leak(vec![0; ARENA_BYTES].into_boxed_slice()),
        Box::leak(Box::new(SourceTable::new())),
    )
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn crc(b: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(b);
    c.finish()
}

/// Fills `len` bytes at `offset` with a pattern unique to `tag`; its CRC.
fn stamp(a: &mut ArenaWriter, offset: u32, len: usize, tag: u64) -> u32 {
    let mut r = Rng(tag | 1);
    for b in a.bytes_mut(offset, len) {
        *b = r.next() as u8;
    }
    crc(a.bytes(offset, len))
}

#[test]
fn cost_rounds_to_granule() {
    assert_eq!(Cost::of(0).bytes(), 0);
    assert_eq!(Cost::of(1).bytes(), 16);
    assert_eq!(Cost::of(16).bytes(), 16);
    assert_eq!(Cost::of(17).bytes(), 32);
}

#[test]
fn kib_labels() {
    assert_eq!(Cost::of(300).kib_label().as_str(), "0.3K");
    assert_eq!(Cost::of(10_240).kib_label().as_str(), "10K");
    assert_eq!(Cost::of(41_000).kib_label().as_str(), "41K");
    // Rounded up: a byte over a tenth reads the next tenth.
    assert_eq!(Cost::of(1024).kib_label().as_str(), "1.0K");
    assert_eq!(Cost::of(1025).kib_label().as_str(), "1.1K");
    assert_eq!(Cost::of(10_224).kib_label().as_str(), "10K");
    assert_eq!(Cost::of(10_100).kib_label().as_str(), "9.9K");
    assert_eq!(Cost::of(ARENA_BYTES).kib_label().as_str(), "160K");
}

#[test]
fn new_arena_is_all_gap() {
    let a = arena();
    assert_eq!(a.used().bytes(), 0);
    assert_eq!(a.pending().bytes(), 0);
    assert_eq!(a.gap().bytes() as usize, ARENA_BYTES);
}

/// A source staged at the gap's bottom with a pattern unique to `tag`, then
/// taken: its offset and CRC.
fn add_source(a: &mut ArenaWriter, len: usize, tag: u64) -> Result<(u32, u32), Refused> {
    let mut r = Rng(tag | 1);
    for b in a.source_stage(len)? {
        *b = r.next() as u8;
    }
    let o = a.source_alloc(len)?;
    Ok((o, crc(a.bytes(o, len))))
}

fn ready_unless(sounding: &[u32]) -> GateAcked<impl Fn(u32) -> bool + '_> {
    GateAcked::assumed(move |o| !sounding.contains(&o))
}

fn all_ready() -> GateAcked<fn(u32) -> bool> {
    GateAcked::assumed(|_| true)
}

#[test]
fn ends_never_meet() {
    let mut a = arena();
    let mut snds = Vec::new();
    let mut srcs = Vec::new();
    let mut rng = Rng(0x5eed);
    loop {
        let len = 1 + rng.below(MAX_SOUND_COST);
        match a.sound_alloc(len) {
            Some(o) => snds.push((o, Cost::of(len).bytes())),
            None => {
                assert!(a.gap().bytes() < Cost::of(len).bytes());
                break;
            }
        }
        if srcs.len() < MAX_SOURCES {
            let len = 1 + rng.below(4096);
            match a.source_alloc(len) {
                Ok(o) => srcs.push((o, Cost::of(len).bytes())),
                Err(e) => {
                    assert_eq!(e, Refused::Full);
                    assert!(a.gap().bytes() < Cost::of(len).bytes());
                    break;
                }
            }
        }
    }
    let mut all: Vec<(u32, u32)> = snds.iter().chain(&srcs).copied().collect();
    all.sort();
    for w in all.windows(2) {
        assert!(w[0].0 + w[0].1 <= w[1].0, "overlap {:?}", w);
    }
    let top = snds.iter().map(|s| s.0 + s.1).max().unwrap();
    assert_eq!(top as usize, ARENA_BYTES);
    assert_eq!(a.used().bytes() + a.gap().bytes(), ARENA_BYTES as u32);
}

#[test]
fn rows_busy_is_not_full() {
    let mut a = arena();
    let mut first = 0;
    for i in 0..MAX_SOURCES {
        let (o, _) = add_source(&mut a, 64, i as u64).unwrap();
        if i == 0 {
            first = o;
        }
    }
    // A retired source still ringing holds its row: the gap is wide, but
    // no source gets a place until the note ends.
    a.source_free_pending(first, 64);
    assert!(a.gap() > Cost::of(64));
    assert_eq!(a.source_alloc(64), Err(Refused::RowsBusy));
    assert_eq!(a.source_stage(64).err(), Some(Refused::RowsBusy));
    assert_eq!(a.source_compact(ready_unless(&[first])), None);
    assert!(a.source_compact(all_ready()).is_some());
    assert!(add_source(&mut a, 64, 99).is_ok());
    // Full still wins when the bytes aren't there either.
    assert_eq!(a.source_alloc(ARENA_BYTES), Err(Refused::Full));
}

#[test]
fn published_sources_take_no_writes() {
    let mut a = arena();
    let (o, _) = add_source(&mut a, 64, 1).unwrap();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.bytes_mut(o, 1)[0] = 0;
    }));
    assert!(r.is_err());
}

#[test]
fn sound_free_compacts_at_once() {
    let mut a = arena();
    let x = a.sound_alloc(100).unwrap();
    let y = a.sound_alloc(40).unwrap();
    let z = a.sound_alloc(20).unwrap();
    assert!(z < y && y < x);
    let cz = stamp(&mut a, z, 20, 3);
    let gap = a.gap().bytes();
    let s = a.sound_free(y, 40);
    assert_eq!(s, Shift { from: y, by: 48 });
    assert_eq!(a.gap().bytes(), gap + 48);
    assert_eq!(crc(a.bytes(z + 48, 20)), cz);
}

/// A load's refill: every Sound goes, every source stays where it was.
#[test]
fn sounds_clear_keeps_the_sources() {
    let mut a = arena();
    let (at, c) = add_source(&mut a, 300, 9).unwrap();
    a.sound_alloc(100).unwrap();
    a.sound_alloc(MAX_SOUND_COST).unwrap();
    a.sounds_clear();
    assert_eq!(a.used(), Cost::of(300));
    assert_eq!(a.gap().bytes() as usize, ARENA_BYTES - 304);
    assert_eq!(crc(a.bytes(at, 300)), c);
    assert_eq!(a.sound_alloc(16), Some(ARENA_BYTES as u32 - 16));
}

#[test]
fn compact_waits_while_not_ready() {
    let mut a = arena();
    let (hole, _) = add_source(&mut a, 1000, 1).unwrap();
    let (above, c) = add_source(&mut a, 500, 9).unwrap();
    a.source_free_pending(hole, 1000);
    let gap = a.gap().bytes();
    assert_eq!(a.pending(), Cost::of(1000));
    assert_eq!(a.source_compact(ready_unless(&[above])), None);
    assert_eq!(a.gap().bytes(), gap);
    assert_eq!(a.pending(), Cost::of(1000));
    assert_eq!(crc(a.bytes(above, 500)), c);
    // The hole itself may still be sounding (a retired source's last note).
    assert_eq!(a.source_compact(ready_unless(&[hole])), None);
    assert_eq!(a.gap().bytes(), gap);
    let s = a.source_compact(all_ready()).unwrap();
    assert_eq!(
        s,
        Shift {
            from: hole,
            by: Cost::of(1000).bytes()
        }
    );
    assert_eq!(a.pending().bytes(), 0);
    assert_eq!(a.gap().bytes(), gap + Cost::of(1000).bytes());
    assert_eq!(crc(a.bytes(hole, 500)), c);
    assert_eq!(a.source_compact(all_ready()), None);
}

#[test]
fn compaction_takes_the_lowest_hole() {
    let mut a = arena();
    let (h0, _) = add_source(&mut a, 64, 1).unwrap();
    let (mid, _) = add_source(&mut a, 64, 2).unwrap();
    let (h1, _) = add_source(&mut a, 64, 3).unwrap();
    let (top, c) = add_source(&mut a, 64, 4).unwrap();
    assert_eq!(a.next_compaction(), None);
    a.source_free_pending(h1, 64);
    a.source_free_pending(h0, 64);
    let lowest = Hole {
        offset: h0,
        cost: Cost::of(64),
    };
    assert_eq!(a.next_compaction(), Some(lowest));
    // A voice on a source above the lowest hole holds every hole: the
    // higher one waits its turn rather than jumping the `above` mask.
    assert_eq!(a.source_compact(ready_unless(&[mid])), None);
    assert_eq!(a.next_compaction(), Some(lowest));
    assert_eq!(
        a.source_compact(all_ready()),
        Some(Shift { from: h0, by: 64 })
    );
    // h1 slid down with everything else above h0.
    let next = Hole {
        offset: h1 - 64,
        cost: Cost::of(64),
    };
    assert_eq!(a.next_compaction(), Some(next));
    assert_eq!(
        a.source_compact(all_ready()),
        Some(Shift {
            from: next.offset,
            by: 64
        })
    );
    assert_eq!(crc(a.bytes(top - 128, 64)), c);
    assert_eq!(a.next_compaction(), None);
    assert_eq!(a.pending().bytes(), 0);
}

#[derive(Clone, Debug)]
struct Entry {
    offset: u32,
    len: usize,
    crc: u32,
    pending: bool,
}

fn check(a: &ArenaWriter, snds: &[Entry], srcs: &[Entry]) {
    let total = ARENA_BYTES as u32;
    assert_eq!(a.used().bytes() + a.gap().bytes(), total);
    let pending: u32 = srcs
        .iter()
        .filter(|e| e.pending)
        .map(|e| Cost::of(e.len).bytes())
        .sum();
    assert_eq!(a.pending().bytes(), pending);
    // Sources contiguous from 0 (pending ones are the holes).
    let mut at = 0;
    for e in srcs {
        assert_eq!(e.offset, at);
        assert_eq!(e.offset as usize % GRANULE, 0);
        at += Cost::of(e.len).bytes();
    }
    let src_end = at;
    let lowest = srcs.iter().find(|e| e.pending);
    assert_eq!(
        a.next_compaction(),
        lowest.map(|e| Hole {
            offset: e.offset,
            cost: Cost::of(e.len)
        })
    );
    // Sounds contiguous to the top.
    let mut sorted: Vec<&Entry> = snds.iter().collect();
    sorted.sort_by_key(|e| e.offset);
    let mut at = total - snds.iter().map(|e| Cost::of(e.len).bytes()).sum::<u32>();
    assert!(src_end <= at, "the ends met");
    assert_eq!(a.gap().bytes(), at - src_end);
    for e in sorted {
        assert_eq!(e.offset, at);
        assert_eq!(e.offset as usize % GRANULE, 0);
        at += Cost::of(e.len).bytes();
    }
    assert_eq!(at, total);
    for e in snds.iter().chain(srcs) {
        assert_eq!(crc(a.bytes(e.offset, e.len)), e.crc, "{e:?} changed");
    }
}

#[test]
fn model_10k_ops() {
    let mut a = arena();
    let mut rng = Rng(0xC41_3E5A);
    let mut snds: Vec<Entry> = Vec::new();
    let mut srcs: Vec<Entry> = Vec::new();
    let mut tag = 1u64;
    let (mut full, mut busy, mut held, mut moved) = (0, 0, 0, 0);
    for _ in 0..10_000 {
        tag += 1;
        match rng.below(6) {
            0 | 1 => {
                let len = 1 + rng.below(MAX_SOUND_COST);
                match a.sound_alloc(len) {
                    Some(offset) => {
                        let crc = stamp(&mut a, offset, len, tag);
                        snds.push(Entry {
                            offset,
                            len,
                            crc,
                            pending: false,
                        });
                    }
                    None => {
                        assert!(a.gap() < Cost::of(len));
                        full += 1;
                    }
                }
            }
            2 if !snds.is_empty() => {
                let e = snds.swap_remove(rng.below(snds.len()));
                let s = a.sound_free(e.offset, e.len);
                for o in &mut snds {
                    if o.offset < s.from {
                        o.offset += s.by;
                    }
                }
            }
            3 => {
                let len = 1 + rng.below(12_000);
                match add_source(&mut a, len, tag) {
                    Ok((offset, crc)) => srcs.push(Entry {
                        offset,
                        len,
                        crc,
                        pending: false,
                    }),
                    Err(Refused::Full) => {
                        assert!(a.gap() < Cost::of(len));
                        full += 1;
                    }
                    Err(Refused::RowsBusy) => {
                        assert_eq!(srcs.len(), MAX_SOURCES);
                        busy += 1;
                    }
                }
            }
            4 => {
                let live: Vec<usize> = (0..srcs.len()).filter(|&i| !srcs[i].pending).collect();
                if !live.is_empty() {
                    let i = live[rng.below(live.len())];
                    srcs[i].pending = true;
                    a.source_free_pending(srcs[i].offset, srcs[i].len);
                }
            }
            _ => {
                // A random set of sources is sounding.
                let sounding: Vec<u32> = srcs
                    .iter()
                    .filter(|_| rng.below(4) == 0)
                    .map(|e| e.offset)
                    .collect();
                let (gap, pending, next) = (a.gap(), a.pending(), a.next_compaction());
                match a.source_compact(ready_unless(&sounding)) {
                    None => {
                        held += next.is_some() as u32;
                        // A refusal touches nothing in the arena.
                        assert_eq!(
                            (a.gap(), a.pending(), a.next_compaction()),
                            (gap, pending, next)
                        );
                    }
                    Some(s) => {
                        moved += 1;
                        let i = srcs.iter().position(|e| e.offset == s.from).unwrap();
                        assert_eq!(
                            next.map(|h| (h.offset, h.cost.bytes())),
                            Some((s.from, s.by))
                        );
                        assert_eq!(s.by, Cost::of(srcs[i].len).bytes());
                        assert_eq!(a.gap().bytes(), gap.bytes() + s.by);
                        assert_eq!(a.pending().bytes(), pending.bytes() - s.by);
                        // No voice was on the hole or anything that moved.
                        for e in &srcs[i..] {
                            assert!(!sounding.contains(&e.offset), "moved under a voice");
                        }
                        srcs.remove(i);
                        for e in &mut srcs[i..] {
                            e.offset -= s.by;
                        }
                    }
                }
            }
        }
        // `check` compares the arena with the model: offsets, next hole,
        // gap, pending and every entry's CRC.
        check(&a, &snds, &srcs);
    }
    // The run reached every branch.
    assert!(
        full > 0 && busy > 0 && held > 0 && moved > 0,
        "{full} {busy} {held} {moved}"
    );
}

/// Why the gate must re-check after its ack (ADR 0087 step 3): compaction
/// trusts `GateAcked`'s answer as of the call. An answer read before a late
/// note-on moves that voice's source; the fresh one moves nothing. The
/// race itself, with the audio thread, is Task 8's test.
#[test]
fn compaction_trusts_the_answer_it_is_given() {
    let setup = || {
        let mut a = arena();
        let (hole, _) = add_source(&mut a, 256, 1).unwrap();
        let (b, cb) = add_source(&mut a, 256, 7).unwrap();
        a.source_free_pending(hole, 256);
        (a, hole, b, cb)
    };

    // Stale: read before the note-on on B, so B moves, and its old bytes
    // are gap the next add overwrites.
    let (mut a, _, b, cb) = setup();
    let stale: Vec<u32> = vec![];
    assert!(a.source_compact(ready_unless(&stale)).is_some());
    assert_eq!(add_source(&mut a, 256, 8).unwrap().0, b);
    assert_ne!(crc(a.bytes(b, 256)), cb);

    // Fresh: B is sounding, nothing moves until its note ends.
    let (mut a, hole, b, cb) = setup();
    for _ in 0..100 {
        assert_eq!(a.source_compact(ready_unless(&[b])), None);
        assert_eq!(crc(a.bytes(b, 256)), cb);
    }
    assert_eq!(
        a.source_compact(all_ready()),
        Some(Shift {
            from: hole,
            by: 256
        })
    );
    assert_eq!(crc(a.bytes(hole, 256)), cb);
}
