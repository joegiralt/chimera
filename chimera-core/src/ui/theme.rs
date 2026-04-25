use embedded_graphics::pixelcolor::Rgb565;

// --- Elektron / TE-inspired palette ---
// Black background, white text, single cyan accent, generous negative space.

pub const BG: Rgb565 = Rgb565::new(0, 0, 0);
pub const TEXT: Rgb565 = Rgb565::new(31, 63, 31);
pub const TEXT_DIM: Rgb565 = Rgb565::new(8, 16, 8); // 25% gray
pub const TEXT_MID: Rgb565 = Rgb565::new(14, 28, 14); // 45% gray

// Accent: cool cyan (Digitone-esque)
pub const ACCENT: Rgb565 = Rgb565::new(0, 56, 28); // bright cyan
pub const ACCENT_DIM: Rgb565 = Rgb565::new(0, 28, 14); // muted cyan
pub const ACCENT_BRIGHT: Rgb565 = Rgb565::new(4, 63, 31); // highlight cyan

// Dungeon map
pub const NODE_ACTIVE_BG: Rgb565 = Rgb565::new(0, 56, 28); // filled cyan
pub const NODE_ACTIVE_TEXT: Rgb565 = Rgb565::new(0, 0, 0);
pub const NODE_INACTIVE_BORDER: Rgb565 = Rgb565::new(8, 16, 8); // dim outline
pub const NODE_INACTIVE_TEXT: Rgb565 = Rgb565::new(10, 20, 10);
pub const NODE_CONNECTOR: Rgb565 = Rgb565::new(6, 12, 6); // subtle line
pub const BRANCH_MARKER: Rgb565 = Rgb565::new(0, 56, 28);
pub const BRANCH_TEXT: Rgb565 = Rgb565::new(10, 20, 10);
pub const BRANCH_TEXT_ACTIVE: Rgb565 = Rgb565::new(31, 63, 31);

// Parameters
pub const PARAM_LABEL: Rgb565 = Rgb565::new(10, 20, 10); // subdued
pub const PARAM_VALUE: Rgb565 = Rgb565::new(31, 63, 31); // crisp
pub const PARAM_BAR_BG: Rgb565 = Rgb565::new(3, 6, 3); // very subtle
pub const PARAM_BAR_FG: Rgb565 = Rgb565::new(0, 48, 24); // accent fill

// Visualization
pub const VIZ_LINE: Rgb565 = Rgb565::new(0, 56, 28);
pub const VIZ_FILL: Rgb565 = Rgb565::new(0, 12, 6); // subtle fill behind curves
pub const VIZ_GRID: Rgb565 = Rgb565::new(3, 6, 3); // barely-visible grid

// Structure
pub const SEPARATOR: Rgb565 = Rgb565::new(4, 8, 4);
pub const HEADER_LABEL: Rgb565 = Rgb565::new(14, 28, 14);

// --- Layout (240x320 screen) ---

/// Content zone: header + content + compact map
pub const ENCODER_ZONE_BOTTOM: i32 = 265;

/// Oscilloscope strip — between cells and dungeon map on CellGrid pages
pub const SCOPE_TOP: i32 = 240;
pub const SCOPE_HEIGHT: i32 = 26;
pub const SCOPE_BOTTOM: i32 = SCOPE_TOP + SCOPE_HEIGHT;

/// Dungeon map: compact bottom strip
pub const MAP_TOP: i32 = 266;

/// Header
pub const HEADER_Y: i32 = 6;

/// Visualization area — expanded with smaller map
pub const VIZ_TOP: i32 = 28;
pub const VIZ_BOTTOM: i32 = 170;
pub const VIZ_LEFT: i32 = 12;
pub const VIZ_RIGHT: i32 = 228;

/// Parameter display (3x2 grid, bottom of content zone)
pub const PARAM_TOP: i32 = 178;
pub const PARAM_ROW_HEIGHT: i32 = 30;
pub const PARAM_COL_WIDTH: i32 = 74; // 3 columns with margins
pub const PARAM_LEFT: i32 = 10; // left margin

/// Dungeon map node geometry — compact single row
pub const NODE_WIDTH: i32 = 30;
pub const NODE_HEIGHT: i32 = 14;
pub const NODE_GAP: i32 = 6;
pub const NODE_ROW_Y: i32 = 278;
pub const BRANCH_START_Y: i32 = 296;
pub const BRANCH_LINE_HEIGHT: i32 = 12;

/// Screen
pub const SCREEN_W: i32 = 240;
pub const SCREEN_H: i32 = 320;

/// Parameter bar dimensions
pub const BAR_WIDTH: i32 = 52;
pub const BAR_HEIGHT: i32 = 3;
