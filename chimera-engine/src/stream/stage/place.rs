//! Where the heads go: the placement itself. With `bank2` it is bank 2's
//! (`bank2.x` globs this module): the UI and a project load reach it
//! through `LoadDoor`, the deck (bank 2's too) directly.

use super::Heads;
use crate::dsp::tape_engine::heads::{Bounds, window_end, window_start};
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeMachine, needs_ram};
use crate::dsp::tape_engine::read::HeadSpec;
use crate::storage::SourceRef;
use crate::stream::admit::head_frames;
use crate::stream::layout::CHUNK_FRAMES;
use crate::stream::units::SourceRate;

use super::XFADE_MAX_MS;

const CF: u32 = CHUNK_FRAMES.get();
/// A head's end past a chunk's first frame: its last read, `end − 3`, the
/// chunk's second frame.
const PAST: u32 = 4;
const _: () = assert!(PAST.is_multiple_of(2) && PAST < CF);

/// `XFADE_MAX_MS` of frames at `rate`, even.
pub fn xfade_max(rate: SourceRate) -> u32 {
    (u32::from(rate) * XFADE_MAX_MS / 1_000) & !1
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
pub(crate) fn heads_in(r: SourceRef, p: &TapeEngineParams) -> Option<Heads> {
    let rate = SourceRate::new(r.rate);
    let (Some(rate), None, true) = (rate, needs_ram(p), r.frames >= 2) else {
        return None;
    };
    let frames = r.frames;
    let reach = head_frames(rate);
    let ws = window_start(p.start, frames);
    let we = window_end(ws, p.end, frames, rate);
    // To the taps of END's last read, no frame past the file's (even, as a
    // row reads it).
    let cap = we.saturating_add(2).min(frames).next_multiple_of(2);
    let start = head(ws, reach, cap)?;
    if p.machine() != TapeMachine::Loop {
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
