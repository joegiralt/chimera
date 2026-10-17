//! Direction A (ADR 0016): the single source of colours, fonts and layout.
//! Dark ground, warm greys, one accent for the active element.

use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::{FontRenderer, fonts};

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
/// A machine's part of the source under the TAPE strip's waveform.
pub const ACCENT_DIM: Rgb565 = ACCENT_SOFT;
/// A prompt's panel `#0d1514`.
pub const PANEL: Rgb565 = Rgb565::new(1, 5, 2);
/// An unpicked option's outline `#3a3937`.
pub const PILL_EDGE: Rgb565 = Rgb565::new(7, 14, 6);
/// Audio load above 60 % / 80 %.
pub const WARN: Rgb565 = Rgb565::new(31, 32, 0);
pub const ALERT: Rgb565 = Rgb565::new(31, 0, 0);

// --- Fonts (u8g2; only these are linked) ---

/// Focus value: Logisoso 42 px, ASCII (choices show as text: POLY, P1).
pub static FONT_FOCUS: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_logisoso42_tr>().with_ignore_unknown_chars(true);
/// Readout riding on a BigViz viz: Logisoso 20 px.
pub static FONT_READOUT: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_logisoso20_tr>().with_ignore_unknown_chars(true);
/// Values, header name, focus label, list names: Helvetica Bold 10.
pub static FONT_VALUE: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB10_tr>().with_ignore_unknown_chars(true);
/// Small uppercase labels: Helvetica 8.
pub static FONT_LABEL: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvR08_tr>().with_ignore_unknown_chars(true);
/// Map pill, header name: Helvetica Bold 8.
pub static FONT_LABEL_BOLD: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB08_tr>().with_ignore_unknown_chars(true);
/// Extra advance between uppercase label letters.
pub const LABEL_TRACKING: i32 = 1;

// --- Layout (240×320) ---

pub const SCREEN_W: i32 = 240;
pub const SCREEN_H: i32 = 320;
/// Left text margin.
pub const MARGIN_X: i32 = 12;
/// The screen less both margins: the widest a readout or hint line runs.
pub const CONTENT_W: i32 = SCREEN_W - 2 * MARGIN_X;

/// Header band 0..28.
pub const HEADER_BOTTOM: i32 = 28;
pub const HEADER_BASELINE: i32 = 20;
/// Sounding dot.
pub const HEADER_DOT_X: i32 = 225;
pub const HEADER_DOT_Y: i32 = 16;
pub const HEADER_DOT_R: i32 = 3;
/// ORBIT's beat dot, one dot left of the sounding dot.
pub const ORBIT_DOT_X: i32 = HEADER_DOT_X - 10;

/// Focus band 28..118: label, big value, arc gauge.
pub const FOCUS_BOTTOM: i32 = 118;
pub const FOCUS_LABEL_Y: i32 = 50;
pub const FOCUS_VALUE_X: i32 = 10;
pub const FOCUS_VALUE_Y: i32 = 104;
pub const ARC_CX: i32 = 188;
pub const ARC_CY: i32 = 80;
pub const ARC_R: i32 = 28;
pub const ARC_WIDTH: u32 = 5;
/// The SWITCH glyph's pill, on the arc's centre line, flush with the
/// right margin so a word has room before it.
pub const SWITCH_W: i32 = 56;
pub const SWITCH_H: i32 = 28;
pub const SWITCH_CX: i32 = SCREEN_W - MARGIN_X - SWITCH_W / 2;
/// The LEVEL BAR glyph: a slim vertical track as tall as the arc, near
/// the right margin, its tick dots to the left.
pub const LEVEL_X: i32 = SCREEN_W - MARGIN_X - 8;
pub const LEVEL_W: i32 = 6;
pub const LEVEL_TOP: i32 = ARC_CY - ARC_R;
pub const LEVEL_BOTTOM: i32 = ARC_CY + ARC_R;
pub const LEVEL_TICK_X: i32 = LEVEL_X - 14;
/// The CROSSFADER glyph: a horizontal track flush with the right margin,
/// a centre detent, and a cap taller than the track.
pub const XF_W: i32 = 64;
pub const XF_CX: i32 = SCREEN_W - MARGIN_X - XF_W / 2;
pub const XF_TRACK_H: i32 = 6;
pub const XF_CAP_W: i32 = 10;
pub const XF_CAP_H: i32 = 26;
/// VA's BLEND: the crossfader on a longer track, room for an end word
/// under each end (ADR 0129), on this baseline, at least `XF_ENDS_GAP`
/// apart.
pub const BLEND_W: i32 = 96;
pub const XF_ENDS_Y: i32 = ARC_CY + XF_CAP_H / 2 + 12;
pub const XF_ENDS_GAP: i32 = 6;
/// The STAFF glyph: three lines on the arc's centre line, flush with the
/// right margin, `STAFF_GAP` apart (a notehead's height); the note sits
/// at `STAFF_NOTE_X`, half a gap a step, its accidental to the left.
pub const STAFF_W: i32 = 60;
pub const STAFF_X: i32 = SCREEN_W - MARGIN_X - STAFF_W;
pub const STAFF_GAP: i32 = 8;
pub const STAFF_NOTE_X: i32 = STAFF_X + STAFF_W / 2 + 8;
/// The WAVE glyph's scope: a frame flush with the right margin on the
/// arc's centre line; the trace keeps `WAVE_PAD` px inside it.
pub const WAVE_W: i32 = 64;
pub const WAVE_H: i32 = 48;
pub const WAVE_X: i32 = SCREEN_W - MARGIN_X - WAVE_W;
pub const WAVE_Y: i32 = ARC_CY - WAVE_H / 2;
pub const WAVE_PAD: i32 = 5;
/// VA's MODE scope: the WAVE scope's frame, wider, for two periods.
pub const VA_MIX_W: i32 = 96;
/// The chorus braid's box, flush with the right margin, as tall as the
/// arc: the only pixels it redraws each frame.
pub const BRAID_W: i32 = 80;
pub const BRAID_H: i32 = 56;
pub const BRAID_X: i32 = SCREEN_W - MARGIN_X - BRAID_W;
pub const BRAID_Y: i32 = ARC_CY - BRAID_H / 2;
/// The delay rings' box, flush with the right margin, round about its
/// centre: the only pixels it redraws each frame.
pub const RINGS_W: i32 = 60;
pub const RINGS_H: i32 = 56;
pub const RINGS_X: i32 = SCREEN_W - MARGIN_X - RINGS_W;
pub const RINGS_Y: i32 = ARC_CY - RINGS_H / 2;
/// The reverb cube's box, flush with the right margin: the only pixels it
/// redraws each frame.
pub const CUBE_W: i32 = 64;
pub const CUBE_H: i32 = 56;
pub const CUBE_X: i32 = SCREEN_W - MARGIN_X - CUBE_W;
pub const CUBE_Y: i32 = ARC_CY - CUBE_H / 2;

/// Viz band on CellGrid / Mixer pages: 118..186, centre line 152.
pub const VIZ_BAND_TOP: i32 = 118;
pub const VIZ_BAND_BOTTOM: i32 = 186;
pub const VIZ_BAND_MID: i32 = 152;
pub const VIZ_BAND_AMP: i32 = 24;
/// Large viz on BigViz pages: 28..186.
pub const BIGVIZ_BOTTOM: i32 = 186;
/// Baseline of the prime-status line on BigViz pages: in the strip between
/// the header and the plot (`viz::PLOT_TOP`).
pub const BIGVIZ_STATUS_Y: i32 = 39;
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
/// THEME's PAGES ROW (ADR 0145): the nodes on a line at 279, the pill at
/// least `PILL_W` wide; the sub-pages on a row under a rule at 302,
/// between `ROW_L` and `ROW_R`, the arrows outside them.
pub const ROW_LINE_Y: i32 = 279;
pub const ROW_PILL_H: i32 = 18;
pub const ROW_PILL_PAD: i32 = 10;
pub const ROW_PILL_LABEL_Y: i32 = 283;
pub const ROW_NODE_R: i32 = 3;
pub const ROW_NODE_LABEL_Y: i32 = 296;
pub const ROW_RULE_Y: i32 = 302;
pub const ROW_Y: i32 = 314;
pub const ROW_L: i32 = 22;
pub const ROW_R: i32 = 218;
pub const ROW_GAP: i32 = 10;
pub const ROW_ARROW_L: i32 = 10;
pub const ROW_ARROW_R: i32 = 230;

/// SETTINGS (spec § Screens): breadcrumb, list and footer, in its own look.
/// The breadcrumb's width before it drops leading parts: it clears the
/// dots. With ORBIT built, always its beat dot too, so the crumbs never
/// move when it starts or stops.
pub const CRUMBS_W: i32 = CRUMBS_RIGHT - MARGIN_X;
/// Where the breadcrumb must end.
pub const CRUMBS_RIGHT: i32 = if cfg!(feature = "orbit") {
    ORBIT_DOT_X
} else {
    HEADER_DOT_X
} - HEADER_DOT_R
    - 4;
/// A list row's label, and the right edge of its note or `›`.
pub const LIST_TEXT_X: i32 = 18;
pub const LIST_RIGHT: i32 = 222;
/// The list's scrollbar.
pub const SCROLL_X: i32 = 236;
/// Footer, in the map's band: a rule, the project line, the key legend.
pub const FOOTER_RULE_Y: i32 = 280;
pub const FOOTER_RULE_W: i32 = VIZ_RIGHT - MARGIN_X;
pub const FOOTER_NAME_Y: i32 = 296;
pub const FOOTER_LEGEND_Y: i32 = 312;
