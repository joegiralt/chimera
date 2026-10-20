//! Spans (slice 1 § 7.2): what a streamed note reads a block through. A
//! span is a run of source frames in RAM, a head in the arena or a Ready
//! ring slot (its guard included), read through a `SourceView` of just
//! those frames: ONE's and LOOP's loops read it as they read a row.
//! Bank 2's, with `TapeDeck` (`build.rs`' `11tape_engine4span` glob).

use core::num::NonZeroU32;

use super::desc::{Descriptor, HeadTable, MIN_SOURCE_FRAMES, head_table_of};
use super::read::SourceView;
use crate::stream::cursor::{self, Cursor, HeadIdx, Plan, RamHead, SpanSrc};
use crate::stream::layout::CHUNK_FRAMES;
use crate::stream::ring::{AudioTurn, GUARD_FRAMES, Ready};
use crate::stream::units::{Frames, SlotIdx, SourceFrame};
use crate::voice_alloc::VoiceIdx;

// A slot's span starts on a 12-bit pair: `SourceView::span` takes an even
// first frame only.
const _: () = assert!(CHUNK_FRAMES.get().is_multiple_of(2));
const _: () = assert!(GUARD_FRAMES.get().is_multiple_of(2));

/// Source frames a span holds, read as a row.
#[derive(Clone, Copy, Debug)]
pub struct Span<'a> {
    pub view: SourceView<'a>,
}

impl Span<'_> {
    /// A Hermite read at source frame `i` (taps `i − 1..=i + 2`) lies
    /// inside, or clamps only at the source's own ends: the cursor's rule
    /// (`cursor::holds`).
    pub fn holds(&self, i: u32) -> bool {
        let v = &self.view;
        let held = SourceFrame::new(v.first())..SourceFrame::new(v.end());
        cursor::holds(held, Frames::new(v.frames()), SourceFrame::new(i))
    }
}

/// A streamed `Plain`'s row (slice 1 § 5.1): the descriptor, each head's
/// frames, then the heads' samples, over a file of `frames`.
#[derive(Clone, Copy)]
pub struct StreamRow<'a> {
    held: &'a [u8],
    table: HeadTable<'a>,
    desc: Descriptor,
    frames: NonZeroU32,
    /// Strict views, for the tests: every tap checked against its span.
    #[cfg(any(test, feature = "test-support"))]
    strict: bool,
}

impl<'a> StreamRow<'a> {
    /// `held` as a streamed row of a file of `file` frames (even, as a
    /// whole row reads an odd one); `None` if it isn't one.
    pub fn new(held: &'a [u8], desc: Descriptor, file: Frames) -> Option<StreamRow<'a>> {
        let frames = file.get().checked_add(1)? & !1;
        let ok = desc.is_streamed() && frames >= MIN_SOURCE_FRAMES;
        let table = head_table_of(held, desc).filter(|_| ok)?;
        let frames = NonZeroU32::new(frames)?;
        Some(StreamRow {
            held,
            table,
            desc,
            frames,
            #[cfg(any(test, feature = "test-support"))]
            strict: false,
        })
    }

    /// Its spans' views check every tap.
    #[cfg(any(test, feature = "test-support"))]
    pub fn strict(self) -> StreamRow<'a> {
        StreamRow {
            strict: true,
            ..self
        }
    }

    pub fn desc(&self) -> Descriptor {
        self.desc
    }

    /// The file's frames, even.
    pub fn frames(&self) -> u32 {
        self.frames.get()
    }

    /// Head `idx` as the cursor plays it: its frames, cut at the file's end.
    pub fn head(&self, idx: HeadIdx) -> Option<RamHead> {
        let (h, _) = self.table.get(idx.index())?;
        let (start, end) = (h.first(), h.end().min(self.frames.get()));
        (start < end).then_some(RamHead {
            idx,
            start: SourceFrame::new(start),
            end: SourceFrame::new(end),
        })
    }

    /// Head `idx`'s span: its first frame is even (`HeadSpec`).
    pub fn head_span(&self, idx: HeadIdx) -> Option<Span<'a>> {
        let (h, at) = self.table.get(idx.index())?;
        // `head_table` checked `held` holds every head's samples.
        self.span(self.held.get(at..at + h.byte_len())?, h.first())
    }

    /// A Ready slot's span: its chunk, and its guard's frames before it.
    pub fn slot_span<'t>(&self, r: &Ready<'t>) -> Option<Span<'t>>
    where
        'a: 't,
    {
        let (rel, bytes) = r.packed();
        let start = r.chunk().get().checked_mul(CHUNK_FRAMES.get())?;
        self.span(bytes, start.checked_sub(rel.unsigned_abs())?)
    }

    fn span<'b>(&self, bytes: &'b [u8], first: u32) -> Option<Span<'b>> {
        #[cfg(any(test, feature = "test-support"))]
        if self.strict {
            return SourceView::strict_span(bytes, self.desc, first, self.frames.get())
                .map(|view| Span { view });
        }
        SourceView::span(bytes, self.desc, first, self.frames.get()).map(|view| Span { view })
    }

    /// Silence over the file: where nothing is held.
    pub fn silent(&self) -> SourceView<'static> {
        SourceView::silent(self.desc, self.frames)
    }
}

/// Where a run hands the head to the next: before a read, or never.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    At(u32),
    Never,
}

/// A block's reads: each run's view and where it stops (past the runs,
/// silence: the gain is 0 there), then what LOOP's next pass reads, which
/// a fade's second head and a wrap read: the loop-start head, or a
/// prefetched wrap's Ready slots, up to two (`second_at`). A row held
/// whole is one run that never stops.
pub struct Reads<'a> {
    runs: [(SourceView<'a>, Stop); 2],
    seconds: [(SourceView<'a>, Stop); 2],
    /// The next pass's reads are held, from their start: the second head
    /// reads no silence, but past `short`'s read if a slot it takes over
    /// to hasn't landed yet.
    pub landed: bool,
    pub short: Option<u32>,
}

impl<'a> Reads<'a> {
    pub fn whole(v: SourceView<'a>) -> Reads<'a> {
        Reads {
            runs: [(v, Stop::Never); 2],
            seconds: [(v, Stop::Never); 2],
            landed: true,
            short: None,
        }
    }

    /// The cursor's `plan` over `row`: its runs, then silence. The second
    /// head reads the loop-start head; `wrap` (from, to): what the next
    /// pass reads, from the loop-start head if it holds all of it, else
    /// from one or two Ready slots that do between them (`ready`: the
    /// cursor's), else silence.
    pub fn streamed(
        plan: &Plan,
        row: &StreamRow<'a>,
        cursor: &Cursor,
        turn: &'a AudioTurn<'_>,
        v: VoiceIdx,
        wrap: Option<(u32, u32)>,
    ) -> Reads<'a> {
        let silent = row.silent();
        let mut runs = [(silent, Stop::Never); 2];
        let slot = |s| cursor.ready(turn, v, s).and_then(|r| row.slot_span(&r));
        for ((src, r), run) in plan.runs().iter().zip(runs.iter_mut()) {
            let span = match *src {
                SpanSrc::Head(h) => row.head_span(h),
                SpanSrc::Slot(s) => slot(s),
            };
            *run = (span.map_or(silent, |s| s.view), Stop::At(r.end.get()));
        }
        let head = row.head_span(HeadIdx::Loop);
        let mut seconds = [(head.map_or(silent, |s| s.view), Stop::Never); 2];
        let (mut landed, mut short) = (true, None);
        if let Some((from, to)) = wrap {
            let holds = |s: &Span<'_>| s.holds(from) && s.holds(to);
            // The head the cursor plays may hold it (a loop near START).
            let other = cursor.head().and_then(|h| row.head_span(h.idx));
            if let Some(o) = other.filter(|o| holds(o) && !head.is_some_and(|h| holds(&h))) {
                seconds = [(o.view, Stop::Never); 2];
            } else if !head.is_some_and(|h| holds(&h)) {
                let ready = SlotIdx::ALL.map(slot);
                match Self::wrapped(ready, from, to, silent) {
                    Some((w, s)) => (seconds, short) = (w, s),
                    None => (seconds, landed) = ([(silent, Stop::Never); 2], false),
                }
            }
        }
        Reads {
            runs,
            seconds,
            landed,
            short,
        }
    }

    /// `from..=to` read from the spans `ready`: one holding both, or one
    /// holding `from` and one taking over before its last read holding
    /// `to`; that one not landed yet, silence past `a`'s last read (`short`).
    #[allow(clippy::type_complexity)]
    fn wrapped(
        ready: [Option<Span<'a>>; crate::stream::ring::SLOTS],
        from: u32,
        to: u32,
        silent: SourceView<'a>,
    ) -> Option<([(SourceView<'a>, Stop); 2], Option<u32>)> {
        let all = || ready.iter().flatten();
        let a = all()
            .filter(|s| s.holds(from))
            .max_by_key(|s| s.view.end())?;
        if a.holds(to) {
            return Some(([(a.view, Stop::Never); 2], None));
        }
        // `a` holds reads up to `end − 3`; `b` from its own `first + 1`.
        let a_last = a.view.end().saturating_sub(3);
        let b = all().find(|s| s.holds(a_last) && s.holds(to) && s.view.first() > a.view.first());
        let (b, short) = b.map_or((silent, Some(a_last)), |b| (b.view, None));
        Some(([(a.view, Stop::At(a_last)), (b, Stop::Never)], short))
    }

    /// The second head reads silence: a fade that started before its
    /// reads landed fades to silence whole, never stepping back in.
    pub fn mute_second(&mut self) {
        let silent = SourceView::silent(self.runs[0].0.desc(), self.runs[0].0.file_frames());
        self.seconds = [(silent, Stop::Never); 2];
        self.short = None;
    }

    /// The second head's view at read `h`, and where it stops.
    pub fn second_at(&self, h: u32) -> (SourceView<'a>, Stop) {
        match self.seconds[0] {
            (_, Stop::At(s)) if h >= s => self.seconds[1],
            x => x,
        }
    }

    /// Run `k`'s view and stop; past the runs, silence.
    pub fn run(&self, k: usize) -> (SourceView<'a>, Stop) {
        self.runs.get(k).copied().unwrap_or_else(|| {
            let v = &self.runs[0].0;
            (SourceView::silent(v.desc(), v.file_frames()), Stop::Never)
        })
    }
}
