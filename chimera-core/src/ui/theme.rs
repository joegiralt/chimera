//! Direction A (ADR 0016): the single source of colours, fonts and layout.
//! Dark ground, warm greys, one accent for the active element.

use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::{fonts, FontRenderer};

// --- Palette (mockup hex → RGB565) ---

/// Ground `#0a0b0d`.
pub const BG: Rgb565 = Rgb565::new(1, 2, 1);
/// Primary text `#ecebe7`: names, the focus value.
pub const INK: Rgb565 = Rgb565::new(29, 58, 28);
/// Secondary text `#c9c7c1`: cell values, list names.
pub const INK2: Rgb565 = Rgb565::new(25, 49, 24);
/// Labels `#8b8a86`.
pub const MID: Rgb565 = Rgb565::new(17, 34, 16);
/// Resting bar fill `#6c6b67`.
pub const BAR_REST: Rgb565 = Rgb565::new(13, 26, 12);
/// Tracks, rules, empty slots `#2a2b2e`.
pub const FAINT: Rgb565 = Rgb565::new(5, 10, 5);
/// The one accent `#7fd4c8`: the active element only.
pub const ACCENT: Rgb565 = Rgb565::new(15, 53, 25);
/// Accent at 12 % over the ground: fill under a viz line.
pub const ACCENT_SOFT: Rgb565 = Rgb565::new(3, 8, 4);
/// Audio load above 60 % / 80 %.
pub const WARN: Rgb565 = Rgb565::new(31, 32, 0);
pub const ALERT: Rgb565 = Rgb565::new(31, 0, 0);

// --- Fonts (u8g2; only these are linked) ---

/// Focus value: Logisoso 42 px, ASCII (choices show as text: POLY, P1).
pub const FONT_FOCUS: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_logisoso42_tr>().with_ignore_unknown_chars(true);
/// Readout riding on a BigViz viz: Logisoso 20 px.
pub const FONT_READOUT: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_logisoso20_tr>().with_ignore_unknown_chars(true);
/// Values, header name, focus label, list names: Helvetica Bold 10.
pub const FONT_VALUE: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB10_tr>().with_ignore_unknown_chars(true);
/// Small uppercase labels: Helvetica 8.
pub const FONT_LABEL: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvR08_tr>().with_ignore_unknown_chars(true);
/// Map pill, header name: Helvetica Bold 8.
pub const FONT_LABEL_BOLD: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB08_tr>().with_ignore_unknown_chars(true);
/// Extra advance between uppercase label letters.
pub const LABEL_TRACKING: i32 = 1;

// --- Layout (240×320) ---

pub const SCREEN_W: i32 = 240;
pub const SCREEN_H: i32 = 320;
/// Left text margin.
pub const MARGIN_X: i32 = 12;

/// Header band 0..28.
pub const HEADER_BOTTOM: i32 = 28;
pub const HEADER_BASELINE: i32 = 20;
/// Sounding dot.
pub const HEADER_DOT_X: i32 = 225;
pub const HEADER_DOT_Y: i32 = 16;
pub const HEADER_DOT_R: i32 = 3;

/// Focus band 28..118: label, big value, arc gauge.
pub const FOCUS_BOTTOM: i32 = 118;
pub const FOCUS_LABEL_Y: i32 = 50;
pub const FOCUS_VALUE_X: i32 = 10;
pub const FOCUS_VALUE_Y: i32 = 104;
pub const ARC_CX: i32 = 188;
pub const ARC_CY: i32 = 80;
pub const ARC_R: i32 = 28;
pub const ARC_WIDTH: u32 = 5;

/// Viz band on CellGrid / Mixer pages: 118..186, centre line 152.
pub const VIZ_BAND_TOP: i32 = 118;
pub const VIZ_BAND_BOTTOM: i32 = 186;
pub const VIZ_BAND_MID: i32 = 152;
pub const VIZ_BAND_AMP: i32 = 24;
/// Large viz on BigViz pages: 28..186.
pub const BIGVIZ_BOTTOM: i32 = 186;
pub const VIZ_LEFT: i32 = 12;
pub const VIZ_RIGHT: i32 = 228;

/// Cells 186..266: 3×2, knob order a–f.
pub const CELLS_BOTTOM: i32 = 266;
/// Baseline of the first row's labels.
pub const CELL_LABEL_Y: i32 = 196;
pub const CELL_ROW_H: i32 = 36;
pub const CELL_COL_W: i32 = 74;
pub const CELL_VALUE_DY: i32 = 17;
pub const CELL_BAR_DY: i32 = 22;
pub const CELL_BAR_W: i32 = 62;
pub const CELL_BAR_H: i32 = 2;
/// Mod amount line under the bar (primed params only).
pub const CELL_MOD_DY: i32 = 26;

/// Map 266..320: nodes on a line, current block a pill.
pub const MAP_TOP: i32 = 266;
pub const MAP_LINE_Y: i32 = 286;
pub const MAP_X0: i32 = 24;
pub const MAP_X1: i32 = 216;
pub const PILL_W: i32 = 34;
pub const PILL_H: i32 = 20;
pub const PILL_LABEL_Y: i32 = 290;
pub const NODE_R: i32 = 4;
pub const NODE_LABEL_Y: i32 = 306;
/// Sub-page branch rows under the pill.
pub const BRANCH_START_Y: i32 = 300;
pub const BRANCH_LINE_HEIGHT: i32 = 10;
