//! START and END on TAPE (#698 finding 6): a turn moves them in time, not
//! in 128ths of the source, and MIX + turn snaps to the next onset the way
//! the turn goes (an MPC's and Octatrack's "snap to transient" while
//! trimming a sample start). PREP isn't built, so the onsets are a cheap
//! energy rise found on what RAM holds: a whole row, or a streamed
//! source's heads (the rest is on the card).

use crate::dsp::tape_engine::read::{Descriptor, SourceView, head_table};

/// Ms a detent moves at `ticks` of the encoder's acceleration
/// (`encoder_accel`: 1, 2, 4, 5, 10): their square, so a slow detent is
/// 1 ms and a fast one 100 ms.
pub fn step_ms(ticks: i8) -> f32 {
    let a = f32::from(ticks.unsigned_abs());
    a * a
}

/// START or END at `v` (of the source) turned `ticks` over a source of
/// `frames` at `rate`: `step_ms` of it, a full-speed detent a hundredth of
/// the source at least, so a long bed crosses in a few turns.
pub fn stepped(v: f32, ticks: i8, rate: u16, frames: u32) -> f32 {
    let secs = frames as f32 / f32::from(rate.max(1));
    if secs <= 0.0 || ticks == 0 {
        return v;
    }
    let mut ms = step_ms(ticks);
    if ticks.unsigned_abs() >= 10 {
        ms = ms.max(secs * 10.0);
    }
    let d = f32::from(ticks.signum()) * ms / (1_000.0 * secs);
    (v + d).clamp(0.0, 1.0)
}

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
    let n = block(v.desc().rate);
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
pub fn onset(held: &[u8], frames: u32, from: u32, fwd: bool) -> Option<u32> {
    let d = Descriptor::from_bytes(held)?;
    if d.heads == 0 {
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

/// START or END at `v` as time into the source: `137 MS` under a second,
/// else `12.345 S`, to the ms a slow detent moves.
pub fn time_text(buf: &mut crate::ui::fmt::FmtBuf, v: f32, rate: u16, frames: u32) {
    let ms = (v.clamp(0.0, 1.0) * frames as f32 * 1_000.0 / f32::from(rate.max(1))) as u32;
    let _ = if ms < 1_000 {
        write!(buf, "{ms} MS")
    } else {
        write!(buf, "{}.{:03} S", ms / 1_000, ms % 1_000)
    };
}
