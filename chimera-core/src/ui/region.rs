//! Dirty region tracking for partial display updates.
//!
//! Each PageLayout defines screen regions with data snapshots.
//! Only regions whose data changed get cleared, redrawn, and flushed.

use crate::ui::PrimeStatus;
use crate::ui::animation::AnimatedValue;
use crate::ui::components::Look;
use crate::ui::page::{PageId, PageKey, PageLayout};
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
const SENTINEL_PAGE: PageKey = PageKey::Legacy(PageId::System(u16::MAX));

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
        /// The ENV title's TYPE suffix (`renderer::title_type`).
        title_type: u8,
        /// The Part's OUT, for its warning (`renderer::header_out`).
        out: u8,
    },
    /// The focus band: which slot, its animated value, and any pending
    /// prime-status message (issue #21) shown in the value's place.
    Focus {
        page: PageKey,
        slot: u8,
        value: u16,
        /// The slot's look: it can change without a `matrix_rev` bump.
        look: Look,
        status: Option<PrimeStatus>,
        /// `glyph::anim_key`: the clock while the glyph animates.
        anim: u32,
        /// The slot's set value (quantized): a glyph may draw it, not `value`.
        set: u16,
    },
    /// The mod matrix readout: the selected route, its animated amount and
    /// the route count.
    Route {
        row: u8,
        col: u8,
        dests: u8,
        value: u16,
        /// `MatrixState::rev`: a deleted route redraws.
        matrix_rev: u16,
        /// `mod_grid::inert_dests`: it can change without a `matrix_rev` bump.
        inert: u16,
    },
    Viz {
        page: PageKey,
        values: [u16; 6],
        /// Fingerprint of outside data the viz shows (live output).
        live: u32,
        /// A pending prime-status message drawn on the viz — BigViz pages
        /// only, which have no focus band to carry it (issue #21).
        status: Option<PrimeStatus>,
    },
    Cells {
        page: PageKey,
        values: [u16; 6],
        focus: u8,
        dest_count: u16,
        /// Each cell's mod bar (its amount's f32 bits), which a Part switch
        /// can change alone.
        mods: [Option<u32>; 6],
        /// `MatrixState::rev`, and each cell's `Look` in two bits.
        matrix_rev: u16,
        looks: u16,
    },
    Nav {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        branch_scroll: u16,
        /// MODEL: MDL2's name on the map.
        model: u8,
    },
    Grid {
        sel_row: u8,
        sel_col: u8,
        scroll_x: u8,
        /// The selected route's animated amount (quantized display value).
        sel_value: u16,
        matrix_rev: u16,
        /// `mod_grid::inert_dests`.
        inert: u16,
    },
    /// A SETTINGS band (breadcrumb, list or footer): a fingerprint of what
    /// it shows.
    Settings { key: u32 },
    /// The prompt's panel: a fingerprint of its words and pick.
    Overlay { key: u32 },
}

impl RegionData {
    pub fn header(
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        load_pct: u8,
        sounding: bool,
        title_type: u8,
        out: u8,
    ) -> Self {
        Self::Header {
            chain_idx,
            node_idx,
            sub_page,
            load_pct,
            sounding,
            title_type,
            out,
        }
    }

    pub fn focus(
        page: PageKey,
        slot: u8,
        value: u16,
        look: Look,
        status: Option<PrimeStatus>,
    ) -> Self {
        Self::Focus {
            page,
            slot,
            value,
            look,
            status,
            anim: 0,
            set: 0,
        }
    }

    /// This key with no animation share: equal when only an animated
    /// glyph's frame moved.
    pub fn without_anim(self) -> Self {
        self.animated(0)
    }

    /// A focus band keyed on its slot's set value too.
    pub fn with_set(mut self, set: u16) -> Self {
        if let Self::Focus { set: s, .. } = &mut self {
            *s = set;
        }
        self
    }

    /// A focus band keyed on its glyph's `anim` too.
    pub fn animated(mut self, anim: u32) -> Self {
        if let Self::Focus { anim: a, .. } = &mut self {
            *a = anim;
        }
        self
    }

    pub fn viz(page: PageKey, values: [u16; 6], live: u32) -> Self {
        Self::viz_with_status(page, values, live, None)
    }

    /// A viz that also shows a prime-status line (BigViz pages).
    pub fn viz_with_status(
        page: PageKey,
        values: [u16; 6],
        live: u32,
        status: Option<PrimeStatus>,
    ) -> Self {
        Self::Viz {
            page,
            values,
            live,
            status,
        }
    }

    pub fn cells(
        page: PageKey,
        values: [u16; 6],
        focus: u8,
        dest_count: u16,
        mods: [Option<u32>; 6],
    ) -> Self {
        Self::Cells {
            page,
            values,
            focus,
            dest_count,
            mods,
            matrix_rev: 0,
            looks: 0,
        }
    }

    pub fn nav(chain_idx: u8, node_idx: u8, sub_page: u8, branch_scroll: u16, model: u8) -> Self {
        Self::Nav {
            chain_idx,
            node_idx,
            sub_page,
            branch_scroll,
            model,
        }
    }

    pub fn sentinel_header() -> Self {
        Self::Header {
            chain_idx: 255,
            node_idx: 255,
            sub_page: 255,
            load_pct: u8::MAX,
            sounding: false,
            title_type: u8::MAX,
            out: u8::MAX,
        }
    }

    pub fn sentinel_focus() -> Self {
        Self::Focus {
            page: SENTINEL_PAGE,
            slot: u8::MAX,
            value: SENTINEL,
            look: Look::Live,
            status: None,
            anim: u32::MAX,
            set: SENTINEL,
        }
    }

    pub fn sentinel_viz() -> Self {
        Self::Viz {
            page: SENTINEL_PAGE,
            values: [SENTINEL; 6],
            live: u32::MAX,
            status: None,
        }
    }

    pub fn sentinel_cells() -> Self {
        Self::Cells {
            page: SENTINEL_PAGE,
            values: [SENTINEL; 6],
            focus: u8::MAX,
            dest_count: u16::MAX,
            mods: [Some(u32::MAX); 6],
            matrix_rev: u16::MAX,
            looks: u16::MAX,
        }
    }

    pub fn sentinel_nav() -> Self {
        Self::Nav {
            chain_idx: 255,
            node_idx: 255,
            sub_page: 255,
            branch_scroll: SENTINEL,
            model: 255,
        }
    }

    pub fn grid(sel_row: u8, sel_col: u8, scroll_x: u8) -> Self {
        Self::grid_with_value(sel_row, sel_col, scroll_x, 0)
    }

    pub fn grid_with_value(sel_row: u8, sel_col: u8, scroll_x: u8, sel_value: u16) -> Self {
        Self::Grid {
            sel_row,
            sel_col,
            scroll_x,
            sel_value,
            matrix_rev: 0,
            inert: 0,
        }
    }

    /// The matrix readout; `keyed` adds the revision and inert columns.
    pub fn route(row: u8, col: u8, dests: u8, value: u16) -> Self {
        Self::Route {
            row,
            col,
            dests,
            value,
            matrix_rev: 0,
            inert: 0,
        }
    }

    pub fn sentinel_grid() -> Self {
        Self::Grid {
            sel_row: 255,
            sel_col: 255,
            scroll_x: 255,
            sel_value: SENTINEL,
            matrix_rev: u16::MAX,
            inert: u16::MAX,
        }
    }

    /// With the matrix's revision in the key, and `looks`: the cells' looks,
    /// or the matrix's inert columns. A deleted route, a dimmed cell or an
    /// inert column redraws.
    pub fn keyed(self, matrix_rev: u16, looks: u16) -> Self {
        match self {
            Self::Cells {
                page,
                values,
                focus,
                dest_count,
                mods,
                ..
            } => Self::Cells {
                page,
                values,
                focus,
                dest_count,
                mods,
                matrix_rev,
                looks,
            },
            Self::Grid {
                sel_row,
                sel_col,
                scroll_x,
                sel_value,
                ..
            } => Self::Grid {
                sel_row,
                sel_col,
                scroll_x,
                sel_value,
                matrix_rev,
                inert: looks,
            },
            Self::Route {
                row,
                col,
                dests,
                value,
                ..
            } => Self::Route {
                row,
                col,
                dests,
                value,
                matrix_rev,
                inert: looks,
            },
            other => other,
        }
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
    /// SETTINGS' header: the breadcrumb.
    Crumbs,
    List,
    /// SETTINGS' project line and key legend, in the map's band.
    Footer,
    /// A prompt's panel, over the bands beneath.
    Prompt,
}

/// A screen region with Y bounds and cached data.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub kind: RegionKind,
    pub y_start: u16,
    pub y_end: u16,
    pub prev_data: RegionData,
}

/// A Part page's five bands and a prompt over them.
pub const MAX_REGIONS: usize = 6;

/// Tracks the region list for the current page layout.
pub struct RegionSet {
    pub regions: [Region; MAX_REGIONS],
    pub count: u8,
    /// The layout built, and whether a prompt is over it; `None` rebuilds.
    pub prev_screen: Option<(Layout, bool)>,
}

/// Which bands a screen has: a page's, or SETTINGS' (a list, or a leaf of
/// a layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Page(PageLayout),
    Settings(Option<PageLayout>),
}

impl Layout {
    pub fn regions(self) -> &'static [(RegionKind, u16, u16)] {
        match self {
            Layout::Page(l) => layout_regions(l),
            Layout::Settings(leaf) => settings_regions(leaf),
        }
    }
}

impl From<PageLayout> for Layout {
    fn from(l: PageLayout) -> Self {
        Layout::Page(l)
    }
}

/// FNV-1a over `parts`: a SETTINGS band's key.
pub fn settings_key(parts: &[&[u8]]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in parts.iter().flat_map(|p| p.iter()) {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
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
            prev_screen: None,
        }
    }

    /// Rebuild the region list for a new layout. All regions start dirty (sentinel data).
    pub fn set_layout(&mut self, layout: impl Into<Layout>) {
        self.set_screen(layout.into(), false);
    }

    /// `layout`'s bands, then the prompt's panel over them when `prompt`.
    /// All start dirty.
    pub fn set_screen(&mut self, layout: Layout, prompt: bool) {
        let bands = layout.regions();
        let over = prompt.then_some(&PROMPT);
        for (r, &(kind, y_start, y_end)) in self.regions.iter_mut().zip(bands.iter().chain(over)) {
            *r = Region {
                kind,
                y_start,
                y_end,
                prev_data: sentinel(kind),
            };
        }
        self.count = (bands.len() + prompt as usize) as u8;
        self.prev_screen = Some((layout, prompt));
    }

    pub fn active_regions(&self) -> &[Region] {
        &self.regions[..self.count as usize]
    }

    pub fn active_regions_mut(&mut self) -> &mut [Region] {
        &mut self.regions[..self.count as usize]
    }
}

impl Default for RegionSet {
    fn default() -> Self {
        Self::new()
    }
}

use RegionKind as K;

const HEADER: u16 = theme::HEADER_BOTTOM as u16;
const FOCUS: u16 = theme::FOCUS_BOTTOM as u16;
const BAND: u16 = theme::VIZ_BAND_BOTTOM as u16;
const CELLS: u16 = theme::CELLS_BOTTOM as u16;
const SCREEN: u16 = theme::SCREEN_H as u16;
const BIG_VIZ_END: u16 = theme::BIGVIZ_BOTTOM as u16;
const MATRIX_GRID: u16 = crate::ui::mod_grid::GRID_BOTTOM as u16;

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
const BIG_VIZ: [(RegionKind, u16, u16); 4] = [
    (K::Header, 0, HEADER),
    (K::Viz, HEADER, BIG_VIZ_END),
    (K::Cells, BIG_VIZ_END, CELLS),
    (K::Nav, CELLS, SCREEN),
];
/// Mod matrix: header, amount grid, the selected route's readout, map.
const MATRIX: [(RegionKind, u16, u16); 4] = [
    (K::Header, 0, HEADER),
    (K::Grid, HEADER, MATRIX_GRID),
    (K::Focus, MATRIX_GRID, CELLS),
    (K::Nav, CELLS, SCREEN),
];

/// The bands of `layout`, top to bottom; they tile 0..320.
pub fn layout_regions(layout: PageLayout) -> &'static [(RegionKind, u16, u16)] {
    match layout {
        PageLayout::CellGrid => &CELL_GRID,
        PageLayout::BigViz => &BIG_VIZ,
        PageLayout::Matrix => &MATRIX,
    }
}

const FOOTER: u16 = theme::MAP_TOP as u16;
// A leaf's footer starts where its cells end.
const _: () = assert!(CELLS == FOOTER);

/// A SETTINGS list: breadcrumb, rows, footer.
const SETTINGS_LIST: [(RegionKind, u16, u16); 3] = [
    (K::Crumbs, 0, HEADER),
    (K::List, HEADER, FOOTER),
    (K::Footer, FOOTER, SCREEN),
];
const SETTINGS_CELL_GRID: [(RegionKind, u16, u16); 5] = [
    (K::Crumbs, 0, HEADER),
    (K::Focus, HEADER, FOCUS),
    (K::Viz, FOCUS, BAND),
    (K::Cells, BAND, CELLS),
    (K::Footer, FOOTER, SCREEN),
];
const SETTINGS_BIG_VIZ: [(RegionKind, u16, u16); 4] = [
    (K::Crumbs, 0, HEADER),
    (K::Viz, HEADER, BIG_VIZ_END),
    (K::Cells, BIG_VIZ_END, CELLS),
    (K::Footer, FOOTER, SCREEN),
];
const SETTINGS_MATRIX: [(RegionKind, u16, u16); 4] = [
    (K::Crumbs, 0, HEADER),
    (K::Grid, HEADER, MATRIX_GRID),
    (K::Focus, MATRIX_GRID, CELLS),
    (K::Footer, FOOTER, SCREEN),
];

/// A prompt's panel: over the List band, and whatever lies there.
pub const PROMPT: (RegionKind, u16, u16) = (K::Prompt, 60, 240);

// Every region set has room for a prompt over it.
const _: () = {
    let sets: [&[(RegionKind, u16, u16)]; 7] = [
        &CELL_GRID,
        &BIG_VIZ,
        &MATRIX,
        &SETTINGS_LIST,
        &SETTINGS_CELL_GRID,
        &SETTINGS_BIG_VIZ,
        &SETTINGS_MATRIX,
    ];
    let mut i = 0;
    while i < sets.len() {
        assert!(sets[i].len() < MAX_REGIONS);
        i += 1;
    }
};

/// SETTINGS' bands: a list (`None`) or a leaf page of `layout`. The
/// breadcrumb takes the header's place and the footer the map's, so a leaf
/// never shows the map.
pub fn settings_regions(leaf: Option<PageLayout>) -> &'static [(RegionKind, u16, u16)] {
    match leaf {
        None => &SETTINGS_LIST,
        Some(PageLayout::CellGrid) => &SETTINGS_CELL_GRID,
        Some(PageLayout::BigViz) => &SETTINGS_BIG_VIZ,
        Some(PageLayout::Matrix) => &SETTINGS_MATRIX,
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
        K::Crumbs | K::List | K::Footer => RegionData::Settings { key: u32::MAX },
        K::Prompt => RegionData::Overlay { key: u32::MAX },
    }
}
