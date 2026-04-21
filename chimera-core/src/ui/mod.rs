pub mod animation;
pub mod block_def;
pub mod block_registry;
pub mod cell;
pub mod chain;
pub mod dungeon_map;
pub mod fmt;
pub mod mod_grid;
pub mod page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod theme;

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

use crate::dsp::lfo::Lfo;
use crate::modulation::{ModState, MAX_MOD_SOURCES};
use crate::params::ParamSnapshot;
use chain::ChainNav;
use mod_grid::MatrixState;
use page::{PageId, PageLayout};
use perf::PerfStats;
use renderer::Renderer;

/// Top-level UI state. Owns navigation, parameters, and display animation.
/// Portable across desktop and hardware — only depends on HAL traits.
pub struct UiState {
    pub nav: ChainNav,
    pub params: ParamSnapshot,
    pub renderer: Renderer,
    pub matrix_state: MatrixState,
    pub mod_state: ModState,
    page: PageId,
    region_set: region::RegionSet,
    /// Last encoder touched (0-5) — used to identify focused param for MIX+Plus/Minus
    last_encoder: usize,
    /// Display-side LFO for animating modulated parameters
    display_lfo: Lfo,
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

        let mut matrix_state = MatrixState::new();
        // Build source + dest lists from the chain
        let chain = nav.active_chain();
        if let Some(last_block) = chain.blocks.last() {
            matrix_state.rebuild_sources(last_block.sub_pages);
        }
        matrix_state.rebuild_dests_from_chain(chain.blocks);

        Self {
            nav,
            params,
            renderer,
            matrix_state,
            mod_state: ModState::new(),
            page,
            region_set: region::RegionSet::new(),
            last_encoder: 0,
            display_lfo: Lfo::new(),
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
        if def.layout == PageLayout::Matrix {
            for (i, &enc) in encoder_ids.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta != 0 {
                    match i {
                        0 => self.matrix_state.move_row(delta),
                        1 => self.matrix_state.move_col(delta),
                        2 => self.matrix_state.scroll_v(delta),
                        3 => self.matrix_state.scroll_h(delta),
                        4 => {
                            self.matrix_state.adjust_amount(delta);
                            self.mod_state.sync_from_matrix(&self.matrix_state);
                        }
                        _ => {}
                    }
                }
            }
        } else {
            for (i, &enc) in encoder_ids.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta != 0 {
                    self.last_encoder = i;
                    self.renderer.focused = i;
                    if shift {
                        let fmt = def.params[i].format;
                        self.page.snap_encoder(i, delta, fmt, &mut self.params);
                    } else {
                        self.page.apply_encoder(i, delta, &mut self.params);
                    }
                }
            }

            // MIX + Plus/Minus: toggle mod destination for last-touched encoder param
            if shift {
                let block_idx = self.nav.node as u8;
                let param_idx = self.last_encoder as u8;
                let chain = self.nav.active_chain();
                if controls.button_state(ButtonId::Plus) == ButtonState::Pressed {
                    self.matrix_state.set_mod_enabled(block_idx, param_idx, true);
                    self.matrix_state.rebuild_dests_from_chain(chain.blocks);
                    self.mod_state.sync_from_matrix(&self.matrix_state);
                }
                if controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
                    self.matrix_state.set_mod_enabled(block_idx, param_idx, false);
                    self.matrix_state.rebuild_dests_from_chain(chain.blocks);
                    self.mod_state.sync_from_matrix(&self.matrix_state);
                }
            }
        }
    }

    /// Advance animations. Call at 30fps.
    pub fn update(&mut self) {
        // Tick the display-side LFO for visual modulation feedback.
        // LFO.process() advances phase by: rate / sample_rate * BLOCK_SIZE
        // We want phase to advance by: rate / ui_fps per call.
        // So: rate / sample_rate * BLOCK_SIZE = rate / ui_fps
        //     sample_rate = BLOCK_SIZE * ui_fps
        // Display-side LFO: advance phase by rate/fps per frame.
        // The audio LFO.process() uses rate/sample_rate*BLOCK_SIZE internally.
        // For the display we call once per UI frame. To get the same real-time rate,
        // pass sample_rate such that: rate/sr * BLOCK_SIZE = rate/fps
        // sr = BLOCK_SIZE * fps. At variable fps, assume ~30.
        // If animations look too slow/fast, this constant needs tuning.
        const UI_FPS: u32 = 20; // tuned to match audio-side LFO rate
        let lfo_val = self.display_lfo.process(&self.params.lfo, chimera_hal::BLOCK_SIZE as u32 * UI_FPS);

        // Read base param values
        let mut values = self.page.read_values(&self.params);

        // Apply mod offsets for display — makes bars and vizzes animate with modulation
        let block_idx = self.nav.node as u8;
        if self.mod_state.num_dests > 0 {
            let mut mod_sources = [0.0f32; MAX_MOD_SOURCES];
            // Source 0 = Envelope (use sustain level as approximation for display)
            if self.mod_state.num_sources > 0 {
                mod_sources[0] = self.params.envelopes[0].sustain.normalized();
            }
            // Source 1 = LFO
            if self.mod_state.num_sources > 1 {
                mod_sources[1] = lfo_val;
            }

            // Apply offsets to the 6 display values
            for i in 0..6 {
                let offset = self.mod_state.compute_offset(&mod_sources, block_idx, i as u8);
                if offset != 0.0 {
                    values[i] = (values[i] + offset).clamp(0.0, 1.0);
                }
            }
        }

        // Feed modulated values to the animator
        for (a, &v) in self.renderer.anim.iter_mut().zip(values.iter()) {
            a.set_target(v);
            a.update();
        }
    }

    /// Render full screen to a display.
    pub fn render<D>(&self, display: &mut D, perf: &PerfStats)
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        let def = self.nav.active_block_def();
        self.renderer.draw_with_def(display, &self.nav, def, perf, &self.matrix_state);
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
                RegionKind::Cells => RegionData::cells(self.page, qvalues, self.matrix_state.mod_enabled),
                RegionKind::Nav => RegionData::nav(nav_tag.0, nav_tag.1, nav_tag.2),
                RegionKind::Grid => RegionData::grid_with_amount(
                    self.matrix_state.sel_row as u8,
                    self.matrix_state.sel_col as u8,
                    self.matrix_state.scroll_x as u8,
                    self.matrix_state.scroll_y as u8,
                    self.matrix_state.current_amount(),
                ),
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
                RegionKind::Cells => RegionData::cells(self.page, qvalues, self.matrix_state.mod_enabled),
                RegionKind::Nav => RegionData::nav(nav_tag.0, nav_tag.1, nav_tag.2),
                RegionKind::Grid => RegionData::grid_with_amount(
                    self.matrix_state.sel_row as u8,
                    self.matrix_state.sel_col as u8,
                    self.matrix_state.scroll_x as u8,
                    self.matrix_state.scroll_y as u8,
                    self.matrix_state.current_amount(),
                ),
            };

            if current_data != r.prev_data {
                // Clear region via direct fb access
                let fb = display.pixel_buffer();
                renderer::Renderer::clear_region_fb(fb, r.y_start, r.y_end);

                // Draw region using BlockDef
                self.renderer.draw_region_with_def(display, r.kind, &self.nav, def, perf, &self.matrix_state);

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
