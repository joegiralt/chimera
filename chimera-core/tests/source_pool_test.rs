//! Sources in the pool (ADR 0087): their table, the gate a compaction
//! closes, retirement, purge, NEW, and pass 1's limit less the sources.

mod common;

use std::collections::BTreeMap;

use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::test_support::{
    FlipOnSecondRead, full_dense_slots, save_at, save_part_to,
};
use chimera_core::project::{
    EncodedSound, LoadLink, PartFrom, PartId, PartSource, Project, ProjectNote, ProjectSource,
    ProjectStatus, Purged, ReplaceGuard, SlotId, SourceId, SourceRef, Subject, TemplateCrc,
    boot_project, load_project, project_crc, project_file, project_status, source_users,
};
use chimera_core::source_table::{Answer, AudioToken, GateView, SourceTable};
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

/// UI passes with no audio reader: a gate is answered at once.
fn quiet(p: &mut Project) {
    for _ in 0..3 {
        p.sources_pass();
    }
}

const BLOCK: usize = 64;

/// A stub TAPE voice: latched to a source at note-on, it plays its bytes
/// a block at a time through the view. It waits while the gate moves its
/// source, and ends when its row is gone.
struct Voice {
    id: SourceId,
    pos: usize,
    len: usize,
    out: Vec<u8>,
    /// The blocks it played in, and those it waited at the gate.
    played: Vec<usize>,
    held: usize,
    gone: bool,
}

impl Voice {
    fn on(id: SourceId, len: usize) -> Voice {
        Voice {
            id,
            pos: 0,
            len,
            out: Vec::new(),
            played: Vec::new(),
            held: 0,
            gone: false,
        }
    }

    fn ended(&self) -> bool {
        self.pos == self.len || self.gone
    }

    fn block(&mut self, v: &GateView<'_>, n: usize) {
        if self.ended() {
            return;
        }
        match v.bytes(self.id) {
            Some(b) => {
                let end = (self.pos + BLOCK).min(self.len);
                self.out.extend_from_slice(&b[self.pos..end]);
                self.pos = end;
                self.played.push(n);
            }
            None if v.moving(self.id) => self.held += 1,
            None => self.gone = true,
        }
    }

    /// Latched and not ended, waiting at the gate included.
    fn sounding(&self) -> u32 {
        if self.ended() { 0 } else { self.id.bit() }
    }
}

/// The audio side: the table's one reader, its voices, and note-ons
/// that land at the next block.
struct Audio {
    tok: AudioToken,
    voices: Vec<Voice>,
    queued: Vec<Voice>,
    n: usize,
}

impl Audio {
    fn of(p: &Project) -> Audio {
        Audio {
            tok: p.pool().table().reader().expect("one reader"),
            voices: Vec::new(),
            queued: Vec::new(),
            n: 0,
        }
    }

    fn note_on(&mut self, id: SourceId, len: usize) {
        self.queued.push(Voice::on(id, len));
    }

    fn block(&mut self) {
        {
            let v = self.tok.begin_block();
            self.voices.append(&mut self.queued);
            for voice in &mut self.voices {
                voice.block(&v, self.n);
            }
        }
        let mask = self.sounding();
        self.tok.store_sounding(mask);
        self.n += 1;
    }

    fn blocks(&mut self, n: usize) {
        for _ in 0..n {
            self.block();
        }
    }

    fn sounding(&self) -> u32 {
        self.voices.iter().fold(0, |m, x| m | x.sounding())
    }
}

/// One UI pass while the audio runs: each wait for its answer is one
/// block, two at most. The blocks it ran.
fn pass(p: &mut Project, a: &mut Audio) -> usize {
    let mut k = 0;
    p.sources_pass_with(|| {
        a.block();
        k += 1;
        k < 3
    });
    k
}

#[test]
fn store_sounding_is_read() {
    let t: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
    let mut tok = t.reader().expect("one reader");
    assert!(t.reader().is_none(), "only one");
    assert_eq!(t.sounding(), 0);
    for m in [1, 0x8000_0001, u32::MAX, 0] {
        tok.store_sounding(m);
        assert_eq!(t.sounding(), m);
    }
}

#[test]
fn gate_view_bytes_none_while_moving() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(1, 1000), sref(2, 2000));
    let (ia, ib) = (store(&mut p, a), store(&mut p, b));
    let table = p.pool().table();
    let mut tok = table.reader().unwrap();
    {
        let open = tok.begin_block();
        assert_eq!(open.bytes(ia), Some(&audio(a)[..]));
        assert_eq!(open.bytes(ib), Some(&audio(b)[..]));
    }
    let e = table.close_gate_for_test(ia.bit());
    assert_eq!(table.wait_for_test(e, || false), Answer::TimedOut);
    {
        let v = tok.begin_block();
        assert_eq!(v.bytes(ia), None);
        assert!(v.moving(ia));
        assert_eq!(v.bytes(ib), Some(&audio(b)[..]));
    }
    assert_eq!(table.wait_for_test(e, || false), Answer::Acked);
    table.open_gate_for_test();
    assert_eq!(tok.begin_block().bytes(ia), Some(&audio(a)[..]));
    // An empty row reads nothing.
    assert_eq!(tok.begin_block().bytes(SourceId::ALL[31]), None);

    // A voice on a moving id: the block nacks and reads on.
    tok.store_sounding(ia.bit());
    let e = table.close_gate_for_test(ia.bit());
    assert_eq!(tok.begin_block().bytes(ia), Some(&audio(a)[..]));
    assert_eq!(table.wait_for_test(e, || false), Answer::Nacked);
    table.open_gate_for_test();
}

#[test]
#[should_panic(expected = "one arena")]
fn a_table_serves_one_arena() {
    let t = SourceTable::new();
    let mut a = [0u8; 4];
    let mut b = [0u8; 4];
    t.attach_for_test(a.as_mut_ptr());
    t.attach_for_test(b.as_mut_ptr());
}

/// Review Focus 3: a purged source still sounding comes back by itself
/// when its note ends.
#[test]
fn pending_hole_resolves_when_note_ends() {
    let (mut p, _) = Project::boxed();
    let r = sref(7, 9000);
    let id = store(&mut p, r);
    let cost = p.pool().source(id).unwrap().cost();
    let mut a = Audio::of(&p);
    a.note_on(id, audio(r).len());
    a.block();
    let gap = p.pool().free();
    let purged = p.purge_unused_sources(p.pool().table().sounding());
    assert_eq!(
        purged,
        Purged {
            freed: cost,
            retired: 1
        }
    );
    for _ in 0..8 {
        a.blocks(3);
        assert!(!p.sources_pass_with(|| false));
        assert_eq!(pending(&p), cost, "on the meter while it sounds");
        assert_eq!(p.pool().free(), gap);
    }
    while a.sounding() != 0 {
        a.block();
    }
    a.block();
    assert!(pass(&mut p, &mut a) > 0);
    assert_eq!(pending(&p), Cost::default());
    assert_eq!(
        p.pool().free(),
        Cost::of((gap.bytes() + cost.bytes()) as usize)
    );
    assert_eq!(p.unused_sources().count, 0, "no second purge");
    assert_eq!(a.voices[0].out, audio(r), "never cut");
}

#[test]
fn retire_never_cuts() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(10, 3000), sref(11, 5000));
    let ia = store(&mut p, a);
    let ib = store(&mut p, b);
    tape_sound(&mut p, "KEEP B", b);
    let mut au = Audio::of(&p);
    au.note_on(ia, audio(a).len());
    au.block();
    assert_eq!(
        p.purge_unused_sources(p.pool().table().sounding()).retired,
        1
    );
    assert!(p.pool().source(ia).is_none(), "out of the index at once");
    while au.sounding() != 0 {
        pass(&mut p, &mut au);
        au.block();
        let v = au.tok.begin_block();
        assert_eq!(v.bytes(ib), Some(&audio(b)[..]));
    }
    assert_eq!(au.voices[0].out, audio(a), "played to its end");
    au.blocks(2);
    pass(&mut p, &mut au);
    let v = au.tok.begin_block();
    assert_eq!(v.bytes(ia), None);
    assert_eq!(v.bytes(ib), Some(&audio(b)[..]), "moved, unchanged");
}

/// Retires the lowest source, `low`: the hole below the kept `above`.
fn hole_below(p: &mut Project, low: SourceRef, above: &[SourceRef]) -> SourceId {
    let id = store(p, low);
    for &r in above {
        store(p, r);
        tape_sound(p, "KEEP", r);
    }
    assert_eq!(p.purge_unused_sources(0).retired, 0);
    assert!(pending(p) > Cost::default());
    id
}

/// A note-on in a block that began before the close: the next block's
/// voices hold a moving id, so it nacks and reads on. Nothing moves, the
/// voice never waits, and the hole closes once the note ends.
#[test]
fn gate_recheck_catches_late_note_on() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(20, 2000), sref(21, 4000));
    let ia = hole_below(&mut p, a, &[b]);
    let ib = p.pool().find_source(b).unwrap();
    let table = p.pool().table();
    let mut au = Audio::of(&p);
    let (gap, held) = (p.pool().free(), pending(&p));
    let mut voice = Voice::on(ib, audio(b).len());

    // The block began open; the UI closes during it; the note starts.
    let e = {
        let v = au.tok.begin_block();
        let e = table.close_gate_for_test(ia.bit() | ib.bit());
        voice.block(&v, 0);
        e
    };
    au.tok.store_sounding(voice.sounding());
    // The next block's voices hold a moving id: a nack, and it reads on.
    voice.block(&au.tok.begin_block(), 1);
    au.tok.store_sounding(voice.sounding());
    assert_eq!(table.wait_for_test(e, || false), Answer::Nacked);
    table.open_gate_for_test();
    assert_eq!((p.pool().free(), pending(&p)), (gap, held), "nothing moved");
    assert_eq!(voice.held, 0);

    // The pool's pass won't close while it sounds; then it closes the hole.
    au.voices.push(voice);
    while au.sounding() != 0 {
        assert!(!p.sources_pass_with(|| false));
        assert_eq!(pending(&p), held, "nothing moves under the voice");
        au.block();
    }
    assert!(pass(&mut p, &mut au) > 0);
    assert_eq!(pending(&p), Cost::default());
    assert_eq!(au.voices[0].out, audio(b), "the unmoved source, throughout");
    assert_eq!(au.tok.begin_block().bytes(ib), Some(&audio(b)[..]));
}

/// A note-on on a moving source in the block that acks waits that block
/// only, and starts on the first block after the gate opens, its first
/// sample unchanged. Another source starts at once.
#[test]
fn gate_holds_note_on() {
    let (mut p, _) = Project::boxed();
    let (c, a, b) = (sref(30, 1500), sref(31, 2000), sref(32, 3000));
    let ic = store(&mut p, c);
    tape_sound(&mut p, "KEEP C", c);
    hole_below(&mut p, a, &[b]);
    let ib = p.pool().find_source(b).unwrap();
    let mut au = Audio::of(&p);
    let mut k = 0;
    assert!(p.sources_pass_with(|| {
        au.note_on(ib, audio(b).len());
        au.note_on(ic, audio(c).len());
        au.block();
        k += 1;
        k < 3
    }));
    assert_eq!(k, 1, "one block under the gate");
    let (moving, other) = (&au.voices[0], &au.voices[1]);
    assert_eq!(other.played, [0], "another source starts at once");
    assert!(
        moving.played.is_empty() && moving.held == 1,
        "held for the block"
    );
    au.block();
    let moving = &au.voices[0];
    assert_eq!(moving.played, [1], "the first block after open");
    assert_eq!(moving.out, audio(b)[..BLOCK], "first sample unchanged");
}

/// F1: a UI pass every 37 blocks (a 50 ms frame), a note-on on the moving
/// source landing with each close: none waits more than two blocks.
#[test]
fn held_blocks_at_most_two() {
    let (mut p, _) = Project::boxed();
    let mut au = Audio::of(&p);
    let mut of = Vec::new();
    let mut moves = 0;
    for i in 0..24u32 {
        // Each round a hole below a kept source B: L, then B above it,
        // then L purged.
        store(&mut p, sref(1000 + i, 300 + 50 * (i % 5)));
        let b = sref(2000 + i, 600);
        let ib = store(&mut p, b);
        tape_sound(&mut p, "KEEP", b);
        assert!(p.purge_unused_sources(0).freed > Cost::default());
        for n in 0..37 {
            if n % 9 == 0 {
                au.note_on(ib, audio(b).len());
                of.push(b);
            }
            au.block();
        }
        // The last note plays out: a pass skips while B sounds.
        au.blocks(20);
        let mut k = 0;
        if p.sources_pass_with(|| {
            au.note_on(ib, audio(b).len());
            au.block();
            k += 1;
            k < 3
        }) {
            moves += 1;
        }
        of.push(b);
        au.blocks(8);
    }
    let worst = au.voices.iter().map(|v| v.held).max().unwrap();
    eprintln!(
        "held blocks: at most {worst}, over {} voices, {moves} moves",
        au.voices.len()
    );
    assert_eq!(moves, 24);
    assert_eq!(worst, 1, "held {worst} blocks");
    for (v, b) in au.voices.iter().zip(&of) {
        assert_eq!(v.out, audio(*b)[..v.out.len()], "never a moved byte");
    }
}

/// F5: a note-on reaches a retired id under the closed gate. It waits,
/// finds its row gone, and ends; its id isn't reused while it is held.
#[test]
fn retired_id_never_reused_while_held() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(50, 2000), sref(51, 2000));
    let ia = hole_below(&mut p, a, &[b]);
    let mut au = Audio::of(&p);
    let mut k = 0;
    assert!(p.sources_pass_with(|| {
        au.note_on(ia, audio(a).len());
        au.block();
        k += 1;
        k < 3
    }));
    assert_eq!(au.voices[0].held, 1);
    assert_eq!(au.sounding(), ia.bit(), "held, so it sounds");
    let c = store(&mut p, sref(52, 100));
    assert_ne!(c, ia, "not while it is held");
    au.block();
    assert!(
        au.voices[0].gone && au.voices[0].out.is_empty(),
        "its row is gone"
    );
    au.blocks(2);
    p.sources_pass_with(|| false);
    assert_eq!(store(&mut p, sref(53, 100)), ia, "free once it has ended");
}

/// The spec's `compaction_waits_for_silence`: no source byte moves while
/// its bit is set; the hole stays pending, on the meter.
#[test]
fn compaction_waits_for_silence() {
    let (mut p, _) = Project::boxed();
    let (a, b) = (sref(60, 1000), sref(61, 60_000));
    hole_below(&mut p, a, &[b]);
    let ib = p.pool().find_source(b).unwrap();
    let held = pending(&p);
    let mut au = Audio::of(&p);
    au.note_on(ib, audio(b).len());
    au.block();
    let base = au.tok.begin_block().bytes(ib).unwrap().as_ptr();
    while au.sounding() != 0 {
        assert!(!p.sources_pass_with(|| false));
        assert_eq!(pending(&p), held);
        assert_eq!(
            au.tok.begin_block().bytes(ib).unwrap().as_ptr(),
            base,
            "unmoved"
        );
        au.block();
    }
    assert!(pass(&mut p, &mut au) > 0);
    assert_eq!(pending(&p), Cost::default());
    assert_ne!(au.tok.begin_block().bytes(ib).unwrap().as_ptr(), base);
    assert_eq!(au.voices[0].out, audio(b));
}

/// The spec's seeded model: stores, uses, purges, voices and passes with
/// the sounding mask flipping. After each step every live source reads
/// its own bytes, free is the gap and the meter's parts sum to used;
/// every voice played its own source's bytes, waiting two blocks at most.
#[test]
fn seeded_model_with_flipping_sounding() {
    let (mut p, _) = Project::boxed();
    let mut au = Audio::of(&p);
    let mut held: Vec<SourceRef> = Vec::new();
    let mut of: Vec<SourceRef> = Vec::new();
    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut next = 0x7000;
    let mut moves = 0;
    for _ in 0..3000 {
        match rnd(8) {
            0 => {
                next += 1;
                let r = sref(next, 200 + rnd(3000) as u32);
                if p.source_store(r, &audio(r)).is_ok() {
                    held.push(r);
                }
            }
            1 if !held.is_empty() && p.pool().first_free().is_some() => {
                let r = held[rnd(held.len() as u64) as usize];
                tape_sound(&mut p, "USE", r);
            }
            2 => {
                p.purge_unused_sources(p.pool().table().sounding());
            }
            3 if !held.is_empty() => {
                let r = held[rnd(held.len() as u64) as usize];
                let id = p.pool().find_source(r).unwrap();
                au.note_on(id, audio(r).len().min(BLOCK * (1 + rnd(6) as usize)));
                of.push(r);
            }
            4 => {
                let before = pending(&p);
                pass(&mut p, &mut au);
                if pending(&p) < before {
                    moves += 1;
                }
            }
            5 => {
                let s = SlotId::ALL[40 + rnd(80) as usize];
                if p.users(s).is_empty() {
                    let _ = p.pool_clear(s);
                }
            }
            _ => au.blocks(1 + rnd(4) as usize),
        }
        held.retain(|r| p.pool().find_source(*r).is_some());
        {
            let v = au.tok.begin_block();
            for r in &held {
                let id = p.pool().find_source(*r).unwrap();
                assert_eq!(v.bytes(id), Some(&audio(*r)[..]), "{r:?}");
            }
        }
        let pool = p.pool();
        let (snd, src, pend) = pool.split();
        assert_eq!(snd + src + pend, pool.used());
        assert_eq!(
            pool.free().bytes() as usize,
            ARENA_BYTES - pool.used().bytes() as usize
        );
    }
    assert!(moves > 10, "{moves} compactions");
    for (v, r) in au.voices.iter().zip(&of) {
        assert!(v.held <= 2, "held {} blocks", v.held);
        assert_eq!(v.out, audio(*r)[..v.out.len()], "{r:?}");
    }
}

fn new_go(q: &Project, t: TemplateCrc) -> chimera_core::project::Confirmed<ProjectSource> {
    ReplaceGuard::check(q, t, ProjectSource::New).expect("no prompt")
}

/// NEW frees the sources: three paths to `reset_new`, an arena full of
/// sources, no panic, and NEW pristine with nothing left in `load_limit`.
#[test]
fn new_frees_the_sources() {
    let fill = |q: &mut Project| {
        let mut i = 0;
        while q.pool().free() > Cost::of(2048) {
            i += 1;
            let room = q.pool().free().bytes() as usize - 16;
            let r = sref(0x9000 + i, 0);
            q.source_store(r, &vec![i as u8; room.min(20_000)]).unwrap();
        }
        assert!(q.pool().source_used() > Cost::of(140 * 1024));
    };
    let fresh = |q: &Project, t: TemplateCrc| {
        assert_eq!(q.pool().source_used(), Cost::default());
        assert_eq!(
            q.pool().load_limit(),
            Cost::of(ARENA_BYTES),
            "no orphan left"
        );
        assert_eq!(project_status(q, t), ProjectStatus::Pristine);
    };
    let link = LoadLink::new();

    // NEW.
    let (mut q, t) = Project::boxed();
    fill(&mut q);
    let go = new_go(&q, t);
    let out = load_project(&mut Card::new(), &mut MemStore::new(1), &mut q, go, &link);
    assert!(out.swap.is_some());
    fresh(&q, t);

    // A failed load: pass 2 sees another card.
    let (mut src, _) = Project::boxed();
    let mut mem = MemStore::new(1);
    let mut card = Card::new();
    assert!(matches!(
        save_at(&mut card, &mut mem, &mut src, 1),
        ProjectNote::Saved(_)
    ));
    let id = ProjectId::new(1).unwrap();
    let mut s = FlipOnSecondRead::new(mem, project_file(id).side(Side::A));
    let (mut q, t) = Project::boxed();
    fill(&mut q);
    let vol = s.mount().unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let out = load_project(&mut card, &mut s, &mut q, go, &link);
    assert_eq!(out.note, Some(ProjectNote::LoadFailed(Subject::File(id))));
    fresh(&q, t);

    // Boot's fallback: the last project isn't on the card.
    let (mut q, t) = Project::boxed();
    fill(&mut q);
    let mut s = MemStore::new(1);
    let last = Some(ProjectId::new(7).unwrap());
    assert!(boot_project(&mut Card::new(), &mut s, last, &mut q).is_some());
    fresh(&q, t);
}

/// A source still sounding at NEW is retired, not freed under its voice.
#[test]
fn new_retires_a_sounding_source() {
    let (mut q, t) = Project::boxed();
    let r = sref(77, 8000);
    let id = store(&mut q, r);
    let cost = q.pool().source(id).unwrap().cost();
    let mut au = Audio::of(&q);
    au.note_on(id, audio(r).len());
    au.block();
    let go = new_go(&q, t);
    let _ = load_project(
        &mut Card::new(),
        &mut MemStore::new(1),
        &mut q,
        go,
        &LoadLink::new(),
    );
    assert!(q.pool().source(id).is_none(), "out of the index");
    assert_eq!(pending(&q), cost, "its bytes kept");
    assert_eq!(project_status(&q, t), ProjectStatus::Pristine);
    while au.sounding() != 0 {
        au.block();
        q.sources_pass_with(|| false);
    }
    assert_eq!(au.voices[0].out, audio(r), "never cut");
    au.block();
    pass(&mut q, &mut au);
    assert_eq!(q.pool().source_used(), Cost::default());
}

/// F10: an unused absent entry alone is nothing to offer; a purge with
/// nothing unused moves nothing, `rev` included.
#[test]
fn absent_alone_is_not_offered() {
    let (mut p, _) = Project::boxed();
    let r = sref(88, 100);
    p.source_absent(r).unwrap();
    assert_eq!(p.unused_sources().count, 0);
    assert_eq!(p.purge_unused_sources(0), Purged::default());
    assert!(
        p.pool().find_source(r).is_none(),
        "the absent entry goes with a purge"
    );
    let rev = p.rev();
    assert_eq!(p.purge_unused_sources(0), Purged::default());
    assert_eq!(p.rev(), rev, "nothing unused: no bump");
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
    quiet(&mut p);
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
                _ => quiet(&mut p),
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
        quiet(&mut p);
        assert_eq!(pending(&p), Cost::default());
    }
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
