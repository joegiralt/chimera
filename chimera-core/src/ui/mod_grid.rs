//! Mod matrix: routing state (cursor, amounts, sources, destinations) and
//! its dot grid (UI refresh spec § Page types).

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::addr::{BlockRef, ParamAddr};
use crate::mod_path::LABEL_LEN;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

/// Dot grid geometry (grid region y 118..266): destination labels across,
/// sources down, one dot per route.
pub const GRID_X: i32 = 58;
pub const GRID_COL_W: i32 = 40;
pub const GRID_TAG_Y: i32 = 130;
pub const GRID_NAME_Y: i32 = 140;
pub const GRID_ROW0_Y: i32 = 162;
pub const GRID_ROW_H: i32 = 24;
pub const HINT_Y: i32 = 236;
pub const STATS_Y: i32 = 254;
const VISIBLE_COLS: usize = 5;
const VISIBLE_ROWS: usize = 3;

/// Max sources and destinations for the amounts grid.
pub const MAX_SOURCES: usize = 16;
pub const MAX_DESTS: usize = 16;

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
pub struct ModSource {
    pub name: &'static str,
}

/// State for the mod matrix grid — cursor, scroll, mutable amounts, sources, and destinations.
#[derive(Clone, Debug)]
pub struct MatrixState {
    pub sel_row: usize,
    pub sel_col: usize,
    pub scroll_x: usize,
    pub scroll_y: usize,
    /// Modulation amounts: [source][dest_idx], -127 to +127. 0 = no connection.
    pub amounts: [[i8; MAX_DESTS]; MAX_SOURCES],
    /// Destination list, rebuilt from ModDestRegistry.
    pub dests: [Option<ModDest>; MAX_DESTS],
    pub num_dests: usize,
    /// Source list — built from mod matrix sub-pages.
    pub sources: [Option<ModSource>; MAX_SOURCES],
    pub num_sources: usize,
}

impl MatrixState {
    pub fn new() -> Self {
        let state = Self {
            sel_row: 0,
            sel_col: 0,
            scroll_x: 0,
            scroll_y: 0,
            amounts: [[0; MAX_DESTS]; MAX_SOURCES],
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
                self.sources[self.num_sources] = Some(ModSource { name });
                self.num_sources += 1;
            }
        }
    }

    /// Rebuild the destination list from a ModDestRegistry.
    pub fn rebuild_dests_from_registry(&mut self, registry: &crate::mod_path::ModDestRegistry) {
        self.num_dests = 0;
        for i in 0..registry.len() {
            if let Some(entry) = registry.get(i) {
                if self.num_dests < MAX_DESTS {
                    self.dests[self.num_dests] = Some(ModDest {
                        addr: entry.addr,
                        label: entry.label,
                    });
                    self.num_dests += 1;
                }
            }
        }
    }

    /// Amounts from a Part's `ModState`, matched by destination address
    /// (0 for a destination it does not route). Call after
    /// `rebuild_sources` and `rebuild_dests_from_registry`.
    pub fn load_amounts(&mut self, mod_state: &crate::modulation::ModState) {
        self.amounts = [[0; MAX_DESTS]; MAX_SOURCES];
        for di in 0..self.num_dests {
            let Some(dest) = self.dests[di] else { continue };
            let Some(d) = (0..mod_state.num_dests()).find(|&d| mod_state.dest(d) == dest.addr) else { continue };
            for si in 0..self.num_sources {
                self.amounts[si][di] = mod_state.amount(si, d);
            }
        }
    }

    /// Get the amount at the current cursor position.
    pub fn current_amount(&self) -> i8 {
        self.amounts[self.sel_row][self.sel_col]
    }

    /// Whether `addr` is a mod destination, and its summed amount (−1..1).
    /// `None` = not primed; `Some(0.0)` = primed with no amounts set.
    pub fn mod_info_for(&self, addr: ParamAddr) -> Option<f32> {
        for di in 0..self.num_dests {
            if let Some(dest) = &self.dests[di] {
                if dest.addr == addr {
                    let mut total: i16 = 0;
                    for si in 0..self.num_sources {
                        total += self.amounts[si][di] as i16;
                    }
                    return Some((total as f32 / 127.0).clamp(-1.0, 1.0));
                }
            }
        }
        None
    }

    /// Adjust the amount at the current cursor position.
    pub fn adjust_amount(&mut self, delta: i8) {
        let current = self.amounts[self.sel_row][self.sel_col] as i16;
        let new = (current + delta as i16).clamp(-127, 127) as i8;
        self.amounts[self.sel_row][self.sel_col] = new;
    }

    pub fn move_row(&mut self, delta: i8) {
        let new = self.sel_row as i32 + delta as i32;
        self.sel_row = new.clamp(0, self.num_sources as i32 - 1) as usize;
        // Auto-scroll to keep cursor visible
        let vis = self.visible_rows();
        if self.sel_row < self.scroll_y {
            self.scroll_y = self.sel_row;
        } else if self.sel_row >= self.scroll_y + vis {
            self.scroll_y = self.sel_row + 1 - vis;
        }
    }

    pub fn move_col(&mut self, delta: i8) {
        let max = if self.num_dests > 0 { self.num_dests - 1 } else { 0 };
        let new = self.sel_col as i32 + delta as i32;
        self.sel_col = new.clamp(0, max as i32) as usize;
        // Auto-scroll to keep cursor visible
        let vis = self.visible_cols();
        if self.sel_col < self.scroll_x {
            self.scroll_x = self.sel_col;
        } else if self.sel_col >= self.scroll_x + vis {
            self.scroll_x = self.sel_col + 1 - vis;
        }
    }

    pub fn scroll_v(&mut self, delta: i8) {
        let max = if self.num_sources > self.visible_rows() {
            self.num_sources - self.visible_rows()
        } else {
            0
        };
        let new = self.scroll_y as i32 + delta as i32;
        self.scroll_y = new.clamp(0, max as i32) as usize;
    }

    pub fn scroll_h(&mut self, delta: i8) {
        let max = if self.num_dests > self.visible_cols() {
            self.num_dests - self.visible_cols()
        } else {
            0
        };
        let new = self.scroll_x as i32 + delta as i32;
        self.scroll_x = new.clamp(0, max as i32) as usize;
    }

    pub fn visible_rows(&self) -> usize {
        VISIBLE_ROWS
    }

    pub fn visible_cols(&self) -> usize {
        VISIBLE_COLS
    }
}

/// Short tag for the block a destination lives in (column header, top line).
pub fn block_tag(b: BlockRef) -> &'static str {
    use crate::addr::Op;
    match b {
        BlockRef::Pizza => "PIZ",
        BlockRef::Modal => "MDL",
        BlockRef::Fm => "FM",
        BlockRef::FmOp(Op::A) => "OP1",
        BlockRef::FmOp(Op::B) => "OP2",
        BlockRef::FmOp(Op::C) => "OP3",
        BlockRef::FmOp(Op::D) => "OP4",
        BlockRef::Drive => "DRV",
        BlockRef::Filter => "FLT",
        BlockRef::Folder => "FLD",
        BlockRef::AmpEnv => "ENV",
        BlockRef::FilterEnv => "FEN",
        BlockRef::AuxEnv => "AEN",
        BlockRef::Lfo => "LFO",
        BlockRef::Out => "OUT",
        BlockRef::Chorus => "CHR",
        BlockRef::Delay => "DLY",
        BlockRef::Reverb => "REV",
        BlockRef::Part => "PRT",
    }
}

/// A destination's parameter name (its spec label).
pub fn dest_name(d: &ModDest) -> &'static str {
    d.addr.spec().map_or("?", |s| s.label)
}

/// A destination as the focus band names it: the column header's two lines
/// on one, `TAG NAME` (`OP1 LEVEL`), so operators read apart.
pub fn fmt_route_dest(buf: &mut FmtBuf, d: &ModDest) {
    let _ = write!(buf, "{} {}", block_tag(d.addr.block), dest_name(d));
}

/// Amount as shown: `+42`, `-30`, `0`.
pub fn fmt_amount(buf: &mut FmtBuf, amount: i8) {
    let _ = if amount > 0 { write!(buf, "+{}", amount) } else { write!(buf, "{}", amount) };
}

/// Route count and destination count, e.g. `"12 ROUTES   5 OF 16 DEST"`.
/// Sized to fit the 32-byte `FmtBuf` even at the worst case: routes up to
/// `MAX_MOD_SOURCES` × `MAX_DESTS` (three digits) and `num_dests` at
/// `MAX_DESTS`/`MAX_DESTS` — the original `"{} OF {} DESTINATIONS"` wording
/// overflowed at max counts, so this is the shortened form.
pub fn fmt_stats(buf: &mut FmtBuf, routes: usize, num_dests: usize) {
    let _ = write!(buf, "{} ROUTES   {} OF {} DEST", routes, num_dests, MAX_DESTS);
}

/// Centre of grid cell (visible column `ci`, visible row `vi`).
pub fn cell_center(ci: usize, vi: usize) -> (i32, i32) {
    (GRID_X + ci as i32 * GRID_COL_W, GRID_ROW0_Y + vi as i32 * GRID_ROW_H)
}

/// Dot grid: sources down, primed destinations across; a filled dot is a
/// positive amount, a ring negative, size = |amount|, a tiny dim dot none;
/// the selected cell outlined in the accent, its dot sized by `sel_amount`
/// (the lerped amount, so it grows with the focus band rather than
/// snapping). Then the hint and route count.
pub fn draw_grid<D>(d: &mut D, state: &MatrixState, sel_amount: i8)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (cols, rows) = (state.visible_cols(), state.visible_rows());
    for ci in 0..cols {
        let di = ci + state.scroll_x;
        let Some(Some(dest)) = state.dests.get(di).filter(|_| di < state.num_dests) else { break };
        let x = GRID_X + ci as i32 * GRID_COL_W;
        let name_color = if di == state.sel_col { theme::INK } else { theme::MID };
        draw::text_center(d, &theme::FONT_LABEL, block_tag(dest.addr.block), x, GRID_TAG_Y, theme::MID, 0);
        draw::text_center(d, &theme::FONT_LABEL, dest_name(dest), x, GRID_NAME_Y, name_color, 0);
    }
    if state.scroll_x > 0 {
        draw::text(d, &theme::FONT_LABEL, "<", GRID_X - 26, GRID_NAME_Y, theme::MID);
    }
    if state.num_dests > state.scroll_x + cols {
        draw::text(d, &theme::FONT_LABEL, ">", theme::SCREEN_W - 8, GRID_NAME_Y, theme::MID);
    }
    for vi in 0..rows {
        let ri = vi + state.scroll_y;
        if ri >= state.num_sources {
            break;
        }
        let (_, y) = cell_center(0, vi);
        let name = state.sources[ri].map_or("?", |s| s.name);
        let color = if ri == state.sel_row { theme::INK } else { theme::MID };
        draw::text(d, &theme::FONT_LABEL_BOLD, name, theme::MARGIN_X, y + 4, color);
        for ci in 0..cols {
            let di = ci + state.scroll_x;
            if di >= state.num_dests {
                break;
            }
            let (x, y) = cell_center(ci, vi);
            let selected = ri == state.sel_row && di == state.sel_col;
            let amount = if selected { sel_amount } else { state.amounts[ri][di] };
            if selected {
                draw::round_outline(d, x - 14, y - 11, 28, 22, 6, theme::ACCENT);
            }
            let r = 2 + (amount as i32).abs() * 8 / 127;
            let color = if selected { theme::ACCENT } else { theme::INK2 };
            match amount {
                0 => draw::dot(d, x, y, 1, theme::FAINT),
                a if a > 0 => draw::dot(d, x, y, r, color),
                _ => draw::ring(d, x, y, r, color, 1),
            }
        }
    }
    draw::text(d, &theme::FONT_LABEL, "MIX+PLUS ADD   MIX+MINUS REMOVE", theme::MARGIN_X, HINT_Y, theme::MID);
    let routes = (0..state.num_sources)
        .flat_map(|r| (0..state.num_dests).map(move |c| (r, c)))
        .filter(|&(r, c)| state.amounts[r][c] != 0)
        .count();
    let mut buf = FmtBuf::new();
    fmt_stats(&mut buf, routes, state.num_dests);
    draw::text(d, &theme::FONT_LABEL, buf.as_str(), theme::MARGIN_X, STATS_Y, theme::MID);
}
