//! The strip's raster (ADR 0175): bank 2's with `bank2` (`bank2.x`'s
//! `tape_strip6raster` glob), reached through the UI's table; the
//! wrappers in `tape_strip` are bank 1's.

use embedded_graphics::pixelcolor::Rgb565;

use super::{COL_READS, COLS, H, Overview, Paint};
use crate::dsp::clampf;
use crate::dsp::tape_engine::Heads;
use crate::dsp::tape_engine::heads::{Bounds, Dir, Window, min_frames};
use crate::dsp::tape_engine::oneshot::Slice;
use crate::dsp::tape_engine::params::{PlayMode, TapeEngineParams, TapeModel};
use crate::dsp::tape_engine::read::{Shape, SourceView};
use crate::dsp::tape_engine::tron::tron_len_s_in;
use crate::stream::units::SourceRate;
use crate::ui::theme;

impl Overview {
    /// `[START, END]` of `v`, either way round, in `COLS` equal spans (a
    /// frame at least, repeated under `COLS` frames). A span is read whole
    /// up to `COL_READS` frames, then strided to that many: a long source
    /// costs a UI frame no more than a 0.3 s one, and a peak between
    /// strides may go undrawn.
    pub(crate) fn of_in(v: &SourceView<'_>, start: f32, end: f32) -> Overview {
        let n = v.frames();
        let at = |x: f32| libm::roundf(clampf(x, 0.0, 1.0) * n as f32) as u32;
        let (lo, hi) = (at(start.min(end)), at(start.max(end)));
        let lo = lo.min(hi.max(1) - 1);
        let w = u64::from(hi.max(lo + 1) - lo);
        let d = v.desc();
        let mut o = Overview {
            frames: n,
            rate: Some(d.rate()),
            count: if d.shape() == Shape::Transwave {
                d.count()
            } else {
                0
            },
            ..Overview::EMPTY
        };
        let mut a = lo;
        for (c, col) in o.cols.iter_mut().enumerate() {
            let b = (lo + ((c as u64 + 1) * w / COLS as u64) as u32).max(a + 1);
            let (mut mn, mut mx) = (i32::MAX, i32::MIN);
            let stride = ((b - a) / COL_READS).max(1) as usize;
            for i in (a..b).step_by(stride) {
                let s = v.sample(i);
                (mn, mx) = (mn.min(s), mx.max(s));
            }
            *col = [(mn >> 4) as i8, (mx >> 4) as i8];
            a = lo + ((c as u64 + 1) * w / COLS as u64) as u32;
        }
        o
    }
}

/// What the viz region redraws on: the overview, the machine's params and
/// the heads to the column.
pub(crate) fn key_in(o: &Overview, model: TapeModel, p: &TapeEngineParams, heads: Heads) -> u32 {
    let col = |h: Option<u32>| {
        h.filter(|_| o.frames > 0)
            .map_or(u32::MAX, |h| o.column(h) as u32)
    };
    let words = [
        o.frames,
        o.rate.map_or(0, u32::from),
        model as u32,
        p.mode as u32,
        u32::from(p.slice.code()),
        p.size.to_bits(),
        p.xfade.to_bits(),
        p.position.to_bits(),
        col(heads.head()),
        col(heads.second()),
    ];
    let cols = o
        .cols
        .iter()
        .map(|c| u32::from(c[0] as u8) << 8 | u32::from(c[1] as u8));
    words
        .into_iter()
        .chain(cols)
        .fold(0x811C_9DC5, |k, w| (k ^ w).wrapping_mul(0x0100_0193))
}

/// The strip's every rectangle.
fn rect(d: &mut dyn Paint, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
    d.rect(x, y, w, h, color);
}

/// A 4 px triangle at `(x, y)` pointing along `(dx, dy)` (one of them 0).
fn tri(d: &mut dyn Paint, x: i32, y: i32, (dx, dy): (i32, i32), color: Rgb565) {
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
pub(crate) fn strip_in(
    d: &mut dyn Paint,
    o: &Overview,
    model: TapeModel,
    p: &TapeEngineParams,
    heads: Heads,
    (x, y): (i32, i32),
) {
    let n = o.frames;
    if n == 0 {
        return rect(d, x, y + H / 2, COLS as i32, 1, theme::FAINT);
    }
    // Only `EMPTY` has no rate, and no frames.
    let rate = o.rate.unwrap_or(SourceRate::MIN);
    let col = |f: u32| x + o.column(f);
    let mut w = Window::of(p.start, p.end, n, min_frames(rate));
    if !model.ordered(p.mode) {
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
    let lit = match (model, p.mode) {
        (TapeModel::Play, PlayMode::One) => {
            let s = p.slice.slice();
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
        (TapeModel::Tron, _) => match heads.second() {
            Some(out) if fwd => (w.lo, out.max(w.lo)),
            Some(out) => (out.min(w.hi), w.hi),
            None => {
                let strip = len as f32 / f32::from(rate);
                let run = (tron_len_s_in(p.size, strip) * f32::from(rate)) as u32;
                place(0, run.min(len))
            }
        },
        (TapeModel::Play, _) => {
            let b = Bounds::of(0, len, (p.size, p.position, p.xfade), rate);
            if p.mode == PlayMode::Ping {
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
        (TapeModel::Transwave, _) => {
            // The table's frames: the two read lit, else POSITION's.
            let c = u32::from(o.count.max(1));
            ticks = c;
            let f = |k: u32| (u64::from(k.min(c)) * u64::from(n) / u64::from(c)) as u32;
            let at = libm::roundf(clampf(p.position, 0.0, 1.0) * (c - 1) as f32) as u32;
            let h = heads.head().unwrap_or(at);
            (f(h), f(heads.second().unwrap_or(h).max(h) + 1))
        }
        (TapeModel::Scrub | TapeModel::Stretch, _) => (w.lo, w.hi),
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
        d.line(col(a), y + H - 1, col(b), y, theme::INK2);
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
        if model.ordered(p.mode) {
            tri(d, col(h) + dir, y + 3, (dir, 0), theme::ACCENT);
        }
    }
}
