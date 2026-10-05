pub mod about_page;
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
pub mod hint;
pub mod hold;
pub mod led;
pub mod midi_settings;
pub mod mod_grid;
pub mod mod_panel;
pub mod nav;
pub mod orbit;
pub mod page;
pub mod part_page;
pub mod perf;
pub mod region;
pub mod renderer;
pub mod reverb_panel;
pub mod settings;
pub mod splash;
pub mod theme;
pub mod theme_settings;
pub mod view;
pub mod viz;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::storage::{Card, CardEvent, Exit, SystemSettings, SystemSync};
use chimera_hal::store::Store;
use chimera_hal::{
    ALL_BUTTONS, ALL_ENCODERS, ButtonId, ButtonState, Controls, EncoderId, PART_BUTTONS,
};

use crate::addr::{BlockRead, BlockRef, Blocks, Op, ParamAddr};
use crate::block::Block;
use crate::boot::RomDfu;
use crate::dsp::lfo::Lfo;
use crate::dsp::modulator::{EnvSlot, EnvType, LfoSlot, LfoType};
use crate::dsp::reverb::KnobMemory;
use crate::in_place::{by_value, uninit_at};
use crate::mod_path::{LABEL_LEN, RegistryError};
use crate::modulation::{CUTOFF, CUTOFF_LABEL, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::ProjectName;
use crate::orbit::RingIdx;
use crate::orbit::chords::ScaleKind;
use crate::orbit::runner::{OrbitShared, OrbitView};
use crate::orbit::scene::{OrbitSong, Refused, SceneIdx, SceneSave};
use crate::params::{EngineType, ParamSnapshot};
use crate::perf::load::AudioStats;
use crate::preset::POOL_SIZE;
use crate::project::{
    self, CardOut, Confirmed, DeleteTarget, Line, LoadLink, PartEdit, PartFrom, PartId, PartSource,
    Project, ProjectFile, ProjectNote, ProjectSource, ProjectStatus, ReplaceGuard, SaveTo,
    SceneAsk, SceneSlot, StatusCache, Swap, TemplateCrc, part_block_mut,
};
use crate::scope::SCOPE_LEN;
use crate::storage::ProjectId;
use block_def::BlockDef;
use block_def::VizType;
use block_def::slot_addr;
use components::Head;
use fmt::FmtBuf;
use hold::{HoldGates, Press};
use midi_settings::MidiSettings;
use mod_grid::MatrixState;
use nav::{
    Browse, Column, ListAt, Location, NavCtx, NavKey, OrbitAct, OrbitCx, OrbitKey, Recall, Rung,
    SettingsAt, Step, chain_def_for,
};
use page::{PageKey, PageLayout};
use perf::PerfStats;
use renderer::Renderer;
use settings::job::{CardCx, Job, LoadAfter, ThenClear, load_job};
use settings::listing::{Listing, LoadRow, Pick, refusal};
use settings::manage::{Command, Run, Whose};
use settings::naming::Naming;
use settings::part::{Offer, PartCmd, SAVE_ROWS, slot_of};
use settings::prompt;
use settings::prompt::ReplaceTo;
use settings::replace::said::Said;
use settings::replace::{Asked, Guarded, PartAsk, ProjectAsk, Reply, said};
use settings::view::PartBand;
use settings::{
    Act, Answered, Ask, Done, Modal, ModalStep, NamingFor, PART_ROW, SaveAs, Screen, screen_path,
};
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
    /// SETTINGS › MIDI CONFIG's SYNC; boot sets it from SYSTEM (`set_midi`).
    midi: MidiSettings,
    /// What the last card operation said, for a moment.
    toast: busy::ToastTimer,
    /// `project_status`, refreshed by `handle_input` and `update` inside
    /// SETTINGS; render only reads it.
    status: StatusCache,
    /// Animation phase for the renderer.
    clock: animation::UiClock,
    /// A prompt or NAMING: while open it takes every key but B*n*.
    modal: Option<Modal>,
    /// The card's projects, as LOAD and MANAGE show them.
    listing: Listing,
    /// Card work for `card_work`, at most one a frame.
    job: Option<Job>,
    /// The card as `sync_system` last saw it, for ABOUT.
    card: Card,
    /// ORBIT's transport and the runner's last view.
    orbit: orbit::OrbitUi,
    /// The front-panel LED: ORBIT's beat.
    led: led::Led,
    /// OS UPGRADE's yes, until the shell takes it.
    dfu: Option<Said<RomDfu>>,
    /// Each reverb TYPE's knobs as last left: a TYPE nudge away and back
    /// loses nothing. RAM only; a project load clears it.
    rev_knobs: KnobMemory,
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
    midi,
    toast,
    status,
    clock,
    modal,
    listing,
    job,
    card,
    orbit,
    led,
    dfu,
    rev_knobs,
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
            let midi = MidiSettings::DEFAULT;
            let read = UiRead {
                project,
                part: PartId::ALL[0],
                theme: &theme,
                midi: &midi,
            };
            renderer.snap_to_current(page_values(page, def, &read, &SlotCtx::read(&read, Op::A)));
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
            addr_of_mut!((*p).midi).write(midi);
            addr_of_mut!((*p).toast).write(busy::ToastTimer::new());
            addr_of_mut!((*p).status).write(StatusCache::new());
            addr_of_mut!((*p).clock).write(animation::UiClock::new());
            addr_of_mut!((*p).modal).write(None);
            addr_of_mut!((*p).listing).write(Listing::new());
            addr_of_mut!((*p).job).write(None);
            addr_of_mut!((*p).card).write(Card::new());
            addr_of_mut!((*p).orbit).write(orbit::OrbitUi::new());
            addr_of_mut!((*p).led).write(led::Led::OFF);
            addr_of_mut!((*p).dfu).write(None);
            addr_of_mut!((*p).rev_knobs).write(KnobMemory::new());
            let ui = slot.assume_init_mut();
            ui.load_matrix(PartId::ALL[0]);
            ui
        }
    }

    /// Before a restart: SYSTEM synced to the card now, as leaving SETTINGS
    /// would. A restart never leaves SETTINGS, so a THEME change made in
    /// this visit would otherwise be lost. Writes the card.
    pub fn sync_system_now<S: Store>(
        &mut self,
        sync: &mut SystemSync,
        card: &mut Card,
        store: &mut S,
        s: &mut SystemSettings,
    ) {
        s.theme = self.theme;
        // As if SETTINGS were left: `left_system` keeps the untouched mark.
        let _ = sync.left_system(false, s);
        let _ = sync.on_exit(card, store, s, self.project.meta().file());
        self.card = *card;
    }

    /// The DFU prompt's yes, once, after `sync_system_now`: the menu's
    /// path to DFU can't skip the card write.
    pub fn take_dfu_synced<S: Store>(
        &mut self,
        sync: &mut SystemSync,
        card: &mut Card,
        store: &mut S,
        s: &mut SystemSettings,
    ) -> Option<Said<RomDfu>> {
        let yes = self.dfu.take()?;
        self.sync_system_now(sync, card, store, s);
        Some(yes)
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

    /// SETTINGS › MIDI CONFIG as last edited.
    pub fn midi(&self) -> MidiSettings {
        self.midi
    }

    /// The MIDI CONFIG SYSTEM held, set at boot before the first frame.
    pub fn set_midi(&mut self, m: MidiSettings) {
        self.midi = m;
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
        s.midi = self.midi;
        self.card = *card;
        if !sync.left_system(self.in_settings(), s) {
            return;
        }
        let r = sync.on_exit(card, store, s, self.project.meta().file());
        self.card = *card;
        if r == Ok(Exit::Loaded) {
            self.adopt_system(s);
        }
        if let Some(t) = busy::toast_for(&r) {
            self.toast.show(t);
        }
    }

    /// A start-up fault as a toast, held `Toast::BOOT_MS`.
    pub fn show_boot_fault(&mut self, text: &str) {
        self.toast.show(busy::Toast {
            text: Line::new(text),
            ms: busy::Toast::BOOT_MS,
        });
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
        self.orbit.booted(self.project.orbit_mut());
        self.rev_knobs.clear();
        self.project_replaced(was);
    }

    /// SAVE, a first save / SAVE AS or SAVE OVER (`SaveTo`). A save that
    /// lands becomes SYSTEM's last project. A bare file isn't a place to
    /// save:
    ///
    /// ```compile_fail,E0308
    /// # use chimera_core::storage::{Card, SystemSync};
    /// # let mut s = chimera_hal::testkit::MemStore::new(1);
    /// # let mut card = Card::new();
    /// # let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    /// let mut ui = chimera_core::ui::UiState::new();
    /// let to = chimera_core::project::new_project_id(&mut card, &mut s).out.unwrap().file();
    /// ui.save_project(&mut card, &mut s, &mut sync, &mut set, to);
    /// ```
    pub fn save_project<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        to: SaveTo,
    ) -> Option<CardEvent> {
        let name = self.project.meta().name();
        let CardOut { out, event } = self.save(card, store, sync, settings, to, name);
        self.show_note(out);
        event
    }

    /// `save_project` under `name`, without the toast.
    fn save<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        to: SaveTo,
        name: ProjectName,
    ) -> CardOut<ProjectNote> {
        let n = project::save_project_as(card, store, &mut self.project, to, name);
        if let (ProjectNote::Saved(_), Some(f)) = (n.out, self.project.meta().file()) {
            self.remember(card, store, sync, settings, f);
        }
        n
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
        publish: impl FnOnce(Swap, &UiState) -> R,
    ) -> Option<R> {
        let mut cx = CardCx {
            card,
            store,
            sync,
            settings,
        };
        self.load_with(&mut cx, go, link, publish).0
    }

    /// `load_project`, and what its mount found.
    fn load_with<S: Store, R>(
        &mut self,
        cx: &mut CardCx<'_, S>,
        go: Confirmed<ProjectSource>,
        link: &LoadLink,
        publish: impl FnOnce(Swap, &UiState) -> R,
    ) -> (Option<R>, Option<CardEvent>) {
        let was = self.active_engine();
        let out = project::load_project(cx.card, cx.store, &mut self.project, go, link);
        if let Some(n) = out.note {
            self.show_note(n);
        }
        let Some(swap) = out.swap else {
            return (None, out.event);
        };
        // Before the publish: it carries the load to the runner.
        self.orbit.loaded(link.epoch(), self.project.orbit_mut());
        let published = publish(swap, self);
        // A project stores only its TYPE's knobs (reverb TYPEs spec § A
        // TYPE change loads that TYPE's knobs).
        self.rev_knobs.clear();
        self.project_replaced(was);
        // Only a file load sets it: NEW, loaded or fallen back to, has none.
        if let Some(f) = self.project.meta().file() {
            self.remember(cx.card, cx.store, cx.sync, cx.settings, f);
        }
        (Some(published), out.event)
    }

    /// Deletes the confirmed file (`project::delete_project`); a refusal or a card
    /// error shows its note. If SYSTEM names it as its card's last project
    /// (possible after `+ NEW`), it names none, so the next boot is NEW,
    /// not PROJECT NOT FOUND.
    pub fn delete_project<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        sync: &mut SystemSync,
        settings: &mut SystemSettings,
        c: Confirmed<DeleteTarget>,
    ) -> Option<CardEvent> {
        let file = c.target().file();
        let CardOut { out, event } = project::delete_project(card, store, &self.project, c);
        match out {
            // A failed write: the next save, load or SETTINGS exit retries
            // SYSTEM; until then a boot falls back to NEW and says why.
            Ok(()) => {
                if sync.forget(card, store, settings, file) == Ok(Exit::Loaded) {
                    self.adopt_system(settings);
                }
            }
            Err(n) => self.show_note(n),
        }
        event
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
            self.adopt_system(settings);
        }
    }

    /// A card's SYSTEM, taken: its theme and MIDI CONFIG.
    fn adopt_system(&mut self, s: &SystemSettings) {
        self.theme = s.theme;
        self.midi = s.midi;
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
        let mut cx = nav_cx(&self.project);
        cx.dyn_rows = self.screen().map_or(0, |s| self.screen_rows(s)) as u8;
        cx
    }

    /// The Screen on show, if any.
    fn screen(&self) -> Option<Screen> {
        self.loc.settings().and_then(|s| s.screen())
    }

    /// A Screen's rows, from the listing.
    fn screen_rows(&self, s: Screen) -> usize {
        match s {
            Screen::LoadProject => self.listing.load_rows(),
            Screen::ManageProjects => self.listing.len(),
            Screen::SaveToProj => SAVE_ROWS.len(),
            #[cfg(feature = "orbit")]
            Screen::SceneSaveTo => SceneIdx::ALL.len(),
        }
    }

    /// The card's projects as last listed.
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// LOAD PROJECT's row `i` as drawn.
    pub fn load_row(&self, i: usize) -> Option<LoadRow> {
        self.listing.load_row(i, self.project.meta().file())
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
            let len = match s.screen() {
                Some(sc) => self.screen_rows(sc),
                None => settings::rows(s.path()).len(),
            };
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

    /// Each reverb TYPE's knobs as last left.
    pub fn rev_knobs(&self) -> &KnobMemory {
        &self.rev_knobs
    }

    /// The knob memory, to fill before a load (tests only).
    #[cfg(any(test, feature = "test-support"))]
    pub fn rev_knobs_mut(&mut self) -> &mut KnobMemory {
        &mut self.rev_knobs
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

    /// The project's status now: hashes when the cache is behind (outside
    /// SETTINGS). Render reads the cache instead.
    pub fn project_status(&self) -> ProjectStatus {
        self.status.peek(&self.project, self.template)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn status_hashes_for_test(&self) -> u32 {
        self.status.hashes_for_test()
    }

    /// The status SETTINGS' footer draws; `None` outside SETTINGS.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn footer_status(&self) -> Option<ProjectStatus> {
        self.bands().map(|b| b.status)
    }

    /// Only SETTINGS draws the status, so only SETTINGS hashes, and only
    /// when the revision moved: `update` catches up on its first frame.
    fn refresh_status(&mut self) {
        if self.in_settings() {
            self.status.get(&self.project, self.template);
        }
    }

    /// Part `part`'s blocks and SETTINGS › THEME, read only: reading leaves
    /// the project's revision alone.
    fn read(&self, part: PartId) -> UiRead<'_> {
        UiRead {
            project: &self.project,
            part,
            theme: &self.theme,
            midi: &self.midi,
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

    /// What the edited Part's page slots resolve against, on the ORBIT
    /// ring the location selects and under ORBIT's clock.
    fn ctx(&self) -> SlotCtx {
        let ctx = SlotCtx::read(
            &self.project.part(self.active_part).sound.params,
            self.sel_op,
        )
        .on_clock(self.orbit.status.source)
        .on_rev(&self.project.perf().fx.reverb);
        match self.loc.orbit() {
            Some(o) => ctx.on_ring(o.ring()),
            None => ctx,
        }
    }

    /// Recompute the page identity and jump the display to its values.
    fn enter_page(&mut self) {
        self.page = PageKey::from_location(self.loc, self.page_def(), self.sel_op);
        let values = self.display_values();
        self.renderer.snap_to_current(values);
        self.renderer.orbit_cursor.snap(self.orbit.cursor as f32);
    }

    /// The six values the display animates toward: the page's slots, and on
    /// the mod matrix the selected route's amount in slot e.
    fn display_values(&self) -> [f32; 6] {
        let def = self.page_def();
        let ctx = self.ctx();
        let mut values = page_values(self.page, def, &self.read(self.active_part), &ctx);
        for (i, value) in values.iter_mut().enumerate() {
            if let View::Route { source, .. } = view::view(def, i, &ctx) {
                *value = renderer::amount_value(
                    self.matrix_state.route(source.index(), CUTOFF).unwrap_or(0),
                );
            }
        }
        let song = self.project.orbit();
        for (i, value) in values.iter_mut().enumerate() {
            if let View::Orbit(c) = view::view(def, i, &ctx) {
                *value = c.value(song, &self.orbit, ctx.ring);
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
        let Some(from) = from else { return };
        // An empty slot has nothing to ask about.
        if let PartFrom::Slot(s) = from
            && self.project.pool().get(s).is_none()
        {
            let engine = self.project.part(part).sound.engine();
            return self.go(Location::part_home(part, engine));
        }
        let to = match from {
            PartFrom::Slot(_) => ReplaceTo::Sound,
            PartFrom::Init(_) => ReplaceTo::Init,
        };
        self.guarded_part(PartSource { part, from }, to);
    }

    /// `src` through the guard, or its prompt.
    fn guarded_part(&mut self, src: PartSource, to: ReplaceTo) {
        match ReplaceGuard::check(&self.project, self.template, src) {
            Ok(c) => self.replace(c),
            Err(n) => self.ask(Ask::ReplacePart(PartAsk::new(n), to)),
        }
    }

    /// A confirmed replace; on the Sound rung, then the Part's pages. A
    /// refused one says why and stays.
    fn replace(&mut self, c: Confirmed<PartSource>) {
        let part = c.target().part;
        let was = self.active_engine();
        if let Err(e) = self.project.replace_part(c) {
            return self.error(Line::new(prompt::refused(&e)));
        }
        match self.loc.browse() {
            Some(_) => self.go(Location::part_home(
                part,
                self.project.part(part).sound.engine(),
            )),
            None => self.project_replaced(was),
        }
    }

    /// SAVE PART FIRST: the save `save` picks, then the replace it was
    /// asked for. With none, the prompt stays.
    fn save_first<A: Guarded<Confirms = PartSource>>(
        &mut self,
        ask: Asked<PartSource, A>,
        save: fn(&Offer) -> Option<project::PartAction>,
        again: impl Fn(Asked<PartSource, A>) -> Ask,
    ) {
        let Some(a) = save(&Offer::of(&self.project, ask.source().part)) else {
            self.error(Line::new(prompt::POOL_FULL));
            return self.ask(again(ask));
        };
        let stale = self.save_sound(a);
        match ask.save_then(&self.project, self.template) {
            Ok(c) => {
                self.replace(c);
                if let Some(x) = stale {
                    self.ask(x);
                }
            }
            Err(ask) => self.ask(again(ask)),
        }
    }

    /// A save of a Part, said; what it leaves Stale, to ask about.
    fn save_sound(&mut self, a: project::PartAction) -> Option<Ask> {
        let slot = slot_of(a.kind());
        let stale = self.project.apply_part_action(a).ok()?;
        self.toast.show(busy::Toast {
            text: prompt::saved_to(slot),
            ms: busy::Toast::SAVED_MS,
        });
        Ask::update_stale(stale, slot, &self.project)
    }

    /// The active Part's `c`, if `part_actions` offers it: SAVE TO PROJ's
    /// rows and RELOAD.
    fn run_part_cmd(&mut self, c: PartCmd) {
        let Some(a) = Offer::of(&self.project, self.active_part).get(c) else {
            return;
        };
        match c {
            PartCmd::Reload => {
                let s = slot_of(a.kind());
                let src = PartSource {
                    part: self.active_part,
                    from: PartFrom::Slot(s),
                };
                match ReplaceGuard::check(&self.project, self.template, src) {
                    Ok(c) => self.replace(c),
                    Err(n) => self.ask(Ask::ReloadPart(Asked::new(n), s)),
                }
            }
            PartCmd::OverSlot | PartCmd::NewSlot => {
                if let Some(x) = self.save_sound(a) {
                    self.ask(x);
                }
            }
        }
    }

    /// MIX + EDIT on the Sound rung: Part `p`'s Sound over its slot, else
    /// to a new one, as SAVE TO PROJ's rows save it (ADR 0081).
    fn save_part(&mut self, p: PartId) {
        match Offer::of(&self.project, p).first_save() {
            Some(a) => {
                if let Some(x) = self.save_sound(a) {
                    self.ask(x);
                }
            }
            None => self.error(Line::new(prompt::POOL_FULL)),
        }
    }

    /// EDIT on an action row; `at` is its list.
    fn run_act(&mut self, a: Act, at: Option<SettingsAt>) {
        if let Some(c) = a.part_cmd() {
            return self.run_part_cmd(c);
        }
        match a {
            Act::SaveProjectAs => self.queue(Job::Fresh(None)),
            Act::PartRename => {
                if let Some(at) = at.and_then(|s| s.list()) {
                    let p = self.active_part;
                    let n = self.project.part(p).sound.name;
                    self.name(at, NamingFor::RenamePart(p), n.as_str());
                }
            }
            Act::PartClear => self.clear_part(),
            // `part_cmd`'s.
            Act::PartReload => {}
            Act::SceneClear => self.ask_scene(false, |a| Ask::ClearScene(a, prompt::Choice::new())),
            Act::SceneReload => {
                self.ask_scene(true, |a| Ask::ReloadScene(a, prompt::Choice::new()))
            }
            Act::EnterDfu => self.ask(Ask::EnterDfu(prompt::Choice::new())),
        }
    }

    /// SETTINGS › ORBIT's CLEAR or RELOAD, `open` on the playing scene:
    /// refused while a switch waits, as SCN's SAVE is. RELOAD (`dims`)
    /// with nothing to drop is dimmed, and inert before that: a dimmed row
    /// says nothing, whatever waits.
    fn ask_scene(&mut self, dims: bool, open: fn(SceneAsk) -> Ask) {
        let song = self.project.orbit();
        if dims && !song.edited() {
            return;
        }
        if song.queued().is_some() {
            return self.error(Line::new(prompt::SWITCH_PENDING));
        }
        self.ask(open(SceneAsk::of(song, song.current())));
    }

    /// CLEAR: the active Part to its engine's INIT, through the guard.
    fn clear_part(&mut self) {
        let src = PartSource {
            part: self.active_part,
            from: PartFrom::Init(self.active_engine()),
        };
        self.guarded_part(src, ReplaceTo::Init);
    }

    /// MIX+MINUS on the Sound rung: a filled slot no Part plays, asked.
    fn ask_clear_slot(&mut self, b: Browse) {
        let Some(s) = browser::slot_at(b.cursor()) else {
            return;
        };
        if self.project.pool().get(s).is_none() {
            return;
        }
        match self.project.users(s) {
            u if u.is_empty() => self.ask(Ask::ClearSlot(s, prompt::Choice::new())),
            u => self.error(prompt::in_use(u)),
        }
    }

    fn error(&mut self, text: Line) {
        self.toast.show(busy::Toast {
            text,
            ms: busy::Toast::ERROR_MS,
        });
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
        #[cfg(feature = "orbit")]
        if presses.seq == Some(Press::Hold) {
            push(NavKey::SeqHold);
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

    /// ORBIT's keys this frame, in order: MENU, B1–B5, B6, MINUS, PLUS,
    /// SEQ, EDIT. MIX + B*n* is a mute on the press, its gate muted, so no
    /// ring tap or cross follows; B*n* alone is a ring on a tap, a cross
    /// on a hold.
    fn orbit_keys(&self, c: &impl Controls, p: hold::Presses) -> [Option<OrbitKey>; 11] {
        let pressed = |b| c.button_state(b) == ButtonState::Pressed;
        let mix = c.edges(ButtonId::Mix).down;
        let mut keys = [None; 11];
        let mut n = 0;
        let mut push = |k| {
            keys[n] = Some(k);
            n += 1;
        };
        if p.menu == Some(Press::Tap) {
            push(OrbitKey::MenuTap);
        }
        for ((r, &b), press) in RingIdx::ALL.into_iter().zip(&PART_BUTTONS).zip(p.ring) {
            match (mix && pressed(b), press) {
                (true, _) => push(OrbitKey::Mute(r)),
                (_, Some(Press::Tap)) => push(OrbitKey::Ring(r)),
                (_, Some(Press::Hold)) => push(OrbitKey::Cross(r)),
                _ => {}
            }
        }
        if !mix && pressed(ButtonId::B6) {
            push(OrbitKey::PlayStop);
        }
        // MIX + PLUS and MIX + MINUS stay free, as on the ladder.
        if !mix && pressed(ButtonId::Minus) {
            push(OrbitKey::Minus);
        }
        if !mix && pressed(ButtonId::Plus) {
            push(OrbitKey::Plus);
        }
        if p.seq == Some(Press::Tap) {
            push(OrbitKey::SeqTap);
        }
        if pressed(ButtonId::Edit) {
            push(OrbitKey::Edit);
        }
        keys
    }

    /// One frame's gates and ORBIT keys, as `handle_input` reads them.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn orbit_frame_for_test(&mut self, c: &impl Controls) -> [Option<OrbitKey>; 11] {
        let p = self.gates.step(c);
        self.orbit_keys(c, p)
    }

    /// Go straight to `to`, as a key would.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn go_for_test(&mut self, to: Location) {
        self.go(to)
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

    fn bands(&self) -> Option<settings::view::Bands<'_>> {
        self.loc.settings().map(|at| self.bands_at(at))
    }

    /// SETTINGS' breadcrumb at `at`, as drawn there: an open NAMING's
    /// crumb ends it.
    pub fn crumbs_at(&self, at: SettingsAt) -> settings::view::Crumbs {
        self.bands_at(at).crumbs()
    }

    fn bands_at(&self, at: SettingsAt) -> settings::view::Bands<'_> {
        use settings::view::BandsModal;
        settings::view::Bands {
            at,
            active: self.active_part,
            first: self.list_first as usize,
            name: self.project.meta().name(),
            status: self.status.cached(),
            modal: self.modal.as_ref().map(|m| match m {
                Modal::Prompt(_) => BandsModal::Prompt,
                Modal::Naming(f, n) => BandsModal::Naming { naming: n, of: f },
            }),
            listing: &self.listing,
            loaded: self.project.meta().file(),
            part: (at.path().first() == Some(&PART_ROW))
                .then(|| PartBand::of(&self.project, self.active_part)),
            scene: self.project.orbit().current(),
            orbit: self.orbit_band(at),
        }
    }

    /// In the ORBIT branch, the playing scene's band.
    #[cfg(feature = "orbit")]
    fn orbit_band(&self, at: SettingsAt) -> Option<settings::view::OrbitBand> {
        (at.path().first() == Some(&settings::ORBIT_ROW))
            .then(|| settings::view::OrbitBand::of(self.project.orbit()))
    }

    #[cfg(not(feature = "orbit"))]
    fn orbit_band(&self, _: SettingsAt) -> Option<settings::view::OrbitBand> {
        None
    }

    /// NAMING, if it is open.
    pub fn naming(&self) -> Option<&Naming> {
        match &self.modal {
            Some(Modal::Naming(_, n)) => Some(n),
            _ => None,
        }
    }

    fn ask(&mut self, a: Ask) {
        self.open(Modal::Prompt(a));
    }

    /// NAMING takes a list's band, so it opens on one: `at`.
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

    /// The open prompt's question and reason.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn prompt_words_for_test(&self) -> Option<(Line, Line)> {
        self.prompt()
            .map(|a| a.with_view(|v| (Line::new(v.question), Line::new(v.reason))))
    }

    /// Which prompt is open.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn prompt_kind_for_test(&self) -> Option<settings::AskKind> {
        self.prompt().map(Ask::kind)
    }

    /// SETTINGS' footer legend as drawn.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn legend_for_test(&self) -> Option<&'static str> {
        self.bands().map(|b| b.legend())
    }

    /// A replace from `src` that asks opens its prompt; whether it did.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn ask_replace_for_test(&mut self, src: PartSource) -> bool {
        match ReplaceGuard::check(&self.project, self.template, src) {
            Ok(_) => false,
            Err(n) => {
                self.ask(Ask::ReplacePart(PartAsk::new(n), ReplaceTo::Sound));
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
        // Only the Sound rung's browser redraws for a prompt's change.
        let sound = self.loc.browse().is_some();
        let key = |m: &Modal| match m {
            Modal::Prompt(a) if sound => Some(a.key()),
            _ => None,
        };
        let was = key(&m);
        match m.input(c, p, &self.project) {
            ModalStep::Open(m) => {
                self.browser_dirty |= key(&m) != was;
                self.modal = Some(m);
            }
            ModalStep::Refused(f, n) => {
                self.modal = Some(Modal::Naming(f, n));
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
        use project::{DeleteTarget, OverwriteTarget};
        use prompt::{
            AlsoUsesAnswer as U, CardChangedAnswer as CC, ClearAnswer as C, NameExistsAnswer as N,
        };
        let Done::Answered(a) = d else {
            return self.named(d);
        };
        match a {
            Answered::ReplacePart(r, to) => match r {
                Reply::Confirmed(c) => self.replace(c),
                Reply::SaveFirst(ask) => {
                    self.save_first(ask, Offer::first_save, |x| Ask::ReplacePart(x, to))
                }
                Reply::Cancel => {}
            },
            Answered::ReloadPart(r, s) => match r {
                Reply::Confirmed(c) => self.replace(c),
                Reply::SaveFirst(ask) => {
                    // Over its own slot, the reload would undo nothing and
                    // leave the slot's other Parts Stale.
                    let new_slot = |o: &Offer| o.get(PartCmd::NewSlot);
                    self.save_first(ask, new_slot, |x| Ask::ReloadPart(x, s))
                }
                Reply::Cancel => {}
            },
            Answered::UpdateStale(reverts, a) => {
                if a.picked() == Some(U::Update) {
                    // Each as offered: a Part edited since refuses its own.
                    for r in reverts.into_iter().flatten() {
                        let _ = self.project.apply_part_action(r);
                    }
                }
            }
            Answered::ClearSlot(s, a) => {
                if a.picked() == Some(C::Clear)
                    && let Err(project::InUse(u)) = self.project.pool_clear(s)
                {
                    self.error(prompt::in_use(u));
                }
            }
            Answered::LoadProject { reply, to, clear } => match reply {
                Reply::Confirmed(c) => self.queue(load_job(c, clear)),
                Reply::SaveFirst(ask) => {
                    // Clearing its own file: the edits go to a new one,
                    // so the file the clear confirmed is untouched.
                    let own = self.project.meta().file().is_some() && clear.is_none();
                    let then = Some(LoadAfter {
                        pending: ask.into_pending(),
                        to,
                        clear,
                    });
                    self.queue(if own {
                        Job::QuickSave(then)
                    } else {
                        Job::Fresh(then)
                    })
                }
                Reply::Cancel => {}
            },
            Answered::NameExists(save, entry, a) => {
                let to = match (said(a), a.picked()) {
                    (Some(yes), _) => {
                        SaveTo::Over(Confirmed::<OverwriteTarget>::answered(&entry, yes))
                    }
                    (None, Some(N::KeepBoth)) => SaveTo::Fresh(save.fresh),
                    (None, _) => return,
                };
                self.queue(Job::Save(to, save.name, save.then));
            }
            Answered::CardChanged(then, a) => {
                if a.picked() == Some(CC::SaveAs) {
                    self.queue(Job::Fresh(then));
                }
            }
            Answered::Delete(c, a) => {
                if let Some(yes) = said(a) {
                    self.queue(Job::Delete(Confirmed::<DeleteTarget>::answered(
                        c.entry(),
                        yes,
                    )))
                }
            }
            Answered::SaveOver(c, a) => {
                if let Some(yes) = said(a) {
                    let over = Confirmed::<OverwriteTarget>::answered(c.entry(), yes);
                    let name = self.project.meta().name();
                    self.queue(Job::Save(SaveTo::Over(over), name, None))
                }
            }
            Answered::SaveSceneOver(ask, a) => {
                if let Some(yes) = said(a) {
                    let n = ask.slot().index();
                    self.scene_answered(Confirmed::said(ask, yes), |ui, yes| {
                        ui.project.orbit_mut().overwrite_scene(yes)?;
                        ui.scene_saved(n);
                        Ok(())
                    });
                }
            }
            Answered::ClearScene(ask, a) => {
                if let Some(yes) = said(a) {
                    self.scene_answered(Confirmed::said(ask, yes), |ui, yes| {
                        ui.relanding(|s| s.clear_scene(yes))
                    });
                }
            }
            Answered::ReloadScene(ask, a) => {
                if let Some(yes) = said(a) {
                    self.scene_answered(Confirmed::said(ask, yes), |ui, yes| {
                        ui.relanding(|s| s.reload_scene(yes))
                    });
                }
            }
            Answered::Clear(c, a) => {
                if let Some(yes) = said(a) {
                    let over = Confirmed::<OverwriteTarget>::answered(c.entry(), yes);
                    match Whose::of(c.entry(), self.project.meta().file()) {
                        Whose::Other => self.queue(Job::Clear(over)),
                        Whose::Loaded => self.guarded_load(ProjectSource::New, None, Some(over)),
                    }
                }
            }
            Answered::EnterDfu(a) => self.dfu = said(a),
        }
    }

    /// NAMING's answer.
    fn named(&mut self, d: Done) {
        match d {
            Done::Named(NamingFor::RenameLoaded, name) => self.project.set_name(name),
            Done::Named(NamingFor::RenamePart(part), name) => {
                self.project.edit_part(part).sound.name = name
            }
            Done::Named(NamingFor::SaveAs(fresh, then), name) => {
                self.queue(Job::Named(SaveAs { fresh, name, then }))
            }
            Done::Cancelled | Done::Answered(_) => {}
        }
    }

    /// Card work for `card_work`: one a frame, the first asked.
    fn queue(&mut self, j: Job) {
        if self.job.is_none() {
            self.job = Some(j);
        }
    }

    /// MENU hold: SAVE over the project's own file; NEW goes to SAVE AS.
    fn quick_save(&mut self) {
        self.queue(Job::QuickSave(None));
    }

    /// EDIT on LOAD PROJECT's row `row`.
    fn run_load(&mut self, row: u8) {
        match self.listing.load_pick(row as usize) {
            Some(Pick::Entry(e)) => self.load_entry(&e),
            Some(Pick::CreateNew) => self.guarded_load(ProjectSource::New, None, None),
            Some(Pick::Retry) => self.queue(Job::List(Screen::LoadProject)),
            Some(Pick::Inert) | None => {}
        }
    }

    /// A listed file's load; an errored one says why.
    fn load_entry(&mut self, e: &project::ProjectEntry) {
        if let Some(n) = refusal(e) {
            return self.show_note(n);
        }
        let src = ProjectSource::File {
            id: e.id,
            vol: e.vol,
        };
        self.guarded_load(src, e.name, None)
    }

    /// `src` through the guard, or its prompt; `clear` follows the load.
    fn guarded_load(&mut self, src: ProjectSource, to: Option<ProjectName>, clear: ThenClear) {
        match ReplaceGuard::check(&self.project, self.template, src) {
            Ok(c) => self.queue(load_job(c, clear)),
            Err(n) => {
                let current = self.project.meta().name();
                self.ask(Ask::LoadProject {
                    ask: ProjectAsk::new(n),
                    to,
                    current,
                    clear,
                })
            }
        }
    }

    /// EDIT on MANAGE's command column: the command the table lets run on
    /// the listed project under the bar.
    fn run_manage(&mut self, s: SettingsAt) {
        let Some(Column::Command(i)) = s.column() else {
            return;
        };
        let (Some(cmd), Some(c)) = (Command::at(i), self.listing.chosen(s.row() as usize)) else {
            return;
        };
        let w = Whose::of(c.entry(), self.project.meta().file());
        match cmd.on(w) {
            Ok(Run::Load) => self.load_entry(c.entry()),
            Ok(Run::SaveOver) => self.ask(Ask::SaveOver(c, prompt::Choice::new())),
            Ok(Run::RenameLoaded) => {
                if let Some(at) = s.list() {
                    let n = self.project.meta().name();
                    self.name(at, NamingFor::RenameLoaded, n.as_str());
                }
            }
            Ok(Run::Clear) => self.ask(Ask::Clear(c, prompt::Choice::new())),
            Ok(Run::Delete) => self.ask(Ask::Delete(c, prompt::Choice::new())),
            Err(_) => {}
        }
    }

    /// MANAGE's command `c` as drawn for the project under the bar.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn manage_command_row(&self, c: Command) -> Option<settings::view::ListRow<'static>> {
        let s = self.loc.settings()?;
        s.column()?;
        let e = self.listing.entry(s.row() as usize);
        Some(c.row(e.map(|e| Whose::of(&e, self.project.meta().file()))))
    }

    /// ORBIT's frame: its keys, stepped as `OrbitKey`s, then its encoders
    /// on the page's param cells. Its acts run in `OrbitUi::act`.
    fn orbit_input(&mut self, c: &impl Controls, p: hold::Presses) {
        for k in self.orbit_keys(c, p).into_iter().flatten() {
            if self.job.is_some() || self.modal.is_some() {
                return;
            }
            let cx = self.cx();
            match self.loc.step_orbit(k, &cx, &mut self.recall) {
                Step::Go(to) => self.go(to),
                Step::Orbit(a) => match a {
                    OrbitAct::PlayStop => self.orbit.play_stop(),
                    OrbitAct::Mute(r) => orbit::OrbitUi::mute(r, &mut self.project),
                    OrbitAct::SaveTo => self.save_scene_to(),
                },
                _ => {}
            }
        }
        let (def, ctx) = (self.page_def(), self.ctx());
        let shift = c.edges(ButtonId::Mix).down;
        let custom = |p: &Project| p.orbit().live().chords.scale == ScaleKind::Custom;
        let was_custom = custom(&self.project);
        for (i, &enc) in ALL_ENCODERS.iter().enumerate() {
            let delta = c.encoder_delta(enc);
            let v = view::view(def, i, &ctx);
            if delta == 0 || v == View::Empty {
                continue;
            }
            self.focus.touch(def.id, i);
            if let View::Orbit(cell) = v {
                let song = self.project.orbit_mut();
                let mut ring = ctx.ring;
                cell.turn(delta, shift, song, &mut self.orbit, &mut ring);
                // RING picks another ring: a move, on the same page.
                if ring != ctx.ring
                    && let Some(o) = self.loc.orbit()
                {
                    self.go(o.with_ring(ring).into());
                }
                continue;
            }
            // Dimmed: BPM under a MIDI clock.
            if view::is_dimmed(&v, &self.project.part(self.active_part).sound, &ctx) {
                continue;
            }
            // A scene's param: on `live` and its slot alike (ADR 0083).
            if let Some(a) = slot_addr(def, i, &ctx)
                && matches!(a.block, BlockRef::OrbitChords | BlockRef::OrbitRing(_))
            {
                self.project.orbit_mut().turn(a, delta, shift);
                continue;
            }
            let params = &mut UiBlocks {
                project: &mut self.project,
                part: self.active_part,
                theme: &mut self.theme,
                midi: &mut self.midi,
            };
            if shift {
                part_page::snap_under(def, i, delta, params, &ctx);
            } else {
                part_page::apply_under(def, i, delta, params, &ctx, &mut self.sel_op);
            }
        }
        // SEGS shrunk under an armed segment drops the capture.
        self.orbit.fit(&self.project.orbit().live().chords);
        // SCALE turned to CUSTOM opens its circle; a toggle on it stays.
        if !was_custom
            && custom(&self.project)
            && let Some(o) = self.loc.orbit()
            && o != o.custom()
        {
            self.go(o.custom().into());
        }
    }

    /// ORBIT's settings for the audio thread, into a slot of its triple
    /// buffer, in the same publish as the Parts.
    pub fn publish_orbit(&self, out: &mut OrbitShared) {
        self.orbit
            .publish(self.project.orbit(), self.midi.sync, out);
    }

    /// SETTINGS › ORBIT's CLEAR or RELOAD, answered: `act` on the song;
    /// a re-landing it queues goes to the runner as a switch.
    fn relanding(
        &mut self,
        act: impl FnOnce(&mut OrbitSong) -> Result<(), Refused>,
    ) -> Result<(), Refused> {
        let song = self.project.orbit_mut();
        act(song)?;
        if song.queued().is_some() {
            self.orbit.issue_scene(song);
        }
        Ok(())
    }

    /// ORBIT's edits and commands into the log, ready for the publish:
    /// the end of every frame's input (ADR 0082).
    pub fn flush_orbit(&mut self) {
        self.orbit.flush(&mut self.project);
    }

    /// The runner's latest view, each loop before `handle_input`: a
    /// capture taken into the song, the view kept for the pages, its drift
    /// absorbed into the project once a loop.
    pub fn set_orbit_view(&mut self, v: &OrbitView) {
        self.orbit.sync(v, &mut self.project);
    }

    /// The front-panel LED: lit on ORBIT's beat, off when it is stopped.
    pub fn led(&self) -> bool {
        self.led.on()
    }

    /// EDIT on SCN: `live` to SAVE's slot.
    fn save_scene_to(&mut self) {
        self.save_scene_in(self.orbit.save_slot(self.project.orbit()));
    }

    /// `live` to slot `n`, at once over the playing or a neutral one; over
    /// another saved scene, SAVE OVER asks.
    fn save_scene_in(&mut self, n: SceneIdx) {
        match self.project.orbit_mut().save_to(n) {
            Ok(SceneSave::Saved(n)) => self.scene_saved(n),
            Ok(SceneSave::Ask(a)) => self.ask(Ask::SaveSceneOver(a, prompt::Choice::new())),
            Err(e) => self.error(Line::new(prompt::scene_refused(e))),
        }
    }

    /// A scene prompt answered yes: `act` on the slot it asked about, a
    /// refusal toasted.
    fn scene_answered(
        &mut self,
        yes: Confirmed<SceneSlot>,
        act: impl FnOnce(&mut Self, Confirmed<SceneSlot>) -> Result<(), Refused>,
    ) {
        if let Err(e) = act(self, yes) {
            self.error(Line::new(prompt::scene_refused(e)));
        }
    }

    fn scene_saved(&mut self, n: SceneIdx) {
        self.toast.show(busy::Toast {
            text: prompt::scene_saved(n),
            ms: busy::Toast::SAVED_MS,
        });
    }

    /// ORBIT's transport and what the pages draw of it.
    pub fn orbit(&self) -> &orbit::OrbitUi {
        &self.orbit
    }

    /// Process one frame of input: navigation + encoder deltas, then
    /// ORBIT's commands flushed for the frame's publish.
    pub fn handle_input(&mut self, controls: &impl Controls) {
        self.input(controls);
        self.flush_orbit();
    }

    fn input(&mut self, controls: &impl Controls) {
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
        if presses.menu == Some(Press::Hold) {
            self.quick_save();
        }
        if self.loc.orbit().is_some() {
            return self.orbit_input(controls, presses);
        }
        let keys = self.nav_keys(controls, presses);
        for k in keys.into_iter().flatten() {
            if self.job.is_some() || self.modal.is_some() {
                break;
            }
            if let (Some((p, b)), NavKey::Edit) = (self.loc.browse(), k) {
                match controls.edges(ButtonId::Mix).down {
                    true => self.save_part(p),
                    false => self.load_sound(p, b),
                }
                continue;
            }
            let cx = self.cx();
            let at = self.loc.settings();
            match self.loc.step(k, &cx, &mut self.recall) {
                Step::Go(to) => self.go(to),
                Step::Act(a) => self.run_act(a, at),
                Step::Screen(s @ (Screen::LoadProject | Screen::ManageProjects)) => {
                    self.queue(Job::List(s))
                }
                Step::Screen(Screen::SaveToProj) => {
                    self.go(Location::settings_at(screen_path(Screen::SaveToProj), 0))
                }
                // On the playing scene's row, as SCN's SAVE starts.
                #[cfg(feature = "orbit")]
                Step::Screen(Screen::SceneSaveTo) => {
                    let row = self.project.orbit().current().get();
                    self.go(Location::settings_at(screen_path(Screen::SceneSaveTo), row))
                }
                Step::Run => match (self.screen(), at) {
                    (Some(Screen::LoadProject), Some(s)) => self.run_load(s.row()),
                    (Some(Screen::ManageProjects), Some(s)) => self.run_manage(s),
                    (Some(Screen::SaveToProj), Some(s)) => {
                        if let Some(&c) = SAVE_ROWS.get(s.row() as usize) {
                            self.run_part_cmd(c);
                        }
                    }
                    #[cfg(feature = "orbit")]
                    (Some(Screen::SceneSaveTo), Some(s)) => {
                        if let Some(&n) = SceneIdx::ALL.get(s.row() as usize) {
                            self.save_scene_in(n);
                        }
                    }
                    _ => {}
                },
                // ORBIT's acts come from `orbit_keys` only.
                Step::Orbit(_) | Step::Stay => {}
            }
        }

        if let Some((p, b)) = self.loc.browse() {
            let mix = controls.edges(ButtonId::Mix).down;
            if mix && controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
                self.ask_clear_slot(b);
            }
            if self.modal.is_some() {
                return;
            }
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
                if view::is_dimmed(&v, &self.project.part(at).sound, &self.ctx()) {
                    continue; // dimmed: the encoder is ignored
                }
                // The reverb turns by its TYPE's panel, and TYPE keeps the
                // knobs it leaves (reverb TYPEs spec § Pages).
                if let Some(a) = v.addr()
                    && a.block == BlockRef::Reverb
                    && (!shift
                        || reverb_panel::whole_ticks(&self.project.perf().fx.reverb, a.param))
                {
                    let (tempo, _) = crate::meter::rev_clock();
                    let rev = &mut self.project.edit_part(at).fx.reverb;
                    *rev = reverb_panel::turn(rev, a.param, delta, &mut self.rev_knobs, tempo);
                    continue;
                }
                let params = &mut UiBlocks {
                    project: &mut self.project,
                    part: at,
                    theme: &mut self.theme,
                    midi: &mut self.midi,
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
                        && view::is_dimmed(&v, sound, &self.ctx())
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

    /// Advance animations one frame: a `Pacer`'s, at `UI_FPS`.
    pub fn update(&mut self, _: animation::UiTick) {
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
            // BLOCK_SIZE · UI_FPS steps it by rate / UI_FPS: real time.
            use animation::UI_FPS;
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

        let pulse = self.orbit.tick();
        self.led = if self.orbit.status.running {
            self.led.step(pulse)
        } else {
            led::Led::OFF
        };

        // CUSTOM's cursor eases the short way round, and comes back into
        // one turn once it rests.
        self.renderer
            .orbit_cursor
            .ease_round(self.orbit.cursor as f32, 12.0);
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
    /// SYSTEM › ABOUT and DIAGNOSTICS › AUDIO LOAD.
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
            browser::draw(
                display,
                self.project.pool(),
                part,
                cursor,
                scroll,
                self.theme.hints,
            );
            self.draw_prompt(display);
            return;
        }
        self.renderer
            .draw_with_def(display, &self.frame(perf, audio, scope));
    }

    /// The open prompt's panel, if any, over the Sound rung.
    fn draw_prompt<D>(&self, d: &mut D)
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        if let Some(a) = self.prompt() {
            prompt::blank_beneath(d, prompt::Beneath::Page);
            a.with_view(|v| prompt::draw_prompt(d, v));
        }
    }

    /// Whose page the header names.
    fn head(&self) -> Head {
        match self.loc.rung() {
            Rung::Pages(p, _) | Rung::Sound(p) => Head::Sound(p),
            Rung::Mixer(p, _) | Rung::Fx(p, _) => Head::Mix(p),
            Rung::Settings(_) => Head::Settings,
            Rung::Orbit(o) if o.page().node() as usize == orbit::RING_NODE => Head::Ring(o.ring()),
            Rung::Orbit(_) => Head::Orbit(self.project.orbit().current()),
        }
    }

    /// The page name the header draws (`FILTER`, `SENDS`, the exciter's
    /// name, ` / A` or ` / B` on an ENV page), short when the line is full;
    /// `None` where no header is drawn: SETTINGS and the Sound rung.
    pub fn page_name(&self) -> Option<FmtBuf> {
        matches!(
            self.loc.rung(),
            Rung::Pages(..) | Rung::Mixer(..) | Rung::Fx(..) | Rung::Orbit(..)
        )
        .then(|| self.page_title())
    }

    /// `page_name` where a header is drawn: Pages, Mixer, FX and ORBIT.
    pub fn page_title(&self) -> FmtBuf {
        renderer::page_header(
            self.head(),
            self.page_def(),
            &self.ctx(),
            self.project.perf().parts(),
        )
        .name
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
        let (ctx, focus, orbit) = (self.ctx(), self.focused_slot(), self.orbit_frame());
        let anim = &self.renderer.anim;
        renderer::Frame {
            head: self.head(),
            map: self.loc.page(&cx).filter(|_| settings.is_none()),
            layout: self.layout(),
            settings: self.bands(),
            prompt: self.prompt(),
            def,
            perf,
            matrix: &self.matrix_state,
            sel_op: self.sel_op,
            orbit_focus: orbit::draw::focus(def, focus, &ctx, &orbit, self.theme.hints, |i| {
                anim[i].current()
            }),
            hints: self.theme.hints,
            ctx,
            focus,
            scope,
            sounding: crate::scope::peak(scope) > crate::scope::SOUNDING_PEAK,
            orbit_dot: self.orbit.anim.dot(),
            parts: self.project.perf().parts(),
            active_part: self.active_part,
            prime_status: self.prime_status,
            audio,
            card: self.card,
            master_gr_db: crate::meter::MASTER_GR.read(),
            rev_clock: crate::meter::rev_clock(),
            clock: self.clock,
            fx: &self.project.perf().fx,
            orbit,
        }
    }

    /// What ORBIT's pages draw from.
    fn orbit_frame(&self) -> orbit::draw::OrbitFrame<'_> {
        orbit::draw::OrbitFrame::new(self.project.orbit(), &self.orbit)
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
    /// Part n's pages n, its mixer 10 + n, its Sound rung 30 + n, SETTINGS 20,
    /// ORBIT on ring r 40 + r.
    fn loc_tag(&self) -> (u8, u8, u8) {
        let place = match (self.loc.settings(), self.loc.part(), self.loc.orbit()) {
            (_, _, Some(o)) => 40 + o.ring().get(),
            (Some(_), _, _) | (_, None, _) => 20,
            (_, Some(p), _) if self.loc.on_mixer() => 10 + p.index() as u8,
            (_, Some(p), _) if self.loc.browse().is_some() => 30 + p.index() as u8,
            (_, Some(p), _) => p.index() as u8,
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
                renderer::title_type(f.def, &f.ctx.envs),
                renderer::header_out(f) as u8,
                f.header_status(),
            )
            .with_scene(match f.head {
                Head::Orbit(n) => n.get() + 1,
                _ => 0,
            }),
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
            .with_text(f.orbit_focus.as_ref().map_or(0, |o| o.key()), f.hints)
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
                    .with_orbit(self.renderer.orbit_viz_key(f))
            }
            RegionKind::Cells => {
                let looks = (0..6).fold(0u16, |k, i| k | (renderer::look(f, i) as u16) << (2 * i));
                RegionData::cells(
                    self.page,
                    match f.def.viz {
                        VizType::AudioStats => audio_page::cells_key(f.audio),
                        VizType::About => about_page::cells_key(f.audio, f.card),
                        _ => qvalues,
                    },
                    f.focus as u8,
                    self.matrix_state.num_dests as u16,
                    core::array::from_fn(|i| renderer::mod_info(f, i).map(f32::to_bits)),
                )
                .keyed(self.matrix_state.rev, looks)
                .with_text_key(rev_text_key(f))
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
                    RegionKind::Crumbs => b.crumbs_key(f.header_status()),
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
        self.region_set.drawn_hints = Some(self.theme.hints);
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

    /// THEME's HINTS moved since the bands were drawn, noted as drawn now:
    /// every band and the browser redraw, so no page keys its band on it.
    fn hints_moved(&mut self) -> bool {
        let now = Some(self.theme.hints);
        core::mem::replace(&mut self.region_set.drawn_hints, now) != now
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
    /// `audio` behind ABOUT and AUDIO LOAD. Returns list of (y_start, y_end)
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
        let hints_moved = self.hints_moved();
        // The Sound rung's browser: one flush region, redrawn only while
        // dirty (opened, cursor/scroll moved; #7).
        if let Some((part, b)) = self.loc.browse() {
            let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
            if self.browser_dirty || hints_moved {
                let fb = display.pixel_buffer();
                Renderer::clear_region_fb(fb, 0, chimera_hal::SCREEN_HEIGHT);
                let (cursor, scroll) = (b.cursor(), b.scroll());
                browser::draw(
                    display,
                    self.project.pool(),
                    part,
                    cursor,
                    scroll,
                    self.theme.hints,
                );
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
        let fresh = self.region_set.prev_screen != Some(screen) || hints_moved;
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
                    // The panel is opaque over its own last frame; what it
                    // lands on blanks once, as it opens.
                    if !prompt {
                        renderer::Renderer::clear_region_fb(
                            display.pixel_buffer(),
                            r.y_start,
                            r.y_end,
                        );
                    } else if fresh {
                        renderer::Renderer::blank_beneath_prompt(display, &f);
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

/// A Part's blocks plus SETTINGS › THEME and MIDI CONFIG, so those edit
/// through the same slot bindings as any other. Only a block written takes
/// `edit_part`, so only a write moves the project's revision.
struct UiBlocks<'a> {
    project: &'a mut Project,
    part: PartId,
    theme: &'a mut ThemeSettings,
    midi: &'a mut MidiSettings,
}

/// `UiBlocks`' shared twin: no `block_mut`, so a frame can't write.
struct UiRead<'a> {
    project: &'a Project,
    part: PartId,
    theme: &'a ThemeSettings,
    midi: &'a MidiSettings,
}

/// Part `part`'s block `b`, the theme, MIDI CONFIG, or ORBIT's: what both
/// views read.
fn read_block<'a>(
    project: &'a Project,
    part: PartId,
    (theme, midi): (&'a ThemeSettings, &'a MidiSettings),
    b: BlockRef,
) -> Option<&'a dyn Block> {
    let song = project.orbit();
    match b {
        BlockRef::Theme => Some(theme),
        BlockRef::Midi => Some(midi),
        BlockRef::PartMix(p) => Some(project.read_part(p).mix),
        BlockRef::OrbitChords => Some(&song.live().chords),
        BlockRef::OrbitRing(r) => Some(&song.live().rings[r.get() as usize]),
        BlockRef::Tempo => Some(&song.bpm),
        _ => project.read_part(part).block(b),
    }
}

impl BlockRead for UiBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        read_block(self.project, self.part, (self.theme, self.midi), b)
    }
}

impl Blocks for UiBlocks<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::Theme => Some(self.theme),
            BlockRef::Midi => Some(self.midi),
            BlockRef::PartMix(p) => Some(self.project.edit_part(p).mix),
            // A scene's params change through `OrbitSong::edit`, which
            // writes its slot too (ADR 0083): never from here. A caller
            // naming one is a bug the tests catch; release reads nothing.
            BlockRef::OrbitChords | BlockRef::OrbitRing(_) => {
                debug_assert!(false, "{b:?} edits through OrbitSong::edit");
                None
            }
            BlockRef::Tempo => Some(&mut self.project.orbit_mut().bpm),
            _ => {
                let PartEdit { sound, mix, fx } = self.project.edit_part(self.part);
                part_block_mut(sound, mix, fx, b)
            }
        }
    }
}

impl BlockRead for UiRead<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        read_block(self.project, self.part, (self.theme, self.midi), b)
    }
}

/// Display values for `page`: Part pages through slot bindings, legacy pages
/// through `PageId`.
fn page_values(page: PageKey, def: &BlockDef, params: &impl BlockRead, ctx: &SlotCtx) -> [f32; 6] {
    match page {
        PageKey::Part { .. } => part_page::values_under(def, params, ctx),
        PageKey::Legacy(p) => p.read_values(params),
    }
}

fn nav_cx(project: &Project) -> NavCtx {
    let live = project.orbit().live();
    NavCtx {
        engines: PartId::ALL.map(|p| project.part(p).sound.engine()),
        dyn_rows: 0,
        orbit: OrbitCx {
            targets: live.rings.map(|r| r.target.part()),
        },
    }
}

/// The page `at` shows; lists, Screens and the Sound rung show `NO_PAGE`.
fn page_def(at: Location, cx: &NavCtx) -> &'static BlockDef {
    at.page(cx)
        .and_then(|(c, p)| c.def_at(p))
        .unwrap_or(&NO_PAGE)
}

/// What a reverb panel's cells read beyond their own values: HALL's plate
/// zone (the SIZE cell's `PLATE`) and the PRE-DLY it plays there, which a
/// SIZE CC moves while REV › MORE shows only PRE-DLY's stored knob; and
/// the clock a synced cell's note reads. 0 on any other page.
fn rev_text_key(f: &renderer::Frame) -> u32 {
    if !(0..f.def.params.len()).any(|i| view::is_reverb_panel(f.def, i)) {
        return 0;
    }
    let pre = crate::dsp::reverb::plate_pull(&f.fx.reverb).predelay;
    // A synced cell reads the tempo, on its 0.25 BPM grid, and the fold.
    let (t, fold) = f.rev_clock;
    let clock = ((t.0 * 4.0) as u32) << 8 | fold.0 as u8 as u32;
    ((f.ctx.plate as u32) << 16 | region::quantize(pre) as u32) ^ clock.wrapping_mul(0x9E37_79B9)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartParams;

    #[test]
    fn part_mix_edits_the_named_part() {
        let (mut project, _) = Project::boxed();
        let mut theme = ThemeSettings::default();
        let mut midi = MidiSettings::default();
        let [p1, _, p3, ..] = PartId::ALL;
        let a = ParamAddr::new(BlockRef::PartMix(p3), PartParams::CHANNEL);
        let ch = |p: &Project, id| p.part(id).mix.get(PartParams::CHANNEL);
        let before = ch(&project, p1);
        let mut ui = UiBlocks {
            project: &mut project,
            part: p1,
            theme: &mut theme,
            midi: &mut midi,
        };
        ui.block_mut(a.block).unwrap().set(a.param, 9.0);
        assert_eq!(ch(&project, p3), 9.0);
        assert_eq!(ch(&project, p1), before);
        let read = UiRead {
            project: &project,
            part: p1,
            theme: &theme,
            midi: &midi,
        };
        assert_eq!(read.block(a.block).unwrap().get(a.param), 9.0);
    }
}
