//! WALK as little people running a track: the owner's mockups
//! (`.superpowers/ledgers/walk-glyph/`). Runners stand on the track,
//! side on, the way they run; DIRECTION sets which way and where, POSITION
//! and LENGTH light the stretch they run, DRIFT jitters their spacing and
//! strides, FADE dims the last of them, and TRON's tape is a finish tape the
//! leader breaks, arms up, the track past it grey.
//!
//! The sprites are `static`s so they link under this module's name, into
//! bank 2 with the rest of `draw` (a `const`'s bytes would land in bank 1's
//! anonymous rodata).

use super::{
    ACC, FAINT, INK, INK2, MID, Pix, SOFT, cos, loop_span, lvis, r, scatter, window_strip,
};
use crate::dsp::algo::math::clampf;
use crate::dsp::fast_sin;
use crate::dsp::tape_engine::tron::{self, TRON_LEN_MAX_S};
use crate::dsp::tape_engine::{Direction, Tron};
use crate::tape_glyph::{Ink, Steps};
use core::f32::consts::{FRAC_PI_2, TAU};

/// A runner's pixels, facing right, feet on the last row: `o` face, `#`
/// singlet, `+` near limbs and shorts, `-` far limbs and hair.
type Big = [[u8; 12]; 14];
type Small = [[u8; 8]; 10];

const fn rows<const N: usize, const M: usize>(s: [&[u8; N]; M]) -> [[u8; N]; M] {
    let mut o = [[0; N]; M];
    let mut i = 0;
    while i < M {
        o[i] = *s[i];
        i += 1;
    }
    o
}

/// Stride, flight, passing, and arms up at the tape.
static BIG: [Big; 4] = [
    rows([
        b".......-oo..",
        b"......--ooo.",
        b".......ooo..",
        b".......##...",
        b"......####..",
        b".....+####-.",
        b"....+.###.-.",
        b"...+..###...",
        b"......===...",
        b".....==.==..",
        b"....--...++.",
        b"..---....+..",
        b".--......+..",
        b".-.......==.",
    ]),
    rows([
        b"........-oo.",
        b".......--ooo",
        b"........ooo.",
        b"........##..",
        b".......####.",
        b"......+####-",
        b".....+.###.-",
        b"......####..",
        b"......====..",
        b".--===...++.",
        b"--........+.",
        b"-.........+.",
        b"..........==",
        b"............",
    ]),
    rows([
        b".......-oo..",
        b"......--ooo.",
        b".......ooo..",
        b".......##...",
        b"......###...",
        b"......###+..",
        b".....-###+..",
        b".....-###...",
        b"......===...",
        b"......+=-...",
        b".....++.-...",
        b".....+..-...",
        b".....+..--..",
        b"....==......",
    ]),
    rows([
        b"..+.......+.",
        b"..+..-oo..+.",
        b"...+--ooo+..",
        b"...+.ooo.+..",
        b"....+.##+...",
        b".....####...",
        b".....####...",
        b".....###....",
        b".....===....",
        b".....=.==...",
        b"....++..-...",
        b"....+....-..",
        b"...++....-..",
        b"...=.....--.",
    ]),
];

static SMALL: [Small; 4] = [
    rows([
        b"....-oo.",
        b"....-oo.",
        b".....#..",
        b"....###-",
        b"...+###.",
        b"..+.###.",
        b"....==+.",
        b"..--..+.",
        b".-....+.",
        b"-.....==",
    ]),
    rows([
        b"...-oo..",
        b"...-oo..",
        b"....#...",
        b"...###..",
        b"..-###+.",
        b".-.###.+",
        b"...==...",
        b"..=..+..",
        b".--..++.",
        b".-....+.",
    ]),
    rows([
        b"....-oo.",
        b"....-oo.",
        b".....#..",
        b"....###.",
        b"....-##+",
        b"....-##+",
        b"....==..",
        b"...-.+..",
        b"..-..+..",
        b"..-..==.",
    ]),
    rows([
        b".+....+.",
        b".+.-oo+.",
        b"..+-oo+.",
        b"...###..",
        b"...###..",
        b"...###..",
        b"...==...",
        b"...+.-..",
        b"..+..-..",
        b"..=..--.",
    ]),
];

const CHEER: usize = 3;

/// One runner: feet at (`x`, `y`), facing left if `back`, `pose` its
/// sprite, `dim` FADE's steps toward the ground (2: grey), `look` its hair
/// (a ponytail, a teal cap, or as drawn).
#[derive(Clone, Copy)]
struct Runner {
    x: i32,
    y: i32,
    back: bool,
    pose: usize,
    dim: u8,
    look: u8,
    big: bool,
}

impl Runner {
    fn draw(self, p: &mut Pix) {
        if self.big {
            person(p, &BIG[self.pose], self);
        } else {
            person(p, &SMALL[self.pose], self);
        }
    }
}

fn person<const N: usize, const M: usize>(p: &mut Pix, s: &[[u8; N]; M], u: Runner) {
    let (x0, y0) = (u.x - N as i32 / 2, u.y - M as i32 + 1);
    let fx = |i: usize| x0 + if u.back { N - 1 - i } else { i } as i32;
    let ink = |c: Ink| match u.dim {
        0 => c,
        1 => c.dim(),
        _ => FAINT,
    };
    for (j, row) in s.iter().enumerate() {
        let mut first = true;
        for (i, &c) in row.iter().enumerate() {
            let base = match c {
                b'o' => INK,
                b'#' => ACC,
                b'+' | b'=' => INK2,
                b'-' if j < 2 && u.look == 2 => ACC,
                b'-' => MID,
                _ => continue,
            };
            p.px(fx(i), y0 + j as i32, ink(base));
            // A ponytail off the first hair of the second row, behind.
            if c == b'-' && j == 1 && first && u.look == 1 {
                let d = if u.back { 1 } else { -1 };
                p.px(fx(i) + d, y0 + 2, ink(MID));
                p.px(fx(i) + 2 * d, y0 + 3, ink(MID));
            }
            if c == b'-' {
                first = false;
            }
        }
    }
}

/// FADE's dim for runner `j` (0 the leader) of `n`: the last ones first.
fn fade_dim(j: usize, n: usize, fade: f32) -> u8 {
    let faded = libm::roundf(fade * 4.0) as usize;
    let d = n - 1 - j;
    match () {
        _ if faded > 0 && d < faded / 2 => 2,
        _ if d < faded => 1,
        _ => 0,
    }
}

/// DRIFT on runner `j`: a fixed nudge along the way (−1..1, scaled by the
/// caller), a bob in px, and a shuffled stride.
fn drift_of(j: usize, w: Steps) -> (f32, i32, usize) {
    let d = w.drift.unit();
    let (sx, sy) = scatter(j as i32 * 3 + 1);
    let pose = if w.drift.step() == 0 {
        j % 3
    } else {
        (j + (w.drift.step() as usize) * (1 + (sy > 0.0) as usize)) % 3
    };
    (sx * d, r(sy * d * 1.5), pose)
}

/// TRON's tape, as a share of the window (`tron::tron_len_s` over 8 s).
fn tron_share(w: Steps) -> f32 {
    tron::TapeSeconds::of_length(w.length.unit()).get() / TRON_LEN_MAX_S
}

/// The pack on a run `span` long (in the caller's units), `margin` in
/// from its ends, TRON's tape `tape` along it: each runner's way along
/// (the leader, 0, furthest), stride and FADE dim. With the tape the
/// leader breaks it, arms up, and the rest run behind it, none past.
fn race(w: Steps, n: usize, span: f32, margin: f32, tape: Option<f32>) -> [(f32, usize, u8); 5] {
    let lead = match tape {
        Some(e) => clampf(e - margin, margin, span - margin),
        None => span - margin,
    };
    let mut out = [(0.0, 0, 0); 5];
    for (j, o) in out.iter_mut().enumerate().take(n) {
        let k = if n > 1 {
            j as f32 / (n - 1) as f32
        } else {
            0.0
        };
        let (dx, _, pose) = drift_of(j, w);
        let u = lead - (lead - margin) * k + dx * margin * 1.3;
        *o = if tape.is_some() && j == 0 {
            (lead, CHEER, 0)
        } else {
            (u, pose, fade_dim(j, n, w.fade.unit()))
        };
    }
    out
}

/// At most eight runners, drawn far to near.
struct Pack {
    r: [Runner; 8],
    n: usize,
}

impl Pack {
    const fn new() -> Self {
        let z = Runner {
            x: 0,
            y: 0,
            back: false,
            pose: 0,
            dim: 0,
            look: 0,
            big: false,
        };
        Pack { r: [z; 8], n: 0 }
    }

    fn push(&mut self, u: Runner) {
        if self.n < self.r.len() {
            self.r[self.n] = u;
            self.n += 1;
        }
    }

    fn draw(&mut self, p: &mut Pix) {
        // Far to near: an insertion sort of eight, here so its code links
        // under this module (core's sort would land in bank 1).
        let s = &mut self.r[..self.n];
        for i in 1..s.len() {
            let mut j = i;
            while j > 0 && s[j - 1].y > s[j].y {
                s.swap(j - 1, j);
                j -= 1;
            }
        }
        for u in s.iter() {
            u.draw(p);
        }
    }
}

// ── The oval ────────────────────────────────────────────────────────

/// An oval track seen from above at an angle: its centre, its inner and
/// outer edges' radii.
#[derive(Clone, Copy)]
struct Oval {
    cx: f32,
    cy: f32,
    inner: (f32, f32),
    outer: (f32, f32),
}

impl Oval {
    /// The point `s` (0..1) round the lap, `t` (0 inner to 1 outer) across
    /// the track. `s` 0 is the near straight's middle, running right.
    fn at(self, s: f32, t: f32) -> (f32, f32) {
        let a = FRAC_PI_2 - TAU * s;
        let rx = self.inner.0 + t * (self.outer.0 - self.inner.0);
        let ry = self.inner.1 + t * (self.outer.1 - self.inner.1);
        (self.cx + rx * cos(a), self.cy + ry * fast_sin(a))
    }

    /// True on the near half, where forward runs right.
    fn near(s: f32) -> bool {
        fast_sin(FRAC_PI_2 - TAU * s) >= 0.0
    }

    fn edges(self, p: &mut Pix) {
        for i in 0..160 {
            let s = i as f32 / 160.0;
            for t in [0.0, 1.0] {
                let (x, y) = self.at(s, t);
                p.px(r(x), r(y), FAINT);
            }
        }
    }

    /// `a..b` of the lap lit: soft across the track, its outer edge accent.
    fn lit(self, p: &mut Pix, a: f32, b: f32) {
        let n = (r((b - a) * 220.0)).max(1);
        for i in 0..=n {
            let s = a + (b - a) * i as f32 / n as f32;
            let (x0, y0) = self.at(s, 0.2);
            let (x1, y1) = self.at(s, 0.8);
            p.line(r(x0), r(y0), r(x1), r(y1), SOFT);
            let (x, y) = self.at(s, 1.0);
            p.px(r(x), r(y), ACC);
        }
    }

    /// A line across the track at `s`.
    fn mark(self, p: &mut Pix, s: f32, ink: Ink) {
        let (x0, y0) = self.at(s, 0.0);
        let (x1, y1) = self.at(s, 1.0);
        p.line(r(x0), r(y0), r(x1), r(y1), ink);
    }

    /// TRON's chequered flag at `s`, on a pole at the track's outer edge.
    fn tape(self, p: &mut Pix, s: f32) {
        let (x, y) = self.at(s, 1.15);
        flag(p, r(x), r(y), x > self.cx);
    }

    /// A small turning cone on the track at `s`.
    fn cone(self, p: &mut Pix, s: f32) {
        let (x, y) = self.at(s, 0.5);
        cone(p, r(x), r(y));
    }
}

/// Posts at `a` and `b` (their feet), 6 px tall, the tape across their
/// tops, a loose end fluttering past the near post.
fn finish_tape(p: &mut Pix, a: (i32, i32), b: (i32, i32)) {
    let h = 6;
    p.rect(a.0, a.1 - h, 1, h + 1, INK2);
    p.rect(b.0, b.1 - h, 1, h + 1, INK2);
    p.line(a.0, a.1 - h, b.0, b.1 - h, INK);
}

fn cone(p: &mut Pix, x: i32, y: i32) {
    p.px(x, y - 3, INK);
    p.rect(x - 1, y - 2, 3, 1, ACC);
    p.rect(x - 1, y - 1, 3, 1, INK);
    p.rect(x - 2, y, 5, 1, ACC);
}

/// The grandstand behind the straight: a rail and a crowd, still.
fn stand(p: &mut Pix) {
    for x in (3..61).step_by(3) {
        let (h, _) = scatter(x);
        let y = 15 + (h > 0.3) as i32;
        p.px(x, y, if h > -0.4 { MID } else { INK2 });
        p.rect(x - 1, y + 1, 2, 2, FAINT);
    }
    p.rect(2, 20, 60, 1, MID);
}

/// A chequered flag on a pole, its foot at (`x`, `y`), flying inward
/// (`left`) on the right of the track.
fn flag(p: &mut Pix, x: i32, y: i32, left: bool) {
    let x = x.clamp(1, 62);
    let d = if left { -1 } else { 1 };
    p.rect(x, y - 11, 1, 12, INK2);
    for j in 0..4 {
        for i in 0..6 {
            let c = if (i / 2 + j / 2) % 2 == 0 { INK } else { MID };
            p.px(x + d * (1 + i), y - 11 + j, c);
        }
    }
}

/// TRON's stopwatch, top right: LENGTH's share of 8 s swept in accent
/// from twelve o'clock.
fn stopwatch(p: &mut Pix, w: Steps) {
    let (cx, cy) = (53, 9);
    p.rect(cx - 1, cy - 7, 3, 1, INK2);
    p.px(cx, cy - 6, INK2);
    p.ring(cx, cy, 5, INK);
    let share = tron_share(w);
    for dy in -4..=4 {
        for dx in -4..=4 {
            if dx * dx + dy * dy > 16 {
                continue;
            }
            // The angle clockwise from twelve, as a share of the turn.
            let a = libm::atan2f(dx as f32, -dy as f32) / TAU;
            let a = if a < 0.0 { a + 1.0 } else { a };
            if a <= share {
                p.px(cx + dx, cy + dy, ACC);
            }
        }
    }
    p.px(cx, cy, INK);
}

/// An arrowhead pointing down from (`x`, `y`).
fn arrow_down(p: &mut Pix, x: i32, y: i32, c: Ink) {
    for i in 0..3 {
        p.rect(x - 2 + i, y + i, 5 - 2 * i, 1, c);
    }
}

/// WALK round an oval: FORWARD runs the lit stretch round one way,
/// REVERSE the other, LOOP fills the lap with arrows round the bends,
/// PING-PONG shuttles the near straight, both ways, between cones. With
/// `depth` the near runners are big and the far ones small.
#[inline(never)]
pub(super) fn oval(p: &mut Pix, w: Steps, depth: bool) {
    let o = if depth {
        Oval {
            cx: 32.0,
            cy: 28.0,
            inner: (19.0, 6.0),
            outer: (31.0, 15.0),
        }
    } else {
        Oval {
            cx: 32.0,
            cy: 29.0,
            inner: (20.0, 6.0),
            outer: (30.0, 13.0),
        }
    };
    let (pos, len, fade) = (w.position.unit(), w.length.unit(), w.fade.unit());
    let tron = w.tron == Tron::On;
    o.edges(p);
    let mut pack = Pack::new();
    let put = |pack: &mut Pack, j: usize, s: f32, t: f32, back: bool, dim: u8, pose| {
        let (x, y) = o.at(s, t);
        let (_, bob, _) = drift_of(j, w);
        pack.push(Runner {
            x: r(x),
            y: r(y) + bob,
            back,
            pose,
            dim,
            look: (j % 3) as u8,
            big: depth && Oval::near(s),
        });
    };
    match w.direction() {
        Direction::Forward | Direction::Reverse => {
            let back = w.direction() == Direction::Reverse;
            // The window round the lap from the near straight's middle;
            // REVERSE runs it from its end.
            let from = pos * 0.9;
            let to = from + lvis(len) * (0.97 - from);
            let span = to - from;
            // TRON's tape along the run, as it plays.
            let tape = tron.then(|| {
                let e = tron_share(w);
                clampf(if back { to - (1.0 - e) } else { e - from }, 0.0, span)
            });
            // The window starts at the near straight's left end.
            let at = |u: f32| (if back { to - u } else { from + u }) - 0.2;
            let played = tape.unwrap_or(span);
            o.lit(p, at(0.0).min(at(played)), at(0.0).max(at(played)));
            o.mark(p, at(0.0), MID);
            o.mark(p, at(span), if tron { FAINT } else { INK2 });
            let n = ((1.0 + played * 6.0) as usize).clamp(1, 5);
            for (j, &(u, pose, dim)) in race(w, n, span, 0.03, tape)[..n].iter().enumerate() {
                let s = at(u);
                put(&mut pack, j, s, 0.5, Oval::near(s) == back, dim, pose);
            }
            if let Some(e) = tape {
                o.tape(p, at(e));
            }
        }
        Direction::Loop => {
            let (lo, hi) = loop_span(pos, len);
            let (a, b) = ((lo - 2.0) / 60.0, (hi - 2.0) / 60.0);
            o.lit(p, a, b);
            // Round the bends: up the right, down the left.
            let (rx, _) = o.at(0.25, -0.3);
            let (lx, _) = o.at(0.75, -0.3);
            p.arrow_up(r(rx), r(o.cy) - 1, INK2);
            arrow_down(p, r(lx), r(o.cy) - 1, INK2);
            let n = 5;
            if tron {
                // Round to the tape at the loop's end, the pack behind.
                for (j, &(u, pose, dim)) in
                    race(w, n, 0.85, 0.03, Some(0.85))[..n].iter().enumerate()
                {
                    let s = b - 0.85 + u;
                    put(&mut pack, j, s, 0.5, !Oval::near(s), dim, pose);
                }
            } else {
                for j in 0..n {
                    let (dx, _, pose) = drift_of(j, w);
                    let s = 0.9 - j as f32 / n as f32 + dx * 0.05;
                    put(
                        &mut pack,
                        j,
                        s,
                        0.5,
                        !Oval::near(s),
                        fade_dim(j, n, fade),
                        pose,
                    );
                }
            }
            if tron {
                o.tape(p, b);
            }
        }
        Direction::Ping => {
            // The near straight, `lo..hi` across it, as lap shares.
            let (lo, hi) = loop_span(pos, len);
            let s_of = |x: f32| {
                let c = clampf((x - o.cx) / ((o.inner.0 + o.outer.0) / 2.0), -1.0, 1.0);
                (FRAC_PI_2 - libm::acosf(c)) / TAU
            };
            let (a, b) = (s_of(lo), s_of(hi));
            o.lit(p, a, b);
            o.cone(p, a - 0.015);
            o.cone(p, b + 0.015);
            let n = (((hi - lo) / 11.0) as usize + 1).clamp(2, 5);
            for j in 0..n {
                let (dx, _, pose) = drift_of(j, w);
                let k = (j as f32 + 0.5) / n as f32;
                let s = a + (b - a) * k + dx * 0.02;
                // Out on the outer lane, back on the inner.
                let back = j % 2 == 1;
                let t = if back { 0.2 } else { 0.8 };
                put(&mut pack, j, s, t, back, fade_dim(j, n, fade), pose);
            }
            if tron {
                o.tape(p, b + 0.04);
            }
        }
    }
    pack.draw(p);
}

// ── The straight ────────────────────────────────────────────────────

/// The straight's lane: its back edge, the runners' feet, the strip.
const BACK: i32 = 29;
const FEET: i32 = 41;

/// WALK on the home straight, seen from the side, the track taking
/// DIRECTION's shape: a straight from start to finish for FORWARD and
/// REVERSE, the oval for LOOP (near runners big, far ones small, running
/// back), cones at both ends of the straight for PING-PONG.
#[inline(never)]
pub(super) fn straight(p: &mut Pix, w: Steps) {
    let (pos, len, fade) = (w.position.unit(), w.length.unit(), w.fade.unit());
    let tron = w.tron == Tron::On;
    let mut pack = Pack::new();
    let runner = |j: usize, x: f32, back: bool, dim: u8, pose: usize, y: i32, big: bool| {
        let (_, bob, _) = drift_of(j, w);
        Runner {
            x: r(x),
            y: y + bob,
            back,
            pose,
            dim,
            look: (j % 3) as u8,
            big,
        }
    };
    stand(p);
    let lane = |p: &mut Pix| {
        for x in (2..62).step_by(3) {
            p.px(x, BACK, FAINT);
        }
        p.rect(2, FEET + 1, 60, 1, FAINT);
    };
    match w.direction() {
        Direction::Forward | Direction::Reverse => {
            let back = w.direction() == Direction::Reverse;
            let (x0, x1) = (6.0, 58.0);
            let from = x0 + pos * (x1 - x0);
            let to = from + lvis(len) * (x1 - from);
            let span = to - from;
            lane(p);
            window_strip(p, r(from), r(to), 45, 0);
            let tape = tron.then(|| {
                let e = tron_share(w) * (x1 - x0);
                clampf(if back { to - (x1 - e) } else { x0 + e - from }, 0.0, span)
            });
            let at = |u: f32| if back { to - u } else { from + u };
            // Start and finish lines across the lane.
            p.line(r(at(0.0)), FEET + 1, r(at(0.0)) + 2, BACK, MID);
            p.line(r(at(span)), FEET + 1, r(at(span)) + 2, BACK, INK2);
            if let Some(e) = tape {
                // The run past the tape never plays.
                let (a, z) = (r(at(e)), r(at(span)));
                p.rect(a.min(z) + 1, 45, (a - z).abs(), 2, MID);
                stopwatch(p, w);
            }
            let n = ((tape.unwrap_or(span) / 13.0) as usize + 1).clamp(1, 4);
            for (j, &(u, pose, dim)) in race(w, n, span, 4.0, tape)[..n].iter().enumerate() {
                pack.push(runner(j, at(u), back, dim, pose, FEET, true));
            }
            if let Some(e) = tape {
                let e = r(at(e) + if back { -3.0 } else { 3.0 });
                finish_tape(p, (e + 2, BACK + 2), (e, FEET + 2));
            }
        }
        Direction::Loop => {
            let o = Oval {
                cx: 32.0,
                cy: 35.0,
                inner: (22.0, 3.0),
                outer: (31.0, 9.0),
            };
            o.edges(p);
            let (lo, hi) = loop_span(pos, len);
            let (a, b) = ((lo - 2.0) / 60.0, (hi - 2.0) / 60.0);
            o.lit(p, a, b);
            let n = 4;
            let lap = |p: &mut Pack, j: usize, s: f32, pose, dim| {
                let (x, y) = o.at(s, 0.6);
                let near = Oval::near(s);
                p.push(runner(j, x, !near, dim, pose, r(y), near));
            };
            if tron {
                stopwatch(p, w);
                for (j, &(u, pose, dim)) in race(w, n, 0.8, 0.03, Some(0.8))[..n].iter().enumerate()
                {
                    lap(&mut pack, j, b - 0.8 + u, pose, dim);
                }
            } else {
                for j in 0..n {
                    let (dx, _, pose) = drift_of(j, w);
                    let s = 0.85 - j as f32 / n as f32 + dx * 0.05;
                    lap(&mut pack, j, s, pose, fade_dim(j, n, fade));
                }
            }
            if tron {
                o.tape(p, b);
            }
        }
        Direction::Ping => {
            let (lo, hi) = loop_span(pos, len);
            lane(p);
            window_strip(p, r(lo), r(hi), 45, 0);
            cone(p, r(lo) - 1, FEET);
            cone(p, r(hi) + 1, FEET);
            let n = (((hi - lo) / 14.0) as usize + 1).clamp(2, 4);
            for j in 0..n {
                let (dx, _, pose) = drift_of(j, w);
                let k = (j as f32 + 0.5) / n as f32;
                let x = lo + 3.0 + (hi - lo - 6.0) * k + dx * 3.0;
                // Out in the near lane, back in the far one.
                let back = j % 2 == 1;
                let y = if back { FEET - 4 } else { FEET };
                pack.push(runner(j, x, back, fade_dim(j, n, fade), pose, y, true));
            }
            if tron {
                stopwatch(p, w);
                let e = r(hi) + 3;
                finish_tape(p, (e + 2, BACK + 2), (e, FEET + 2));
            }
        }
    }
    pack.draw(p);
}
