//! Dirty region tracking for partial display updates.
//!
//! Each PageLayout defines screen regions with data snapshots.
//! Only regions whose data changed get cleared, redrawn, and flushed.

use crate::ui::page::{PageId, PageLayout};
use crate::ui::animation::AnimatedValue;

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

/// Data snapshot for a screen region. If current != previous, region is dirty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionData {
    Header {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        render_us: u32,
    },
    Viz {
        page: PageId,
        values: [u16; 6],
    },
    Params {
        page: PageId,
        values: [u16; 6],
    },
    Cells {
        page: PageId,
        values: [u16; 6],
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
    pub fn header(chain_idx: u8, node_idx: u8, sub_page: u8, render_us: u32) -> Self {
        Self::Header { chain_idx, node_idx, sub_page, render_us }
    }

    pub fn viz(page: PageId, values: [u16; 6]) -> Self {
        Self::Viz { page, values }
    }

    pub fn params(page: PageId, values: [u16; 6]) -> Self {
        Self::Params { page, values }
    }

    pub fn cells(page: PageId, values: [u16; 6], dest_count: u16) -> Self {
        Self::Cells { page, values, dest_count }
    }

    pub fn nav(chain_idx: u8, node_idx: u8, sub_page: u8, branch_scroll: u16) -> Self {
        Self::Nav { chain_idx, node_idx, sub_page, branch_scroll }
    }

    pub fn sentinel_header() -> Self {
        Self::Header { chain_idx: 255, node_idx: 255, sub_page: 255, render_us: u32::MAX }
    }

    pub fn sentinel_viz() -> Self {
        Self::Viz { page: PageId::Filter, values: [SENTINEL; 6] }
    }

    pub fn sentinel_params() -> Self {
        Self::Params { page: PageId::Filter, values: [SENTINEL; 6] }
    }

    pub fn sentinel_cells() -> Self {
        Self::Cells { page: PageId::Filter, values: [SENTINEL; 6], dest_count: u16::MAX }
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
    Viz,
    Params,
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
        match layout {
            PageLayout::BigViz => {
                self.count = 4;
                self.regions[0] = Region {
                    kind: RegionKind::Header, y_start: 0, y_end: 28,
                    prev_data: RegionData::sentinel_header(),
                };
                self.regions[1] = Region {
                    kind: RegionKind::Viz, y_start: 28, y_end: 170,
                    prev_data: RegionData::sentinel_viz(),
                };
                self.regions[2] = Region {
                    kind: RegionKind::Params, y_start: 170, y_end: 266,
                    prev_data: RegionData::sentinel_params(),
                };
                self.regions[3] = Region {
                    kind: RegionKind::Nav, y_start: 266, y_end: 320,
                    prev_data: RegionData::sentinel_nav(),
                };
            }
            PageLayout::CellGrid => {
                self.count = 3;
                self.regions[0] = Region {
                    kind: RegionKind::Header, y_start: 0, y_end: 28,
                    prev_data: RegionData::sentinel_header(),
                };
                self.regions[1] = Region {
                    kind: RegionKind::Cells, y_start: 28, y_end: 266,
                    prev_data: RegionData::sentinel_cells(),
                };
                self.regions[2] = Region {
                    kind: RegionKind::Nav, y_start: 266, y_end: 320,
                    prev_data: RegionData::sentinel_nav(),
                };
            }
            PageLayout::Matrix => {
                self.count = 2;
                self.regions[0] = Region {
                    kind: RegionKind::Grid, y_start: 0, y_end: 266,
                    prev_data: RegionData::sentinel_grid(),
                };
                self.regions[1] = Region {
                    kind: RegionKind::Nav, y_start: 266, y_end: 320,
                    prev_data: RegionData::sentinel_nav(),
                };
            }
        }
        self.prev_layout = Some(layout);
    }

    pub fn active_regions(&self) -> &[Region] {
        &self.regions[..self.count as usize]
    }

    pub fn active_regions_mut(&mut self) -> &mut [Region] {
        &mut self.regions[..self.count as usize]
    }
}
