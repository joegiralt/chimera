//! A loop's heads, bounds and crossfade (spec § 2 The macros and the
//! window, LOOP): the pure arithmetic. The loop is `[ls, le)`; over its
//! last `lx` frames a second head reads into the next pass's start, and the
//! output fades linearly to it (Digitakt II manual § Playback modes: a
//! loop's bounds are latched at the wrap).
//!
//! With too few frames before the next loop's start, the fade folds inside
//! it: the pass after plays `[ls + lx, le)`, with `[ls, ls + lx)` as the
//! fade's pre-roll. So no setting but XFADE 0 ever hard-wraps.
//!
//! The heads move forward in their own frame `u`. A mirrored frame reads the
//! source at `off − u`, so REVERSE, PING-PONG's way down and SCRUB's
//! backward vari-speed are LOOP's arithmetic read backward.

use super::read::Pos;
use super::{MIN_LOOP_FRAMES, MIN_LOOP_S, MIN_PASS_SAMPLES};
use crate::dsp::algo::math::clampf;

/// One read head: a 32.32 position in frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub pos: Pos,
}

/// `n` whole frames as a position.
pub const fn at(n: u32) -> Pos {
    Pos((n as u64) << 32)
}

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

/// PING-PONG's clamp (spec § 2): a head left outside new bounds plays on,
/// its own way, while the output moves to the head that starts inside, over
/// `len` samples, linearly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ghost {
    /// Where it reads, in source frames.
    pub pos: Pos,
    /// Added to what it reads: the share of a fade it retargeted, so the
    /// output carries on from where it was and fades with it.
    pub lift: f32,
    /// It moves toward frame 0.
    pub back: bool,
    /// Samples of the fade played, and its length.
    pub done: u32,
    pub len: u32,
}

/// PING-PONG's turn in progress: `k` samples of `2·th`. The head's step is
/// scaled by `|th − k − ½| / th`, so it slows to a stop over `th` samples
/// (travelling `th/2` steps) and comes back as it went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    pub k: u16,
    pub th: u16,
    /// `2^16 / 2·th`, worked out when the turn starts.
    pub unit: u32,
}

/// What a head `rem` frames before its pass's end does about `next`, the
/// next pass's bounds, in a window from `ws`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Plan {
    /// Fade now into `restart`, where the head lands at the wrap.
    Fade { restart: Pos },
    /// Too long a fade for the room: start it at this frame instead.
    Later(u32),
    /// No room at all: a hard splice at the end.
    Splice,
}

/// A fade of `rem` reads `rem` frames before the next pass's restart. They
/// lie before `next.ls` if the window has them there; else inside the loop
/// (folded), up to half of it.
pub fn plan(rem: Pos, next: &Bounds, ws: u32, le: u32) -> Plan {
    let r = rem.whole() as f32 + rem.frac();
    let before = next.ls - ws;
    if r <= before as f32 {
        return Plan::Fade {
            restart: at(next.ls),
        };
    }
    let room = before.max(next.frames() / 2);
    if r <= room as f32 {
        Plan::Fade {
            restart: at(next.ls) + rem,
        }
    } else if room == 0 {
        Plan::Splice
    } else {
        Plan::Later(le - room)
    }
}

/// A fade in progress: the second head's share is `1 − (end − head) · inv`.
///
/// The fade advances with the head, so one shorter than a sample's step
/// (`lx < inc`: a 2 ms loop played far above its own speed) is crossed in a
/// sample or two, and the wrap is in effect hard. `inc`'s clamp bounds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    /// The pass's end, where the second head takes over.
    pub end: Pos,
    /// 1 / the fade's length in frames.
    pub inv: f32,
    /// The pass it fades into.
    pub next: Bounds,
}

impl Fade {
    /// A fade over the `rem` frames to `end`. A fade under one frame takes
    /// one, so `inv` stays finite.
    pub fn new(end: Pos, rem: Pos, next: Bounds) -> Fade {
        let r = rem.whole() as f32 + rem.frac();
        Fade {
            end,
            inv: 1.0 / r.max(1.0),
            next,
        }
    }

    /// The second head's share with the first at `h`.
    #[inline(always)]
    pub fn gain(&self, h: Pos) -> f32 {
        let rem = self.end - h;
        1.0 - (rem.whole() as f32 + rem.frac()) * self.inv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fade 100 / 2³² of a frame long: finite, its gain in [0, 1].
    #[test]
    fn a_sliver_of_a_fade_stays_finite() {
        let end = at(1_000);
        let f = Fade::new(end, Pos(100), Bounds::default());
        assert!(f.inv.is_finite());
        let g = f.gain(end - Pos(100));
        assert!((0.0..=1.0).contains(&g), "{g}");
        assert_eq!(f.gain(end), 1.0);
    }

    #[test]
    fn mirror_twice_is_the_same() {
        let b = Bounds {
            ls: 10,
            le: 90,
            lx: 8,
        };
        assert_eq!(
            b.mirror(100),
            Bounds {
                ls: 10,
                le: 90,
                lx: 8
            }
        );
        assert_eq!(b.mirror(200).mirror(200), b);
        assert_eq!(b.mirror(200).ls, 110);
    }

    #[test]
    fn loop_len_spans_2ms_to_the_window() {
        assert_eq!(loop_len(0.0, 48_000, 48_000), 96);
        assert_eq!(loop_len(1.0, 48_000, 48_000), 48_000);
        assert_eq!(loop_len(0.5, 9_600, 48_000), 960);
        // A window under 2 ms is the loop; never 0.
        assert_eq!(loop_len(0.0, 3, 48_000), 3);
        assert_eq!(loop_len(0.0, 0, 48_000), 1);
    }

    #[test]
    fn end_keeps_2ms_past_start() {
        assert_eq!(window_end(1_000, 0.0, 48_000, 48_000), 1_096);
        assert_eq!(window_end(47_990, 1.0, 48_000, 48_000), 48_000);
        assert_eq!(window_end(1, 0.0, 2, 48_000), 2);
        assert_eq!(window_start(1.0, 2), 1);
    }

    #[test]
    fn neutral_folds_inside_the_loop() {
        let b = Bounds::of(0, 10_000, (1.0, 0.0, 0.1), 48_000);
        assert_eq!(
            b,
            Bounds {
                ls: 0,
                le: 10_000,
                lx: 1_000
            }
        );
        let rem = at(1_000);
        assert_eq!(plan(rem, &b, 0, b.le), Plan::Fade { restart: at(1_000) });
    }

    #[test]
    fn room_before_the_loop_reads_there() {
        let b = Bounds::of(0, 10_000, (0.5, 0.5, 0.1), 48_000);
        assert!(b.ls > b.lx);
        assert_eq!(
            plan(at(b.lx), &b, 0, b.le),
            Plan::Fade { restart: at(b.ls) }
        );
    }

    #[test]
    fn a_fade_too_long_for_the_room_waits() {
        let b = Bounds {
            ls: 0,
            le: 100,
            lx: 10,
        };
        assert_eq!(plan(at(5_000), &b, 0, 48_000), Plan::Later(48_000 - 50));
        let one = Bounds {
            ls: 0,
            le: 1,
            lx: 0,
        };
        assert_eq!(plan(at(5), &one, 0, 48_000), Plan::Splice);
    }
}
