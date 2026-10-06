//! TAPE's strip (spec § 3): how SRC and DECK show what the machine does to
//! the source. A 216 × 40 px view of it, min/max a column in `MID`, START
//! and END marked; over it the machine's part in `ACCENT_DIM`, and its
//! heads in `ACCENT`, as the audio stores them once a block
//! (`instrument::HEADS`). The head's arrow points from START to END.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::clampf;
use crate::dsp::tape_engine::Heads;
use crate::dsp::tape_engine::heads::{Bounds, Dir, Window, min_frames};
use crate::dsp::tape_engine::oneshot::Slice;
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeModel};
use crate::dsp::tape_engine::read::{Shape, SourceView};
use crate::dsp::tape_engine::tron::tron_len_s;
use crate::ui::{draw, theme};

/// The strip's columns: one a pixel.
pub const COLS: usize = 216;
/// Its height, px.
pub const H: i32 = 40;
/// Its top on SRC and DECK: the viz band's middle.
pub const Y: i32 = theme::VIZ_BAND_MID - H / 2;

/// A source's min and max a column, its top 8 bits: the UI's one cache,
/// for the page shown, worked out when the page opens or SOURCE changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overview {
    cols: [[i8; 2]; COLS],
    frames: u32,
    rate: u16,
    /// A transwave's frames; 0 for any other shape.
    count: u8,
}

impl Overview {
    /// No source: a flat line.
    pub const EMPTY: Overview = Overview {
        cols: [[0; 2]; COLS],
        frames: 0,
        rate: 0,
        count: 0,
    };

    /// `[START, END]` of `v`, either way round, in `COLS` equal spans (a
    /// frame at least, repeated under `COLS` frames).
    #[inline(never)]
    pub fn of(v: &SourceView<'_>, start: f32, end: f32) -> Overview {
        let n = v.frames();
        let at = |x: f32| libm::roundf(clampf(x, 0.0, 1.0) * n as f32) as u32;
        let (lo, hi) = (at(start.min(end)), at(start.max(end)));
        let lo = lo.min(hi.max(1) - 1);
        let w = u64::from(hi.max(lo + 1) - lo);
        let d = v.desc();
        let mut o = Overview {
            frames: n,
            rate: d.rate,
            count: if d.shape == Shape::Transwave { d.count } else { 0 },
            ..Overview::EMPTY
        };
        let mut a = lo;
        for (c, col) in o.cols.iter_mut().enumerate() {
            let b = (lo + ((c as u64 + 1) * w / COLS as u64) as u32).max(a + 1);
            let (mut mn, mut mx) = (i32::MAX, i32::MIN);
            for i in a..b {
                let s = v.sample(i);
                (mn, mx) = (mn.min(s), mx.max(s));
            }
            *col = [(mn >> 4) as i8, (mx >> 4) as i8];
            a = lo + ((c as u64 + 1) * w / COLS as u64) as u32;
        }
        o
    }

    pub fn col(&self, c: usize) -> [i8; 2] {
        self.cols[c]
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    pub fn rate(&self) -> u16 {
        self.rate
    }

    /// Frame `f`'s column, from 0.
    fn column(&self, f: u32) -> i32 {
        (u64::from(f.min(self.frames - 1)) * COLS as u64 / u64::from(self.frames)) as i32
    }
}

/// What the viz region redraws on: the overview, the machine's params and
/// the heads to the column.
#[inline(never)]
pub fn key(o: &Overview, model: TapeModel, p: &TapeEngineParams, heads: Heads) -> u32 {
    let col = |h: Option<u32>| h.filter(|_| o.frames > 0).map_or(u32::MAX, |h| o.column(h) as u32);
    let words = [
        o.frames,
        u32::from(o.rate),
        model as u32,
        u32::from(p.slice),
        p.size.to_bits(),
        p.xfade.to_bits(),
        p.position.to_bits(),
        col(heads.head()),
        col(heads.second()),
    ];
    let cols = o.cols.iter().map(|c| u32::from(c[0] as u8) << 8 | u32::from(c[1] as u8));
    words
        .into_iter()
        .chain(cols)
        .fold(0x811C_9DC5, |k, w| (k ^ w).wrapping_mul(0x0100_0193))
}

/// `draw::fill_rect`, once: the strip's every rectangle calls it here.
#[inline(never)]
fn rect<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::fill_rect(d, x, y, w, h, color);
}

/// A 4 px triangle at `(x, y)` pointing along `(dx, dy)` (one of them 0).
fn tri<D>(d: &mut D, x: i32, y: i32, (dx, dy): (i32, i32), color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    for i in 0..4 {
        let r = 3 - i;
        if dy == 0 {
            rect(d, x + dx * i, y - r, 1, 2 * r + 1, color);
        } else {
            rect(d, x - r, y + dy * i, 2 * r + 1, 1, color);
        }
    }
}

/// The strip at `(x, y)`, 216 × 40 px (ticks hang 3 px below): `model`
/// over `o` at `p` (the page's values as they glide), `heads` the audio's.
#[inline(never)]
pub fn draw_strip<D>(
    d: &mut D,
    o: &Overview,
    model: TapeModel,
    p: &TapeEngineParams,
    heads: Heads,
    x: i32,
    y: i32,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let n = o.frames;
    if n == 0 {
        return rect(d, x, y + H / 2, COLS as i32, 1, theme::FAINT);
    }
    let col = |f: u32| x + o.column(f);
    let mut w = Window::of(p.start, p.end, n, min_frames(o.rate));
    if !model.ordered() {
        w.dir = Dir::Fwd;
    }
    let len = w.frames();
    let fwd = w.dir == Dir::Fwd;
    // `[a, b)` of the window, counted from START, as source frames.
    let place = |a: u32, b: u32| {
        if fwd {
            (w.lo + a, w.lo + b)
        } else {
            (w.hi - b, w.hi - a)
        }
    };
    let slice = |k: u64| w.lo + (k * u64::from(len) / 16) as u32;
    let (mut ramp, mut edges, mut ticks) = (None, None, 0u32);
    let lit = match model {
        TapeModel::Oneshot => {
            let s = Slice::of(f32::from(p.slice));
            ticks = if s == Slice::Off { 0 } else { 16 };
            // Slice k from START (`slice_bounds`), or under NOTE the one
            // the newest note plays.
            let k = match (s, heads.head()) {
                (Slice::At(k), _) if fwd => Some(u64::from(k)),
                (Slice::At(k), _) => Some(15 - u64::from(k)),
                (Slice::Note, Some(h)) if (w.lo..w.hi).contains(&h) => {
                    Some(u64::from(h - w.lo) * 16 / u64::from(len))
                }
                _ => None,
            };
            k.map_or((w.lo, w.hi), |k| (slice(k), slice(k + 1)))
        }
        // The tape SIZE gives at the newest note's pitch (the audio's), or
        // at ROOT's with none: from START to where it runs out, or END.
        TapeModel::Tron => match heads.second() {
            Some(out) if fwd => (w.lo, out.max(w.lo)),
            Some(out) => (out.min(w.hi), w.hi),
            None => {
                let run = (tron_len_s(p.size) * f32::from(o.rate)) as u32;
                place(0, run.min(len))
            }
        },
        TapeModel::Loop | TapeModel::PingPong => {
            let b = Bounds::of(0, len, (p.size, p.position, p.xfade), o.rate);
            if model == TapeModel::PingPong {
                edges = Some(place(b.ls, b.le));
            } else if b.lx > 0 {
                // The fade's incoming head: before the loop, or inside it
                // when there's no room before (folded).
                ramp = Some(if b.ls >= b.lx {
                    place(b.ls - b.lx, b.ls)
                } else {
                    place(b.ls, b.ls + b.lx)
                });
            }
            place(b.ls, b.le)
        }
        TapeModel::Transwave => {
            // The table's frames: the two read lit, else POSITION's.
            let c = u32::from(o.count.max(1));
            ticks = c;
            let f = |k: u32| (u64::from(k.min(c)) * u64::from(n) / u64::from(c)) as u32;
            let at = libm::roundf(clampf(p.position, 0.0, 1.0) * (c - 1) as f32) as u32;
            let h = heads.head().unwrap_or(at);
            (f(h), f(heads.second().unwrap_or(h).max(h) + 1))
        }
        TapeModel::Scrub | TapeModel::Stretch => (w.lo, w.hi),
    };
    let (l, r) = (col(lit.0), col(lit.1.max(lit.0 + 1) - 1));
    rect(d, l, y, r - l + 1, H, theme::ACCENT_DIM);

    let mid = y + H / 2;
    for (c, &[mn, mx]) in o.cols.iter().enumerate() {
        let px = |v: i8| i32::from(v) * (H / 2 - 1) / 128;
        let top = mid - px(mx);
        rect(d, x + c as i32, top, 1, mid - px(mn) - top + 1, theme::MID);
    }

    // Slices of the window, or a transwave's frames of the source.
    for k in 1..ticks {
        let f = if model == TapeModel::Transwave {
            (u64::from(k) * u64::from(n) / u64::from(ticks)) as u32
        } else {
            slice(u64::from(k))
        };
        rect(d, col(f), y + H, 1, 3, theme::INK2);
    }

    // START and END, START's flag pointing at END.
    let dir = if fwd { 1 } else { -1 };
    let (s, e) = if fwd {
        (w.lo, w.hi - 1)
    } else {
        (w.hi - 1, w.lo)
    };
    for f in [s, e] {
        rect(d, col(f), y, 1, H, theme::INK2);
    }
    rect(d, col(s).min(col(s) + 3 * dir), y, 4, 1, theme::INK2);

    if let Some((a, b)) = ramp {
        let (a, b) = if fwd { (a, b) } else { (b, a) };
        draw::line(d, col(a), y + H - 1, col(b), y, theme::INK2, 1);
    }
    if let Some((a, b)) = edges {
        tri(d, col(a), mid, (-1, 0), theme::INK2);
        tri(d, col(b), mid, (1, 0), theme::INK2);
    }
    if model == TapeModel::Scrub {
        // The finger.
        let f = w.lo + (clampf(p.position, 0.0, 1.0) * len as f32) as u32;
        tri(d, col(f), y, (0, 1), theme::INK);
    }
    if model == TapeModel::Transwave {
        return;
    }
    if let Some(h) = heads.second().filter(|_| model != TapeModel::Tron) {
        for dy in (0..H).step_by(2) {
            rect(d, col(h), y + dy, 1, 1, theme::ACCENT);
        }
    }
    if let Some(h) = heads.head() {
        rect(d, col(h), y, 1, H, theme::ACCENT);
        if model.ordered() {
            tri(d, col(h) + dir, y + 3, (dir, 0), theme::ACCENT);
        }
    }
}
