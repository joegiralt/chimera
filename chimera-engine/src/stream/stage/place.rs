//! Where the heads go: the placement itself. With `bank2` it is bank 2's
//! (`bank2.x` globs this module): the UI and a project load reach it
//! through `LoadDoor`, the deck (bank 2's too) directly.

use super::Heads;
use core::num::NonZeroU32;

use crate::dsp::tape_engine::heads::{Bounds, Sizing, window_end, window_start};
use crate::dsp::tape_engine::params::{TapeEngineParams, Way, needs_ram};
use crate::dsp::tape_engine::read::HeadSpec;
use crate::project::Refusal;
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

/// `r`'s heads for `p`, or why it has none: `p` needs the source in RAM,
/// or too few frames for a head. `SourceCheck` has passed `r`'s rate, so
/// `clamped` never moves it.
pub(crate) fn heads_in(r: SourceRef, p: &TapeEngineParams) -> Result<Heads, Refusal> {
    if let Some(n) = needs_ram(p) {
        return Err(Refusal::NeedsRam(n));
    }
    let Some(file) = NonZeroU32::new(r.frames).filter(|f| f.get() >= 2) else {
        return Err(Refusal::TooShort);
    };
    let rate = SourceRate::clamped(r.rate);
    let frames = r.frames;
    let reach = head_frames(rate);
    let ws = window_start(p.start, file);
    let we = window_end(ws, p.end, frames, rate);
    // To the taps of END's last read, no frame past the file's (even, as a
    // row reads it).
    let cap = we.saturating_add(2).min(frames).next_multiple_of(2);
    let start = head(ws, reach, cap).ok_or(Refusal::TooShort)?;
    // LOOP (TRON or not) loops off a second head. ONE and TRON read one,
    // from START: POSITION or DRIFT past it starts cold (ADR 0202);
    // `needs_ram` refused the others.
    if p.way() != Way::Loop {
        return Ok(Heads::One(start));
    }
    let b = Bounds::of(
        ws,
        we,
        Sizing {
            size: p.size,
            place: p.position,
            xfade: p.xfade,
        },
        rate,
    );
    let lx = b.lx().min(xfade_max(rate));
    // With no room before the loop the fade folds inside it (`heads`).
    let early = if b.ls() - ws >= lx { lx } else { 0 };
    let looped = head(b.ls() - early, lx + reach, cap).ok_or(Refusal::TooShort)?;
    Ok(Heads::Looped([start, looped]))
}
