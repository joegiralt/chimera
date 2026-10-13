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
use crate::ui::library::{BrowserTab, LibRow, LibState, row as lib_row_at, rows as lib_rows};
use crate::ui::meter::{self, MeterView};
use crate::ui::settings::part::Offer;
use crate::ui::settings::prompt::POOL_FULL;
use crate::ui::theme;

/// Rows on screen.
pub const VISIBLE_ROWS: usize = 8;
/// One init Sound per engine a player picks, then the engines coming
/// soon, then the pool's slots.
pub const INIT_TYPES: [EngineType; EngineType::PLAYABLE.len()] = EngineType::PLAYABLE;
pub const INITS: usize = INIT_TYPES.len();
/// The rows above slot 1.
pub const HEAD: usize = INITS + Soon::ALL.len();
pub const TOTAL_ENTRIES: usize = HEAD + MAX_SOUNDS;
/// What a coming-soon row says in place of an engine tag.
pub const SOON_TAG: &str = "COMING SOON";

/// An engine listed but not playable yet: its row, and a pool Sound of
/// it, read COMING SOON, and EDIT only toasts (ADR 0132).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Soon {
    /// Until its rework (#476).
    Tape,
    /// Until the DRUM engine exists.
    Drum,
}

impl Soon {
    pub const ALL: [Soon; 2] = [Soon::Tape, Soon::Drum];

    /// Its row's entry, under the INIT rows.
    pub const fn entry(self) -> usize {
        INITS + self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Soon::Tape => "TAPE",
            Soon::Drum => "DRUM",
        }
    }

    /// Its toast on a load.
    pub const fn toast(self) -> &'static str {
        match self {
            Soon::Tape => "TAPE: COMING SOON",
            Soon::Drum => "DRUM: COMING SOON",
        }
    }

    /// The engine a pool Sound can't load on yet: TAPE's.
    pub fn of(engine: EngineType) -> Option<Soon> {
        (engine == EngineType::Tape).then_some(Soon::Tape)
    }
}

/// What a list entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Init(EngineType),
    Soon(Soon),
    Slot(SlotId),
}

/// The Sound rung's browser: each tab's cursor and first row shown. Only
/// `input`, `input_library` and a listing's landing move them; PROJECT
/// opens on slot 1, the INIT and coming-soon rows just above, LIBRARY at its top.
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
            cursor: HEAD as u8,
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

/// The pool slot at list entry `entry`; `None` for the rows above them.
pub fn slot_at(entry: usize) -> Option<SlotId> {
    SlotId::new(u8::try_from(entry.checked_sub(HEAD)?).ok()?)
}

/// The entry at `entry`; `None` past the list.
pub fn entry_at(entry: usize) -> Option<Entry> {
    if let Some(&s) = entry.checked_sub(INITS).and_then(|i| Soon::ALL.get(i)) {
        return Some(Entry::Soon(s));
    }
    init_at(entry)
        .map(Entry::Init)
        .or_else(|| slot_at(entry).map(Entry::Slot))
}

/// The engine of the INIT row at `entry`; `None` for the rest.
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
/// FACTORY's (showcase spec § 2).
pub const EMPTY_FACTORY: &str = "NO FACTORY SOUNDS";
pub const PREVIOUS: &str = "‹ PREVIOUS";
pub const NEXT: &str = "NEXT ›";
/// EDIT's words on a row of an engine coming soon.
pub const SOON_LOAD: Gesture<'static> = Gesture("EDIT", SOON_TAG);
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

/// FACTORY's line 2, the description's room: it draws clipped to this.
pub const NOTE_W: i32 = theme::CONTENT_W;

/// A description's width as FACTORY draws it; the factory-sounds tool
/// refuses one past `NOTE_W`.
pub fn note_width(n: &str) -> i32 {
    draw::text_width(&theme::FONT_LABEL, n, theme::LABEL_TRACKING)
}

/// FACTORY's line 3, the page's set stamp: `FACTORY · 0.1.0 1A2B3C4D`.
pub fn factory_words(stamp: Option<BuildStamp>) -> FmtBuf {
    let mut f = FmtBuf::new();
    let _ = match stamp {
        Some(s) => {
            let mut b = FmtBuf::new();
            let _ = write!(b, "{s}");
            write!(f, "FACTORY · {}", components::upper(b.as_str()).as_str())
        }
        None => f.write_str("FACTORY · NO STAMP"),
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

/// What the browser lists: PROJECT's pool slots, or LIBRARY's or
/// FACTORY's page (the state's tab) with this build's stamp.
#[derive(Clone, Copy)]
pub enum BrowserView<'a> {
    Project,
    Library(&'a LibState, &'a BuildStamp),
}

/// The space between the title's tabs, and before `· P1`.
pub const TITLE_GAP: i32 = 8;
/// B's mark: right on hint line 1, as the title's three tabs in full and
/// it run past `CONTENT_W` (showcase spec § 2).
pub const TURN: &str = "B ◂▸";

/// Where the title's last tab ends: every tab in full.
pub fn title_tabs_end() -> i32 {
    let tabs: i32 = BrowserTab::ALL
        .iter()
        .map(|t| draw::text_width(&theme::FONT_LABEL_BOLD, t.label(), theme::LABEL_TRACKING))
        .sum();
    theme::MARGIN_X + tabs + TITLE_GAP * (BrowserTab::ALL.len() as i32 - 1)
}

/// The title row: `PROJECT  LIBRARY  FACTORY`, the current tab in `INK`,
/// then `· P1` right-aligned (ADR 0086's marks).
fn title<D>(d: &mut D, tab: BrowserTab, part: PartId)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let mut x = theme::MARGIN_X;
    for t in BrowserTab::ALL {
        let ink = if t == tab { theme::INK } else { theme::FAINT };
        let font = &theme::FONT_LABEL_BOLD;
        x += draw::text_tracked(d, font, t.label(), x, y, ink, theme::LABEL_TRACKING) + TITLE_GAP;
    }
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
            title(d, BrowserTab::Project, part);
            project_body(d, project, part, cursor, scroll, hints, refusal);
        }
        BrowserView::Library(lib, this) => {
            title(d, lib.tab(), part);
            library_body(d, project.pool(), lib, this, cursor, scroll, hints, refusal);
        }
    }
    // B's turn between the tabs: a hint, right on line 1.
    if hints && refusal.is_none() {
        let (font, t) = (&theme::FONT_LABEL, theme::LABEL_TRACKING);
        draw::text_right(d, font, TURN, theme::VIZ_RIGHT, HINT_Y, theme::MID, t);
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

    let factory = lib.tab() == BrowserTab::Factory;
    let on = match lib_row_at(page, cursor) {
        LibRow::Sound(e) => Some(e),
        _ => None,
    };
    let soon = on.and_then(|e| lib.soon(e));
    // A listing that failed partway: its words under the rows it left.
    if let (Some(err), Some(p)) = (lib.err(), page)
        && !p.entries().is_empty()
    {
        let msg = clip(&theme::FONT_LABEL, err.message(), ORPHAN_W, 0);
        let (x, y, c) = (theme::MARGIN_X, INFO_Y, theme::WARN);
        draw::text_tracked(d, &theme::FONT_LABEL, msg, x, y, c, theme::LABEL_TRACKING);
    }
    let loadable = on.filter(|e| e.err.is_none() && soon.is_none());
    let fit = loadable.map(|e| fitting(pool, e));
    if let Some(r) = refusal {
        refusal_lines(d, r);
    } else if hints {
        if soon.is_some() {
            hint::draw(d, &[SOON_LOAD], theme::MARGIN_X, HINT_Y);
        }
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
        // FACTORY is read-only: nothing saves, so line 2 is free.
        if !factory {
            hint::draw(d, &[SAVE_TO_LIBRARY], theme::MARGIN_X, SAVE_Y);
        }
    }
    // The description is information: with HINTS off too.
    if let Some(n) = on
        .and_then(|e| e.note())
        .filter(|_| factory && refusal.is_none())
    {
        // `clip` measures as `note_width` does.
        let shown = clip(
            &theme::FONT_LABEL,
            n.as_str(),
            NOTE_W,
            theme::LABEL_TRACKING,
        );
        let (x, y, c) = (theme::MARGIN_X, SAVE_Y, theme::INK);
        draw::text_tracked(d, &theme::FONT_LABEL, shown, x, y, c, theme::LABEL_TRACKING);
    }
    if let Some(e) = on {
        let words = match page.filter(|_| factory) {
            Some(p) => factory_words(p.set_stamp),
            None => stamp_words(e.stamp(), this),
        };
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
    let preview = loadable.map(|e| entry_cost(e.len));
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
    let factory = lib.tab() == BrowserTab::Factory;
    let (prev, next, empty) = if factory {
        (PREVIOUS, NEXT, EMPTY_FACTORY)
    } else {
        (NEWER, OLDER, EMPTY_LIBRARY)
    };
    let e = match row {
        LibRow::Prev => return value(d, prev, dim),
        LibRow::Next => return value(d, next, dim),
        LibRow::Empty => {
            let msg = lib.err().map_or(empty, |n| n.message());
            return value(d, msg, dim);
        }
        LibRow::Sound(e) => e,
    };
    // Greyed: the name dimmed, the `FileError` (or COMING SOON) in place
    // of cost and tag.
    let greyed = e
        .err
        .map(|err| err.message())
        .or(lib.soon(e).map(|_| SOON_TAG));
    let ink = match (selected, greyed) {
        (true, _) => theme::BG,
        (false, Some(_)) => theme::MID,
        (false, None) => theme::INK,
    };
    let right = match greyed {
        Some(m) => {
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
                0,
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

/// The longest start of `s` that fits `w` px in `font` at `tracking`.
fn clip<'s>(font: &FontRenderer, s: &'s str, w: i32, tracking: i32) -> &'s str {
    let mut end = s.len();
    while end > 0 && draw::text_width(font, &s[..end], tracking) > w {
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
    let (name, chain, cost) = match entry_at(entry) {
        Some(Entry::Slot(s)) => {
            let _ = write!(slot, "{:02}", s.index() + 1);
            match pool.get(s) {
                Some(s) => (
                    components::upper(s.name().as_str()),
                    Some(s.engine()),
                    Some(s.cost()),
                ),
                None => (FmtBuf::new(), None, None),
            }
        }
        Some(Entry::Init(engine)) => {
            let _ = slot.write_str("INIT");
            (components::upper(engine.label()), Some(engine), None)
        }
        Some(Entry::Soon(s)) => {
            let _ = slot.write_str("INIT");
            let mut name = FmtBuf::new();
            let _ = name.write_str(s.label());
            (name, None, None)
        }
        None => return,
    };
    let soon = match entry_at(entry) {
        Some(Entry::Soon(s)) => Some(s),
        _ => chain.and_then(Soon::of),
    };
    // A pool Sound of a coming-soon engine reads like its INIT row: its
    // cost would sit under the wider tag.
    let cost = cost.filter(|_| soon.is_none());
    let saved = cost.is_some();
    let empty = chain.is_none() && soon.is_none();
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
        let room = match (cost, soon) {
            (Some(c), _) => {
                COST_RIGHT - draw::text_width(&theme::FONT_LABEL, c.kib_label().as_str(), 0)
            }
            (None, Some(_)) => TAG_RIGHT - draw::text_width(&theme::FONT_LABEL, SOON_TAG, 0),
            (None, None) => TAG_RIGHT - TAG_ROOM,
        } - NAME_GAP
            - NAME_X;
        let shown = clip(&theme::FONT_VALUE, name.as_str(), room, 0);
        draw::text(d, &theme::FONT_VALUE, shown, NAME_X, y, color);
    }
    if let Some(c) = cost {
        let label = c.kib_label();
        draw::text_right(d, &theme::FONT_LABEL, label.as_str(), COST_RIGHT, y, dim, 0);
    }
    match (soon, chain) {
        (Some(_), _) => draw::text_right(d, &theme::FONT_LABEL, SOON_TAG, TAG_RIGHT, y, dim, 0),
        (None, Some(engine)) => tag(d, engine, y, dim),
        (None, None) => {}
    }
}
