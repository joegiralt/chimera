//! WALK's other step styles: the owner's footsteps, signal and treadmill
//! mockups (ADR 0203's listening page). Kept, never shipped: they link
//! only into tests and `walk-styles` builds; the firmware draws STRAIGHT
//! (`super::straight`).

use super::{
    ACC, BG, FAINT, INK, INK2, MID, Pix, SOFT, cos, loop_span, lvis, people, r, scatter,
    window_strip,
};
use crate::dsp::algo::math::clampf;
use crate::dsp::fast_sin;
use crate::dsp::tape_engine::tron::{self, TRON_LEN_MAX_S};
use crate::dsp::tape_engine::{Direction, Tron};
use crate::tape_glyph::{Ink, StepStyle, Steps};
use core::f32::consts::TAU;

/// One footstep at (`x`, `y`), heading `(dx, dy)` (unit), on the `side`
/// (±1) of its line, in `style`.
fn step(
    p: &mut Pix,
    (x, y): (i32, i32),
    (dx, dy): (f32, f32),
    side: i32,
    ink: Ink,
    style: StepStyle,
) {
    // Off the line by 2 px to its side.
    let (ox, oy) = (r(-dy * 2.0) * side, r(dx * 2.0) * side);
    let (x, y) = (x + ox, y + oy);
    match style {
        StepStyle::Prints => {
            // The heel behind, the ball ahead, the toes past it.
            p.dot(x - r(dx * 2.5), y - r(dy * 2.5), 1, ink);
            p.dot(x + r(dx * 1.0), y + r(dy * 1.0), 1, ink);
            p.px(
                x + r(dx * 1.0) + r(dy * 1.5),
                y + r(dy * 1.0) - r(dx * 1.5),
                ink,
            );
            p.px(x + r(dx * 3.0), y + r(dy * 3.0), ink);
        }
        StepStyle::Treads => {
            let (bx, by) = (x - r(dx * 2.0), y - r(dy * 2.0));
            p.line(bx - r(dy * 2.0), by + r(dx * 2.0), x, y, ink);
            p.line(bx + r(dy * 2.0), by - r(dx * 2.0), x, y, ink);
        }
        // The figures draw no steps of their own (`signal`, `treadmill`).
        _ => {
            p.rect(x - 1, y - 1, 2, 2, ink);
        }
    }
}

/// The ink of step `k` of `n`: FADE fades the last ones, down to FAINT.
fn step_ink(k: i32, n: i32, fade: f32) -> Ink {
    let faded = libm::roundf(fade * 4.0) as i32;
    match n - k {
        d if d <= faded / 2 && faded > 0 => FAINT,
        d if d <= faded => MID,
        _ => ACC,
    }
}

/// TRON's tape end at `x`: the leader's edge, hatched, with its spool's
/// tab above.
fn tape_end(p: &mut Pix, x: i32, y: i32) {
    p.rect(x, y - 8, 1, 17, INK);
    for i in 0..4 {
        p.px(x + 1 + i % 2, y - 7 + 4 * i, INK2);
    }
    p.rect(x - 2, y - 10, 5, 2, INK2);
}

/// WALK: footsteps. FORWARD walks the run POSITION and LENGTH cut from the
/// window left to right, REVERSE right to left; LOOP round a track LENGTH
/// sizes and POSITION moves; PING-PONG there on one lane and back on the
/// other. DRIFT scatters each step, FADE fades the last, TRON stops them
/// at its tape's end (LENGTH's share of 8 s once through, the strip's end
/// round a loop) and greys the rest.
#[inline(never)]
pub(super) fn steps(p: &mut Pix, w: Steps, style: StepStyle) {
    match style {
        StepStyle::Signal => return signal(p, w),
        StepStyle::Treadmill => return treadmill(p, w),
        StepStyle::Oval => return people::oval(p, w, false),
        StepStyle::Stadium => return people::oval(p, w, true),
        StepStyle::StraightMockup => return people::straight(p, w),
        _ => {}
    }
    let (pos, len) = (w.position.unit(), w.length.unit());
    let drift = w.drift.unit() * 3.0;
    let fade = w.fade.unit();
    let at = |k: i32, x: f32, y: f32| {
        let (sx, sy) = scatter(k);
        (r(x + sx * drift * 0.7), r(y + sy * drift))
    };
    match w.direction() {
        Direction::Forward | Direction::Reverse => {
            let back = w.direction() == Direction::Reverse;
            let (x0, x1) = (6.0, 58.0);
            let from = x0 + pos * (x1 - x0);
            let to = from + lvis(len) * (x1 - from);
            let (lo, hi) = (r(from), r(to));
            window_strip(p, lo, hi, 44, 0);
            // TRON once through: its tape is LENGTH's share of 8 s.
            let end = (w.tron == Tron::On).then(|| {
                r(x0 + tron::TapeSeconds::of_length(len).get() / TRON_LEN_MAX_S * (x1 - x0))
            });
            let n = (((to - from) / 8.0) as i32).max(1) + 1;
            let d = if back { -1.0 } else { 1.0 };
            for k in 0..n {
                let x = if back {
                    to - 8.0 * k as f32
                } else {
                    from + 8.0 * k as f32
                };
                let out = end.is_some_and(|e| {
                    if back {
                        x < (x1 - (e as f32 - x0))
                    } else {
                        x > e as f32
                    }
                });
                let ink = if out { FAINT } else { step_ink(k, n - 1, fade) };
                let side = if k % 2 == 0 { 1 } else { -1 };
                step(p, at(k, x, 24.0), (d, 0.0), side, ink, style);
            }
            if let Some(e) = end {
                tape_end(p, if back { r(x1) - (e - r(x0)) } else { e }, 24);
            }
            p.arrow(
                if back { lo - 2 } else { hi + 2 },
                24,
                if back { -1 } else { 1 },
                INK2,
            );
        }
        Direction::Loop => {
            let (lo, hi) = loop_span(pos, len);
            window_strip(p, r(lo), r(hi), 44, 0);
            let rx = 6.0 + 22.0 * lvis(len);
            let cx = lo + (hi - lo) / 2.0;
            let cx = clampf(cx, rx + 2.0, 62.0 - rx);
            let (cy, ry) = (21.0, 13.0);
            for i in 0..64 {
                let a = TAU * i as f32 / 64.0;
                p.px(r(cx + rx * cos(a)), r(cy - ry * fast_sin(a)), FAINT);
            }
            let n = 8 + (rx / 5.0) as i32;
            for k in 0..n {
                let a = TAU * k as f32 / n as f32;
                let (c, sn) = (cos(a), fast_sin(a));
                // Clockwise, as a loop reads left to right on top.
                let (dx, dy) = (-sn, c);
                let ink = if w.tron == Tron::On && k == n - 1 {
                    FAINT
                } else {
                    step_ink(k, n - 1, fade)
                };
                step(
                    p,
                    at(k, cx + rx * c, cy - ry * sn),
                    (dx, -dy),
                    if k % 2 == 0 { 1 } else { -1 },
                    ink,
                    style,
                );
            }
            if w.tron == Tron::On {
                tape_end(p, r(cx), r(cy - ry));
            }
        }
        Direction::Ping => {
            let (lo, hi) = loop_span(pos, len);
            window_strip(p, r(lo), r(hi), 44, 0);
            let n = (((hi - lo) / 7.0) as i32).max(1) + 1;
            for (lane, y, d) in [(0, 16.0, 1.0f32), (1, 32.0, -1.0)] {
                for k in 0..n {
                    let x = if d > 0.0 {
                        lo + 7.0 * k as f32
                    } else {
                        hi - 7.0 * k as f32
                    };
                    let i = lane * n + k;
                    let ink = step_ink(i, 2 * n - 1, fade);
                    step(
                        p,
                        at(i, x, y),
                        (d, 0.0),
                        if k % 2 == 0 { 1 } else { -1 },
                        ink,
                        style,
                    );
                }
            }
            // The turns at each end.
            p.line(r(hi) + 2, 16, r(hi) + 4, 24, INK2);
            p.line(r(hi) + 4, 24, r(hi) + 2, 32, INK2);
            p.line(r(lo) - 2, 32, r(lo) - 4, 24, INK2);
            p.line(r(lo) - 4, 24, r(lo) - 2, 16, INK2);
            if w.tron == Tron::On {
                tape_end(p, r(hi) + 6, 24);
            }
        }
    }
}

/// The pedestrian figure, mid-stride, facing right: 10 × 16.
const WALKER: [&[u8; 10]; 16] = [
    b"....##....",
    b"....##....",
    b"..........",
    b"...####...",
    b"..#.##.#..",
    b".#..##..#.",
    b".#..##...#",
    b"....##....",
    b"....##....",
    b"...#..#...",
    b"...#...#..",
    b"..#....#..",
    b"..#.....#.",
    b".#......#.",
    b".#.......#",
    b"##.......#",
];

/// DON'T WALK's raised hand: 8 × 10.
const HAND: [&[u8; 8]; 10] = [
    b"..#.#...",
    b"..#.#.#.",
    b"#.#.#.#.",
    b"#.#.#.#.",
    b"#.#####.",
    b"#######.",
    b"######..",
    b".#####..",
    b"..###...",
    b"..###...",
];

/// The countdown's digits, 3 × 5, a bit a pixel row by row.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// `rows` at (`x`, `y`), mirrored left to right if `back`.
fn sprite<const N: usize>(
    p: &mut Pix,
    rows: &[&[u8; N]],
    (x, y): (i32, i32),
    back: bool,
    ink: Ink,
) {
    for (j, row) in rows.iter().enumerate() {
        for (i, &c) in row.iter().enumerate() {
            if c == b'#' {
                let i = if back { N - 1 - i } else { i };
                p.px(x + i as i32, y + j as i32, ink);
            }
        }
    }
}

/// `n` (0 to 99) in the 3 × 5 digits at (`x`, `y`), two places.
fn digits(p: &mut Pix, n: u32, (x, y): (i32, i32), ink: Ink) {
    for (k, d) in [n / 10 % 10, n % 10].into_iter().enumerate() {
        for (j, bits) in DIGITS[d as usize].iter().enumerate() {
            for i in 0..3 {
                if bits & (4 >> i) != 0 {
                    p.px(x + 4 * k as i32 + i, y + j as i32, ink);
                }
            }
        }
    }
}

/// The countdown WALK's signal and treadmill read: LENGTH's tape in
/// seconds with TRON on, else LENGTH in hundredths.
fn countdown(w: Steps) -> u32 {
    if w.tron == Tron::On {
        libm::ceilf(tron::TapeSeconds::of_length(w.length.unit()).get()) as u32
    } else {
        libm::roundf(w.length.unit() * 99.0) as u32
    }
}

/// A turning-back mark over (`x`, `y`): an arc and its head, `back` the
/// way it ends.
fn turn_mark(p: &mut Pix, x: i32, y: i32, ink: Ink) {
    p.line(x, y + 3, x + 2, y, ink);
    p.rect(x + 2, y, 6, 1, ink);
    p.line(x + 8, y, x + 10, y + 3, ink);
    p.arrow(x - 1, y + 3, -1, ink);
}

/// WALK as a WALK / DON'T WALK signal: the lit figure strides FORWARD's
/// way or REVERSE's (mirrored), a loop arrow rings it on LOOP, a turning
/// mark crowns it on PING-PONG. DRIFT nudges it off its place by a fixed
/// step; the countdown reads LENGTH; with TRON on, the raised hand waits
/// beside it for the tape's end.
#[inline(never)]
fn signal(p: &mut Pix, w: Steps) {
    // The signal head: a dark box, the figure's lamp and the countdown's.
    p.frame(2, 2, 60, 44, INK2);
    p.frame(4, 4, 34, 40, FAINT);
    p.frame(40, 4, 20, 40, FAINT);
    let d = w.direction();
    let back = d == Direction::Reverse;
    let (sx, _) = scatter(i32::from(w.drift.step()));
    let nudge = r(sx * w.drift.unit() * 6.0);
    let (fx, fy) = (16 + nudge, 18);
    sprite(p, &WALKER, (fx, fy), back, ACC);
    match d {
        Direction::Loop => {
            for i in 0..40 {
                let a = TAU * i as f32 / 40.0;
                if i % 10 != 9 {
                    p.px(
                        fx + 5 + r(13.0 * cos(a)),
                        fy + 8 - r(13.0 * fast_sin(a)),
                        MID,
                    );
                }
            }
            p.arrow(fx + 5 + 13, fy + 8, -1, INK2);
        }
        Direction::Ping => turn_mark(p, fx, fy - 8, INK2),
        _ => p.arrow(
            if back { fx - 3 } else { fx + 12 },
            fy + 8,
            if back { -1 } else { 1 },
            INK2,
        ),
    }
    // The countdown, and DON'T WALK's hand when the tape can run out.
    digits(
        p,
        countdown(w),
        (44, 8),
        if w.tron == Tron::On { INK } else { MID },
    );
    if w.tron == Tron::On {
        sprite(p, &HAND, (46, 24), false, INK);
    } else {
        p.rect(45, 26, 10, 10, FAINT);
    }
    // FADE dims the lamp's last rows, as the signal flickers out.
    let faded = libm::roundf(w.fade.unit() * 4.0) as i32;
    for k in 0..faded {
        p.rect(6, 41 - k, 30, 1, BG);
    }
}

/// WALK on a treadmill: the figure walks the belt, the belt's chevrons
/// run under it the DIRECTION's way (round on LOOP, both ways on
/// PING-PONG); POSITION and LENGTH light the belt's run; DRIFT wobbles
/// the figure along it; TRON lights the stop button on the console and
/// marks the belt's end of tape.
#[inline(never)]
fn treadmill(p: &mut Pix, w: Steps) {
    let (pos, len) = (w.position.unit(), w.length.unit());
    let d = w.direction();
    let back = d == Direction::Reverse;
    // The console post and its button.
    p.rect(55, 8, 2, 28, INK2);
    p.rect(50, 6, 10, 4, INK2);
    if w.tron == Tron::On {
        p.dot(55, 3, 2, ACC);
        p.px(55, 3, INK);
    } else {
        p.ring(55, 3, 2, MID);
    }
    // The deck: rollers and the belt, its run lit.
    let (x0, x1, yb) = (6, 52, 38);
    p.ring(x0, yb + 2, 2, INK2);
    p.ring(x1, yb + 2, 2, INK2);
    p.rect(x0, yb, x1 - x0, 1, MID);
    p.rect(x0, yb + 4, x1 - x0, 1, MID);
    let from = x0 as f32 + pos * (x1 - x0) as f32;
    let to = from + lvis(len) * (x1 as f32 - from);
    p.rect(r(from), yb + 1, (r(to) - r(from)).max(2), 3, SOFT);
    // The chevrons: on top the way it runs; LOOP's come back below.
    let chevron = |p: &mut Pix, x: i32, y: i32, dir: i32| {
        p.line(x, y - 1, x + dir, y, INK);
        p.line(x + dir, y, x, y + 1, INK);
    };
    let run = if back { 1 } else { -1 };
    for k in 0..5 {
        let x = x0 + 5 + 9 * k;
        match d {
            Direction::Ping => chevron(p, x, yb + 2, if k % 2 == 0 { 1 } else { -1 }),
            _ => chevron(p, x, yb + 2, run),
        }
        if d == Direction::Loop {
            chevron(p, x + 4, yb + 7, -run);
        }
    }
    if d == Direction::Loop {
        p.line(x0 - 3, yb + 1, x0 - 3, yb + 6, MID);
        p.line(x1 + 3, yb + 1, x1 + 3, yb + 6, MID);
    }
    // TRON: the tape's end on the belt, LENGTH's share of 8 s.
    if w.tron == Tron::On {
        let e = x0 + r(tron::TapeSeconds::of_length(len).get() / TRON_LEN_MAX_S * (x1 - x0) as f32);
        p.rect(e, yb - 2, 1, 9, INK);
        p.px(e + 1, yb - 2, INK);
        p.px(e + 1, yb, INK);
    }
    // The walker, DRIFT wobbling its place and its stride.
    let (sx, sy) = scatter(i32::from(w.drift.step()));
    let wob = w.drift.unit();
    let fx = 22 + r(sx * wob * 8.0);
    let fy = yb - 17 + r(sy * wob * 1.5);
    let turned = back || (d == Direction::Ping && w.drift.step() % 2 == 1);
    sprite(p, &WALKER, (fx, fy), turned, ACC);
    if d == Direction::Ping {
        turn_mark(p, fx, fy - 6, INK2);
    }
    // FADE: the belt's far end fades.
    let faded = libm::roundf(w.fade.unit() * 6.0) as i32;
    for k in 0..faded {
        p.px(x1 - 2 - 2 * k, yb + 2, FAINT);
    }
}
