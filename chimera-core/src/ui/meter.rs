//! The pool meter (ADR 0087): one bar of the arena, Sounds then sources
//! then pending holes then free, with an add's preview after the used
//! part. A readout of pool state, so it redraws at its width, never lerps.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::project::Pool;
use crate::project::arena::{ARENA_BYTES, Cost};
use crate::ui::{draw, theme};

/// The screen less its margins (ADR 0086's measure).
pub const W: i32 = theme::CONTENT_W;
pub const H: i32 = 4;

/// What the bar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeterView {
    pub sounds: Cost,
    pub sources: Cost,
    pub pending: Cost,
    /// An add's cost, drawn after the used part.
    pub preview: Option<Cost>,
}

impl MeterView {
    pub fn of(pool: &Pool, preview: Option<Cost>) -> MeterView {
        let (sounds, sources, pending) = pool.split();
        MeterView {
            sounds,
            sources,
            pending,
            preview,
        }
    }

    pub fn used(&self) -> u32 {
        self.sounds.bytes() + self.sources.bytes() + self.pending.bytes()
    }
}

/// `⌈used × 100 / ARENA_BYTES⌉`: never emptier than it is; empty reads 0.
pub fn percent(used: Cost) -> u8 {
    (used.bytes() as u64 * 100)
        .div_ceil(ARENA_BYTES as u64)
        .min(100) as u8
}

/// Where `bytes` from the bar's start ends, rounded up: any bytes show.
fn edge(bytes: u32) -> i32 {
    (u64::from(bytes) * W as u64)
        .div_ceil(ARENA_BYTES as u64)
        .min(W as u64) as i32
}

/// The bar, `W` × `H` from `x`, `y` its top.
pub fn draw<D>(d: &mut D, v: &MeterView, x: i32, y: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let a = edge(v.sounds.bytes());
    let b = edge(v.sounds.bytes() + v.sources.bytes());
    let used = v.used();
    let c = edge(used);
    let mut seg = |from: i32, to: i32, color| {
        if to > from {
            draw::fill_rect(d, x + from, y, to - from, H, color);
        }
    };
    seg(0, a, theme::INK);
    seg(a, b, theme::MID);
    seg(b, c, theme::FAINT);
    seg(c, W, theme::BAR_REST);
    if let Some(p) = v.preview {
        if used + p.bytes() <= ARENA_BYTES as u32 {
            seg(c, edge(used + p.bytes()), theme::ACCENT);
        } else {
            seg(c, W, theme::ALERT);
        }
    }
}
