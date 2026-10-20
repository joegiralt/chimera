//! Dirty region tracking for partial display updates.
//!
//! Each PageLayout defines screen regions with data snapshots.
//! Only regions whose data changed get cleared, redrawn, and flushed.

use crate::dsp::clampf;
use crate::ui::PrimeStatus;
use crate::ui::animation::AnimatedValue;
use crate::ui::components::{HeaderStatus, Look};
use crate::ui::page::{PageId, PageKey, PageLayout};
use crate::ui::theme;

/// Quantize a float to u16 for cheap comparison. Range 0.0..65.0 → 0..65000.
pub fn quantize(f: f32) -> u16 {
    (clampf(f, 0.0, 65.0) * 1000.0) as u16
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

/// `key` and `keys` (printed-text hashes) folded to one word: equal only
/// when both are.
pub fn fold_keys(key: u64, keys: &[u32]) -> u32 {
    use core::hash::Hasher;
    let mut h = Fnv::new();
    h.write(&key.to_le_bytes());
    keys.iter().for_each(|k| h.write(&k.to_le_bytes()));
    h.get()
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
        /// The ENV title's TYPE suffix (`renderer::title_type`).
        title_type: u8,
        /// The Part's OUT, for its warning (`renderer::header_out`).
        out: u8,
        /// The load and the dots.
        status: HeaderStatus,
        /// ORBIT's `SCENE n`, one-based; 0 where the header reads no scene.
        scene: u8,
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
        /// What a still glyph draws from other params (`Gauge::outside_key`).
        gauge: u32,
        /// What the band shows from outside its slot (ORBIT's readings, a
        /// synced reverb cell's clock).
        text: u32,
        /// HINTS on: the band draws its hint.
        hints: bool,
        /// The mixer's jump hint shows on a bare cell (`view::mixer_hint`).
        mixer: bool,
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
        inert: u32,
    },
    Viz {
        page: PageKey,
        values: [u16; 6],
        /// Fingerprint of outside data the viz shows (live output).
        live: u32,
        /// A pending prime-status message drawn on the viz — BigViz pages
        /// only, which have no focus band to carry it (issue #21).
        status: Option<PrimeStatus>,
        /// What an ORBIT viz draws of the scene and the runner
        /// (`orbit::draw::viz_key`).
        orbit: u32,
        /// `viz::ModMark::key`: a moved ghost redraws its boxes alone.
        mark: u32,
        /// `lfo_glyph::Dot::key`: a moved playhead redraws its boxes alone.
        dot: u16,
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
        looks: u32,
        /// What a cell's text reads beyond its own value: the reverb
        /// panels' plate zone and its effective PRE-DLY, and the clock a
        /// synced cell reads.
        text: u64,
    },
    Nav {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        branch_scroll: u16,
        /// MODEL: MDL2's name on the map.
        model: u8,
        /// THEME's PAGES.
        pages: u8,
    },
    Grid {
        sel_row: u8,
        sel_col: u8,
        scroll_x: u8,
        /// The selected route's animated amount (quantized display value).
        sel_value: u16,
        matrix_rev: u16,
        /// `mod_grid::inert_dests`.
        inert: u32,
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
        title_type: u8,
        out: u8,
        status: HeaderStatus,
    ) -> Self {
        Self::Header {
            chain_idx,
            node_idx,
            sub_page,
            title_type,
            out,
            status,
            scene: 0,
        }
    }

    /// A header reading `SCENE n`: it redraws when a switch lands.
    pub fn with_scene(mut self, n: u8) -> Self {
        if let Self::Header { scene, .. } = &mut self {
            *scene = n;
        }
        self
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
            gauge: 0,
            text: 0,
            hints: false,
            mixer: false,
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

    /// A focus band keyed on what its glyph reads from other params.
    pub fn with_gauge(mut self, key: u32) -> Self {
        if let Self::Focus { gauge, .. } = &mut self {
            *gauge = key;
        }
        self
    }

    /// A focus band keyed on the text it shows from outside its slot,
    /// and on HINTS, too.
    pub fn with_text(mut self, key: u32, hints_on: bool, in_mixer: bool) -> Self {
        if let Self::Focus {
            text, hints, mixer, ..
        } = &mut self
        {
            *text = key;
            *hints = hints_on;
            *mixer = in_mixer;
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
            orbit: 0,
            mark: 0,
            dot: 0,
        }
    }

    /// This key with no mark share: equal when only the mark moved.
    pub fn without_mark(self) -> Self {
        self.with_mark(0)
    }

    /// A viz keyed on its modulation mark too.
    pub fn with_mark(mut self, key: u32) -> Self {
        if let Self::Viz { mark, .. } = &mut self {
            *mark = key;
        }
        self
    }

    /// This key with no playhead share: equal when only the dot moved.
    pub fn without_dot(self) -> Self {
        self.with_dot(0)
    }

    /// A viz keyed on the LFO scope's playhead too.
    pub fn with_dot(mut self, key: u16) -> Self {
        if let Self::Viz { dot, .. } = &mut self {
            *dot = key;
        }
        self
    }

    /// The LFO playhead this key was drawn with.
    pub fn dot(&self) -> u16 {
        match self {
            Self::Viz { dot, .. } => *dot,
            _ => 0,
        }
    }

    /// A viz keyed on what it draws of ORBIT too.
    pub fn with_orbit(mut self, key: u32) -> Self {
        if let Self::Viz { orbit, .. } = &mut self {
            *orbit = key;
        }
        self
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
            text: 0,
        }
    }

    pub fn nav(
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        branch_scroll: u16,
        model: u8,
        pages: u8,
    ) -> Self {
        Self::Nav {
            chain_idx,
            node_idx,
            sub_page,
            branch_scroll,
            model,
            pages,
        }
    }

    pub fn sentinel_header() -> Self {
        Self::Header {
            chain_idx: 255,
            node_idx: 255,
            sub_page: 255,
            title_type: u8::MAX,
            out: u8::MAX,
            status: HeaderStatus {
                parts: None,
                sounding: false,
                load_pct: u8::MAX,
                orbit_dot: None,
            },
            scene: u8::MAX,
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
            gauge: u32::MAX,
            text: u32::MAX,
            hints: false,
            mixer: false,
        }
    }

    pub fn sentinel_viz() -> Self {
        Self::Viz {
            page: SENTINEL_PAGE,
            values: [SENTINEL; 6],
            live: u32::MAX,
            status: None,
            orbit: u32::MAX,
            mark: u32::MAX,
            dot: u16::MAX,
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
            looks: u32::MAX,
            text: u64::MAX,
        }
    }

    pub fn sentinel_nav() -> Self {
        Self::Nav {
            chain_idx: 255,
            node_idx: 255,
            sub_page: 255,
            branch_scroll: SENTINEL,
            model: 255,
            pages: 255,
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
            inert: u32::MAX,
        }
    }

    /// With the matrix's revision in the key, and `looks`: the cells' looks,
    /// or the matrix's inert columns. A deleted route, a dimmed cell or an
    /// inert column redraws.
    /// Cells keyed on `text` too: what their text reads beyond their
    /// values.
    pub fn with_text_key(self, key: u64) -> Self {
        match self {
            Self::Cells {
                page,
                values,
                focus,
                dest_count,
                mods,
                matrix_rev,
                looks,
                ..
            } => Self::Cells {
                page,
                values,
                focus,
                dest_count,
                mods,
                matrix_rev,
                looks,
                text: key,
            },
            other => other,
        }
    }

    pub fn keyed(self, matrix_rev: u16, looks: u32) -> Self {
        match self {
            Self::Cells {
                page,
                values,
                focus,
                dest_count,
                mods,
                text,
                ..
            } => Self::Cells {
                page,
                values,
                focus,
                dest_count,
                mods,
                matrix_rev,
                looks,
                text,
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
    /// THEME's HINTS, SYSTEM's PUSH ENCODERS and TEXT SIZE as the bands
    /// were last drawn: any band may draw a hint or scaled text, so a
    /// change redraws them all (`UiState::hints_moved`).
    pub drawn_hints: Option<(bool, bool, crate::ui::theme::TextSize)>,
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

/// FNV-1a, 32 bits wide, as a `Hasher`: every region key's hash.
#[derive(Clone, Copy, Debug)]
pub struct Fnv(u32);

impl Fnv {
    pub const fn new() -> Self {
        Fnv(0x811c_9dc5)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}

impl core::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0 as u64
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u32).wrapping_mul(0x0100_0193);
        }
    }
}

/// FNV-1a over `parts`: a SETTINGS band's key.
pub fn settings_key(parts: &[&[u8]]) -> u32 {
    let mut h = Fnv::new();
    parts
        .iter()
        .for_each(|p| core::hash::Hasher::write(&mut h, p));
    h.get()
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
            drawn_hints: None,
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
