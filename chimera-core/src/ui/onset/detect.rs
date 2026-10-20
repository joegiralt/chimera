//! The onset detector (`onset`): bank 2's with `bank2` (`bank2.x` globs
//! this module), reached through `UiDoor::onset`.

use crate::dsp::tape_engine::read::{Descriptor, SourceView, head_table};

/// Frames an energy block: 2 ms, at least 16.
fn block(rate: u16) -> u32 {
    (u32::from(rate) / 500).max(16)
}

/// The block's energy, |x| summed over `[at, at + n)`.
fn energy(v: &SourceView<'_>, at: u32, n: u32) -> u32 {
    (at..at + n).map(|i| v.sample(i).unsigned_abs()).sum()
}

/// Where an attack starts in `v`'s frames, `[first, end)`: a block whose
/// energy is over 3× the two before it and over −48 dBFS, pulled back to
/// the zero crossing before its first loud sample. The nearest more than
/// a block after `from` (`fwd`) or before it, so a mark on an onset moves
/// on to the next.
pub fn onset_in(v: &SourceView<'_>, from: u32, fwd: bool) -> Option<u32> {
    let n = block(v.desc().rate().get());
    let (lo, hi) = (v.first(), v.end());
    if hi < lo + 3 * n {
        return None;
    }
    // −48 dBFS of 2048 full scale, |x| a sample.
    let floor = n * 8;
    let at = |b: u32| lo + b * n;
    let blocks = (hi - lo) / n;
    let found = |b: u32| -> Option<u32> {
        let (e0, e1, e) = (
            energy(v, at(b - 2), n),
            energy(v, at(b - 1), n),
            energy(v, at(b), n),
        );
        if e < floor || e <= 3 * (e0 + e1) / 2 {
            return None;
        }
        let peak = (at(b)..at(b) + n)
            .map(|i| v.sample(i).unsigned_abs())
            .max()?;
        let loud = (at(b)..at(b) + n).find(|&i| 2 * v.sample(i).unsigned_abs() >= peak)?;
        // Back to the crossing: the first sample whose sign differs before it.
        let s = v.sample(loud).signum();
        let start = (at(b - 1)..loud)
            .rev()
            .find(|&i| v.sample(i).signum() != s)
            .map_or(loud, |i| i + 1);
        Some(start)
    };
    let here = (from.saturating_sub(lo) / n).min(blocks - 1);
    if fwd {
        (here.max(2)..blocks)
            .filter_map(found)
            .find(|&f| f > from + n)
    } else {
        (2..=here).rev().filter_map(found).find(|&f| f + n < from)
    }
}

/// `onset_in` over a source's held bytes: the whole row, or each RAM head
/// of a streamed one (of a file of `frames`), the nearest of them.
pub(crate) fn onset_in_bank2(held: &[u8], frames: u32, from: u32, fwd: bool) -> Option<u32> {
    let d = Descriptor::from_bytes(held)?;
    if !d.is_streamed() {
        return onset_in(&SourceView::of(held, d)?, from, fwd);
    }
    let table = head_table(held)?;
    let frames = frames.saturating_add(1) & !1;
    (0..table.len())
        .filter_map(|i| {
            let (h, at) = table.get(i)?;
            let bytes = held.get(at..at + h.byte_len())?;
            onset_in(&SourceView::span(bytes, d, h.first(), frames)?, from, fwd)
        })
        .reduce(|a, b| if (b < a) == fwd { b } else { a })
}
