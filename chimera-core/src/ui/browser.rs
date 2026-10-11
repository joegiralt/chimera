//! Sound browser overlay in Direction A (UI refresh spec § Page types):
//! title, list rows (slot / name / engine tag) with the selected row an
//! accent pill and empty slots dimmed, each Sound's cost, a thin scroll
//! indicator, its gesture hints with the bytes they move, and the pool
//! meter (ADR 0087).

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::FontRenderer;

use chimera_hal::{Controls, EncoderId};

use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::MAX_SOUNDS;
use crate::project::arena::Cost;
use crate::project::{Line, PartId, Pool, Project, SlotId};
use crate::storage::{BUILD_LEN, BuildStamp, HEADER_LEN, TRAILER_LEN};
use crate::ui::components;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{self, Gesture};
use crate::ui::library::{LibRow, LibState, Tab, row as lib_row_at, rows as lib_rows};
use crate::ui::meter::{self, MeterView};
use crate::ui::settings::part::Offer;
use crate::ui::settings::prompt::POOL_FULL;
use crate::ui::theme;

/// Rows on screen.
pub const VISIBLE_ROWS: usize = 8;
/// One init Sound per engine, in `EngineType::ALL` order, then the pool's
/// slots.
pub const INIT_TYPES: [EngineType; EngineType::ALL.len()] = EngineType::ALL;
pub const INITS: usize = INIT_TYPES.len();
pub const TOTAL_ENTRIES: usize = INITS + MAX_SOUNDS;

/// The Sound rung's browser: each tab's cursor and first row shown. Only
/// `input`, `input_library` and a listing's landing move them; PROJECT
/// opens on slot 1, the INIT rows just above, LIBRARY at its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Browse {
    cursor: u8,
    scroll: u8,
    lib_cursor: u8,
    lib_scroll: u8,
}

const _: () = assert!(TOTAL_ENTRIES <= 256);

impl Default for Browse {
    fn default() -> Self {
        Browse {
            cursor: INITS as u8,
            scroll: 0,
            lib_cursor: 0,
            lib_scroll: 0,
        }
    }
}

impl Browse {
    pub const fn cursor(self) -> usize {
        self.cursor as usize
    }

    pub const fn scroll(self) -> usize {
        self.scroll as usize
    }

    pub const fn lib_cursor(self) -> usize {
        self.lib_cursor as usize
    }

    pub const fn lib_scroll(self) -> usize {
        self.lib_scroll as usize
    }

    /// LIBRARY's cursor on `cursor` of `rows`, the scroll following it.
    pub(crate) fn lib_at(self, cursor: usize, rows: usize) -> Self {
        let (cursor, scroll) = follow(cursor, self.lib_scroll(), rows);
        Browse {
            lib_cursor: cursor,
            lib_scroll: scroll,
            ..self
        }
    }

    /// Any cursor and scroll, unchecked: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn of(cursor: u8, scroll: u8) -> Self {
        Browse {
            cursor,
            scroll,
            lib_cursor: 0,
            lib_scroll: 0,
        }
    }
}

/// `cursor` clamped to `rows`, and a scroll from `scroll` that shows it.
fn follow(cursor: usize, scroll: usize, rows: usize) -> (u8, u8) {
    let visible = VISIBLE_ROWS.min(rows);
    let cursor = cursor.min(rows.saturating_sub(1));
    let scroll = scroll.min(cursor).max((cursor + 1).saturating_sub(visible));
    // Both lists are under 256 rows.
    (cursor as u8, scroll as u8)
}

/// The pool slot at list entry `entry`; `None` for the INIT rows.
pub fn slot_at(entry: usize) -> Option<SlotId> {
    SlotId::new(u8::try_from(entry.checked_sub(INITS)?).ok()?)
}

/// The engine of the INIT row at `entry`; `None` for the pool's slots.
pub fn init_at(entry: usize) -> Option<EngineType> {
    INIT_TYPES.get(entry).copied()
}

pub const LIST_TOP: i32 = 44;
pub const ROW_H: i32 = 26;
pub const SCROLL_X: i32 = theme::SCROLL_X;
pub const SCROLL_TOP: i32 = 40;
pub const SCROLL_H: i32 = 208;
/// The hint lines: EDIT's load, MIX + EDIT's save, MIX + MINUS's clear.
/// A refusal takes the first two: `POOL FULL`, then what it needs.
pub const HINT_Y: i32 = 250;
pub const SAVE_Y: i32 = 266;
pub const CLEAR_Y: i32 = 282;
pub const INFO_Y: i32 = 300;
/// The pool meter's top, under its percentage.
pub const METER_Y: i32 = 306;
/// A row's cost, right-aligned short of its engine tag.
const COST_RIGHT: i32 = 184;
/// Where a row's name starts, and its clearance from the cost or tag.
const NAME_X: i32 = 52;
const NAME_GAP: i32 = 6;
/// The engine tag's right edge.
pub const TAG_RIGHT: i32 = 223;
/// The widest engine tag, `MODAL`.
const TAG_ROOM: i32 = 38;
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

/// LIBRARY's copy (library spec § Browser).
pub const EMPTY_LIBRARY: &str = "NO SOUNDS YET";
pub const NEWER: &str = "‹ NEWER";
pub const OLDER: &str = "OLDER ›";
pub const SAVE_TO_LIBRARY: Gesture<'static> = Gesture("MIX + EDIT", "SAVES TO LIBRARY");
/// Line 3, the selected row's stamp: information, drawn with HINTS off.
pub const STAMP_Y: i32 = CLEAR_Y;

/// A listed file's cost in the pool: its bytes less the frame and the
/// `Build` record (head + payload), plus the 16 B entry header; a granule a row.
pub fn entry_cost(len: u32) -> Cost {
    let body = (len as usize).saturating_sub(HEADER_LEN + TRAILER_LEN + 4 + BUILD_LEN);
    Cost::of(body + 16)
}

/// EDIT's words on a LIBRARY row: `LOADS · +0.3K`, `WON'T FIT` with none
/// that fits.
pub fn load_words(cost: Option<Cost>) -> FmtBuf {
    let mut f = FmtBuf::new();
    let _ = match cost {
        Some(c) => write!(f, "LOADS · +{}", c.kib_label().as_str()),
        None => f.write_str("WON'T FIT"),
    };
    f
}

/// Line 3: `SAVED ON 0.1.0 02E440E2` (`+` after a dirty hash),
/// `SAVED ON THIS BUILD`, or `NO STAMP`.
pub fn stamp_words(stamp: Option<BuildStamp>, this: &BuildStamp) -> FmtBuf {
    let mut f = FmtBuf::new();
    let _ = match stamp {
        Some(s) if s.is_this_build(this) => f.write_str("SAVED ON THIS BUILD"),
        Some(s) => {
            let mut b = FmtBuf::new();
            let _ = write!(b, "{s}");
            write!(f, "SAVED ON {}", components::upper(b.as_str()).as_str())
        }
        None => f.write_str("NO STAMP"),
    };
    f
}

/// MIX + MINUS's words on a filled slot: `CLEARS · FREES 0.3K`.
pub fn clear_words(cost: Cost) -> FmtBuf {
    let mut f = FmtBuf::new();
    let _ = write!(f, "CLEARS · FREES {}", cost.kib_label().as_str());
    f
}

/// The orphan line's room: left of the meter's percentage, `100%` at
/// its widest.
pub const ORPHAN_W: i32 = 168;

/// Under MIX + MINUS's line, on a slot whose clear leaves its source
/// unused: `CHOIR 41K UNUSED`, the source's cost, the name cut to fit
/// `ORPHAN_W`. The spec's one line, `CLEARS · CHOIR 41K UNUSED`, is wider
/// than the screen beside the keys.
pub fn orphan_words(name: SoundName, cost: Cost) -> Line {
    let name = components::upper(name.as_str());
    let mut n = name.as_str();
    loop {
        let mut l = Line::new(n);
        let _ = write!(l, " {} UNUSED", cost.kib_label().as_str());
        let w = draw::text_width(&theme::FONT_LABEL, l.as_str(), theme::LABEL_TRACKING);
        match n.char_indices().last() {
            Some((cut, _)) if w > ORPHAN_W => n = &n[..cut],
            _ => return l,
        }
    }
}

/// Encoder A moves the cursor, clamped to the list; the scroll follows it
/// on screen. EDIT's load, MIX+EDIT's save, MIX+MINUS's clear and every
/// key's way out are `UiState`'s.
pub fn input(controls: &impl Controls, b: Browse) -> Browse {
    let cursor = moved(controls, b.cursor());
    let (cursor, scroll) = follow(cursor, b.scroll(), TOTAL_ENTRIES);
    Browse {
        cursor,
        scroll,
        ..b
    }
}

/// `input` on LIBRARY's `rows`.
pub fn input_library(controls: &impl Controls, b: Browse, rows: usize) -> Browse {
    b.lib_at(moved(controls, b.lib_cursor()), rows)
}

/// `cursor` moved by encoder A, not below 0.
fn moved(controls: &impl Controls, cursor: usize) -> usize {
    let delta = i32::from(controls.encoder_delta(EncoderId::A));
    (cursor as i32 + delta).max(0) as usize
}

/// Baseline of visible row `i`.
pub fn row_y(i: usize) -> i32 {
    LIST_TOP + i as i32 * ROW_H
}

/// What the browser lists: PROJECT's pool slots, or LIBRARY's page with
/// this build's stamp.
#[derive(Clone, Copy)]
pub enum BrowserView<'a> {
    Project,
    Library(&'a LibState, &'a BuildStamp),
}

/// The title row: `PROJECT  LIBRARY  B ◂▸`, the current tab in `INK`, then
/// `· P1` right-aligned (ADR 0086's marks).
fn title<D>(d: &mut D, tab: Tab, part: PartId)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let mut x = theme::MARGIN_X;
    for (t, label) in [(Tab::Project, "PROJECT"), (Tab::Library, "LIBRARY")] {
        let ink = if t == tab { theme::INK } else { theme::FAINT };
        let font = &theme::FONT_LABEL_BOLD;
        x += draw::text_tracked(d, font, label, x, y, ink, theme::LABEL_TRACKING) + 10;
    }
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        "B ◂▸",
        x,
        y,
        theme::MID,
        theme::LABEL_TRACKING,
    );
    let mut p = FmtBuf::new();
    let _ = write!(p, "· P{}", part.index() + 1);
    draw::text_right(
        d,
        &theme::FONT_LABEL,
        p.as_str(),
        theme::VIZ_RIGHT,
        y,
        theme::MID,
        0,
    );
}

/// The scroll position of `total` rows; none when they all show.
fn scroll_bar<D>(d: &mut D, scroll: usize, total: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    if total <= VISIBLE_ROWS {
        return;
    }
    draw::fill_rect(d, SCROLL_X, SCROLL_TOP, 2, SCROLL_H, theme::FAINT);
    let thumb_h = (SCROLL_H * VISIBLE_ROWS as i32 / total as i32).max(8);
    let max_scroll = (total - VISIBLE_ROWS) as i32;
    let thumb_y =
        SCROLL_TOP + (SCROLL_H - thumb_h) * scroll.min(max_scroll as usize) as i32 / max_scroll;
    draw::fill_rect(d, SCROLL_X, thumb_y, 2, thumb_h, theme::MID);
}

/// A refusal reads in the first two hint lines, in `ALERT`.
fn refusal_lines<D>(d: &mut D, r: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (line, y) in [(POOL_FULL, HINT_Y), (r, SAVE_Y)] {
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            line,
            theme::MARGIN_X,
            y,
            theme::ALERT,
            theme::LABEL_TRACKING,
        );
    }
}

/// The meter and its percentage, `preview` after the used part.
fn meter_line<D>(d: &mut D, pool: &Pool, preview: Option<Cost>)
where
    D: DrawTarget<Color = Rgb565>,
{
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
    meter::draw(d, &MeterView::of(pool, preview), theme::MARGIN_X, METER_Y);
}

/// The full-screen browser for Part `part` (0-based), on a screen the
/// caller has cleared to `theme::BG`; its gesture hints with `hints`
/// (THEME's HINTS). A `refusal` reads in their place, in `ALERT`.
#[allow(clippy::too_many_arguments)]
pub fn draw<D>(
    d: &mut D,
    project: &Project,
    view: BrowserView<'_>,
    part: PartId,
    cursor: usize,
    scroll: usize,
    hints: bool,
    refusal: Option<&str>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    match view {
        BrowserView::Project => {
            title(d, Tab::Project, part);
            project_body(d, project, part, cursor, scroll, hints, refusal);
        }
        BrowserView::Library(lib, this) => {
            title(d, Tab::Library, part);
            library_body(d, project.pool(), lib, this, cursor, scroll, hints, refusal);
        }
    }
}

fn project_body<D>(
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
    for i in 0..VISIBLE_ROWS {
        let entry = scroll + i;
        if entry >= TOTAL_ENTRIES {
            break;
        }
        row(d, pool, entry, row_y(i), entry == cursor);
    }
    scroll_bar(d, scroll, TOTAL_ENTRIES);

    if let Some(r) = refusal {
        refusal_lines(d, r);
    } else if hints {
        hint::draw(d, &[LOAD], theme::MARGIN_X, HINT_Y);
        let cost = Offer::of(project, part).save_cost();
        let words = save_words(cost);
        let does = if cost.is_some() {
            theme::MID
        } else {
            theme::ALERT
        };
        let save = Gesture(SAVE_KEYS, words.as_str());
        hint::gesture(d, &save, theme::MARGIN_X, SAVE_Y, does);
        if let Some((s, e)) = slot_at(cursor).and_then(|s| Some((s, pool.get(s)?))) {
            if let Some(c) = project.orphaned_by_clear(s) {
                let words = orphan_words(e.name(), c);
                draw::text_tracked(
                    d,
                    &theme::FONT_LABEL,
                    words.as_str(),
                    theme::MARGIN_X,
                    INFO_Y,
                    theme::WARN,
                    theme::LABEL_TRACKING,
                );
            }
            let words = clear_words(e.cost());
            hint::draw(
                d,
                &[Gesture(CLEAR_KEYS, words.as_str())],
                theme::MARGIN_X,
                CLEAR_Y,
            );
        }
    }
    meter_line(d, pool, None);
}

/// A LIBRARY Sound's cost if it fits the pool, a free slot included.
fn fitting(pool: &Pool, e: &crate::project::LibEntry) -> Option<Cost> {
    let c = entry_cost(e.len);
    (pool.first_free().is_some() && pool.fits(c)).then_some(c)
}

#[allow(clippy::too_many_arguments)]
fn library_body<D>(
    d: &mut D,
    pool: &Pool,
    lib: &LibState,
    this: &BuildStamp,
    cursor: usize,
    scroll: usize,
    hints: bool,
    refusal: Option<&str>,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let page = lib.page();
    let total = lib_rows(page);
    for i in 0..VISIBLE_ROWS {
        let at = scroll + i;
        if at >= total {
            break;
        }
        let r = lib_row_at(page, at);
        library_row(d, lib, r, row_y(i), at == cursor);
    }
    scroll_bar(d, scroll, total);

    let on = match lib_row_at(page, cursor) {
        LibRow::Sound(e) => Some(e),
        _ => None,
    };
    // A listing that failed partway: its words under the rows it left.
    if let (Some(err), Some(p)) = (lib.err(), page)
        && !p.entries().is_empty()
    {
        let msg = clip(&theme::FONT_LABEL, err.message(), ORPHAN_W);
        let (x, y, c) = (theme::MARGIN_X, INFO_Y, theme::WARN);
        draw::text_tracked(d, &theme::FONT_LABEL, msg, x, y, c, theme::LABEL_TRACKING);
    }
    let fit = on.filter(|e| e.err.is_none()).map(|e| fitting(pool, e));
    if let Some(r) = refusal {
        refusal_lines(d, r);
    } else if hints {
        if let Some(fit) = fit {
            let words = load_words(fit);
            let does = if fit.is_some() {
                theme::MID
            } else {
                theme::ALERT
            };
            let load = Gesture(LOAD.0, words.as_str());
            hint::gesture(d, &load, theme::MARGIN_X, HINT_Y, does);
        }
        hint::draw(d, &[SAVE_TO_LIBRARY], theme::MARGIN_X, SAVE_Y);
    }
    if let Some(e) = on {
        let words = stamp_words(e.stamp, this);
        let (x, y, c) = (theme::MARGIN_X, STAMP_Y, theme::MID);
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            words.as_str(),
            x,
            y,
            c,
            theme::LABEL_TRACKING,
        );
    }
    let preview = on.filter(|e| e.err.is_none()).map(|e| entry_cost(e.len));
    meter_line(d, pool, preview);
}

/// Where a LIBRARY row's name starts.
const LIB_NAME_X: i32 = 18;

/// One LIBRARY row at baseline `y`.
fn library_row<D>(d: &mut D, lib: &LibState, row: LibRow<'_>, y: i32, selected: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    // The lone `Empty` row is never a selection: inert, dim, no pill.
    let selected = selected && !matches!(row, LibRow::Empty);
    if selected {
        draw::pill(d, 8, y - 16, 224, 22, theme::ACCENT);
    }
    let dim = if selected { theme::BG } else { theme::MID };
    let value = |d: &mut D, s: &str, color| {
        let font = &theme::FONT_VALUE;
        draw::text_tracked(d, font, s, LIB_NAME_X, y, color, 0);
    };
    let e = match row {
        LibRow::Newer => return value(d, NEWER, dim),
        LibRow::Older => return value(d, OLDER, dim),
        LibRow::Empty => {
            let msg = lib.err().map_or(EMPTY_LIBRARY, |n| n.message());
            return value(d, msg, dim);
        }
        LibRow::Sound(e) => e,
    };
    // Greyed: the name dimmed, the `FileError` in place of cost and tag.
    let ink = match (selected, e.err) {
        (true, _) => theme::BG,
        (false, Some(_)) => theme::MID,
        (false, None) => theme::INK,
    };
    let right = match e.err {
        Some(err) => {
            let m = err.message();
            draw::text_right(d, &theme::FONT_LABEL, m, TAG_RIGHT, y, dim, 0);
            draw::text_width(&theme::FONT_LABEL, m, 0)
        }
        None => {
            let label = entry_cost(e.len).kib_label();
            draw::text_right(d, &theme::FONT_LABEL, label.as_str(), COST_RIGHT, y, dim, 0);
            if let Some(engine) = e.engine {
                tag(d, engine, y, dim);
            }
            TAG_RIGHT - COST_RIGHT + draw::text_width(&theme::FONT_LABEL, label.as_str(), 0)
        }
    };
    let room = TAG_RIGHT - right - NAME_GAP - LIB_NAME_X;
    match e.seen.name {
        Some(n) => value(
            d,
            clip(
                &theme::FONT_VALUE,
                components::upper(n.as_str()).as_str(),
                room,
            ),
            ink,
        ),
        // No readable name: PROJECT's dash.
        None => {
            let dash = if selected { theme::BG } else { theme::FAINT };
            draw::fill_rect(d, LIB_NAME_X, y - 4, 10, 1, dash);
        }
    }
}

/// `engine`'s name in full, right-aligned at `TAG_RIGHT`.
fn tag<D>(d: &mut D, engine: EngineType, y: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let label = components::upper(engine.label());
    draw::text_right(
        d,
        &theme::FONT_LABEL,
        label.as_str(),
        TAG_RIGHT,
        y,
        color,
        0,
    );
}

/// The longest start of `s` that fits `w` px in `font`.
fn clip<'s>(font: &FontRenderer, s: &'s str, w: i32) -> &'s str {
    let mut end = s.len();
    while end > 0 && draw::text_width(font, &s[..end], 0) > w {
        end -= 1;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
    }
    &s[..end]
}

fn row<D>(d: &mut D, pool: &Pool, entry: usize, y: i32, selected: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut slot = FmtBuf::new();
    let (name, chain, cost) = if let Some(s) = slot_at(entry) {
        let _ = write!(slot, "{:02}", s.index() + 1);
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
        let engine = INIT_TYPES[entry];
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
            NAME_X,
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
        // A long name stops short of the cost, or of the tag.
        let room = match cost {
            Some(c) => COST_RIGHT - draw::text_width(&theme::FONT_LABEL, c.kib_label().as_str(), 0),
            None => TAG_RIGHT - TAG_ROOM,
        } - NAME_GAP
            - NAME_X;
        let shown = clip(&theme::FONT_VALUE, name.as_str(), room);
        draw::text(d, &theme::FONT_VALUE, shown, NAME_X, y, color);
    }
    if let Some(c) = cost {
        let label = c.kib_label();
        draw::text_right(d, &theme::FONT_LABEL, label.as_str(), COST_RIGHT, y, dim, 0);
    }
    if let Some(engine) = chain {
        tag(d, engine, y, dim);
    }
}
