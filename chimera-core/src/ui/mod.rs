pub mod animation;
pub mod block_def;
pub mod block_registry;
pub mod cell;
pub mod chain;
pub mod dungeon_map;
pub mod fmt;
pub mod page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod theme;

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

use crate::params::ParamSnapshot;
use chain::ChainNav;
use page::PageId;
use perf::PerfStats;
use renderer::Renderer;

/// Top-level UI state. Owns navigation, parameters, and display animation.
/// Portable across desktop and hardware — only depends on HAL traits.
pub struct UiState {
    pub nav: ChainNav,
    pub params: ParamSnapshot,
    pub renderer: Renderer,
    page: PageId,
    region_set: region::RegionSet,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        let nav = ChainNav::new();
        let params = ParamSnapshot::default();
        let page = PageId::from_nav(&nav);
        let mut renderer = Renderer::new();
        renderer.snap_to_current(page, &params);

        Self {
            nav,
            params,
            renderer,
            page,
            region_set: region::RegionSet::new(),
        }
    }

    /// Current page id.
    pub fn page(&self) -> PageId {
        self.page
    }

    /// Process one frame of input: navigation + encoder deltas.
    pub fn handle_input(&mut self, controls: &impl Controls) {
        // Navigation
        let nav_changed = self.nav.handle_input(controls);
        if nav_changed {
            self.page = PageId::from_nav(&self.nav);
            self.renderer.snap_to_current(self.page, &self.params);

            // Engine type is set by the Part's chain, not by page navigation.
            // For now, keep whatever engine was set at init (Pizza by default).
        }

        // Encoder deltas -> parameter changes
        let shift = matches!(
            controls.button_state(ButtonId::Mix),
            ButtonState::Pressed | ButtonState::Held
        );

        let encoder_ids = [
            EncoderId::A,
            EncoderId::B,
            EncoderId::C,
            EncoderId::D,
            EncoderId::E,
            EncoderId::F,
        ];
        let def = self.nav.active_block_def();
        for (i, &enc) in encoder_ids.iter().enumerate() {
            let delta = controls.encoder_delta(enc);
            if delta != 0 {
                if shift {
                    let fmt = def.params[i].format;
                    self.page.snap_encoder(i, delta, fmt, &mut self.params);
                } else {
                    self.page.apply_encoder(i, delta, &mut self.params);
                }
            }
        }
    }

    /// Advance animations. Call at 30fps.
    pub fn update(&mut self) {
        self.renderer.update(self.page, &self.params);
    }

    /// Render full screen to a display.
    pub fn render<D>(&self, display: &mut D, perf: &PerfStats)
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        let def = self.nav.active_block_def();
        self.renderer.draw_with_def(display, &self.nav, def, perf);
    }

    /// Prime the region set after an initial full render, so render_dirty
    /// won't redundantly redraw everything on the first call.
    pub fn prime_regions(&mut self, perf: &PerfStats) {
        use region::{RegionData, RegionKind};

        let def = self.nav.active_block_def();
        let layout = def.layout;
        self.region_set.set_layout(layout);
        let qvalues = region::quantize_values(&self.renderer.anim);
        let nav_tag = nav_tag(&self.nav);

        for r in self.region_set.active_regions_mut() {
            r.prev_data = match r.kind {
                RegionKind::Header => RegionData::header(
                    nav_tag.0, nav_tag.1, nav_tag.2, perf.render_us,
                ),
                RegionKind::Viz => RegionData::viz(self.page, qvalues),
                RegionKind::Params => RegionData::params(self.page, qvalues),
                RegionKind::Cells => RegionData::cells(self.page, qvalues),
                RegionKind::Nav => RegionData::nav(nav_tag.0, nav_tag.1, nav_tag.2),
            };
        }
    }

    /// Render only dirty regions. Returns list of (y_start, y_end) pairs to flush.
    /// Slots with (0, 0) are unused.
    pub fn render_dirty<D>(
        &mut self,
        display: &mut D,
        perf: &PerfStats,
    ) -> [(u16, u16); region::MAX_REGIONS]
    where
        D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>
            + chimera_hal::ChimeraDisplay,
    {
        use region::{RegionData, RegionKind};

        let def = self.nav.active_block_def();
        let layout = def.layout;
        let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
        let mut flush_count = 0;

        // Rebuild regions if layout changed
        if self.region_set.prev_layout != Some(layout) {
            self.region_set.set_layout(layout);
        }

        let qvalues = region::quantize_values(&self.renderer.anim);
        let nav_tag = nav_tag(&self.nav);

        for r in self.region_set.active_regions_mut() {
            let current_data = match r.kind {
                RegionKind::Header => RegionData::header(
                    nav_tag.0, nav_tag.1, nav_tag.2, perf.render_us,
                ),
                RegionKind::Viz => RegionData::viz(self.page, qvalues),
                RegionKind::Params => RegionData::params(self.page, qvalues),
                RegionKind::Cells => RegionData::cells(self.page, qvalues),
                RegionKind::Nav => RegionData::nav(nav_tag.0, nav_tag.1, nav_tag.2),
            };

            if current_data != r.prev_data {
                // Clear region via direct fb access
                let fb = display.pixel_buffer();
                renderer::Renderer::clear_region_fb(fb, r.y_start, r.y_end);

                // Draw region using BlockDef
                self.renderer.draw_region_with_def(display, r.kind, &self.nav, def, perf);

                r.prev_data = current_data;
                flush_list[flush_count] = (r.y_start, r.y_end);
                flush_count += 1;
            }
        }

        flush_list
    }
}

/// Encode ChainId + node + sub_page into (u8, u8, u8) for region snapshot.
/// The chain_idx byte encodes ChainId discriminant + index.
fn nav_tag(nav: &ChainNav) -> (u8, u8, u8) {
    use chain::ChainId;
    let chain_byte = match nav.chain_id {
        ChainId::Part(i) => i as u8,          // 0-5
        ChainId::Mixer(i) => 10 + i as u8,    // 10-15
        ChainId::System => 20,
        ChainId::Demo => 21,
    };
    (chain_byte, nav.node as u8, nav.sub_page as u8)
}
