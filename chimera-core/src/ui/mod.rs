pub mod alg_layout;
pub mod animation;
pub mod audio_page;
pub mod block_def;
pub mod block_registry;
pub mod browser;
pub mod chain;
pub mod components;
pub mod draw;
pub mod dungeon_map;
pub mod fmt;
pub mod focus;
pub mod mod_grid;
pub mod page;
pub mod part_page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod theme;
pub mod theme_settings;
pub mod viz;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::{ALL_BUTTONS, ALL_ENCODERS, ButtonId, ButtonState, Controls, EncoderId};

use crate::addr::{BlockRef, Blocks, Op, ParamAddr};
use crate::block::Block;
use crate::dsp::lfo::Lfo;
use crate::in_place::{by_value, uninit_at};
use crate::mod_path::{LABEL_LEN, RegistryError};
use crate::modulation::{MAX_MOD_SOURCES, ModState};
use crate::params::{EnvParams, ParamSnapshot};
use crate::perf::load::AudioStats;
use crate::preset::{POOL_SIZE, PartEdit, Performance, SoundPool};
use crate::scope::SCOPE_LEN;
use block_def::BlockDef;
use block_def::VizType;
use block_def::slot_addr;
use chain::{ChainId, ChainNav};
use mod_grid::MatrixState;
use page::{PageKey, PageLayout};
use perf::PerfStats;
use renderer::Renderer;
use theme_settings::ThemeSettings;

/// CUTOFF: the destination the filter's route knobs view (spec § 6).
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, crate::params::FilterParams::CUTOFF);

/// MIX + turn on a route knob: the next of −127, 0, +127 that way.
fn snap_amount(a: i8, delta: i8) -> i8 {
    match (delta > 0, a) {
        (true, a) if a < 0 => 0,
        (true, _) => 127,
        (false, a) if a > 0 => 0,
        (false, _) => -127,
    }
}

/// UI mode — Normal chain navigation vs overlay screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiMode {
    Normal,
    SoundBrowser {
        part: usize,
        cursor: usize,
        scroll: usize,
    },
}

/// The outcome of the last MIX+PLUS attempt on a parameter page, shown in
/// the focus band in place of the value readout (on BigViz pages, which
/// have no focus band, as a line at the top of the viz) until the next encoder,
/// button or page change — no timer, matching the focus band's own rule
/// (issue #21).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimeStatus {
    /// The parameter was not yet a mod destination; it is now.
    Added,
    /// The parameter was already primed.
    AlreadyRouted,
    /// The parameter's spec is not modulatable (ADR 0010).
    NotModulatable,
    /// The registry is at `MAX_REGISTRY_DESTS`, the matrix capacity.
    Full,
}

impl PrimeStatus {
    /// The word(s) shown in the focus band.
    pub fn label(self) -> &'static str {
        match self {
            PrimeStatus::Added => "ADDED",
            PrimeStatus::AlreadyRouted => "ALREADY ROUTED",
            PrimeStatus::NotModulatable => "NOT MODULATABLE",
            PrimeStatus::Full => "MATRIX FULL",
        }
    }
}

impl From<RegistryError> for PrimeStatus {
    fn from(e: RegistryError) -> Self {
        match e {
            RegistryError::NotModulatable => PrimeStatus::NotModulatable,
            RegistryError::Full => PrimeStatus::Full,
        }
    }
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
    /// Set when the sound browser opens, its cursor/scroll moves, or a save
    /// changes the pool; `render_dirty_with_scope` redraws it only then, and
    /// clears the flag once flushed (#7).
    browser_dirty: bool,
    page: PageKey,
    /// Selected operator — one global selection, as before (spec §5).
    sel_op: Op,
    region_set: region::RegionSet,
    /// Last-touched slot per page: the focus band and MIX + Plus/Minus.
    focus: focus::FocusMemory,
    /// Display-side LFO for animating modulated parameters
    display_lfos: [Lfo; 3],
    /// The last MIX+PLUS outcome; `None` once retired (issue #21).
    prime_status: Option<PrimeStatus>,
    /// System › Theme. Not stored yet: every boot starts at the default.
    theme: ThemeSettings,
}

crate::in_place::field_list!(UiState => UiState {
    nav,
    performance,
    pool,
    active_part,
    renderer,
    matrix_state,
    ui_mode,
    browser_dirty,
    page,
    sel_op,
    region_set,
    focus,
    display_lfos,
    prime_status,
    theme,
});

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(Self::init_in_place) }
    }

    // In place so the ~27 KB state (the 21 KB sound pool) never passes
    // through the firmware's stack.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the pool is built in place,
        // then filled with the factory bank, every other field is written
        // once, and `performance` is written before it is borrowed, all
        // before `assume_init_mut`.
        unsafe {
            let nav = ChainNav::new();
            let page = PageKey::from_nav(&nav, Op::A);
            addr_of_mut!((*p).performance).write(Performance::new());
            let performance = &mut *addr_of_mut!((*p).performance);
            let mut renderer = Renderer::new();
            renderer.snap_to_current(page_values(
                page,
                nav.active_block_def(),
                &performance.edit(0),
                Op::A,
            ));
            addr_of_mut!((*p).nav).write(nav);
            let pool = SoundPool::init_in_place(uninit_at(addr_of_mut!((*p).pool)));
            crate::factory::load_factory(pool);
            addr_of_mut!((*p).active_part).write(0);
            addr_of_mut!((*p).renderer).write(renderer);
            addr_of_mut!((*p).matrix_state).write(MatrixState::new());
            addr_of_mut!((*p).ui_mode).write(UiMode::Normal);
            addr_of_mut!((*p).browser_dirty).write(false);
            addr_of_mut!((*p).page).write(page);
            addr_of_mut!((*p).sel_op).write(Op::A);
            addr_of_mut!((*p).region_set).write(region::RegionSet::new());
            addr_of_mut!((*p).focus).write(focus::FocusMemory::new());
            addr_of_mut!((*p).display_lfos).write([Lfo::new(); 3]);
            addr_of_mut!((*p).prime_status).write(None);
            addr_of_mut!((*p).theme).write(ThemeSettings::DEFAULT);
            let ui = slot.assume_init_mut();
            ui.load_matrix(0);
            ui
        }
    }

    /// The last MIX+PLUS outcome, shown in the focus band until the next
    /// encoder, button or page change (issue #21).
    pub fn prime_status(&self) -> Option<PrimeStatus> {
        self.prime_status
    }

    /// System › Theme as last edited; the display shell applies it.
    pub fn theme(&self) -> ThemeSettings {
        self.theme
    }

    /// The edited Part's blocks and the UI's own (THEME), as one `Blocks`.
    fn blocks(&mut self, part: usize) -> UiBlocks<'_> {
        UiBlocks {
            part: self.performance.edit(part),
            theme: &mut self.theme,
        }
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

    /// The slot the focus band shows on the current page: the last one
    /// turned there, slot a until then.
    pub fn focused_slot(&self) -> usize {
        self.focus.get(self.nav.active_block_def().id)
    }

    /// The selected operator.
    pub fn selected_op(&self) -> Op {
        self.sel_op
    }

    /// Recompute the page identity and jump the display to its values.
    fn enter_page(&mut self) {
        self.page = PageKey::from_nav(&self.nav, self.sel_op);
        let values = self.display_values();
        self.renderer.snap_to_current(values);
    }

    /// The six values the display animates toward: the page's slots, and on
    /// the mod matrix the selected route's amount in slot e.
    fn display_values(&mut self) -> [f32; 6] {
        let def = self.nav.active_block_def();
        let (page, sel_op) = (self.page, self.sel_op);
        let mut values = page_values(page, def, &self.blocks(self.active_part), sel_op);
        for (i, slot) in def.params.iter().enumerate() {
            if let block_def::SlotBinding::Route(src) = slot.binding {
                values[i] = renderer::amount_value(
                    self.matrix_state.route(src.index(), CUTOFF).unwrap_or(0),
                );
            }
        }
        if def.layout == PageLayout::Matrix {
            values[renderer::MATRIX_AMOUNT_SLOT] =
                renderer::amount_value(self.matrix_state.current_amount());
        }
        values
    }

    /// Show Part `part`'s routing in the matrix: source rows from its
    /// Sound's chain (ENV, LFO — even while the Mixer chain is on screen),
    /// destinations from its registry, amounts from its `ModState`. Call
    /// whenever the edited Part or its Sound changes.
    fn load_matrix(&mut self, part: usize) {
        let sound = &self.performance.parts[part].sound;
        self.matrix_state
            .rebuild_sources(chain::chain_def_for(sound.engine()).mod_sources);
        self.matrix_state
            .rebuild_dests_from_registry(&sound.dest_registry);
        self.matrix_state.load_amounts(&sound.mod_state);
        // The cursor may be left over from a Part with more destinations
        // than this one (issue #11).
        self.matrix_state.clamp_cursor();
    }

    /// A route knob turned: create CUTOFF's column if needed (MATRIX FULL
    /// when there is no room), then set `source → CUTOFF` to `f(amount)`.
    fn edit_route(&mut self, source: crate::modulation::ModSource, f: impl FnOnce(i8) -> i8) {
        let at = self.active_part;
        let sound = &mut self.performance.parts[at].sound;
        if !sound.dest_registry.is_primed(CUTOFF) {
            if let Err(e) = sound.dest_registry.add(CUTOFF, *b"FLTCUTOF") {
                self.prime_status = Some(e.into());
                return;
            }
            self.matrix_state
                .rebuild_dests_from_registry(&sound.dest_registry);
            self.matrix_state.load_amounts(&sound.mod_state);
        }
        if let Some(col) = self.matrix_state.col_of(CUTOFF) {
            let row = source.index();
            let a = self.matrix_state.amounts[row][col];
            self.matrix_state.set(row, col, f(a));
            self.sync_mod_state(at);
        }
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
        slot_addr(
            self.nav.active_block_def(),
            self.focused_slot(),
            self.sel_op,
        )
    }

    /// 8-byte matrix column label for a primed destination: `O<n> ` + spec
    /// label for operator params, else the address's block tag (≤ 3 chars,
    /// `mod_grid::block_tag` — the page can be a sub-page with a different
    /// short name, e.g. FLT › MODE for the filter's DRIVE) + the slot label.
    fn mod_label(&self, addr: ParamAddr) -> [u8; LABEL_LEN] {
        let def = self.nav.active_block_def();
        let op_prefix;
        let (prefix, name): (&[u8], &str) = match addr.block {
            BlockRef::AlgoOp(op) => {
                op_prefix = [b'O', b'1' + op.index() as u8, b' '];
                (&op_prefix, addr.spec().map_or("", |s| s.label))
            }
            _ => {
                let short = mod_grid::block_tag(addr.block).as_bytes();
                (
                    &short[..short.len().min(3)],
                    def.params[self.focused_slot()].label(),
                )
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
        // Any encoder turn or button press retires the last prime-status
        // message (issue #21; no timer). The MIX+Plus branch below re-sets
        // it when this same frame is itself a prime attempt.
        if any_input(controls) {
            self.prime_status = None;
        }

        // ── Sound Browser mode input ─────────────────────────────────
        if let UiMode::SoundBrowser {
            part,
            ref mut cursor,
            ref mut scroll,
        } = self.ui_mode
        {
            let total = browser::TOTAL_ENTRIES;
            let visible = browser::VISIBLE_ROWS.min(total);

            // Encoder A scrolls the cursor.
            let delta = i32::from(controls.encoder_delta(EncoderId::A));
            if delta != 0 {
                let new_cursor = (*cursor as i32 + delta).clamp(0, total as i32 - 1) as usize;
                *cursor = new_cursor;
                // Adjust scroll to keep cursor visible
                if new_cursor < *scroll {
                    *scroll = new_cursor;
                } else if new_cursor >= *scroll + visible {
                    *scroll = new_cursor + 1 - visible;
                }
                self.browser_dirty = true;
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
                    // Init entries follow the pool slots.
                    if let Some(&engine) = browser::INIT_TYPES.get(sel_cursor - POOL_SIZE) {
                        self.performance.parts[sel_part].load_init(engine);
                    }
                }
                // Switch to the loaded part and return to normal mode
                self.active_part = sel_part;
                self.nav.chain_id = ChainId::Part(sel_part);
                self.nav.node = 0;
                self.nav.sub_page = 0;
                self.nav.engine = self.performance.parts[sel_part].sound.engine();
                self.load_matrix(sel_part);
                self.enter_page();
                self.ui_mode = UiMode::Normal;
                self.browser_dirty = true;
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
                self.browser_dirty = true;
                return;
            }

            // Any B-button press: cancel browser
            let b_buttons = [
                ButtonId::B1,
                ButtonId::B2,
                ButtonId::B3,
                ButtonId::B4,
                ButtonId::B5,
                ButtonId::B6,
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
                ButtonId::B1,
                ButtonId::B2,
                ButtonId::B3,
                ButtonId::B4,
                ButtonId::B5,
                ButtonId::B6,
            ];
            for (i, &btn) in b_buttons.iter().enumerate() {
                if controls.button_state(btn) == ButtonState::Pressed {
                    self.ui_mode = UiMode::SoundBrowser {
                        part: i,
                        cursor: 0,
                        scroll: 0,
                    };
                    self.browser_dirty = true;
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
                self.nav.engine = self.performance.parts[i].sound.engine();
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
                    self.focus.touch(def.id, i);
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
                    // An empty slot edits nothing, so it does not take the focus.
                    if def.params[i].binding != block_def::SlotBinding::Empty {
                        self.focus.touch(def.id, i);
                    }
                    if let block_def::SlotBinding::Route(src) = def.params[i].binding {
                        self.edit_route(src, |a| {
                            if shift {
                                snap_amount(a, delta)
                            } else {
                                (a as i16 + delta as i16).clamp(-127, 127) as i8
                            }
                        });
                        continue;
                    }
                    let params = &mut UiBlocks {
                        part: self.performance.edit(at),
                        theme: &mut self.theme,
                    };
                    match (self.page, shift) {
                        (PageKey::Part { .. }, true) => {
                            part_page::snap_encoder(def, i, delta, params, self.sel_op)
                        }
                        (PageKey::Part { .. }, false) => {
                            part_page::apply_encoder(def, i, delta, params, &mut self.sel_op)
                        }
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
                    // Unbound slots prime nothing; non-modulatable params are
                    // refused. Report the outcome in the focus band (#21):
                    // an already-primed address is a silent `Ok` from
                    // `add`, so it must be checked for before calling it.
                    if let Some(addr) = self.current_param_addr() {
                        let label = self.mod_label(addr);
                        let sound = &mut self.performance.parts[at].sound;
                        self.prime_status = Some(if sound.dest_registry.is_primed(addr) {
                            PrimeStatus::AlreadyRouted
                        } else {
                            match sound.dest_registry.add(addr, label) {
                                Ok(()) => PrimeStatus::Added,
                                Err(e) => e.into(),
                            }
                        });
                        self.matrix_state
                            .rebuild_dests_from_registry(&sound.dest_registry);
                        // Amounts follow their destination's ParamAddr, not the
                        // column: reload from the still-committed ModState so a
                        // stale column (e.g. left over from a Part switch) can't
                        // leak into the new route (issue #11).
                        self.matrix_state.load_amounts(&sound.mod_state);
                        self.sync_mod_state(at);
                    }
                }
                if controls.button_state(ButtonId::Minus) == ButtonState::Pressed
                    && let Some(addr) = self.current_param_addr()
                {
                    let sound = &mut self.performance.parts[at].sound;
                    sound.dest_registry.remove(addr);
                    self.matrix_state
                        .rebuild_dests_from_registry(&sound.dest_registry);
                    // Re-key amounts to the (possibly shifted) destination
                    // columns by ParamAddr rather than position, so surviving
                    // routes keep their own amount, not their old column's
                    // (issue #11).
                    self.matrix_state.load_amounts(&sound.mod_state);
                    self.sync_mod_state(at);
                }
            }
        }
    }

    /// Advance animations. Call at UI_FPS (~20fps).
    pub fn update(&mut self) {
        let at = self.active_part;

        // Read base param values
        let def = self.nav.active_block_def();
        let mut values = self.display_values();
        let sound = &self.performance.parts[at].sound;

        // Apply mod offsets for display — makes bars and vizzes animate with modulation.
        // Skip the LFO tick entirely when no modulation is active.
        if sound.mod_state.num_dests() > 0 {
            // The display LFO ticks once per UI frame. `process` steps its
            // phase by rate · BLOCK_SIZE / sample_rate, so a sample rate of
            // BLOCK_SIZE · UI_FPS steps it by rate / UI_FPS: real time if
            // the loop runs at UI_FPS frames a second.
            const UI_FPS: u32 = 20;
            let lfo_val = self.display_lfos[0].run_block(
                &sound.params.lfos[0],
                chimera_hal::BLOCK_SIZE as u32 * UI_FPS,
            );

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
            for (i, value) in values.iter_mut().enumerate() {
                let Some(addr) = slot_addr(def, i, self.sel_op) else {
                    continue;
                };
                let offset = sound.mod_state.offset_for(addr, &mod_sources);
                if offset != 0.0
                    && let Some(spec) = addr.spec()
                {
                    *value = spec.offset_normalized(*value, offset);
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
        let needed_bottom =
            theme::BRANCH_START_Y + (self.nav.sub_page as i32 + 1) * theme::BRANCH_LINE_HEIGHT;
        let overflow = needed_bottom - chimera_hal::SCREEN_HEIGHT as i32;
        let target_scroll = if overflow > 0 {
            overflow as f32 / theme::BRANCH_LINE_HEIGHT as f32
        } else {
            0.0
        };
        self.renderer.branch_scroll.set_target(target_scroll);
        self.renderer.branch_scroll.update();
    }

    /// Render full screen with `scope` as the live output (tests pass a
    /// fixed buffer so screen goldens are deterministic).
    pub fn render_with_scope<D>(&self, display: &mut D, perf: &PerfStats, scope: &[f32; SCOPE_LEN])
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        self.render_with_audio(display, perf, None, scope);
    }

    /// Render full screen with `scope` as the live output and `audio` behind
    /// the System ▸ About ▸ AUDIO sub-page.
    pub fn render_with_audio<D>(
        &self,
        display: &mut D,
        perf: &PerfStats,
        audio: Option<&AudioStats>,
        scope: &[f32; SCOPE_LEN],
    ) where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        if let UiMode::SoundBrowser {
            part,
            cursor,
            scroll,
        } = self.ui_mode
        {
            let _ = display.clear(theme::BG);
            browser::draw(display, &self.pool, part, cursor, scroll);
            return;
        }
        self.renderer
            .draw_with_def(display, &self.frame(perf, audio, scope));
    }

    /// What one frame draws from.
    fn frame<'a>(
        &'a self,
        perf: &'a PerfStats,
        audio: Option<&'a AudioStats>,
        scope: &'a [f32; SCOPE_LEN],
    ) -> renderer::Frame<'a> {
        renderer::Frame {
            nav: &self.nav,
            def: self.nav.active_block_def(),
            perf,
            matrix: &self.matrix_state,
            sel_op: self.sel_op,
            focus: self.focused_slot(),
            scope,
            sounding: crate::scope::peak(scope) > crate::scope::SOUNDING_PEAK,
            parts: &self.performance.parts,
            active_part: self.active_part,
            prime_status: self.prime_status,
            audio,
            master_gr_db: crate::meter::MASTER_GR.read(),
        }
    }

    /// Snapshot of what region `kind` shows; a region redraws when it changes.
    fn region_data(&self, kind: region::RegionKind, f: &renderer::Frame) -> region::RegionData {
        use region::{RegionData, RegionKind};
        let qvalues = region::quantize_values(&self.renderer.anim);
        let audio_page = f.def.viz == VizType::AudioStats;
        let (chain, node, sub) = nav_tag(&self.nav);
        match kind {
            RegionKind::Header => {
                RegionData::header(chain, node, sub, f.perf.audio_load_pct, f.sounding)
            }
            RegionKind::Focus if f.def.layout == PageLayout::Matrix => RegionData::Route {
                row: self.matrix_state.sel_row as u8,
                col: self.matrix_state.sel_col as u8,
                dests: self.matrix_state.num_dests as u8,
                value: qvalues[renderer::MATRIX_AMOUNT_SLOT],
            },
            RegionKind::Focus => RegionData::focus(
                self.page,
                f.focus as u8,
                if audio_page {
                    audio_page::focus_key(f.audio)
                } else {
                    qvalues[f.focus]
                },
                self.prime_status,
            ),
            RegionKind::Viz => {
                let (values, live) = self.renderer.viz_inputs(f);
                // BigViz pages have no focus band: their viz carries the
                // prime status instead.
                let status = self
                    .prime_status
                    .filter(|_| f.def.layout == PageLayout::BigViz);
                RegionData::viz_with_status(self.page, values, live, status)
            }
            RegionKind::Cells => RegionData::cells(
                self.page,
                if audio_page {
                    audio_page::cells_key(f.audio)
                } else {
                    qvalues
                },
                f.focus as u8,
                self.matrix_state.num_dests as u16,
                core::array::from_fn(|i| {
                    Renderer::cell_mod_info(f.def, i, f.sel_op, f.matrix).map(f32::to_bits)
                }),
            ),
            RegionKind::Nav => RegionData::nav(
                chain,
                node,
                sub,
                region::quantize(self.renderer.branch_scroll.current()),
            ),
            RegionKind::Grid => RegionData::grid_with_value(
                self.matrix_state.sel_row as u8,
                self.matrix_state.sel_col as u8,
                self.matrix_state.scroll_x as u8,
                self.matrix_state.scroll_y as u8,
                qvalues[renderer::MATRIX_AMOUNT_SLOT],
            ),
        }
    }

    /// Prime the region set after an initial full render, so render_dirty
    /// won't redundantly redraw everything on the first call.
    pub fn prime_regions(
        &mut self,
        perf: &PerfStats,
        audio: Option<&AudioStats>,
        scope: &[f32; SCOPE_LEN],
    ) {
        self.region_set
            .set_layout(self.nav.active_block_def().layout);
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, audio, scope);
            for (d, r) in data.iter_mut().zip(self.region_set.active_regions()) {
                *d = self.region_data(r.kind, &f);
            }
        }
        for (r, d) in self.region_set.active_regions_mut().iter_mut().zip(data) {
            r.prev_data = d;
        }
    }

    /// Render only dirty regions, with `scope` as the live output and
    /// `audio` behind the AUDIO sub-page. Returns list of (y_start, y_end)
    /// pairs to flush. Slots with (0, 0) are unused.
    pub fn render_dirty_with_audio<D>(
        &mut self,
        display: &mut D,
        perf: &PerfStats,
        audio: Option<&AudioStats>,
        scope: &[f32; SCOPE_LEN],
    ) -> [(u16, u16); region::MAX_REGIONS]
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            > + chimera_hal::ChimeraDisplay,
    {
        // Sound browser overlay — one flush region, redrawn only while dirty
        // (opened, cursor/scroll moved, or a save changed the pool; #7).
        if let UiMode::SoundBrowser {
            part,
            cursor,
            scroll,
        } = self.ui_mode
        {
            let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
            if self.browser_dirty {
                let fb = display.pixel_buffer();
                Renderer::clear_region_fb(fb, 0, chimera_hal::SCREEN_HEIGHT);
                browser::draw(display, &self.pool, part, cursor, scroll);
                // Invalidate region set so normal layout forces full rebuild on exit
                self.region_set.prev_layout = None;
                flush_list[0] = (0, chimera_hal::SCREEN_HEIGHT);
                self.browser_dirty = false;
            }
            return flush_list;
        }

        let layout = self.nav.active_block_def().layout;
        let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
        let mut flush_count = 0;

        // Rebuild regions if layout changed
        if self.region_set.prev_layout != Some(layout) {
            self.region_set.set_layout(layout);
        }

        let count = self.region_set.count as usize;
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, audio, scope);
            for i in 0..count {
                let r = self.region_set.regions[i];
                data[i] = self.region_data(r.kind, &f);
                if data[i] != r.prev_data {
                    renderer::Renderer::clear_region_fb(display.pixel_buffer(), r.y_start, r.y_end);
                    self.renderer.draw_region_with_def(display, r.kind, &f);
                    flush_list[flush_count] = (r.y_start, r.y_end);
                    flush_count += 1;
                }
            }
        }
        for (r, d) in self.region_set.regions[..count].iter_mut().zip(data) {
            r.prev_data = d;
        }

        flush_list
    }

    /// Render only dirty regions, with `scope` as the live output. Returns
    /// list of (y_start, y_end) pairs to flush. Slots with (0, 0) are unused.
    pub fn render_dirty_with_scope<D>(
        &mut self,
        display: &mut D,
        perf: &PerfStats,
        scope: &[f32; SCOPE_LEN],
    ) -> [(u16, u16); region::MAX_REGIONS]
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            > + chimera_hal::ChimeraDisplay,
    {
        self.render_dirty_with_audio(display, perf, None, scope)
    }
}

/// Whether `controls` reports an encoder turn or a button press this frame —
/// any of which retires the last prime-status message (issue #21).
fn any_input(controls: &impl Controls) -> bool {
    ALL_ENCODERS.iter().any(|&e| controls.encoder_delta(e) != 0)
        || ALL_BUTTONS
            .iter()
            .any(|&b| controls.button_state(b) == ButtonState::Pressed)
}

/// A Part's blocks plus the settings the UI holds itself (System › Theme),
/// so the Theme page edits through the same slot bindings as any other.
struct UiBlocks<'a> {
    part: PartEdit<'a>,
    theme: &'a mut ThemeSettings,
}

impl Blocks for UiBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        match b {
            BlockRef::Theme => Some(&*self.theme),
            _ => self.part.block(b),
        }
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::Theme => Some(self.theme),
            _ => self.part.block_mut(b),
        }
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
        ChainId::Part(i) => i as u8,       // 0-5
        ChainId::Mixer(i) => 10 + i as u8, // 10-15
        ChainId::System => 20,
        ChainId::Demo => 21,
    };
    (chain_byte, nav.node as u8, nav.sub_page as u8)
}
