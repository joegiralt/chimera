//! The card interrupt's decisions (slice 1 § 7.2), pure so the board only
//! runs them: the earliest Wanted slot it can read, claimed, with its
//! file's registry row held for the read (#597), else the bus goes back;
//! and what a failed read does next, a chunk that won't de-frame blamed on
//! the card once and on its file after (#609).

use core::sync::atomic::Ordering;

use chimera_hal::card_bus::{CardBus, CardFault, ProtoError};
use chimera_hal::units::UiWait;

use super::edf::{Pick, Want, pick, wants};
use super::layout::{ChunkPlan, ChunkRead, FileChunk, StreamFile};
use super::registry::{RowHold, StreamRegistry};
use super::ring::{CardSide, Claim, Rings, SLOTS};
use super::stats::StreamStats;
use super::units::AudioBlocks;
use crate::hw::MAX_VOICES;

pub enum ServeStep<'r> {
    /// Read `claimed`'s chunk (`row.read()`); the row stays held until
    /// `row` drops.
    Read { claimed: Claim, row: HeldRead<'r> },
    /// Every ring full and the UI waits: the bus goes to it.
    Yield,
    /// Nothing to read.
    Idle,
}

/// A held row and the claimed chunk's read on it, worked out once.
pub struct HeldRead<'r> {
    row: RowHold<'r>,
    plan: ChunkPlan,
}

impl HeldRead<'_> {
    pub fn file(&self) -> &StreamFile {
        self.row.file()
    }

    /// The claimed chunk's read, as `serve_step` found it.
    pub fn read(&self) -> ChunkRead<'_> {
        self.row.file().read_of(self.plan)
    }
}

/// The Wanted slots whose rings name a registry row.
pub fn servable<'a>(rings: &'a Rings, reg: &'a StreamRegistry) -> impl Iterator<Item = Want> + 'a {
    wants(rings).filter(|w| {
        rings
            .source(w.voice)
            .is_some_and(|id| reg.get(id).is_some())
    })
}

/// The next read: EDF over the servable wants (owner rule: any want
/// before a waiting UI, `edf::pick`). A want cancelled since the pick, or
/// whose chunk its file can't read (past its runs, or `rot` holds it
/// rotted), is passed over, the latter left Wanted (requeued) for a later
/// wake and counted (`unreadable`). Never a fault (`on_error`): the
/// file's, not the card's, so a re-init can't mend it, and its voice alone
/// starves, fading out, and comes back past the chunk as tape time runs.
pub fn serve_step<'r>(
    card: &mut CardSide<'_>,
    reg: &'r StreamRegistry,
    now: AudioBlocks,
    ui: UiWait,
    rot: &Rot,
    stats: &StreamStats,
) -> ServeStep<'r> {
    let rings = card.rings();
    let mut passed = [[false; SLOTS]; MAX_VOICES];
    loop {
        let open = servable(rings, reg).filter(|w| !passed[w.voice.index()][w.slot.index()]);
        let Pick::Fill(w) = pick(open, now) else {
            return match ui {
                UiWait::Waiting => ServeStep::Yield,
                UiWait::Idle => ServeStep::Idle,
            };
        };
        passed[w.voice.index()][w.slot.index()] = true;
        let Some(claimed) = card.claim(w.voice, w.slot) else {
            continue;
        };
        // After the claim: the source the want's note set (`Rings::source`).
        let held = rings.source(w.voice).and_then(|id| reg.hold(id));
        let plan = |f: &StreamFile| {
            let c = claimed.chunk();
            let read = f.chunk_read(c)?;
            (!rot.rotted(f.chunk_key(c))).then(|| read.plan())
        };
        match held.and_then(|row| Some((plan(row.file())?, row))) {
            Some((plan, row)) => {
                return ServeStep::Read {
                    claimed,
                    row: HeldRead { row, plan },
                };
            }
            None => {
                card.requeue(claimed);
                stats.unreadable.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Who a chunk that won't de-frame is blamed on (#609).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blame {
    /// Its first failure: the card may have sent the wrong blocks, and a
    /// re-init may mend it.
    Card,
    /// It failed again after the re-init: the file's own bytes.
    File,
}

/// Why a fill failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The card's: CMD18's R1, a data token or CRC16, a timeout.
    Proto(ProtoError),
    /// CRC-good blocks whose records don't de-frame (`layout::deframe`),
    /// blamed as `Rot::blame` says.
    Frame(Blame),
}

/// A failed fill's next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorStep {
    /// The transfer once more, its claim kept.
    Retry,
    /// `CardFault::Read`: the claim requeued, nothing more starts until
    /// the UI's re-init and mount clear it.
    Fault,
    /// The fault, with no retry: the same bytes would come back. Counted
    /// in `frame_errors`.
    FrameError,
    /// The file's, not the card's: no fault, the claim requeued and its
    /// chunk passed over from now on (`serve_step`), so only the voices
    /// reading it drop out over it. Counted in `frame_errors`.
    Rotted,
}

/// A CRC16 mismatch or an error token is read again, once; a second
/// failure, a token timeout or a bad R1 faults (C1). A chunk that won't
/// de-frame faults the first time, and is the file's after (#609).
pub fn on_error(err: ReadError, retried: bool) -> ErrorStep {
    match err {
        ReadError::Proto(ProtoError::Crc | ProtoError::ErrorToken(_)) if !retried => {
            ErrorStep::Retry
        }
        ReadError::Proto(_) => ErrorStep::Fault,
        ReadError::Frame(Blame::Card) => ErrorStep::FrameError,
        ReadError::Frame(Blame::File) => ErrorStep::Rotted,
    }
}

/// `on_error`'s step, taken: a fault set on `bus`, a frame error counted.
pub fn read_failed(err: ReadError, retried: bool, bus: &CardBus, stats: &StreamStats) -> ErrorStep {
    let step = on_error(err, retried);
    if matches!(step, ErrorStep::FrameError | ErrorStep::Rotted) {
        stats.frame_errors.fetch_add(1, Ordering::Relaxed);
    }
    if matches!(step, ErrorStep::Fault | ErrorStep::FrameError) {
        bus.set_fault(CardFault::Read);
    }
    step
}

/// How many chunks `Rot` keeps a verdict on.
const MARKS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    /// Failed once: the card's fault, maybe.
    Suspect(FileChunk),
    /// Failed again after the re-init: the file's.
    Rotted(FileChunk),
}

impl Mark {
    fn chunk(self) -> FileChunk {
        match self {
            Mark::Suspect(c) | Mark::Rotted(c) => c,
        }
    }
}

/// The chunks that didn't de-frame (#609), the oldest forgotten first: a
/// first failure suspected, so the card faults and re-inits as for any
/// read error (it may have sent the wrong blocks); the same chunk failing
/// again after that is the file's, rotted, and never read again. A rotted
/// record in a LOOP bed so drops out its own voice each lap, not every
/// stream. The card interrupt's alone.
#[derive(Debug)]
pub struct Rot {
    marks: [Option<Mark>; MARKS],
    /// The mark the next suspect takes.
    next: usize,
}

impl Default for Rot {
    fn default() -> Self {
        Self::new()
    }
}

impl Rot {
    pub const fn new() -> Rot {
        Rot {
            marks: [None; MARKS],
            next: 0,
        }
    }

    /// Chunk `c` didn't de-frame: the card's the first time, the file's
    /// once it failed before.
    pub fn blame(&mut self, c: FileChunk) -> Blame {
        match self.find(c) {
            Some(i) => {
                self.marks[i] = Some(Mark::Rotted(c));
                Blame::File
            }
            None => {
                self.marks[self.next] = Some(Mark::Suspect(c));
                self.next = (self.next + 1) % MARKS;
                Blame::Card
            }
        }
    }

    /// Chunk `c` de-framed: a suspicion of it was the card's.
    pub fn cleared(&mut self, c: FileChunk) {
        if let Some(m) = self
            .marks
            .iter_mut()
            .find(|m| **m == Some(Mark::Suspect(c)))
        {
            *m = None;
        }
    }

    /// Chunk `c` is the file's: passed over.
    pub fn rotted(&self, c: FileChunk) -> bool {
        self.marks.contains(&Some(Mark::Rotted(c)))
    }

    fn find(&self, c: FileChunk) -> Option<usize> {
        self.marks
            .iter()
            .position(|m| m.is_some_and(|m| m.chunk() == c))
    }
}
