pub mod alg_layout;
pub mod animation;
pub mod audio_page;
pub mod block_def;
pub mod block_registry;
pub mod browser;
pub mod busy;
pub mod components;
pub mod draw;
pub mod dungeon_map;
pub mod filter_panel;
pub mod fmt;
pub mod focus;
pub mod glyph;
pub mod hold;
pub mod mod_grid;
pub mod mod_panel;
pub mod nav;
pub mod page;
pub mod part_page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod settings;
pub mod splash;
pub mod theme;
pub mod theme_settings;
pub mod view;
pub mod viz;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::storage::{Card, Exit, SystemSettings, SystemSync};
use chimera_hal::store::Store;
use chimera_hal::{
    ALL_BUTTONS, ALL_ENCODERS, ButtonId, ButtonState, Controls, EncoderId, PART_BUTTONS,
};

use crate::addr::{BlockRead, BlockRef, Blocks, Op, ParamAddr};
use crate::block::Block;
use crate::dsp::lfo::Lfo;
use crate::dsp::modulator::{EnvSlot, EnvType, LfoSlot, LfoType};
use crate::in_place::{by_value, uninit_at};
use crate::mod_path::{LABEL_LEN, RegistryError};
use crate::modulation::{CUTOFF, CUTOFF_LABEL, MAX_MOD_SOURCES, ModSource, ModState};
use crate::params::{EngineType, ParamSnapshot};
use crate::perf::load::AudioStats;
use crate::preset::POOL_SIZE;
use crate::project::{
    self, Confirmed, Line, LoadLink, PartEdit, PartFrom, PartId, PartSource, Project, ProjectFile,
    ProjectNote, ProjectSource, ProjectStatus, ReplaceGuard, StatusCache, Swap, TemplateCrc,
    part_block_mut,
};
use crate::scope::SCOPE_LEN;
use crate::storage::ProjectId;
use block_def::BlockDef;
use block_def::VizType;
use block_def::slot_addr;
use components::Head;
use hold::{HoldGates, Press};
use mod_grid::MatrixState;
use nav::{Browse, ListAt, Location, NavCtx, NavKey, Recall, Step, chain_def_for};
use page::{PageKey, PageLayout};
use perf::PerfStats;
use renderer::Renderer;
use settings::naming::Naming;
use settings::prompt::{self, Answer};
use settings::{Answered, Ask, Done, Modal, ModalStep, NamingFor};
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

/// What lists, Screens and the Sound rung show for a page: six empty
/// slots, under an id no chain uses (`no_page_id_is_reserved`).
pub static NO_PAGE: BlockDef = BlockDef {
    id: NO_PAGE_ID,
    name: "",
    short: "",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [block_def::ParamSlot::EMPTY; 6],
};
/// No page has it: `focus` keeps no slot for it.
pub const NO_PAGE_ID: u16 = u16::MAX;

/// The SETTINGS rows and keys not wired yet (Tasks 11–13).
const NOT_YET: &str = "NOT YET";

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
    /// Where the UI is (ADR 0066).
    loc: Location,
    /// What leaving a place remembers.
    recall: Recall,
    /// MENU and SEQ: tap on release, hold at `hold::HOLD_MS`.
    gates: HoldGates,
    /// A SETTINGS list's first row shown.
    list_first: u8,
    /// The pool, the Parts and the FX: what a project file holds.
    project: Project,
    /// NEW's CRC, computed when the project was built.
    template: TemplateCrc,
    pub active_part: PartId,
    pub renderer: Renderer,
    pub matrix_state: MatrixState,
    /// Set on every move, and when the project is replaced:
    /// `render_dirty_with_scope` redraws the Sound rung's browser only
    /// then, and clears the flag once flushed (#7).
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
    /// SETTINGS › THEME; boot sets it from SYSTEM (`set_theme`).
    theme: ThemeSettings,
    /// What the last card operation said, for a moment.
    toast: busy::ToastTimer,
    /// `project_status`, refreshed by `handle_input` and `update`; render
    /// only reads it.
    status: StatusCache,
    /// Animation phase for the renderer.
    clock: animation::UiClock,
    /// A prompt or NAMING: while open it takes every key but B*n*.
    modal: Option<Modal>,
}

crate::in_place::field_list!(UiState => UiState {
    loc,
    recall,
    gates,
    list_first,
    project,
    template,
    active_part,
    renderer,
    matrix_state,
    browser_dirty,
    page,
    sel_op,
    region_set,
    focus,
    display_lfos,
    prime_status,
    theme,
    toast,
    status,
    clock,
    modal,
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

    // In place so the ~34 KB state (the 28 KB pool) never passes through
    // the firmware's stack.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the project is built in
        // place before it is borrowed, and every other field is written
        // once, all before `assume_init_mut`.
        unsafe {
            let template = Project::init_in_place(uninit_at(addr_of_mut!((*p).project)));
            let project = &mut *addr_of_mut!((*p).project);
            let cx = nav_cx(project);
            let loc = Location::home(&cx);
            let def = page_def(loc, &cx);
            let page = PageKey::from_location(loc, def, Op::A);
            let mut renderer = Renderer::new();
            let theme = ThemeSettings::DEFAULT;
            let read = UiRead {
                project,
                part: PartId::ALL[0],
                theme: &theme,
            };
            renderer.snap_to_current(page_values(page, def, &read, Op::A));
            addr_of_mut!((*p).loc).write(loc);
            addr_of_mut!((*p).recall).write(Recall::new());
            addr_of_mut!((*p).gates).write(HoldGates::new());
            addr_of_mut!((*p).list_first).write(0);
            addr_of_mut!((*p).template).write(template);
            addr_of_mut!((*p).active_part).write(PartId::ALL[0]);
            addr_of_mut!((*p).renderer).write(renderer);
            addr_of_mut!((*p).matrix_state).write(MatrixState::new());
            addr_of_mut!((*p).browser_dirty).write(false);
            addr_of_mut!((*p).page).write(page);
            addr_of_mut!((*p).sel_op).write(Op::A);
            addr_of_mut!((*p).region_set).write(region::RegionSet::new());
            addr_of_mut!((*p).focus).write(focus::FocusMemory::new());
            addr_of_mut!((*p).display_lfos).write([Lfo::new(); 3]);
            addr_of_mut!((*p).prime_status).write(None);
            addr_of_mut!((*p).theme).write(theme);
            addr_of_mut!((*p).toast).write(busy::ToastTimer::new());
            addr_of_mut!((*p).status).write(StatusCache::new());
            addr_of_mut!((*p).clock).write(animation::UiClock::new());
            addr_of_mut!((*p).modal).write(None);
            let ui = slot.assume_init_mut();
            ui.load_matrix(PartId::ALL[0]);
            ui
        }
    }

    /// The last MIX+PLUS outcome, shown in the focus band until the next
    /// encoder, button or page change (issue #21).
    pub fn prime_status(&self) -> Option<PrimeStatus> {
        self.prime_status
    }

    /// Where the UI is.
    pub fn location(&self) -> Location {
        self.loc
    }

    /// The UI's animation clock.
    pub fn clock(&self) -> animation::UiClock {
        self.clock
    }

    /// SETTINGS › THEME as last edited; the display shell applies it.
    pub fn theme(&self) -> ThemeSettings {
        self.theme
    }

    /// The theme SYSTEM held, set at boot before the first frame.
    pub fn set_theme(&mut self, t: ThemeSettings) {
        self.theme = t;
    }

    /// Once a frame, after input: on leaving SETTINGS, syncs SYSTEM (one
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
        if !sync.left_system(self.in_settings(), s) {
            return;
        }
        let r = sync.on_exit(card, store, s, self.project.meta().file());
        if r == Ok(Exit::Loaded) {
            self.theme = s.theme;
        }
        if let Some(t) = busy::toast_for(&r) {
            self.toast.show(t);
        }
    }

    /// A project note as a toast: a save's time for SAVED, else an error's.
    pub fn show_note(&mut self, n: ProjectNote) {
        let ms = match n {
            ProjectNote::Saved(_) => busy::Toast::SAVED_MS,
            _ => busy::Toast::ERROR_MS,
        };
        self.toast.show(busy::Toast { text: n.line(), ms });
    }

    /// The active Part's engine, before a replace (`project_replaced`).
    fn active_engine(&self) -> EngineType {
        self.project.part(self.active_part).sound.engine()
    }

    /// After a load or a boot replaced the project: the active Part's
    /// engine and matrix, the page, and its values snapped, not lerped
    /// (projects spec § Loading while playing: the voice fade covers it).
    /// Its pages go home when its engine is no longer `was` (ADR 0066).
    fn project_replaced(&mut self, was: EngineType) {
        let part = self.active_part;
        let cx = self.cx();
        if let (Some((c, at)), Some(p)) = (self.loc.page(&cx), self.loc.part())
            && self.loc.settings().is_none()
            && !self.loc.on_mixer()
            && self.loc.browse().is_none()
        {
            let engine = cx.engines[p.index()];
            if engine != was || c.def_at(at).is_none() {
                self.loc = Location::part_home(p, engine);
            }
        }
        self.load_matrix(part);
        self.enter_page();
        self.browser_dirty = true;
    }

    /// Boot step 2, after SYSTEM: SYSTEM's last project, or NEW and why.
    pub fn boot_project<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        last: Option<ProjectId>,
    ) {
        let was = self.active_engine();
        if let Some(n) = project::boot_project(card, store, last, &mut self.project) {
            self.show_note(n);
        }
        self.project_replaced(was);
    }

    /// SAVE (`meta().file()`) or a first save / SAVE AS (`new_project_id`).
    /// A save that lands becomes SYSTEM's last project.
    ///
    /// ```compile_fail,E0308
    /// # use chimera_core::storage::{Card, SystemSync};
    /// # let mut s = chimera_hal::testkit::MemStore::new(1);
    /// # let mut card = Card::new();
    /// # let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    /// let mut ui = chimera_core::ui::UiState::new();
    /// let to = ui.project().meta().file(); // NEW has none
    /// ui.save_project(&mut card, &mut s, &mut sync, &mut set, to);
    /// ```
    pub fn save_project<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        to: ProjectFile,
    ) {
        let n = project::save_project(card, store, &mut self.project, to);
        if let ProjectNote::Saved(_) = n {
            self.remember(card, store, sync, settings, to);
        }
        self.show_note(n);
    }

    /// Replaces the project as `go` confirmed. A file that loads becomes
    /// SYSTEM's last project; `+ NEW` and a fallback to NEW leave it.
    ///
    /// On a swap, `publish` settles the `Swap` (`LOAD_ACK_TIMEOUT_MS`) and
    /// publishes the project, and its result comes back. It runs before the
    /// SYSTEM write: from the bump to the publish the audio holds the note
    /// queues, so no card write may sit in that gap (ADR 0046).
    // The card, SYSTEM and the audio each take their own: no struct
    // groups them.
    #[allow(clippy::too_many_arguments)]
    pub fn load_project<S: Store, R>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        go: Confirmed<ProjectSource>,
        link: &LoadLink,
        publish: impl FnOnce(Swap, &Project) -> R,
    ) -> Option<R> {
        let was = self.active_engine();
        let out = project::load_project(card, store, &mut self.project, go, link);
        if let Some(n) = out.note {
            self.show_note(n);
        }
        let swap = out.swap?;
        let published = publish(swap, &self.project);
        self.project_replaced(was);
        // Only a file load sets it: NEW, loaded or fallen back to, has none.
        if let Some(f) = self.project.meta().file() {
            self.remember(card, store, sync, settings, f);
        }
        Some(published)
    }

    /// Deletes `file` (`project::delete_project`); a refusal or a card
    /// error shows its note. If SYSTEM names it as its card's last project
    /// (possible after `+ NEW`), it names none, so the next boot is NEW,
    /// not PROJECT NOT FOUND.
    pub fn delete_project<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        file: ProjectFile,
    ) {
        match project::delete_project(card, store, &self.project, file) {
            // A failed write: the next save, load or SETTINGS exit retries
            // SYSTEM; until then a boot falls back to NEW and says why.
            Ok(()) => {
                if sync.forget(card, store, settings, file) == Ok(Exit::Loaded) {
                    self.theme = settings.theme;
                }
            }
            Err(n) => self.show_note(n),
        }
    }

    /// `f` becomes its card's last project in SYSTEM; a card SYSTEM was
    /// taken from brings its theme. A failed write leaves the toast to the
    /// project's note: the next save, load or SETTINGS exit retries it.
    fn remember<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        f: ProjectFile,
    ) {
        if sync.write(card, store, settings, f) == Ok(Exit::Loaded) {
            self.theme = settings.theme;
        }
    }

    /// Once a frame: the toast, `elapsed_ms` after the last frame.
    pub fn step_toast(&mut self, elapsed_ms: u32) -> busy::ToastStep {
        self.toast.step(elapsed_ms)
    }

    /// Anywhere in SETTINGS: moving inside it is no exit. Leaving it is
    /// when SYSTEM syncs.
    pub fn in_settings(&self) -> bool {
        self.loc.settings().is_some()
    }

    fn cx(&self) -> NavCtx {
        nav_cx(&self.project)
    }

    /// The page shown: its def, `NO_PAGE` on lists, Screens and the Sound
    /// rung.
    pub fn page_def(&self) -> &'static BlockDef {
        page_def(self.loc, &self.cx())
    }

    /// Move to `to`: a Part's place selects that Part for editing (its
    /// matrix), and the page's values snap.
    fn go(&mut self, to: Location) {
        let from = self.loc;
        self.loc = to;
        if let Some(s) = to.settings() {
            let prev = match from.settings() {
                Some(f) if f.path() == s.path() => self.list_first as usize,
                _ => 0,
            };
            let len = settings::rows(s.path()).len();
            self.list_first = settings::view::first_visible(s.row() as usize, len, prev) as u8;
        }
        if let Some(p) = to.part() {
            self.active_part = p;
            self.load_matrix(p);
        }
        self.browser_dirty = true;
        self.enter_page();
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The cache is dropped first: a whole project assigned through this
    /// brings its own revision, which may be the one last hashed.
    pub fn project_mut(&mut self) -> &mut Project {
        self.status.invalidate();
        &mut self.project
    }

    /// NEW's CRC: a project whose `project_crc` equals it is `Pristine`.
    pub fn template(&self) -> TemplateCrc {
        self.template
    }

    /// The project's status as `update` last found it:
    /// render reads this, and never hashes.
    pub fn project_status(&self) -> ProjectStatus {
        self.status.cached()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn status_hashes_for_test(&self) -> u32 {
        self.status.hashes_for_test()
    }

    /// Hashes only when the project's revision moved.
    fn refresh_status(&mut self) {
        self.status.get(&self.project, self.template);
    }

    /// Part `part`'s blocks and SETTINGS › THEME, read only: reading leaves
    /// the project's revision alone.
    fn read(&self, part: PartId) -> UiRead<'_> {
        UiRead {
            project: &self.project,
            part,
            theme: &self.theme,
        }
    }

    /// Returns a reference to the active part's params.
    pub fn params(&self) -> &ParamSnapshot {
        &self.project.part(self.active_part).sound.params
    }

    /// Returns a mutable reference to the active part's params.
    pub fn params_mut(&mut self) -> &mut ParamSnapshot {
        &mut self.project.edit_part(self.active_part).sound.params
    }

    /// Returns a reference to the active part's mod state.
    pub fn mod_state(&self) -> &ModState {
        &self.project.part(self.active_part).sound.mod_state
    }

    /// Returns a mutable reference to the active part's mod state.
    pub fn mod_state_mut(&mut self) -> &mut ModState {
        &mut self.project.edit_part(self.active_part).sound.mod_state
    }

    /// Current page identity.
    pub fn page(&self) -> PageKey {
        self.page
    }

    /// The slot the focus band shows on the current page: the last one
    /// turned there, slot a until then.
    pub fn focused_slot(&self) -> usize {
        self.focus.get(self.page_def().id)
    }

    /// The selected operator.
    pub fn selected_op(&self) -> Op {
        self.sel_op
    }

    /// What the edited Part's page slots resolve against.
    fn ctx(&self) -> SlotCtx {
        SlotCtx::read(
            &self.project.part(self.active_part).sound.params,
            self.sel_op,
        )
    }

    /// Recompute the page identity and jump the display to its values.
    fn enter_page(&mut self) {
        self.page = PageKey::from_location(self.loc, self.page_def(), self.sel_op);
        let values = self.display_values();
        self.renderer.snap_to_current(values);
    }

    /// The six values the display animates toward: the page's slots, and on
    /// the mod matrix the selected route's amount in slot e.
    fn display_values(&self) -> [f32; 6] {
        let def = self.page_def();
        let (page, sel_op) = (self.page, self.sel_op);
        let mut values = page_values(page, def, &self.read(self.active_part), sel_op);
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
    fn load_matrix(&mut self, part: PartId) {
        let sound = &self.project.part(part).sound;
        self.matrix_state
            .rebuild_sources(chain_def_for(sound.engine()).mod_sources);
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
        let sound = self.project.edit_part(at).sound;
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
    fn sync_mod_state(&mut self, part: PartId) {
        let sound = self.project.edit_part(part).sound;
        sound.mod_state.sync_from_matrix(&self.matrix_state);
    }

    /// The address the focused encoder edits, if its slot is bound. Leaf
    /// and DEMO slots are `Legacy`, so priming there does nothing; Mixer
    /// params are bound but not modulatable, so the registry refuses them.
    fn current_param_addr(&self) -> Option<ParamAddr> {
        slot_addr(self.page_def(), self.focused_slot(), &self.ctx()).map(prime_target)
    }

    /// 8-byte matrix column label for a primed destination: `O<n> ` + spec
    /// label for operator params, else the address's block tag (≤ 3 chars,
    /// `mod_grid::block_tag` — the page can be a sub-page with a different
    /// short name, e.g. FLT › MODE for the filter's DRIVE) + the slot label.
    fn mod_label(&self, addr: ParamAddr) -> [u8; LABEL_LEN] {
        let def = self.page_def();
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

    /// EDIT on the Sound rung: the entry under the cursor into `part`, then
    /// its pages.
    fn load_sound(&mut self, part: PartId, b: Browse) {
        let cursor = b.cursor();
        // Init entries follow the pool slots.
        let from = browser::slot_at(cursor).map(PartFrom::Slot).or_else(|| {
            cursor
                .checked_sub(POOL_SIZE)
                .and_then(|j| browser::INIT_TYPES.get(j))
                .map(|&e| PartFrom::Init(e))
        });
        if let Some(from) = from {
            // No prompt screen yet: https://github.com/joegiralt/chimera/issues/258.
            let src = PartSource { part, from };
            let c = ReplaceGuard::check(&self.project, self.template, src)
                .unwrap_or_else(|n| n.into_pending().anyway(&self.project));
            // An empty slot loads nothing.
            let _ = self.project.replace_part(c);
        }
        let to = Location::part_home(part, self.project.part(part).sound.engine());
        self.go(to);
    }

    /// The keys this frame, in order: MENU, B*n*, PLUS and MINUS, SEQ, EDIT,
    /// then encoder A on a list. With MIX down, PLUS and MINUS are MIX's
    /// own (prime, delete), never a move.
    fn nav_keys(&self, c: &impl Controls, presses: hold::Presses) -> [Option<NavKey>; 12] {
        let pressed = |b| c.button_state(b) == ButtonState::Pressed;
        let down = |b| matches!(c.button_state(b), ButtonState::Pressed | ButtonState::Held);
        let mix = c.edges(ButtonId::Mix).down;
        let edit = down(ButtonId::Edit);
        let mut keys = [None; 12];
        let mut n = 0;
        let mut push = |k| {
            keys[n] = Some(k);
            n += 1;
        };
        if presses.menu == Some(Press::Tap) {
            push(NavKey::MenuTap);
        }
        let mut edit_part = false;
        for (p, &b) in PartId::ALL.into_iter().zip(PART_BUTTONS.iter()) {
            if pressed(b) {
                edit_part |= edit;
                push(match (edit, mix) {
                    (true, _) => NavKey::EditPart(p),
                    (_, true) => NavKey::MixPart(p),
                    _ => NavKey::Part(p),
                });
            }
        }
        if !mix && pressed(ButtonId::Minus) {
            push(NavKey::Minus);
        }
        if !mix && pressed(ButtonId::Plus) {
            push(NavKey::Plus);
        }
        if presses.seq == Some(Press::Tap) {
            push(NavKey::SeqTap);
        }
        if pressed(ButtonId::Edit) && !edit_part {
            push(NavKey::Edit);
        }
        let a = c.encoder_delta(EncoderId::A);
        let on_list = self.loc.settings().is_some_and(|s| s.at_leaf().is_none());
        if on_list && a != 0 {
            push(NavKey::Bar(a));
        }
        keys
    }

    /// Whether a prompt is open.
    pub fn prompt_open(&self) -> bool {
        self.prompt().is_some()
    }

    fn prompt(&self) -> Option<&Ask> {
        match &self.modal {
            Some(Modal::Prompt(a)) => Some(a),
            _ => None,
        }
    }

    /// SETTINGS' breadcrumb as drawn.
    pub fn crumbs(&self) -> Option<settings::view::Crumbs> {
        self.bands().map(|b| b.crumbs())
    }

    fn bands(&self) -> Option<settings::view::Bands> {
        use settings::view::BandsModal;
        self.loc.settings().map(|at| settings::view::Bands {
            at,
            active: self.active_part,
            first: self.list_first as usize,
            name: self.project.meta().name(),
            status: self.project_status(),
            modal: self.modal.as_ref().map(|m| match m {
                Modal::Prompt(_) => BandsModal::Prompt,
                Modal::Naming(f, n) => BandsModal::Naming {
                    naming: *n,
                    title: f.title(),
                    crumb: f.crumb(),
                },
            }),
        })
    }

    /// NAMING, if it is open.
    pub fn naming(&self) -> Option<&Naming> {
        match &self.modal {
            Some(Modal::Naming(_, n)) => Some(n),
            _ => None,
        }
    }

    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "Tasks 11–13 open prompts")
    )]
    fn ask(&mut self, a: Ask) {
        self.open(Modal::Prompt(a));
    }

    /// NAMING takes a list's band, so it opens on one: `at`.
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "Tasks 11–13 open NAMING")
    )]
    fn name(&mut self, at: ListAt, f: NamingFor, start: &str) {
        self.go(at.location());
        self.open(Modal::Naming(f, Naming::new(start)));
    }

    fn open(&mut self, m: Modal) {
        self.modal = Some(m);
        self.browser_dirty |= self.loc.browse().is_some();
    }

    /// The screen beneath redraws whole once a modal closes.
    fn closed(&mut self) {
        self.region_set.prev_screen = None;
        self.browser_dirty |= self.loc.browse().is_some();
    }

    /// A replace from `src` that asks opens its prompt; whether it did.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn ask_replace_for_test(&mut self, src: PartSource) -> bool {
        match ReplaceGuard::check(&self.project, self.template, src) {
            Ok(_) => false,
            Err(n) => {
                self.ask(Ask::replace_part(n.into_pending()));
                true
            }
        }
    }

    /// NAMING on `at` for the loaded project (`None`) or Part `part`'s Sound.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn rename_for_test(&mut self, at: ListAt, part: Option<PartId>, start: &str) {
        let f = part.map_or(NamingFor::RenameLoaded, NamingFor::RenamePart);
        self.name(at, f, start);
    }

    /// A frame for the prompt or NAMING.
    fn modal_input(&mut self, m: Modal, c: &impl Controls, p: &hold::Presses) {
        let key = |m: &Modal| match m {
            Modal::Prompt(a) => Some(a.key()),
            Modal::Naming(..) => None,
        };
        let was = key(&m);
        match m.input(c, p) {
            ModalStep::Open(m) => {
                self.browser_dirty |= self.loc.browse().is_some() && key(&m) != was;
                self.modal = Some(m);
            }
            ModalStep::Empty(m) => {
                self.modal = Some(m);
                self.toast.show(busy::Toast {
                    text: Line::new("NAME IS EMPTY"),
                    ms: busy::Toast::ERROR_MS,
                });
            }
            ModalStep::Done(d) => {
                self.closed();
                self.done(d);
            }
        }
    }

    /// A cancel drops what the prompt held.
    fn done(&mut self, d: Done) {
        use prompt::ReplaceAnswer as R;
        match d {
            Done::Answered(Answered::ReplacePart(p, Answer::Pick(R::Replace))) => {
                let was = self.active_engine();
                let _ = self.project.replace_part(p.anyway(&self.project));
                self.project_replaced(was);
            }
            // Task 13 (Pre-flight 15).
            Done::Answered(Answered::ReplacePart(_, Answer::Pick(R::SavePartFirst))) => {
                self.toast.show(busy::Toast {
                    text: Line::new(NOT_YET),
                    ms: busy::Toast::ERROR_MS,
                })
            }
            Done::Answered(Answered::ReplacePart(_, Answer::Pick(R::Cancel) | Answer::Cancel)) => {}
            Done::Named(NamingFor::RenameLoaded, name) => self.project.set_name(name),
            Done::Named(NamingFor::RenamePart(part), name) => {
                self.project.edit_part(part).sound.name = name
            }
            Done::Cancelled => {}
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
        self.refresh_status();

        let presses = self.gates.step(controls);
        if let Some(m) = self.modal.take() {
            if !part_key(controls) {
                self.modal_input(m, controls, &presses);
                return;
            }
            // Dropped: a pending replace with it.
            self.closed();
        }
        let keys = self.nav_keys(controls, presses);
        for k in keys.into_iter().flatten() {
            if let (Some((p, b)), NavKey::Edit) = (self.loc.browse(), k) {
                self.load_sound(p, b);
                continue;
            }
            let cx = self.cx();
            match self.loc.step(k, &cx, &mut self.recall) {
                Step::Go(to) => self.go(to),
                Step::Act(_) | Step::Screen(_) | Step::Run => self.toast.show(busy::Toast {
                    text: Line::new(NOT_YET),
                    ms: busy::Toast::ERROR_MS,
                }),
                Step::Stay => {}
            }
        }

        if let Some((p, b)) = self.loc.browse() {
            let moved = browser::input(controls, b);
            if moved != b {
                self.loc = Location::sound_at(p, moved);
                self.browser_dirty = true;
            }
            return;
        }
        if self.loc.page(&self.cx()).is_none() {
            return;
        }

        // Encoder deltas -> parameter changes
        let shift = matches!(
            controls.button_state(ButtonId::Mix),
            ButtonState::Pressed | ButtonState::Held
        );

        let def = self.page_def();
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
                if view::is_dimmed(&v, &self.project.part(at).sound) {
                    continue; // dimmed: the encoder is ignored
                }
                let params = &mut UiBlocks {
                    project: &mut self.project,
                    part: at,
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
            self.page = PageKey::from_location(self.loc, def, self.sel_op);

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
                    let sound = &self.project.part(at).sound;
                    if let Some(a) = v.addr()
                        && prime_target(a) == a
                        && view::is_dimmed(&v, sound)
                    {
                        self.prime_status = Some(PrimeStatus::NotModulatable);
                    } else if let Some(addr) = self.current_param_addr() {
                        let label = self.mod_label(addr);
                        let sound = self.project.edit_part(at).sound;
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
                    let sound = self.project.edit_part(at).sound;
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
        self.refresh_status();
        self.clock.tick();
        let at = self.active_part;

        // Read base param values
        let def = self.page_def();
        let mut values = self.display_values();
        // Set values, before any modulation offset.
        for (a, &v) in self.renderer.set.iter_mut().zip(values.iter()) {
            a.set_target(v);
            a.update();
        }
        let ctx = self.ctx();
        let sound = &self.project.part(at).sound;

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
        let sub = self.loc.page(&self.cx()).map_or(0, |(_, at)| at.sub());
        let needed_bottom = theme::BRANCH_START_Y + (sub as i32 + 1) * theme::BRANCH_LINE_HEIGHT;
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
    /// SETTINGS › SYSTEM › ABOUT's AUDIO sub-page.
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
        if let Some((part, b)) = self.loc.browse() {
            let _ = display.clear(theme::BG);
            let (cursor, scroll) = (b.cursor(), b.scroll());
            browser::draw(display, self.project.pool(), part, cursor, scroll);
            self.draw_prompt(display);
            return;
        }
        self.renderer
            .draw_with_def(display, &self.frame(perf, audio, scope));
    }

    /// The open prompt's panel, if any, over whatever is drawn.
    fn draw_prompt<D>(&self, d: &mut D)
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        if let Some(a) = self.prompt() {
            a.with_view(|v| prompt::draw_prompt(d, v));
        }
    }

    /// What one frame draws from.
    fn frame<'a>(
        &'a self,
        perf: &'a PerfStats,
        audio: Option<&'a AudioStats>,
        scope: &'a [f32; SCOPE_LEN],
    ) -> renderer::Frame<'a> {
        let cx = self.cx();
        let def = self.page_def();
        let settings = self.loc.settings();
        let head = match (settings, self.loc.part()) {
            (None, Some(p)) if self.loc.on_mixer() => Head::Mix(p),
            (None, Some(p)) => Head::Sound(p),
            _ => Head::Settings,
        };
        renderer::Frame {
            head,
            map: self.loc.page(&cx).filter(|_| settings.is_none()),
            layout: self.layout(),
            settings: self.bands(),
            prompt: self.prompt(),
            def,
            perf,
            matrix: &self.matrix_state,
            sel_op: self.sel_op,
            ctx: self.ctx(),
            focus: self.focused_slot(),
            scope,
            sounding: crate::scope::peak(scope) > crate::scope::SOUNDING_PEAK,
            parts: self.project.perf().parts(),
            active_part: self.active_part,
            prime_status: self.prime_status,
            audio,
            master_gr_db: crate::meter::MASTER_GR.read(),
            clock: self.clock,
            fx: &self.project.perf().fx,
        }
    }

    /// The bands on screen: a page's, or SETTINGS' list or leaf.
    fn layout(&self) -> region::Layout {
        let def = self.page_def();
        match self.loc.settings() {
            Some(s) => region::Layout::Settings(s.at_leaf().map(|_| def.layout)),
            None => region::Layout::Page(def.layout),
        }
    }

    /// Where the UI is as (place, node, sub) for the header and map keys:
    /// Part n's pages n, its mixer 10 + n, its Sound rung 30 + n, SETTINGS 20.
    fn loc_tag(&self) -> (u8, u8, u8) {
        let place = match (self.loc.settings(), self.loc.part()) {
            (Some(_), _) | (_, None) => 20,
            (_, Some(p)) if self.loc.on_mixer() => 10 + p.index() as u8,
            (_, Some(p)) if self.loc.browse().is_some() => 30 + p.index() as u8,
            (_, Some(p)) => p.index() as u8,
        };
        let at = self
            .loc
            .page(&self.cx())
            .map_or((0, 0), |(_, at)| (at.node(), at.sub()));
        (place, at.0, at.1)
    }

    /// Snapshot of what region `kind` shows; a region redraws when it changes.
    /// `shown` is the focus band's gauge this frame (`Renderer::shown_gauge`).
    fn region_data(
        &self,
        kind: region::RegionKind,
        f: &renderer::Frame,
        shown: Option<glyph::Gauge>,
    ) -> region::RegionData {
        use region::{RegionData, RegionKind};
        let qvalues = region::quantize_values(&self.renderer.anim);
        let audio_page = f.def.viz == VizType::AudioStats;
        let (chain, node, sub) = self.loc_tag();
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
            )
            .with_set(region::quantize(self.renderer.set[f.focus].current()))
            .animated(glyph::anim_key(
                shown.is_some_and(|g| g.animates()),
                f.clock.frame(),
            )),
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
            RegionKind::Crumbs | RegionKind::List | RegionKind::Footer => {
                let key = f.settings.map_or(0, |b| match kind {
                    RegionKind::Crumbs => b.crumbs_key(f.sounding),
                    RegionKind::List => b.list_key(),
                    _ => b.footer_key(),
                });
                RegionData::Settings { key }
            }
            RegionKind::Prompt => {
                let key = f.prompt.map_or(0, Ask::key);
                RegionData::Overlay { key }
            }
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
            .set_screen(self.layout(), self.prompt_open());
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, audio, scope);
            let shown = self.renderer.shown_gauge(&f);
            for (d, r) in data.iter_mut().zip(self.region_set.active_regions()) {
                *d = self.region_data(r.kind, &f, shown);
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
        // The Sound rung's browser: one flush region, redrawn only while
        // dirty (opened, cursor/scroll moved; #7).
        if let Some((part, b)) = self.loc.browse() {
            let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
            if self.browser_dirty {
                let fb = display.pixel_buffer();
                Renderer::clear_region_fb(fb, 0, chimera_hal::SCREEN_HEIGHT);
                let (cursor, scroll) = (b.cursor(), b.scroll());
                browser::draw(display, self.project.pool(), part, cursor, scroll);
                self.draw_prompt(display);
                // Invalidate region set so normal layout forces full rebuild on exit
                self.region_set.prev_screen = None;
                flush_list[0] = (0, chimera_hal::SCREEN_HEIGHT);
                self.browser_dirty = false;
            }
            return flush_list;
        }

        let layout = self.layout();
        let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
        let mut flush_count = 0;

        // Rebuild regions if layout changed
        let screen = (layout, self.prompt_open());
        let fresh = self.region_set.prev_screen != Some(screen);
        if fresh {
            self.region_set.set_screen(layout, screen.1);
        }
        // Beneath a prompt the screen is frozen: drawn once, then only the
        // panel redraws, when its key moves.
        let frozen = screen.1 && !fresh;

        let count = self.region_set.count as usize;
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, audio, scope);
            // The focus band's gauge, resolved once for the frame.
            let shown = self.renderer.shown_gauge(&f);
            for i in 0..count {
                let r = self.region_set.regions[i];
                let prompt = r.kind == region::RegionKind::Prompt;
                if frozen && !prompt {
                    data[i] = r.prev_data;
                    continue;
                }
                data[i] = self.region_data(r.kind, &f, shown);
                // Only an animated glyph moved: redraw its box alone.
                if r.kind == region::RegionKind::Focus
                    && data[i] != r.prev_data
                    && data[i].without_anim() == r.prev_data.without_anim()
                    && let Some(gauge) = shown
                    && let Some(rows) =
                        renderer::Renderer::redraw_gauge(display, |d| d.pixel_buffer(), gauge)
                {
                    flush_list[flush_count] = rows;
                    flush_count += 1;
                } else if data[i] != r.prev_data {
                    // The panel is opaque over its own last frame.
                    if !prompt {
                        renderer::Renderer::clear_region_fb(
                            display.pixel_buffer(),
                            r.y_start,
                            r.y_end,
                        );
                    }
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

/// B*n* or MIX+B*n*: the keys a prompt or NAMING lets through, after
/// dropping itself. EDIT+B*n* is theirs.
fn part_key(c: &impl Controls) -> bool {
    let down = |b| matches!(c.button_state(b), ButtonState::Pressed | ButtonState::Held);
    !down(ButtonId::Edit)
        && PART_BUTTONS
            .iter()
            .any(|&b| c.button_state(b) == ButtonState::Pressed)
}

/// Whether `controls` reports an encoder turn or a button press this frame —
/// any of which retires the last prime-status message (issue #21).
fn any_input(controls: &impl Controls) -> bool {
    ALL_ENCODERS.iter().any(|&e| controls.encoder_delta(e) != 0)
        || ALL_BUTTONS
            .iter()
            .any(|&b| controls.button_state(b) == ButtonState::Pressed)
}

/// A Part's blocks plus SETTINGS › THEME, so the theme edits through the same
/// slot bindings as any other. Only a block written takes `edit_part`, so
/// only a write moves the project's revision.
struct UiBlocks<'a> {
    project: &'a mut Project,
    part: PartId,
    theme: &'a mut ThemeSettings,
}

/// `UiBlocks`' shared twin: no `block_mut`, so a frame can't write.
struct UiRead<'a> {
    project: &'a Project,
    part: PartId,
    theme: &'a ThemeSettings,
}

/// Part `part`'s block `b`, or the theme: what both views read.
fn read_block<'a>(
    project: &'a Project,
    part: PartId,
    theme: &'a ThemeSettings,
    b: BlockRef,
) -> Option<&'a dyn Block> {
    match b {
        BlockRef::Theme => Some(theme),
        BlockRef::PartMix(p) => Some(project.read_part(p).mix),
        _ => project.read_part(part).block(b),
    }
}

impl BlockRead for UiBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        read_block(self.project, self.part, self.theme, b)
    }
}

impl Blocks for UiBlocks<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::Theme => Some(self.theme),
            BlockRef::PartMix(p) => Some(self.project.edit_part(p).mix),
            _ => {
                let PartEdit { sound, mix, fx } = self.project.edit_part(self.part);
                part_block_mut(sound, mix, fx, b)
            }
        }
    }
}

impl BlockRead for UiRead<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        read_block(self.project, self.part, self.theme, b)
    }
}

/// Display values for `page`: Part pages through slot bindings, legacy pages
/// through `PageId`.
fn page_values(page: PageKey, def: &BlockDef, params: &impl BlockRead, sel_op: Op) -> [f32; 6] {
    match page {
        PageKey::Part { .. } => part_page::read_values(def, params, sel_op),
        PageKey::Legacy(p) => p.read_values(params),
    }
}

fn nav_cx(project: &Project) -> NavCtx {
    NavCtx {
        engines: PartId::ALL.map(|p| project.part(p).sound.engine()),
        dyn_rows: 0,
    }
}

/// The page `at` shows; lists, Screens and the Sound rung show `NO_PAGE`.
fn page_def(at: Location, cx: &NavCtx) -> &'static BlockDef {
    at.page(cx)
        .and_then(|(c, p)| c.def_at(p))
        .unwrap_or(&NO_PAGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartParams;

    #[test]
    fn part_mix_edits_the_named_part() {
        let (mut project, _) = Project::boxed();
        let mut theme = ThemeSettings::default();
        let [p1, _, p3, ..] = PartId::ALL;
        let a = ParamAddr::new(BlockRef::PartMix(p3), PartParams::CHANNEL);
        let ch = |p: &Project, id| p.part(id).mix.get(PartParams::CHANNEL);
        let before = ch(&project, p1);
        let mut ui = UiBlocks {
            project: &mut project,
            part: p1,
            theme: &mut theme,
        };
        ui.block_mut(a.block).unwrap().set(a.param, 9.0);
        assert_eq!(ch(&project, p3), 9.0);
        assert_eq!(ch(&project, p1), before);
        let read = UiRead {
            project: &project,
            part: p1,
            theme: &theme,
        };
        assert_eq!(read.block(a.block).unwrap().get(a.param), 9.0);
    }
}
