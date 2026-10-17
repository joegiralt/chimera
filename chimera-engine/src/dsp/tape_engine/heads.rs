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
//! source at `off − u`, so LOOP with END before START, PING-PONG's way down
//! and SCRUB's backward vari-speed are LOOP's arithmetic read backward.

use super::read::Pos;
pub use super::window::{
    Bounds, loop_len, loop_len_min, min_frames, shortest, window_end, window_end_min, window_start,
};
use crate::dsp::algo::math::clampf;

/// One read head: a 32.32 position in frames, and its frame: mirrored, it
/// reads the source backward (`off − u`). Each head has its own, so at a
/// flip the head going out finishes its pass the old way while the one
/// coming in starts the new (spec § 2 Machines).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub pos: Pos,
    pub mirror: bool,
}

/// Which way a machine plays its window: START is where the head starts,
/// END where it heads (spec § 2 Machines, #369).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Fwd,
    Rev,
}

impl Dir {
    /// END before START plays reversed; END on START plays forward.
    pub fn of(start: f32, end: f32) -> Dir {
        if end < start { Dir::Rev } else { Dir::Fwd }
    }
}

/// A window of the source, `[lo, hi)` in frames, sorted, and the way it
/// plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub lo: u32,
    pub hi: u32,
    pub dir: Dir,
}

impl Window {
    /// START and END as fractions of `frames` (≥ 1): sorted, and `Rev` when
    /// END is before START. Under `floor` frames it widens from START
    /// toward END's side, so the head always starts at START: at the
    /// source's edge it stays shorter, never empty.
    pub fn of(start: f32, end: f32, frames: u32, floor: u32) -> Window {
        let at = |x: f32| (libm::roundf(clampf(x, 0.0, 1.0) * frames as f32) as u32).min(frames);
        let dir = Dir::of(start, end);
        let (s, e) = (at(start), at(end));
        let (mut lo, mut hi) = (s.min(e), s.max(e));
        if hi - lo < floor {
            match dir {
                Dir::Fwd => {
                    lo = lo.min(frames - 1);
                    hi = (lo + floor).min(frames);
                }
                Dir::Rev => {
                    hi = hi.max(1);
                    lo = hi.saturating_sub(floor);
                }
            }
        }
        Window { lo, hi, dir }
    }

    pub fn frames(&self) -> u32 {
        self.hi - self.lo
    }
}

/// `n` whole frames as a position.
pub const fn at(n: u32) -> Pos {
    Pos((n as u64) << 32)
}

/// PING-PONG's clamp (spec § 2): a head left outside new bounds plays on,
/// its own way, while the output moves to the head that starts inside, over
/// `len` samples, linearly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ghost {
    /// Where it reads, in source frames.
    pub pos: Pos,
    /// The output a fade it retargeted had reached: the ghost's share
    /// fades from it rather than from what the ghost reads (R1), so the
    /// output carries on from there and stays a mix of it and the reads.
    pub hold: Option<f32>,
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

    /// A fade run back (SCRUB turning round mid-fade) from a second head's
    /// share `share` over the `span` frames to `end`: its slope is
    /// `share / span`. `span` is floored at the least `Pos::frac` reads,
    /// 2⁻²⁴ of a frame, so a way back shorter than that (a reversal at a
    /// share of 0 just under a whole frame) is a sliver, never 0/0.
    pub fn back(end: Pos, span: Pos, share: f32, next: Bounds) -> Fade {
        const LEAST: f32 = 1.0 / 16_777_216.0;
        let r = span.whole() as f32 + span.frac();
        Fade {
            end,
            inv: share / r.max(LEAST),
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

    /// A fade run back over 2⁻³² of a frame, which `frac` reads as 0, at
    /// a share of 0 or of a half: finite, its gain in [0, 1].
    #[test]
    fn a_fade_run_back_over_a_sliver_stays_finite() {
        let end = at(1_000);
        for share in [0.0, 0.5] {
            let f = Fade::back(end, Pos(1), share, Bounds::default());
            assert!(f.inv.is_finite(), "{share}");
            for h in [end - Pos(1), end] {
                let g = f.gain(h);
                assert!((0.0..=1.0).contains(&g), "{share}: {g}");
            }
        }
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
