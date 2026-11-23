// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window and its loop (spec § 2 The macros and the window): the pure
//! arithmetic both banks take. Bank 1's, outside `bank2.x`'s globs: the
//! deck calls it per pass, and a streamed load places its heads by it.
//! Inlined always, so each bank keeps its own copy and none lands in bank
//! 1 alone for bank 2's callers (ADR 0203).

use core::num::NonZeroU32;

use super::{MIN_LOOP_FRAMES, MIN_LOOP_S, MIN_PASS_SAMPLES};
use crate::dsp::algo::math::clampf;
use crate::stream::units::SourceRate;

/// The shortest loop at `rate`, in frames: 2 ms, at least
/// `MIN_LOOP_FRAMES`.
#[inline(always)]
pub fn min_frames(rate: SourceRate) -> u32 {
    shortest(rate, 0.0)
}

/// The shortest loop at `rate` for a head moving up to `inc` frames a
/// sample: `min_frames`, and at least `MIN_PASS_SAMPLES · inc`, so a pass
/// (or a turn) comes at most once in `MIN_PASS_SAMPLES` output samples.
#[inline(always)]
pub fn shortest(rate: SourceRate, inc: f32) -> u32 {
    let pass = libm::ceilf(MIN_PASS_SAMPLES as f32 * inc) as u32;
    (libm::roundf(MIN_LOOP_S * f32::from(rate)) as u32)
        .max(MIN_LOOP_FRAMES)
        .max(pass)
}

/// The window's start, of `frames`: in `[0, frames − 1]`.
pub fn window_start(start: f32, frames: NonZeroU32) -> u32 {
    let n = frames.get();
    ((clampf(start, 0.0, 1.0) * n as f32) as u32).min(n - 1)
}

/// The window's end from `ws`: END, kept ≥ START + 2 ms and ≤ `frames`.
/// Always more than `ws`.
#[inline(always)]
pub fn window_end(ws: u32, end: f32, frames: u32, rate: SourceRate) -> u32 {
    window_end_min(ws, end, frames, min_frames(rate))
}

/// `window_end`, kept ≥ START + `shortest` frames.
#[inline(always)]
pub fn window_end_min(ws: u32, end: f32, frames: u32, shortest: u32) -> u32 {
    let e = libm::roundf(clampf(end, 0.0, 1.0) * frames as f32) as u32;
    e.max(ws.saturating_add(shortest)).min(frames)
}

/// SIZE's loop length in frames: `2 ms · (W / 2 ms)^size`, at least
/// `MIN_LOOP_FRAMES` (or `W`, if shorter; 1 at least), at most `W`.
#[inline(always)]
pub fn loop_len(size: f32, w: u32, rate: SourceRate) -> u32 {
    loop_len_min(size, w, rate, min_frames(rate))
}

/// `loop_len`, at least `shortest` frames (or `W`, if shorter).
#[inline(always)]
pub fn loop_len_min(size: f32, w: u32, rate: SourceRate, shortest: u32) -> u32 {
    let min = MIN_LOOP_S * f32::from(rate);
    let l = min * libm::powf(w as f32 / min, clampf(size, 0.0, 1.0));
    let w = w.max(1);
    (libm::roundf(l) as u32).clamp(shortest.min(w), w)
}

/// A loop's SIZE, its place (0–1) in the room the window leaves, and its
/// XFADE: what `Bounds::of` sizes it by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizing {
    pub size: f32,
    pub place: f32,
    pub xfade: f32,
}

/// One pass's loop: `[ls, le)`, its last `lx` frames the crossfade. Never
/// `le` before `ls`, nor a fade longer than the loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    ls: u32,
    le: u32,
    lx: u32,
}

impl Bounds {
    /// `[ls, le)` fading over its last `lx`; `None` if `le` is before `ls`
    /// or the fade longer than the loop.
    pub const fn new(ls: u32, le: u32, lx: u32) -> Option<Bounds> {
        if le >= ls && lx <= le - ls {
            Some(Bounds { ls, le, lx })
        } else {
            None
        }
    }

    /// `len` frames from `ls`, with no fade.
    pub const fn plain(ls: u32, len: u32) -> Bounds {
        Bounds {
            ls,
            le: ls + len,
            lx: 0,
        }
    }

    pub const fn ls(&self) -> u32 {
        self.ls
    }

    pub const fn le(&self) -> u32 {
        self.le
    }

    pub const fn lx(&self) -> u32 {
        self.lx
    }

    /// The same loop, its fade at most `cap` frames.
    pub fn lx_at_most(self, cap: u32) -> Bounds {
        Bounds {
            lx: self.lx.min(cap),
            ..self
        }
    }

    /// The loop in the window `[ws, we)`: SIZE's length, at `place` of the
    /// room left, its last XFADE of it the fade.
    #[inline(always)]
    pub fn of(ws: u32, we: u32, sizing: Sizing, rate: SourceRate) -> Bounds {
        Self::of_min(ws, we, sizing, rate, min_frames(rate))
    }

    /// `of`, the loop at least `shortest` frames (or the window). The
    /// window is never empty, `we > ws` (`window_end_min` keeps it so), and
    /// `loop_len_min` never exceeds it, so neither subtraction wraps.
    #[inline(always)]
    pub fn of_min(ws: u32, we: u32, sizing: Sizing, rate: SourceRate, shortest: u32) -> Bounds {
        let Sizing { size, place, xfade } = sizing;
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
