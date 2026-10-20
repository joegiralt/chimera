//! The glyphs' raster: `render`, a pure function of a `TapeGauge` into a
//! `Canvas`. With `bank2` this module is bank 2's (`bank2.x`), reached
//! only through the door (`super::paint`).
//!
//! The drawings and their primitives follow the owner's mockup
//! (`.superpowers/ledgers/tape-glyphs/`): rows, Bresenham lines, discs of
//! `r² + r`, then the wear passes.

use super::{
    Bellows, Blind, Canvas, Film, H, Ink, Kind, Loop, Machine, Q, Q3, Q4, Q5, Q6, Run, Sponge,
    TapeGauge, W, Wear,
};
use crate::dsp::algo::math::clampf;
use crate::dsp::fast_sin;
use core::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

const BG: Ink = Ink::Bg;
const INK: Ink = Ink::Ink;
const INK2: Ink = Ink::Ink2;
const MID: Ink = Ink::Mid;
const FAINT: Ink = Ink::Faint;
const ACC: Ink = Ink::Accent;
const SOFT: Ink = Ink::Soft;

/// From `TAPE_GLYPH_PARAMS`' set values, normalized: MODEL, START, END,
/// POSITION, MOTION, SIZE, DRAG, TAPE, AGE, XFADE. `still` for MODEL's
/// cell, which shows the machine, not its knobs. A machine reads only
/// what it plays (`reads`), so a param it ignores leaves the glyph as it
/// was.
pub fn inputs(set: [f32; 10], still: bool) -> TapeGauge {
    let [model, start, end, pos, motion, size, drag, tape, age, xfade] = set;
    let pick = |v: f32, n: u8| libm::roundf(clampf(v, 0.0, 1.0) * n as f32) as u8;
    let kind = Kind::from_index(pick(model, Kind::ALL.len() as u8 - 1));
    if still {
        return TapeGauge::still(kind);
    }
    let (q6, q5) = (Q6::of, Q5::of);
    let machine = match kind {
        Kind::One => Machine::One(Run {
            start: q6(start),
            end: q6(end),
        }),
        Kind::Loop | Kind::Ping => {
            let l = Loop {
                position: q6(pos),
                length: q6(size),
                walk: q6(motion),
                // PING has no crossfade.
                xfade: Q3::of(if kind == Kind::Loop { xfade } else { 0.0 }),
            };
            if kind == Kind::Loop {
                Machine::Loop(l)
            } else {
                Machine::Ping(l)
            }
        }
        Kind::Tron => Machine::Tron(Blind {
            length: q6(size),
            fade: q5(drag),
        }),
        Kind::Stretch => Machine::Stretch(Bellows {
            speed: q6(motion),
            d_time: q5(size),
        }),
        Kind::Wave => Machine::Wave(Film {
            frame: q6(pos),
            range: q6(size),
            scan: q5(motion),
        }),
        Kind::Scrub => Machine::Scrub(Sponge {
            finger: q6(pos),
            speed: Q4::of(motion),
            length: q5(size),
            drag: Q4::of(drag),
            xfade: Q3::of(xfade),
        }),
    };
    TapeGauge {
        machine,
        wear: Wear {
            tape: Q::of(tape),
            age: Q::of(age),
        },
    }
}

/// `g` into `out`, from the ground up: the machine, then TAPE's bend,
/// AGE's grain on the ground and its dropouts.
#[inline(never)]
pub fn render(g: &TapeGauge, out: &mut Canvas) {
    let mut p = Pix(Canvas::new());
    match g.machine {
        Machine::One(r) => one(&mut p, r),
        Machine::Loop(l) => wheel(&mut p, l),
        Machine::Ping(l) => pong(&mut p, l),
        Machine::Tron(b) => blind(&mut p, b),
        Machine::Stretch(b) => accordion(&mut p, b),
        Machine::Wave(f) => film(&mut p, f),
        Machine::Scrub(s) => sponge(&mut p, s),
    }
    wear(&p.0, g.wear.tape.unit(), g.wear.age.unit(), out);
}

// ── Wear ─────────────────────────────────────────────────────────────

/// TAPE's widest bend, px.
const WOW_PX: f32 = 1.6;
/// The dropouts' rows and heights: the first above AGE ½, all three at 1.
const DROPOUTS: [(i32, i32); 3] = [(31, 2), (12, 3), (40, 2)];

/// A pixel's grain draw, 0..1: a fixed hash of where it is.
fn grain_at(i: usize) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9e37_79b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65_536.0
}

/// TAPE bends each row by a fixed sine of the row, up to `WOW_PX` at 1:
/// frozen, not running. AGE adds grain on the ground and, above ½, dims
/// rows as dropouts.
#[inline(never)]
fn wear(b: &Canvas, tape: f32, age: f32, out: &mut Canvas) {
    for y in 0..H {
        let sh = libm::roundf(tape * WOW_PX * fast_sin(y as f32 * 0.42 + 1.3)) as i32;
        for x in 0..W {
            let sx = x - sh;
            out.0[(y * W + x) as usize] = if (0..W).contains(&sx) {
                b.at(sx, y)
            } else {
                BG
            };
        }
    }
    if age <= 0.02 {
        return;
    }
    for (i, c) in out.0.iter_mut().enumerate() {
        let v = grain_at(i);
        if *c == BG && v < age * 0.07 {
            *c = if v < age * 0.018 { MID } else { FAINT };
        }
    }
    if age > 0.5 {
        let n = (libm::ceilf((age - 0.5) * 6.0) as usize).min(DROPOUTS.len());
        for &(row, h) in &DROPOUTS[..n] {
            for c in &mut out.0[(row * W) as usize..((row + h) * W) as usize] {
                *c = c.dim();
            }
        }
    }
}

// ── Primitives ───────────────────────────────────────────────────────

struct Pix(Canvas);

impl Pix {
    #[inline(never)]
    fn px(&mut self, x: i32, y: i32, c: Ink) {
        if (0..W).contains(&x) && (0..H).contains(&y) {
            self.0.0[(y * W + x) as usize] = c;
        }
    }

    #[inline(never)]
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Ink) {
        for yy in y.max(0)..(y + h).min(H) {
            for xx in x.max(0)..(x + w).min(W) {
                self.0.0[(yy * W + xx) as usize] = c;
            }
        }
    }

    fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, c: Ink) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x + w - 1, y, 1, h, c);
    }

    /// Bresenham, both ends drawn.
    #[inline(never)]
    fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, c: Ink) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut e = dx + dy;
        loop {
            self.px(x0, y0, c);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * e;
            if e2 >= dy {
                e += dy;
                x0 += sx;
            }
            if e2 <= dx {
                e += dx;
                y0 += sy;
            }
        }
    }

    /// The pixels within `r² + r` of the centre, or (`ring`) outside
    /// `(r−1)² + (r−1)` too.
    #[inline(never)]
    fn disc(&mut self, cx: i32, cy: i32, r: i32, ring: bool, c: Ink) {
        let (out, ri) = (r * r + r, (r - 1) * (r - 1) + r - 1);
        for dy in -r..=r {
            for dx in -r..=r {
                let d = dx * dx + dy * dy;
                if d <= out && !(ring && d <= ri) {
                    self.px(cx + dx, cy + dy, c);
                }
            }
        }
    }

    fn dot(&mut self, cx: i32, cy: i32, r: i32, c: Ink) {
        self.disc(cx, cy, r, false, c);
    }

    fn ring(&mut self, cx: i32, cy: i32, r: i32, c: Ink) {
        self.disc(cx, cy, r, true, c);
    }

    /// A filled ellipse, radii `rx`, `ry`.
    #[inline(never)]
    fn oval(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, c: Ink) {
        let (kx, ky) = ((rx * rx) as f32 + 0.5, (ry * ry) as f32 + 0.5);
        for dy in -ry..=ry {
            for dx in -rx..=rx {
                if (dx * dx) as f32 / kx + (dy * dy) as f32 / ky <= 1.0 {
                    self.px(cx + dx, cy + dy, c);
                }
            }
        }
    }

    /// A triangle pointing up from its base row `y0` (`x0..=x1`) to its
    /// tip (`tx`, `ty`), filled a row at a time.
    #[inline(never)]
    fn spike(&mut self, x0: i32, x1: i32, y0: i32, tx: i32, ty: i32, c: Ink) {
        let h = (y0 - ty) as f32;
        for y in ty..=y0 {
            let t = (y - ty) as f32 / h;
            let l = r(tx as f32 + (x0 - tx) as f32 * t);
            let rr = r(tx as f32 + (x1 - tx) as f32 * t);
            self.rect(l, y, rr - l + 1, 1, c);
        }
    }

    /// A 3 × 5 arrowhead pointing `dir` (±1) along the row from (`x`, `y`).
    fn arrow(&mut self, x: i32, y: i32, dir: i32, c: Ink) {
        for i in 0..3 {
            self.rect(x + dir * i, y - 2 + i, 1, 5 - 2 * i, c);
        }
    }

    /// A 5 × 3 arrowhead pointing up from (`x`, `y`).
    fn arrow_up(&mut self, x: i32, y: i32, c: Ink) {
        for i in 0..3 {
            self.rect(x - 2 + i, y - i, 5 - 2 * i, 1, c);
        }
    }
}

fn r(v: f32) -> i32 {
    libm::roundf(v) as i32
}

fn cos(a: f32) -> f32 {
    fast_sin(a + FRAC_PI_2)
}

/// SIZE on the glyph: linear in the knob (the real length is
/// exponential), so it reads at 64 px; the strip and the value show it.
fn lvis(size: f32) -> f32 {
    0.06 + 0.94 * size
}

/// The loop in the 60 px window: LENGTH's span at POSITION of the room.
fn loop_span(pos: f32, size: f32) -> (f32, f32) {
    let l = lvis(size) * 60.0;
    let lo = 2.0 + pos * (60.0 - l);
    (lo, lo + l)
}

/// XFADE's share of a loop `len` px long, px: up to half of it.
fn fade_px(len: i32, xfade: f32) -> i32 {
    r(len as f32 * 0.5 * xfade)
}

/// Every other column of `x0..x1` back to `ink`: XFADE's dashed end, the
/// last column kept.
fn dash(p: &mut Pix, x0: i32, x1: i32, y: i32, h: i32, ink: Ink) {
    for x in (x0..x1 - 1).rev().step_by(2) {
        p.rect(x, y, 1, h, ink);
    }
}

/// The window along the floor at `y`, the loop `lo..hi` lit, its last
/// `fade` px (XFADE) dashed.
#[inline(never)]
fn window_strip(p: &mut Pix, lo: i32, hi: i32, y: i32, fade: i32) {
    p.rect(2, y, 60, 2, FAINT);
    p.rect(lo, y, (hi - lo).max(2), 2, ACC);
    dash(p, hi - fade, hi, y, 2, FAINT);
    p.rect(lo, y - 2, 1, 4, ACC);
    p.rect((lo + 1).max(hi - 1), y - 2, 1, 4, ACC);
}

// ── The machines ─────────────────────────────────────────────────────

/// ONE: a match with 16 notches, struck once and spent. The flame sits at
/// START, the stick before it charred, and the run to END lit under it;
/// played backward (END before START) it burns from the far end. The
/// notches are the stick's, not slices: SLICE is retired, KIT slices.
#[inline(never)]
fn one(p: &mut Pix, run: Run) {
    let (x0, x1, y) = (7, 59, 30);
    let at = |u: f32| r(x0 as f32 + u * (x1 - x0) as f32);
    let (s, e) = (at(run.start.unit()), at(run.end.unit()));
    let (a, z) = (s.min(e), s.max(e));
    let back = e < s;
    let charred = if back { z + 1..x1 + 1 } else { x0..a };
    for x in charred {
        p.px(x, y + ((x >> 2) & 1), MID);
    }
    if back {
        p.rect(x0, y, z - x0 + 1, 2, INK2);
    } else {
        p.rect(a, y, x1 - a + 1, 2, INK2);
    }
    for k in 0..=16 {
        let h = if k % 4 == 0 { 3 } else { 2 };
        p.rect(
            x0 + r(k as f32 * (x1 - x0) as f32 / 16.0),
            y + 4,
            1,
            h,
            FAINT,
        );
    }
    p.rect(a, y + 8, (z - a).max(2), 2, ACC);
    p.oval(x0 - 3, y, 3, 2, MID);
    p.oval(s, y - 3, 3, 3, ACC);
    p.spike(s - 3, s + 3, y - 4, s + 1, y - 15, ACC);
    p.oval(s, y - 4, 1, 2, INK);
}

/// LOOP: a hamster wheel on the lit loop. LENGTH sizes the wheel (10 to
/// 15 px, so it has room to roll), POSITION rolls it along the window,
/// its spokes turning as it goes, even where LENGTH leaves the loop no
/// room (the strip shows that); XFADE dashes the loop's end; WALK turns
/// the hamster to the drift and adds speed lines; at 0 it sits.
#[inline(never)]
fn wheel(p: &mut Pix, l: Loop) {
    let (size, m) = (l.length.unit(), l.walk.bi());
    let (lo, hi) = loop_span(l.position.unit(), size);
    let (lo, hi) = (r(lo), r(hi));
    window_strip(p, lo, hi, 45, fade_px(hi - lo, l.xfade.unit()));
    let rad = r(10.0 + 5.0 * size);
    let (left, right) = (rad + 1, 62 - rad);
    let cx = left + r(l.position.unit() * (right - left) as f32);
    let cy = 41 - rad;
    // Rolled: a turn of the spokes for every px along.
    let spin = 0.2 + cx as f32 / rad as f32;
    let leg = rad as f32 * 0.6;
    p.line(cx, cy, r(cx as f32 - leg), 42, FAINT);
    p.line(cx, cy, r(cx as f32 + leg), 42, FAINT);
    p.ring(cx, cy, rad, INK2);
    if rad > 9 {
        p.ring(cx, cy, rad - 2, MID);
    }
    let out = (rad - 3) as f32;
    for i in 0..8 {
        let a = spin + i as f32 * FRAC_PI_4;
        let (co, si) = (cos(a), fast_sin(a));
        p.line(
            cx + r(3.0 * co),
            cy + r(3.0 * si),
            cx + r(out * co),
            cy + r(out * si),
            FAINT,
        );
    }
    p.dot(cx, cy, 1, INK2);
    let d = if m < 0.0 { -1 } else { 1 };
    let run = libm::fabsf(m) > 0.04;
    let (hy, bx) = (cy + rad - 7, cx - d);
    p.oval(bx, hy, 5, if run { 3 } else { 4 }, ACC);
    p.dot(bx + d * 5, hy - 1, 3, ACC);
    p.dot(bx + d * 3, hy - 5, 1, ACC);
    p.px(bx + d * 3, hy - 5, INK);
    p.px(bx + d * 6, hy - 2, BG);
    p.px(bx + d * 8, hy - 1, INK);
    p.px(bx - d * 6, hy - 2, ACC);
    if run {
        p.line(bx + d * 3, hy + 3, bx + d * 5, hy + 5, ACC);
        p.line(bx - d * 2, hy + 3, bx - d * 4, hy + 5, ACC);
        let n = (libm::ceilf(libm::fabsf(m) * 3.0) as i32).clamp(1, 3);
        let len = 2 + r(5.0 * libm::fabsf(m));
        let x = if d > 0 { cx - rad - 3 } else { cx + rad + 3 };
        for i in 0..n {
            let y = cy - 5 + i * 5;
            p.line(x, y, x - d * len, y, MID);
        }
    } else {
        p.rect(bx - 3, hy + 4, 2, 1, ACC);
        p.rect(bx + 2, hy + 4, 2, 1, ACC);
    }
}

/// PING: Pong. LENGTH spaces the paddles (the loop, lit on the floor),
/// POSITION slides the court along the window, WALK tilts the ball's path
/// and lengthens its trail.
#[inline(never)]
fn pong(p: &mut Pix, l: Loop) {
    let walk = l.walk.bi();
    let (lo, hi) = loop_span(l.position.unit(), l.length.unit());
    let g = (hi - lo).max(12.0);
    let cx = (lo + hi) / 2.0;
    let (left, right) = (r(cx - g / 2.0), r(cx + g / 2.0) - 2);
    p.rect(2, 46, 60, 1, FAINT);
    p.rect(left, 46, right - left + 2, 1, ACC);
    let c = r(cx);
    for y in (2..44).step_by(4) {
        p.rect(c - 1, y, 1, 2, FAINT);
    }
    // The score: two 3 × 5 zeros.
    p.frame(c - 7, 3, 3, 5, MID);
    p.frame(c + 4, 3, 3, 5, MID);
    p.rect(left, 19, 2, 10, INK2);
    p.rect(right, 19, 2, 10, INK2);
    let a = walk * 0.75;
    let (co, si) = (cos(a), fast_sin(a));
    let (bx, by) = (r(cx + g * 0.18), r(24.0 - si * 3.0));
    let n = 2 + r(libm::fabsf(walk) * 3.0);
    for i in (1..=n).rev() {
        let k = (i * 4) as f32;
        let ink = if i == 1 { MID } else { FAINT };
        p.rect(r(bx as f32 - k * co), r(by as f32 + k * si), 2, 2, ink);
    }
    p.rect(bx, by, 2, 2, ACC);
}

/// TRON: a window blind on a spring, pulled down while a key is held. It
/// stops at LENGTH (0.25 to 8 s, drawn as its share of 8 s) and snaps up
/// on release at FADE: solid with speed lines fast, dotted slow.
#[inline(never)]
fn blind(p: &mut Pix, b: Blind) {
    // `tron::tron_len_s` over its 8 s: 2^(5·size − 5).
    let frac = crate::dsp::algo::math::exp2(5.0 * b.length.unit() - 5.0);
    let (x0, x1) = (8, 50);
    let yb = r(9.0 + 30.0 * frac);
    p.frame(x0 + 2, 9, x1 - x0 - 4, 33, FAINT);
    p.rect((x0 + x1) / 2, 9, 1, 33, FAINT);
    p.rect(x0 + 2, 25, x1 - x0 - 4, 1, FAINT);
    p.rect(x0 - 2, 42, x1 - x0 + 4, 2, MID);
    p.rect(x0, 9, x1 - x0, yb - 9, SOFT);
    p.rect(x0, 9, 1, yb - 9, ACC);
    p.rect(x1 - 1, 9, 1, yb - 9, ACC);
    for y in (13..yb - 1).step_by(4) {
        p.rect(x0 + 1, y, x1 - x0 - 2, 1, MID);
    }
    p.rect(x0 - 1, yb, x1 - x0 + 2, 2, ACC);
    let mx = (x0 + x1) / 2;
    p.rect(mx, yb + 2, 1, 3, INK2);
    p.dot(mx, yb + 6, 1, INK);
    p.rect(x0 - 3, 3, x1 - x0 + 6, 6, INK2);
    p.rect(x0 - 4, 4, 1, 4, MID);
    p.rect(x1 + 3, 4, 1, 4, MID);
    for i in 0..4 {
        p.px(x1 - 4 - i * 2, 5 + (i % 2) * 2, BG);
    }
    // The snap back, upward.
    let fade = b.fade.unit();
    let (x, top) = (57, 10);
    let gap = r(fade * 3.0);
    for (i, y) in (top + 3..=yb + 2).rev().enumerate() {
        if gap == 0 || i as i32 % (gap + 1) == 0 {
            p.px(x, y, INK);
        }
    }
    p.arrow_up(x, top, INK);
    if fade < 0.35 {
        p.rect(x - 3, top + 4, 1, 3, MID);
        p.rect(x + 3, top + 5, 1, 4, MID);
    }
}

/// STRETCH: an accordion under a note. SPEED (−1 to +2 times) opens the
/// bellows: wide with pause bars at 0, natural at 1×, squeezed at 2×, the
/// chevrons back below 0. D-TIME folds them: short splices, fine folds.
/// The note never moves (the pitch).
#[inline(never)]
fn accordion(p: &mut Pix, b: Bellows) {
    // `stretch::time_speed`.
    let m = b.speed.bi();
    let sp = crate::dsp::tape_engine::stretch::time_speed(m);
    let w = r(56.0 - 19.0 * libm::fabsf(sp));
    let n = r(3.0 + 9.0 * (1.0 - b.d_time.unit())).max(1);
    let (x0, x1, top, bot) = (32 - (w >> 1), 32 + (w >> 1), 20, 40);
    p.rect(x0, top + 2, x1 - x0, bot - top - 4, SOFT);
    let xs = |i: i32| x0 + r((x1 - x0) as f32 * i as f32 / n as f32);
    let inset = |i: i32| if i % 2 == 1 { 3 } else { 0 };
    for i in 0..=n {
        p.line(xs(i), top + inset(i), xs(i), bot - inset(i), ACC);
    }
    for i in 0..n {
        let (a, z) = (xs(i), xs(i + 1));
        p.line(a, top + inset(i), z, top + inset(i + 1), ACC);
        p.line(a, bot - inset(i), z, bot - inset(i + 1), ACC);
    }
    p.rect(x0 - 4, top - 2, 4, bot - top + 5, INK2);
    p.rect(x1 + 1, top - 2, 4, bot - top + 5, INK2);
    for j in 0..4 {
        p.px(x0 - 3, top + 2 + 5 * j, BG);
        p.px(x0 - 2, top + 4 + 5 * j, BG);
    }
    // The note over a faint staff line.
    let (nx, ny) = (30, 11);
    p.oval(nx, ny, 3, 2, INK);
    p.rect(nx + 3, ny - 9, 1, 9, INK);
    p.line(nx + 4, ny - 8, nx + 6, ny - 5, INK);
    p.line(nx + 4, ny - 7, nx + 6, ny - 4, INK);
    for i in (2..62).step_by(4) {
        if (i - nx - 1).abs() > 7 {
            p.rect(i, ny, 2, 1, FAINT);
        }
    }
    // Time's way under it: pause bars at a standstill, else up to four
    // chevrons.
    let (cx, y) = (32, 45);
    if libm::fabsf(sp) < 0.02 {
        p.rect(cx - 3, y - 2, 2, 5, MID);
        p.rect(cx + 1, y - 2, 2, 5, MID);
        return;
    }
    let k = (libm::ceilf(libm::fabsf(sp) * 2.0) as i32).clamp(1, 4);
    let d = if sp < 0.0 { -1 } else { 1 };
    for i in 0..k {
        let x = cx + d * r(i as f32 * 5.0 - (k - 1) as f32 * 2.5) - d;
        p.arrow(x, y, d, ACC);
    }
}

/// A frame's cycle at `u` (0..1), −1..1: five shapes, frame by frame.
#[inline(never)]
fn shape(u: f32, k: i32) -> f32 {
    let u = u - libm::floorf(u);
    match k.rem_euclid(5) {
        0 => fast_sin(TAU * u),
        1 if u < 0.25 => 4.0 * u,
        1 if u < 0.75 => 2.0 - 4.0 * u,
        1 => 4.0 * u - 4.0,
        2 => 1.0 - 2.0 * u,
        3 if u < 0.5 => 0.85,
        3 => -0.85,
        _ => 0.7 * fast_sin(TAU * u) + 0.35 * fast_sin(3.0 * TAU * u),
    }
}

/// TRANSWAVE: film through a gate, a wave cycle a frame. FRAME is the
/// frame in the gate (fractional, so the strip slides), RANGE opens the
/// gate over the frames the scan reaches, SCAN's rate is the triangle
/// under it, more teeth the faster.
#[inline(never)]
fn film(p: &mut Pix, f: Film) {
    const N: i32 = 16;
    const FW: i32 = 10;
    let last = (N - 1) as f32;
    let (at, half) = (f.frame.unit() * last, f.range.unit() * last);
    let x_of = |k: f32| 32.0 + (k - at) * FW as f32;
    p.rect(0, 11, 64, 26, FAINT);
    let mut x = r(x_of(-1.0)) % 5 - 5;
    while x < 64 {
        p.rect(x, 12, 2, 2, BG);
        p.rect(x, 34, 2, 2, BG);
        x += 5;
    }
    let lo = clampf(at - half, 0.0, last);
    let hi = clampf(at + half, 0.0, last);
    let here = r(at);
    for k in 0..N {
        let fx = r(x_of(k as f32) - FW as f32 / 2.0);
        if fx > 64 || fx + FW < 0 {
            continue;
        }
        let scanned = k >= libm::floorf(lo) as i32 && k <= libm::ceilf(hi) as i32;
        let (fill, ink) = match (k == here, scanned) {
            (true, _) => (SOFT, ACC),
            (false, true) => (BG, INK2),
            (false, false) => (BG, MID),
        };
        p.rect(fx + 1, 16, FW - 2, 16, fill);
        let y_at = |i: i32| 24 - r(5.0 * shape(i as f32 / (FW - 3) as f32, k));
        for i in 1..FW - 2 {
            p.line(fx + i, y_at(i - 1), fx + 1 + i, y_at(i), ink);
        }
    }
    let gl = r(clampf(x_of(lo) - FW as f32 / 2.0 - 1.0, 0.0, 63.0));
    let gr = r(clampf(x_of(hi) + FW as f32 / 2.0, 0.0, 63.0));
    for (x, d) in [(gl, 1), (gr, -1)] {
        let x4 = x.min(x + d * 3);
        p.rect(x4, 8, 4, 1, INK);
        p.rect(x, 8, 1, 4, INK);
        p.rect(x4, 39, 4, 1, INK);
        p.rect(x, 36, 1, 4, INK);
    }
    let teeth = r(libm::fabsf(f.scan.bi()) * 6.0);
    let (ax, aw) = (gl + 2, (gr - gl - 4).max(6));
    if teeth == 0 || half < 0.05 {
        p.rect(ax, 44, aw, 1, MID);
        return;
    }
    let y_at = |i: i32| {
        let t = i as f32 / aw as f32 * teeth as f32;
        let t = t - libm::floorf(t);
        r(44.0 - 3.0 * (1.0 - libm::fabsf(2.0 * t - 1.0)) + 1.5)
    };
    for i in 1..=aw {
        p.line(ax + i - 1, y_at(i - 1), ax + i, y_at(i), ACC);
    }
}

/// SCRUB's bubbles round the sponge: (dx, dy, radius), foamed in order,
/// clear of SPEED's arrow (dx −4..4, dy −6..−2).
const BUBBLES: [(i8, i8, i8); 10] = [
    (-9, -4, 1),
    (9, -6, 2),
    (-2, -11, 2),
    (12, -14, 1),
    (-12, -15, 3),
    (3, -19, 1),
    (9, -23, 2),
    (-6, -25, 1),
    (0, -30, 2),
    (-14, -27, 1),
];

/// The sponge's pores.
const PORES: [(i8, i8); 9] = [
    (3, 3),
    (8, 2),
    (14, 4),
    (18, 2),
    (5, 6),
    (11, 6),
    (17, 7),
    (2, 8),
    (20, 5),
];

/// SCRUB: a sponge on the tape. FINGER moves it (DRAG is the jog's own
/// lag, in the value itself), LENGTH is the loop outlined under it (its
/// tape hollow, so it reads beside the sponge's own accent), XFADE dashes
/// its end, DRAG leaves a wet trail behind the way it plays, and bubbles
/// foam with |SPEED|: at a standstill there are none, and the tape is
/// silent. SPEED's arrow sits over the sponge, drawn last, so nothing
/// hides it.
#[inline(never)]
fn sponge(p: &mut Pix, s: Sponge) {
    let m = s.speed.bi();
    let sx = r(14.0 + s.finger.unit() * 36.0);
    let lw = r(lvis(s.length.unit()) * 30.0).max(3);
    let (end, fade) = (sx + lw, fade_px(lw, s.xfade.unit()));
    let trail = r(s.drag.unit() * 14.0);
    let ahead = m >= 0.0;
    let amp = |x: i32| {
        let xf = x as f32;
        1 + r(3.0 * libm::fabsf(fast_sin(xf * 0.71) * fast_sin(xf * 0.23 + 1.0)))
    };
    for x in 1..63 {
        let a = amp(x);
        let wet = if ahead {
            x < sx - 10 && x >= sx - 10 - trail
        } else {
            x > sx + 10 && x <= sx + 10 + trail
        };
        if (sx..end).contains(&x) {
            // The run's outline: its ends whole, each edge down to its
            // lower neighbour's so the line joins, the inside open.
            if x == sx || x == end - 1 {
                p.rect(x, 42 - a, 1, 2 * a + 1, ACC);
            } else {
                let b = amp(x - 1).min(amp(x + 1)).min(a);
                p.rect(x, 42 - a, 1, a - b + 1, ACC);
                p.rect(x, 42 + b, 1, a - b + 1, ACC);
            }
        } else {
            p.rect(x, 42 - a, 1, 2 * a + 1, if wet { FAINT } else { MID });
        }
        if wet && x % 2 == 1 {
            p.px(x, 47, MID);
        }
    }
    dash(p, end - fade, end, 38, 9, BG);
    let (l, t) = (sx - 11, 21);
    p.rect(l + 1, t, 21, 14, BG);
    p.frame(l, t, 23, 14, INK2);
    p.rect(l + 1, t + 10, 21, 4, ACC);
    p.rect(l + 1, t + 9, 21, 1, INK2);
    for (x, y) in PORES {
        p.px(l + x as i32, t + y as i32, MID);
    }
    let nb = r(clampf(libm::fabsf(m), 0.0, 1.0) * BUBBLES.len() as f32) as usize;
    for &(dx, dy, rad) in &BUBBLES[..nb] {
        let (x, y) = ((sx + dx as i32).clamp(2, 61), t + dy as i32 + 1);
        p.ring(x, y, rad as i32, INK2);
        if rad > 1 {
            p.px(x - 1, y - 1, INK);
        }
    }
    // SPEED's way: a 9 px arrow centred over the sponge, clear of the
    // bubbles (none sits in its rows) and drawn over them anyway.
    if libm::fabsf(m) > 0.04 {
        let (d, y) = (if ahead { 1 } else { -1 }, t - 4);
        p.rect(if ahead { sx - 4 } else { sx - 1 }, y, 6, 1, ACC);
        p.arrow(sx + 2 * d, y, d, ACC);
    }
}
