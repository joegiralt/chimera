pub mod alg_layout;
pub mod animation;
pub mod audio_page;
pub mod block_def;
pub mod block_registry;
pub mod browser;
pub mod busy;
pub mod chain;
pub mod components;
pub mod draw;
pub mod dungeon_map;
pub mod filter_panel;
pub mod fmt;
pub mod focus;
pub mod mod_grid;
pub mod mod_panel;
pub mod page;
pub mod part_page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod theme;
pub mod theme_settings;
pub mod view;
pub mod viz;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::storage::{Card, Exit, SystemSettings, SystemSync};
use chimera_hal::store::Store;
use chimera_hal::{ALL_BUTTONS, ALL_ENCODERS, ButtonId, ButtonState, Controls, PART_BUTTONS};

use crate::addr::{BlockRef, Blocks, Op, ParamAddr};
use crate::block::Block;
use crate::dsp::lfo::Lfo;
use crate::dsp::modulator::{EnvSlot, EnvType, LfoSlot, LfoType};
use crate::in_place::{by_value, uninit_at};
use crate::mod_path::{LABEL_LEN, RegistryError};
use crate::modulation::{CUTOFF, CUTOFF_LABEL, MAX_MOD_SOURCES, ModSource, ModState};
use crate::params::ParamSnapshot;
use crate::perf::load::AudioStats;
use crate::preset::{POOL_SIZE, Performance, SoundPool, part_block, part_block_mut};
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
use view::{SlotCtx, View};

/// MIX + turn on a route knob: the next of −127, 0, +127 that way.
fn snap_amount(a: i8, delta: i8) -> i8 {
    match (delta > 0, a) {
        (true, a) if a < 0 => 0,
        (true, _) => 127,
        (false, a) if a > 0 => 0,
        (false, _) => -127,
    }
}

/// The address MIX+PLUS primes for a slot's `addr` (Decisions table): an
/// A stage's time primes the slot's TIME, S its LEVEL; the rest themselves.
fn prime_target(addr: ParamAddr) -> ParamAddr {
    use crate::params::EnvParams as E;
    match (addr.block, addr.param) {
        (BlockRef::Env(_), E::ATTACK | E::DECAY | E::RELEASE | E::HOLD) => {
            ParamAddr::new(addr.block, E::TIME)
        }
        (BlockRef::Env(_), E::SUSTAIN) => ParamAddr::new(addr.block, E::LEVEL),
        (BlockRef::Out, crate::params::OutParams::VCA_VEL) => crate::modulation::VCA,
        _ => addr,
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
    /// System › Theme; boot sets it from SYSTEM (`set_theme`).
    theme: ThemeSettings,
    /// What the last card operation said, for a moment.
    toast: busy::ToastTimer,
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
    toast,
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
            addr_of_mut!((*p).toast).write(busy::ToastTimer::new());
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

    /// The theme SYSTEM held, set at boot before the first frame.
    pub fn set_theme(&mut self, t: ThemeSettings) {
        self.theme = t;
    }

    /// Once a frame, after input: on leaving System, syncs SYSTEM (one
    /// mount, which may load or save) and puts up what came of it. It draws
    /// nothing, so no BUSY covers a save too quick to read; a `Loaded`
    /// theme is applied here.
    pub fn sync_system<S: Store>(
        &mut self,
        sync: &mut SystemSync,
        card: &mut Card,
        store: &mut S,
        s: &mut SystemSettings,
    ) {
        s.theme = self.theme;
        if !sync.left_system(self.in_system(), s) {
            return;
        }
        let r = sync.on_exit(card, store, s);
        if r == Ok(Exit::Loaded) {
            self.theme = s.theme;
        }
        if let Some(t) = busy::toast_for(&r) {
            self.toast.show(t);
        }
    }

    /// Once a frame: the toast, `elapsed_ms` after the last frame.
    pub fn step_toast(&mut self, elapsed_ms: u32) -> busy::ToastStep {
        self.toast.step(elapsed_ms)
    }

    /// On the System chain, any of its pages or sub-pages: moving between
    /// them is no exit. Leaving the chain is when SYSTEM syncs.
    pub fn in_system(&self) -> bool {
        self.nav.chain_id == ChainId::System
    }

    /// The edited Part's blocks and the System pages' (THEME, MIDI), as one `Blocks`.
    fn blocks(&mut self, part: usize) -> UiBlocks<'_> {
        UiBlocks {
            perf: &mut self.performance,
            at: part,
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

    /// What the edited Part's page slots resolve against.
    fn ctx(&self) -> SlotCtx {
        SlotCtx::read(
            &self.performance.parts[self.active_part].sound.params,
            self.sel_op,
        )
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
        let ctx = self.ctx();
        for (i, value) in values.iter_mut().enumerate() {
            if let View::Route { source, .. } = view::view(def, i, &ctx) {
                *value = renderer::amount_value(
                    self.matrix_state.route(source.index(), CUTOFF).unwrap_or(0),
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
            if let Err(e) = sound.dest_registry.add(CUTOFF, CUTOFF_LABEL) {
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
            &self.ctx(),
        )
        .map(prime_target)
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
            BlockRef::Env(s) => {
                op_prefix = [b'E', b'1' + s.index() as u8, b' '];
                (&op_prefix, addr.spec().map_or("", |s| s.label))
            }
            _ if addr == crate::modulation::VCA => (b"OUT ".as_slice(), "VCA"),
            _ => {
                let short = mod_grid::block_tag(addr.block).as_bytes();
                (
                    &short[..short.len().min(3)],
                    view::view(def, self.focused_slot(), &self.ctx()).label(),
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

    /// One frame of input while the sound browser for `part` is open.
    fn browser_input(
        &mut self,
        controls: &impl Controls,
        part: usize,
        cursor: usize,
        scroll: usize,
    ) {
        let (cursor, scroll, act) = browser::handle(controls, cursor, scroll);
        if self.ui_mode
            != (UiMode::SoundBrowser {
                part,
                cursor,
                scroll,
            })
        {
            self.ui_mode = UiMode::SoundBrowser {
                part,
                cursor,
                scroll,
            };
            self.browser_dirty = true;
        }
        match act {
            Some(browser::BrowserAct::Load) => {
                if cursor < POOL_SIZE {
                    if let Some(sound) = self.pool.get(cursor) {
                        self.performance.parts[part].sound = sound.clone();
                        self.performance.parts[part].loaded_from = Some(cursor as u8);
                    }
                } else if let Some(&engine) = browser::INIT_TYPES.get(cursor - POOL_SIZE) {
                    // Init entries follow the pool slots.
                    self.performance.parts[part].load_init(engine);
                }
                self.active_part = part;
                self.nav.chain_id = ChainId::Part(part);
                self.nav.node = 0;
                self.nav.sub_page = 0;
                self.nav.engine = self.performance.parts[part].sound.engine();
                self.load_matrix(part);
                self.enter_page();
                self.ui_mode = UiMode::Normal;
                self.browser_dirty = true;
            }
            Some(browser::BrowserAct::Save) => {
                if cursor < POOL_SIZE {
                    let sound = self.performance.parts[part].sound.clone();
                    self.pool.store(cursor, sound);
                }
                self.browser_dirty = true;
            }
            Some(browser::BrowserAct::Cancel) => self.ui_mode = UiMode::Normal,
            None => {}
        }
    }

    /// Process one frame of input: navigation + encoder deltas.
    pub fn handle_input(&mut self, controls: &impl Controls) {
        // Any encoder turn or button press retires the last prime-status
        // message (issue #21; no timer). The MIX+Plus branch below re-sets
        // it when this same frame is itself a prime attempt.
        if any_input(controls) {
            self.prime_status = None;
            self.toast.dismiss();
        }

        if let UiMode::SoundBrowser {
            part,
            cursor,
            scroll,
        } = self.ui_mode
        {
            // The browser takes every input while it is open.
            self.browser_input(controls, part, cursor, scroll);
            return;
        }

        // ── Normal mode ──────────────────────────────────────────────

        // Edit + B1-B6: open sound browser for that part
        let edit_held = matches!(
            controls.button_state(ButtonId::Edit),
            ButtonState::Pressed | ButtonState::Held
        );
        if edit_held {
            for (i, &btn) in PART_BUTTONS.iter().enumerate() {
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
                self.nav
                    .set_engine(self.performance.parts[i].sound.engine());
                self.load_matrix(i);
            }
            self.enter_page();
        }

        // Encoder deltas -> parameter changes
        let shift = matches!(
            controls.button_state(ButtonId::Mix),
            ButtonState::Pressed | ButtonState::Held
        );

        let def = self.nav.active_block_def();
        if def.layout == PageLayout::Matrix {
            for (i, &enc) in ALL_ENCODERS.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta != 0 {
                    self.focus.touch(def.id, i);
                    match i {
                        0 => self.matrix_state.move_row(delta),
                        1 => self.matrix_state.move_col(delta),
                        3 => self.matrix_state.scroll_h(delta),
                        4 => {
                            self.matrix_state.adjust_amount(delta);
                            self.sync_mod_state(self.active_part);
                        }
                        _ => {}
                    }
                }
            }
            // MIX + Minus deletes the route under the cursor (spec § 2).
            if shift && controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
                self.matrix_state.delete_selected();
                self.sync_mod_state(self.active_part);
            }
        } else {
            let at = self.active_part;
            let before = self.ctx();
            for (i, &enc) in ALL_ENCODERS.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta == 0 {
                    continue;
                }
                let v = view::view(def, i, &self.ctx());
                // An empty slot edits nothing, so it does not take the focus.
                if v != View::Empty {
                    self.focus.touch(def.id, i);
                }
                if let View::Route { source, .. } = v {
                    self.edit_route(source, |a| {
                        if shift {
                            snap_amount(a, delta)
                        } else {
                            (a as i16 + delta as i16).clamp(-127, 127) as i8
                        }
                    });
                    continue;
                }
                if view::is_dimmed(&v, &self.performance.parts[at].sound) {
                    continue; // dimmed: the encoder is ignored
                }
                let params = &mut UiBlocks {
                    perf: &mut self.performance,
                    at,
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
            // A KIND, TYPE or operator change re-seeds the page's animators: a lerp
            // between two parameters' values would draw a meaningless sweep.
            if self.ctx() != before {
                let values = self.display_values();
                self.renderer.snap_to_current(values);
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
                    // A dimmed slot that primes itself is refused; one that
                    // primes elsewhere (AMP's VEL: the VCA) still primes.
                    let v = view::view(def, self.focused_slot(), &self.ctx());
                    let sound = &self.performance.parts[at].sound;
                    if let Some(a) = v.addr()
                        && prime_target(a) == a
                        && view::is_dimmed(&v, sound)
                    {
                        self.prime_status = Some(PrimeStatus::NotModulatable);
                    } else if let Some(addr) = self.current_param_addr() {
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
        let ctx = self.ctx();
        let sound = &self.performance.parts[at].sound;

        // Apply mod offsets for display — makes bars and vizzes animate with modulation.
        // Skip the LFO tick entirely when no modulation is active.
        if sound.mod_state.num_dests() > 0 {
            // The display LFO ticks once per UI frame. `process` steps its
            // phase by rate · BLOCK_SIZE / sample_rate, so a sample rate of
            // BLOCK_SIZE · UI_FPS steps it by rate / UI_FPS: real time if
            // the loop runs at UI_FPS frames a second.
            const UI_FPS: u32 = 20;
            let p = &sound.params;
            let mut mod_sources = [0.0f32; MAX_MOD_SOURCES];
            // Spec § UI: an A slot stands in with its SUS, a B slot with ½;
            // each CLASSIC LFO its own display LFO, a FUNC LFO 0; VEL 1; NOTE 0.
            for s in EnvSlot::ALL {
                let e = &p.envelopes[s.index()];
                mod_sources[ModSource::of_env(s).index()] = match e.env_type {
                    EnvType::A => e.sustain,
                    EnvType::B => 0.5,
                };
            }
            for s in LfoSlot::ALL {
                let l = &p.lfos[s.index()];
                // FUNC is not run: its stand-in is 0, whatever the rate.
                if l.lfo_type == LfoType::Classic {
                    mod_sources[ModSource::of_lfo(s).index()] = self.display_lfos[s.index()]
                        .run_block(l, chimera_hal::BLOCK_SIZE as u32 * UI_FPS);
                }
            }
            mod_sources[ModSource::Vel.index()] = 1.0;

            // Apply offsets to the 6 display values
            for (i, value) in values.iter_mut().enumerate() {
                let Some(addr) = slot_addr(def, i, &ctx) else {
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
            ctx: self.ctx(),
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
            RegionKind::Header => RegionData::header(
                chain,
                node,
                sub,
                f.perf.audio_load_pct,
                f.sounding,
                renderer::title_type(f),
                renderer::header_out(f) as u8,
            ),
            RegionKind::Focus if f.def.layout == PageLayout::Matrix => RegionData::route(
                self.matrix_state.sel_row as u8,
                self.matrix_state.sel_col as u8,
                self.matrix_state.num_dests as u8,
                qvalues[renderer::MATRIX_AMOUNT_SLOT],
            )
            .keyed(self.matrix_state.rev, renderer::inert(f)),
            RegionKind::Focus => RegionData::focus(
                self.page,
                f.focus as u8,
                if audio_page {
                    audio_page::focus_key(f.audio)
                } else {
                    qvalues[f.focus]
                },
                renderer::look(f, f.focus),
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
            RegionKind::Cells => {
                let looks = (0..6).fold(0u16, |k, i| k | (renderer::look(f, i) as u16) << (2 * i));
                RegionData::cells(
                    self.page,
                    if audio_page {
                        audio_page::cells_key(f.audio)
                    } else {
                        qvalues
                    },
                    f.focus as u8,
                    self.matrix_state.num_dests as u16,
                    core::array::from_fn(|i| renderer::mod_info(f, i).map(f32::to_bits)),
                )
                .keyed(self.matrix_state.rev, looks)
            }
            RegionKind::Nav => RegionData::nav(
                chain,
                node,
                sub,
                region::quantize(self.renderer.branch_scroll.current()),
                f.ctx.model as u8,
            ),
            RegionKind::Grid => RegionData::grid_with_value(
                self.matrix_state.sel_row as u8,
                self.matrix_state.sel_col as u8,
                self.matrix_state.scroll_x as u8,
                qvalues[renderer::MATRIX_AMOUNT_SLOT],
            )
            .keyed(self.matrix_state.rev, renderer::inert(f)),
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

    /// The key region `kind` was last drawn with, if the page has it.
    pub fn drawn_key(&self, kind: region::RegionKind) -> Option<region::RegionData> {
        self.region_set
            .active_regions()
            .iter()
            .find(|r| r.kind == kind)
            .map(|r| r.prev_data)
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

/// A Part's blocks plus the ones the System pages edit (Theme, every
/// Part's channel), so they edit through the same slot bindings as any other.
struct UiBlocks<'a> {
    perf: &'a mut Performance,
    at: usize,
    theme: &'a mut ThemeSettings,
}

impl Blocks for UiBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        match b {
            BlockRef::Theme => Some(&*self.theme),
            BlockRef::Channels => Some(&self.perf.parts),
            _ => part_block(&self.perf.parts[self.at], &self.perf.fx, b),
        }
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::Theme => Some(self.theme),
            BlockRef::Channels => Some(&mut self.perf.parts),
            _ => part_block_mut(&mut self.perf.parts[self.at], &mut self.perf.fx, b),
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
