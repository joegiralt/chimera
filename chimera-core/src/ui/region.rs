//! Dirty region tracking for partial display updates.
//!
//! Each PageLayout defines screen regions with data snapshots.
//! Only regions whose data changed get cleared, redrawn, and flushed.

use crate::ui::page::{PageId, PageKey, PageLayout};
use crate::ui::animation::AnimatedValue;
use crate::ui::theme;

/// Quantize a float to u16 for cheap comparison. Range 0.0..65.0 → 0..65000.
pub fn quantize(f: f32) -> u16 {
    (f.clamp(0.0, 65.0) * 1000.0) as u16
}

/// Quantize 6 animated values into a [u16; 6] for snapshot comparison.
pub fn quantize_values(anim: &[AnimatedValue; 6]) -> [u16; 6] {
    [
        quantize(anim[0].current()),
        quantize(anim[1].current()),
        quantize(anim[2].current()),
        quantize(anim[3].current()),
        quantize(anim[4].current()),
        quantize(anim[5].current()),
    ]
}

/// Sentinel value that never matches real data — forces initial redraw.
const SENTINEL: u16 = u16::MAX;
/// Page used in sentinel snapshots (the SENTINEL values make them unequal).
const SENTINEL_PAGE: PageKey = PageKey::Legacy(PageId::System);

/// Data snapshot for a screen region. If current != previous, region is dirty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionData {
    Header {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        /// Audio load shown in the header (0 = not measured).
        load_pct: u8,
        sounding: bool,
    },
    /// The focus band: which slot, and its animated value.
    Focus {
        page: PageKey,
        slot: u8,
        value: u16,
    },
    /// The mod matrix focus band: the selected route and its animated amount.
    Route {
        row: u8,
        col: u8,
        dests: u8,
        value: u16,
    },
    Viz {
        page: PageKey,
        values: [u16; 6],
        /// Fingerprint of outside data the viz shows (live output).
        live: u32,
    },
    Cells {
        page: PageKey,
        values: [u16; 6],
        focus: u8,
        dest_count: u16,
    },
    Nav {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        branch_scroll: u16,
    },
    Grid {
        sel_row: u8,
        sel_col: u8,
        scroll_x: u8,
        scroll_y: u8,
        sel_amount: i8,
    },
}

impl RegionData {
    pub fn header(chain_idx: u8, node_idx: u8, sub_page: u8, load_pct: u8, sounding: bool) -> Self {
        Self::Header { chain_idx, node_idx, sub_page, load_pct, sounding }
    }

    pub fn focus(page: PageKey, slot: u8, value: u16) -> Self {
        Self::Focus { page, slot, value }
    }

    pub fn viz(page: PageKey, values: [u16; 6], live: u32) -> Self {
        Self::Viz { page, values, live }
    }

    pub fn cells(page: PageKey, values: [u16; 6], focus: u8, dest_count: u16) -> Self {
        Self::Cells { page, values, focus, dest_count }
    }

    pub fn nav(chain_idx: u8, node_idx: u8, sub_page: u8, branch_scroll: u16) -> Self {
        Self::Nav { chain_idx, node_idx, sub_page, branch_scroll }
    }

    pub fn sentinel_header() -> Self {
        Self::Header { chain_idx: 255, node_idx: 255, sub_page: 255, load_pct: u8::MAX, sounding: false }
    }

    pub fn sentinel_focus() -> Self {
        Self::Focus { page: SENTINEL_PAGE, slot: u8::MAX, value: SENTINEL }
    }

    pub fn sentinel_viz() -> Self {
        Self::Viz { page: SENTINEL_PAGE, values: [SENTINEL; 6], live: u32::MAX }
    }

    pub fn sentinel_cells() -> Self {
        Self::Cells { page: SENTINEL_PAGE, values: [SENTINEL; 6], focus: u8::MAX, dest_count: u16::MAX }
    }

    pub fn sentinel_nav() -> Self {
        Self::Nav { chain_idx: 255, node_idx: 255, sub_page: 255, branch_scroll: SENTINEL }
    }

    pub fn grid(sel_row: u8, sel_col: u8, scroll_x: u8, scroll_y: u8) -> Self {
        Self::Grid { sel_row, sel_col, scroll_x, scroll_y, sel_amount: 0 }
    }

    pub fn grid_with_amount(sel_row: u8, sel_col: u8, scroll_x: u8, scroll_y: u8, sel_amount: i8) -> Self {
        Self::Grid { sel_row, sel_col, scroll_x, scroll_y, sel_amount }
    }

    pub fn sentinel_grid() -> Self {
        Self::Grid { sel_row: 255, sel_col: 255, scroll_x: 255, scroll_y: 255, sel_amount: i8::MIN }
    }
}

/// Which draw method to dispatch for a region.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    Header,
    Focus,
    Viz,
    Cells,
    Nav,
    Grid,
}

/// A screen region with Y bounds and cached data.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub kind: RegionKind,
    pub y_start: u16,
    pub y_end: u16,
    pub prev_data: RegionData,
}

pub const MAX_REGIONS: usize = 5;

/// Tracks the region list for the current page layout.
pub struct RegionSet {
    pub regions: [Region; MAX_REGIONS],
    pub count: u8,
    pub prev_layout: Option<PageLayout>,
}

impl RegionSet {
    pub fn new() -> Self {
        Self {
            regions: [Region {
                kind: RegionKind::Header,
                y_start: 0,
                y_end: 0,
                prev_data: RegionData::sentinel_header(),
            }; MAX_REGIONS],
            count: 0,
            prev_layout: None,
        }
    }

    /// Rebuild the region list for a new layout. All regions start dirty (sentinel data).
    pub fn set_layout(&mut self, layout: PageLayout) {
        let bands = layout_regions(layout);
        for (r, &(kind, y_start, y_end)) in self.regions.iter_mut().zip(bands) {
            *r = Region { kind, y_start, y_end, prev_data: sentinel(kind) };
        }
        self.count = bands.len() as u8;
        self.prev_layout = Some(layout);
    }

    pub fn active_regions(&self) -> &[Region] {
        &self.regions[..self.count as usize]
    }

    pub fn active_regions_mut(&mut self) -> &mut [Region] {
        &mut self.regions[..self.count as usize]
    }
}

use RegionKind as K;

const HEADER: u16 = theme::HEADER_BOTTOM as u16;
const FOCUS: u16 = theme::FOCUS_BOTTOM as u16;
const BAND: u16 = theme::VIZ_BAND_BOTTOM as u16;
const CELLS: u16 = theme::CELLS_BOTTOM as u16;
const SCREEN: u16 = theme::SCREEN_H as u16;
const BIG_VIZ_END: u16 = theme::BIGVIZ_BOTTOM as u16;

/// CellGrid (UI refresh spec § Page types): header, focus band, viz band,
/// cells, map.
const CELL_GRID: [(RegionKind, u16, u16); 5] = [
    (K::Header, 0, HEADER),
    (K::Focus, HEADER, FOCUS),
    (K::Viz, FOCUS, BAND),
    (K::Cells, BAND, CELLS),
    (K::Nav, CELLS, SCREEN),
];
/// BigViz: header, large viz, cells, map.
const BIG_VIZ: [(RegionKind, u16, u16); 4] =
    [(K::Header, 0, HEADER), (K::Viz, HEADER, BIG_VIZ_END), (K::Cells, BIG_VIZ_END, CELLS), (K::Nav, CELLS, SCREEN)];
/// Mod matrix: header, the selected route, dot grid, map.
const MATRIX: [(RegionKind, u16, u16); 4] =
    [(K::Header, 0, HEADER), (K::Focus, HEADER, FOCUS), (K::Grid, FOCUS, CELLS), (K::Nav, CELLS, SCREEN)];

/// The bands of `layout`, top to bottom; they tile 0..320.
pub fn layout_regions(layout: PageLayout) -> &'static [(RegionKind, u16, u16)] {
    match layout {
        PageLayout::CellGrid => &CELL_GRID,
        PageLayout::BigViz => &BIG_VIZ,
        PageLayout::Matrix => &MATRIX,
    }
}

fn sentinel(kind: RegionKind) -> RegionData {
    match kind {
        K::Header => RegionData::sentinel_header(),
        K::Focus => RegionData::sentinel_focus(),
        K::Viz => RegionData::sentinel_viz(),
        K::Cells => RegionData::sentinel_cells(),
        K::Nav => RegionData::sentinel_nav(),
        K::Grid => RegionData::sentinel_grid(),
    }
}
