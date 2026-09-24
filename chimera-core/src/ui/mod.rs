pub mod animation;
pub mod block_def;
pub mod block_registry;
pub mod cell;
pub mod chain;
pub mod dungeon_map;
pub mod fmt;
pub mod mod_grid;
pub mod page;
pub mod part_page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod theme;

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

use crate::block::Block;
use crate::dsp::lfo::Lfo;
use crate::addr::{BlockRef, Blocks, Op, ParamAddr};
use crate::mod_path::LABEL_LEN;
use crate::modulation::{ModState, MAX_MOD_SOURCES};
use crate::params::{EnvParams, ParamSnapshot};
use crate::preset::{Performance, SoundPool, POOL_SIZE};
use crate::scope::SCOPE_LEN;
use block_def::slot_addr;
use chain::{ChainId, ChainNav};
use mod_grid::MatrixState;
use block_def::BlockDef;
use page::{PageKey, PageLayout};
use perf::PerfStats;
use renderer::Renderer;

/// UI mode — Normal chain navigation vs overlay screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    SoundBrowser { part: usize, cursor: usize, scroll: usize },
}


/// Top-level UI state. Owns navigation, parameters, and display animation.
/// Portable across desktop and hardware — only depends on HAL traits.
pub struct UiState {
    pub nav: ChainNav,
    pub performance: Performance,
    /// Saved Sounds that can be loaded into a Part (not part of the Performance).
    pub pool: SoundPool,
    pub active_part: usize,
    pub renderer: Renderer,
    pub matrix_state: MatrixState,
    pub ui_mode: UiMode,
    page: PageKey,
    /// Selected FM operator — one global selection, as before (spec §5).
    sel_op: Op,
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
        let mut performance = Performance::new();
        let page = PageKey::from_nav(&nav, Op::A);
        let mut renderer = Renderer::new();
        renderer.snap_to_current(page_values(page, nav.active_block_def(), &performance.edit(0), Op::A));

        let mut ui = Self {
            nav,
            performance,
            pool: SoundPool::new(),
            active_part: 0,
            renderer,
            matrix_state: MatrixState::new(),
            ui_mode: UiMode::Normal,
            page,
            sel_op: Op::A,
            region_set: region::RegionSet::new(),
            last_encoder: 0,
            display_lfo: Lfo::new(),
        };
        ui.load_matrix(0);
        ui
    }

    /// Returns a reference to the active part's params.
    pub fn params(&self) -> &ParamSnapshot {
        &self.performance.parts[self.active_part].sound.params
    }

    /// Returns a mutable reference to the active part's params.
    pub fn params_mut(&mut self) -> &mut ParamSnapshot {
        &mut self.performance.parts[self.active_part].sound.params
    }

    /// Returns a reference to the active part's mod state.
    pub fn mod_state(&self) -> &ModState {
        &self.performance.parts[self.active_part].sound.mod_state
    }

    /// Returns a mutable reference to the active part's mod state.
    pub fn mod_state_mut(&mut self) -> &mut ModState {
        &mut self.performance.parts[self.active_part].sound.mod_state
    }

    /// Current page identity.
    pub fn page(&self) -> PageKey {
        self.page
    }

    /// The selected FM operator.
    pub fn selected_op(&self) -> Op {
        self.sel_op
    }

    /// Recompute the page identity and jump the display to its values.
    fn enter_page(&mut self) {
        self.page = PageKey::from_nav(&self.nav, self.sel_op);
        let values = page_values(self.page, self.nav.active_block_def(), &self.performance.edit(self.active_part), self.sel_op);
        self.renderer.snap_to_current(values);
    }

    /// Show Part `part`'s routing in the matrix: source rows from its
    /// Sound's chain (ENV, LFO — even while the Mixer chain is on screen),
    /// destinations from its registry, amounts from its `ModState`. Call
    /// whenever the edited Part or its Sound changes.
    fn load_matrix(&mut self, part: usize) {
        let sound = &self.performance.parts[part].sound;
        self.matrix_state.rebuild_sources(chain::chain_def_for(sound.chain_type).mod_sources);
        self.matrix_state.rebuild_dests_from_registry(&sound.dest_registry);
        self.matrix_state.load_amounts(&sound.mod_state);
    }

    /// Rebuild a part's audio-side `ModState` from the matrix.
    fn sync_mod_state(&mut self, part: usize) {
        let sound = &mut self.performance.parts[part].sound;
        sound.mod_state.sync_from_matrix(&self.matrix_state);
    }

    /// The address the focused encoder edits, if its slot is bound. System
    /// and Demo slots are `Legacy`, so priming there does nothing; Mixer
    /// params are bound but not modulatable, so the registry refuses them.
    fn current_param_addr(&self) -> Option<ParamAddr> {
        slot_addr(self.nav.active_block_def(), self.last_encoder, self.sel_op)
    }

    /// 8-byte matrix column label for a primed destination: `O<n> ` + spec
    /// label for FM operator params, else the page's short name (≤ 3 chars)
    /// + the slot label.
    fn mod_label(&self, addr: ParamAddr) -> [u8; LABEL_LEN] {
        let def = self.nav.active_block_def();
        let op_prefix;
        let (prefix, name): (&[u8], &str) = match addr.block {
            BlockRef::FmOp(op) => {
                op_prefix = [b'O', b'1' + op.index() as u8, b' '];
                (&op_prefix, addr.spec().map_or("", |s| s.label))
            }
            _ => {
                let short = def.short.as_bytes();
                (&short[..short.len().min(3)], def.params[self.last_encoder].label())
            }
        };
        let mut label = [0u8; LABEL_LEN];
        label[..prefix.len()].copy_from_slice(prefix);
        let rest = name.as_bytes();
        let rlen = rest.len().min(LABEL_LEN - prefix.len());
        label[prefix.len()..prefix.len() + rlen].copy_from_slice(&rest[..rlen]);
        label
    }

    /// Process one frame of input: navigation + encoder deltas.
    pub fn handle_input(&mut self, controls: &impl Controls) {
        // ── Sound Browser mode input ─────────────────────────────────
        if let UiMode::SoundBrowser { part, ref mut cursor, ref mut scroll } = self.ui_mode {
            let total = Renderer::BROWSER_TOTAL_ENTRIES;
            let visible = Renderer::BROWSER_VISIBLE_ROWS.min(total);

            // Encoder A or Main: scroll cursor
            let delta = controls.encoder_delta(EncoderId::Main)
                + controls.encoder_delta(EncoderId::A);
            if delta != 0 {
                let new_cursor = (*cursor as i32 + delta as i32)
                    .clamp(0, total as i32 - 1) as usize;
                *cursor = new_cursor;
                // Adjust scroll to keep cursor visible
                if new_cursor < *scroll {
                    *scroll = new_cursor;
                } else if new_cursor >= *scroll + visible {
                    *scroll = new_cursor + 1 - visible;
                }
            }

            // Edit button: confirm selection / load
            if controls.button_state(ButtonId::Edit) == ButtonState::Pressed {
                let sel_cursor = *cursor;
                let sel_part = part;
                if sel_cursor < POOL_SIZE {
                    // Load from pool — clone sound first to avoid borrow conflict
                    if let Some(sound) = self.pool.get(sel_cursor) {
                        let loaded = sound.clone();
                        self.performance.parts[sel_part].sound = loaded;
                        self.performance.parts[sel_part].loaded_from = Some(sel_cursor as u8);
                    }
                } else {
                    // Init entries: POOL_SIZE=Pizza, POOL_SIZE+1=Modal, POOL_SIZE+2=FM
                    let init_types = [
                        crate::preset::ChainType::PizzaPoly,
                        crate::preset::ChainType::Modal,
                        crate::preset::ChainType::Fm,
                    ];
                    let init_idx = sel_cursor - POOL_SIZE;
                    if init_idx < init_types.len() {
                        self.performance.parts[sel_part].load_init(init_types[init_idx]);
                    }
                }
                // Switch to the loaded part and return to normal mode
                self.active_part = sel_part;
                self.nav.chain_id = ChainId::Part(sel_part);
                self.nav.node = 0;
                self.nav.sub_page = 0;
                self.nav.chain_type = self.performance.parts[sel_part].sound.chain_type;
                self.load_matrix(sel_part);
                self.enter_page();
                self.ui_mode = UiMode::Normal;
                return;
            }

            // Seq button: save current part's sound into highlighted pool slot
            if controls.button_state(ButtonId::Seq) == ButtonState::Pressed {
                let save_cursor = *cursor;
                let save_part = part;
                if save_cursor < POOL_SIZE {
                    let sound = self.performance.parts[save_part].sound.clone();
                    self.pool.store(save_cursor, sound);
                }
                // Stay in browser mode so the user can see the saved slot
                return;
            }

            // Any B-button press: cancel browser
            let b_buttons = [
                ButtonId::B1, ButtonId::B2, ButtonId::B3,
                ButtonId::B4, ButtonId::B5, ButtonId::B6,
            ];
            for &btn in &b_buttons {
                if controls.button_state(btn) == ButtonState::Pressed {
                    self.ui_mode = UiMode::Normal;
                    return;
                }
            }

            // Menu button also cancels
            if controls.button_state(ButtonId::Menu) == ButtonState::Pressed {
                self.ui_mode = UiMode::Normal;
                return;
            }

            // Consume all other input — don't pass to normal handlers
            return;
        }

        // ── Normal mode ──────────────────────────────────────────────

        // Edit + B1-B6: open sound browser for that part
        let edit_held = matches!(
            controls.button_state(ButtonId::Edit),
            ButtonState::Pressed | ButtonState::Held
        );
        if edit_held {
            let b_buttons = [
                ButtonId::B1, ButtonId::B2, ButtonId::B3,
                ButtonId::B4, ButtonId::B5, ButtonId::B6,
            ];
            for (i, &btn) in b_buttons.iter().enumerate() {
                if controls.button_state(btn) == ButtonState::Pressed {
                    self.ui_mode = UiMode::SoundBrowser { part: i, cursor: 0, scroll: 0 };
                    return; // consume — don't pass to navigation
                }
            }
        }

        // Navigation
        let nav_changed = self.nav.handle_input(controls);
        if nav_changed {
            // B<n> and MIX + B<n> both select Part n for editing.
            if let ChainId::Part(i) | ChainId::Mixer(i) = self.nav.chain_id {
                self.active_part = i;
                self.nav.chain_type = self.performance.parts[i].sound.chain_type;
                self.load_matrix(i);
            }
            self.enter_page();
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
                            self.sync_mod_state(self.active_part);
                        }
                        _ => {}
                    }
                }
            }
        } else {
            let at = self.active_part;
            for (i, &enc) in encoder_ids.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta != 0 {
                    self.last_encoder = i;
                    self.renderer.focused = i;
                    let params = &mut self.performance.edit(at);
                    match (self.page, shift) {
                        (PageKey::Part { .. }, true) => part_page::snap_encoder(def, i, delta, params, self.sel_op),
                        (PageKey::Part { .. }, false) => part_page::apply_encoder(def, i, delta, params, &mut self.sel_op),
                        (PageKey::Legacy(p), true) => p.snap_encoder(i, delta, params),
                        (PageKey::Legacy(p), false) => p.apply_encoder(i, delta, params),
                    }
                }
            }
            // The operator selection is part of the page identity.
            self.page = PageKey::from_nav(&self.nav, self.sel_op);

            // MIX + Plus/Minus: prime/un-prime parameter for modulation
            if shift {
                let at = self.active_part;
                if controls.button_state(ButtonId::Plus) == ButtonState::Pressed {
                    // Unbound slots prime nothing; non-modulatable params are refused.
                    if let Some(addr) = self.current_param_addr() {
                        let label = self.mod_label(addr);
                        let _ = self.performance.parts[at].sound.dest_registry.add(addr, label);
                        self.matrix_state.rebuild_dests_from_registry(
                            &self.performance.parts[at].sound.dest_registry
                        );
                        self.sync_mod_state(at);
                    }
                }
                if controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
                    if let Some(addr) = self.current_param_addr() {
                        self.performance.parts[at].sound.dest_registry.remove(addr);
                        self.matrix_state.rebuild_dests_from_registry(
                            &self.performance.parts[at].sound.dest_registry
                        );
                        self.sync_mod_state(at);
                    }
                }
            }
        }
    }

    /// Advance animations. Call at UI_FPS (~20fps).
    pub fn update(&mut self) {

        let at = self.active_part;

        // Read base param values
        let def = self.nav.active_block_def();
        let mut values = page_values(self.page, def, &self.performance.edit(at), self.sel_op);
        let sound = &self.performance.parts[at].sound;

        // Apply mod offsets for display — makes bars and vizzes animate with modulation.
        // Skip the LFO tick entirely when no modulation is active.
        if sound.mod_state.num_dests() > 0 {
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
            let lfo_val = self.display_lfo.process(&sound.params.lfo, chimera_hal::BLOCK_SIZE as u32 * UI_FPS);

            let mut mod_sources = [0.0f32; MAX_MOD_SOURCES];
            // Source 0 = Envelope (use sustain level as approximation for display)
            if sound.mod_state.num_sources() > 0 {
                mod_sources[0] = sound.params.envelopes[0].normalized(EnvParams::SUSTAIN);
            }
            // Source 1 = LFO
            if sound.mod_state.num_sources() > 1 {
                mod_sources[1] = lfo_val;
            }

            // Apply offsets to the 6 display values
            for i in 0..6 {
                let offset = slot_addr(def, i, self.sel_op).map_or(0.0, |a| sound.mod_state.offset_for(a, &mod_sources));
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

        // Animate branch scroll for dungeon map sub-pages.
        // Ensure the active row's bottom edge (y + LINE_HEIGHT) is on screen.
        // scroll_px = max(0, BRANCH_START_Y + (sub_page+1)*LINE_HEIGHT - SCREEN_HEIGHT)
        let needed_bottom = theme::BRANCH_START_Y
            + (self.nav.sub_page as i32 + 1) * theme::BRANCH_LINE_HEIGHT;
        let overflow = needed_bottom - chimera_hal::SCREEN_HEIGHT as i32;
        let target_scroll = if overflow > 0 {
            overflow as f32 / theme::BRANCH_LINE_HEIGHT as f32
        } else {
            0.0
        };
        self.renderer.branch_scroll.set_target(target_scroll);
        self.renderer.branch_scroll.update();
        // Force nav region redraw while scroll is animating
    }

    /// Render full screen to a display, with live output from the scope buffer.
    pub fn render<D>(&self, display: &mut D, perf: &PerfStats)
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        let mut scope = [0.0f32; SCOPE_LEN];
        crate::scope::read_samples(&mut scope);
        self.render_with_scope(display, perf, &scope);
    }

    /// Render full screen with `scope` as the live output (tests pass a
    /// fixed buffer so screen goldens are deterministic).
    pub fn render_with_scope<D>(&self, display: &mut D, perf: &PerfStats, scope: &[f32; SCOPE_LEN])
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        if let UiMode::SoundBrowser { part, cursor, scroll } = self.ui_mode {
            Renderer::draw_sound_browser(display, &self.pool, part, cursor, scroll, self.performance.parts[part].sound.chain_type);
            return;
        }
        let def = self.nav.active_block_def();
        self.renderer.draw_with_def(display, &self.nav, def, perf, &self.matrix_state, self.sel_op, scope);
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
                RegionKind::Cells => RegionData::cells(self.page, qvalues, self.matrix_state.num_dests as u16),
                RegionKind::Nav => RegionData::nav(nav_tag.0, nav_tag.1, nav_tag.2, region::quantize(self.renderer.branch_scroll.current())),
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
        let mut scope = [0.0f32; SCOPE_LEN];
        crate::scope::read_samples(&mut scope);
        self.render_dirty_with_scope(display, perf, &scope)
    }

    /// `render_dirty` with `scope` as the live output.
    pub fn render_dirty_with_scope<D>(
        &mut self,
        display: &mut D,
        perf: &PerfStats,
        scope: &[f32; SCOPE_LEN],
    ) -> [(u16, u16); region::MAX_REGIONS]
    where
        D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>
            + chimera_hal::ChimeraDisplay,
    {
        use region::{RegionData, RegionKind};

        // Sound browser overlay — always full redraw, single flush region
        if let UiMode::SoundBrowser { part, cursor, scroll } = self.ui_mode {
            let fb = display.pixel_buffer();
            Renderer::clear_region_fb(fb, 0, chimera_hal::SCREEN_HEIGHT);
            Renderer::draw_sound_browser(display, &self.pool, part, cursor, scroll, self.performance.parts[part].sound.chain_type);
            // Invalidate region set so normal layout forces full rebuild on exit
            self.region_set.prev_layout = None;
            let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
            flush_list[0] = (0, chimera_hal::SCREEN_HEIGHT);
            return flush_list;
        }

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

        let sel_op = self.sel_op;
        for r in self.region_set.active_regions_mut() {
            let current_data = match r.kind {
                RegionKind::Header => RegionData::header(
                    nav_tag.0, nav_tag.1, nav_tag.2, perf.render_us,
                ),
                RegionKind::Viz => RegionData::viz(self.page, qvalues),
                RegionKind::Params => RegionData::params(self.page, qvalues),
                RegionKind::Cells => RegionData::cells(self.page, qvalues, self.matrix_state.num_dests as u16),
                RegionKind::Nav => RegionData::nav(nav_tag.0, nav_tag.1, nav_tag.2, region::quantize(self.renderer.branch_scroll.current())),
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
                self.renderer.draw_region_with_def(display, r.kind, &self.nav, def, perf, &self.matrix_state, sel_op);

                r.prev_data = current_data;
                flush_list[flush_count] = (r.y_start, r.y_end);
                flush_count += 1;
            }
        }

        // Scope strip — always redraws after regions (so regions can't overwrite it)
        if layout == PageLayout::CellGrid {
            let fb = display.pixel_buffer();
            renderer::Renderer::clear_region_fb(fb, theme::SCOPE_TOP as u16, theme::SCOPE_BOTTOM as u16);
            renderer::Renderer::draw_scope(display, scope);
            if flush_count < flush_list.len() {
                flush_list[flush_count] = (theme::SCOPE_TOP as u16, theme::SCOPE_BOTTOM as u16);
                flush_count += 1;
            }
        }

        flush_list
    }
}

/// Display values for `page`: Part pages through slot bindings, legacy pages
/// through `PageId`.
fn page_values(page: PageKey, def: &BlockDef, params: &impl Blocks, sel_op: Op) -> [f32; 6] {
    match page {
        PageKey::Part { .. } => part_page::read_values(def, params, sel_op),
        PageKey::Legacy(p) => p.read_values(params),
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
