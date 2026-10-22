//! The card interrupt's decisions (slice 1 § 7.2), pure so the board only
//! runs them: the earliest Wanted slot it can read, claimed, with its
//! file's registry row held for the read (#597), else the bus goes back;
//! and what a failed read does next, a chunk that won't de-frame blamed on
//! the card once and on its file after (#609).

use core::sync::atomic::{AtomicU32, Ordering};

use chimera_hal::card_bus::{CardBus, CardFault, ProtoError};

use super::edf::{Pick, Want, pick, wants};
use super::layout::{ChunkIdx, ChunkPlan, ChunkRead, FileChunk, FileKey, StreamFile};
use super::registry::{CardEpoch, RowHold, StreamRegistry};
use super::ring::{CardSide, Claim, Rings, SLOTS};
use super::slot::SlotState;
use super::stats::StreamStats;
use super::units::{AudioBlocks, Generation};
use crate::hw::MAX_VOICES;

pub enum ServeStep<'r> {
    /// Read `claimed`'s chunk (`row.read()`); the row stays held until
    /// `row` drops.
    Read { claimed: Claim, row: HeldRead<'r> },
    /// Nothing to read: the bus goes back.
    Idle,
}

/// A held row and the claimed chunk's read on it, worked out once.
pub struct HeldRead<'r> {
    row: RowHold<'r>,
    plan: ChunkPlan,
    key: FileChunk,
}

impl HeldRead<'_> {
    /// The chunk where it lies, as `rot` names it.
    pub fn key(&self) -> FileChunk {
        self.key
    }

    /// Its chunk didn't de-frame: blamed by `rot`, its row marked when
    /// that condemns the file whole (the SOURCE cell says so).
    pub fn frame_error(&self, rot: &mut Rot) -> ReadError {
        self.judged(ReadError::Frame(rot.blame(self.key)))
    }

    /// Its read failed the protocol (`e`) at attempt `a`: as
    /// `ReadError::proto`, the row marked as for a frame error.
    pub fn proto_error(&self, e: ProtoError, a: Attempt, rot: &mut Rot) -> ReadError {
        self.judged(ReadError::proto(e, a, self.key, rot))
    }

    fn judged(&self, err: ReadError) -> ReadError {
        if err.blame() == Some(Blame::File) {
            self.row.mark_damaged();
        }
        err
    }

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

/// The servable wants the card can read: none `parked` (`serve_step`).
/// The UI's wake (`watch`) and the card IRQ's look ask this, so a want
/// that can never be read neither pends nor takes the bus.
pub fn readable<'a>(
    rings: &'a Rings,
    reg: &'a StreamRegistry,
    parked: &'a Parked,
) -> impl Iterator<Item = Want> + 'a {
    servable(rings, reg).filter(|w| !parked.holds(rings, reg, w))
}

/// The wants `serve_step` found it can't read (a chunk past its file's
/// runs, or one `Rot` passes over), each by its want's chunk and
/// generation, and the card they were judged on. Each is counted `unreadable` once, and
/// left out of `readable`, so a voice looping over one doesn't wake the
/// card IRQ to take and give the bus every UI pass. A want made since (a
/// new chunk or generation, which the audio's pend brings to `serve_step`) or
/// another card's (`CardEpoch`) is not parked. The card IRQ writes it;
/// the UI reads it.
pub struct Parked {
    /// The want parked at each voice's slot (`Parked::mark`); 0 none.
    slots: [[AtomicU32; SLOTS]; MAX_VOICES],
    epoch: AtomicU32,
}

// A mark's chunk, generation and set bit fit its word.
const _: () = assert!(ChunkIdx::MAX.get() < 1 << (31 - Generation::BITS));

impl Default for Parked {
    fn default() -> Self {
        Self::new()
    }
}

impl Parked {
    pub const fn new() -> Parked {
        Parked {
            slots: [const { [const { AtomicU32::new(0) }; SLOTS] }; MAX_VOICES],
            epoch: AtomicU32::new(0),
        }
    }

    /// Want `w` is parked: still Wanted for the chunk and under the
    /// generation it was parked with, on the same card.
    pub fn holds(&self, rings: &Rings, reg: &StreamRegistry, w: &Want) -> bool {
        let SlotState::Wanted {
            chunk, generation, ..
        } = rings.seen(w.voice, w.slot)
        else {
            return false;
        };
        self.epoch.load(Ordering::Acquire) == reg.epoch().bits()
            && self.slots[w.voice.index()][w.slot.index()].load(Ordering::Relaxed)
                == Self::mark(chunk, generation)
    }

    /// A want's chunk (24 bits) and generation (6), and a set bit: never 0.
    fn mark(chunk: ChunkIdx, generation: Generation) -> u32 {
        1 << 31 | chunk.get() << Generation::BITS | generation.bits()
    }

    /// Every slot judged again on `reg`'s card: a servable want whose
    /// chunk can't be read is parked, counted the first time; any other
    /// slot is not.
    fn renew(&self, rings: &Rings, reg: &StreamRegistry, rot: &Rot, stats: &StreamStats) {
        let epoch = reg.epoch().bits();
        if self.epoch.swap(epoch, Ordering::AcqRel) != epoch {
            self.slots
                .iter()
                .flatten()
                .for_each(|s| s.store(0, Ordering::Relaxed));
        }
        let mut unread = [[0; SLOTS]; MAX_VOICES];
        for w in servable(rings, reg) {
            let SlotState::Wanted {
                chunk, generation, ..
            } = rings.seen(w.voice, w.slot)
            else {
                continue;
            };
            let file = rings.source(w.voice).and_then(|id| reg.get(id));
            let key = file.and_then(|f| f.chunk_read(chunk)?.key());
            if !key.is_some_and(|k| !rot.passes_over(k)) {
                unread[w.voice.index()][w.slot.index()] = Self::mark(chunk, generation);
            }
        }
        for (slots, unread) in self.slots.iter().zip(unread) {
            for (slot, g) in slots.iter().zip(unread) {
                if slot.swap(g, Ordering::Relaxed) != g && g != 0 {
                    stats.unreadable.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// The next read: EDF over the readable wants (owner rule: any want
/// before a waiting UI, `edf::pick`, so the UI gets the bus only at
/// `Idle`). A want whose chunk its file can't read (past its runs, or
/// `rot` passes it over) is `parked`, left Wanted and counted
/// (`unreadable`) once; one cancelled since the pick, or whose row went
/// between the look and the hold, is passed over, requeued and counted.
/// `rot` forgets everything when the registry's card moves on
/// (`CardEpoch`). Never a fault (`on_error`): the file's, not the card's,
/// so a re-init can't mend it, and its voice alone starves, fading out,
/// and comes back past the chunk as tape time runs.
pub fn serve_step<'r>(
    card: &mut CardSide<'_>,
    reg: &'r StreamRegistry,
    now: AudioBlocks,
    rot: &mut Rot,
    parked: &Parked,
    stats: &StreamStats,
) -> ServeStep<'r> {
    rot.sync(reg.epoch());
    let rot = &*rot;
    let rings = card.rings();
    parked.renew(rings, reg, rot, stats);
    let mut passed = [[false; SLOTS]; MAX_VOICES];
    loop {
        let open =
            readable(rings, reg, parked).filter(|w| !passed[w.voice.index()][w.slot.index()]);
        let Pick::Fill(w) = pick(open, now) else {
            return ServeStep::Idle;
        };
        passed[w.voice.index()][w.slot.index()] = true;
        let Some(claimed) = card.claim(w.voice, w.slot) else {
            continue;
        };
        // After the claim: the source the want's note set (`Rings::source`).
        let held = rings.source(w.voice).and_then(|id| reg.hold(id));
        let plan = |f: &StreamFile| {
            let read = f.chunk_read(claimed.chunk())?;
            let key = read.key()?;
            (!rot.passes_over(key)).then(|| (read.plan(), key))
        };
        match held.and_then(|row| Some((plan(row.file())?, row))) {
            Some(((plan, key), row)) => {
                return ServeStep::Read {
                    claimed,
                    row: HeldRead { row, plan, key },
                };
            }
            None => {
                card.requeue(claimed);
                stats.unreadable.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Who a chunk that failed is blamed on (#609): one that won't de-frame,
/// or one whose read fails the protocol past its retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blame {
    /// Its first failure: the card may have sent the wrong blocks, or
    /// hiccuped, and a re-init may mend it.
    Card,
    /// It failed again after the re-init: the file's own bytes there, or
    /// the sector under them.
    Chunk,
    /// The file is condemned whole: `ROTTED_PER_FILE` of its chunks were.
    File,
}

/// Why a fill failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// A CRC16 or an error token at a piece's first read: read again once,
    /// nobody blamed yet.
    Again(ProtoError),
    /// The card's protocol past that: CMD18's R1, a token timeout, a
    /// second CRC16 or error token, blamed as `Rot::blame` says, so a bad
    /// sector faults the card once, not every pass.
    Proto(ProtoError, Blame),
    /// CRC-good blocks whose records don't de-frame (`layout::deframe`),
    /// blamed as `Rot::blame` says.
    Frame(Blame),
}

impl ReadError {
    /// Chunk `key`'s read failed the protocol (`e`) at attempt `a`: a CRC
    /// or a token is read again at once; any other failure, or that one
    /// again, is blamed by `rot`.
    pub fn proto(e: ProtoError, a: Attempt, key: FileChunk, rot: &mut Rot) -> ReadError {
        match (e, a) {
            (ProtoError::Crc | ProtoError::ErrorToken(_), Attempt::First) => ReadError::Again(e),
            _ => ReadError::Proto(e, rot.blame(key)),
        }
    }

    fn blame(self) -> Option<Blame> {
        match self {
            ReadError::Again(_) => None,
            ReadError::Proto(_, b) | ReadError::Frame(b) => Some(b),
        }
    }
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
    /// reading it drop out over it. Counted in `frame_errors` when it
    /// didn't de-frame.
    Rotted,
}

/// Which read of a fill's piece failed: its first, or the one retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attempt {
    First,
    Retry,
}

/// A first CRC16 mismatch or error token is read again (C1). Past that,
/// a failure blamed on the card faults it, so the UI re-inits it; one
/// blamed on the chunk or its file (it failed again after that re-init)
/// is passed over, no fault (#609). A frame error is never read again.
pub fn on_error(err: ReadError) -> ErrorStep {
    match err {
        ReadError::Again(_) => ErrorStep::Retry,
        ReadError::Proto(_, Blame::Card) => ErrorStep::Fault,
        ReadError::Frame(Blame::Card) => ErrorStep::FrameError,
        ReadError::Proto(_, Blame::Chunk | Blame::File)
        | ReadError::Frame(Blame::Chunk | Blame::File) => ErrorStep::Rotted,
    }
}

/// `on_error`'s step, taken: a fault set on `bus`, a frame error counted.
pub fn read_failed(err: ReadError, bus: &CardBus, stats: &StreamStats) -> ErrorStep {
    let step = on_error(err);
    if matches!(err, ReadError::Frame(_)) {
        stats.frame_errors.fetch_add(1, Ordering::Relaxed);
    }
    if matches!(step, ErrorStep::Fault | ErrorStep::FrameError) {
        bus.set_fault(CardFault::Read);
    }
    step
}

/// Fills failed in a row for a reason neither the protocol names nor
/// `Rot` can blame on a chunk (a card that stays busy, a recurring DMA
/// error, the watchdog's ask): at `FAULT` the card faults, so the UI
/// re-inits it and SOURCE says so, never a retry every UI pass for good
/// (m1, #761). A fill that lands starts it over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strikes(u8);

impl Strikes {
    pub const FAULT: u8 = 3;

    pub const fn new() -> Strikes {
        Strikes(0)
    }

    /// One more: true at the fault, the count starting over.
    pub fn strike(&mut self) -> bool {
        self.0 += 1;
        let out = self.0 >= Self::FAULT;
        if out {
            self.0 = 0;
        }
        out
    }
}

impl Default for Strikes {
    fn default() -> Self {
        Self::new()
    }
}

/// How many verdicts `Rot` keeps.
const MARKS: usize = 8;

/// Rotted chunks that condemn their whole file: real rot (a short copy, a
/// zeroed or cross-linked cluster) spans a cluster, 8 chunks or more.
pub const ROTTED_PER_FILE: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    /// Failed once: the card's fault, maybe. It never ages: the card
    /// interrupt reads nothing while faulted, so its next failure is
    /// always after a re-init.
    Suspect(FileChunk),
    /// Failed again after the re-init: the file's.
    Rotted(FileChunk),
    /// `ROTTED_PER_FILE` chunks rotted: every chunk of it passed over.
    File(FileKey),
}

impl Mark {
    fn file(self) -> FileKey {
        match self {
            Mark::Suspect(c) | Mark::Rotted(c) => c.file(),
            Mark::File(f) => f,
        }
    }

    /// Eviction order: the lowest goes first, the oldest among equals.
    fn worth(self) -> u8 {
        match self {
            Mark::Suspect(_) => 0,
            Mark::Rotted(_) => 1,
            Mark::File(_) => 2,
        }
    }
}

/// A verdict and when it was given.
#[derive(Clone, Copy, Debug)]
struct Entry {
    mark: Mark,
    at: u32,
}

/// The chunks that didn't de-frame, or whose reads failed past their
/// retry (#609), by where they lie. A first failure is suspected, so the
/// card faults and re-inits (it may have sent the wrong blocks, or
/// hiccuped); the same chunk failing again
/// after that is the file's, rotted, never read again; two rotted condemn
/// the file whole, its voices starving quietly and the card never blamed
/// for it again. A full table drops a suspect first, then the oldest
/// rotted chunk, a file's verdict last. A copy elsewhere is another key,
/// and another card (`CardEpoch`) clears it all. The card interrupt's
/// alone.
#[derive(Debug)]
pub struct Rot {
    marks: [Option<Entry>; MARKS],
    /// Verdicts given, wrapping: their order.
    clock: u32,
    epoch: CardEpoch,
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
            clock: 0,
            epoch: CardEpoch::FIRST,
        }
    }

    /// The registry's card is `e`: another one forgets everything.
    pub fn sync(&mut self, e: CardEpoch) {
        if super::STREAMS && e != self.epoch {
            *self = Rot {
                epoch: e,
                ..Rot::new()
            };
        }
    }

    /// Chunk `c` failed: the card's the first time, the file's once it
    /// failed before, the whole file's at `ROTTED_PER_FILE`.
    pub fn blame(&mut self, c: FileChunk) -> Blame {
        // Without TAPE nothing streams: no file to blame, and no code.
        if !super::STREAMS {
            return Blame::Card;
        }
        let f = c.file();
        if self.has(Mark::File(f)) {
            return Blame::File;
        }
        let Some(i) = self.marks.iter().position(|m| {
            m.is_some_and(|e| e.mark == Mark::Suspect(c) || e.mark == Mark::Rotted(c))
        }) else {
            self.put(Mark::Suspect(c));
            return Blame::Card;
        };
        self.marks[i] = None;
        self.put(Mark::Rotted(c));
        let rotted = self.marks.iter().flatten();
        let rotted = rotted
            .filter(|e| matches!(e.mark, Mark::Rotted(x) if x.file() == f))
            .count();
        if rotted < ROTTED_PER_FILE {
            return Blame::Chunk;
        }
        for m in &mut self.marks {
            if m.is_some_and(|e| e.mark.file() == f) {
                *m = None;
            }
        }
        self.put(Mark::File(f));
        Blame::File
    }

    /// Chunk `c` read and de-framed: a suspicion of it was the card's.
    pub fn cleared(&mut self, c: FileChunk) {
        if !super::STREAMS {
            return;
        }
        for m in &mut self.marks {
            if m.is_some_and(|e| e.mark == Mark::Suspect(c)) {
                *m = None;
            }
        }
    }

    /// Chunk `c` is the file's, or its file is condemned: passed over.
    pub fn passes_over(&self, c: FileChunk) -> bool {
        super::STREAMS && (self.has(Mark::Rotted(c)) || self.has(Mark::File(c.file())))
    }

    // Inline: a tail call to it the outliner shares lands in bank 1's
    // `.text`, which may not call bank 2 (`door-check`).
    #[inline(always)]
    fn has(&self, m: Mark) -> bool {
        self.marks.iter().flatten().any(|e| e.mark == m)
    }

    /// `m` in an empty slot, else over the least worth keeping.
    fn put(&mut self, mark: Mark) {
        let clock = self.clock;
        let slot = self.marks.iter_mut().min_by_key(|m| match m {
            None => (0, 0, 0),
            Some(e) => (1, e.mark.worth(), e.at.wrapping_sub(clock)),
        });
        if let Some(slot) = slot {
            *slot = Some(Entry { mark, at: clock });
        }
        self.clock = clock.wrapping_add(1);
    }
}
