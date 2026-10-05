//! Sound browser overlay in Direction A (UI refresh spec § Page types):
//! title, list rows (slot / name / engine tag) with the selected row an
//! accent pill and empty slots dimmed, each Sound's cost, a thin scroll
//! indicator, its gesture hints with the bytes they move, and the pool
//! meter (ADR 0087).

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use chimera_hal::{Controls, EncoderId};

use crate::params::EngineType;
use crate::preset::MAX_SOUNDS;
use crate::project::arena::Cost;
use crate::project::{PartActionKind, PartId, Pool, Project, SlotId, part_actions, save_needs};
use crate::ui::components;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{self, Gesture};
use crate::ui::meter::{self, MeterView};
use crate::ui::nav::chain_def_for;
use crate::ui::theme;

/// Rows on screen.
pub const VISIBLE_ROWS: usize = 8;
/// The pool's slots, then one init Sound per engine, in `EngineType::ALL`
/// order.
pub const INIT_TYPES: [EngineType; EngineType::ALL.len()] = EngineType::ALL;
pub const TOTAL_ENTRIES: usize = MAX_SOUNDS + INIT_TYPES.len();

/// The Sound rung's browser: its cursor and the first row shown. Only
/// `input` moves it; it opens at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Browse {
    cursor: u8,
    scroll: u8,
}

const _: () = assert!(TOTAL_ENTRIES <= 256);

impl Browse {
    pub const fn cursor(self) -> usize {
        self.cursor as usize
    }

    pub const fn scroll(self) -> usize {
        self.scroll as usize
    }

    /// Any cursor and scroll, unchecked: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn of(cursor: u8, scroll: u8) -> Self {
        Browse { cursor, scroll }
    }
}

/// The pool slot at list entry `entry`; `None` for the INIT rows.
pub fn slot_at(entry: usize) -> Option<SlotId> {
    SlotId::new(u8::try_from(entry).ok()?)
}

pub const LIST_TOP: i32 = 44;
pub const ROW_H: i32 = 26;
pub const SCROLL_X: i32 = theme::SCROLL_X;
pub const SCROLL_TOP: i32 = 40;
pub const SCROLL_H: i32 = 208;
/// The hint lines: EDIT's load, MIX + EDIT's save, MIX + MINUS's clear.
/// A refusal reads from the first.
pub const HINT_Y: i32 = 250;
pub const SAVE_Y: i32 = 266;
pub const CLEAR_Y: i32 = 282;
pub const INFO_Y: i32 = 300;
/// The pool meter's top, under its percentage.
pub const METER_Y: i32 = 306;
/// The refusal untracked: `POOL FULL · NEEDS 999K, 999K FREE` fits 216
/// px only so.
pub const REFUSAL_TRACKING: i32 = 0;
/// A row's cost, right-aligned short of its engine tag.
const COST_RIGHT: i32 = 196;
/// The keys and what they do here (Pre-flight 22, ADR 0086). SEQ's way up
/// is every chain's (ADR 0081), so it goes unsaid. MIX + EDIT's save says
/// what it costs; MIX + MINUS's clear, on a filled slot alone, what it
/// frees.
pub const LOAD: Gesture<'static> = Gesture("EDIT", "LOADS");
pub const SAVE_KEYS: &str = "MIX + EDIT";
pub const CLEAR_KEYS: &str = "MIX + MINUS";

/// MIX + EDIT's words: `SAVES · +0.1K`, the save against the slot it
/// replaces; `WON'T FIT` with none that fits.
pub fn save_words(cost: Option<Cost>) -> FmtBuf {
    let mut f = FmtBuf::new();
    let _ = match cost {
        Some(c) => write!(f, "SAVES · +{}", c.kib_label().as_str()),
        None => f.write_str("WON'T FIT"),
    };
    f
}

/// MIX + MINUS's words on a filled slot: `CLEARS · FREES 0.3K`.
pub fn clear_words(cost: Cost) -> FmtBuf {
    let mut f = FmtBuf::new();
    let _ = write!(f, "CLEARS · FREES {}", cost.kib_label().as_str());
    f
}

/// What MIX + EDIT's save of `part` takes, if one is offered: over its
/// slot, else to a new one, as `Offer::first_save` picks.
pub fn save_cost(p: &Project, part: PartId) -> Option<Cost> {
    part_actions(p, part)
        .iter()
        .any(|a| {
            matches!(
                a.kind(),
                PartActionKind::OverSlot(_) | PartActionKind::NewSlot(_)
            )
        })
        .then(|| save_needs(p, part))
}

/// Encoder A moves the cursor, clamped to the list; the scroll follows it
/// on screen. EDIT's load, MIX+EDIT's save, MIX+MINUS's clear and every
/// key's way out are `UiState`'s.
pub fn input(controls: &impl Controls, b: Browse) -> Browse {
    let visible = VISIBLE_ROWS.min(TOTAL_ENTRIES);
    let delta = i32::from(controls.encoder_delta(EncoderId::A));
    let cursor = (b.cursor as i32 + delta).clamp(0, TOTAL_ENTRIES as i32 - 1) as usize;
    let scroll = (b.scroll as usize)
        .min(cursor)
        .max((cursor + 1).saturating_sub(visible));
    // TOTAL_ENTRIES <= 256.
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
/// caller has cleared to `theme::BG`; its gesture hints with `hints`
/// (THEME's HINTS). A `refusal` reads in their place, in `ALERT`.
pub fn draw<D>(
    d: &mut D,
    project: &Project,
    part: PartId,
    cursor: usize,
    scroll: usize,
    hints: bool,
    refusal: Option<&str>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let pool = project.pool();
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

    if let Some(r) = refusal {
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            r,
            theme::MARGIN_X,
            HINT_Y,
            theme::ALERT,
            REFUSAL_TRACKING,
        );
    } else if hints {
        hint::draw(d, &[LOAD], theme::MARGIN_X, HINT_Y);
        let cost = save_cost(project, part);
        let words = save_words(cost);
        let does = if cost.is_some() {
            theme::MID
        } else {
            theme::ALERT
        };
        let save = Gesture(SAVE_KEYS, words.as_str());
        hint::gesture(d, &save, theme::MARGIN_X, SAVE_Y, does);
        if let Some(e) = slot_at(cursor).and_then(|s| pool.get(s)) {
            let words = clear_words(e.cost());
            hint::draw(
                d,
                &[Gesture(CLEAR_KEYS, words.as_str())],
                theme::MARGIN_X,
                CLEAR_Y,
            );
        }
    }
    let mut info = FmtBuf::new();
    let _ = write!(info, "{}%", meter::percent(pool.used()));
    draw::text_right(
        d,
        &theme::FONT_LABEL,
        info.as_str(),
        theme::VIZ_RIGHT,
        INFO_Y,
        theme::MID,
        0,
    );
    meter::draw(d, &MeterView::of(pool, None), theme::MARGIN_X, METER_Y);
}

fn row<D>(d: &mut D, pool: &Pool, entry: usize, y: i32, selected: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut slot = FmtBuf::new();
    let (name, chain, cost) = if let Some(s) = slot_at(entry) {
        let _ = write!(slot, "{:02}", entry + 1);
        match pool.get(s) {
            Some(s) => (
                components::upper(s.name().as_str()),
                Some(s.engine()),
                Some(s.cost()),
            ),
            None => (FmtBuf::new(), None, None),
        }
    } else {
        let _ = slot.write_str("INIT");
        let engine = INIT_TYPES[entry - MAX_SOUNDS];
        (components::upper(engine.label()), Some(engine), None)
    };
    let saved = cost.is_some();
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
    if let Some(c) = cost {
        let label = c.kib_label();
        draw::text_right(d, &theme::FONT_LABEL, label.as_str(), COST_RIGHT, y, dim, 0);
    }
    if let Some(engine) = chain {
        let chain = chain_def_for(engine);
        let label = chain.blocks[chain.engine_node()].def.short;
        draw::text_right(d, &theme::FONT_LABEL, label, 223, y, dim, 0);
    }
}
