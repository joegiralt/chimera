//! SETTINGS › PROJECT › POOL: the pool's breakdown, read-only (sound-pool
//! spec § The meter, and the UI). The meter, then what fills it.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::project::Pool;
use crate::project::arena::{ARENA_BYTES, Cost};
use crate::ui::block_def::BlockDef;
use crate::ui::fmt::FmtBuf;
use crate::ui::meter::{self, MeterView};
use crate::ui::{components, theme};

/// The bar's top in the viz band.
pub const METER_Y: i32 = 152;

/// `SOUNDS 12 · 9.8K`, `SOURCES 2 · 52K`, `FREE 34K`, and `PENDING` only
/// while a hole waits; `None` is an empty cell.
pub fn cell_texts(pool: &Pool) -> [Option<FmtBuf>; 6] {
    let (sounds, sources, pending) = pool.split();
    let free = pool.free();
    core::array::from_fn(|i| {
        let mut b = FmtBuf::new();
        let _ = match i {
            0 => write!(
                b,
                "{} · {}",
                pool.sound_count(),
                sounds.kib_label().as_str()
            ),
            1 => write!(
                b,
                "{} · {}",
                pool.source_count(),
                sources.kib_label().as_str()
            ),
            // Floored: never more than there is.
            2 => b.write_str(free.kib_floor_label().as_str()),
            3 if pending > Cost::default() => b.write_str(pending.kib_label().as_str()),
            _ => return None,
        };
        Some(b)
    })
}

pub fn draw_cells<D>(d: &mut D, def: &BlockDef, pool: &Pool, top: i32, size: theme::TextSize)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, text) in cell_texts(pool).iter().enumerate() {
        let Some(text) = text else {
            components::cell(d, i, top, None, size);
            continue;
        };
        let slot = &def.params[i];
        let cell = components::Cell {
            label: slot.label(),
            text: text.as_str(),
            value: 0.0,
            fmt: slot.format(),
            active: false,
            mod_amount: None,
            look: components::Look::Live,
            motion: None,
            mini: None,
        };
        components::cell(d, i, top, Some(&cell), size);
    }
}

/// `USED 64%`, its arc the same share.
pub fn draw_focus<D>(d: &mut D, pool: &Pool, size: theme::TextSize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut text = FmtBuf::new();
    let _ = write!(text, "{}%", meter::percent(pool.used()));
    components::focus_band(
        d,
        "USED",
        text.as_str(),
        crate::ui::glyph::Gauge::Arc {
            value: pool.used().bytes() as f32 / ARENA_BYTES as f32,
            bipolar: false,
        },
        components::Look::Live,
        None,
        size,
    );
}

pub fn draw_viz<D>(d: &mut D, pool: &Pool)
where
    D: DrawTarget<Color = Rgb565>,
{
    meter::draw(d, &MeterView::of(pool, None), theme::MARGIN_X, METER_Y);
}

fn fold(c: Cost) -> u16 {
    let v = c.bytes() / crate::project::arena::GRANULE as u32;
    (v ^ (v >> 16)) as u16
}

/// What the cells show, for dirty tracking.
pub fn cells_key(pool: &Pool) -> [u16; 6] {
    let (sounds, sources, pending) = pool.split();
    [
        fold(sounds),
        pool.sound_count() as u16,
        fold(sources),
        pool.source_count() as u16,
        fold(pool.free()),
        fold(pending),
    ]
}

pub fn focus_key(pool: &Pool) -> u16 {
    meter::percent(pool.used()) as u16
}

/// The bar moves with any of its parts.
pub fn viz_key(pool: &Pool) -> u32 {
    let (sounds, sources, pending) = pool.split();
    sounds.bytes() ^ sources.bytes().rotate_left(11) ^ pending.bytes().rotate_left(22)
}
