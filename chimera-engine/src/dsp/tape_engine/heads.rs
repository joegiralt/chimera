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

use super::Way;
use super::read::{Pos, SourceView};
pub use super::window::{
    Bounds, Sizing, loop_len, loop_len_min, min_frames, shortest, window_end, window_end_min,
    window_start,
};
use crate::dsp::algo::math::clampf;
use crate::hw::Cost;
use crate::stream::units::SourceRate;

/// A pass's length in output samples, banded (ADR 0211): under 16 (only a
/// source shorter than the loop's floor), then from 16, 60, 256, 1,024
/// and 4,096 up.
const BANDS: [u32; 5] = [16, 60, 256, 1_024, 4_096];
/// LOOP's and TRON's bill over `COST_LOOP` by band, a fade summing to 1
/// and an equal-power one (none under 16: such a fade is short), at each
/// band's shortest pass, rounded up to 10. A pass costs `512 + 1,216 /
/// spw` (the unit's 816 every 4 samples, 588 every 16; 591 reversed, so 80
/// at 16); an equal-power fade 10 more a sample (`xgains`' 8 flops) and
/// 10,500 a pass (`power_of`, fitted to the 40 ms loop's 699 every 62).
/// The 0.3 unit (ADR 0218): LOOP 64 776 and TRON 958 every 4, 180 and 370
/// over the floor's 596. The 16–59 band stays: LOOP 256 reads 642, but no
/// row reads TRON or the equal-power fade there.
const LOOP_EXTRA: [(u32, u32); 5] = [(80, 740), (20, 200), (0, 50), (0, 20), (0, 10)];
const LOOP_TINY: u32 = 180;
const TRON_TINY: u32 = 370;
/// PING-PONG's turn under 16 samples: 650 on the 0.3 unit, 84 over
/// `COST_PING`'s floor (ADR 0218).
const PING_TINY: u32 = 90;

/// What a note's loop bills over its model's (ADR 0211): `frames` a pass
/// and `fade` of them crossfading, at `rate`, read `inc` frames a sample at
/// the fastest its note reaches. Only LOOP, TRON and PING-PONG pay; a fade
/// of 20 ms or more may be equal-power (`POWER_FADE_S`).
pub fn loop_extra(way: Way, frames: u32, fade: u32, rate: SourceRate, inc: f32) -> Cost {
    let spw = frames as f32 / inc.max(1e-3);
    let band = BANDS.iter().filter(|&&b| spw >= b as f32).count();
    let power = fade as f32 >= POWER_FADE_S * f32::from(rate);
    let c = match (way, band) {
        (Way::Ping, 0) => PING_TINY,
        (Way::Ping, _) => 0,
        (Way::Loop, 0) => LOOP_TINY,
        (Way::Tron, 0) => TRON_TINY,
        (Way::Loop | Way::Tron, b) => {
            let (sum, pow) = LOOP_EXTRA[b - 1];
            if power { pow } else { sum }
        }
        _ => 0,
    };
    Cost(c)
}

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

    /// The other way: WALK's REVERSE.
    pub const fn flip(self) -> Dir {
        match self {
            Dir::Fwd => Dir::Rev,
            Dir::Rev => Dir::Fwd,
        }
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
/// lie before `next.ls()` if the window has them there; else inside the loop
/// (folded), up to half of it.
pub fn plan(rem: Pos, next: &Bounds, ws: u32, le: u32) -> Plan {
    let r = rem.whole() as f32 + rem.frac();
    let before = next.ls() - ws;
    if r <= before as f32 {
        return Plan::Fade {
            restart: at(next.ls()),
        };
    }
    let room = before.max(next.frames() / 2);
    if r <= room as f32 {
        Plan::Fade {
            restart: at(next.ls()) + rem,
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
    /// 0: gains that sum to 1 (a short loop, correlated across the
    /// splice); `POWER`: gains whose powers sum to about 1 (`xmix`).
    pub power: f32,
}

/// `Fade::power` for an equal-power fade.
pub const POWER: f32 = 0.5;
/// A fade this long or longer may be equal-power (`power_of`): over a
/// pad's or a bed's uncorrelated material a sum-to-1 fade dips 3 dB at its
/// middle (#698). A shorter one, a short loop's, splices correlated
/// material: sum to 1.
pub const POWER_FADE_S: f32 = 0.020;
/// Pairs `power_of` reads across a fade.
const PAIRS: u32 = 64;

/// A fade's `xgains` power from what its two heads will read (Kontakt and
/// the S950 offer both laws; this picks by the material): the normalized
/// correlation `ρ` of `PAIRS` sample pairs, from `a0` in `va` and `b0` in
/// `vb`, `span` frames on, and the power that holds a mix of material
/// that correlated at the fade's middle, `POWER` at most: correlated
/// material (a held tone) sums to 1 and never peaks past its own level,
/// and uncorrelated material (a bed, noise) keeps its power.
/// `None` where the views hold under `4 · PAIRS` frames of it.
pub fn power_of(
    va: &SourceView<'_>,
    a0: u32,
    vb: &SourceView<'_>,
    b0: u32,
    span: u32,
) -> Option<f32> {
    // Over what both views hold of the fade, `MIN_PAIRS_SPAN` at least.
    let room = |v: &SourceView<'_>, x: u32| (v.first() <= x).then(|| v.end().saturating_sub(x + 1));
    let n = span.min(room(va, a0)?).min(room(vb, b0)?);
    if n < PAIRS * 4 {
        return None;
    }
    let d = n / PAIRS;
    let (mut ab, mut aa, mut bb) = (0f32, 0f32, 0f32);
    let mut r = 0x9E37_79B9u32 ^ n;
    for j in 0..PAIRS {
        // A random place in each stride: a tone whose period divides the
        // stride can't alias to one phase.
        let x = j * d + crate::dsp::xorshift32(&mut r) % d;
        let (a, b) = (va.sample(a0 + x) as f32, vb.sample(b0 + x) as f32);
        (ab, aa, bb) = (ab + a * b, aa + a * a, bb + b * b);
    }
    let rho = if aa * bb > 0.0 {
        ab / crate::dsp::sqrt(aa * bb)
    } else {
        0.0
    };
    // Constant power at the middle: `x·(1 + 0.75c) = 1 / √(2 + 2ρ)`.
    let rho = clampf(rho, 0.0, 1.0);
    Some(clampf(
        (2.0 / crate::dsp::sqrt(2.0 + 2.0 * rho) - 1.0) * (4.0 / 3.0),
        0.0,
        POWER,
    ))
}

/// The fade's output at the second head's share `g`: a smoothstep `x`,
/// and gains `y·(1 + c·(1 − y²))` and `x·(1 + c·(1 − x²))` (`y = 1 − x`).
/// `c` 0 is the plain `a + x·(b − a)`; at `POWER` the powers sum to
/// 0.945–1 (−0.24 dB at the middle, against −3 dB).
#[inline(always)]
pub fn xmix(a: f32, b: f32, g: f32, c: f32) -> f32 {
    let (ga, gb) = xgains(g, c);
    a * ga + b * gb
}

/// `xmix`'s gains at share `g`: the head going out's, the one coming in's.
#[inline(always)]
pub fn xgains(g: f32, c: f32) -> (f32, f32) {
    let x = g * g * (3.0 - 2.0 * g);
    let y = 1.0 - x;
    (y * (1.0 + c * (1.0 - y * y)), x * (1.0 + c * (1.0 - x * x)))
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
            power: 0.0,
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
            power: 0.0,
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
    use crate::stream::units::SourceRate;
    use core::num::NonZeroU32;

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
        let b = Bounds::new(10, 90, 8).unwrap();
        assert_eq!(b.mirror(100), Bounds::new(10, 90, 8).unwrap());
        assert_eq!(b.mirror(200).mirror(200), b);
        assert_eq!(b.mirror(200).ls(), 110);
    }

    #[test]
    fn loop_len_spans_2ms_to_the_window() {
        assert_eq!(loop_len(0.0, 48_000, SourceRate::MAX), 96);
        assert_eq!(loop_len(1.0, 48_000, SourceRate::MAX), 48_000);
        assert_eq!(loop_len(0.5, 9_600, SourceRate::MAX), 960);
        // A window under 2 ms is the loop; never 0.
        assert_eq!(loop_len(0.0, 3, SourceRate::MAX), 3);
        assert_eq!(loop_len(0.0, 0, SourceRate::MAX), 1);
    }

    #[test]
    fn end_keeps_2ms_past_start() {
        assert_eq!(window_end(1_000, 0.0, 48_000, SourceRate::MAX), 1_096);
        assert_eq!(window_end(47_990, 1.0, 48_000, SourceRate::MAX), 48_000);
        assert_eq!(window_end(1, 0.0, 2, SourceRate::MAX), 2);
        assert_eq!(window_start(1.0, NonZeroU32::new(2).unwrap()), 1);
    }

    #[test]
    fn neutral_folds_inside_the_loop() {
        let b = Bounds::of(
            0,
            10_000,
            Sizing {
                size: 1.0,
                place: 0.0,
                xfade: 0.1,
            },
            SourceRate::MAX,
        );
        assert_eq!(b, Bounds::new(0, 10_000, 1_000).unwrap());
        let rem = at(1_000);
        assert_eq!(plan(rem, &b, 0, b.le()), Plan::Fade { restart: at(1_000) });
    }

    #[test]
    fn room_before_the_loop_reads_there() {
        let b = Bounds::of(
            0,
            10_000,
            Sizing {
                size: 0.5,
                place: 0.5,
                xfade: 0.1,
            },
            SourceRate::MAX,
        );
        assert!(b.ls() > b.lx());
        assert_eq!(
            plan(at(b.lx()), &b, 0, b.le()),
            Plan::Fade {
                restart: at(b.ls())
            }
        );
    }

    #[test]
    fn a_fade_too_long_for_the_room_waits() {
        let b = Bounds::new(0, 100, 10).unwrap();
        assert_eq!(plan(at(5_000), &b, 0, 48_000), Plan::Later(48_000 - 50));
        let one = Bounds::new(0, 1, 0).unwrap();
        assert_eq!(plan(at(5), &one, 0, 48_000), Plan::Splice);
    }
}
