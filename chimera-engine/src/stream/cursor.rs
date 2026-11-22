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

use chimera_hal::BLOCK_SIZE;
use chimera_hal::card_bus::{CardBus, CardFault, Quiet};

use super::admit::{ReadRatio, STREAM_BUDGET_BPS, bill};
use super::layout::{CHUNK_FRAMES, ChunkIdx};
use super::port::StreamPort;
use super::ring::{AudioTurn, GUARD_FRAMES, Guard, Ready, SLOTS};
use super::slot::SlotState;
use super::units::{
    AudioBlocks, Frames, Generation, Inc, OutSamples, SlotIdx, SourceFrame, SourceRate,
};
use crate::project::SourceId;
use crate::voice_alloc::VoiceIdx;

/// A drop-out's fade, out and back in.
pub const DROP_FADE: OutSamples = OutSamples::new(144);
/// Quiet's fade out.
pub const QUIET_FADE: OutSamples = OutSamples::new(480);
/// The fastest read planned for, source frames an output sample.
const MAX_INC: f32 = Inc::MAX.get();
// No admitted voice reads faster than the cursor plans for: the fastest
// source at 1 frame a sample is the dearest bill a ratio scales.
const _: () = assert!(
    STREAM_BUDGET_BPS
        .get()
        .div_ceil(bill(SourceRate::MAX, ReadRatio::ONE).get())
        < MAX_INC as u32
);

const CF: u32 = CHUNK_FRAMES.get();
const BLOCK: u32 = BLOCK_SIZE as u32;
/// Taps after a read's own frame.
const AFTER: u32 = 2;
const AFTER_F: Frames = Frames::new(AFTER);
/// Full gain, in steps both fades divide exactly, so each lands on 0
/// and 1 with no residue.
const FULL: i16 = 1_440;
const INV_FULL: f32 = 1.0 / FULL as f32;
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

/// The deck's own fade (`Cursor::duck`): out before a jump no span
/// holds, or back in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duck {
    Out,
    In,
}

/// What a block's tend does with the chunks wanted: asks the missing, or
/// keeps only those Ready (under a quiet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tend {
    Ask,
    ReadyOnly,
}

/// Which of a source's heads: START's, or the loop's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadIdx {
    Start,
    Loop,
}

impl HeadIdx {
    /// Its entry in the head table.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A head in RAM: source frames `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RamHead {
    pub idx: HeadIdx,
    pub start: SourceFrame,
    pub end: SourceFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Head(HeadIdx),
    Ring(SlotIdx),
    /// Out of held frames: silent, tape time running on.
    Starved,
    /// A write's quiet or a card fault: fading to silence, no wants, tape
    /// time running on; cold once it ends.
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
        end_frame: SourceFrame,
    },
    In,
}

/// A block's reads and its fade. It holds no bytes: the deck reads a
/// slot's through `Cursor::ready`, under the block's turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    runs: [(SpanSrc, Range<SourceFrame>); 2],
    n: u8,
    pub fade: FadeCmd,
    /// The note's fastest read: the deck caps each sample's step at it.
    pub inc_max: Inc,
}

impl Plan {
    const NONE: (SpanSrc, Range<SourceFrame>) = (
        SpanSrc::Head(HeadIdx::Start),
        SourceFrame::ZERO..SourceFrame::ZERO,
    );

    fn silent(fade: FadeCmd, inc_max: Inc) -> Plan {
        Plan {
            runs: [Self::NONE; 2],
            n: 0,
            fade,
            inc_max,
        }
    }

    /// The walk's first two legs, to the block's last read or the last
    /// held, whichever is first.
    fn read(&mut self, w: &Walk, i_last: SourceFrame) {
        let end = w.held.min_at(i_last).saturating_add(Frames::new(1));
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
    pub fn runs(&self) -> &[(SpanSrc, Range<SourceFrame>)] {
        self.runs.get(..usize::from(self.n)).unwrap_or(&[])
    }
}

/// Where a span's reads end: at a frame, or held to the file's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpanEnd {
    At(SourceFrame),
    FileEnd,
}

impl SpanEnd {
    fn covers(self, i: SourceFrame) -> bool {
        match self {
            SpanEnd::At(last) => i <= last,
            SpanEnd::FileEnd => true,
        }
    }

    /// The last read held, or `i` if that's sooner.
    fn min_at(self, i: SourceFrame) -> SourceFrame {
        match self {
            SpanEnd::At(last) => last.min(i),
            SpanEnd::FileEnd => i,
        }
    }

    /// The first read after; `SourceFrame::MAX` held to the file's end.
    fn after(self) -> SourceFrame {
        match self {
            SpanEnd::At(last) => last.saturating_add(Frames::new(1)),
            SpanEnd::FileEnd => SourceFrame::MAX,
        }
    }
}

/// The reads frames `held` of a file of `file` take, `first..=last`: from
/// the frame after its first (a read takes the frame before), or from 0
/// at the file's start; to `AFTER` short of its end (the taps after),
/// or every read on at the file's end, where the view clamps. `None` if
/// it can end no read.
pub fn reads(held: Range<SourceFrame>, file: Frames) -> Option<(SourceFrame, SpanEnd)> {
    let first = match held.start.get() {
        0 => SourceFrame::ZERO,
        f => SourceFrame::new(f.saturating_add(1)),
    };
    let last = if held.end.get() >= file.get() {
        SpanEnd::FileEnd
    } else {
        SpanEnd::At(SourceFrame::new(held.end.get().checked_sub(AFTER + 1)?))
    };
    Some((first, last))
}

/// A Hermite read at `i` (taps `i - 1..=i + 2`) lies inside `held`, or
/// clamps only at the file's own ends: the one rule the cursor plans by
/// and a span reads by.
pub fn holds(held: Range<SourceFrame>, file: Frames, i: SourceFrame) -> bool {
    reads(held, file).is_some_and(|(first, last)| first <= i && last.covers(i))
}

/// The read positions a span holds, `first..=last`.
#[derive(Clone, Copy)]
struct Span {
    src: SpanSrc,
    first: SourceFrame,
    last: SpanEnd,
}

impl Span {
    fn new(src: SpanSrc, held: Range<SourceFrame>, file: Frames) -> Option<Span> {
        let (first, last) = reads(held, file)?;
        Some(Span { src, first, last })
    }

    fn holds(&self, i: SourceFrame) -> bool {
        self.first <= i && self.last.covers(i)
    }
}

const SPANS: usize = SLOTS + 1;

/// Span to span from a read: legs, each a span and the read it takes over
/// at, and the last read held.
struct Walk {
    legs: [(SpanSrc, SourceFrame); SPANS],
    n: usize,
    held: SpanEnd,
}

/// From `i0`: a Ready slot holding it (the one reaching furthest), else
/// the head; then, at each span's end, the span that takes over earliest
/// and reaches further. Taking over early keeps a span entered mid-block
/// a whole chunk long, so a block needs two runs at most. `None` if
/// nothing holds `i0`.
fn walk(spans: &[Option<Span>; SPANS], i0: SourceFrame) -> Option<Walk> {
    let all = || spans.iter().flatten();
    let mut cur = *all()
        .filter(|s| s.holds(i0))
        .max_by_key(|s| (matches!(s.src, SpanSrc::Slot(_)), s.last))?;
    let mut legs = [(cur.src, i0); SPANS];
    let mut n = 1;
    while let Some(&from) = legs.get(n - 1).filter(|_| n < SPANS) {
        let Some(next) = all()
            .filter(|s| s.first > from.1 && s.first <= cur.last.after() && s.last > cur.last)
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

fn last_chunk(file: Frames) -> Option<ChunkIdx> {
    file.get()
        .checked_sub(1)
        .map(|f| ChunkIdx::of(SourceFrame::new(f)))
}

fn head_span(h: RamHead, file: Frames) -> Option<Span> {
    Span::new(SpanSrc::Head(h.idx), h.start..h.end, file)
}

/// Chunk `c` in slot `s`, its guard ahead of it if Guarded; the first
/// chunk and the last clamp at the file's ends.
fn slot_span(s: SlotIdx, c: ChunkIdx, g: Guard, file: Frames) -> Option<Span> {
    let guard = match g {
        Guard::Guarded => GUARD_FRAMES,
        Guard::Unguarded => Frames::new(0),
    };
    let start = c.first_frame();
    let held = start.saturating_sub(guard)..start.saturating_add(CHUNK_FRAMES);
    Span::new(SpanSrc::Slot(s), held, file)
}

fn flow_of(src: SpanSrc) -> Flow {
    match src {
        SpanSrc::Head(i) => Flow::Head(i),
        SpanSrc::Slot(s) => Flow::Ring(s),
    }
}

/// A write's quiet or a card fault: the fade's step down (a fault's over
/// `DROP_FADE`, the quiet's over `QUIET_FADE`); `None` reading freely.
fn hush(bus: &CardBus) -> Option<i16> {
    match (bus.fault(), bus.quiet()) {
        (CardFault::Read, _) => Some(DROP_STEP),
        (CardFault::Clear, Quiet::Asked | Quiet::Held) => Some(QUIET_STEP),
        (CardFault::Clear, Quiet::Off) => None,
    }
}

/// A LOOP wrap known a pass ahead (#698): this pass's reads end at `last`
/// (the read at `fade` starts the fade), and the next pass's fade and
/// first block read `from..=to`. The cursor wants no chunk past this
/// pass's end, and the next pass's start chunks in the slots that frees,
/// so the wrap lands warm: REEL's "known a step ahead" (spec § 5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wrap {
    pub last: SourceFrame,
    pub fade: SourceFrame,
    pub from: SourceFrame,
    pub to: SourceFrame,
}

/// What a note streams: its source, whose registry row the card reads,
/// and its file's frames, both latched at its start (#594).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streamed {
    pub id: SourceId,
    pub frames: Frames,
}

#[derive(Clone, Copy)]
pub struct Cursor {
    /// Each slot's want: a Ready slot is read only under it.
    wants: [Generation; SLOTS],
    /// The note's stream; `None` before the first start.
    src: Option<Streamed>,
    head: Option<RamHead>,
    gain: Fade,
    state: Flow,
    /// The note's fastest read: every plan and deadline uses it, so a
    /// fade planned in one block holds through the next.
    inc: Inc,
    /// The wrap the next plans prefetch for (`expect`).
    wrap: Option<Wrap>,
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
            src: None,
            head: None,
            gain: Fade::SILENT,
            state: Flow::Cold,
            inc: Inc::MIN,
            wrap: None,
        }
    }

    /// The wrap the next plans prefetch for; `None`: the read runs on.
    pub fn expect(&mut self, wrap: Option<Wrap>) {
        self.wrap = wrap;
    }

    /// The Ready slot holding read `pos`, reaching furthest.
    fn ring_at(&self, pos: SourceFrame, port: &StreamPort<'_, '_>) -> Option<SlotIdx> {
        let spans = self.spans(port);
        let s = spans[..SLOTS]
            .iter()
            .flatten()
            .filter(|s| s.holds(pos))
            .max_by_key(|s| s.last)?;
        match s.src {
            SpanSrc::Slot(i) => Some(i),
            SpanSrc::Head(_) => None,
        }
    }

    pub fn flow(&self) -> Flow {
        self.state
    }

    /// The head it plays or has yet to pass; `None` once passed.
    pub fn head(&self) -> Option<RamHead> {
        self.head
    }

    /// The deck's own fade, out before a jump no span holds, or back in:
    /// over `DROP_FADE`, while it plays a span (a drop-out, a cold start or
    /// quiet runs its own).
    pub fn duck(&mut self, d: Duck) {
        if matches!(self.state, Flow::Head(_) | Flow::Ring(_)) {
            self.gain.step = match d {
                Duck::Out => -DROP_STEP,
                Duck::In => DROP_STEP,
            };
        }
    }

    /// The note is over: every slot dropped, silent until the next start.
    pub fn stop(&mut self, port: &mut StreamPort<'_, '_>) {
        port.turn.cancel_all(port.voice);
        (self.head, self.state, self.gain) = (None, Flow::Cold, Fade::SILENT);
    }

    /// Note-on at source frame `pos` of `src`, reading at most `inc_max`
    /// source frames a sample until the next start: drops the last note's
    /// slots, names `src` on the ring, plays from `head` if it holds `pos`
    /// (else starts cold; silent under a quiet), and asks every slot.
    pub fn start(
        &mut self,
        pos: SourceFrame,
        inc_max: Inc,
        head: Option<RamHead>,
        src: Streamed,
        port: &mut StreamPort<'_, '_>,
    ) {
        port.turn.cancel_all(port.voice);
        port.turn.stream(port.voice, src.id);
        self.src = Some(src);
        self.wrap = None;
        match head.filter(|&h| head_span(h, src.frames).is_some_and(|s| s.holds(pos))) {
            Some(h) => {
                self.head = Some(h);
                self.state = Flow::Head(h.idx);
                // Under a write's quiet or a fault, silent until it ends.
                self.gain = match hush(port.bus) {
                    None => Fade::OPEN,
                    Some(_) => Fade::SILENT,
                };
            }
            None => {
                self.cold(port);
                self.gain = Fade::SILENT;
            }
        }
        self.inc = inc_max;
        self.tend(pos, Tend::Ask, port);
    }

    /// A jump mid-note (a wrap, heads re-staged) to `pos`: `start` on the
    /// note's stream, but a warm landing keeps the gain where it was and
    /// rises over `DROP_FADE`, so a voice that ducked before it never
    /// steps, nor one under a quiet snaps to 0. Nothing before a start.
    pub fn jump(
        &mut self,
        pos: SourceFrame,
        inc_max: Inc,
        head: Option<RamHead>,
        port: &mut StreamPort<'_, '_>,
    ) {
        let Some(src) = self.src else {
            return;
        };
        let level = self.gain.level;
        // A prefetched wrap (`Wrap`) lands on its Ready slot: warm, its
        // slots kept.
        let on_head = head.is_some_and(|h| head_span(h, src.frames).is_some_and(|s| s.holds(pos)));
        if !on_head && let Some(s) = self.ring_at(pos, port) {
            (self.head, self.state, self.inc) = (None, Flow::Ring(s), inc_max);
            self.wrap = None;
            let step = if level < FULL { DROP_STEP } else { 0 };
            self.gain = Fade { level, step };
            self.tend(pos, Tend::Ask, port);
            return;
        }
        self.start(pos, inc_max, head, src, port);
        // Its level kept, under a quiet too (`plan` fades it from there).
        if matches!(self.state, Flow::Head(_)) {
            let step = if level < FULL { DROP_STEP } else { 0 };
            self.gain = Fade { level, step };
        } else if level > 0 {
            port.stats.cold_loud.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The note may read faster from here (a glide, #585): never slower.
    pub fn raise(&mut self, inc_max: Inc) {
        if inc_max > self.inc {
            self.inc = inc_max;
        }
    }

    /// Once a block, before the model renders from `pos`.
    pub fn plan(&mut self, pos: f64, port: &mut StreamPort<'_, '_>) -> Plan {
        let inc = self.inc.get();
        // `as` saturates (NaN to 0): no read past `u32`.
        let i0 = SourceFrame::new(pos as u32);
        let i_last = SourceFrame::new((pos + f64::from(inc * BLOCK as f32)) as u32);
        let ahead = (self.inc * DROP_FADE).get() + 1;
        let mut look = i_last.saturating_add(Frames::new(ahead));
        // The pass reads no further than its wrap.
        if let Some(w) = self.wrap {
            look = look.min(w.last);
        }

        let hushed = hush(port.bus);
        match (hushed, self.state == Flow::Quiet) {
            // A fault under a quiet hurries it to `DROP_FADE`.
            (Some(step), _) => {
                self.state = Flow::Quiet;
                self.gain.step = self.gain.step.min(-step);
            }
            // Released: on from the head, as a start there would, if tape
            // time is still inside it, rising over `DROP_FADE` (#608).
            (None, true) => {
                let file = self.file();
                match self
                    .head
                    .filter(|&h| head_span(h, file).is_some_and(|s| s.holds(i0)))
                {
                    Some(h) => {
                        self.state = Flow::Head(h.idx);
                        self.gain.step = DROP_STEP;
                    }
                    None => self.cold(port),
                }
            }
            _ => {}
        }
        let quiet = hushed.is_some();
        if quiet && self.gain.level > 0 {
            port.loud.set(true);
        }
        let file = self.file();
        // A head that holds no read is passed too.
        let passed = |h| head_span(h, file).is_none_or(|s| s.last < SpanEnd::At(i0));
        if self.head.is_some_and(passed) {
            self.head = None;
        }
        if self.state == Flow::Quiet && self.gain.level <= 0 {
            port.turn.cancel_all(port.voice);
            return Plan::silent(FadeCmd::Hold, self.inc);
        }
        let t = if self.state == Flow::Quiet {
            Tend::ReadyOnly
        } else {
            Tend::Ask
        };
        self.tend(i0, t, port);

        let spans = self.spans(port);
        let walk = walk(&spans, i0);
        let held = walk.as_ref().map(|w| w.held);
        let ok = held.is_some_and(|h| h >= SpanEnd::At(look));
        // Not `ok`, the walk ends before the file's.
        let end_frame = held.map_or(i0, |h| h.min_at(SourceFrame::MAX));
        if walk.is_none() && self.gain.level > 0 {
            // Audible with nothing to read: a jump that skipped `start`.
            port.stats.unplanned.fetch_add(1, Ordering::Relaxed);
        }
        let mut plan = Plan::silent(FadeCmd::Hold, self.inc);
        if let Some(w) = &walk {
            plan.read(w, i_last);
        }
        plan.fade = match (self.state, ok) {
            (Flow::Head(_) | Flow::Ring(_) | Flow::Quiet, true) => FadeCmd::Hold,
            (Flow::Head(_) | Flow::Ring(_), false) => {
                port.stats.underruns.fetch_add(1, Ordering::Relaxed);
                self.state = Flow::Starved;
                self.gain.step = -DROP_STEP;
                FadeCmd::Out { end_frame }
            }
            (Flow::Starved | Flow::Cold, true) => {
                self.gain.step = DROP_STEP;
                FadeCmd::In
            }
            (Flow::Starved | Flow::Cold | Flow::Quiet, false) => {
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
        f32::from(f.level) * INV_FULL
    }

    /// `out` under the gain, a sample at a time; at rest (full, or silent
    /// and falling) one multiply a sample, or none at full.
    pub fn apply(&mut self, out: &mut [f32]) {
        let f = self.gain;
        let resting = (f.level == FULL && f.step >= 0) || (f.level == 0 && f.step <= 0);
        if !resting {
            for y in out.iter_mut() {
                *y *= self.gain();
            }
        } else if f.level == 0 {
            out.fill(0.0);
        }
    }

    /// The note's file's frames; none before a start.
    fn file(&self) -> Frames {
        self.src.map_or(Frames::new(0), |s| s.frames)
    }

    fn cold(&mut self, port: &StreamPort<'_, '_>) {
        self.head = None;
        self.state = Flow::Cold;
        port.stats.cold.fetch_add(1, Ordering::Relaxed);
    }

    /// The first chunk wanted: the one under the read (the last, past
    /// END), or with a head, the one holding the head's last read (so a
    /// slot takes over in time); `None` if the head reaches the end.
    fn base(&self, i0: SourceFrame, file: Frames, last: ChunkIdx) -> Option<ChunkIdx> {
        let under = ChunkIdx::of(i0.saturating_sub(Frames::new(1))).min(last);
        match self.head.map(|h| head_span(h, file).map(|s| s.last)) {
            Some(Some(SpanEnd::FileEnd)) => None,
            Some(Some(SpanEnd::At(l))) => Some(under.max(ChunkIdx::of(l))),
            Some(None) | None => Some(under),
        }
    }

    /// Drops every slot outside the chunks wanted (released if Ready, else
    /// cancelled, so a late fill lands nowhere) and, `Tend::Ask`, wants the
    /// missing ones into the free slots. `Tend::ReadyOnly`, every want goes.
    fn tend(&mut self, i0: SourceFrame, t: Tend, port: &mut StreamPort<'_, '_>) {
        let asking = t == Tend::Ask;
        let file = self.file();
        let Some(last) = last_chunk(file) else {
            return;
        };
        let wanted = self.wanted(i0, file, last);
        let v = port.voice;
        let mut have = [false; SLOTS];
        for s in SlotIdx::ALL {
            let mine = |g: Generation| g == self.wants[s.index()];
            let (chunk, ready) = match port.turn.seen(v, s) {
                SlotState::Empty => continue,
                SlotState::Ready { chunk, generation } if mine(generation) => (chunk, true),
                SlotState::Wanted {
                    chunk, generation, ..
                }
                | SlotState::Filling { chunk, generation }
                    if mine(generation) =>
                {
                    (chunk, false)
                }
                _ => {
                    port.turn.cancel(v, s);
                    continue;
                }
            };
            let at = wanted.iter().position(|&w| w == Some(chunk));
            match (at.filter(|_| ready || asking), ready) {
                (Some(k), _) => have[k] = true,
                (None, true) => port.turn.release(v, s),
                (None, false) => port.turn.cancel(v, s),
            }
        }
        if !asking {
            return;
        }
        for (c, _) in wanted
            .iter()
            .zip(have)
            .filter_map(|(&c, h)| Some((c?, h)))
            .filter(|&(_, h)| !h)
        {
            let Some(s) = SlotIdx::ALL
                .into_iter()
                .find(|&s| port.turn.seen(v, s) == SlotState::Empty)
            else {
                return;
            };
            let by = self.deadline(c, i0, port);
            if let Some(g) = port.turn.want(v, s, c, by) {
                self.wants[s.index()] = g;
                (port.pend)();
            }
        }
    }

    /// The chunks wanted, in order, a slot each: from `base` on, and with
    /// a `Wrap`, none past the pass's end but the next pass's start in the
    /// slots that leaves.
    fn wanted(&self, i0: SourceFrame, file: Frames, last: ChunkIdx) -> [Option<ChunkIdx>; SLOTS] {
        let base = self.base(i0, file, last);
        let mut top = base.map(|b| b.saturating_add(SLOTS as u32 - 1).min(last));
        let mut next = None;
        if let Some(w) = self.wrap {
            let end = ChunkIdx::of(w.last.saturating_add(AFTER_F));
            top = top.map(|t| t.min(end));
            let lo = ChunkIdx::of(w.from.saturating_sub(Frames::new(1))).min(last);
            let hi = ChunkIdx::of(w.to.saturating_add(AFTER_F)).min(last);
            next = Some(lo.through(hi));
        }
        let mut out = [None; SLOTS];
        let mut n = 0;
        let linear = base
            .zip(top)
            .filter(|(b, t)| b <= t)
            .map(|(b, t)| b.through(t))
            .into_iter()
            .flatten();
        for c in linear.chain(next.into_iter().flatten()) {
            if n < SLOTS && !out[..n].contains(&Some(c)) {
                out[n] = Some(c);
                n += 1;
            }
        }
        out
    }

    /// `now` plus the blocks until the read needs chunk `c`: at its start,
    /// or once the head runs out.
    fn deadline(&self, c: ChunkIdx, i0: SourceFrame, port: &StreamPort<'_, '_>) -> AudioBlocks {
        let mut need = c.first_frame();
        if let Some(s) = self.head.and_then(|h| head_span(h, self.file())) {
            need = need.max(s.last.after());
        }
        // A wrap's chunk is read from its fade's start on.
        if let Some(w) = self
            .wrap
            .filter(|w| c < ChunkIdx::of(i0) || c > ChunkIdx::of(w.last.saturating_add(AFTER_F)))
        {
            need = w.fade;
        }
        let ahead = (need - i0).get() as f32;
        let blocks = (ahead / (self.inc.get() * BLOCK as f32).max(1.0)) as u32;
        port.now.after(blocks.min(MAX_AHEAD))
    }

    /// The head, and every slot Ready under this cursor's want.
    fn spans(&self, port: &StreamPort<'_, '_>) -> [Option<Span>; SPANS] {
        let mut out = [None; SPANS];
        let file = self.file();
        for (o, s) in out.iter_mut().zip(SlotIdx::ALL) {
            *o = port
                .turn
                .ready(port.voice, s, self.wants[s.index()])
                .and_then(|r| slot_span(s, r.chunk(), r.guard(), file));
        }
        out[SLOTS] = self.head.and_then(|h| head_span(h, file));
        out
    }
}
