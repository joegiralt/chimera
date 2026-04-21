//! Mod matrix grid renderer — draws the source×destination grid.
//! For now uses hardcoded demo data. Will be dynamic from chain later.

use embedded_graphics::Drawable;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, StyledDrawable};
use embedded_graphics::text::Text;

use crate::ui::theme;

/// Demo source labels (Y-axis rows)
const SOURCES: &[&str] = &[
    "LFO 1",
    "LFO 2",
    "Env 1",
    "Env 2",
    "Vel",
    "MW",
    "Note",
    "Rand",
    "AT",
    "Env 3",
    "LFO 3",
    "Expr",
];

/// Demo destination labels (X-axis columns) — two lines: block short + param
const DESTS: &[(&str, &str)] = &[
    ("PIZ", "SHP"),
    ("PIZ", "CRS"),
    ("PIZ", "LVL"),
    ("DRV", "AMT"),
    ("DRV", "TON"),
    ("FLT", "CUT"),
    ("FLT", "RES"),
    ("FLD", "FLD"),
    ("FLD", "SYM"),
    ("VCA", "LVL"),
];

/// Demo amounts — sparse. [source_idx][dest_idx], 0 = empty.
const DEMO_AMOUNTS: &[(usize, usize, i8)] = &[
    (0, 5, 64),   // LFO 1 → FLT.CUT +64
    (0, 7, 32),   // LFO 1 → FLD.FLD +32
    (2, 5, -40),  // Env 1 → FLT.CUT -40
    (2, 9, 100),  // Env 1 → VCA.LVL +100
    (3, 3, 50),   // Env 2 → DRV.AMT +50
    (4, 6, 80),   // Vel → FLT.RES +80
    (5, 5, 48),   // MW → FLT.CUT +48
    (6, 0, -20),  // Note → PIZ.SHP -20
    (7, 1, 30),   // Rand → PIZ.CRS +30
    (8, 4, -60),  // AT → DRV.TON -60
    (9, 8, 44),   // Env 3 → FLD.SYM +44
    (10, 2, 70),  // LFO 3 → PIZ.LVL +70
];

/// Grid geometry
const GRID_TOP: i32 = 28;        // below header
const GRID_LEFT: i32 = 0;        // left edge
const ROW_LABEL_W: i32 = 36;     // width for source labels
const COL_HEADER_H: i32 = 22;    // height for dest column headers (2 lines)
const CELL_W: i32 = 28;          // width per cell
const CELL_H: i32 = 14;          // height per cell
const GRID_BOTTOM: i32 = 265;    // above dungeon map

/// Calculated visible dimensions
const VISIBLE_COLS: usize = ((240 - ROW_LABEL_W) / CELL_W) as usize;
const VISIBLE_ROWS: usize = ((GRID_BOTTOM - GRID_TOP - COL_HEADER_H) / CELL_H) as usize;

/// Max sources and destinations for the amounts grid.
pub const MAX_SOURCES: usize = 16;
pub const MAX_DESTS: usize = 16;

/// State for the mod matrix grid — cursor, scroll, and mutable amounts.
#[derive(Clone, Debug)]
pub struct MatrixState {
    pub sel_row: usize,
    pub sel_col: usize,
    pub scroll_x: usize,
    pub scroll_y: usize,
    pub num_rows: usize,
    pub num_cols: usize,
    /// Modulation amounts: [source][dest], -127 to +127. 0 = no connection.
    pub amounts: [[i8; MAX_DESTS]; MAX_SOURCES],
}

impl MatrixState {
    pub fn new() -> Self {
        let mut state = Self {
            sel_row: 0,
            sel_col: 0,
            scroll_x: 0,
            scroll_y: 0,
            num_rows: SOURCES.len(),
            num_cols: DESTS.len(),
            amounts: [[0; MAX_DESTS]; MAX_SOURCES],
        };
        // Pre-fill with demo data
        for &(src, dst, amt) in DEMO_AMOUNTS {
            if src < MAX_SOURCES && dst < MAX_DESTS {
                state.amounts[src][dst] = amt;
            }
        }
        state
    }

    /// Get the amount at the current cursor position.
    pub fn current_amount(&self) -> i8 {
        self.amounts[self.sel_row][self.sel_col]
    }

    /// Adjust the amount at the current cursor position.
    pub fn adjust_amount(&mut self, delta: i8) {
        let current = self.amounts[self.sel_row][self.sel_col] as i16;
        let new = (current + delta as i16).clamp(-127, 127) as i8;
        self.amounts[self.sel_row][self.sel_col] = new;
    }

    pub fn move_row(&mut self, delta: i8) {
        let new = self.sel_row as i32 + delta as i32;
        self.sel_row = new.clamp(0, self.num_rows as i32 - 1) as usize;
        // Auto-scroll to keep cursor visible
        let vis = self.visible_rows();
        if self.sel_row < self.scroll_y {
            self.scroll_y = self.sel_row;
        } else if self.sel_row >= self.scroll_y + vis {
            self.scroll_y = self.sel_row + 1 - vis;
        }
    }

    pub fn move_col(&mut self, delta: i8) {
        let new = self.sel_col as i32 + delta as i32;
        self.sel_col = new.clamp(0, self.num_cols as i32 - 1) as usize;
        // Auto-scroll to keep cursor visible
        let vis = self.visible_cols();
        if self.sel_col < self.scroll_x {
            self.scroll_x = self.sel_col;
        } else if self.sel_col >= self.scroll_x + vis {
            self.scroll_x = self.sel_col + 1 - vis;
        }
    }

    pub fn scroll_v(&mut self, delta: i8) {
        let max = if self.num_rows > self.visible_rows() {
            self.num_rows - self.visible_rows()
        } else {
            0
        };
        let new = self.scroll_y as i32 + delta as i32;
        self.scroll_y = new.clamp(0, max as i32) as usize;
    }

    pub fn scroll_h(&mut self, delta: i8) {
        let max = if self.num_cols > self.visible_cols() {
            self.num_cols - self.visible_cols()
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

/// Draw the mod matrix grid in the content zone.
/// Reads cursor position and scroll from `MatrixState`.
pub fn draw_grid<D>(
    display: &mut D,
    state: &MatrixState,
) where
    D: DrawTarget<Color = Rgb565>,
{
    let sel_row = state.sel_row;
    let sel_col = state.sel_col;
    let scroll_y = state.scroll_y;
    let scroll_x = state.scroll_x;
    let dim = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
    let mid = MonoTextStyle::new(&FONT_6X10, theme::TEXT_MID);
    let bright = MonoTextStyle::new(&FONT_6X10, theme::PARAM_VALUE);
    let accent = MonoTextStyle::new(&FONT_6X10, theme::ACCENT);

    let visible_cols = state.visible_cols();
    let visible_rows = state.visible_rows();

    // ── Column headers (2-line: block short + param) ──
    for ci in 0..visible_cols {
        let di = ci + scroll_x;
        if di >= DESTS.len() { break; }
        let (block, param) = DESTS[di];
        let x = ROW_LABEL_W + ci as i32 * CELL_W + 2;
        let y = GRID_TOP;

        let style = if di == sel_col { accent } else { dim };
        let _ = Text::new(block, Point::new(x, y + 10), style).draw(display);
        let _ = Text::new(param, Point::new(x, y + 20), style).draw(display);
    }

    // ── Row labels + cells ──

    for vi in 0..visible_rows {
        let ri = vi + scroll_y;
        if ri >= SOURCES.len() { break; }
        let y = GRID_TOP + COL_HEADER_H + vi as i32 * CELL_H;

        // Row label
        let label_style = if ri == sel_row { accent } else { dim };
        // Truncate label to 5 chars
        let label = if SOURCES[ri].len() > 5 { &SOURCES[ri][..5] } else { SOURCES[ri] };
        let _ = Text::new(label, Point::new(GRID_LEFT + 2, y + 10), label_style).draw(display);

        // Cells
        for ci in 0..visible_cols {
            let di = ci + scroll_x;
            if di >= DESTS.len() { break; }
            let x = ROW_LABEL_W + ci as i32 * CELL_W;

            let is_selected = ri == sel_row && di == sel_col;

            // Read amount from mutable state
            let amt = state.amounts[ri][di];
            let amount = if amt != 0 { Some(amt) } else { None };

            // Cell background for selected
            if is_selected {
                let _ = Rectangle::new(
                    Point::new(x, y),
                    Size::new(CELL_W as u32 - 1, CELL_H as u32 - 1),
                )
                .draw_styled(
                    &PrimitiveStyle::with_fill(Rgb565::new(0, 8, 4)),
                    display,
                );
            }

            // Draw amount
            if let Some(amt) = amount {
                let text_style = if is_selected { bright } else { mid };
                // Format: +64, -40, etc.
                let mut buf = [0u8; 5];
                let s = format_amount(amt, &mut buf);
                let _ = Text::new(s, Point::new(x + 2, y + 10), text_style).draw(display);
            } else if is_selected {
                // Show cursor in empty selected cell
                let _ = Text::new("·", Point::new(x + 10, y + 10), dim).draw(display);
            }
        }
    }

    // ── Grid lines ──
    let grid_style = PrimitiveStyle::with_stroke(Rgb565::new(2, 4, 2), 1);

    // Horizontal line below column headers
    let header_line_y = GRID_TOP + COL_HEADER_H - 1;
    let _ = embedded_graphics::primitives::Line::new(
        Point::new(ROW_LABEL_W, header_line_y),
        Point::new(239, header_line_y),
    )
    .draw_styled(&grid_style, display);

    // Vertical line after row labels
    let _ = embedded_graphics::primitives::Line::new(
        Point::new(ROW_LABEL_W - 1, GRID_TOP),
        Point::new(ROW_LABEL_W - 1, GRID_BOTTOM),
    )
    .draw_styled(&grid_style, display);

    // ── Scroll indicator ──
    let max_col_scroll = if DESTS.len() > visible_cols { DESTS.len() - visible_cols } else { 0 };
    if max_col_scroll > 0 {
        let indicator_style = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        if scroll_x > 0 {
            let _ = Text::new("<", Point::new(ROW_LABEL_W - 8, GRID_BOTTOM - 2), indicator_style).draw(display);
        }
        if scroll_x < max_col_scroll {
            let _ = Text::new(">", Point::new(234, GRID_BOTTOM - 2), indicator_style).draw(display);
        }
    }

    // ── Stats line ──
    let mut stats_buf = [0u8; 32];
    let stats = format_stats(SOURCES.len(), DESTS.len(), visible_rows, visible_cols, &mut stats_buf);
    let _ = Text::new(stats, Point::new(4, GRID_BOTTOM - 2), dim).draw(display);
}

fn format_amount(amt: i8, buf: &mut [u8; 5]) -> &str {
    let negative = amt < 0;
    let abs = if negative { -(amt as i16) } else { amt as i16 } as u16;

    let mut pos = 0;
    if negative {
        buf[pos] = b'-';
    } else {
        buf[pos] = b'+';
    }
    pos += 1;

    if abs >= 100 {
        buf[pos] = b'0' + (abs / 100) as u8;
        pos += 1;
    }
    if abs >= 10 {
        buf[pos] = b'0' + ((abs / 10) % 10) as u8;
        pos += 1;
    }
    buf[pos] = b'0' + (abs % 10) as u8;
    pos += 1;

    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

fn format_stats(rows: usize, cols: usize, vis_r: usize, vis_c: usize, buf: &mut [u8; 32]) -> &str {
    // "12×10 (11×7 vis)"
    let mut pos = 0;

    pos += write_num(rows, &mut buf[pos..]);
    buf[pos] = b'x'; pos += 1;
    pos += write_num(cols, &mut buf[pos..]);
    buf[pos] = b' '; pos += 1;
    buf[pos] = b'('; pos += 1;
    pos += write_num(vis_r.min(rows), &mut buf[pos..]);
    buf[pos] = b'x'; pos += 1;
    pos += write_num(vis_c.min(cols), &mut buf[pos..]);
    buf[pos] = b' '; pos += 1;
    buf[pos] = b'v'; pos += 1;
    buf[pos] = b'i'; pos += 1;
    buf[pos] = b's'; pos += 1;
    buf[pos] = b')'; pos += 1;

    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}

fn write_num(n: usize, buf: &mut [u8]) -> usize {
    if n >= 10 {
        buf[0] = b'0' + (n / 10) as u8;
        buf[1] = b'0' + (n % 10) as u8;
        2
    } else {
        buf[0] = b'0' + n as u8;
        1
    }
}
