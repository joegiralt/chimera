//! SETTINGS › SYSTEM › ABOUT: the firmware, the chip and the card, read-only.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::perf::load::AudioStats;
use crate::storage::Card;
use crate::ui::audio_page::NONE;
use crate::ui::block_def::BlockDef;
use crate::ui::fmt::FmtBuf;
use crate::ui::{components, draw, theme};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD: &str = if cfg!(debug_assertions) {
    "DEBUG"
} else {
    "RELEASE"
};

const NAME_Y: i32 = 110;
const LINE_Y: i32 = 136;
/// Clearance kept from the next cell's text.
const GAP: i32 = 4;

/// The card as its cell names it: its volume label, its serial without
/// one, `NO CARD` or `ERROR`. As last seen: no new card I/O.
fn card_text(b: &mut FmtBuf, card: Card) {
    let _ = match card {
        Card::Absent => b.write_str("NO CARD"),
        Card::Failed { .. } => b.write_str("ERROR"),
        Card::Ready(v) => match core::str::from_utf8(&v.label).map(str::trim) {
            Ok(l) if !l.is_empty() && l != "NO NAME" && l.is_ascii() => {
                let fits = |n: &usize| {
                    draw::text_width(&theme::FONT_VALUE, &l[..*n], 0) <= theme::CELL_COL_W - GAP
                };
                b.write_str(&l[..(1..=l.len()).rev().find(fits).unwrap_or(1)])
            }
            _ => write!(b, "{:08X}", v.serial),
        },
    };
}

pub fn cell_texts(s: Option<&AudioStats>, card: Card) -> [FmtBuf; 6] {
    core::array::from_fn(|i| {
        let mut b = FmtBuf::new();
        let _ = match (i, s) {
            (0, _) => b.write_str(VERSION),
            (1, _) => b.write_str(BUILD),
            (2, Some(s)) => b.write_str(s.rev.label()),
            (3, Some(s)) => write!(b, "{} MHZ", s.cpu_hz / 1_000_000),
            (4, Some(s)) => b.write_str(s.reset.label()),
            (5, _) => {
                card_text(&mut b, card);
                Ok(())
            }
            _ => b.write_str(NONE),
        };
        b
    })
}

pub fn draw_cells<D>(d: &mut D, def: &BlockDef, s: Option<&AudioStats>, card: Card, top: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, text) in cell_texts(s, card).iter().enumerate() {
        let slot = &def.params[i];
        let cell = components::Cell {
            label: slot.label(),
            text: text.as_str(),
            value: 0.0,
            fmt: slot.format(),
            active: false,
            mod_amount: None,
            look: components::Look::Live,
        };
        components::cell(d, i, top, Some(&cell));
    }
}

/// The name, and the firmware it runs.
pub fn draw_viz<D>(d: &mut D)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cx = theme::SCREEN_W / 2;
    draw::text_center(d, &theme::FONT_FOCUS, "CHIMERA", cx, NAME_Y, theme::INK, 0);
    let mut line = FmtBuf::new();
    let _ = write!(line, "FIRMWARE {VERSION}  {BUILD}");
    draw::text_center(
        d,
        &theme::FONT_LABEL,
        line.as_str(),
        cx,
        LINE_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    );
}

/// What the cells show, for dirty tracking.
pub fn cells_key(s: Option<&AudioStats>, card: Card) -> [u16; 6] {
    let h = cell_texts(s, card).map(|b| {
        b.as_str().bytes().fold(0x811c_9dc5u32, |h, c| {
            (h ^ c as u32).wrapping_mul(0x0100_0193)
        })
    });
    h.map(|h| (h ^ (h >> 16)) as u16)
}
