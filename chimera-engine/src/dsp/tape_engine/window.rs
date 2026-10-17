//! The window and its loop (spec § 2 The macros and the window): the pure
//! arithmetic both banks take. Bank 1's, outside `bank2.x`'s globs: the
//! deck calls it per pass, and a streamed load places its heads by it.

use super::{MIN_LOOP_FRAMES, MIN_LOOP_S, MIN_PASS_SAMPLES};
use crate::dsp::algo::math::clampf;

/// The shortest loop at `rate`, in frames: 2 ms, at least
/// `MIN_LOOP_FRAMES`.
pub fn min_frames(rate: u16) -> u32 {
    shortest(rate, 0.0)
}

/// The shortest loop at `rate` for a head moving up to `inc` frames a
/// sample: `min_frames`, and at least `MIN_PASS_SAMPLES · inc`, so a pass
/// (or a turn) comes at most once in `MIN_PASS_SAMPLES` output samples.
pub fn shortest(rate: u16, inc: f32) -> u32 {
    let pass = libm::ceilf(MIN_PASS_SAMPLES as f32 * inc) as u32;
    (libm::roundf(MIN_LOOP_S * rate as f32) as u32)
        .max(MIN_LOOP_FRAMES)
        .max(pass)
}

/// The window's start, of `frames` (≥ 1): in `[0, frames − 1]`.
pub fn window_start(start: f32, frames: u32) -> u32 {
    ((clampf(start, 0.0, 1.0) * frames as f32) as u32).min(frames - 1)
}

/// The window's end from `ws`: END, kept ≥ START + 2 ms and ≤ `frames`.
/// Always more than `ws`.
pub fn window_end(ws: u32, end: f32, frames: u32, rate: u16) -> u32 {
    window_end_min(ws, end, frames, min_frames(rate))
}

/// `window_end`, kept ≥ START + `shortest` frames.
pub fn window_end_min(ws: u32, end: f32, frames: u32, shortest: u32) -> u32 {
    let e = libm::roundf(clampf(end, 0.0, 1.0) * frames as f32) as u32;
    e.max(ws.saturating_add(shortest)).min(frames)
}

/// SIZE's loop length in frames: `2 ms · (W / 2 ms)^size`, at least
/// `MIN_LOOP_FRAMES` (or `W`, if shorter; 1 at least), at most `W`.
pub fn loop_len(size: f32, w: u32, rate: u16) -> u32 {
    loop_len_min(size, w, rate, min_frames(rate))
}

/// `loop_len`, at least `shortest` frames (or `W`, if shorter).
pub fn loop_len_min(size: f32, w: u32, rate: u16, shortest: u32) -> u32 {
    let min = MIN_LOOP_S * rate as f32;
    let l = min * libm::powf(w as f32 / min, clampf(size, 0.0, 1.0));
    let w = w.max(1);
    (libm::roundf(l) as u32).clamp(shortest.min(w), w)
}

/// One pass's loop: `[ls, le)`, its last `lx` frames the crossfade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub ls: u32,
    pub le: u32,
    pub lx: u32,
}

impl Bounds {
    /// The loop in the window `[ws, we)`: SIZE's length, at `place` (0–1)
    /// of the room left, its last XFADE of it the fade.
    pub fn of(ws: u32, we: u32, sizing: (f32, f32, f32), rate: u16) -> Bounds {
        Self::of_min(ws, we, sizing, rate, min_frames(rate))
    }

    /// `of`, the loop at least `shortest` frames (or the window).
    pub fn of_min(
        ws: u32,
        we: u32,
        (size, place, xfade): (f32, f32, f32),
        rate: u16,
        shortest: u32,
    ) -> Bounds {
        let w = we - ws;
        let ll = loop_len_min(size, w, rate, shortest);
        let ls = ws + ((clampf(place, 0.0, 1.0) * (w - ll) as f32) as u32).min(w - ll);
        Bounds {
            ls,
            le: ls + ll,
            lx: (clampf(xfade, 0.0, 0.5) * ll as f32) as u32,
        }
    }

    pub fn frames(&self) -> u32 {
        self.le - self.ls
    }

    /// Where the fade into the next pass starts: the end, at XFADE 0.
    pub fn fade_at(&self) -> u32 {
        self.le - self.lx
    }

    /// The same loop in a frame mirrored about `n` (≥ `le`): `u = n − x`.
    pub fn mirror(&self, n: u32) -> Bounds {
        Bounds {
            ls: n - self.le,
            le: n - self.ls,
            lx: self.lx,
        }
    }
}
