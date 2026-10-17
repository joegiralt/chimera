//! The cursor (slice 1 § 7.3): a streamed voice's audio side. Once a
//! block it tends the ring (releases what the read passed, asks the
//! chunks ahead), plans which spans the block reads, the RAM head or the
//! ring's Ready slots, and fades out before a read would pass the frames
//! held, back in once they land.
//!
//! Positions are read positions: a Hermite read at `i` takes the taps
//! `i - 1..=i + 2`, and a span holds `i` only with all four.
//!
//! The read moves forward, at most `Plan::inc_max` a sample, from the
//! `pos` each block's plan was given. Every other move of the read goes
//! through `start`: a jump is a new start, cold, or on a head when one
//! holds the target.

use core::ops::Range;
use core::sync::atomic::Ordering;

use chimera_hal::card_bus::Quiet;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use super::admit::STREAM_BUDGET_BPS;
use super::layout::{CHUNK_BYTES, CHUNK_FRAMES, ChunkIdx, RAW_CHUNK_BYTES};
use super::port::StreamPort;
use super::ring::{AudioTurn, GUARD_FRAMES, Guard, Ready, SLOTS};
use super::slot::SlotState;
use super::units::{AudioBlocks, Frames, Generation, SlotIdx};
use crate::voice_alloc::VoiceIdx;

/// A drop-out's fade, out and back in, in output samples.
pub const DROP_FADE: Frames = Frames::new(144);
/// Quiet's fade out, in output samples.
pub const QUIET_FADE: Frames = Frames::new(480);
/// The fastest read planned for, source frames an output sample.
pub const MAX_INC: f32 = 16.0;
/// Admission's bill (`admit::bill`) for a read at 1 frame a sample: 1.5 B
/// a frame, 9 raw blocks a chunk.
const BILL_AT_ONE: u64 =
    SAMPLE_RATE as u64 * 3 * RAW_CHUNK_BYTES as u64 / (2 * CHUNK_BYTES.get() as u64);
// No admitted voice reads faster than the cursor plans for.
const _: () = assert!((STREAM_BUDGET_BPS as u64).div_ceil(BILL_AT_ONE) < MAX_INC as u64);

const CF: u32 = CHUNK_FRAMES.get();
const BLOCK: u32 = BLOCK_SIZE as u32;
/// Taps after a read's own frame.
const AFTER: u32 = 2;
/// Full gain, in steps both fades divide exactly, so each lands on 0
/// and 1 with no residue.
const FULL: i16 = 1_440;
const DROP_STEP: i16 = FULL / DROP_FADE.get() as i16;
const QUIET_STEP: i16 = FULL / QUIET_FADE.get() as i16;
const _: () = assert!(DROP_STEP * DROP_FADE.get() as i16 == FULL);
const _: () = assert!(QUIET_STEP * QUIET_FADE.get() as i16 == FULL);
/// The furthest deadline asked, in blocks.
const MAX_AHEAD: u32 = 1 << 16;
const TRAVEL_MAX: u32 = MAX_INC as u32 * BLOCK + 1;
// Two runs a block: a span taken over mid-block holds from its first
// read to the end of its chunk, `CF - 3` reads, more than a block's.
const _: () = assert!(TRAVEL_MAX < CF - 3);
// A block and a drop fade ahead stay inside the chunks always asked:
// the one under the read and the two after, `2 x CF - 3` past it.
const _: () = assert!(TRAVEL_MAX + MAX_INC as u32 * DROP_FADE.get() < 2 * CF - 3);

/// Which of a source's heads: START's, or the loop's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadIdx(u8);

impl HeadIdx {
    pub const START: HeadIdx = HeadIdx(0);
    pub const LOOP: HeadIdx = HeadIdx(1);

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A head in RAM: source frames `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RamHead {
    pub idx: HeadIdx,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Head(HeadIdx),
    Ring(SlotIdx),
    /// Out of held frames: silent, tape time running on.
    Starved {
        since: AudioBlocks,
    },
    Quiet,
    /// Silent until the chunk under the read lands.
    Cold,
}

/// The gain on the model's output, in `FULL`ths, ramped a sample at a
/// time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fade {
    level: i16,
    step: i16,
}

impl Fade {
    const SILENT: Fade = Fade { level: 0, step: 0 };
    const OPEN: Fade = Fade {
        level: FULL,
        step: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanSrc {
    Head(HeadIdx),
    Slot(SlotIdx),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeCmd {
    Hold,
    /// Falling, silent by the read at `end_frame`, the last held.
    Out {
        end_frame: u32,
    },
    In,
}

/// A block's reads and its fade. It holds no bytes: the deck reads a
/// slot's through `Cursor::ready`, under the block's turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    runs: [(SpanSrc, Range<u32>); 2],
    n: u8,
    pub fade: FadeCmd,
    /// The note's fastest read: the deck caps each sample's step at it.
    pub inc_max: f32,
}

impl Plan {
    const NONE: (SpanSrc, Range<u32>) = (SpanSrc::Head(HeadIdx::START), 0..0);

    fn silent(fade: FadeCmd, inc_max: f32) -> Plan {
        Plan {
            runs: [Self::NONE; 2],
            n: 0,
            fade,
            inc_max,
        }
    }

    /// The walk's first two legs, to the block's last read or the last
    /// held, whichever is first.
    fn read(&mut self, w: &Walk, i_last: u32) {
        let end = w.held.min(i_last).saturating_add(1);
        for (k, run) in self.runs.iter_mut().enumerate().take(w.n) {
            let Some(&(src, at)) = w.legs.get(k).filter(|l| l.1 < end) else {
                break;
            };
            let to = w
                .legs
                .get(k + 1)
                .filter(|_| k + 1 < w.n)
                .map_or(end, |l| l.1.min(end));
            *run = (src, at..to);
            self.n += 1;
        }
    }

    /// The block's reads, in order: a span each, and the read positions
    /// it takes. Past the last, the voice reads nothing (its gain is 0).
    pub fn runs(&self) -> &[(SpanSrc, Range<u32>)] {
        self.runs.get(..usize::from(self.n)).unwrap_or(&[])
    }
}

/// The read positions a span holds, `first..=last`.
#[derive(Clone, Copy)]
struct Span {
    src: SpanSrc,
    first: u32,
    last: u32,
}

impl Span {
    fn holds(&self, i: u32) -> bool {
        self.first <= i && i <= self.last
    }
}

const SPANS: usize = SLOTS + 1;

/// Span to span from a read: legs, each a span and the read it takes over
/// at, and the last read held.
struct Walk {
    legs: [(SpanSrc, u32); SPANS],
    n: usize,
    held: u32,
}

/// From `i0`: a Ready slot holding it (the one reaching furthest), else
/// the head; then, at each span's end, the span that takes over earliest
/// and reaches further. Taking over early keeps a span entered mid-block
/// a whole chunk long, so a block needs two runs at most. `None` if
/// nothing holds `i0`.
fn walk(spans: &[Option<Span>; SPANS], i0: u32) -> Option<Walk> {
    let all = || spans.iter().flatten();
    let mut cur = *all()
        .filter(|s| s.holds(i0))
        .max_by_key(|s| (matches!(s.src, SpanSrc::Slot(_)), s.last))?;
    let mut legs = [(cur.src, i0); SPANS];
    let mut n = 1;
    while let Some(&from) = legs.get(n - 1).filter(|_| n < SPANS) {
        let Some(next) = all()
            .filter(|s| {
                s.first > from.1 && s.first <= cur.last.saturating_add(1) && s.last > cur.last
            })
            .min_by_key(|s| s.first)
        else {
            break;
        };
        if let Some(leg) = legs.get_mut(n) {
            *leg = (next.src, next.first);
        }
        n += 1;
        cur = *next;
    }
    Some(Walk {
        legs,
        n,
        held: cur.last,
    })
}

fn chunk_of(i: u32) -> u32 {
    i / CF
}

fn last_chunk(file: Frames) -> Option<u32> {
    file.get().checked_sub(1).map(chunk_of)
}

/// A head holds from its second frame (a read takes the frame before),
/// or from 0 at the file's start; one reaching the file's end holds every
/// read after it (the view clamps at both ends).
fn head_span(h: RamHead, file: Frames) -> Span {
    let last = if h.end >= file.get() {
        u32::MAX
    } else {
        h.end.saturating_sub(AFTER + 1)
    };
    let first = if h.start == 0 {
        0
    } else {
        h.start.saturating_add(1)
    };
    Span {
        src: SpanSrc::Head(h.idx),
        first,
        last,
    }
}

/// Chunk `c` in slot `s`, its guard ahead of it if Guarded; the first
/// chunk and the last clamp at the file's ends.
fn slot_span(s: SlotIdx, c: ChunkIdx, g: Guard, last: u32) -> Span {
    let c = c.get();
    let guard = match g {
        Guard::Guarded => GUARD_FRAMES.get(),
        Guard::Unguarded => 0,
    };
    let start = c.saturating_mul(CF);
    let first = if c == 0 {
        0
    } else {
        start.saturating_sub(guard).saturating_add(1)
    };
    let last = if c >= last {
        u32::MAX
    } else {
        start.saturating_add(CF - AFTER - 1)
    };
    Span {
        src: SpanSrc::Slot(s),
        first,
        last,
    }
}

fn flow_of(src: SpanSrc) -> Flow {
    match src {
        SpanSrc::Head(i) => Flow::Head(i),
        SpanSrc::Slot(s) => Flow::Ring(s),
    }
}

/// `inc_max`, finite and inside `0..=MAX_INC`.
fn clamp_inc(inc_max: f32) -> f32 {
    if inc_max > 0.0 {
        inc_max.min(MAX_INC)
    } else {
        0.0
    }
}

#[derive(Clone, Copy)]
pub struct Cursor {
    /// Each slot's want: a Ready slot is read only under it.
    wants: [Generation; SLOTS],
    head: Option<RamHead>,
    gain: Fade,
    state: Flow,
    /// The note's fastest read, clamped: every plan and deadline uses it,
    /// so a fade planned in one block holds through the next.
    inc: f32,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursor {
    pub const fn new() -> Cursor {
        Cursor {
            wants: [Generation::ZERO; SLOTS],
            head: None,
            gain: Fade::SILENT,
            state: Flow::Cold,
            inc: 0.0,
        }
    }

    pub fn flow(&self) -> Flow {
        self.state
    }

    /// The head it plays or has yet to pass; `None` once passed.
    pub fn head(&self) -> Option<RamHead> {
        self.head
    }

    /// The note is over: every slot dropped, silent until the next start.
    pub fn stop(&mut self, port: &mut StreamPort<'_, '_>) {
        for s in SlotIdx::ALL {
            port.turn.cancel(port.voice, s);
        }
        (self.head, self.state, self.gain) = (None, Flow::Cold, Fade::SILENT);
    }

    /// Note-on (or any jump) at source frame `pos`, reading at most
    /// `inc_max` source frames a sample until the next start: drops the
    /// last note's slots, plays from `head` if it holds `pos` (else starts
    /// cold), and asks every slot.
    pub fn start(
        &mut self,
        pos: u32,
        inc_max: f32,
        head: Option<RamHead>,
        port: &mut StreamPort<'_, '_>,
    ) {
        for s in SlotIdx::ALL {
            port.turn.cancel(port.voice, s);
        }
        match head.filter(|&h| head_span(h, port.file_frames).holds(pos)) {
            Some(h) => {
                self.head = Some(h);
                self.state = Flow::Head(h.idx);
                self.gain = Fade::OPEN;
            }
            None => {
                self.cold(port);
                self.gain = Fade::SILENT;
            }
        }
        self.inc = clamp_inc(inc_max);
        self.tend(pos, true, port);
    }

    /// Once a block, before the model renders from `pos`.
    pub fn plan(&mut self, pos: f64, port: &mut StreamPort<'_, '_>) -> Plan {
        let inc = self.inc;
        // `as` saturates (NaN to 0): no read past `u32`.
        let i0 = pos as u32;
        let i_last = (pos + f64::from(inc * BLOCK as f32)) as u32;
        let look = i_last.saturating_add((inc * DROP_FADE.get() as f32) as u32 + 1);

        let quiet = port.bus.quiet() != Quiet::Off;
        match (quiet, self.state == Flow::Quiet) {
            (true, false) => {
                self.state = Flow::Quiet;
                self.gain.step = self.gain.step.min(-QUIET_STEP);
            }
            (false, true) => self.cold(port),
            _ => {}
        }
        let file = port.file_frames;
        if self.head.is_some_and(|h| head_span(h, file).last < i0) {
            self.head = None;
        }
        if self.state == Flow::Quiet && self.gain.level <= 0 {
            for s in SlotIdx::ALL {
                port.turn.cancel(port.voice, s);
            }
            return Plan::silent(FadeCmd::Hold, inc);
        }
        self.tend(i0, self.state != Flow::Quiet, port);

        let spans = self.spans(port);
        let walk = walk(&spans, i0);
        let held = walk.as_ref().map(|w| w.held);
        let ok = held.is_some_and(|h| h >= look);
        let end_frame = held.unwrap_or(i0);
        if walk.is_none() && self.gain.level > 0 {
            // Audible with nothing to read: a jump that skipped `start`.
            port.stats.faults.fetch_add(1, Ordering::Relaxed);
        }
        let mut plan = Plan::silent(FadeCmd::Hold, inc);
        if let Some(w) = &walk {
            plan.read(w, i_last);
        }
        plan.fade = match (self.state, ok) {
            (Flow::Head(_) | Flow::Ring(_) | Flow::Quiet, true) => FadeCmd::Hold,
            (Flow::Head(_) | Flow::Ring(_), false) => {
                port.stats.underruns.fetch_add(1, Ordering::Relaxed);
                self.state = Flow::Starved { since: port.now };
                self.gain.step = -DROP_STEP;
                FadeCmd::Out { end_frame }
            }
            (Flow::Starved { .. } | Flow::Cold, true) => {
                self.gain.step = DROP_STEP;
                FadeCmd::In
            }
            (Flow::Starved { .. } | Flow::Cold | Flow::Quiet, false) => {
                self.gain.step = self.gain.step.min(-DROP_STEP);
                if self.gain.level > 0 {
                    FadeCmd::Out { end_frame }
                } else {
                    FadeCmd::Hold
                }
            }
        };
        if ok
            && self.state != Flow::Quiet
            && let Some((src, _)) = plan.runs().last()
        {
            self.state = flow_of(*src);
        }
        // Staying silent: nothing to read.
        if self.gain.level <= 0 && self.gain.step <= 0 {
            plan.n = 0;
        }
        plan
    }

    /// Slot `s`'s bytes, if Ready under this cursor's want: what a
    /// `SpanSrc::Slot` run reads, for the block `turn` is.
    pub fn ready<'t>(&self, turn: &'t AudioTurn<'_>, v: VoiceIdx, s: SlotIdx) -> Option<Ready<'t>> {
        turn.ready(v, s, self.wants[s.index()])
    }

    /// The gain for the next sample.
    pub fn gain(&mut self) -> f32 {
        let f = &mut self.gain;
        f.level = f.level.saturating_add(f.step).clamp(0, FULL);
        f32::from(f.level) / f32::from(FULL)
    }

    fn cold(&mut self, port: &StreamPort<'_, '_>) {
        self.head = None;
        self.state = Flow::Cold;
        port.stats.cold.fetch_add(1, Ordering::Relaxed);
    }

    /// The first chunk wanted: the one under the read (the last, past
    /// END), or with a head, the one holding the head's last read (so a
    /// slot takes over in time); past the last chunk if the head reaches
    /// the end.
    fn base(&self, i0: u32, file: Frames, last: u32) -> u32 {
        let under = chunk_of(i0.saturating_sub(1)).min(last);
        match self.head.map(|h| head_span(h, file).last) {
            Some(u32::MAX) => u32::MAX,
            Some(last) => under.max(chunk_of(last)),
            None => under,
        }
    }

    /// Drops every slot outside the chunks wanted (released if Ready, else
    /// cancelled, so a late fill lands nowhere) and, `asking`, wants the
    /// missing ones into the free slots. Not `asking`, every want goes.
    fn tend(&mut self, i0: u32, asking: bool, port: &mut StreamPort<'_, '_>) {
        let Some(last) = last_chunk(port.file_frames) else {
            return;
        };
        let base = self.base(i0, port.file_frames, last);
        let top = base.saturating_add(SLOTS as u32 - 1).min(last);
        let v = port.voice;
        let mut have = [false; SLOTS];
        for s in SlotIdx::ALL {
            let mine = |g: Generation| g == self.wants[s.index()];
            let (chunk, ready) = match port.turn.seen(v, s) {
                SlotState::Empty => continue,
                SlotState::Ready { chunk, generation } if mine(generation) => (chunk.get(), true),
                SlotState::Wanted {
                    chunk, generation, ..
                }
                | SlotState::Filling { chunk, generation }
                    if mine(generation) =>
                {
                    (chunk.get(), false)
                }
                _ => {
                    port.turn.cancel(v, s);
                    continue;
                }
            };
            let keep = (base..=top).contains(&chunk) && (ready || asking);
            match (keep, ready) {
                (true, _) => {
                    if let Some(h) = have.get_mut((chunk - base) as usize) {
                        *h = true;
                    }
                }
                (false, true) => port.turn.release(v, s),
                (false, false) => port.turn.cancel(v, s),
            }
        }
        if !asking || base > top {
            return;
        }
        for (c, _) in (base..=top).zip(have).filter(|&(_, h)| !h) {
            let Some(s) = SlotIdx::ALL
                .into_iter()
                .find(|&s| port.turn.seen(v, s) == SlotState::Empty)
            else {
                return;
            };
            let Some(chunk) = ChunkIdx::new(c) else {
                return;
            };
            let by = self.deadline(c, i0, port);
            if let Some(g) = port.turn.want(v, s, chunk, by) {
                self.wants[s.index()] = g;
                (port.pend)();
            }
        }
    }

    /// `now` plus the blocks until the read needs chunk `c`: at its start,
    /// or once the head runs out.
    fn deadline(&self, c: u32, i0: u32, port: &StreamPort<'_, '_>) -> AudioBlocks {
        let mut need = c.saturating_mul(CF);
        if let Some(h) = self.head {
            need = need.max(head_span(h, port.file_frames).last.saturating_add(1));
        }
        let ahead = need.saturating_sub(i0) as f32;
        let blocks = (ahead / (self.inc * BLOCK as f32).max(1.0)) as u32;
        port.now.after(blocks.min(MAX_AHEAD))
    }

    /// The head, and every slot Ready under this cursor's want.
    fn spans(&self, port: &StreamPort<'_, '_>) -> [Option<Span>; SPANS] {
        let mut out = [None; SPANS];
        let last = last_chunk(port.file_frames).unwrap_or(0);
        for (o, s) in out.iter_mut().zip(SlotIdx::ALL) {
            *o = port
                .turn
                .ready(port.voice, s, self.wants[s.index()])
                .map(|r| slot_span(s, r.chunk(), r.guard(), last));
        }
        out[SLOTS] = self.head.map(|h| head_span(h, port.file_frames));
        out
    }
}
