//! WALK's glyph, STRAIGHT (ADR 0209): runners on the home straight, seen
//! from the side, drawn for the glyph's real size (64 × 48 on the unit's
//! screen): at most two, each a head and 2 px limbs, one ink a runner,
//! no detail under 2 px.
//!
//! They face DIRECTION's way, the leader at the run's far end; the strip
//! under them is the window POSITION and LENGTH cut, its arrow the way
//! the run plays (both ends on PING-PONG), FADE its dashed tail. LOOP
//! runs them in front of an oval, its far side heading back; PING-PONG
//! sends one out and one back. DRIFT staggers them and shuffles their
//! strides; TRON plants a finish post where its tape runs out, the leader
//! there arms up, the strip past it grey.
//!
//! The poses are `static`s so they link under this module's name, into
//! bank 2 with the rest of `draw`.

use super::{ACC, FAINT, INK, INK2, MID, Pix, cos, fade_px, lvis, r, scatter, window_strip};
use crate::dsp::algo::math::clampf;
use crate::dsp::fast_sin;
use crate::dsp::tape_engine::tron::{self, TRON_LEN_MAX_S};
use crate::dsp::tape_engine::{Direction, Tron};
use crate::tape_glyph::{Ink, Steps};
use core::f32::consts::TAU;

/// The runners' feet, the lane under them, the strip.
const FEET: i32 = 40;
const LANE: i32 = 41;
const STRIP: i32 = 45;
/// The run's ends, px.
const X0: f32 = 6.0;
const X1: f32 = 58.0;

/// A pose's joints from the feet, facing right: head, neck, hip, near
/// knee and foot, far knee and foot, near elbow and hand, far elbow and
/// hand.
type Joints = [(i8, i8); 11];

/// A stride, a flight, and arms up at the finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pose {
    Stride,
    Flight,
    Cheer,
}

static POSES: [Joints; 3] = [
    // Stride: the near knee and arm reaching, the far foot pushing off
    // behind, the far arm back (near limbs lead, so the way reads).
    [
        (4, -19),
        (2, -15),
        (-1, -8),
        (4, -6),
        (3, -1),
        (-3, -4),
        (-6, 0),
        (4, -12),
        (6, -15),
        (-2, -11),
        (-5, -9),
    ],
    // Flight: both feet off the lane, the near leg out ahead.
    [
        (4, -20),
        (2, -16),
        (-1, -9),
        (4, -7),
        (5, -2),
        (-2, -5),
        (-6, -6),
        (4, -12),
        (6, -15),
        (-2, -12),
        (-5, -10),
    ],
    // Cheer: upright, both arms up.
    [
        (0, -19),
        (0, -15),
        (0, -8),
        (2, -4),
        (2, 0),
        (-2, -4),
        (-2, 0),
        (3, -18),
        (4, -22),
        (-3, -18),
        (-4, -22),
    ],
];

/// A runner: feet at `x`, facing left if `back`, inked `dim` steps
/// toward the ground.
#[derive(Clone, Copy)]
struct Runner {
    x: i32,
    back: bool,
    pose: Pose,
    dim: u8,
}

impl Runner {
    fn draw(self, p: &mut Pix) {
        let j = &POSES[self.pose as usize];
        let s = if self.back { -1 } else { 1 };
        let at = |k: usize| (self.x + s * j[k].0 as i32, FEET + j[k].1 as i32);
        let ink = |c: Ink| (0..self.dim).fold(c, |c, _| c.dim());
        // A limb 2 px wide: the line and its neighbour behind.
        let limb = |p: &mut Pix, a: usize, b: usize, c: Ink| {
            let ((x0, y0), (x1, y1)) = (at(a), at(b));
            p.line(x0, y0, x1, y1, c);
            p.line(x0 - s, y0, x1 - s, y1, c);
        };
        let (far, near) = (ink(MID), ink(INK));
        for (a, b) in [(2, 5), (5, 6), (1, 9), (9, 10)] {
            limb(p, a, b, far);
        }
        for (a, b) in [(1, 2), (2, 3), (3, 4), (1, 7), (7, 8)] {
            limb(p, a, b, near);
        }
        let (hx, hy) = at(0);
        p.dot(hx, hy, 2, near);
    }
}

/// DRIFT on runner `k`: a fixed nudge in px and a shuffled stride.
fn drift_of(k: usize, w: Steps) -> (f32, Pose) {
    let (sx, sy) = scatter(k as i32 * 3 + 1);
    let shuffle = w.drift.step() as usize * (1 + (sy > 0.0) as usize);
    let pose = if (k + shuffle).is_multiple_of(2) {
        Pose::Stride
    } else {
        Pose::Flight
    };
    (sx * w.drift.unit() * 4.0, pose)
}

/// TRON's tape, as a share of the run (`tron::tron_len_s` over 8 s).
fn tron_share(w: Steps) -> f32 {
    tron::TapeSeconds::of_length(w.length.unit()).get() / TRON_LEN_MAX_S
}

/// The finish post at `x`: a pole, a chequered flag flying `away` (±1)
/// from the runners, 2 px squares.
fn post(p: &mut Pix, x: i32, away: i32) {
    p.rect(x, 20, 2, LANE - 20, INK2);
    for (i, j) in [(0, 0), (1, 1), (2, 0)] {
        let fx = if away > 0 {
            x + 2 + 2 * i
        } else {
            x - 2 - 2 * i
        };
        p.rect(fx, 20 + 2 * j, 2, 2, INK);
        p.rect(fx, 22 - 2 * j, 2, 2, MID);
    }
}

/// The strip: the window `lo..hi` lit, FADE's tail dashed at the end the
/// run fades out (`lo` when `back`), an arrow past it pointing the way.
fn strip(p: &mut Pix, (lo, hi): (i32, i32), w: Steps, back: bool, arrow: bool) {
    window_strip(p, lo, hi, STRIP, 0);
    let f = fade_px(hi - lo, w.fade.unit());
    for i in (1..f).step_by(2) {
        let x = if back { lo + i } else { hi - 1 - i };
        p.rect(x, STRIP, 1, 2, FAINT);
    }
    if arrow {
        if back {
            p.arrow(lo - 2, STRIP, -1, ACC);
        } else {
            p.arrow(hi + 1, STRIP, 1, ACC);
        }
    }
}

/// The leader at `lead` and, with `two`, one more at `tail` nudged by
/// DRIFT and dimmed by FADE (the leader never fades).
fn pack(p: &mut Pix, w: Steps, two: bool, (lead, tail): (f32, f32), back: bool, cheer: bool) {
    let fade = libm::roundf(w.fade.unit() * 4.0) as u8;
    // Behind first, so the leader draws over them.
    for k in (0..1 + two as usize).rev() {
        let (dx, pose) = drift_of(k, w);
        let (x, dim) = if k == 0 {
            (lead, 0)
        } else {
            (tail + dx, fade.div_ceil(2))
        };
        Runner {
            x: r(clampf(x, 9.0, 55.0)),
            back,
            pose: if cheer && k == 0 { Pose::Cheer } else { pose },
            dim,
        }
        .draw(p);
    }
}

/// Two runners once they fit apart in a run `span` px long.
fn two(span: f32) -> bool {
    span >= 26.0
}

/// A loop's window on the run: LENGTH's span at POSITION of the room,
/// inside `X0..X1` so the strip's arrows fit the box.
fn span(pos: f32, len: f32) -> (f32, f32) {
    let l = lvis(len) * (X1 - X0);
    let lo = X0 + pos * (X1 - X0 - l);
    (lo, lo + l)
}

/// WALK on the home straight (the module's doc).
#[inline(never)]
pub(super) fn straight(p: &mut Pix, w: Steps) {
    let (pos, len) = (w.position.unit(), w.length.unit());
    let tron = w.tron == Tron::On;
    match w.direction() {
        Direction::Forward | Direction::Reverse => {
            let back = w.direction() == Direction::Reverse;
            let from = X0 + pos * (X1 - X0);
            let to = from + lvis(len) * (X1 - from);
            p.rect(2, LANE, 60, 1, FAINT);
            strip(p, (r(from), r(to)), w, back, true);
            // REVERSE plays the window from its end.
            let (start, end) = if back { (to, from) } else { (from, to) };
            let d = if back { -1.0 } else { 1.0 };
            // TRON's tape along the run, as it plays, within the window.
            let tape = tron.then(|| {
                let e = tron_share(w) * (X1 - X0);
                let at = if back { X1 - e } else { X0 + e };
                clampf(at, from, to)
            });
            if let Some(t) = tape {
                // The run past the tape never plays.
                let (a, z) = (r(t), r(end));
                p.rect(a.min(z), STRIP, (a - z).abs(), 2, MID);
                post(p, r(t + d * 3.0) - (back as i32), d as i32);
            }
            let stop = tape.unwrap_or(end);
            let two = two((stop - start).abs());
            let ends = (stop - d * 4.0, start + d * 4.0);
            pack(p, w, two, ends, back, tape.is_some());
        }
        Direction::Loop => {
            let (lo, hi) = span(pos, len);
            // The track round them, its far side heading back.
            for i in 0..128 {
                let a = TAU * i as f32 / 128.0;
                p.px(r(32.0 + 30.0 * cos(a)), r(28.0 + 12.0 * fast_sin(a)), MID);
            }
            p.arrow(30, 16, -1, ACC);
            strip(p, (r(lo), r(hi)), w, false, false);
            if tron {
                post(p, r(hi) + 3, 1);
            }
            pack(p, w, two(hi - lo), (hi - 4.0, lo + 4.0), false, tron);
        }
        Direction::Ping => {
            let (lo, hi) = span(pos, len);
            p.rect(2, LANE, 60, 1, FAINT);
            strip(p, (r(lo), r(hi)), w, false, true);
            p.arrow(r(lo) - 2, STRIP, -1, ACC);
            if tron {
                post(p, r(hi) + 2, 1);
            }
            // One out, one back, each toward the end it heads for, back
            // to back and never closer than their strides.
            let half = ((hi - lo) * 0.2).max(8.0);
            let mid = clampf((lo + hi) / 2.0, 9.0 + half, 55.0 - half);
            let (a, b) = (mid - half, mid + half);
            for (k, x, back) in [(1, a, true), (0, b, false)] {
                let (dx, pose) = drift_of(k, w);
                Runner {
                    x: r(x + dx),
                    back,
                    pose: if tron && k == 0 { Pose::Cheer } else { pose },
                    dim: 0,
                }
                .draw(p);
            }
        }
    }
}
