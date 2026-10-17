//! Where a streamed source's heads go (slice 1 § 5.6): START's, and in
//! LOOP the loop's. Each starts a frame before its first read, made even
//! (a read takes the frame before: `stream::cursor`), and ends just past
//! the chunk boundary after its reach, so the first chunk it asks for
//! holds most of a chunk past it and a chord's second chunks aren't due
//! behind its first (#583). The loop's starts XFADE early and holds XFADE
//! and `HEAD_MS` (#584). Bank 1's: the UI's load places them.

use super::admit::head_frames;
use super::layout::CHUNK_FRAMES;
use crate::dsp::tape_engine::heads::{Bounds, window_end, window_start};
use crate::dsp::tape_engine::params::{PlayMode, TapeEngineParams, needs_ram};
use crate::dsp::tape_engine::read::{HeadSpec, streamed_held_len};
use crate::storage::SourceRef;

/// A streamed loop's longest crossfade, in ms of source (§ 7.3 step 6):
/// what its loop head holds before the loop.
pub const XFADE_MAX_MS: u32 = 100;

const CF: u32 = CHUNK_FRAMES.get();
/// A head's end past a chunk's first frame: its last read, `end − 3`, the
/// chunk's second frame.
const PAST: u32 = 4;
const _: () = assert!(PAST.is_multiple_of(2) && PAST < CF);

/// `XFADE_MAX_MS` of frames at `rate`, even.
pub fn xfade_max(rate: u16) -> u32 {
    (u32::from(rate) * XFADE_MAX_MS / 1_000) & !1
}

/// A streamed source's heads: START's, then the loop's in LOOP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heads {
    specs: [HeadSpec; 2],
    n: u8,
}

impl Heads {
    pub fn as_slice(&self) -> &[HeadSpec] {
        &self.specs[..usize::from(self.n)]
    }

    /// The row's `held`: the descriptor, the table, the heads.
    pub fn held_len(&self) -> usize {
        streamed_held_len(self.as_slice())
    }
}

/// The head read from `at` on, holding `reach` frames past it: to just
/// past the boundary after, at most to `cap`.
fn head(at: u32, reach: u32, cap: u32) -> Option<HeadSpec> {
    let first = at.saturating_sub(1) & !1;
    let past = at.saturating_add(reach).saturating_sub(PAST);
    let end = past.div_ceil(CF).saturating_mul(CF).saturating_add(PAST);
    HeadSpec::new(first, end.min(cap))
}

/// `r`'s heads for `p`; `None` where `p` needs the source in RAM.
pub fn heads_for(r: SourceRef, p: &TapeEngineParams) -> Option<Heads> {
    if needs_ram(p).is_some() || r.frames < 2 {
        return None;
    }
    let (frames, rate) = (r.frames, r.rate);
    let reach = head_frames(rate);
    let ws = window_start(p.start, frames);
    let we = window_end(ws, p.end, frames, rate);
    // To the taps of END's last read, no frame past the file's (even, as a
    // row reads it).
    let cap = we.saturating_add(2).min(frames).next_multiple_of(2);
    let start = head(ws, reach, cap)?;
    if p.mode != PlayMode::Loop {
        return Some(Heads {
            specs: [start; 2],
            n: 1,
        });
    }
    let b = Bounds::of(ws, we, (p.size, p.position, p.xfade), rate);
    let lx = b.lx.min(xfade_max(rate));
    // With no room before the loop the fade folds inside it (`heads`).
    let early = if b.ls - ws >= lx { lx } else { 0 };
    let looped = head(b.ls - early, lx + reach, cap)?;
    Some(Heads {
        specs: [start, looped],
        n: 2,
    })
}
