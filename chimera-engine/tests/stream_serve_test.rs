//! The card interrupt's decision (slice 1 § 7.2): which voice's Wanted
//! slot it reads next, from the file its ring names, or that it gives the
//! bus up. The board only runs it.

use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::name::SoundName;
use chimera_engine::project::SourceId;
use chimera_engine::storage::{SourceHead, SourceLayout, SourceRef, data_offset};
use chimera_engine::stream::layout::{ChunkIdx, StreamFile};
#[cfg(feature = "tape")]
use chimera_engine::stream::layout::{FileChunk, FileKey};
use chimera_engine::stream::registry::StreamRegistry;
use chimera_engine::stream::ring::{AudioSide, CardSide, Rings, ThirdSlots};
#[cfg(feature = "tape")]
use chimera_engine::stream::serve::Blame;
use chimera_engine::stream::serve::{Parked, Rot, ServeStep, readable, serve_step};
use chimera_engine::stream::slot::SlotState;
use chimera_engine::stream::stats::StreamStats;
use chimera_engine::stream::units::{AudioBlocks, Frames, SlotIdx};
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_hal::store::{FileRuns, Runs};
use chimera_hal::units::{BlockCount, Bytes, Lba};

const NOW: AudioBlocks = AudioBlocks::new(100);
const V1: VoiceIdx = VoiceIdx::ALL[1];
const V2: VoiceIdx = VoiceIdx::ALL[2];

/// A `Plain` file of `frames` in one run at `lba`.
fn file(frames: u32, lba: u32) -> StreamFile {
    let head = SourceHead {
        sref: SourceRef {
            crc: frames,
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("ROW").unwrap(),
    };
    let l = SourceLayout::whole(head).unwrap();
    let data = l.data_len() as u32;
    let len = data_offset(&l).get() + data + 4 * data.div_ceil(504);
    let mut runs = Runs::new();
    runs.push(Lba::new(lba), BlockCount::new(len.div_ceil(512)).unwrap());
    let runs = FileRuns {
        ext: runs.finish().unwrap(),
        len: Bytes::new(len),
    };
    StreamFile::from_layout(&l, runs).unwrap()
}

fn id(n: u8) -> SourceId {
    SourceId::new(n).unwrap()
}

fn ix(n: u32) -> ChunkIdx {
    ChunkIdx::new(n).unwrap()
}

struct Rig {
    rings: Box<Rings>,
    third: Box<ThirdSlots>,
    reg: StreamRegistry,
}

impl Rig {
    /// Sources 3 and 4 streamed, 50,000 and 90,000 frames.
    fn new() -> Rig {
        let reg = StreamRegistry::new();
        reg.insert(id(3), file(50_000, 1_000)).unwrap();
        reg.insert(id(4), file(90_000, 9_000)).unwrap();
        Rig {
            rings: Box::new(Rings::new()),
            third: Box::new(ThirdSlots::new()),
            reg,
        }
    }

    fn sides(&self) -> (AudioSide<'_>, CardSide<'_>) {
        self.rings.sides(&self.third).unwrap()
    }
}

fn step<'r>(card: &mut CardSide<'_>, reg: &'r StreamRegistry) -> ServeStep<'r> {
    serve_step(
        card,
        reg,
        NOW,
        &mut Rot::new(),
        &Parked::new(),
        &StreamStats::new(),
    )
}

/// Runs a `Read` as the interrupt does: its chunk's bytes into the slot,
/// then Ready. The chunk's first byte says which file it came from.
fn land(card: &mut CardSide<'_>, s: ServeStep<'_>) -> (VoiceIdx, SlotIdx, Frames) {
    let ServeStep::Read { claimed, row } = s else {
        panic!("a read");
    };
    let frames = row.file().frames();
    assert!(row.file().chunk_read(claimed.chunk()).is_some());
    let (v, slot) = (claimed.voice(), claimed.slot());
    card.fill_buf(&claimed).unwrap()[0] = (frames.get() / 10_000) as u8;
    card.write_guard(&claimed);
    assert!(card.fill_done(claimed));
    (v, slot, frames)
}

#[test]
fn irq_step_fills_wanted_slot() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let turn = audio.turn();
    turn.stream(V1, id(3));
    turn.stream(V2, id(4));
    let g1 = turn.want(V1, SlotIdx::A, ix(0), NOW.after(9)).unwrap();
    let g2 = turn.want(V2, SlotIdx::B, ix(2), NOW.after(3)).unwrap();

    let first = step(&mut card, &rig.reg);
    assert_eq!(
        land(&mut card, first),
        (V2, SlotIdx::B, Frames::new(90_000))
    );
    let r = turn.ready(V2, SlotIdx::B, g2).unwrap();
    assert_eq!((r.chunk(), r.sample(0).map(|s| s & 0xFF)), (ix(2), Some(9)));

    let next = step(&mut card, &rig.reg);
    assert_eq!(land(&mut card, next), (V1, SlotIdx::A, Frames::new(50_000)));
    assert!(turn.ready(V1, SlotIdx::A, g1).is_some());
    assert!(matches!(step(&mut card, &rig.reg), ServeStep::Idle));
}

/// A slot the audio cancels while its read is in flight takes nothing:
/// the read's bytes are never seen, and its new want is read next.
#[test]
fn a_slot_cancelled_mid_read_lands_nowhere() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let mut turn = audio.turn();
    turn.stream(V1, id(3));
    turn.want(V1, SlotIdx::C, ix(1), NOW).unwrap();
    let ServeStep::Read { claimed, row } = step(&mut card, &rig.reg) else {
        panic!("a read");
    };
    turn.cancel(V1, SlotIdx::C);
    let again = turn.want(V1, SlotIdx::C, ix(1), NOW).unwrap();
    assert!(card.fill_buf(&claimed).is_none());
    card.write_guard(&claimed);
    assert!(!card.fill_done(claimed), "dropped");
    drop(row);
    assert!(turn.ready(V1, SlotIdx::C, again).is_none());
    assert!(matches!(
        turn.seen(V1, SlotIdx::C),
        SlotState::Wanted { .. }
    ));
    let s = step(&mut card, &rig.reg);
    assert_eq!(land(&mut card, s).1, SlotIdx::C);
    assert!(turn.ready(V1, SlotIdx::C, again).is_some());
}

/// Owner rule (#569): any want beats a waiting UI: `Idle`, the bus going
/// back, only once no want is left to read.
#[test]
fn the_ui_is_last() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let turn = audio.turn();
    assert!(matches!(step(&mut card, &rig.reg), ServeStep::Idle));
    turn.stream(V1, id(3));
    turn.want(V1, SlotIdx::A, ix(0), NOW.after(5_000)).unwrap();
    let s = step(&mut card, &rig.reg);
    assert_eq!(land(&mut card, s).0, V1, "a far want before the UI");
    assert!(matches!(step(&mut card, &rig.reg), ServeStep::Idle));
}

/// A want whose ring names no row, or a chunk past its file, is left
/// Wanted and passed over: the next want is read.
#[test]
fn a_want_the_card_cannot_read_is_passed_over() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let turn = audio.turn();
    let v0 = VoiceIdx::ALL[0];
    turn.want(v0, SlotIdx::A, ix(0), NOW).unwrap();
    turn.stream(V1, id(9));
    turn.want(V1, SlotIdx::A, ix(0), NOW).unwrap();
    turn.stream(V2, id(3));
    let past = rig.reg.get(id(3)).unwrap().last_chunk().next().unwrap();
    turn.want(V2, SlotIdx::A, past, NOW).unwrap();
    turn.want(V2, SlotIdx::B, ix(0), NOW.after(50)).unwrap();

    let (stats, parked) = (StreamStats::new(), Parked::new());
    let mut rot = Rot::new();
    let s = serve_step(&mut card, &rig.reg, NOW, &mut rot, &parked, &stats);
    assert_eq!(land(&mut card, s).1, SlotIdx::B);
    for _ in 0..3 {
        assert!(matches!(
            serve_step(&mut card, &rig.reg, NOW, &mut rot, &parked, &stats),
            ServeStep::Idle
        ));
    }
    // The chunk off its file, parked and counted once, not each look; a
    // ring with no row isn't a read at all.
    assert_eq!(
        stats.unreadable.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(readable(&rig.rings, &rig.reg, &parked).count(), 0);
    for (v, s) in [(v0, SlotIdx::A), (V1, SlotIdx::A), (V2, SlotIdx::A)] {
        assert!(
            matches!(turn.seen(v, s), SlotState::Wanted { .. }),
            "{v:?} {s:?} left Wanted"
        );
    }
}

/// #597: a read holds its row until it ends: the pool's drain can't free
/// it under the read.
#[test]
fn a_read_holds_its_row() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let turn = audio.turn();
    turn.stream(V1, id(3));
    turn.want(V1, SlotIdx::A, ix(0), NOW).unwrap();
    let s = step(&mut card, &rig.reg);
    // SAFETY: no voice holds source 3 but the read's hold.
    assert!(!unsafe { rig.reg.clear(id(3)) }, "held by the read");
    land(&mut card, s);
    assert!(unsafe { rig.reg.clear(id(3)) }, "free once it ends");
}

/// C1, I2: a CRC or an error token is read again once; a second failure,
/// a token timeout or CMD18's bad R1 faults the card; a chunk that won't
/// de-frame faults it with no retry, counted apart, or is the file's once
/// blamed on it (#609).
#[test]
fn read_errors_retry_once_then_fault() {
    use chimera_engine::stream::serve::{Blame, ErrorStep, ReadError, on_error};
    use chimera_hal::card_bus::ProtoError;
    let p = ReadError::Proto;
    for (err, first, again) in [
        (p(ProtoError::Crc), ErrorStep::Retry, ErrorStep::Fault),
        (
            p(ProtoError::ErrorToken(0x08)),
            ErrorStep::Retry,
            ErrorStep::Fault,
        ),
        (p(ProtoError::Timeout), ErrorStep::Fault, ErrorStep::Fault),
        (p(ProtoError::R1(0x04)), ErrorStep::Fault, ErrorStep::Fault),
        (
            ReadError::Frame(Blame::Card),
            ErrorStep::FrameError,
            ErrorStep::FrameError,
        ),
        (
            ReadError::Frame(Blame::Chunk),
            ErrorStep::Rotted,
            ErrorStep::Rotted,
        ),
        (
            ReadError::Frame(Blame::File),
            ErrorStep::Rotted,
            ErrorStep::Rotted,
        ),
    ] {
        assert_eq!(on_error(err, false), first, "{err:?}");
        assert_eq!(on_error(err, true), again, "{err:?} retried");
    }
}

/// The step taken: a retry touches nothing; a fault sets the bus's; a
/// frame error counts, then faults, unless it is the file's (#609).
#[test]
fn a_failed_read_faults_the_bus_and_counts() {
    use chimera_engine::stream::serve::{Blame, ErrorStep, ReadError, read_failed};
    use chimera_hal::card_bus::{CardBus, CardFault, ProtoError};
    use std::sync::atomic::Ordering::Relaxed;
    let (bus, stats) = (CardBus::new(), StreamStats::new());
    let crc = ReadError::Proto(ProtoError::Crc);
    assert_eq!(read_failed(crc, false, &bus, &stats), ErrorStep::Retry);
    assert_eq!(bus.fault(), CardFault::Clear);
    assert_eq!(read_failed(crc, true, &bus, &stats), ErrorStep::Fault);
    assert_eq!(bus.fault(), CardFault::Read);
    assert_eq!(stats.frame_errors.load(Relaxed), 0);

    let bus = CardBus::new();
    assert_eq!(
        read_failed(ReadError::Frame(Blame::Card), false, &bus, &stats),
        ErrorStep::FrameError
    );
    assert_eq!(bus.fault(), CardFault::Read);
    assert_eq!(stats.frame_errors.load(Relaxed), 1);

    let bus = CardBus::new();
    assert_eq!(
        read_failed(ReadError::Frame(Blame::File), false, &bus, &stats),
        ErrorStep::Rotted
    );
    assert_eq!(bus.fault(), CardFault::Clear, "the file's: no fault");
    assert_eq!(stats.frame_errors.load(Relaxed), 2);
}

#[cfg(feature = "tape")]
fn chunk(stem: u32, file_at: u32, at: u32) -> FileChunk {
    FileChunk::new(FileKey::new(stem, Lba::new(file_at)), Lba::new(at))
}

/// #609: a chunk's first frame error is the card's; the same chunk failing
/// again, whatever failed between, is the file's; a second such chunk
/// condemns the file whole, every chunk of it passed over. One that
/// de-framed in between was the card's after all.
#[test]
// `Rot` blames nothing without TAPE: nothing streams.
#[cfg(feature = "tape")]
fn rot_blames_the_card_once_then_the_file() {
    let (a, b, c) = (chunk(7, 100, 108), chunk(7, 100, 116), chunk(7, 100, 124));
    let other_file = chunk(8, 900, 908);
    let mut rot = Rot::new();
    assert_eq!(rot.blame(a), Blame::Card);
    assert!(!rot.passes_over(a), "suspected, still read");
    assert_eq!(rot.blame(b), Blame::Card);
    assert_eq!(rot.blame(other_file), Blame::Card, "another file's chunk");
    assert_eq!(rot.blame(a), Blame::Chunk);
    assert!(rot.passes_over(a) && !rot.passes_over(b) && !rot.passes_over(c));
    rot.cleared(b);
    assert_eq!(rot.blame(b), Blame::Card, "it de-framed: the card's");
    rot.cleared(a);
    assert!(rot.passes_over(a), "a rotted chunk stays rotted");
    assert_eq!(rot.blame(b), Blame::File, "two rotted: the whole file");
    assert!(rot.passes_over(c), "never read, passed over with its file");
    assert!(!rot.passes_over(other_file));
}

/// #609 (I1): a full table drops its oldest suspects, never a verdict, so
/// any number of bad chunks elsewhere can't make a condemned file or a
/// rotted chunk fault the card again.
#[test]
// `Rot` blames nothing without TAPE: nothing streams.
#[cfg(feature = "tape")]
fn rot_full_keeps_its_verdicts() {
    let mut rot = Rot::new();
    let (f1, f2) = (chunk(1, 10, 18), chunk(1, 10, 26));
    let lone = chunk(2, 20, 28);
    for c in [f1, f1, f2, f2, lone, lone] {
        rot.blame(c);
    }
    assert!(rot.passes_over(chunk(1, 10, 99)), "file 1 condemned");
    // Twenty suspects from twenty files: the table holds 8.
    let s = |n: u32| chunk(100 + n, 1_000 + n, 1_008 + n);
    for n in 0..20 {
        assert_eq!(rot.blame(s(n)), Blame::Card);
    }
    assert!(rot.passes_over(chunk(1, 10, 99)), "the verdict kept");
    assert!(rot.passes_over(lone), "the rotted chunk kept");
    assert_eq!(rot.blame(s(19)), Blame::Chunk, "the newest suspect kept");
    assert_eq!(rot.blame(s(0)), Blame::Card, "the oldest suspect dropped");
}

/// #609 (I2): a copy of the same content at other blocks is another file:
/// it plays clean, though the old copy is condemned.
#[test]
// `Rot` blames nothing without TAPE: nothing streams.
#[cfg(feature = "tape")]
fn a_copy_at_new_blocks_is_clean() {
    let mut rot = Rot::new();
    for c in [chunk(7, 100, 108), chunk(7, 100, 108), chunk(7, 100, 116)] {
        rot.blame(c);
    }
    assert_eq!(rot.blame(chunk(7, 100, 116)), Blame::File);
    assert!(rot.passes_over(chunk(7, 100, 124)));
    assert!(
        !rot.passes_over(chunk(7, 5_000, 5_008)),
        "same stem, new blocks"
    );
    assert_eq!(rot.blame(chunk(7, 5_000, 5_008)), Blame::Card);
}

/// #609: a rotted chunk's want is passed over, requeued and counted
/// `unreadable`, never read; other voices' wants are served. Another card
/// (`clear_all` moves the registry's epoch) forgets it: read again.
#[test]
// `Rot` blames nothing without TAPE: nothing streams.
#[cfg(feature = "tape")]
fn serve_step_passes_over_a_rotted_chunk_till_the_card_changes() {
    use std::sync::atomic::Ordering::Relaxed;
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let turn = audio.turn();
    turn.stream(V1, id(3));
    turn.stream(V2, id(4));
    turn.want(V1, SlotIdx::A, ix(1), NOW.after(1)).unwrap();
    turn.want(V2, SlotIdx::A, ix(2), NOW.after(9)).unwrap();
    let mut rot = Rot::new();
    let f = *rig.reg.get(id(3)).unwrap();
    let key = f.chunk_read(ix(1)).unwrap().key().unwrap();
    assert_eq!(rot.blame(key), Blame::Card);
    assert_eq!(rot.blame(key), Blame::Chunk);
    let (stats, parked) = (StreamStats::new(), Parked::new());
    let s = serve_step(&mut card, &rig.reg, NOW, &mut rot, &parked, &stats);
    assert_eq!(land(&mut card, s), (V2, SlotIdx::A, Frames::new(90_000)));
    assert_eq!(stats.unreadable.load(Relaxed), 1);
    assert!(matches!(
        turn.seen(V1, SlotIdx::A),
        SlotState::Wanted { .. }
    ));
    // M2: parked, so no wake for it, and no count each look.
    for _ in 0..3 {
        assert!(matches!(
            serve_step(&mut card, &rig.reg, NOW, &mut rot, &parked, &stats),
            ServeStep::Idle
        ));
    }
    assert_eq!(stats.unreadable.load(Relaxed), 1);
    assert_eq!(readable(&rig.rings, &rig.reg, &parked).count(), 0);
    // Another card: every row freed, the same file's row back.
    // SAFETY: no voice or card read holds a reference into the rows.
    assert!(unsafe { rig.reg.clear_all() });
    rig.reg.insert(id(3), f).unwrap();
    assert_eq!(
        readable(&rig.rings, &rig.reg, &parked).count(),
        1,
        "unparked"
    );
    let s = serve_step(&mut card, &rig.reg, NOW, &mut rot, &parked, &stats);
    let ServeStep::Read { claimed, .. } = s else {
        panic!("read again on the new card");
    };
    assert_eq!((claimed.voice(), claimed.chunk()), (V1, ix(1)));
}

/// #609: the read that condemns a file whole marks its row, under any row
/// on that file, for the SOURCE cell; a fresh insert starts clean.
#[test]
// `Rot` blames nothing without TAPE: nothing streams.
#[cfg(feature = "tape")]
fn a_condemned_file_marks_its_rows() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let mut turn = audio.turn();
    turn.stream(V1, id(3));
    let f = *rig.reg.get(id(3)).unwrap();
    rig.reg.insert(id(5), f).unwrap();
    let mut rot = Rot::new();
    let (stats, parked) = (StreamStats::new(), Parked::new());
    let mut fail = |c: u32| {
        let s = turn.want(V1, SlotIdx::A, ix(c), NOW.after(1));
        assert!(s.is_some());
        let s = serve_step(&mut card, &rig.reg, NOW, &mut rot, &parked, &stats);
        let ServeStep::Read { claimed, row } = s else {
            panic!("a read");
        };
        let e = row.frame_error(&mut rot);
        drop(row);
        card.requeue(claimed);
        turn.cancel(V1, SlotIdx::A);
        e
    };
    use chimera_engine::stream::serve::ReadError::Frame;
    assert_eq!(fail(1), Frame(Blame::Card));
    assert_eq!(fail(1), Frame(Blame::Chunk));
    assert!(!rig.reg.damaged(id(3)));
    assert_eq!(fail(2), Frame(Blame::Card));
    assert_eq!(fail(2), Frame(Blame::File));
    assert!(rig.reg.damaged(id(3)) && rig.reg.damaged(id(5)));
    assert!(!rig.reg.damaged(id(4)), "another file");
}
