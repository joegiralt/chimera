//! The bench's streams (slice 0's gate): up to 8 virtual heads, each over
//! its own bed on the card, every chunk checked against the bed's table,
//! stream 0 played aloud on the master output.
//!
//! The audio runs `block` and `fill_listen`, the card IRQ `check_failed`,
//! the UI `arm` and `report`: atomics only, so one static serves them all.

use core::mem::MaybeUninit;
use core::num::NonZeroU32;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use chimera_hal::units::StreamCount;

use super::layout::{CHUNK_BYTES, CHUNK_FRAMES, ChunkIdx, StreamFile};
use super::ring::{AudioTurn, Ready};
use super::slot::SlotState;
use super::units::{AudioBlocks, Frames, Ratio, SlotIdx, SourceRate};
use crate::hw::{BLOCK_SIZE, MAX_VOICES};
use crate::storage::Crc32;
use crate::voice_alloc::VoiceIdx;

/// A 1 MiB bed (`put`'s cap) is under 259 chunks.
pub const MAX_BED_CHUNKS: usize = 264;
const MAX_BEDS: usize = MAX_VOICES;

// A head in thousandths of a frame, a block past a whole bed, fits `u32`.
const _: () = assert!(
    (MAX_BED_CHUNKS as u64 * CHUNK_FRAMES.get() as u64 + 2 * 2_048) * 1_000 < u32::MAX as u64
);
// The fastest `Ratio` (32x at 48 kHz) moves a head under a chunk a block,
// so a head never skips a chunk.
const _: () = assert!(BLOCK_SIZE as u32 * 32 < CHUNK_FRAMES.get());

const MAGIC: [u8; 4] = *b"BEDS";

/// A bed's table: the `Crc32` of each de-framed chunk.
#[derive(Clone, Copy, Debug)]
pub struct BedCheck {
    stem: u32,
    /// The last chunk.
    chunks: ChunkIdx,
    crc: [u32; MAX_BED_CHUNKS],
}

impl BedCheck {
    /// One CRC a chunk; `None` for none or over `MAX_BED_CHUNKS`.
    pub fn new(stem: u32, crcs: &[u32]) -> Option<BedCheck> {
        let last = ChunkIdx::new(u32::try_from(crcs.len().checked_sub(1)?).ok()?)?;
        let mut crc = [0; MAX_BED_CHUNKS];
        crc.get_mut(..crcs.len())?.copy_from_slice(crcs);
        Some(BedCheck {
            stem,
            chunks: last,
            crc,
        })
    }

    /// The table of a bed's packed samples (the tool's, and the tests').
    pub fn of_packed(stem: u32, packed: &[u8]) -> Option<BedCheck> {
        let mut crcs = [0; MAX_BED_CHUNKS];
        let mut n = 0;
        for chunk in packed.chunks(CHUNK_BYTES.get() as usize) {
            *crcs.get_mut(n)? = crc32(chunk);
            n += 1;
        }
        BedCheck::new(stem, &crcs[..n])
    }

    pub fn stem(&self) -> u32 {
        self.stem
    }

    /// The last chunk.
    pub fn chunks(&self) -> ChunkIdx {
        self.chunks
    }

    /// `None` past `chunks`.
    pub fn crc(&self, c: ChunkIdx) -> Option<u32> {
        (c <= self.chunks)
            .then(|| self.crc.get(c.get() as usize).copied())
            .flatten()
    }

    /// `bytes` is chunk `c`, de-framed.
    pub fn verify(&self, c: ChunkIdx, bytes: &[u8]) -> bool {
        self.crc(c) == Some(crc32(bytes))
    }

    fn count(&self) -> usize {
        self.chunks.get() as usize + 1
    }
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(b);
    c.finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BedError {
    Magic,
    /// Over 8 beds.
    Count,
    /// A bed of no chunks, or over `MAX_BED_CHUNKS`.
    Chunks,
    Short,
}

/// `BEDS.CHK`: `b"BEDS"`, a count, then per bed its stem (`u32` LE), its
/// chunk count (`u16` LE) and a `Crc32` (LE) a chunk. The tool's, which
/// `encode`s it; the unit reads it through `BedTable`.
#[derive(Clone, Copy, Debug)]
pub struct BedChecks {
    checks: [BedCheck; MAX_BEDS],
    count: usize,
}

const NO_CHECK: BedCheck = BedCheck {
    stem: 0,
    chunks: ChunkIdx::MAX,
    crc: [0; MAX_BED_CHUNKS],
};

impl BedChecks {
    /// `None` over 8.
    pub fn new(checks: &[BedCheck]) -> Option<BedChecks> {
        let mut all = [NO_CHECK; MAX_BEDS];
        all.get_mut(..checks.len())?.copy_from_slice(checks);
        Some(BedChecks {
            checks: all,
            count: checks.len(),
        })
    }

    pub fn checks(&self) -> &[BedCheck] {
        &self.checks[..self.count]
    }

    /// The bytes written; 0, and nothing, if `out` is short.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        let len = 5 + self
            .checks()
            .iter()
            .map(|c| 6 + 4 * c.count())
            .sum::<usize>();
        let Some(out) = out.get_mut(..len) else {
            return 0;
        };
        let mut w = out.iter_mut();
        let mut put = |b: &[u8]| b.iter().zip(&mut w).for_each(|(&b, o)| *o = b);
        put(&MAGIC);
        // At most 8 (`new`).
        put(&[self.count as u8]);
        for c in self.checks() {
            put(&c.stem.to_le_bytes());
            // At most `MAX_BED_CHUNKS`.
            put(&(c.count() as u16).to_le_bytes());
            for crc in &c.crc[..c.count()] {
                put(&crc.to_le_bytes());
            }
        }
        len
    }
}

/// A `BEDS.CHK` in place, checked once by `parse`: each bed's table is
/// copied straight into its `BedCheck`, so no table of all eight (8.5 KiB)
/// is ever built.
#[derive(Clone, Copy, Debug)]
pub struct BedTable<'a> {
    beds: &'a [u8],
    count: usize,
}

impl<'a> BedTable<'a> {
    /// The magic, the count, each bed's chunks and the length checked.
    pub fn parse(b: &'a [u8]) -> Result<BedTable<'a>, BedError> {
        let mut r = Reader(b);
        if r.take(4)? != MAGIC {
            return Err(BedError::Magic);
        }
        let count = usize::from(r.take(1)?[0]);
        if count > MAX_BEDS {
            return Err(BedError::Count);
        }
        let beds = r.0;
        for _ in 0..count {
            r.entry()?;
        }
        Ok(BedTable { beds, count })
    }

    /// Beds in the table.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Bed `k`'s stem and CRC bytes; `None` past `count`.
    fn entry(&self, k: usize) -> Option<(u32, &'a [u8])> {
        if k >= self.count {
            return None;
        }
        let mut r = Reader(self.beds);
        for _ in 0..k {
            r.entry().ok()?;
        }
        r.entry().ok()
    }

    pub fn stem(&self, k: usize) -> Option<u32> {
        self.entry(k).map(|(stem, _)| stem)
    }

    /// Bed `k`'s table into `out`; false past `count`.
    pub fn check_into(&self, k: usize, out: &mut BedCheck) -> bool {
        let Some((stem, crcs)) = self.entry(k) else {
            return false;
        };
        let n = crcs.len() / 4;
        // `parse`: 1..=MAX_BED_CHUNKS.
        let Some(last) = u32::try_from(n - 1).ok().and_then(ChunkIdx::new) else {
            return false;
        };
        out.stem = stem;
        out.chunks = last;
        for (c, b) in out.crc.iter_mut().zip(crcs.as_chunks::<4>().0) {
            *c = u32::from_le_bytes(*b);
        }
        true
    }

    /// Bed `k` with `file`, built in `slot`; false (`slot` left as it was)
    /// past `count`, or if the table doesn't cover the file's chunks.
    pub fn place(&self, k: usize, file: StreamFile, slot: &mut Option<Bed>) -> bool {
        let fits = self
            .entry(k)
            .is_some_and(|(_, crcs)| crcs.len() / 4 == file.last_chunk().get() as usize + 1);
        if !fits || file.frames().get() == 0 {
            return false;
        }
        let bed = slot.insert(Bed {
            file,
            check: NO_CHECK,
        });
        self.check_into(k, &mut bed.check)
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BedError> {
        let (head, rest) = self.0.split_at_checked(n).ok_or(BedError::Short)?;
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, BedError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// One bed: its stem and its CRCs' bytes.
    fn entry(&mut self) -> Result<(u32, &'a [u8]), BedError> {
        let stem = self.u32()?;
        let n = self.take(2)?;
        let n = usize::from(u16::from_le_bytes([n[0], n[1]]));
        if n == 0 || n > MAX_BED_CHUNKS {
            return Err(BedError::Chunks);
        }
        Ok((stem, self.take(4 * n)?))
    }
}

/// A bed on the card and its table, which covers exactly its chunks.
#[derive(Clone, Copy, Debug)]
pub struct Bed {
    file: StreamFile,
    check: BedCheck,
}

impl Bed {
    pub fn new(file: StreamFile, check: BedCheck) -> Option<Bed> {
        (check.chunks == file.last_chunk() && file.frames().get() > 0)
            .then_some(Bed { file, check })
    }

    pub fn file(&self) -> &StreamFile {
        &self.file
    }

    pub fn check(&self) -> &BedCheck {
        &self.check
    }
}

/// Bed k is stream k's.
pub struct BenchBeds {
    pub beds: [Option<Bed>; MAX_BEDS],
}

impl Default for BenchBeds {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchBeds {
    pub const fn new() -> BenchBeds {
        BenchBeds {
            beds: [None; MAX_BEDS],
        }
    }

    /// No beds, built where it stays (10 KiB: never by value on the
    /// firmware's stack).
    pub fn init_in_place(slot: &mut MaybeUninit<BenchBeds>) -> &mut BenchBeds {
        let p = slot.as_mut_ptr();
        for i in 0..MAX_BEDS {
            // SAFETY: `p` is the slot's, valid for writes; `beds` is the one
            // field, so once each bed is written the value is whole.
            unsafe { addr_of_mut!((*p).beds[i]).write(None) };
        }
        // SAFETY: every field written above.
        unsafe { slot.assume_init_mut() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listen {
    Off,
    Stream0,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmError {
    /// Fewer beds than streams.
    Beds,
}

/// Whether a block made a new want: the audio then pends the card IRQ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    New,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchReport {
    /// Heads that entered a chunk not Ready.
    pub empties: u32,
    /// The most blocks a head waited on a chunk past its deadline.
    pub late_max_blocks: u32,
    /// Blocks run since the arm.
    pub blocks: u32,
    /// Chunks Ready with another chunk than the head's.
    pub order_errors: u32,
    /// Fills whose bytes failed their bed's table, a stream.
    pub check_errors: [u32; MAX_BEDS],
}

/// One virtual head over its bed.
struct Head {
    /// Thousandths of a frame into the bed: where this block plays from.
    at: AtomicU32,
    /// The bed's frames, for the listen path's wrap.
    frames: AtomicU32,
    /// The slot holding the head's chunk.
    cur: AtomicU8,
    primed: AtomicBool,
    /// The head entered a chunk that wasn't Ready yet.
    waiting: AtomicBool,
    /// Each slot's deadline, as wanted.
    due: [AtomicU32; SlotIdx::ALL.len()],
}

impl Head {
    const fn new() -> Head {
        Head {
            at: AtomicU32::new(0),
            frames: AtomicU32::new(1),
            cur: AtomicU8::new(0),
            primed: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            due: [const { AtomicU32::new(0) }; SlotIdx::ALL.len()],
        }
    }

    fn cur(&self) -> SlotIdx {
        match self.cur.load(Ordering::Relaxed) {
            1 => SlotIdx::B,
            2 => SlotIdx::C,
            _ => SlotIdx::A,
        }
    }

    fn set_cur(&self, s: SlotIdx) {
        self.cur.store(s.index() as u8, Ordering::Relaxed);
    }
}

pub struct BenchStreams {
    /// `StreamCount`'s bits: 0 is disarmed.
    n: AtomicU8,
    fpb_x1000: AtomicU32,
    listen: AtomicBool,
    burst: AtomicBool,
    heads: [Head; MAX_BEDS],
    empties: AtomicU32,
    late_max: AtomicU32,
    blocks: AtomicU32,
    order_errors: AtomicU32,
    check_errors: [AtomicU32; MAX_BEDS],
}

pub static BENCH: BenchStreams = BenchStreams::new();

impl Default for BenchStreams {
    fn default() -> Self {
        Self::new()
    }
}

/// One block's clock, for every head.
#[derive(Clone, Copy)]
struct Tick {
    /// Thousandths of a frame a block.
    fpb: NonZeroU32,
    now: AudioBlocks,
    burst: bool,
}

/// A slot freed: Ready released, anything else (a want, a fill in flight)
/// cancelled.
fn free(turn: &mut AudioTurn<'_>, v: VoiceIdx, s: SlotIdx) {
    if matches!(turn.seen(v, s), SlotState::Ready { .. }) {
        turn.release(v, s);
    } else {
        turn.cancel(v, s);
    }
}

fn ready<'t>(turn: &'t AudioTurn<'_>, v: VoiceIdx, s: SlotIdx) -> Option<Ready<'t>> {
    match turn.seen(v, s) {
        SlotState::Ready { generation, .. } => turn.ready(v, s, generation),
        _ => None,
    }
}

impl BenchStreams {
    pub const fn new() -> BenchStreams {
        BenchStreams {
            n: AtomicU8::new(0),
            fpb_x1000: AtomicU32::new(1),
            listen: AtomicBool::new(false),
            burst: AtomicBool::new(false),
            heads: [const { Head::new() }; MAX_BEDS],
            empties: AtomicU32::new(0),
            late_max: AtomicU32::new(0),
            blocks: AtomicU32::new(0),
            order_errors: AtomicU32::new(0),
            check_errors: [const { AtomicU32::new(0) }; MAX_BEDS],
        }
    }

    /// The UI's: `n` heads from frame 0, the counts zeroed. The first
    /// block of each primes it (its slots cancelled, chunks 0 to 2 wanted).
    pub fn arm(
        &self,
        beds: &BenchBeds,
        rate: SourceRate,
        n: StreamCount,
        speed: Ratio,
        listen: Listen,
    ) -> Result<(), ArmError> {
        let k = usize::from(n.get());
        if beds.beds[..k].iter().any(Option::is_none) {
            return Err(ArmError::Beds);
        }
        // Off while the rest is written: a block that preempts does nothing.
        self.n.store(0, Ordering::Release);
        for (h, b) in self.heads.iter().zip(&beds.beds) {
            h.primed.store(false, Ordering::Relaxed);
            let frames = b.as_ref().map_or(1, |b| b.file.frames().get());
            h.frames.store(frames, Ordering::Relaxed);
        }
        for c in [
            &self.empties,
            &self.late_max,
            &self.blocks,
            &self.order_errors,
        ] {
            c.store(0, Ordering::Relaxed);
        }
        for c in &self.check_errors {
            c.store(0, Ordering::Relaxed);
        }
        let fpb = speed.frames_per_block_x1000(rate).get();
        self.fpb_x1000.store(fpb, Ordering::Relaxed);
        self.listen
            .store(listen == Listen::Stream0, Ordering::Relaxed);
        self.burst.store(false, Ordering::Relaxed);
        self.n.store(n.get(), Ordering::Release);
        Ok(())
    }

    pub fn disarm(&self) {
        self.n.store(0, Ordering::Release);
    }

    /// The next block frees and re-wants every armed stream's chunk ahead
    /// at once (B3).
    pub fn queue_burst(&self) {
        self.burst.store(true, Ordering::Relaxed);
    }

    /// Armed with `Listen::Stream0`: `fill_listen` replaces the synth.
    pub fn listening(&self) -> bool {
        self.n.load(Ordering::Acquire) != 0 && self.listen.load(Ordering::Relaxed)
    }

    /// The card IRQ's: a fill of `stream` failed its bed's table. The fill
    /// is published all the same.
    pub fn check_failed(&self, stream: VoiceIdx) {
        self.check_errors[stream.index()].fetch_add(1, Ordering::Relaxed);
    }

    pub fn report(&self) -> BenchReport {
        let get = |c: &AtomicU32| c.load(Ordering::Relaxed);
        BenchReport {
            empties: get(&self.empties),
            late_max_blocks: get(&self.late_max),
            blocks: get(&self.blocks),
            order_errors: get(&self.order_errors),
            check_errors: self.check_errors.each_ref().map(get),
        }
    }

    /// The audio's, once a block: every armed head over its bed.
    pub fn block(&self, turn: &mut AudioTurn<'_>, beds: &BenchBeds, now: AudioBlocks) -> Asked {
        let Some(n) = StreamCount::from_bits(self.n.load(Ordering::Acquire)) else {
            return Asked::Nothing;
        };
        let Some(fpb) = NonZeroU32::new(self.fpb_x1000.load(Ordering::Relaxed)) else {
            return Asked::Nothing;
        };
        let tick = Tick {
            fpb,
            now,
            burst: self.burst.swap(false, Ordering::Relaxed),
        };
        self.blocks.fetch_add(1, Ordering::Relaxed);
        let mut asked = Asked::Nothing;
        let streams = VoiceIdx::ALL.into_iter().zip(&self.heads).zip(&beds.beds);
        for ((v, head), bed) in streams.take(usize::from(n.get())) {
            if let Some(bed) = bed
                && self.step(turn, v, head, &bed.file, &tick)
            {
                asked = Asked::New;
            }
        }
        asked
    }

    /// One head's block: advance, free the slot it passed, then want what
    /// its slots lack. True if it wanted.
    fn step(
        &self,
        turn: &mut AudioTurn<'_>,
        v: VoiceIdx,
        head: &Head,
        file: &StreamFile,
        tick: &Tick,
    ) -> bool {
        let Tick { fpb, now, burst } = *tick;
        // `Bed::new` and the const asserts above: none of this overflows.
        let total = file.frames().get() * 1_000;
        if !head.primed.swap(true, Ordering::Relaxed) {
            for s in SlotIdx::ALL {
                turn.cancel(v, s);
            }
            head.at.store(0, Ordering::Relaxed);
            head.set_cur(SlotIdx::A);
            head.waiting.store(false, Ordering::Relaxed);
        } else {
            let was = head.at.load(Ordering::Relaxed);
            let at = (was + fpb.get()) % total;
            head.at.store(at, Ordering::Relaxed);
            let old = file.chunk_of(Frames::new(was / 1_000));
            let new = file.chunk_of(Frames::new(at / 1_000));
            if new != old {
                let passed = head.cur();
                free(turn, v, passed);
                head.set_cur(passed.next());
                let waiting = match turn.seen(v, passed.next()) {
                    SlotState::Ready { chunk, .. } if Some(chunk) == new => false,
                    SlotState::Ready { .. } => {
                        self.order_errors.fetch_add(1, Ordering::Relaxed);
                        false
                    }
                    _ => {
                        self.empties.fetch_add(1, Ordering::Relaxed);
                        true
                    }
                };
                head.waiting.store(waiting, Ordering::Relaxed);
            }
        }
        let cur = head.cur();
        if burst {
            free(turn, v, cur.next());
        }
        if head.waiting.load(Ordering::Relaxed) {
            let due = AudioBlocks::new(head.due[cur.index()].load(Ordering::Relaxed));
            if let Ok(late) = u32::try_from(due.until(now)) {
                self.late_max.fetch_max(late, Ordering::Relaxed);
            }
            if matches!(turn.seen(v, cur), SlotState::Ready { .. }) {
                head.waiting.store(false, Ordering::Relaxed);
            }
        }
        let at = head.at.load(Ordering::Relaxed);
        let Some(here) = file.chunk_of(Frames::new(at / 1_000)) else {
            return false;
        };
        let after = |c: ChunkIdx| {
            if c == file.last_chunk() {
                ChunkIdx::new(0)
            } else {
                c.next()
            }
        };
        let end = file.chunk_frames(here).map_or(0, |r| r.end.get() * 1_000);
        let ahead = end.saturating_sub(at) / fpb;
        let mut asked = want(turn, v, head, cur, here, now);
        let Some(next) = after(here) else {
            return asked;
        };
        asked |= want(turn, v, head, cur.next(), next, now.after(ahead));
        if let Some(then) = after(next) {
            // Due once `next` has played: a short last chunk is due sooner.
            let len = file
                .chunk_frames(next)
                .map_or(0, |r| (r.end.get() - r.start.get()) * 1_000);
            let due = now.after(ahead.saturating_add(len / fpb));
            asked |= want(turn, v, head, cur.next().next(), then, due);
        }
        asked
    }

    /// Stream 0's block, at −6 dBFS on both sides of `out` (interleaved
    /// pairs, the master's words), upsampled linearly from its rate to
    /// 48 kHz. A frame whose chunk isn't Ready plays as 0. Untouched unless
    /// `listening`.
    pub fn fill_listen(&self, turn: &AudioTurn<'_>, out: &mut [i32]) {
        if !self.listening() {
            return;
        }
        let v = VoiceIdx::ALL[0];
        let head = &self.heads[0];
        let slots = SlotIdx::ALL.map(|s| ready(turn, v, s));
        let frames = head.frames.load(Ordering::Relaxed).max(1);
        let frame = |f: u32| {
            let f = f % frames;
            let (c, i) = (f / CHUNK_FRAMES.get(), f % CHUNK_FRAMES.get());
            slots
                .iter()
                .flatten()
                .find(|r| r.chunk().get() == c)
                // Under `CHUNK_FRAMES`: an `i32`.
                .and_then(|r| r.sample(i as i32))
                .map_or(0, i64::from)
        };
        let at = u64::from(head.at.load(Ordering::Relaxed));
        let fpb = u64::from(self.fpb_x1000.load(Ordering::Relaxed));
        for (i, pair) in (0u64..).zip(out.as_chunks_mut::<2>().0) {
            let p = at + i * fpb / BLOCK_SIZE as u64;
            // Under a bed and a block of thousandths: `u32` (asserted above).
            let (f, frac) = ((p / 1_000) as u32, (p % 1_000) as i64);
            let s = frame(f) * (1_000 - frac) + frame(f + 1) * frac;
            // 12 bits to the word's 32, less one for −6 dBFS: under 2^31.
            let w = (s << 19) / 1_000;
            pair.fill(w as i32);
        }
    }
}

/// An Empty slot wanted with chunk `c` by `due`.
fn want(
    turn: &AudioTurn<'_>,
    v: VoiceIdx,
    head: &Head,
    s: SlotIdx,
    c: ChunkIdx,
    due: AudioBlocks,
) -> bool {
    if turn.seen(v, s) != SlotState::Empty {
        return false;
    }
    head.due[s.index()].store(due.get(), Ordering::Relaxed);
    turn.want(v, s, c, due).is_some()
}
