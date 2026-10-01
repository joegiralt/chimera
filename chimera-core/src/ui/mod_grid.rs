//! Mod matrix: routing state (cursor, amounts, sources, destinations), its
//! amount grid and the route readout (UI refresh spec § Page types, #161).

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::addr::{BlockRef, ParamAddr};
use crate::mod_path::LABEL_LEN;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

/// Grid geometry (grid region 28..`GRID_BOTTOM`): destination labels
/// across, every source down (no vertical scroll), one amount cell per route.
/// A 12 px margin both sides: row labels at `MARGIN_X`, the last visible
/// cell ending at `SCREEN_W - MARGIN_X`.
pub const GRID_X: i32 = 52;
pub const GRID_COL_W: i32 = 36;
pub const GRID_TAG_Y: i32 = 38;
pub const GRID_NAME_Y: i32 = 47;
/// Top of the first row; a cell is inset `CELL_INSET` in its column and
/// spans `row + 1 .. row + GRID_ROW_H - 2`.
pub const GRID_ROW0_Y: i32 = 50;
pub const GRID_ROW_H: i32 = 17;
const CELL_INSET: i32 = 4;
pub const GRID_BOTTOM: i32 = 190;
/// The readout band below the grid (`GRID_BOTTOM`..cells bottom).
/// Readout: the route's names, then its amount and what it means.
pub const READOUT_Y: i32 = 206;
pub const AMOUNT_Y: i32 = 223;
pub const HINT_Y: i32 = 241;
pub const STATS_Y: i32 = 257;
/// Widest readout line: the screen less both margins.
pub const READOUT_MAX_W: i32 = theme::SCREEN_W - 2 * theme::MARGIN_X;

/// Rows top to bottom as `ModSource` indices: envelopes, then LFOs, VELO,
/// NOTE (the mockup's grouping). Amounts and presence stay by source index.
pub const ROW_ORDER: [usize; MAX_SOURCES] = {
    use crate::modulation::ModSource as S;
    [
        S::Env1 as usize,
        S::Env2 as usize,
        S::Env3 as usize,
        S::Lfo1 as usize,
        S::Lfo2 as usize,
        S::Lfo3 as usize,
        S::Vel as usize,
        S::Note as usize,
    ]
};
const VISIBLE_COLS: usize = 5;

/// Max sources and destinations for the amounts grid (presence is a `u8`).
pub const MAX_SOURCES: usize = crate::modulation::MAX_MOD_SOURCES;
pub const MAX_DESTS: usize = crate::modulation::MAX_MOD_DESTS;
const _: () = assert!(MAX_DESTS <= u16::BITS as usize, "inert_dests is a u16");

/// A destination in the mod matrix — a primed param.
#[derive(Clone, Copy, Debug)]
pub struct ModDest {
    pub addr: ParamAddr,
    pub label: [u8; LABEL_LEN],
}

impl ModDest {
    /// Return the label as a &str (up to the first NUL byte).
    pub fn label_str(&self) -> &str {
        let end = self.label.iter().position(|&b| b == 0).unwrap_or(LABEL_LEN);
        core::str::from_utf8(&self.label[..end]).unwrap_or("???")
    }
}

/// A source row in the mod matrix.
#[derive(Clone, Copy, Debug)]
pub struct SourceRow {
    pub name: &'static str,
}

/// State for the mod matrix grid — cursor, scroll, mutable amounts, sources, and destinations.
#[derive(Clone, Debug)]
pub struct MatrixState {
    pub sel_row: usize,
    pub sel_col: usize,
    pub scroll_x: usize,
    /// Modulation amounts: [source][dest_idx], -127 to +127 (0 when absent).
    pub amounts: [[i8; MAX_DESTS]; MAX_SOURCES],
    /// Route presence, one bit per source, as ModState's.
    pub present: [u8; MAX_DESTS],
    /// Bumped on every amount, presence or column change: part of the
    /// dirty-region keys.
    pub rev: u16,
    /// Destination list, rebuilt from ModDestRegistry.
    pub dests: [Option<ModDest>; MAX_DESTS],
    pub num_dests: usize,
    /// Source list — built from mod matrix sub-pages.
    pub sources: [Option<SourceRow>; MAX_SOURCES],
    pub num_sources: usize,
}

impl MatrixState {
    pub fn new() -> Self {
        let state = Self {
            sel_row: 0,
            sel_col: 0,
            scroll_x: 0,
            amounts: [[0; MAX_DESTS]; MAX_SOURCES],
            present: [0; MAX_DESTS],
            rev: 0,
            dests: [None; MAX_DESTS],
            num_dests: 0,
            sources: [None; MAX_SOURCES],
            num_sources: 0,
        };
        // Matrix starts empty — no destinations enabled, no amounts set.
        // Users enable params with MIX+Plus on block pages, set amounts in grid.
        state
    }

    /// Rebuild the source rows from the chain's `mod_sources`.
    /// Call this when the chain changes or at init.
    pub fn rebuild_sources(&mut self, names: &[&'static str]) {
        self.num_sources = 0;
        for &name in names {
            if self.num_sources < MAX_SOURCES {
                self.sources[self.num_sources] = Some(SourceRow { name });
                self.num_sources += 1;
            }
        }
    }

    /// Rebuild the destination list from a ModDestRegistry.
    pub fn rebuild_dests_from_registry(&mut self, registry: &crate::mod_path::ModDestRegistry) {
        self.num_dests = 0;
        for i in 0..registry.len() {
            if let Some(entry) = registry.get(i)
                && self.num_dests < MAX_DESTS
            {
                self.dests[self.num_dests] = Some(ModDest {
                    addr: entry.addr,
                    label: entry.label,
                });
                self.num_dests += 1;
            }
        }
        self.bump();
    }

    /// Amounts from a Part's `ModState`, matched by destination address
    /// (0 for a destination it does not route). Call after
    /// `rebuild_sources` and `rebuild_dests_from_registry`.
    pub fn load_amounts(&mut self, mod_state: &crate::modulation::ModState) {
        self.amounts = [[0; MAX_DESTS]; MAX_SOURCES];
        self.present = [0; MAX_DESTS];
        for di in 0..self.num_dests {
            let Some(dest) = self.dests[di] else { continue };
            let Some(d) = mod_state.find(dest.addr) else {
                continue;
            };
            for si in 0..self.num_sources {
                self.amounts[si][di] = mod_state.amount(si, d);
            }
            self.present[di] = mod_state.present(d);
        }
        self.bump();
    }

    fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// The source index at display row `row`: `ROW_ORDER` over the
    /// sources this matrix has.
    pub fn row_source(&self, row: usize) -> usize {
        ROW_ORDER
            .iter()
            .copied()
            .filter(|&s| s < self.num_sources)
            .nth(row)
            .unwrap_or(row)
    }

    /// The source under the cursor.
    pub fn sel_source(&self) -> usize {
        self.row_source(self.sel_row)
    }

    /// Get the amount at the current cursor position.
    pub fn current_amount(&self) -> i8 {
        self.amounts[self.sel_source()][self.sel_col]
    }

    /// Whether `addr` is a mod destination, and its summed amount (−1..1).
    /// `None` = not primed; `Some(0.0)` = primed with no amounts set.
    pub fn mod_info_for(&self, addr: ParamAddr) -> Option<f32> {
        for di in 0..self.num_dests {
            if let Some(dest) = &self.dests[di]
                && dest.addr == addr
            {
                let mut total: i16 = 0;
                for amounts in self.amounts.iter().take(self.num_sources) {
                    total += amounts[di] as i16;
                }
                return Some((total as f32 / 127.0).clamp(-1.0, 1.0));
            }
        }
        None
    }

    /// Adjust the amount at the current cursor position. A no-op when
    /// `sel_col` is stale (e.g. left over from a Part with more
    /// destinations, not yet clamped by `clamp_cursor`) — otherwise this
    /// would write into a column with no destination, which a later prime
    /// landing on that same column would then inherit as a phantom amount
    /// (issue #11).
    pub fn adjust_amount(&mut self, delta: i8) {
        if self.sel_col >= self.num_dests {
            return;
        }
        let src = self.sel_source();
        let a = self.amounts[src][self.sel_col];
        self.set(
            src,
            self.sel_col,
            (a as i16 + delta as i16).clamp(-127, 127) as i8,
        );
    }

    /// The column of destination `addr`, if the matrix has one.
    pub fn col_of(&self, addr: ParamAddr) -> Option<usize> {
        (0..self.num_dests).find(|&c| self.dests[c].is_some_and(|d| d.addr == addr))
    }

    /// The amount of route `row → addr`; `None` when the route is absent.
    pub fn route(&self, row: usize, addr: ParamAddr) -> Option<i8> {
        self.col_of(addr)
            .filter(|&c| self.is_present(row, c))
            .map(|c| self.amounts[row][c])
    }

    pub fn is_present(&self, row: usize, col: usize) -> bool {
        row < MAX_SOURCES && col < self.num_dests && self.present[col] & (1 << row) != 0
    }

    /// Set (and so create) cell (`row`, `col`); out of range does nothing.
    pub fn set(&mut self, row: usize, col: usize, amount: i8) {
        if row < self.num_sources && col < self.num_dests {
            self.amounts[row][col] = amount;
            self.present[col] |= 1 << row;
            self.bump();
        }
    }

    /// MIX+MINUS: delete the route under the cursor.
    pub fn delete_selected(&mut self) {
        let src = self.sel_source();
        if self.sel_col < self.num_dests && src < self.num_sources {
            self.amounts[src][self.sel_col] = 0;
            self.present[self.sel_col] &= !(1 << src);
            self.bump();
        }
    }

    /// Clamp the cursor and scroll position to the current source/
    /// destination counts, keeping the cursor inside the visible columns —
    /// the rule `move_col` uses. Call after `rebuild_sources`/
    /// `rebuild_dests_from_registry` (e.g. on a Part switch), whose new
    /// counts may be smaller than the cursor/scroll position left over from
    /// before (issue #11).
    pub fn clamp_cursor(&mut self) {
        self.sel_row = self.sel_row.min(self.num_sources.saturating_sub(1));
        self.sel_col = self.sel_col.min(self.num_dests.saturating_sub(1));
        self.scroll_x = self.scroll_x.min(self.max_scroll_x());
        self.follow_col();
    }

    /// Every source is on screen: rows never scroll.
    pub fn move_row(&mut self, delta: i8) {
        let new = self.sel_row as i32 + delta as i32;
        self.sel_row = new.clamp(0, self.num_sources.saturating_sub(1) as i32) as usize;
    }

    pub fn move_col(&mut self, delta: i8) {
        let new = self.sel_col as i32 + delta as i32;
        self.sel_col = new.clamp(0, self.num_dests.saturating_sub(1) as i32) as usize;
        self.follow_col();
    }

    /// Scroll the columns; the cursor comes along when it would leave the
    /// screen, so the readout never names a hidden cell.
    pub fn scroll_h(&mut self, delta: i8) {
        let new = self.scroll_x as i32 + delta as i32;
        self.scroll_x = new.clamp(0, self.max_scroll_x() as i32) as usize;
        let last = (self.scroll_x + self.visible_cols()).min(self.num_dests.max(1)) - 1;
        self.sel_col = self.sel_col.clamp(self.scroll_x, last.max(self.scroll_x));
    }

    fn max_scroll_x(&self) -> usize {
        self.num_dests.saturating_sub(self.visible_cols())
    }

    /// Scroll just enough to show the cursor's column.
    fn follow_col(&mut self) {
        let vis = self.visible_cols();
        if self.sel_col < self.scroll_x {
            self.scroll_x = self.sel_col;
        } else if self.sel_col >= self.scroll_x + vis {
            self.scroll_x = self.sel_col + 1 - vis;
        }
    }

    pub fn visible_cols(&self) -> usize {
        VISIBLE_COLS
    }
}

impl Default for MatrixState {
    fn default() -> Self {
        Self::new()
    }
}

/// The matrix's hint line (spec § UI: MIX+MINUS deletes a route).
pub const HINT: &str = "PRIME MIX+PLUS  DELETE MIX+MINUS";

/// Short tag for the block a destination lives in (column header, top line).
pub fn block_tag(b: BlockRef) -> &'static str {
    match b {
        BlockRef::Modal => "MDL",
        BlockRef::AlgoOp(op) => ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][op.index()],
        BlockRef::Algo => "ALG",
        BlockRef::Drive => "DRV",
        BlockRef::Filter => "FLT",
        BlockRef::Folder => "FLD",
        BlockRef::Env(s) => ["E1", "E2", "E3"][s.index()],
        BlockRef::Lfo(s) => ["LF1", "LF2", "LF3"][s.index()],
        BlockRef::Out => "OUT",
        BlockRef::Pitch => "PIT",
        BlockRef::Chorus => "CHR",
        BlockRef::Delay => "DLY",
        BlockRef::Reverb => "REV",
        BlockRef::Tape => "TPE",
        BlockRef::Comp => "CMP",
        BlockRef::Part => "PRT",
        BlockRef::Theme => "THM",
        BlockRef::Channels => "MID",
    }
}

/// A destination's parameter name (its spec label).
pub fn dest_name(d: &ModDest) -> &'static str {
    d.addr.spec().map_or("?", |s| s.label)
}

/// Widest column header: the column pitch minus a 3px gap, so neighbouring
/// headers never touch (#22).
pub const HEADER_MAX_W: i32 = GRID_COL_W - 3;

/// A column header's name: the spec's `short` (`CUT`), else its `label`,
/// clipped to `HEADER_MAX_W` only as a fallback. The readout shows the full
/// name.
pub fn fit_header(spec: &crate::block::ParamSpec) -> &'static str {
    let label = spec.short.unwrap_or(spec.label);
    let max = HEADER_MAX_W;
    let mut end = label.len();
    while end > 0 && draw::text_width(&theme::FONT_LABEL, &label[..end], 0) > max {
        end = label[..end].char_indices().last().map_or(0, |(i, _)| i);
    }
    &label[..end]
}

/// A destination as the focus band names it: the column header's two lines
/// on one, `TAG NAME` (`OP1 LEVEL`), so operators read apart.
pub fn fmt_route_dest(buf: &mut FmtBuf, d: &ModDest) {
    let _ = write!(buf, "{} {}", block_tag(d.addr.block), dest_name(d));
}

/// A block's full name, for the readout (`FILTER`; `block_tag` is `FLT`).
pub fn block_name(b: BlockRef) -> &'static str {
    match b {
        BlockRef::Modal => "MODAL",
        BlockRef::AlgoOp(op) => [
            "OPERATOR 1",
            "OPERATOR 2",
            "OPERATOR 3",
            "OPERATOR 4",
            "OPERATOR 5",
            "OPERATOR 6",
        ][op.index()],
        BlockRef::Algo => "ALGO",
        BlockRef::Drive => "DRIVE",
        BlockRef::Filter => "FILTER",
        BlockRef::Folder => "FOLDER",
        BlockRef::Env(s) => ["ENV 1", "ENV 2", "ENV 3"][s.index()],
        BlockRef::Lfo(s) => ["LFO 1", "LFO 2", "LFO 3"][s.index()],
        BlockRef::Out => "OUTPUT",
        // `VOICE PITCH`, not `PITCH PITCH`.
        BlockRef::Pitch => "VOICE",
        BlockRef::Chorus => "CHORUS",
        BlockRef::Delay => "DELAY",
        BlockRef::Reverb => "REVERB",
        BlockRef::Tape => "TAPE",
        BlockRef::Comp => "COMP",
        BlockRef::Part => "PART",
        BlockRef::Theme => "THEME",
        BlockRef::Channels => "MIDI",
    }
}

/// Readout line width: `source`, the arrow, `dest`.
fn names_width(source: &str, dest: &str) -> i32 {
    let w = |s| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
    w(source) + ARROW_GAP + 8 + ARROW_GAP + w(dest)
}
const ARROW_GAP: i32 = 5;

/// The readout's destination: `FILTER CUTOFF`, or `FLT CUTOFF` when the
/// full block name would push the line past `READOUT_MAX_W`. The param
/// name is never cut.
pub fn fmt_readout_dest(buf: &mut FmtBuf, source: &str, d: &ModDest) {
    let _ = write!(buf, "{} {}", block_name(d.addr.block), dest_name(d));
    if names_width(source, buf.as_str()) > READOUT_MAX_W {
        *buf = FmtBuf::new();
        fmt_route_dest(buf, d);
    }
}

/// A route's amount and what it does to `spec`, by its offset law:
/// `+42 = +3.3 oct` (octaves), `+32 = +6.0 st` (semitones), `+32 = +25 ct`
/// (cents), else `+42 = +33%` of the param's span.
pub fn fmt_route_effect(buf: &mut FmtBuf, spec: &crate::block::ParamSpec, amount: i8) {
    fmt_amount(buf, amount);
    let sign = match amount {
        0 => "",
        a if a < 0 => "-",
        _ => "+",
    };
    let off = (amount as f32 / 127.0).abs();
    let _ = write!(buf, " = {sign}");
    match spec.law {
        crate::block::OffsetLaw::Octaves(_) if amount == 0 => {
            let _ = buf.write_str("0 oct");
        }
        crate::block::OffsetLaw::Octaves(n) => crate::ui::fmt::fixed(buf, off * n, 1, "oct"),
        crate::block::OffsetLaw::Semitones(_) if amount == 0 => {
            let _ = buf.write_str("0 st");
        }
        crate::block::OffsetLaw::Semitones(n) => crate::ui::fmt::fixed(buf, off * n, 1, "st"),
        crate::block::OffsetLaw::Cents(n) => {
            let _ = write!(buf, "{} ct", libm::roundf(off * n) as i32);
        }
        crate::block::OffsetLaw::Linear => {
            let _ = write!(buf, "{}%", libm::roundf(off * 100.0) as i32);
        }
    }
}

/// Amount as shown: `+42`, `-30`, `0`.
pub fn fmt_amount(buf: &mut FmtBuf, amount: i8) {
    let _ = if amount > 0 {
        write!(buf, "+{}", amount)
    } else {
        write!(buf, "{}", amount)
    };
}

/// Route count and destination count, e.g. `"12 ROUTES   5 OF 16 DEST"`.
/// Sized to fit the 32-byte `FmtBuf` even at the worst case: routes up to
/// `MAX_MOD_SOURCES` × `MAX_DESTS` (three digits) and `num_dests` at
/// `MAX_DESTS`/`MAX_DESTS` — the original `"{} OF {} DESTINATIONS"` wording
/// overflowed at max counts, so this is the shortened form.
pub fn fmt_stats(buf: &mut FmtBuf, routes: usize, num_dests: usize) {
    let _ = write!(
        buf,
        "{} ROUTES   {} OF {} DEST",
        routes, num_dests, MAX_DESTS
    );
}

/// Top-left of the cell box at (visible column `ci`, visible row `vi`).
pub fn cell_origin(ci: usize, vi: usize) -> (i32, i32) {
    (
        GRID_X + ci as i32 * GRID_COL_W + CELL_INSET,
        GRID_ROW0_Y + vi as i32 * GRID_ROW_H + 1,
    )
}

/// Cell box width and height.
pub const CELL_W: i32 = GRID_COL_W - 2 * CELL_INSET;
pub const CELL_H: i32 = GRID_ROW_H - 3;

/// Centre of the cell at (visible column `ci`, visible row `vi`).
pub fn cell_center(ci: usize, vi: usize) -> (i32, i32) {
    let (x, y) = cell_origin(ci, vi);
    (x + CELL_W / 2, y + CELL_H / 2)
}

/// The amount grid: sources down, primed destinations across. A route's
/// cell is lit and prints its amount (`+60`, `-30`, `0` in the rest grey);
/// an absent route is an empty outline. The cursor's cell is outlined in
/// the accent and prints `sel_amount`, the lerped amount. A column in
/// `inert` (`inert_dests`) reads as a dimmed cell does: its name and
/// amounts in MID, its routes unlit.
pub fn draw_grid<D>(d: &mut D, state: &MatrixState, sel_amount: i8, inert: u16)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cols = state.visible_cols();
    let col_x = |ci: usize| GRID_X + ci as i32 * GRID_COL_W + GRID_COL_W / 2;
    for ci in 0..cols {
        let di = ci + state.scroll_x;
        let Some(Some(dest)) = state.dests.get(di).filter(|_| di < state.num_dests) else {
            break;
        };
        let name_color = if di == state.sel_col && inert & (1 << di) == 0 {
            theme::INK
        } else {
            theme::MID
        };
        let x = col_x(ci);
        draw::text_center(
            d,
            &theme::FONT_LABEL,
            block_tag(dest.addr.block),
            x,
            GRID_TAG_Y,
            theme::MID,
            0,
        );
        draw::text_center(
            d,
            &theme::FONT_LABEL,
            dest.addr.spec().map_or("?", fit_header),
            x,
            GRID_NAME_Y,
            name_color,
            0,
        );
    }
    if state.scroll_x > 0 {
        draw::text(
            d,
            &theme::FONT_LABEL,
            "<",
            theme::MARGIN_X,
            GRID_NAME_Y,
            theme::MID,
        );
    }
    if state.num_dests > state.scroll_x + cols {
        // On the tag row, in the right margin: tags are <= 3 chars
        // (`block_tag`), so the last column's never reaches it, unlike a
        // name (issue #15).
        draw::text(
            d,
            &theme::FONT_LABEL,
            ">",
            theme::SCREEN_W - 8,
            GRID_TAG_Y,
            theme::MID,
        );
    }
    for row in 0..state.num_sources.min(MAX_SOURCES) {
        let si = state.row_source(row);
        let (_, y) = cell_origin(0, row);
        let name = state.sources[si].map_or("?", |s| s.name);
        let color = if row == state.sel_row {
            theme::INK
        } else {
            theme::MID
        };
        draw::text(
            d,
            &theme::FONT_LABEL_BOLD,
            name,
            theme::MARGIN_X,
            y + 10,
            color,
        );
        for ci in 0..cols {
            let di = ci + state.scroll_x;
            if di >= state.num_dests {
                break;
            }
            let selected = row == state.sel_row && di == state.sel_col;
            let amount = if selected {
                sel_amount
            } else {
                state.amounts[si][di]
            };
            draw_cell(
                d,
                ci,
                row,
                state.is_present(si, di).then_some(amount),
                selected,
                inert & (1 << di) != 0,
            );
        }
    }
}

/// One cell: an outline (the accent under the cursor); a route fills it
/// and prints its amount, unlit in MID when `inert`.
fn draw_cell<D>(d: &mut D, ci: usize, vi: usize, route: Option<i8>, selected: bool, inert: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x, y) = cell_origin(ci, vi);
    let edge = if selected {
        theme::ACCENT
    } else {
        theme::FAINT
    };
    draw::round_outline(d, x, y, CELL_W, CELL_H, 0, edge);
    let Some(amount) = route else { return };
    let text = if inert {
        theme::MID
    } else if amount == 0 {
        theme::BAR_REST
    } else {
        draw::fill_rect(d, x + 1, y + 1, CELL_W - 2, CELL_H - 2, theme::ACCENT_SOFT);
        theme::ACCENT
    };
    let mut buf = FmtBuf::new();
    fmt_amount(&mut buf, amount);
    draw::text_center(
        d,
        &theme::FONT_LABEL_BOLD,
        buf.as_str(),
        x + CELL_W / 2,
        y + 10,
        text,
        0,
    );
}

/// The band under the grid: the cursor's route by full names (`ENV 3 →
/// FILTER CUTOFF`), then its lerped amount and effect (`+42 = +3.3 oct`,
/// `--` with no route; in MID and tagged `INERT` on an `inert` column), the
/// hint and the route count. `NO DESTINATIONS` without one.
pub fn draw_readout<D>(d: &mut D, state: &MatrixState, sel_amount: i8, inert: u16)
where
    D: DrawTarget<Color = Rgb565>,
{
    let dest = state
        .dests
        .get(state.sel_col)
        .copied()
        .flatten()
        .filter(|_| state.sel_col < state.num_dests);
    let si = state.sel_source();
    let src = crate::modulation::ModSource::ALL
        .get(si)
        .filter(|_| si < state.num_sources);
    let label = |d: &mut D, s: &str, x: i32, color| {
        x + draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            s,
            x,
            READOUT_Y,
            color,
            theme::LABEL_TRACKING,
        )
    };
    match (src, dest) {
        (Some(src), Some(dest)) => {
            let x = label(d, src.name(), theme::MARGIN_X, theme::ACCENT) + ARROW_GAP;
            let x = x + draw::arrow(d, x, READOUT_Y, theme::ACCENT) + ARROW_GAP;
            let mut name = FmtBuf::new();
            fmt_readout_dest(&mut name, src.name(), &dest);
            label(d, name.as_str(), x, theme::ACCENT);
            let mut effect = FmtBuf::new();
            let dead = inert & (1 << state.sel_col) != 0;
            let color = match dest.addr.spec() {
                Some(spec) if state.is_present(si, state.sel_col) => {
                    fmt_route_effect(&mut effect, spec, sel_amount);
                    if dead { theme::MID } else { theme::INK }
                }
                _ => {
                    let _ = effect.write_str("--");
                    theme::MID
                }
            };
            draw::text(
                d,
                &theme::FONT_VALUE,
                effect.as_str(),
                theme::MARGIN_X,
                AMOUNT_Y,
                color,
            );
            if dead {
                let w = draw::text_width(&theme::FONT_VALUE, effect.as_str(), 0);
                draw::text_tracked(
                    d,
                    &theme::FONT_LABEL,
                    "INERT",
                    theme::MARGIN_X + w + 2 * ARROW_GAP,
                    AMOUNT_Y,
                    theme::MID,
                    theme::LABEL_TRACKING,
                );
            }
        }
        _ => {
            label(d, "NO DESTINATIONS", theme::MARGIN_X, theme::MID);
        }
    }
    draw::text(
        d,
        &theme::FONT_LABEL,
        HINT,
        theme::MARGIN_X,
        HINT_Y,
        theme::MID,
    );
    let routes = (0..state.num_sources)
        .flat_map(|r| (0..state.num_dests).map(move |c| (r, c)))
        .filter(|&(r, c)| state.is_present(r, c))
        .count();
    let mut buf = FmtBuf::new();
    fmt_stats(&mut buf, routes, state.num_dests);
    draw::text(
        d,
        &theme::FONT_LABEL,
        buf.as_str(),
        theme::MARGIN_X,
        STATS_Y,
        theme::MID,
    );
}

/// Bit `di`: destination column `di` is a param `view::dimmed` rules
/// inapplicable (MORPH while ALG A = ALG B), so its routes do nothing.
pub fn inert_dests(state: &MatrixState, sound: &crate::preset::Sound) -> u16 {
    (0..state.num_dests).fold(0, |m, di| match state.dests[di] {
        Some(d) if crate::ui::view::dimmed(d.addr, sound) => m | 1 << di,
        _ => m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::modal::{ModalParams, ResonatorMode};
    use crate::mod_path::ModDestRegistry;
    use crate::modulation::CUTOFF;
    use crate::params::EngineType;
    use crate::part::PartParams;
    use crate::preset::Sound;
    use crate::ui::view::dimmed;

    const MACROS: [crate::block::ParamId; 4] = [
        ModalParams::STRUCTURE,
        ModalParams::BRIGHT,
        ModalParams::DAMP,
        ModalParams::POS,
    ];

    #[test]
    fn bowed_dims_structure_and_its_column() {
        let modal = |id| ParamAddr::new(BlockRef::Modal, id);
        let mut s = Sound::init(EngineType::Modal);
        s.params.modal.mode = ResonatorMode::Bowed;
        assert!(dimmed(modal(ModalParams::STRUCTURE), &s));
        for id in MACROS.into_iter().skip(1).chain([ModalParams::MODE]) {
            assert!(!dimmed(modal(id), &s), "{id:?}");
        }
        let space = ParamAddr::new(BlockRef::Part, PartParams::SEND_REVERB);
        assert!(!dimmed(space, &s));

        let mut reg = ModDestRegistry::new();
        reg.add(CUTOFF, *b"CUTOFF\0\0").unwrap();
        reg.add(modal(ModalParams::STRUCTURE), *b"MDLSTRUC")
            .unwrap();
        reg.add(modal(ModalParams::DAMP), *b"MDLDAMP\0").unwrap();
        let mut m = MatrixState::new();
        m.rebuild_dests_from_registry(&reg);
        assert_eq!(inert_dests(&m, &s), 0b10, "STRUCTURE, column 2");
        s.params.modal.mode = ResonatorMode::String;
        assert_eq!(inert_dests(&m, &s), 0);
    }
}
