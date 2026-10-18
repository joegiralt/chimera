//! The card interrupt's decision (slice 1 § 7.2): which voice's Wanted
//! slot it reads next, from the file its ring names, or that it gives the
//! bus up. The board only runs it.

use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::name::SoundName;
use chimera_engine::project::SourceId;
use chimera_engine::storage::{SourceHead, SourceLayout, SourceRef, data_offset};
use chimera_engine::stream::layout::{ChunkIdx, StreamFile};
use chimera_engine::stream::registry::StreamRegistry;
use chimera_engine::stream::ring::{AudioSide, CardSide, Rings, ThirdSlots};
use chimera_engine::stream::serve::{ServeStep, serve_step};
use chimera_engine::stream::slot::SlotState;
use chimera_engine::stream::stats::StreamStats;
use chimera_engine::stream::units::{AudioBlocks, Frames, SlotIdx};
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_hal::store::{FileRuns, Runs};
use chimera_hal::units::{BlockCount, Bytes, Lba, UiWait};

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

fn step<'r>(card: &mut CardSide<'_>, reg: &'r StreamRegistry, ui: UiWait) -> ServeStep<'r> {
    serve_step(card, reg, NOW, ui, &StreamStats::new())
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

    let first = step(&mut card, &rig.reg, UiWait::Idle);
    assert_eq!(
        land(&mut card, first),
        (V2, SlotIdx::B, Frames::new(90_000))
    );
    let r = turn.ready(V2, SlotIdx::B, g2).unwrap();
    assert_eq!((r.chunk(), r.sample(0).map(|s| s & 0xFF)), (ix(2), Some(9)));

    let next = step(&mut card, &rig.reg, UiWait::Idle);
    assert_eq!(land(&mut card, next), (V1, SlotIdx::A, Frames::new(50_000)));
    assert!(turn.ready(V1, SlotIdx::A, g1).is_some());
    assert!(matches!(
        step(&mut card, &rig.reg, UiWait::Idle),
        ServeStep::Idle
    ));
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
    let ServeStep::Read { claimed, row } = step(&mut card, &rig.reg, UiWait::Idle) else {
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
    let s = step(&mut card, &rig.reg, UiWait::Idle);
    assert_eq!(land(&mut card, s).1, SlotIdx::C);
    assert!(turn.ready(V1, SlotIdx::C, again).is_some());
}

/// Owner rule (#569): any want beats a waiting UI; the UI gets the bus
/// only once every ring is full.
#[test]
fn the_ui_is_last() {
    let rig = Rig::new();
    let (mut audio, mut card) = rig.sides();
    let turn = audio.turn();
    assert!(matches!(
        step(&mut card, &rig.reg, UiWait::Waiting),
        ServeStep::Yield
    ));
    turn.stream(V1, id(3));
    turn.want(V1, SlotIdx::A, ix(0), NOW.after(5_000)).unwrap();
    let s = step(&mut card, &rig.reg, UiWait::Waiting);
    assert_eq!(land(&mut card, s).0, V1, "a far want before the UI");
    assert!(matches!(
        step(&mut card, &rig.reg, UiWait::Waiting),
        ServeStep::Yield
    ));
    assert!(matches!(
        step(&mut card, &rig.reg, UiWait::Idle),
        ServeStep::Idle
    ));
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

    let stats = StreamStats::new();
    let s = serve_step(&mut card, &rig.reg, NOW, UiWait::Idle, &stats);
    assert_eq!(land(&mut card, s).1, SlotIdx::B);
    assert!(matches!(
        serve_step(&mut card, &rig.reg, NOW, UiWait::Idle, &stats),
        ServeStep::Idle
    ));
    // The chunk off its file, passed over each look; a ring with no row
    // isn't a read at all.
    assert_eq!(
        stats.unreadable.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
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
    let s = step(&mut card, &rig.reg, UiWait::Idle);
    // SAFETY: no voice holds source 3 but the read's hold.
    assert!(!unsafe { rig.reg.clear(id(3)) }, "held by the read");
    land(&mut card, s);
    assert!(unsafe { rig.reg.clear(id(3)) }, "free once it ends");
}

/// C1, I2: a CRC or an error token is read again once; a second failure,
/// a token timeout or CMD18's bad R1 faults the card; a chunk that won't
/// de-frame faults it with no retry, counted apart.
#[test]
fn read_errors_retry_once_then_fault() {
    use chimera_engine::stream::serve::{ErrorStep, ReadError, on_error};
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
            ReadError::Frame,
            ErrorStep::FrameError,
            ErrorStep::FrameError,
        ),
    ] {
        assert_eq!(on_error(err, false), first, "{err:?}");
        assert_eq!(on_error(err, true), again, "{err:?} retried");
    }
}

/// The step taken: a retry touches nothing; a fault sets the bus's; a
/// frame error counts, then faults.
#[test]
fn a_failed_read_faults_the_bus_and_counts() {
    use chimera_engine::stream::serve::{ErrorStep, ReadError, read_failed};
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
        read_failed(ReadError::Frame, false, &bus, &stats),
        ErrorStep::FrameError
    );
    assert_eq!(bus.fault(), CardFault::Read);
    assert_eq!(stats.frame_errors.load(Relaxed), 1);
}
