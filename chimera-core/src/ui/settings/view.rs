//! SETTINGS' own look (spec § Screens), so it never reads as a Part page: a
//! breadcrumb for the header, a list with a bar, and a project footer in the
//! map's band. Direction A's tokens (ADR 0016).

use core::fmt::{self, Write};

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::FontRenderer;

use super::tree::{Kind, ROOT, Row, row_at};
use crate::project::{PartId, ProjectStatus};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

pub const LIST_TOP: i32 = 34;
pub const ROW_H: i32 = 28;
pub const VISIBLE_ROWS: usize = 8;
/// The footer takes the map's band.
pub const FOOTER_TOP: i32 = theme::MAP_TOP;

/// SETTINGS and one part per level of the tree.
const MAX_CRUMBS: usize = 5;
const DOTS: &str = "..";
/// Space either side of a breadcrumb's `›`.
const SEP_GAP: i32 = 4;
const BAR_X: i32 = 8;
const BAR_H: i32 = ROW_H - 4;
const BAR_R: u32 = 6;
const TICK_X: i32 = 4;
const ROW_BASELINE: i32 = 17;

/// One part of the breadcrumb: a row's crumb, or a run-time `PART n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crumb {
    Name(&'static str),
    Part(PartId),
}

impl fmt::Display for Crumb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Crumb::Name(s) => f.write_str(s),
            Crumb::Part(p) => write!(f, "PART {}", p.index() + 1),
        }
    }
}

/// `SETTINGS › AUDIO › OUTPUTS`, the last part bold. Too wide for its
/// width, it drops leading parts behind `..`; the last always shows.
#[derive(Clone, Copy, Debug)]
pub struct Crumbs {
    parts: [Crumb; MAX_CRUMBS],
    len: u8,
    width: i32,
}

impl Crumbs {
    /// The crumbs of the rows on `path`, from SETTINGS down.
    pub fn of(path: &[u8]) -> Self {
        let mut c = Crumbs {
            parts: [Crumb::Name(ROOT.crumb); MAX_CRUMBS],
            len: 1,
            width: theme::CRUMBS_W,
        };
        for d in 1..=path.len() {
            if let Some(r) = row_at(&path[..d]) {
                c.push(Crumb::Name(r.crumb));
            }
        }
        c
    }

    /// One more part; past five, ignored.
    pub fn push(&mut self, c: Crumb) {
        debug_assert!((self.len as usize) < MAX_CRUMBS);
        if let Some(p) = self.parts.get_mut(self.len as usize) {
            *p = c;
            self.len += 1;
        }
    }

    /// The same crumbs fitted to `width` px.
    pub fn within(self, width: i32) -> Self {
        Crumbs { width, ..self }
    }

    fn parts(&self) -> &[Crumb] {
        &self.parts[..self.len as usize]
    }

    /// Leading parts dropped behind `..` to fit.
    fn dropped(&self) -> usize {
        let n = self.parts().len();
        let sep = SEP_GAP * 2 + draw::text_width(&theme::FONT_LABEL, "›", 0);
        let mut widths = [0; MAX_CRUMBS];
        for (i, (w, &c)) in widths.iter_mut().zip(self.parts()).enumerate() {
            *w = with_text(c, |s| crumb_width(s, i + 1 == n));
        }
        let dots = draw::text_width(&theme::FONT_LABEL, DOTS, theme::LABEL_TRACKING) + sep;
        (0..n.saturating_sub(1))
            .find(|&k| {
                let shown: i32 = widths[k..n].iter().sum::<i32>() + sep * (n - k - 1) as i32;
                shown + if k > 0 { dots } else { 0 } <= self.width
            })
            .unwrap_or(n.saturating_sub(1))
    }
}

/// As drawn: `.. › AUDIO › OUTPUTS` once it drops parts.
impl fmt::Display for Crumbs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let k = self.dropped();
        if k > 0 {
            write!(f, "{DOTS} › ")?;
        }
        for (i, c) in self.parts()[k..].iter().enumerate() {
            if i > 0 {
                f.write_str(" › ")?;
            }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

fn with_text<R>(c: Crumb, f: impl FnOnce(&str) -> R) -> R {
    let mut buf = FmtBuf::new();
    let _ = write!(buf, "{c}");
    f(buf.as_str())
}

fn crumb_font(last: bool) -> &'static FontRenderer {
    if last {
        &theme::FONT_VALUE
    } else {
        &theme::FONT_LABEL
    }
}

fn crumb_width(s: &str, last: bool) -> i32 {
    draw::text_width(crumb_font(last), s, theme::LABEL_TRACKING)
}

pub fn draw_crumbs<D: DrawTarget<Color = Rgb565>>(d: &mut D, c: &Crumbs) {
    let y = theme::HEADER_BASELINE;
    let mut x = theme::MARGIN_X;
    let sep = |d: &mut D, x: &mut i32| {
        *x += SEP_GAP;
        *x += draw::text_tracked(d, &theme::FONT_LABEL, "›", *x, y, theme::BAR_REST, 0);
        *x += SEP_GAP;
    };
    let k = c.dropped();
    if k > 0 {
        x += draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            DOTS,
            x,
            y,
            theme::MID,
            theme::LABEL_TRACKING,
        );
        sep(d, &mut x);
    }
    let shown = &c.parts()[k..];
    for (i, &part) in shown.iter().enumerate() {
        if i > 0 {
            sep(d, &mut x);
        }
        let last = i + 1 == shown.len();
        let color = if last { theme::INK } else { theme::MID };
        x += with_text(part, |s| {
            draw::text_tracked(d, crumb_font(last), s, x, y, color, theme::LABEL_TRACKING)
        });
    }
}

/// How a row reads: `Dimmed` doesn't apply now, `Later` isn't built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowLook {
    Normal,
    Dimmed,
    Later,
}

impl RowLook {
    pub fn of(kind: Kind) -> Self {
        match kind {
            Kind::Later(_) => RowLook::Later,
            _ => RowLook::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ListRow<'a> {
    pub label: &'a str,
    /// EDIT opens more: a `›` when there's no note.
    pub opens: bool,
    /// A value or status on the right (`● LOADED`, `SLOT 03`).
    pub note: Option<&'a str>,
    pub look: RowLook,
}

impl ListRow<'static> {
    /// A tree row as its list shows it.
    pub fn of(r: &'static Row) -> Self {
        ListRow {
            label: r.label,
            opens: matches!(r.kind, Kind::List(_) | Kind::Leaf(_) | Kind::Screen(_)),
            note: None,
            look: RowLook::of(r.kind),
        }
    }
}

/// `rows` from `first`, the bar on `bar`; a scrollbar when they overflow.
pub fn draw_list<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    rows: &[ListRow<'_>],
    bar: usize,
    first: usize,
) {
    for (i, r) in rows.iter().enumerate().skip(first).take(VISIBLE_ROWS) {
        let y = LIST_TOP + (i - first) as i32 * ROW_H;
        let on = i == bar;
        if on {
            draw::round_fill(
                d,
                BAR_X,
                y,
                theme::LIST_RIGHT + 10 - BAR_X,
                BAR_H,
                BAR_R,
                theme::ACCENT_SOFT,
            );
            draw::fill_rect(d, TICK_X, y + 4, 2, BAR_H - 8, theme::ACCENT);
        }
        let live = r.look == RowLook::Normal;
        let (label, side) = match (live, on) {
            (true, true) => (theme::ACCENT, theme::ACCENT),
            (true, false) => (theme::INK, theme::MID),
            (false, true) => (theme::MID, theme::MID),
            (false, false) => (theme::BAR_REST, theme::BAR_REST),
        };
        let base = y + ROW_BASELINE;
        draw::text(
            d,
            &theme::FONT_VALUE,
            r.label,
            theme::LIST_TEXT_X,
            base,
            label,
        );
        let note = match r.look {
            RowLook::Later => Some("LATER"),
            _ => r.note,
        };
        let right = |d: &mut D, s: &str, c| {
            draw::text_right(
                d,
                &theme::FONT_LABEL,
                s,
                theme::LIST_RIGHT,
                base - 1,
                c,
                theme::LABEL_TRACKING,
            )
        };
        match note {
            Some(n) => right(d, n, side),
            None if r.opens => right(
                d,
                "›",
                if on && live {
                    theme::ACCENT
                } else {
                    theme::BAR_REST
                },
            ),
            None => {}
        }
    }
    if rows.len() > VISIBLE_ROWS {
        let h = VISIBLE_ROWS as i32 * ROW_H - 4;
        draw::fill_rect(d, theme::SCROLL_X, LIST_TOP, 2, h, theme::FAINT);
        let thumb = (h * VISIBLE_ROWS as i32 / rows.len() as i32).max(8);
        let max_first = rows.len() - VISIBLE_ROWS;
        let y = LIST_TOP + (h - thumb) * first.min(max_first) as i32 / max_first as i32;
        draw::fill_rect(d, theme::SCROLL_X, y, 2, thumb, theme::MID);
    }
}

/// The first row shown: `prev_first` while the bar stays on screen, else
/// just enough scroll to show it.
pub fn first_visible(bar: usize, len: usize, prev_first: usize) -> usize {
    prev_first
        .min(len.saturating_sub(VISIBLE_ROWS))
        .min(bar)
        .max((bar + 1).saturating_sub(VISIBLE_ROWS))
}

pub struct Footer<'a> {
    pub name: &'a str,
    pub status: ProjectStatus,
    pub legend: &'static str,
}

/// `NEW`, `SAVED`, or `* MODIFIED` in the warning colour.
pub fn status_text(s: ProjectStatus) -> (&'static str, Rgb565) {
    match s {
        ProjectStatus::Pristine => ("NEW", theme::MID),
        ProjectStatus::Saved => ("SAVED", theme::MID),
        ProjectStatus::Modified => ("* MODIFIED", theme::WARN),
    }
}

pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &Footer<'_>) {
    draw::fill_rect(
        d,
        theme::MARGIN_X,
        theme::FOOTER_RULE_Y,
        theme::CRUMBS_W,
        1,
        theme::FAINT,
    );
    let w = draw::text_tracked(
        d,
        &theme::FONT_LABEL_BOLD,
        f.name,
        theme::MARGIN_X,
        theme::FOOTER_NAME_Y,
        theme::INK,
        theme::LABEL_TRACKING,
    );
    let (s, c) = status_text(f.status);
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        s,
        theme::MARGIN_X + w + 6,
        theme::FOOTER_NAME_Y,
        c,
        theme::LABEL_TRACKING,
    );
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        f.legend,
        theme::MARGIN_X,
        theme::FOOTER_LEGEND_Y,
        theme::BAR_REST,
        0,
    );
}

/// Which keys the footer's legend names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendFor {
    Opens,
    Action,
    Later,
    Leaf,
    Prompt,
    Naming,
    ManageList,
    ManageCommands,
}

impl LegendFor {
    /// The legend for a list's bar on a row of `kind`.
    pub fn of(kind: Kind) -> Self {
        match kind {
            Kind::List(_) | Kind::Leaf(_) | Kind::Screen(_) => LegendFor::Opens,
            Kind::Act(_) => LegendFor::Action,
            Kind::Later(_) => LegendFor::Later,
        }
    }
}

/// The footer's key legend; `at_top`: MENU closes SETTINGS.
pub fn legend(on: LegendFor, at_top: bool) -> &'static str {
    use LegendFor as L;
    match (on, at_top) {
        (L::Opens, false) => "EDIT OPEN · MENU BACK",
        (L::Opens, true) => "EDIT OPEN · MENU CLOSE",
        (L::Action, false) => "SEQ RUN · MENU BACK",
        (L::Action, true) => "SEQ RUN · MENU CLOSE",
        (L::Later, false) => "LATER · MENU BACK",
        (L::Later, true) => "LATER · MENU CLOSE",
        (L::Leaf, _) => "A-F EDIT · MENU BACK",
        (L::Prompt, _) => "A PICK · SEQ OK · MENU CANCEL",
        (L::Naming, _) => "SEQ SAVE · MENU CANCEL",
        (L::ManageList, _) => "EDIT COMMANDS · MENU BACK",
        (L::ManageCommands, _) => "SEQ RUN · MENU LIST",
    }
}
