//! Spans (slice 1 § 7.2): what a streamed note reads a block through. A
//! span is a run of source frames in RAM, a head in the arena or a Ready
//! ring slot (its guard included), read through a `SourceView` of just
//! those frames: ONE's and LOOP's loops read it as they read a row.
//! Bank 2's, with `TapeDeck` (`build.rs`' `11tape_engine4span` glob).

use super::desc::{Descriptor, HeadTable, MIN_SOURCE_FRAMES, Shape, head_table_of};
use super::read::SourceView;
use crate::stream::cursor::{Cursor, HeadIdx, Plan, RamHead, SpanSrc};
use crate::stream::layout::CHUNK_FRAMES;
use crate::stream::ring::{AudioTurn, GUARD_FRAMES, Ready};
use crate::stream::units::Frames;
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
    /// (`stream::cursor`).
    pub fn holds(&self, i: u32) -> bool {
        let v = &self.view;
        (v.first() == 0 || i > v.first())
            && (v.end() == v.frames() || i.saturating_add(2) < v.end())
    }
}

/// A streamed `Plain`'s row (slice 1 § 5.1): the descriptor, each head's
/// frames, then the heads' samples, over a file of `frames`.
#[derive(Clone, Copy)]
pub struct StreamRow<'a> {
    held: &'a [u8],
    table: HeadTable<'a>,
    desc: Descriptor,
    frames: u32,
    /// Strict views, for the tests: every tap checked against its span.
    #[cfg(any(test, feature = "test-support"))]
    strict: bool,
}

impl<'a> StreamRow<'a> {
    /// `held` as a streamed row of a file of `file` frames (even, as a
    /// whole row reads an odd one); `None` if it isn't one.
    pub fn new(held: &'a [u8], desc: Descriptor, file: Frames) -> Option<StreamRow<'a>> {
        let frames = file.get().checked_add(1)? & !1;
        let ok = desc.shape == Shape::Plain && desc.heads > 0 && frames >= MIN_SOURCE_FRAMES;
        let table = head_table_of(held, desc).filter(|_| ok)?;
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
        self.frames
    }

    /// Head `idx` as the cursor plays it: its frames, cut at the file's end.
    pub fn head(&self, idx: HeadIdx) -> Option<RamHead> {
        let (h, _) = self.table.get(idx.index())?;
        let (start, end) = (h.first(), h.end().min(self.frames));
        (start < end).then_some(RamHead { idx, start, end })
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
            return SourceView::strict_span(bytes, self.desc, first, self.frames)
                .map(|view| Span { view });
        }
        SourceView::span(bytes, self.desc, first, self.frames).map(|view| Span { view })
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
/// silence: the gain is 0 there), then LOOP's loop-start head, which a
/// fade's second head and a wrap read. A row held whole is one run that
/// never stops.
pub struct Reads<'a> {
    runs: [(SourceView<'a>, Stop); 2],
    /// The loop-start head (a whole row: the row).
    pub second: SourceView<'a>,
}

impl<'a> Reads<'a> {
    pub fn whole(v: SourceView<'a>) -> Reads<'a> {
        Reads {
            runs: [(v, Stop::Never); 2],
            second: v,
        }
    }

    /// The cursor's `plan` over `row`: its runs, then silence.
    pub fn streamed(
        plan: &Plan,
        row: &StreamRow<'a>,
        cursor: &Cursor,
        turn: &'a AudioTurn<'_>,
        v: VoiceIdx,
    ) -> Reads<'a> {
        let silent = row.silent();
        let mut runs = [(silent, Stop::Never); 2];
        for ((src, r), run) in plan.runs().iter().zip(runs.iter_mut()) {
            let span = match *src {
                SpanSrc::Head(h) => row.head_span(h),
                SpanSrc::Slot(s) => cursor.ready(turn, v, s).and_then(|r| row.slot_span(&r)),
            };
            *run = (span.map_or(silent, |s| s.view), Stop::At(r.end));
        }
        Reads {
            runs,
            second: row.head_span(HeadIdx::LOOP).map_or(silent, |s| s.view),
        }
    }

    /// Run `k`'s view and stop; past the runs, silence.
    pub fn run(&self, k: usize) -> (SourceView<'a>, Stop) {
        self.runs.get(k).copied().unwrap_or_else(|| {
            let v = &self.runs[0].0;
            (SourceView::silent(v.desc(), v.frames()), Stop::Never)
        })
    }
}
