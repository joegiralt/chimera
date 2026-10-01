//! Sound browser overlay in Direction A (UI refresh spec § Page types):
//! title, list rows (slot / name / engine tag) with the selected row an
//! accent pill and empty slots dimmed, a thin scroll indicator, key hints.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use chimera_hal::{Controls, EncoderId};

use crate::params::EngineType;
use crate::preset::POOL_SIZE;
use crate::project::{PartId, Pool, SlotId};
use crate::ui::components;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::nav::{Browse, chain_def_for};
use crate::ui::theme;

/// Rows on screen.
pub const VISIBLE_ROWS: usize = 8;
/// The pool's slots, then one init Sound per engine, in `EngineType::ALL`
/// order.
pub const INIT_TYPES: [EngineType; EngineType::ALL.len()] = EngineType::ALL;
pub const TOTAL_ENTRIES: usize = POOL_SIZE + INIT_TYPES.len();

/// The pool slot at list entry `entry`; `None` for the INIT rows.
pub fn slot_at(entry: usize) -> Option<SlotId> {
    SlotId::new(u8::try_from(entry).ok()?)
}

pub const LIST_TOP: i32 = 44;
pub const ROW_H: i32 = 26;
pub const SCROLL_X: i32 = theme::SCROLL_X;
pub const SCROLL_TOP: i32 = 40;
pub const SCROLL_H: i32 = 208;
const HINT_Y: i32 = 284;
const INFO_Y: i32 = 304;

/// Encoder A moves the cursor, clamped to the list; the scroll follows it
/// on screen. EDIT's load and every key's way out are `UiState`'s.
pub fn input(controls: &impl Controls, b: Browse) -> Browse {
    let visible = VISIBLE_ROWS.min(TOTAL_ENTRIES);
    let delta = i32::from(controls.encoder_delta(EncoderId::A));
    let cursor = (b.cursor as i32 + delta).clamp(0, TOTAL_ENTRIES as i32 - 1) as usize;
    let scroll = (b.scroll as usize)
        .min(cursor)
        .max((cursor + 1).saturating_sub(visible));
    // TOTAL_ENTRIES <= 256 (`nav`).
    Browse {
        cursor: cursor as u8,
        scroll: scroll as u8,
    }
}

/// Baseline of visible row `i`.
pub fn row_y(i: usize) -> i32 {
    LIST_TOP + i as i32 * ROW_H
}

/// The full-screen browser for Part `part` (0-based), on a screen the
/// caller has cleared to `theme::BG`.
pub fn draw<D>(d: &mut D, pool: &Pool, part: PartId, cursor: usize, scroll: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut name = FmtBuf::new();
    let _ = write!(name, "PART {}", part.index() + 1);
    components::title_to(d, "LOAD SOUND", name.as_str());

    for i in 0..VISIBLE_ROWS {
        let entry = scroll + i;
        if entry >= TOTAL_ENTRIES {
            break;
        }
        row(d, pool, entry, row_y(i), entry == cursor);
    }

    // Scroll position.
    draw::fill_rect(d, SCROLL_X, SCROLL_TOP, 2, SCROLL_H, theme::FAINT);
    let thumb_h = (SCROLL_H * VISIBLE_ROWS as i32 / TOTAL_ENTRIES as i32).max(8);
    let max_scroll = (TOTAL_ENTRIES - VISIBLE_ROWS) as i32;
    let thumb_y =
        SCROLL_TOP + (SCROLL_H - thumb_h) * scroll.min(max_scroll as usize) as i32 / max_scroll;
    draw::fill_rect(d, SCROLL_X, thumb_y, 2, thumb_h, theme::MID);

    for (i, (key, what)) in [("EDIT", "LOAD"), ("SEQ", "SAVE"), ("B", "CANCEL")]
        .iter()
        .enumerate()
    {
        let x = theme::MARGIN_X + i as i32 * 76;
        let w = draw::text_tracked(
            d,
            &theme::FONT_LABEL_BOLD,
            key,
            x,
            HINT_Y,
            theme::INK,
            theme::LABEL_TRACKING,
        );
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            what,
            x + w + 4,
            HINT_Y,
            theme::MID,
            theme::LABEL_TRACKING,
        );
    }
    // Two strings: FmtBuf holds 32 bytes and one line would not fit.
    draw::text(
        d,
        &theme::FONT_LABEL,
        "A SCROLLS",
        theme::MARGIN_X,
        INFO_Y,
        theme::MID,
    );
    let mut info = FmtBuf::new();
    let _ = write!(info, "{} INIT + {} SLOTS", INIT_TYPES.len(), POOL_SIZE);
    draw::text_right(
        d,
        &theme::FONT_LABEL,
        info.as_str(),
        theme::VIZ_RIGHT,
        INFO_Y,
        theme::MID,
        0,
    );
}

fn row<D>(d: &mut D, pool: &Pool, entry: usize, y: i32, selected: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut slot = FmtBuf::new();
    let (name, chain, saved) = if let Some(s) = slot_at(entry) {
        let _ = write!(slot, "{:02}", entry + 1);
        match pool.get(s) {
            Some(s) => (components::upper(s.name.as_str()), Some(s.engine()), true),
            None => (FmtBuf::new(), None, false),
        }
    } else {
        let _ = slot.write_str("INIT");
        let engine = INIT_TYPES[entry - POOL_SIZE];
        (components::upper(engine.label()), Some(engine), false)
    };
    let empty = chain.is_none();
    if selected {
        draw::pill(d, 8, y - 16, 224, 22, theme::ACCENT);
    }
    let dim = if selected { theme::BG } else { theme::MID };
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        slot.as_str(),
        18,
        y,
        dim,
        theme::LABEL_TRACKING,
    );
    if empty {
        draw::fill_rect(
            d,
            52,
            y - 4,
            10,
            1,
            if selected { theme::BG } else { theme::FAINT },
        );
    } else {
        let color = if selected {
            theme::BG
        } else if saved {
            theme::INK
        } else {
            theme::INK2
        };
        draw::text(d, &theme::FONT_VALUE, name.as_str(), 52, y, color);
    }
    if let Some(engine) = chain {
        let chain = chain_def_for(engine);
        let label = chain.blocks[chain.engine_node()].def.short;
        draw::text_right(d, &theme::FONT_LABEL, label, 223, y, dim, 0);
    }
}
