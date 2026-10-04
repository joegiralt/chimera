//! The pool arena (ADR 0087): Sounds down from the top, sources up from 0,
//! one gap; no source byte moves under a sounding voice.

use chimera_core::project::arena::{ARENA_BYTES, ArenaWriter, Cost, GRANULE, MAX_SOURCES, Shift};
use chimera_core::storage::{Crc32, MAX_SOUND_COST};

fn arena() -> ArenaWriter {
    ArenaWriter::new(Box::leak(vec![0; ARENA_BYTES].into_boxed_slice()))
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
                Some(o) => srcs.push((o, Cost::of(len).bytes())),
                None => {
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
fn source_count_is_capped() {
    let mut a = arena();
    for _ in 0..MAX_SOURCES {
        assert!(a.source_alloc(1).is_some());
    }
    assert_eq!(a.source_alloc(1), None);
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

#[test]
fn compact_waits_while_not_ready() {
    let mut a = arena();
    let hole = a.source_alloc(1000).unwrap();
    let above = a.source_alloc(500).unwrap();
    let c = stamp(&mut a, above, 500, 9);
    a.source_free_pending(hole, 1000);
    let gap = a.gap().bytes();
    assert_eq!(a.pending(), Cost::of(1000));
    assert_eq!(a.source_compact(|o| o != above), None);
    assert_eq!(a.gap().bytes(), gap);
    assert_eq!(a.pending(), Cost::of(1000));
    assert_eq!(crc(a.bytes(above, 500)), c);
    // The hole itself may still be sounding (a retired source's last note).
    assert_eq!(a.source_compact(|o| o != hole), None);
    assert_eq!(a.gap().bytes(), gap);
    let s = a.source_compact(|_| true).unwrap();
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
    assert_eq!(a.source_compact(|_| true), None);
}

#[test]
fn a_higher_hole_compacts_past_a_held_lower_one() {
    let mut a = arena();
    let h0 = a.source_alloc(64).unwrap();
    let held = a.source_alloc(64).unwrap();
    let h1 = a.source_alloc(64).unwrap();
    let top = a.source_alloc(64).unwrap();
    let c = stamp(&mut a, top, 64, 4);
    a.source_free_pending(h0, 64);
    a.source_free_pending(h1, 64);
    let s = a.source_compact(|o| o != held).unwrap();
    assert_eq!(s, Shift { from: h1, by: 64 });
    assert_eq!(crc(a.bytes(h1, 64)), c);
    assert_eq!(a.pending().bytes(), 64);
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
    let (mut refused, mut held, mut moved) = (0, 0, 0);
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
                        refused += 1;
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
                match a.source_alloc(len) {
                    Some(offset) => {
                        let crc = stamp(&mut a, offset, len, tag);
                        srcs.push(Entry {
                            offset,
                            len,
                            crc,
                            pending: false,
                        });
                    }
                    None => {
                        assert!(a.gap() < Cost::of(len) || srcs.len() == MAX_SOURCES);
                        refused += 1;
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
                let before: Vec<Entry> = srcs.clone();
                match a.source_compact(|o| !sounding.contains(&o)) {
                    None => {
                        held += srcs.iter().any(|e| e.pending) as u32;
                        for (x, y) in before.iter().zip(&srcs) {
                            assert_eq!(x.offset, y.offset);
                        }
                    }
                    Some(s) => {
                        moved += 1;
                        let i = srcs.iter().position(|e| e.offset == s.from).unwrap();
                        assert!(srcs[i].pending);
                        assert_eq!(s.by, Cost::of(srcs[i].len).bytes());
                        // No voice reads the hole or anything that moved.
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
        check(&a, &snds, &srcs);
    }
    // The run reached every branch.
    assert!(
        refused > 0 && held > 0 && moved > 0,
        "{refused} {held} {moved}"
    );
}

/// The audio, as far as compaction sees it: voices latched to a source's
/// offset at note-on, and the `sounding` set it publishes each block.
struct Audio {
    voices: Vec<(u32, u32)>, // (offset, crc at note-on)
}

impl Audio {
    fn sounding(&self) -> Vec<u32> {
        self.voices.iter().map(|v| v.0).collect()
    }
    /// One block: every voice reads its source's bytes as they were.
    fn block(&self, a: &ArenaWriter, len: usize) {
        for &(o, c) in &self.voices {
            assert_eq!(crc(a.bytes(o, len)), c, "a voice read moved bytes");
        }
    }
}

/// ADR 0087's gate, as Task 8 runs it: wait for `sounding & above == 0`,
/// close, ack, re-check from a block after the ack, then move.
#[test]
fn no_move_under_a_sounding_voice() {
    let mut a = arena();
    let hole = a.source_alloc(256).unwrap();
    let b = a.source_alloc(256).unwrap();
    let cb = stamp(&mut a, b, 256, 7);
    a.source_free_pending(hole, 256);
    let mut audio = Audio {
        voices: vec![(b, cb)],
    };
    // A drone holds B: the hole stays pending for as long as it sounds.
    for _ in 0..100 {
        let sounding = audio.sounding();
        assert_eq!(a.source_compact(|o| !sounding.contains(&o)), None);
        audio.block(&a, 256);
    }
    assert_eq!(a.pending().bytes(), 256);
    audio.voices.clear();
    let sounding = audio.sounding();
    let s = a.source_compact(|o| !sounding.contains(&o)).unwrap();
    assert_eq!(
        s,
        Shift {
            from: hole,
            by: 256
        }
    );
    assert_eq!(crc(a.bytes(hole, 256)), cb);
}

/// The late note-on: a voice starts on B in the block just before the gate
/// closes. A move gated on the check from before the close would move B
/// under it; the re-check after the ack sees its bit and moves nothing.
#[test]
fn late_note_on_caught_by_recheck() {
    let setup = || {
        let mut a = arena();
        let hole = a.source_alloc(256).unwrap();
        let b = a.source_alloc(256).unwrap();
        let cb = stamp(&mut a, b, 256, 7);
        a.source_free_pending(hole, 256);
        (a, b, cb)
    };

    // Without the re-check: the pre-close snapshot is clear, so it moves.
    let (mut a, b, cb) = setup();
    let mut audio = Audio { voices: vec![] };
    let before_close = audio.sounding();
    audio.voices.push((b, cb)); // note-on, block before the close
    assert!(a.source_compact(|o| !before_close.contains(&o)).is_some());
    // B's old bytes are gap now; the next add lands on them.
    assert_eq!(a.source_alloc(256), Some(b));
    stamp(&mut a, b, 256, 8);
    assert_ne!(
        crc(a.bytes(b, 256)),
        cb,
        "the stale check let B move under its voice"
    );

    // With it: the gate closed and acked, the UI reads `sounding` again.
    let (mut a, b, cb) = setup();
    let mut audio = Audio { voices: vec![] };
    assert!(audio.sounding().is_empty()); // step 1: clear
    audio.voices.push((b, cb)); // the late note-on
    audio.block(&a, 256); // the block that acks the close
    let after_ack = audio.sounding();
    assert_eq!(a.source_compact(|o| !after_ack.contains(&o)), None);
    for _ in 0..10 {
        audio.block(&a, 256); // the gate reopened; the note plays on
    }
    audio.voices.clear(); // the note ends
    let after_ack = audio.sounding();
    assert!(a.source_compact(|o| !after_ack.contains(&o)).is_some());
    assert_eq!(a.pending().bytes(), 0);
}
