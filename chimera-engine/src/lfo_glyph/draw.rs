//! The LFO pictures' raster: `render`, a pure function of an `LfoGauge`
//! into a `Canvas`, and `curve`, the scope's columns of a `Wave`. With
//! `bank2` this module is bank 2's (`bank2.x`), reached only through the
//! door (`super::paint`, `super::curve`).
//!
//! The drawings follow the owner's mockup (`.superpowers/ledgers/
//! lfo-glyphs/`, the recommended pick of each): rows, Bresenham lines,
//! discs of `r² + r`, the curves the LFO's own arithmetic.

use super::{COLS, Canvas, Glyph, H, Ink, LfoGauge, SCOPE_AMP, W, Wave};
use crate::dsp::clampf;
use crate::dsp::fast_sin_turns;
use crate::dsp::lfo::LfoShape;
use crate::dsp::modulator::LfoForm;
use crate::dsp::modulator::law::tilt;

const INK: Ink = Ink::Ink;
const MID: Ink = Ink::Mid;
const FAINT: Ink = Ink::Faint;
const ACC: Ink = Ink::Accent;
const SOFT: Ink = Ink::Soft;

/// The plot's columns in the box, and its left pad.
const N: usize = 54;
const PADX: i32 = 5;
/// The longest walk a picture draws, in cycles (the scope's eight steps;
/// RATE's two seconds draw a wave only at 13.5 cycles or fewer).
const WALK_MAX: usize = 16;

/// S&H's picture: eight fixed levels.
const SH: [f32; 8] = [0.55, -0.35, 0.9, -0.7, 0.15, -0.9, 0.7, -0.2];
/// LFV's picture: each target's step from the last, a share of DELTA.
const WALK_R: [f32; 12] = [
    0.8, -0.95, 0.55, -0.4, 1.0, -0.75, 0.3, -0.9, 0.65, -0.5, 0.9, -0.3,
];
/// The key lane's two notes, columns into the plot.
const KEYS: [i32; 2] = [16, 38];

/// `g` into `out`, every pixel of the box.
#[inline(never)]
pub fn render(g: &LfoGauge, out: &mut Canvas) {
    let mut p = Pix(Canvas::new());
    let w = &g.wave;
    match g.glyph {
        Glyph::Rate => rate(&mut p, w),
        Glyph::Shape => shape(&mut p, w),
        Glyph::Lane => lane(&mut p, w),
        Glyph::Phase => phase(&mut p, w),
        Glyph::Depth => depth(&mut p, w),
        Glyph::Tilt => tilt_glyph(&mut p, w),
        Glyph::Delta => delta(&mut p, w),
        Glyph::Slew => slew(&mut p, w),
    }
    *out = p.0;
}

/// The scope's one cycle of `w` (S&H and LFV: their eight steps), each
/// column's offset from the band's middle, up positive.
#[inline(never)]
pub fn curve(w: &Wave, out: &mut [i8; COLS]) {
    let mut v = [0.0f32; COLS];
    match *w {
        Wave::Classic {
            shape: LfoShape::Random,
            depth,
            ..
        } => {
            for (i, o) in v.iter_mut().enumerate() {
                *o = depth.unit() * SH[i * 8 / COLS];
            }
        }
        Wave::Func {
            form: LfoForm::Lfv,
            fall,
            tilt,
            ..
        } => walk(fall.unit(), tilt.unit(), 8.0, 0, &mut v),
        _ => wave(w, 0.999, 0.0, Opt::ALL, &mut v),
    }
    // `mid − round(mid − v·amp)`, half up as `plot` rounds.
    for (o, &v) in out.iter_mut().zip(&v) {
        *o = libm::ceilf(clampf(v, -1.0, 1.0) * SCOPE_AMP as f32 - 0.5) as i8;
    }
}

// ── The LFO's arithmetic ─────────────────────────────────────────────

fn frac(x: f32) -> f32 {
    x - libm::floorf(x)
}

/// Half up, as the mockup rounds.
fn round(x: f32) -> i32 {
    libm::floorf(x + 0.5) as i32
}

/// CLASSIC's `shape` at `u` cycles (`Lfo::process`, depth 1).
fn raw_c(shape: LfoShape, u: f32) -> f32 {
    let t = frac(u);
    match shape {
        LfoShape::Sine => fast_sin_turns(t),
        LfoShape::Triangle => {
            if t < 0.25 {
                4.0 * t
            } else if t < 0.75 {
                2.0 - 4.0 * t
            } else {
                4.0 * t - 4.0
            }
        }
        LfoShape::Saw => 2.0 * t - 1.0,
        LfoShape::Square => {
            if t < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoShape::Random => SH[(libm::floorf(u) as i32).rem_euclid(8) as usize],
    }
}

/// FUNC at `u` cycles with its rise share `r` (`law::tilt`).
fn raw_f(r: f32, u: f32) -> f32 {
    2.0 * tilt(frac(u), r) - 1.0
}

/// LFV's walk over `cycles`, into `out`: a target a cycle, the last's
/// plus DELTA × `WALK_R` (from `lane`), clamped to ±1, ramped linearly,
/// then a one-pole of τ = SLEW cycles after a cycle of warm-up.
fn walk(delta: f32, slew: f32, cycles: f32, lane: usize, out: &mut [f32]) {
    let n = out.len();
    let cycles = cycles.min(WALK_MAX as f32 - 3.0);
    let mut t = [0.0f32; WALK_MAX];
    let last = (libm::ceilf(cycles) as usize + 2).min(WALK_MAX - 1);
    for k in 1..=last {
        t[k] = clampf(
            t[k - 1] + delta * WALK_R[(k - 1 + lane * 5) % WALK_R.len()],
            -1.0,
            1.0,
        );
    }
    let lin = |u: f32| {
        let k = (libm::floorf(u) as usize).min(WALK_MAX - 2);
        t[k] + (t[k + 1] - t[k]) * frac(u)
    };
    let dt = cycles / (n - 1) as f32;
    let c = if slew > 0.0 {
        1.0 - libm::expf(-dt / slew)
    } else {
        1.0
    };
    let mut y = lin(0.0);
    let mut u = 0.0;
    while u < 1.0 {
        y += c * (lin(u) - y);
        u += dt;
    }
    for (i, o) in out.iter_mut().enumerate() {
        y += c * (lin(1.0 + i as f32 * dt) - y);
        *o = y;
    }
}

/// What a picture of the wave leaves out.
#[derive(Clone, Copy)]
struct Opt {
    phase: bool,
    depth: bool,
}

impl Opt {
    const ALL: Opt = Opt {
        phase: true,
        depth: true,
    };
    const NO_PHASE: Opt = Opt {
        phase: false,
        depth: true,
    };
    const BARE: Opt = Opt {
        phase: false,
        depth: false,
    };
}

/// The output over `cycles` from `u0`, into `out`: CLASSIC with DEPTH and
/// (RETRIG) PHASE, FUNC with PHASE; LFV its walk.
fn wave(w: &Wave, cycles: f32, u0: f32, o: Opt, out: &mut [f32]) {
    let n = out.len();
    match *w {
        Wave::Func {
            form: LfoForm::Lfv,
            fall,
            tilt,
            ..
        } => walk(fall.unit(), tilt.unit(), cycles.max(0.05), 0, out),
        Wave::Classic {
            shape,
            retrig,
            phase,
            depth,
            ..
        } => {
            let ph = if o.phase && retrig { phase.unit() } else { 0.0 };
            let d = if o.depth { depth.unit() } else { 1.0 };
            for (i, v) in out.iter_mut().enumerate() {
                let u = u0 + cycles * i as f32 / (n - 1) as f32;
                *v = d * raw_c(shape, u + ph);
            }
        }
        Wave::Func { fall, tilt, .. } => {
            let ph = if o.phase { fall.unit() } else { 0.0 };
            for (i, v) in out.iter_mut().enumerate() {
                let u = u0 + cycles * i as f32 / (n - 1) as f32;
                *v = raw_f(tilt.unit(), u + ph);
            }
        }
    }
}

// ── The glyphs ───────────────────────────────────────────────────────

/// How a plot draws: its fill, its weight, the columns it skips.
#[derive(Clone, Copy)]
struct Line {
    ink: Ink,
    fill: Option<Ink>,
    thick: bool,
}

const LIT: Line = Line {
    ink: ACC,
    fill: Some(SOFT),
    thick: true,
};
const BOLD: Line = Line {
    ink: ACC,
    fill: None,
    thick: true,
};

const fn thin(ink: Ink) -> Line {
    Line {
        ink,
        fill: None,
        thick: false,
    }
}

/// RATE: the wave over two seconds, a comb past four columns a cycle,
/// solid past two; the one-second ruler under it (its `1 S` is the UI's).
fn rate(p: &mut Pix, w: &Wave) {
    let cyc = w.hz() * 2.0;
    let (x0, mid, amp) = (PADX, 18, 13);
    let ppc = N as f32 / cyc;
    if ppc >= 4.0 {
        let mut v = [0.0; N];
        wave(w, cyc, 0.0, Opt::ALL, &mut v);
        p.plot(x0, mid, amp, &v, LIT);
    } else if ppc >= 2.0 {
        let mut k = 0;
        while (k as f32) < cyc {
            p.rect(x0 + round(k as f32 * ppc), mid - amp, 1, 2 * amp + 1, ACC);
            k += 1;
        }
    } else {
        p.rect(x0, mid - amp, N as i32, 2 * amp + 1, SOFT);
        p.rect(x0, mid - amp, N as i32, 2, ACC);
        p.rect(x0, mid + amp - 1, N as i32, 2, ACC);
    }
    let y = 38;
    p.rect(x0, y, N as i32, 1, FAINT);
    for t in [0, 26, 53] {
        p.rect(x0 + t, y - 2, 1, 3, MID);
    }
}

/// SHAPE: one cycle of the shape (S&H four of its steps), full scale.
fn shape(p: &mut Pix, w: &Wave) {
    let Wave::Classic { shape, .. } = *w else {
        return;
    };
    let cyc = if shape == LfoShape::Random { 4.0 } else { 1.0 };
    let mut v = [0.0; N];
    for (i, o) in v.iter_mut().enumerate() {
        *o = raw_c(shape, cyc * i as f32 / (N - 1) as f32 * 0.999);
    }
    p.plot(PADX, 24, 17, &v, LIT);
}

/// SYNC and FORM: two keys under two and a half cycles. FREE runs through
/// them, RETRIG and SYNC restart at each (RETRIG at PHASE), LFV walks.
fn lane(p: &mut Pix, w: &Wave) {
    let resets = match *w {
        Wave::Classic { retrig, .. } => retrig,
        Wave::Func { form, .. } => form == LfoForm::Sync,
    };
    for k in KEYS {
        let x = PADX + k;
        let mut y = 3;
        while y <= 35 {
            p.px(x, y, FAINT);
            y += 2;
        }
        p.keycap(x, 39, if resets { INK } else { MID });
    }
    let (span, u0) = (2.5, 0.15);
    let mut v = [0.0; N];
    match *w {
        Wave::Func {
            form: LfoForm::Lfv,
            fall,
            tilt,
            ..
        } => walk(fall.unit(), tilt.unit(), span, 0, &mut v),
        _ => {
            for (i, o) in v.iter_mut().enumerate() {
                let (mut start, mut base) = (0, u0);
                if resets {
                    for k in KEYS {
                        if i as i32 >= k {
                            (start, base) = (k, 0.0);
                        }
                    }
                }
                let u = base + (i as i32 - start) as f32 / (N - 1) as f32 * span;
                *o = match *w {
                    Wave::Classic {
                        shape,
                        phase,
                        depth,
                        ..
                    } => {
                        let ph = if resets { phase.unit() } else { 0.0 };
                        depth.unit() * raw_c(shape, u + ph)
                    }
                    Wave::Func { fall, tilt, .. } => raw_f(tilt.unit(), u + fall.unit()),
                };
            }
        }
    }
    p.plot(PADX, 19, 13, &v, LIT);
}

/// PHASE: the cycle slides under a fixed start key and dot, the unshifted
/// cycle ghosted behind.
fn phase(p: &mut Pix, w: &Wave) {
    let (x0, mid, amp) = (PADX, 19, 13);
    let mut ghost = [0.0; N];
    wave(w, 1.0, 0.0, Opt::BARE, &mut ghost);
    let mut f = [0.0; N];
    for (i, o) in f.iter_mut().enumerate() {
        let t = i as f32 / (N - 1) as f32 * 0.999;
        *o = match *w {
            Wave::Classic { shape, phase, .. } => raw_c(shape, phase.unit() + t),
            Wave::Func { fall, tilt, .. } => raw_f(tilt.unit(), fall.unit() + t),
        };
    }
    p.plot(x0, mid, amp, &ghost, thin(FAINT));
    let y0 = p.plot(x0, mid, amp, &f, BOLD);
    p.dot(x0, y0, 2, INK);
    p.keycap(x0, 39, INK);
    p.rect(x0, y0 + 4, 1, 37 - y0 - 4, MID);
}

/// DEPTH: the scaled wave over the full-scale one, ghosted.
fn depth(p: &mut Pix, w: &Wave) {
    let (x0, mid, amp) = (PADX, 24, 17);
    let mut v = [0.0; N];
    wave(w, 1.0, 0.0, Opt::BARE, &mut v);
    p.plot(x0, mid, amp, &v, thin(MID));
    wave(w, 1.0, 0.0, Opt::NO_PHASE, &mut v);
    p.plot(x0, mid, amp, &v, LIT);
}

/// TILT: one cycle, its rise lit up to the peak's dot, and under it the
/// rise:fall bar with a mark at the split.
fn tilt_glyph(p: &mut Pix, w: &Wave) {
    let Wave::Func { tilt: r, .. } = *w else {
        return;
    };
    let (x0, mid, amp, r) = (PADX, 18, 13, r.unit());
    let mut v = [0.0; N];
    for (i, o) in v.iter_mut().enumerate() {
        *o = raw_f(r, i as f32 / (N - 1) as f32 * 0.999);
    }
    let k = round(r * (N - 1) as f32);
    p.plot(x0, mid, amp, &v, Line { ink: MID, ..BOLD });
    let y = |i: usize| round(mid as f32 - v[i] * amp as f32);
    for i in 1..=k as usize {
        let (a, b) = (y(i - 1), y(i));
        p.line(x0 + i as i32 - 1, a, x0 + i as i32, b, ACC);
        p.line(x0 + i as i32 - 1, a + 1, x0 + i as i32, b + 1, ACC);
    }
    p.dot(x0 + k, y((k as usize).min(N - 1)), 2, INK);
    let bar = 38;
    if k > 0 {
        p.rect(x0, bar, k, 3, ACC);
    }
    if k < N as i32 - 1 {
        p.rect(x0 + k + 1, bar, N as i32 - 1 - k, 3, MID);
    }
    // ▴, its base on the label row.
    let mx = x0 + (k - 2).clamp(0, N as i32 - 5);
    for i in 0..3 {
        p.rect(mx + i, 47 - 3 - i, 5 - 2 * i, 1, ACC);
    }
}

/// DELTA: the walk over five cycles between the ±1 rails, each target a
/// dot.
fn delta(p: &mut Pix, w: &Wave) {
    let Wave::Func { fall, tilt, .. } = *w else {
        return;
    };
    let (x0, mid, amp, cyc) = (PADX, 24, 17, 5);
    for y in [mid - amp, mid + amp + 1] {
        let mut x = x0;
        while x < x0 + N as i32 {
            p.px(x, y, FAINT);
            x += 2;
        }
    }
    let mut v = [0.0; N];
    walk(fall.unit(), tilt.unit(), cyc as f32, 0, &mut v);
    p.plot(x0, mid, amp, &v, LIT);
    walk(fall.unit(), 0.0, cyc as f32, 0, &mut v);
    for k in 0..=cyc {
        let i = (round(k as f32 / cyc as f32 * (N - 1) as f32) as usize).min(N - 1);
        p.dot(x0 + i as i32, round(mid as f32 - v[i] * amp as f32), 1, INK);
    }
}

/// SLEW: the walk slewed, over its straight ghost (DELTA at least 0.6, so
/// there is a walk to round).
fn slew(p: &mut Pix, w: &Wave) {
    let Wave::Func { fall, tilt, .. } = *w else {
        return;
    };
    let d = fall.unit().max(0.6);
    let mut v = [0.0; N];
    walk(d, 0.0, 5.0, 0, &mut v);
    p.plot(PADX, 24, 17, &v, thin(MID));
    walk(d, tilt.unit(), 5.0, 0, &mut v);
    p.plot(PADX, 24, 17, &v, BOLD);
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

    /// The pixels within `r² + r` of the centre.
    #[inline(never)]
    fn dot(&mut self, cx: i32, cy: i32, r: i32, c: Ink) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r + r {
                    self.px(cx + dx, cy + dy, c);
                }
            }
        }
    }

    /// A key: a 5 × 5 cap on a 3 × 1 stem, from (`x` − 2, `y`).
    fn keycap(&mut self, x: i32, y: i32, c: Ink) {
        self.rect(x - 2, y, 5, 5, c);
        self.rect(x - 1, y + 5, 3, 1, c);
    }

    /// `v` from column `x0`, `mid − v·amp` (clamped to ±1, half up): the
    /// fill to the middle under it, then the line, two rows when `thick`.
    /// The first column's y.
    #[inline(never)]
    fn plot(&mut self, x0: i32, mid: i32, amp: i32, v: &[f32], l: Line) -> i32 {
        let y = |i: usize| round(mid as f32 - clampf(v[i], -1.0, 1.0) * amp as f32);
        if let Some(fill) = l.fill {
            for i in 0..v.len() {
                let yi = y(i);
                let (a, b) = if yi < mid { (yi + 1, mid) } else { (mid, yi) };
                self.rect(x0 + i as i32, a, 1, b - a, fill);
            }
        }
        for i in 1..v.len() {
            let (a, b, x) = (y(i - 1), y(i), x0 + i as i32);
            self.line(x - 1, a, x, b, l.ink);
            if l.thick {
                self.line(x - 1, a + 1, x, b + 1, l.ink);
            }
        }
        y(0)
    }
}
