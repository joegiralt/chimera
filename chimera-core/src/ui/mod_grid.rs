//! Mod matrix: routing state (cursor, amounts, sources, destinations), its
//! amount grid and the route readout (UI refresh spec § Page types, #161).

use crate::dsp::clampf;
use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::tape_engine::{TapeModel, machine};
use crate::mod_path::LABEL_LEN;
use crate::modulation::{FixedCol, ModState};
use crate::preset::Sound;
use crate::taps::{TapOp, TapRoutes};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{self, Gesture, Hint};
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
/// centred in its row (`row_h`, `cell_h`).
pub const GRID_ROW0_Y: i32 = 50;
/// Eight rows' pitch.
pub const GRID_ROW_H: i32 = 17;
const CELL_INSET: i32 = 4;
pub const GRID_BOTTOM: i32 = 190;

/// Row pitch for `rows` display rows: they always fill `GRID_ROW0_Y` to
/// `GRID_BOTTOM` with no scroll, 17 for 8 and 14 for ALGO's 10.
pub const fn row_h(rows: usize) -> i32 {
    (GRID_BOTTOM - GRID_ROW0_Y) / if rows == 0 { 1 } else { rows as i32 }
}

/// A cell's height: 14 at 8 rows; at 14 px rows `CELL_INSET` leaves 10
/// (filter-types spec § 7).
pub const fn cell_h(rows: usize) -> i32 {
    let h = row_h(rows);
    if h < GRID_ROW_H {
        h - CELL_INSET
    } else {
        h - 3
    }
}

/// A cell's or row label's text baseline, from the cell's top.
pub const fn text_dy(rows: usize) -> i32 {
    cell_h(rows) / 2 + 3
}
/// The readout band below the grid (`GRID_BOTTOM`..cells bottom).
/// Readout: the route's names, then its amount and what it means.
pub const READOUT_Y: i32 = 206;
pub const AMOUNT_Y: i32 = 223;
pub const HINT_Y: i32 = 241;
pub const STATS_Y: i32 = 257;
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
/// The permanent columns, before the registry's.
pub const FIXED_COLS: usize = FixedCol::ALL.len();
const _: () = assert!(
    FIXED_COLS + MAX_DESTS <= u32::BITS as usize,
    "inert_dests is a u32"
);

/// A grid column: one of the four permanent ones, or registry column `d`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Fixed(FixedCol),
    Reg(usize),
}

/// What a display row shows: a matrix source (its `ModSource` index), or
/// on ALGO an operator's tap (spec § 7), whose cells are `TapRoutes`'.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSource {
    Mod(usize),
    Tap(TapOp),
}

impl RowSource {
    /// The index the cell API takes: a source's, then the taps' after
    /// `MAX_SOURCES`.
    pub const fn index(self) -> usize {
        match self {
            RowSource::Mod(s) => s,
            RowSource::Tap(t) => MAX_SOURCES + t as usize,
        }
    }
}

impl From<crate::taps::Jack> for RowSource {
    fn from(j: crate::taps::Jack) -> Self {
        match j {
            crate::taps::Jack::Mod(s) => RowSource::Mod(s.index()),
            crate::taps::Jack::Tap(t) => RowSource::Tap(t),
        }
    }
}

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
    /// The permanent columns' amounts and presence, as ModState's.
    pub fixed: [[i8; FIXED_COLS]; MAX_SOURCES],
    pub fixed_present: [u8; FIXED_COLS],
    /// The taps' cells, as the Sound's `TapRoutes`; rows only on ALGO.
    pub taps: TapRoutes,
    pub tap_rows: bool,
    /// Bumped on every amount, presence or column change: part of the
    /// dirty-region keys.
    pub rev: u16,
    /// Destination list, rebuilt from ModDestRegistry.
    pub dests: [Option<ModDest>; MAX_DESTS],
    pub num_dests: usize,
    /// Source list — built from mod matrix sub-pages.
    pub sources: [Option<SourceRow>; MAX_SOURCES],
    pub num_sources: usize,
    /// The edited Part's TAPE machine: its labels name TAPE's columns.
    pub tape: TapeModel,
}

impl MatrixState {
    pub fn new() -> Self {
        let state = Self {
            sel_row: 0,
            sel_col: 0,
            scroll_x: 0,
            amounts: [[0; MAX_DESTS]; MAX_SOURCES],
            present: [0; MAX_DESTS],
            fixed: [[0; FIXED_COLS]; MAX_SOURCES],
            fixed_present: [0; FIXED_COLS],
            taps: TapRoutes::default(),
            tap_rows: false,
            rev: 0,
            dests: [None; MAX_DESTS],
            num_dests: 0,
            sources: [None; MAX_SOURCES],
            num_sources: 0,
            tape: TapeModel::Play,
        };
        // Matrix starts empty — no destinations enabled, no amounts set.
        // Users enable params with MIX+Plus on block pages, set amounts in grid.
        state
    }

    /// The edited Part's machine; a change redraws the headers.
    pub fn set_tape(&mut self, m: TapeModel) {
        if self.tape != m {
            self.tape = m;
            self.bump();
        }
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
        for c in FixedCol::ALL {
            for si in 0..self.num_sources {
                self.fixed[si][c.index()] = mod_state.fixed_amount(si, c);
            }
            self.fixed_present[c.index()] = mod_state.fixed_present(c);
        }
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

    /// Everything from `s`: its chain's sources, its registry's columns,
    /// its routes and, on ALGO, its tap rows. The cursor is not clamped.
    pub fn load_sound(&mut self, s: &Sound) {
        self.rebuild_sources(crate::ui::nav::chain_def_for(s.engine()).mod_sources);
        self.rebuild_dests_from_registry(&s.dest_registry);
        self.load_amounts(&s.mod_state);
        self.tap_rows = s.engine() == crate::params::EngineType::Algo;
        self.load_taps(&s.params.taps);
    }

    pub fn load_taps(&mut self, t: &TapRoutes) {
        self.taps = *t;
        self.bump();
    }

    /// The tap cells into the Sound's `TapRoutes`.
    pub fn sync_taps(&self, t: &mut TapRoutes) {
        *t = self.taps;
    }

    /// Display rows: the sources, then OP5 and OP6 on ALGO.
    pub fn rows(&self) -> usize {
        self.num_sources + if self.tap_rows { TapOp::ALL.len() } else { 0 }
    }

    /// The tap at cell-API index `src`, if this matrix has its row.
    fn tap_of(&self, src: usize) -> Option<TapOp> {
        src.checked_sub(MAX_SOURCES)
            .and_then(|i| TapOp::ALL.get(i).copied())
            .filter(|_| self.tap_rows)
    }

    /// An impossible patch: a tap row's cell outside the four fixed
    /// columns. It takes no amount.
    pub fn is_x(&self, row: usize, col: usize) -> bool {
        matches!(self.row_source(row), RowSource::Tap(_)) && matches!(self.col(col), Col::Reg(_))
    }

    fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// What display row `row` shows: `ROW_ORDER` over the sources this
    /// matrix has, then on ALGO OP5 and OP6.
    pub fn row_source(&self, row: usize) -> RowSource {
        if let Some(t) = row
            .checked_sub(self.num_sources)
            .and_then(|i| TapOp::ALL.get(i))
            .filter(|_| self.tap_rows)
        {
            return RowSource::Tap(*t);
        }
        RowSource::Mod(
            ROW_ORDER
                .iter()
                .copied()
                .filter(|&s| s < self.num_sources)
                .nth(row)
                .unwrap_or(row),
        )
    }

    /// The cell-API index of the row under the cursor.
    pub fn sel_source(&self) -> usize {
        self.row_source(self.sel_row).index()
    }

    /// Get the amount at the current cursor position.
    pub fn current_amount(&self) -> i8 {
        self.amount(self.sel_source(), self.sel_col)
    }

    /// Every column: the four fixed, then the registry's.
    pub fn num_cols(&self) -> usize {
        FIXED_COLS + self.num_dests
    }

    /// What grid column `c` is (`c < FIXED_COLS` is fixed).
    pub fn col(&self, c: usize) -> Col {
        match FixedCol::ALL.get(c) {
            Some(&f) => Col::Fixed(f),
            None => Col::Reg(c - FIXED_COLS),
        }
    }

    /// Column `c`'s destination; `None` past the last column.
    pub fn col_dest(&self, c: usize) -> Option<ModDest> {
        match self.col(c) {
            Col::Fixed(f) => {
                let mut label = [0; LABEL_LEN];
                label[..f.tag().len()].copy_from_slice(f.tag().as_bytes());
                Some(ModDest {
                    addr: f.addr(),
                    label,
                })
            }
            Col::Reg(d) => self
                .dests
                .get(d)
                .copied()
                .flatten()
                .filter(|_| d < self.num_dests),
        }
    }

    /// The amount in cell (`row`, `c`), 0 when absent or out of range.
    pub fn amount(&self, row: usize, c: usize) -> i8 {
        if let Some(t) = self.tap_of(row) {
            return match self.col(c) {
                Col::Fixed(f) => self.taps.get(t, f).unwrap_or(0),
                Col::Reg(_) => 0,
            };
        }
        if row >= MAX_SOURCES {
            return 0;
        }
        match self.col(c) {
            Col::Fixed(f) => self.fixed[row][f.index()],
            Col::Reg(d) if d < self.num_dests => self.amounts[row][d],
            Col::Reg(_) => 0,
        }
    }

    /// Column `c`'s presence bits.
    fn present_bits(&self, c: usize) -> u8 {
        match self.col(c) {
            Col::Fixed(f) => self.fixed_present[f.index()],
            Col::Reg(d) if d < self.num_dests => self.present[d],
            Col::Reg(_) => 0,
        }
    }

    /// Whether `addr` is a mod destination, and its summed amount (−1..1).
    /// `None` = not primed; `Some(0.0)` = primed with no amounts set.
    /// A fixed column counts as primed once it has a route.
    pub fn mod_info_for(&self, addr: ParamAddr) -> Option<f32> {
        let c = self
            .col_of(addr)
            .filter(|&c| matches!(self.col(c), Col::Reg(_)) || self.present_bits(c) != 0)?;
        let total: i16 = (0..self.num_sources)
            .map(|si| self.amount(si, c) as i16)
            .sum();
        Some(clampf(total as f32 / 127.0, -1.0, 1.0))
    }

    /// Adjust the amount at the current cursor position. A no-op when
    /// `sel_col` is stale (e.g. left over from a Part with more
    /// destinations, not yet clamped by `clamp_cursor`) — otherwise this
    /// would write into a column with no destination, which a later prime
    /// landing on that same column would then inherit as a phantom amount
    /// (issue #11).
    pub fn adjust_amount(&mut self, delta: i8) {
        if self.sel_col >= self.num_cols() {
            return;
        }
        let src = self.sel_source();
        let a = self.amount(src, self.sel_col);
        self.set(
            src,
            self.sel_col,
            (a as i16 + delta as i16).clamp(-127, 127) as i8,
        );
    }

    /// The grid column of destination `addr`, if the matrix has one: a
    /// fixed column always.
    pub fn col_of(&self, addr: ParamAddr) -> Option<usize> {
        match FixedCol::of(addr) {
            Some(f) => Some(f.index()),
            None => (0..self.num_dests)
                .find(|&d| self.dests[d].is_some_and(|x| x.addr == addr))
                .map(|d| FIXED_COLS + d),
        }
    }

    /// The amount of route `row → addr`; `None` when the route is absent.
    pub fn route(&self, row: usize, addr: ParamAddr) -> Option<i8> {
        self.col_of(addr)
            .filter(|&c| self.is_present(row, c))
            .map(|c| self.amount(row, c))
    }

    pub fn is_present(&self, row: usize, col: usize) -> bool {
        if let Some(t) = self.tap_of(row) {
            return matches!(self.col(col), Col::Fixed(f) if self.taps.get(t, f).is_some());
        }
        row < MAX_SOURCES && self.present_bits(col) & (1 << row) != 0
    }

    /// Set (and so create) cell (`row`, `col`); out of range, or an X
    /// cell, does nothing.
    pub fn set(&mut self, row: usize, col: usize, amount: i8) {
        if let Some(t) = self.tap_of(row) {
            if let Col::Fixed(f) = self.col(col) {
                self.taps.set(t, f, amount);
                self.bump();
            }
            return;
        }
        if row >= self.num_sources {
            return;
        }
        match self.col(col) {
            Col::Fixed(f) => {
                self.fixed[row][f.index()] = amount;
                self.fixed_present[f.index()] |= 1 << row;
            }
            Col::Reg(d) if d < self.num_dests => {
                self.amounts[row][d] = amount;
                self.present[d] |= 1 << row;
            }
            Col::Reg(_) => return,
        }
        self.bump();
    }

    /// MIX+MINUS: delete the route under the cursor.
    pub fn delete_selected(&mut self) {
        self.clear(self.sel_source(), self.sel_col);
    }

    /// Delete cell (`src`, `col`)'s route; out of range, or an X cell, does
    /// nothing.
    pub fn clear(&mut self, src: usize, col: usize) {
        if let Some(t) = self.tap_of(src) {
            if let Col::Fixed(f) = self.col(col) {
                self.taps.clear(t, f);
                self.bump();
            }
            return;
        }
        if src >= self.num_sources {
            return;
        }
        match self.col(col) {
            Col::Fixed(f) => {
                self.fixed[src][f.index()] = 0;
                self.fixed_present[f.index()] &= !(1 << src);
            }
            Col::Reg(d) if d < self.num_dests => {
                self.amounts[src][d] = 0;
                self.present[d] &= !(1 << src);
            }
            Col::Reg(_) => return,
        }
        self.bump();
    }

    /// Clamp the cursor and scroll position to the current source/
    /// destination counts, keeping the cursor inside the visible columns —
    /// the rule `move_col` uses. Call after `rebuild_sources`/
    /// `rebuild_dests_from_registry` (e.g. on a Part switch), whose new
    /// counts may be smaller than the cursor/scroll position left over from
    /// before (issue #11).
    pub fn clamp_cursor(&mut self) {
        self.sel_row = self.sel_row.min(self.rows().saturating_sub(1));
        self.sel_col = self.sel_col.min(self.num_cols() - 1);
        self.scroll_x = self.scroll_x.min(self.max_scroll_x());
        self.follow_col();
    }

    /// Every source is on screen: rows never scroll.
    pub fn move_row(&mut self, delta: i8) {
        let new = self.sel_row as i32 + delta as i32;
        self.sel_row = new.clamp(0, self.rows().saturating_sub(1) as i32) as usize;
    }

    pub fn move_col(&mut self, delta: i8) {
        let new = self.sel_col as i32 + delta as i32;
        self.sel_col = new.clamp(0, self.num_cols() as i32 - 1) as usize;
        self.follow_col();
    }

    /// Scroll the columns; the cursor comes along when it would leave the
    /// screen, so the readout never names a hidden cell.
    pub fn scroll_h(&mut self, delta: i8) {
        let new = self.scroll_x as i32 + delta as i32;
        self.scroll_x = new.clamp(0, self.max_scroll_x() as i32) as usize;
        let last = (self.scroll_x + self.visible_cols()).min(self.num_cols()) - 1;
        self.sel_col = self.sel_col.clamp(self.scroll_x, last.max(self.scroll_x));
    }

    fn max_scroll_x(&self) -> usize {
        self.num_cols().saturating_sub(self.visible_cols())
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

/// The cursor's gesture (ADR 0086): MIX + MINUS on a route deletes it;
/// on no route, where its columns come from, MIX + PLUS on a param.
pub const DELETE: Hint = &[Gesture("MIX + MINUS", "DELETES THE ROUTE")];
pub const PRIME: Hint = &[Gesture("MIX + PLUS", "ON A PARAM PRIMES IT")];
/// An X cell's: what a tap can reach (spec § 7). The spec's long form,
/// `OP5 · AUDIO RATE: …`, is 264 px, past the 216 a hint may take.
/// An X cell's readout: what its tap reaches, then why the cell takes no
/// amount.
pub const X_REACH: &str = "FREQ Q VCA FOLD";
pub const X_NOT: &str = "NOT A TAP DESTINATION";
pub const X_OP5: Hint = &[Gesture("OP5:", "FREQ Q VCA FOLD ONLY")];
pub const X_OP6: Hint = &[Gesture("OP6:", "FREQ Q VCA FOLD ONLY")];

impl MatrixState {
    /// `mods` routed as this matrix is (`ModState::sync_from_matrix`).
    pub fn sync_into(&self, mods: &mut ModState) {
        let columns = self.dests[..self.num_dests].iter().zip(0..).map(|(d, di)| {
            let amounts = core::array::from_fn(|si| self.amounts[si][di]);
            (d.map(|d| d.addr), amounts, self.present[di])
        });
        mods.sync_from_matrix(self.num_sources, (self.fixed, self.fixed_present), columns);
    }

    /// The hint under the cursor's route.
    pub fn hint(&self) -> Hint {
        if self.is_x(self.sel_row, self.sel_col) {
            return match self.row_source(self.sel_row) {
                RowSource::Tap(TapOp::Op6) => X_OP6,
                _ => X_OP5,
            };
        }
        if self.is_present(self.sel_source(), self.sel_col) {
            DELETE
        } else {
            PRIME
        }
    }
}

/// A registry column's stored 8-byte label: `O5 LEVEL`, `E2 DEC`, else
/// the block's tag (3 letters at most), a space and the param's label,
/// cut at 8 (`ALG MORP`). One spelling for the unit's priming and the
/// host's factory Sources; the grid draws its column from the address.
pub fn dest_label(a: ParamAddr) -> [u8; LABEL_LEN] {
    let tag = block_tag(a.block).as_bytes();
    let (prefix, n): ([u8; 4], usize) = match a.block {
        BlockRef::AlgoOp(op) => ([b'O', b'1' + op.index() as u8, b' ', 0], 3),
        BlockRef::Env(e) => ([b'E', b'1' + e.index() as u8, b' ', 0], 3),
        _ => {
            let n = tag.len().min(3);
            let mut p = [b' '; 4];
            p[..n].copy_from_slice(&tag[..n]);
            (p, n + 1)
        }
    };
    let name = a.spec().map_or("", |s| s.label).as_bytes();
    let mut label = [0u8; LABEL_LEN];
    for (o, &c) in label.iter_mut().zip(prefix[..n].iter().chain(name)) {
        *o = c;
    }
    label
}

/// Short tag for the block a destination lives in (column header, top line).
pub fn block_tag(b: BlockRef) -> &'static str {
    match b {
        BlockRef::Modal => "MDL",
        BlockRef::TapeEngine => "TPE",
        BlockRef::Va => "VA",
        BlockRef::AlgoOp(op) => ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][op.index()],
        BlockRef::Algo => "ALG",
        BlockRef::Drive => "DRV",
        BlockRef::Filter => "FLT",
        BlockRef::Folder => "FLD",
        BlockRef::VoiceFx => "FX",
        BlockRef::Env(s) => ["E1", "E2", "E3"][s.index()],
        BlockRef::Lfo(s) => ["LF1", "LF2", "LF3"][s.index()],
        BlockRef::Out => "OUT",
        BlockRef::Pitch => "PIT",
        BlockRef::Chorus => "CHR",
        BlockRef::Delay => "DLY",
        BlockRef::Reverb => "REV",
        BlockRef::Tape => "REL",
        BlockRef::Comp => "CMP",
        BlockRef::Part | BlockRef::PartMix(_) => "PRT",
        BlockRef::Theme => "THM",
        BlockRef::Midi => "MID",
        BlockRef::OrbitChords => "CHD",
        BlockRef::OrbitRing(r) => ["R1", "R2", "R3", "R4", "R5"][r.get() as usize],
        BlockRef::Tempo => "BPM",
    }
}

/// A TAPE destination's cell on `tape`'s machine (`WALK`, `WLK`).
fn machine_cell(a: ParamAddr, tape: TapeModel) -> Option<&'static machine::Cell> {
    (a.block == BlockRef::TapeEngine)
        .then(|| machine::cell_of(tape, a.param))
        .flatten()
}

/// A destination's parameter name: its spec label, or the machine's.
pub fn dest_name(d: &ModDest, tape: TapeModel) -> &'static str {
    machine_cell(d.addr, tape)
        .map(|c| c.label)
        .or_else(|| d.addr.spec().map(|s| s.label))
        .unwrap_or("?")
}

/// A column's header: the machine's short on TAPE, else `fit_header`.
fn header(d: &ModDest, tape: TapeModel) -> &'static str {
    match machine_cell(d.addr, tape) {
        Some(c) => c.short,
        None => d.addr.spec().map_or("?", fit_header),
    }
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
pub fn fmt_route_dest(buf: &mut FmtBuf, d: &ModDest, tape: TapeModel) {
    let _ = write!(buf, "{} {}", block_tag(d.addr.block), dest_name(d, tape));
}

/// A block's full name, for the readout (`FILTER`; `block_tag` is `FLT`).
pub fn block_name(b: BlockRef) -> &'static str {
    match b {
        BlockRef::Modal => "MODAL",
        BlockRef::TapeEngine => "TAPE",
        BlockRef::Va => "VA",
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
        BlockRef::VoiceFx => "FX",
        BlockRef::Env(s) => ["ENV 1", "ENV 2", "ENV 3"][s.index()],
        BlockRef::Lfo(s) => ["LFO 1", "LFO 2", "LFO 3"][s.index()],
        BlockRef::Out => "OUTPUT",
        // `VOICE PITCH`, not `PITCH PITCH`.
        BlockRef::Pitch => "VOICE",
        BlockRef::Chorus => "CHORUS",
        BlockRef::Delay => "DELAY",
        BlockRef::Reverb => "REVERB",
        BlockRef::Tape => "REEL",
        BlockRef::Comp => "COMP",
        BlockRef::Part | BlockRef::PartMix(_) => "PART",
        BlockRef::Theme => "THEME",
        BlockRef::Midi => "MIDI",
        BlockRef::OrbitChords => "ORBIT CHORDS",
        BlockRef::OrbitRing(r) => {
            ["RING 1", "RING 2", "RING 3", "RING 4", "RING 5"][r.get() as usize]
        }
        BlockRef::Tempo => "TEMPO",
    }
}

/// Readout line width: `source`, the arrow, `dest`.
fn names_width(source: &str, dest: &str) -> i32 {
    let w = |s| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
    w(source) + ARROW_GAP + 8 + ARROW_GAP + w(dest)
}
const ARROW_GAP: i32 = 5;

/// The readout's destination: `FILTER CUTOFF`, or `FLT CUTOFF` when the
/// full block name would push the line past `theme::CONTENT_W`. The param
/// name is never cut.
pub fn fmt_readout_dest(buf: &mut FmtBuf, source: &str, d: &ModDest, tape: TapeModel) {
    let _ = write!(buf, "{} {}", block_name(d.addr.block), dest_name(d, tape));
    if names_width(source, buf.as_str()) > theme::CONTENT_W {
        *buf = FmtBuf::new();
        fmt_route_dest(buf, d, tape);
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
        crate::block::OffsetLaw::Octaves(n) => crate::ui::fmt::fixed(buf, off * n, 1, " oct"),
        crate::block::OffsetLaw::Semitones(_) if amount == 0 => {
            let _ = buf.write_str("0 st");
        }
        crate::block::OffsetLaw::Semitones(n) => crate::ui::fmt::fixed(buf, off * n, 1, " st"),
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

/// Top-left of the cell box at (visible column `ci`, visible row `vi`) in
/// a grid of `rows`.
pub fn cell_origin(ci: usize, vi: usize, rows: usize) -> (i32, i32) {
    (
        GRID_X + ci as i32 * GRID_COL_W + CELL_INSET,
        GRID_ROW0_Y + vi as i32 * row_h(rows) + (row_h(rows) - cell_h(rows)) / 2,
    )
}

/// Cell box width.
pub const CELL_W: i32 = GRID_COL_W - 2 * CELL_INSET;

/// Centre of the cell at (visible column `ci`, visible row `vi`).
pub fn cell_center(ci: usize, vi: usize, rows: usize) -> (i32, i32) {
    let (x, y) = cell_origin(ci, vi, rows);
    (x + CELL_W / 2, y + cell_h(rows) / 2)
}

/// The amount grid: sources down (on ALGO, then OP5 and OP6, whose cells
/// outside the fixed four are X cells), primed destinations across. A route's
/// cell is lit and prints its amount (`+60`, `-30`, `0` in the rest grey);
/// an absent route is an empty outline. The cursor's cell is outlined in
/// the accent and prints `sel_amount`, the lerped amount. A column in
/// `inert` (`inert_dests`) reads as a dimmed cell does: its name and
/// amounts in MID, its routes unlit.
pub fn draw_grid<D>(d: &mut D, state: &MatrixState, sel_amount: i8, inert: u32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let cols = state.visible_cols();
    let col_x = |ci: usize| GRID_X + ci as i32 * GRID_COL_W + GRID_COL_W / 2;
    for ci in 0..cols {
        let di = ci + state.scroll_x;
        let Some(dest) = state.col_dest(di) else {
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
        let name = match state.col(di) {
            Col::Fixed(f) => f.tag(),
            Col::Reg(_) => header(&dest, state.tape),
        };
        draw::text_center(d, &theme::FONT_LABEL, name, x, GRID_NAME_Y, name_color, 0);
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
    if state.num_cols() > state.scroll_x + cols {
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
    let rows = state.rows();
    for row in 0..rows {
        let src = state.row_source(row);
        let si = src.index();
        let (_, y) = cell_origin(0, row, rows);
        let name = match src {
            RowSource::Mod(_) => state.sources[si].map_or("?", |s| s.name),
            RowSource::Tap(t) => t.tag(),
        };
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
            y + text_dy(rows),
            color,
        );
        for ci in 0..cols {
            let di = ci + state.scroll_x;
            if di >= state.num_cols() {
                break;
            }
            let selected = row == state.sel_row && di == state.sel_col;
            if state.is_x(row, di) {
                draw_x_cell(d, ci, row, rows, selected);
                continue;
            }
            let amount = if selected {
                sel_amount
            } else {
                state.amount(si, di)
            };
            draw_cell(
                d,
                (ci, row, rows),
                state.is_present(si, di).then_some(amount),
                selected,
                inert & (1 << di) != 0,
            );
        }
    }
}

/// An X cell: its mark alone, outlined in the accent under the cursor.
fn draw_x_cell<D>(d: &mut D, ci: usize, vi: usize, rows: usize, selected: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x, y) = cell_origin(ci, vi, rows);
    if selected {
        draw::round_outline(d, x, y, CELL_W, cell_h(rows), 0, theme::ACCENT);
    }
    let (cx, cy) = cell_center(ci, vi, rows);
    draw::x_mark(d, cx, cy);
}

/// One cell at (visible column, visible row, rows): an outline (the
/// accent under the cursor); a route fills it and prints its amount,
/// unlit in MID when `inert`.
fn draw_cell<D>(
    d: &mut D,
    (ci, vi, rows): (usize, usize, usize),
    route: Option<i8>,
    selected: bool,
    inert: bool,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let (x, y) = cell_origin(ci, vi, rows);
    let cell_h = cell_h(rows);
    let edge = if selected {
        theme::ACCENT
    } else {
        theme::FAINT
    };
    draw::round_outline(d, x, y, CELL_W, cell_h, 0, edge);
    let Some(amount) = route else { return };
    let text = if inert {
        theme::MID
    } else if amount == 0 {
        theme::BAR_REST
    } else {
        draw::fill_rect(d, x + 1, y + 1, CELL_W - 2, cell_h - 2, theme::ACCENT_SOFT);
        theme::ACCENT
    };
    let mut buf = FmtBuf::new();
    fmt_amount(&mut buf, amount);
    draw::text_center(
        d,
        &theme::FONT_LABEL_BOLD,
        buf.as_str(),
        x + CELL_W / 2,
        y + text_dy(rows),
        text,
        0,
    );
}

/// The band under the grid: the cursor's route by full names (`ENV 3 →
/// FILTER CUTOFF`), then its lerped amount and effect (`+42 = +3.3 oct`,
/// `--` with no route; in MID and tagged `INERT` on an `inert` column), its
/// gesture `hint` (`MatrixState::hint`, or none with HINTS off) and the
/// route count. `NO DESTINATIONS` without one.
pub fn draw_readout<D>(d: &mut D, state: &MatrixState, sel_amount: i8, inert: u32, hint: Hint)
where
    D: DrawTarget<Color = Rgb565>,
{
    let dest = state.col_dest(state.sel_col);
    let si = state.sel_source();
    let src = match state.row_source(state.sel_row) {
        RowSource::Mod(i) => crate::modulation::ModSource::ALL
            .get(i)
            .filter(|_| i < state.num_sources)
            .map(|s| s.name()),
        RowSource::Tap(t) => Some(t.name()),
    };
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
        // An X cell names what its tap reaches, not the column it sits in.
        (Some(src), Some(_)) if state.is_x(state.sel_row, state.sel_col) => {
            let x = label(d, src, theme::MARGIN_X, theme::ACCENT) + ARROW_GAP;
            let x = x + draw::arrow(d, x, READOUT_Y, theme::ACCENT) + ARROW_GAP;
            label(d, X_REACH, x, theme::ACCENT);
            draw::text_tracked(
                d,
                &theme::FONT_LABEL,
                X_NOT,
                theme::MARGIN_X,
                AMOUNT_Y,
                theme::MID,
                theme::LABEL_TRACKING,
            );
        }
        (Some(src), Some(dest)) => {
            let x = label(d, src, theme::MARGIN_X, theme::ACCENT) + ARROW_GAP;
            let x = x + draw::arrow(d, x, READOUT_Y, theme::ACCENT) + ARROW_GAP;
            let mut name = FmtBuf::new();
            fmt_readout_dest(&mut name, src, &dest, state.tape);
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
    hint::draw(d, hint, theme::MARGIN_X, HINT_Y);
    let routes = (0..state.rows())
        .map(|r| state.row_source(r).index())
        .flat_map(|r| (0..state.num_cols()).map(move |c| (r, c)))
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

/// Bit `c`: grid column `c` is a param `view::dimmed` rules inapplicable
/// (MORPH while ALG A = ALG B), so its routes do nothing.
pub fn inert_dests(state: &MatrixState, sound: &crate::preset::Sound) -> u32 {
    (0..state.num_cols()).fold(0, |m, c| match state.col_dest(c) {
        Some(d) if crate::ui::view::dimmed(d.addr, sound) => m | 1 << c,
        _ => m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::modal::{ModalParams, ResonatorMode};
    use crate::mod_path::ModDestRegistry;
    use crate::params::{EngineType, OutParams};
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
        let space = ParamAddr::new(BlockRef::Out, OutParams::SEND3);
        assert!(!dimmed(space, &s));

        let mut reg = ModDestRegistry::new();
        reg.add(modal(ModalParams::STRUCTURE), *b"MDLSTRUC")
            .unwrap();
        reg.add(modal(ModalParams::DAMP), *b"MDLDAMP\0").unwrap();
        let mut m = MatrixState::new();
        m.rebuild_dests_from_registry(&reg);
        assert_eq!(
            inert_dests(&m, &s),
            0b1 << FIXED_COLS,
            "STRUCTURE, the first registry column"
        );
        s.params.modal.mode = ResonatorMode::String;
        assert_eq!(inert_dests(&m, &s), 0);
    }
}
