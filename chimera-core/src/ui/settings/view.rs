// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SETTINGS' own look (spec § Screens), so it never reads as a Part page: a
//! breadcrumb for the header, a list with a bar, and a project footer in the
//! map's band. Direction A's tokens (ADR 0016).

use core::fmt::{self, Write};

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::FontRenderer;

use super::NamingFor;
use super::listing::{LOADED, Listing};
use super::manage::{Command, Note, Off, Whose, command_rows};
use super::naming::{Naming, draw_naming};
use super::part::{Offer, SAVE_ROWS};
use super::prompt::Slot;
use super::tree::{Act, Kind, PART, ROOT, Row, Screen, row_at, rows};
use crate::name::{ProjectName, SoundName};
use crate::orbit::scene::{OrbitSong, SceneIdx, SceneSet};
use crate::project::{
    Line, Origin, PartId, Project, ProjectFile, ProjectStatus, SlotId, SoundStatus, sound_status,
};
use crate::ui::components::HeaderStatus;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
#[cfg(feature = "midi-map")]
use crate::ui::hint;
use crate::ui::nav::{Column, SettingsAt};
use crate::ui::region::settings_key;
use crate::ui::theme;
use crate::ui::theme::{Face, TextSize};

pub const LIST_TOP: i32 = 34;
pub const ROW_H: i32 = 28;
pub const VISIBLE_ROWS: usize = 8;
/// MIDI MAP's rows: the bar's binding is spelled out beneath them.
#[cfg(feature = "midi-map")]
pub const MAP_ROWS: usize = 6;
/// The footer takes the map's band.
pub const FOOTER_TOP: i32 = theme::MAP_TOP;
/// The PART strip, under the breadcrumb; PART's lists start below it.
pub const STRIP_TOP: i32 = theme::HEADER_BOTTOM;
pub const STRIP_H: i32 = 20;
const STRIP_BASELINE: i32 = STRIP_TOP + 14;
const PART_LIST_TOP: i32 = LIST_TOP + STRIP_H;
/// PART's rows beneath the strip; a longer list scrolls (ADR 0232).
pub const PART_ROWS: usize = ((FOOTER_TOP - PART_LIST_TOP) / ROW_H) as usize;
const _: () = assert!(SAVE_ROWS.len() <= PART_ROWS);

/// SETTINGS, one part per level of the tree, and a leaf's page or NAMING's.
const MAX_CRUMBS: usize = 6;
const DOTS: &str = "..";
/// Space either side of a breadcrumb's `›`.
const SEP_GAP: i32 = 3;
const BAR_H: i32 = ROW_H - 4;
const BAR_R: u32 = 6;
const ROW_BASELINE: i32 = 17;
/// A two-line row: the label, then its note beneath.
const UPPER_BASELINE: i32 = 11;
const NOTE_BASELINE: i32 = 21;

/// Where a column of rows draws.
#[derive(Clone, Copy)]
struct Col {
    tick: i32,
    bar: (i32, i32),
    text: i32,
    right: i32,
    scroll: i32,
    font: &'static FontRenderer,
    /// No glyph is wider, tracking included.
    max_glyph: i32,
    /// A note goes beneath the label, not beside it.
    note_below: bool,
    /// TEXT SIZE: LARGE scales a whole-width row where it fits (ADR 0154).
    size: TextSize,
}

/// A whole-width list.
const FULL: Col = Col {
    tick: 4,
    bar: (8, theme::SCROLL_X - 4),
    text: theme::LIST_TEXT_X,
    right: theme::LIST_RIGHT,
    scroll: theme::SCROLL_X,
    font: &theme::FONT_VALUE,
    max_glyph: 15,
    note_below: false,
    size: TextSize::Normal,
};
/// MANAGE's projects, x 0–140.
const MANAGE_LIST: Col = Col {
    bar: (8, 134),
    text: 14,
    right: 130,
    scroll: 137,
    font: &theme::FONT_LABEL_BOLD,
    max_glyph: 12,
    ..FULL
};
/// MANAGE's commands, x 144–240.
const MANAGE_CMDS: Col = Col {
    tick: 144,
    bar: (147, theme::SCROLL_X - 2),
    text: 152,
    right: theme::SCROLL_X - 2,
    font: &theme::FONT_LABEL_BOLD,
    max_glyph: 12,
    note_below: true,
    ..FULL
};
/// What a MANAGE command's label and note have, in px.
pub const COMMAND_W: i32 = MANAGE_CMDS.right - MANAGE_CMDS.text;
const MANAGE_RULE_X: i32 = 141;

/// A row's bar: none, held while the other column has the keys, or keyed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bar {
    Off,
    Held,
    Keyed,
}

/// One part of the breadcrumb: a row's crumb, or a run-time `PART n` or
/// `SCENE n` (ORBIT's list, as `PART n` is PART's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crumb {
    Name(&'static str),
    Part(PartId),
    Orbit(SceneIdx),
}

impl fmt::Display for Crumb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Crumb::Name(s) => f.write_str(s),
            Crumb::Part(p) => write!(f, "PART {}", p.index() + 1),
            Crumb::Orbit(s) => write!(f, "SCENE {}", s.get() + 1),
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
    /// The crumbs of the rows on `path`, from SETTINGS down; the PART list
    /// is `PART n` for the `active` Part, the ORBIT list `SCENE n` for the
    /// playing `scene`.
    // Without ORBIT no list names a scene.
    #[cfg_attr(not(feature = "orbit"), allow(unused_variables))]
    pub fn of(path: &[u8], active: PartId, scene: SceneIdx) -> Self {
        let mut c = Crumbs {
            parts: [Crumb::Name(ROOT.crumb); MAX_CRUMBS],
            len: 1,
            width: theme::CRUMBS_W,
        };
        for d in 1..=path.len() {
            match row_at(&path[..d]) {
                Some(r) if is_list(r, &PART) => c.push(Crumb::Part(active)),
                #[cfg(feature = "orbit")]
                Some(r) if is_list(r, &super::tree::ORBIT) => c.push(Crumb::Orbit(scene)),
                Some(r) => c.push(Crumb::Name(r.crumb)),
                None => {}
            }
        }
        c
    }

    /// One more part; past `MAX_CRUMBS`, ignored.
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

    pub fn parts(&self) -> &[Crumb] {
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

/// Row `r` is the list `rows`.
fn is_list(r: &Row, rows: &'static [Row]) -> bool {
    matches!(r.kind, Kind::List(rs) if core::ptr::eq(rs, rows))
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
            Kind::List(_) | Kind::Leaf(_) | Kind::Screen(_) | Kind::Act(_) => RowLook::Normal,
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
    size: TextSize,
) {
    draw_rows(d, rows.len(), bar, first, size, |i, f| f(rows[i]));
}

/// `draw_list` under the PART strip.
fn draw_part_list<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    len: usize,
    bar: usize,
    first: usize,
    size: TextSize,
    row: impl FnMut(usize, &mut dyn FnMut(ListRow<'_>)),
) {
    let c = Col { size, ..FULL };
    draw_col::<PART_ROWS, _>(d, &c, PART_LIST_TOP, len, bar, first, Bar::Keyed, row);
}

/// MANAGE PROJECTS: `rows` on the left from `first`, the bar on `bar`;
/// the bar's project's commands on the right, `cmd` keyed when they have
/// the keys.
pub fn draw_manage<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    rows: &[ListRow<'_>],
    bar: usize,
    first: usize,
    cmds: &[ListRow<'_>],
    cmd: Option<u8>,
) {
    draw_manage_rows(d, rows.len(), bar, first, |i, f| f(rows[i]), cmds, cmd);
}

fn draw_manage_rows<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    len: usize,
    bar: usize,
    first: usize,
    row: impl FnMut(usize, &mut dyn FnMut(ListRow<'_>)),
    cmds: &[ListRow<'_>],
    cmd: Option<u8>,
) {
    let held = if cmd.is_some() { Bar::Held } else { Bar::Keyed };
    draw_col::<VISIBLE_ROWS, _>(d, &MANAGE_LIST, LIST_TOP, len, bar, first, held, row);
    let h = VISIBLE_ROWS as i32 * ROW_H - 4;
    draw::fill_rect(d, MANAGE_RULE_X, LIST_TOP, 1, h, theme::FAINT);
    let on = cmd.map_or(usize::MAX, usize::from);
    draw_col::<VISIBLE_ROWS, _>(
        d,
        &MANAGE_CMDS,
        LIST_TOP,
        cmds.len(),
        on,
        0,
        Bar::Keyed,
        |i, f| f(cmds[i]),
    );
}

/// `len` rows, each lent by `row` to the draw, from `first`.
pub fn draw_rows<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    len: usize,
    bar: usize,
    first: usize,
    size: TextSize,
    row: impl FnMut(usize, &mut dyn FnMut(ListRow<'_>)),
) {
    let c = Col { size, ..FULL };
    draw_col::<VISIBLE_ROWS, _>(d, &c, LIST_TOP, len, bar, first, Bar::Keyed, row);
}

#[allow(clippy::too_many_arguments)]
fn draw_col<const N: usize, D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    c: &Col,
    top: i32,
    len: usize,
    bar: usize,
    first: usize,
    on: Bar,
    mut row: impl FnMut(usize, &mut dyn FnMut(ListRow<'_>)),
) {
    let n = N;
    for i in (first..len).take(n) {
        let y = top + (i - first) as i32 * ROW_H;
        let b = if i == bar { on } else { Bar::Off };
        row(i, &mut |r| draw_row(d, c, &r, y, b));
    }
    if len > n {
        let h = n as i32 * ROW_H - 4;
        draw::fill_rect(d, c.scroll, top, 2, h, theme::FAINT);
        let thumb = (h * n as i32 / len as i32).max(8);
        let max_first = len - n;
        let y = top + (h - thumb) * first.min(max_first) as i32 / max_first as i32;
        draw::fill_rect(d, c.scroll, y, 2, thumb, theme::MID);
    }
}

fn draw_row<D: DrawTarget<Color = Rgb565>>(d: &mut D, c: &Col, r: &ListRow<'_>, y: i32, b: Bar) {
    let on = b != Bar::Off;
    if on {
        let (l, rt) = c.bar;
        draw::round_rect(d, l, y, rt - l, BAR_H, BAR_R, theme::ACCENT_SOFT);
    }
    if b == Bar::Keyed {
        draw::fill_rect(d, c.tick, y + 4, 2, BAR_H - 8, theme::ACCENT);
    }
    let live = r.look == RowLook::Normal;
    let (label, side) = match (live, on) {
        (true, true) => (theme::ACCENT, theme::ACCENT),
        (true, false) => (theme::INK, theme::MID),
        (false, true) => (theme::MID, theme::MID),
        (false, false) => (theme::BAR_REST, theme::BAR_REST),
    };
    let note = match r.look {
        RowLook::Later => Some("LATER"),
        _ => r.note,
    };
    let below = c.note_below && note.is_some();
    // LARGE (ADR 0154): the whole column in the large faces, a pixel lower;
    // a label too long is cut with an ellipsis, never set smaller.
    let size = if below { TextSize::Normal } else { c.size };
    let large = size == TextSize::Large;
    let base = y + if below { UPPER_BASELINE } else { ROW_BASELINE } + i32::from(large);
    let t = theme::LABEL_TRACKING;
    let note_font = theme::font(Face::Label, size);
    let beside = match note {
        Some(n) if !below => draw::text_width(note_font, n, t) + 4,
        _ => 0,
    };
    if large {
        let slot = draw::Slot {
            left: c.text,
            top: y,
            right: c.right - beside,
            bottom: y + BAR_H,
        };
        let font = theme::font(Face::Value, size);
        draw::text_cut(d, font, r.label, c.text, base, label, t, slot);
    } else {
        let shown = fit(c, r.label, c.right - c.text - beside);
        draw::text_tracked(d, c.font, shown, c.text, base, label, t);
    }
    let small = |d: &mut D, font, s: &str, x, y, color| {
        draw::text_right(d, font, s, x, y, color, t);
    };
    match note {
        Some(n) if below => {
            draw::text(d, &theme::FONT_LABEL, n, c.text, y + NOTE_BASELINE, side);
        }
        Some(n) => small(d, note_font, n, c.right, base - 1, side),
        None if r.opens => {
            let color = if on && live {
                theme::ACCENT
            } else {
                theme::BAR_REST
            };
            small(d, &theme::FONT_LABEL, "›", c.right, base - 1, color)
        }
        None => {}
    }
}

/// `s` cut to `w` px at a character; short enough, it isn't measured.
fn fit<'s>(c: &Col, s: &'s str, w: i32) -> &'s str {
    if s.len() as i32 * c.max_glyph <= w {
        return s;
    }
    let mut end = s.len();
    while end > 0 && draw::text_width(c.font, &s[..end], theme::LABEL_TRACKING) > w {
        end -= 1;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
    }
    &s[..end]
}

/// MIDI MAP's bar spelled out under its rows (spec § 8.2): the input in
/// full words over two lines, then MIN, MAX and ECHO beside their keys,
/// then B's and E's hint.
#[cfg(feature = "midi-map")]
fn draw_map_focus<D: DrawTarget<Color = Rgb565>>(d: &mut D, t: &super::midi_map::MapText) {
    let (x, tr) = (theme::MARGIN_X, theme::LABEL_TRACKING);
    let y = |line: i32| LIST_TOP + MAP_ROWS as i32 * ROW_H + 12 + line * 14;
    draw::text_tracked(
        d,
        &theme::FONT_LABEL_BOLD,
        t.head.as_str(),
        x,
        y(0),
        theme::INK,
        tr,
    );
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        t.tail.as_str(),
        x,
        y(1),
        theme::MID,
        tr,
    );
    hint::draw(d, &t.values(), x, y(2));
    hint::draw(d, t.hint(), x, y(3));
}

/// The first row shown: `prev_first` while the bar stays on screen, else
/// just enough scroll to show it.
pub fn first_visible(bar: usize, len: usize, prev_first: usize) -> usize {
    first_visible_of(bar, len, prev_first, VISIBLE_ROWS)
}

/// `first_visible` for a list showing `rows` at once.
pub fn first_visible_of(bar: usize, len: usize, prev_first: usize, rows: usize) -> usize {
    prev_first
        .min(len.saturating_sub(rows))
        .min(bar)
        .max((bar + 1).saturating_sub(rows))
}

/// A Part against its slot, as the PART strip says it (Pre-flight 19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartMark {
    Clean,
    /// From its slot, or INIT (`None`).
    Edited(Option<SlotId>),
    /// As loaded from this slot, which moved on.
    SlotMoved(SlotId),
}

impl PartMark {
    pub fn of(p: &Project, part: PartId) -> Self {
        let x = p.part(part);
        match sound_status(x, p.pool()) {
            SoundStatus::Clean => PartMark::Clean,
            SoundStatus::Stale(s) => PartMark::SlotMoved(s),
            SoundStatus::Edited => PartMark::Edited(match x.origin() {
                Origin::Slot { slot, .. } => Some(slot),
                Origin::Init(_) => None,
            }),
        }
    }

    fn color(self) -> Rgb565 {
        match self {
            PartMark::Clean => theme::MID,
            PartMark::Edited(_) | PartMark::SlotMoved(_) => theme::WARN,
        }
    }

    fn key(self) -> [u8; 2] {
        match self {
            PartMark::Clean => [0, 0],
            PartMark::Edited(s) => [1, s.map_or(0, |s| s.index() as u8 + 1)],
            PartMark::SlotMoved(s) => [2, s.index() as u8 + 1],
        }
    }
}

impl fmt::Display for PartMark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PartMark::Clean => f.write_str("CLEAN"),
            PartMark::Edited(Some(s)) => write!(f, "* EDITED · FROM {}", Slot(*s)),
            PartMark::Edited(None) => f.write_str("* EDITED · FROM INIT"),
            PartMark::SlotMoved(s) => write!(f, "◦ {} MOVED", Slot(*s)),
        }
    }
}

/// `P2 · NAME` on the left, the mark on the right; the name gives way.
pub fn draw_part_strip<D: DrawTarget<Color = Rgb565>>(
    d: &mut D,
    part: PartId,
    name: &str,
    mark: PartMark,
) {
    // Untracked: the longest mark and a name share 216 px.
    let (mf, nf) = (&theme::FONT_LABEL, &theme::FONT_LABEL_BOLD);
    let mut m = Line::new("");
    let _ = write!(m, "{mark}");
    let right = theme::VIZ_RIGHT;
    let mw = draw::text_width(mf, m.as_str(), 0);
    draw::text_right(d, mf, m.as_str(), right, STRIP_BASELINE, mark.color(), 0);
    let mut l = Line::new("");
    let _ = write!(
        l,
        "P{} · {}",
        part.index() + 1,
        crate::ui::components::upper(name).as_str()
    );
    let room = right - mw - 8 - theme::MARGIN_X;
    let x = theme::MARGIN_X;
    let full = l.as_str();
    if draw::text_width(nf, full, 0) <= room {
        draw::text_tracked(d, nf, full, x, STRIP_BASELINE, theme::INK, 0);
    } else {
        let room = room - draw::text_width(nf, DOTS, 0);
        let cut = full
            .char_indices()
            .map(|(i, _)| &full[..i])
            .take_while(|c| draw::text_width(nf, c, 0) <= room)
            .last()
            .unwrap_or("");
        let w = draw::text_tracked(d, nf, cut, x, STRIP_BASELINE, theme::INK, 0);
        draw::text_tracked(d, nf, DOTS, x + w, STRIP_BASELINE, theme::MID, 0);
    }
    draw::fill_rect(
        d,
        theme::MARGIN_X,
        STRIP_TOP + STRIP_H - 1,
        theme::FOOTER_RULE_W,
        1,
        theme::FAINT,
    );
}

/// What the PART branch's bands show of the active Part.
#[derive(Clone, Copy, Debug)]
pub struct PartBand {
    pub name: SoundName,
    pub mark: PartMark,
    pub offer: Offer,
}

impl PartBand {
    pub fn of(p: &Project, part: PartId) -> Self {
        PartBand {
            name: p.part(part).sound.name,
            mark: PartMark::of(p, part),
            offer: Offer::of(p, part),
        }
    }

    /// A PART row's look: RELOAD from the offer, the rest from the tree.
    fn look(&self, kind: Kind) -> RowLook {
        match kind {
            Kind::Act(a) if let Some(c) = a.part_cmd() => self.offer.look(c),
            k => RowLook::of(k),
        }
    }
}

/// What SETTINGS › ORBIT's bands show of the playing scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitBand {
    pub scene: SceneIdx,
    /// The saved slots (`OrbitSong::saved`).
    pub saved: SceneSet,
    /// `live` differs from its slot: RELOAD has drift to restart.
    pub edited: bool,
}

impl OrbitBand {
    pub fn of(song: &OrbitSong) -> Self {
        OrbitBand {
            scene: song.current(),
            saved: song.saved(),
            edited: song.edited(),
        }
    }

    /// An ORBIT row's look: RELOAD dimmed with no drift, the rest
    /// from the tree.
    pub fn look(&self, kind: Kind) -> RowLook {
        match kind {
            Kind::Act(Act::SceneReload) if !self.edited => RowLook::Dimmed,
            k => RowLook::of(k),
        }
    }

    /// SAVE TO PROJ's row for slot `n`: `OVER SCENE 3`, or `SCENE 3` when
    /// neutral.
    pub fn save_label(&self, n: SceneIdx) -> Line {
        let over = if self.saved.contains(n) { "OVER " } else { "" };
        let mut l = Line::new(over);
        let _ = write!(l, "SCENE {}", n.get() + 1);
        l
    }

    /// Bits for a region key.
    fn key(&self) -> [u8; 3] {
        [self.scene.get(), self.saved.bits(), self.edited as u8]
    }
}

pub struct Footer<'a> {
    pub name: &'a str,
    pub status: ProjectStatus,
    pub legend: &'static str,
}

/// `SAVED`, or `* MODIFIED` in the warning colour. NEW has none: its
/// CRC holds the name, so the name already reads NEW PROJECT (#286).
pub fn status_text(s: ProjectStatus) -> Option<(&'static str, Rgb565)> {
    match s {
        ProjectStatus::Pristine => None,
        ProjectStatus::Saved => Some(("SAVED", theme::MID)),
        ProjectStatus::Modified => Some(("* MODIFIED", theme::WARN)),
    }
}

pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &Footer<'_>) {
    draw::fill_rect(
        d,
        theme::MARGIN_X,
        theme::FOOTER_RULE_Y,
        theme::FOOTER_RULE_W,
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
    if let Some((s, c)) = status_text(f.status) {
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            s,
            theme::MARGIN_X + w + 6,
            theme::FOOTER_NAME_Y,
            c,
            theme::LABEL_TRACKING,
        );
    }
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
    /// An action that doesn't apply now.
    Dimmed,
    Later,
    Leaf,
    /// A leaf with nothing to edit.
    ReadOnly,
    Prompt,
    Naming,
    ManageList,
    ManageCommands,
    /// A MANAGE command that doesn't apply, and why.
    ManageDimmed(Option<Note>),
    ManageLater,
    /// LOAD PROJECT's rows.
    Load,
    /// MIDI MAP's keys.
    #[cfg(feature = "midi-map")]
    Map,
    /// MIDI MAP's row 0: the controller map (ADR 0226).
    #[cfg(feature = "midi-map")]
    MapPick,
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
        (L::Action, false) => "EDIT RUN · MENU BACK",
        (L::Action, true) => "EDIT RUN · MENU CLOSE",
        (L::Dimmed, false) => "MENU BACK",
        (L::Dimmed, true) => "MENU CLOSE",
        (L::Later, false) => "LATER · MENU BACK",
        (L::Later, true) => "LATER · MENU CLOSE",
        (L::Leaf, _) => "A-F EDIT · MENU BACK",
        (L::ReadOnly, _) => "MENU BACK",
        (L::Prompt, _) => "EDIT OK · MENU BACK",
        (L::Naming, _) => "EDIT SAVE · MENU BACK",
        (L::ManageList, _) => "EDIT COMMANDS · MENU BACK",
        (L::ManageCommands, _) => "EDIT RUN · MENU LIST",
        (L::ManageDimmed(None), _) => "MENU LIST",
        (L::ManageDimmed(Some(Note::LoadToRename)), _) => "LOAD TO RENAME · MENU LIST",
        (L::ManageLater, _) => "LATER · MENU LIST",
        (L::Load, _) => "EDIT LOAD · MENU BACK",
        #[cfg(feature = "midi-map")]
        (L::Map, _) => "A MOVE · MIX+MINUS DELETES",
        #[cfg(feature = "midi-map")]
        (L::MapPick, _) => "B PICK · EDIT SAVE",
    }
}

/// The most rows a tree list holds.
const MAX_ROWS: usize = 16;

/// What SETTINGS' bands draw from in one frame.
#[derive(Clone, Copy)]
pub struct Bands<'a> {
    pub at: SettingsAt,
    /// The Part the PART crumb names.
    pub active: PartId,
    /// The list's first row shown (`first_visible`).
    pub first: usize,
    pub name: ProjectName,
    pub status: ProjectStatus,
    pub(crate) modal: Option<BandsModal<'a>>,
    /// A Screen's rows.
    pub listing: &'a Listing,
    /// What `● LOADED` marks.
    pub loaded: Option<ProjectFile>,
    /// In the PART branch, the active Part.
    pub part: Option<PartBand>,
    /// On MIDI MAP, the active Part's map and engine.
    #[cfg(feature = "midi-map")]
    pub map: Option<super::midi_map::MapBand<'a>>,
    /// The playing scene, for ORBIT's crumb; its band in the ORBIT branch.
    pub scene: SceneIdx,
    pub orbit: Option<OrbitBand>,
    /// TEXT SIZE, for the whole-width lists (ADR 0154).
    pub text: TextSize,
}

/// What is over SETTINGS' bands.
#[derive(Clone, Copy)]
pub(crate) enum BandsModal<'a> {
    /// The list beneath is blank; the legend is the prompt's.
    Prompt,
    /// In the list's place, under its title; its crumb ends the breadcrumb.
    Naming {
        naming: &'a Naming,
        of: &'a NamingFor,
    },
}

impl Bands<'_> {
    /// The list's rows; none on a leaf.
    fn rows(&self) -> &'static [Row] {
        rows(self.at.path())
    }

    pub fn legend(&self) -> &'static str {
        match self.modal {
            Some(BandsModal::Prompt) => return legend(LegendFor::Prompt, false),
            Some(BandsModal::Naming { .. }) => return legend(LegendFor::Naming, false),
            None => {}
        }
        if let Some(leaf) = self.at.at_leaf() {
            let on = match leaf.read_only() {
                true => LegendFor::ReadOnly,
                false => LegendFor::Leaf,
            };
            return legend(on, false);
        }
        match (self.at.screen(), self.at.column()) {
            (Some(Screen::LoadProject), _) => return legend(LegendFor::Load, false),
            (Some(Screen::SaveToProj), _) => {
                let live = self.part.zip(SAVE_ROWS.get(self.at.row() as usize));
                let on = match live.map(|(b, &c)| b.offer.look(c)) {
                    Some(RowLook::Normal) => LegendFor::Action,
                    _ => LegendFor::Dimmed,
                };
                return legend(on, false);
            }
            #[cfg(feature = "orbit")]
            (Some(Screen::SceneSaveTo), _) => return legend(LegendFor::Action, false),
            #[cfg(feature = "midi-map")]
            (Some(Screen::MidiMap), _) => {
                // By the bar's row (ADR 0288): `+ NEW BINDING` points, or
                // nothing on a full map.
                use super::midi_map::MapRow;
                let on = match self.map.filter(|m| m.open) {
                    None => LegendFor::Dimmed,
                    Some(m) => {
                        let t = m.text(self.at.row() as usize);
                        match (t.row, t.look) {
                            (MapRow::Pick, _) => LegendFor::MapPick,
                            (MapRow::Layer | MapRow::Own, _) => LegendFor::Map,
                            (MapRow::New, RowLook::Dimmed) => LegendFor::Dimmed,
                            (MapRow::New, RowLook::Normal | RowLook::Later) => LegendFor::Action,
                        }
                    }
                };
                return legend(on, false);
            }
            (_, Some(Column::Projects)) => return legend(LegendFor::ManageList, false),
            (_, Some(Column::Command(n))) => return legend(self.command_legend(n), false),
            _ => {}
        }
        let on = self
            .rows()
            .get(self.at.row() as usize)
            .map_or(LegendFor::Opens, |r| {
                let looks =
                    (self.part.map(|b| b.look(r.kind))).or(self.orbit.map(|b| b.look(r.kind)));
                match looks {
                    Some(RowLook::Dimmed) => LegendFor::Dimmed,
                    _ => LegendFor::of(r.kind),
                }
            });
        legend(on, self.at.path().is_empty())
    }

    /// The breadcrumb and its dots; not the load, which it never shows.
    pub fn crumbs_key(&self, s: HeaderStatus) -> u32 {
        let head = [
            self.at.path().len() as u8,
            self.active.index() as u8,
            self.scene.get(),
            s.sounding as u8,
            s.orbit_dot.is_some() as u8,
            s.orbit_dot.unwrap_or(0),
        ];
        settings_key(&[self.at.path(), &head, self.naming_crumb().as_bytes()])
    }

    fn naming_crumb(&self) -> &'static str {
        match self.modal {
            Some(BandsModal::Naming { of, .. }) => of.crumb(),
            _ => "",
        }
    }

    /// As drawn: NAMING's crumb ends it.
    pub fn crumbs(&self) -> Crumbs {
        let mut c = Crumbs::of(self.at.path(), self.active, self.scene);
        if let Some(BandsModal::Naming { of, .. }) = self.modal {
            c.push(Crumb::Name(of.crumb()));
        }
        c
    }

    pub fn list_key(&self) -> u32 {
        let col = match self.at.column() {
            None => 0,
            Some(Column::Projects) => 1,
            Some(Column::Command(n)) => 2 + n,
        };
        let list = [
            self.at.path().len() as u8,
            self.at.row(),
            self.first as u8,
            col,
        ];
        let (tag, text, title, cursor) = match &self.modal {
            None => (0, "", "", 0),
            Some(BandsModal::Prompt) => (1, "", "", 0),
            Some(BandsModal::Naming { naming, of }) => {
                (2, naming.text(), of.title(), naming.cursor())
            }
        };
        let rev = self.listing.revision().to_le_bytes();
        let loaded = self.loaded.map_or(0, |f| f.id().get()).to_le_bytes();
        let (pname, pkey) = match &self.part {
            Some(b) => {
                let [m0, m1] = b.mark.key();
                let [o0, o1, o2, o3, o4, o5] = b.offer.key();
                (
                    b.name.as_str(),
                    [1, self.active.index() as u8, m0, m1, o0, o1, o2, o3, o4, o5],
                )
            }
            None => ("", [0; 10]),
        };
        let okey = self.orbit.map_or([0xFF; 3], |b| b.key());
        #[cfg(feature = "midi-map")]
        let mkey = self.map_key().to_le_bytes();
        #[cfg(not(feature = "midi-map"))]
        let mkey = [0; 4];
        settings_key(&[
            pname.as_bytes(),
            &pkey,
            &okey,
            &mkey,
            self.at.path(),
            &list,
            text.as_bytes(),
            title.as_bytes(),
            &[tag, cursor, text.len() as u8],
            &rev,
            &loaded,
        ])
    }

    pub fn footer_key(&self) -> u32 {
        settings_key(&[
            self.name.as_str().as_bytes(),
            &[self.status as u8],
            self.legend().as_bytes(),
        ])
    }

    /// The breadcrumb, and the dots where the header has them.
    pub fn draw_crumbs<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D, s: HeaderStatus) {
        draw_crumbs(d, &self.crumbs());
        crate::ui::components::header_dots(d, s);
    }

    pub fn draw_list<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D) {
        match &self.modal {
            Some(BandsModal::Prompt) => return,
            Some(BandsModal::Naming { naming, of }) => {
                return draw_naming(d, naming, of.title());
            }
            None => {}
        }
        if self.at.screen() == Some(Screen::LoadProject) {
            let (l, loaded) = (self.listing, self.loaded);
            return draw_rows(
                d,
                l.load_rows(),
                self.at.row() as usize,
                self.first,
                self.text,
                |i, f| {
                    if let Some(r) = l.load_row(i, loaded) {
                        f(ListRow {
                            label: r.label.as_str(),
                            opens: false,
                            note: r.note,
                            look: r.look,
                        })
                    }
                },
            );
        }
        if let Some(col) = self.at.column() {
            return self.draw_manage(d, col);
        }
        #[cfg(feature = "midi-map")]
        if let Some(m) = &self.map {
            return self.draw_map(d, m);
        }
        if let Some(b) = &self.part {
            return self.draw_part(d, b);
        }
        if let Some(b) = &self.orbit {
            return self.draw_orbit(d, b);
        }
        let rows = self.rows();
        debug_assert!(rows.len() <= MAX_ROWS);
        let mut shown = [ListRow::of(&ROOT); MAX_ROWS];
        let n = rows.len().min(MAX_ROWS);
        for (s, r) in shown.iter_mut().zip(rows) {
            *s = ListRow::of(r);
        }
        draw_list(
            d,
            &shown[..n],
            self.at.row() as usize,
            self.first,
            self.text,
        );
    }

    /// MIDI MAP's rows and, beneath, the bar's binding spelled out; the
    /// empty page's words; closed, `NEEDS UPDATE`.
    #[cfg(feature = "midi-map")]
    fn draw_map<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D, m: &super::midi_map::MapBand) {
        if !m.open {
            return draw_rows(d, 1, usize::MAX, 0, self.text, |_, f| {
                f(ListRow {
                    label: crate::bank2::NEEDS_UPDATE,
                    opens: false,
                    note: None,
                    look: RowLook::Dimmed,
                })
            });
        }
        // No binding: how to learn one, under the pick and `+ NEW BINDING`
        // (two rows: the one line outruns the list).
        let note = ["NO CCS", "MIX+SEQ ON A CELL"];
        let rows = if m.empty { m.rows + note.len() } else { m.rows };
        let bar = self.at.row() as usize;
        let c = Col {
            size: self.text,
            ..FULL
        };
        draw_col::<MAP_ROWS, _>(
            d,
            &c,
            LIST_TOP,
            rows,
            bar,
            self.first,
            Bar::Keyed,
            |i, f| {
                if m.empty && i >= m.rows {
                    return f(ListRow {
                        label: note[i - m.rows],
                        opens: false,
                        note: None,
                        look: RowLook::Dimmed,
                    });
                }
                let t = m.text(i);
                f(ListRow {
                    label: t.label.as_str(),
                    opens: false,
                    note: Some(t.word.as_str()),
                    look: t.look,
                })
            },
        );
        if bar < m.rows {
            draw_map_focus(d, &m.text(bar));
        }
    }

    /// The map's rows as one number, for the list's region key (each
    /// row's words, the bar's lines among them, bank 2's `MapText::key`);
    /// closed, 1.
    #[cfg(feature = "midi-map")]
    fn map_key(&self) -> u32 {
        let Some(m) = &self.map else { return 0 };
        if !m.open {
            return 1;
        }
        let n = m.rows;
        (0..n).fold(n as u32, |h, i| h.rotate_left(7) ^ m.text(i).key)
    }

    /// The PART strip, then PART's rows or SAVE TO PROJ's.
    fn draw_part<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D, b: &PartBand) {
        draw_part_strip(d, self.active, b.name.as_str(), b.mark);
        let bar = self.at.row() as usize;
        if self.at.screen() == Some(Screen::SaveToProj) {
            return draw_part_list(d, SAVE_ROWS.len(), bar, 0, self.text, |i, f| {
                let c = SAVE_ROWS[i];
                f(ListRow {
                    label: b.offer.label(c).as_str(),
                    opens: false,
                    note: None,
                    look: b.offer.look(c),
                })
            });
        }
        let rows = self.rows();
        // RELOAD's slot is in the strip: beside the label, it would cut it.
        draw_part_list(d, rows.len(), bar, self.first, self.text, |i, f| {
            let r = &rows[i];
            f(ListRow {
                look: b.look(r.kind),
                ..ListRow::of(r)
            })
        });
    }

    /// ORBIT's rows or SAVE TO PROJ's.
    fn draw_orbit<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D, b: &OrbitBand) {
        let bar = self.at.row() as usize;
        #[cfg(feature = "orbit")]
        if self.at.screen() == Some(Screen::SceneSaveTo) {
            return draw_rows(
                d,
                SceneIdx::ALL.len(),
                bar,
                self.first,
                self.text,
                |i, f| {
                    f(ListRow {
                        label: b.save_label(SceneIdx::ALL[i]).as_str(),
                        opens: false,
                        note: None,
                        look: RowLook::Normal,
                    })
                },
            );
        }
        let rows = self.rows();
        draw_rows(d, rows.len(), bar, self.first, self.text, |i, f| {
            let r = &rows[i];
            f(ListRow {
                look: b.look(r.kind),
                ..ListRow::of(r)
            })
        });
    }

    /// The keyed command's legend: what stops it, if anything.
    fn command_legend(&self, n: u8) -> LegendFor {
        let e = self.listing.entry(self.at.row() as usize);
        let on = Command::at(n)
            .zip(e)
            .map(|(c, e)| c.on(Whose::of(&e, self.loaded)));
        match on {
            Some(Ok(_)) => LegendFor::ManageCommands,
            Some(Err(Off::Dimmed(why))) => LegendFor::ManageDimmed(why),
            Some(Err(Off::Later(_))) => LegendFor::ManageLater,
            None => LegendFor::ManageDimmed(None),
        }
    }

    /// MANAGE: the listing, and the commands of the project under the bar.
    fn draw_manage<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D, col: Column) {
        let (l, loaded) = (self.listing, self.loaded);
        let bar = self.at.row() as usize;
        let whose = l.entry(bar).map(|e| Whose::of(&e, loaded));
        let cmd = match col {
            Column::Command(n) => Some(n),
            Column::Projects => None,
        };
        let row = |i, f: &mut dyn FnMut(ListRow<'_>)| {
            if let Some(r) = l.load_row(i, loaded) {
                f(ListRow {
                    label: r.label.as_str(),
                    opens: false,
                    // Only the loaded mark fits beside a name.
                    note: (r.note == Some(LOADED)).then_some("●"),
                    look: r.look,
                })
            }
        };
        draw_manage_rows(d, l.len(), bar, self.first, row, &command_rows(whose), cmd);
    }

    pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(&self, d: &mut D) {
        let name = crate::ui::components::upper(self.name.as_str());
        draw_footer(
            d,
            &Footer {
                name: name.as_str(),
                status: self.status,
                legend: self.legend(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `fit` skips measuring on `max_glyph`: no glyph is wider.
    #[test]
    fn max_glyph_bounds_every_glyph() {
        for c in [FULL, MANAGE_LIST, MANAGE_CMDS] {
            for b in 32u8..127 {
                let s = [b];
                let ch = core::str::from_utf8(&s).unwrap();
                let w = draw::text_width(c.font, ch, theme::LABEL_TRACKING);
                assert!(w <= c.max_glyph, "{ch:?}: {w} px");
            }
        }
    }
}
