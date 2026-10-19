//! Where the UI is, and where each key takes it: one `Location` and a pure
//! `step` (ADR 0044, amended by ADR 0066; its D-pad by ADR 0081).

use crate::hw::MAX_PARTS;
use crate::orbit::{RINGS, RingIdx};
use crate::params::EngineType;
use crate::preset::Sound;
use crate::project::PartId;
use crate::ui::block_def::{ChainDef2, Move, refocus};
use crate::ui::block_registry::{
    self, ALGO_CHAIN, CHORUS, MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART, MODAL_PLUCK_CHAIN,
};
use crate::ui::orbit::{ORB_NODE, ORBIT_CHAIN, RING_NODE, SCN_NODE};
use crate::ui::settings::leaves::OnePage;
use crate::ui::settings::{Act, Kind, MANAGE_COMMANDS, Row, Screen, row_at};
use crate::ui::theme_settings::Pages;

pub use crate::ui::block_def::PageAt;
pub use crate::ui::browser::Browse;

/// Path depth limit of the SETTINGS tree.
const MAX_DEPTH: usize = 4;
/// The mixer's first shared FX node, after PART and SENDS.
const FX_FIRST: u8 = if MIXER_HOME > MIXER_PART {
    MIXER_HOME as u8 + 1
} else {
    MIXER_PART as u8 + 1
};
const _: () = assert!(MIXER_CHANNEL_CHAIN.blocks[FX_FIRST as usize].def.id == CHORUS.id);

// The mixer's home is SENDS: from outside it, B*n* and MIX+B*n* open there.
const _: () = assert!(MIXER_CHANNEL_CHAIN.home().node() as usize == MIXER_HOME);

/// A mixer chain page, checked at compile time.
const fn mixer_page(node: usize) -> PageAt {
    match MIXER_CHANNEL_CHAIN.page(node, 0) {
        Some(p) => p,
        None => panic!("not a mixer node"),
    }
}
const PART_PAGE: PageAt = mixer_page(MIXER_PART);
const SENDS_PAGE: PageAt = mixer_page(MIXER_HOME);
const FX_FIRST_PAGE: PageAt = mixer_page(FX_FIRST as usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixPage {
    Part,
    Sends,
}

/// The mixer page B*n* and MIX+B*n* reopen from outside the mixer: its
/// home (SENDS), or the FX page last left; never PART, whose C is OUT
/// (ADR 0057).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MixAt {
    Home,
    Fx(PageAt),
}

impl MixPage {
    fn page(self) -> PageAt {
        match self {
            MixPage::Part => PART_PAGE,
            MixPage::Sends => SENDS_PAGE,
        }
    }
}

/// What a SETTINGS place shows besides its bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// A list, or a Screen's rows.
    List,
    Leaf,
    /// MANAGE PROJECTS: which column has the bar.
    Manage(Column),
}

/// MANAGE PROJECTS' columns (ADR 0066).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Projects,
    Command(u8),
}

/// A SETTINGS list: only `SettingsAt::list` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListAt(SettingsAt);

impl ListAt {
    /// The list at `path`, the bar on `row`: a path that isn't a list
    /// fails the build where a const asks.
    pub const fn at(path: &[u8], row: u8) -> ListAt {
        assert!(path.len() <= MAX_DEPTH, "too deep");
        assert!(
            matches!(
                crate::ui::settings::tree::kind_at(path),
                Some(Kind::List(_))
            ),
            "not a list"
        );
        let mut p = [0; MAX_DEPTH];
        let mut i = 0;
        while i < path.len() {
            p[i] = path[i];
            i += 1;
        }
        ListAt(SettingsAt {
            path: p,
            depth: path.len() as u8,
            row,
            at: At::List,
        })
    }

    pub fn location(self) -> Location {
        Location(Loc::Settings(self.0))
    }
}

/// A place in SETTINGS: a path, the bar on `row`, and what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsAt {
    path: [u8; MAX_DEPTH],
    depth: u8,
    row: u8,
    at: At,
}

impl SettingsAt {
    pub fn path(&self) -> &[u8] {
        &self.path[..self.depth as usize]
    }

    pub fn row(&self) -> u8 {
        self.row
    }

    /// On MANAGE PROJECTS, the column with the bar.
    pub fn column(&self) -> Option<Column> {
        match self.at {
            At::Manage(c) => Some(c),
            _ => None,
        }
    }

    /// This place as a list, MANAGE's columns included: NAMING opens only
    /// on one.
    pub fn list(self) -> Option<ListAt> {
        let listed = matches!(self.at, At::List | At::Manage(_));
        (listed && !matches!(self.kind(), Some(Kind::Leaf(_)))).then_some(ListAt(self))
    }

    /// The Screen this place shows, its rows built at run time.
    pub fn screen(&self) -> Option<Screen> {
        match self.kind()? {
            Kind::Screen(s) => Some(s),
            _ => None,
        }
    }

    pub fn at_leaf(&self) -> Option<&'static OnePage> {
        match self.kind()? {
            Kind::Leaf(c) => Some(c),
            _ => None,
        }
    }

    /// Arrived: a leaf on its page, MANAGE on its projects.
    fn landed(self) -> SettingsAt {
        let at = match self.kind() {
            Some(Kind::Leaf(_)) => At::Leaf,
            Some(Kind::Screen(Screen::ManageProjects)) => At::Manage(Column::Projects),
            _ => At::List,
        };
        SettingsAt { at, ..self }
    }

    fn kind(&self) -> Option<Kind> {
        row_at(self.path()).map(|r| r.kind)
    }

    fn child(self) -> Option<SettingsAt> {
        let d = self.depth as usize;
        if d == MAX_DEPTH {
            return None;
        }
        let mut path = self.path;
        path[d] = self.row;
        Some(
            SettingsAt {
                path,
                depth: self.depth + 1,
                row: 0,
                at: At::List,
            }
            .landed(),
        )
    }

    /// MENU and SEQ: MANAGE's commands back to its list, else one level up
    /// with the bar on the row just left.
    fn back(self) -> Option<SettingsAt> {
        if let At::Manage(Column::Command(_)) = self.at {
            return Some(SettingsAt {
                at: At::Manage(Column::Projects),
                ..self
            });
        }
        self.parent()
    }

    /// One level up, the bar on the row just left.
    fn parent(self) -> Option<SettingsAt> {
        let d = self.depth.checked_sub(1)?;
        let mut path = self.path;
        // Zero past `depth`, so equal places compare equal.
        path[d as usize] = 0;
        Some(
            SettingsAt {
                path,
                depth: d,
                row: self.path[d as usize],
                at: At::List,
            }
            .landed(),
        )
    }

    fn step(self, k: NavKey, cx: &NavCtx) -> Step {
        let go = |s: SettingsAt| Step::Go(Location(Loc::Settings(s)));
        let wrap = |i: u8, n: usize, d: i32| step_wrapped(i as i32, d, n) as u8;
        let delta = match k {
            NavKey::Bar(d) => Some(d as i32),
            NavKey::Plus => Some(1),
            NavKey::Minus => Some(-1),
            _ => None,
        };
        let row = |n: usize, d: i32| {
            go(SettingsAt {
                row: wrap(self.row, n, d),
                ..self
            })
        };
        let under_bar = |rs: &'static [Row]| rs.get(self.row as usize).map(|r| r.kind);
        match (self.kind(), k, delta) {
            // Up, as MENU; at the top, nothing (ADR 0081).
            (_, NavKey::SeqTap, _) => self.back().map_or(Step::Stay, go),
            // One page: no other key steps it.
            (Some(Kind::Leaf(_)), _, _) => Step::Stay,
            (Some(Kind::List(rs)), _, Some(d)) => row(rs.len(), d),
            (_, _, Some(d)) if matches!(self.at, At::Manage(Column::Command(_))) => {
                let At::Manage(Column::Command(n)) = self.at else {
                    return Step::Stay;
                };
                go(SettingsAt {
                    at: At::Manage(Column::Command(wrap(n, MANAGE_COMMANDS.len(), d))),
                    ..self
                })
            }
            (_, NavKey::Edit, _) if self.at == At::Manage(Column::Projects) => {
                if cx.dyn_rows == 0 {
                    return Step::Stay;
                }
                go(SettingsAt {
                    at: At::Manage(Column::Command(0)),
                    ..self
                })
            }
            (Some(Kind::Screen(_)), _, Some(d)) => row(cx.dyn_rows as usize, d),
            (Some(Kind::Screen(_)), NavKey::Edit, _) => Step::Run,
            (Some(Kind::List(rs)), NavKey::Edit, _) => match under_bar(rs) {
                Some(Kind::List(_) | Kind::Leaf(_)) => self.child().map_or(Step::Stay, go),
                Some(Kind::Screen(s)) => Step::Screen(s),
                Some(Kind::Act(a)) => Step::Act(a),
                _ => Step::Stay,
            },
            _ => Step::Stay,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Loc {
    /// A Part's sound pages.
    Pages(PartId, PageAt),
    /// A Part's rung: its mixer pages.
    Part(PartId, MixPage),
    /// The shared FX, `node` from CHORUS on the mixer chain, reached from
    /// Part n's mixer: it counts as Part n's own mixer.
    Fx(PartId, PageAt),
    /// The Sound rung, its browser where it was left.
    Sound(PartId, Browse),
    Settings(SettingsAt),
    /// ORBIT, a place of its own: SEQ hold goes in, MENU leaves.
    Orbit(OrbitAt),
}

/// ORB › CUSTOM, checked at compile time.
const CUSTOM_PAGE: PageAt = match ORBIT_CHAIN.page(ORB_NODE, 1) {
    Some(p) => p,
    None => panic!("ORB has no CUSTOM"),
};

/// RING's first sub-page, checked at compile time.
const RING_PAGE: PageAt = match ORBIT_CHAIN.page(RING_NODE, 0) {
    Some(p) => p,
    None => panic!("no RING"),
};

/// A place in ORBIT: one of `ORBIT_CHAIN`'s pages, and the ring it edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitAt {
    page: PageAt,
    ring: RingIdx,
}

impl OrbitAt {
    /// ORB, ring 1.
    pub const HOME: OrbitAt = OrbitAt {
        page: ORBIT_CHAIN.home(),
        ring: RingIdx::ALL[0],
    };

    pub fn page(self) -> PageAt {
        self.page
    }

    pub fn ring(self) -> RingIdx {
        self.ring
    }

    pub fn with_ring(self, ring: RingIdx) -> OrbitAt {
        OrbitAt { ring, ..self }
    }

    /// ORB › CUSTOM, the same ring.
    pub fn custom(self) -> OrbitAt {
        OrbitAt {
            page: CUSTOM_PAGE,
            ..self
        }
    }

    fn on(self, node: usize) -> bool {
        self.page.node() as usize == node
    }

    fn step(self, k: OrbitKey, cx: &NavCtx, r: &Recall) -> Step {
        let go = |o: OrbitAt| Step::Go(Location(Loc::Orbit(o)));
        let page = |at: Option<PageAt>| at.map_or(Step::Stay, |page| go(OrbitAt { page, ..self }));
        let sub = self.page.sub() as usize;
        match k {
            OrbitKey::Ring(ring) => {
                // On RING the sub-page shown stays; elsewhere the last one.
                let page = if self.on(RING_NODE) {
                    self.page
                } else {
                    r.ring_page
                };
                go(OrbitAt { page, ring })
            }
            OrbitKey::Cross(ring) => {
                let p = cx.orbit.targets[ring.get() as usize];
                Step::Go(Location(Loc::Pages(p, r.pages_of(p, cx))))
            }
            OrbitKey::Mute(ring) => Step::Orbit(OrbitAct::Mute(ring)),
            OrbitKey::PlayStop => Step::Orbit(OrbitAct::PlayStop),
            OrbitKey::Plus => page(ORBIT_CHAIN.step(self.page, Move::Next)),
            OrbitKey::Minus => page(ORBIT_CHAIN.step(self.page, Move::Prev)),
            // SCN's commit: SAVE → n, EDIT under either PAGES.
            OrbitKey::Edit if self.on(SCN_NODE) => Step::Orbit(OrbitAct::SaveTo),
            OrbitKey::Edit | OrbitKey::SeqTap => {
                let k = if k == OrbitKey::Edit {
                    NavKey::Edit
                } else {
                    NavKey::SeqTap
                };
                match sub_move(cx.pages, k) {
                    // Up a sub-page, never a save (ADR 0081).
                    Some(Move::Up) => page(ORBIT_CHAIN.step(self.page, Move::Up)),
                    Some(Move::Down) if self.on(RING_NODE) => {
                        let n = ORBIT_CHAIN.blocks[RING_NODE].sub_page_count().max(1);
                        page(ORBIT_CHAIN.page(RING_NODE, (sub + 1) % n))
                    }
                    // Down (#316): ORB's one sub-page, CUSTOM, always
                    // opens, a named scale shown by its notes (ADR 0084);
                    // from it, down wraps back to ORB.
                    Some(Move::Down) if self.on(ORB_NODE) && sub == 1 => {
                        page(ORBIT_CHAIN.page(ORB_NODE, 0))
                    }
                    Some(Move::Down) if self.on(ORB_NODE) => page(Some(CUSTOM_PAGE)),
                    _ => Step::Stay,
                }
            }
            OrbitKey::MenuTap => Step::Go(r.resolve(r.orbit_from, cx)),
        }
    }
}

impl From<OrbitAt> for Location {
    fn from(o: OrbitAt) -> Location {
        Location(Loc::Orbit(o))
    }
}

/// A key in ORBIT, already decided (`UiState::orbit_keys`). It has no SEQ
/// hold and no B*n* that jumps to a Part's pages: only `Cross`, a held
/// B*n*, leaves for a Part, and only `MenuTap` for where ORBIT was entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitKey {
    /// B1–B5 tapped: the ring, on RING.
    Ring(RingIdx),
    /// B1–B5 held: the ring's Part's pages.
    Cross(RingIdx),
    /// MIX + B1–B5.
    Mute(RingIdx),
    /// B6.
    PlayStop,
    Plus,
    Minus,
    Edit,
    SeqTap,
    MenuTap,
}

/// What an ORBIT key asks `UiState` to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitAct {
    PlayStop,
    Mute(RingIdx),
    /// EDIT on SCN: `live` to SAVE's slot, asking over another saved one.
    SaveTo,
}

/// What ORBIT's keys read from the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitCx {
    /// Each ring's Part: where a held B*n* crosses to.
    pub targets: [PartId; RINGS],
}

impl OrbitCx {
    /// Ring *n* plays Part *n*.
    pub const NONE: OrbitCx = OrbitCx {
        targets: {
            let mut t = [PartId::ALL[0]; RINGS];
            let mut i = 0;
            while i < RINGS {
                t[i] = PartId::ALL[i];
                i += 1;
            }
            t
        },
    };
}

/// Where `Recall` starts, before anything is left: re-resolved on use.
const PLACEHOLDER: Loc = Loc::Pages(PartId::ALL[0], ALGO_CHAIN.home());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location(Loc);

/// Which rung of the ladder `Location` is on, without its private memory
/// (the Sound rung's browser): the public mirror of `Loc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    Pages(PartId, PageAt),
    Mixer(PartId, MixPage),
    Fx(PartId, PageAt),
    Sound(PartId),
    Settings(SettingsAt),
    Orbit(OrbitAt),
}

/// A sound-side `Location`, neither SETTINGS nor ORBIT: where MENU at
/// SETTINGS' top, and MENU in ORBIT, return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outside(Loc);

impl Outside {
    pub fn new(l: Location) -> Option<Outside> {
        match l.0 {
            Loc::Settings(_) | Loc::Orbit(_) => None,
            l => Some(Outside(l)),
        }
    }

    pub fn get(self) -> Location {
        Location(self.0)
    }
}

#[derive(Clone, Copy, Debug)]
struct SoundPage {
    chain: PartChain,
    at: PageAt,
}

/// What leaving a place remembers (ADR 0044).
#[derive(Clone, Copy, Debug)]
pub struct Recall {
    pages: [Option<SoundPage>; MAX_PARTS],
    mix: MixAt,
    settings_from: Outside,
    /// The ORBIT page last left.
    orbit: OrbitAt,
    /// Where SEQ hold entered ORBIT from.
    orbit_from: Outside,
    /// RING's sub-page last shown: B1–B5 reopen it.
    ring_page: PageAt,
}

impl Default for Recall {
    fn default() -> Self {
        Self::new()
    }
}

impl Recall {
    pub const fn new() -> Self {
        Self {
            pages: [None; MAX_PARTS],
            mix: MixAt::Home,
            settings_from: Outside(PLACEHOLDER),
            orbit: OrbitAt::HOME,
            orbit_from: Outside(PLACEHOLDER),
            ring_page: RING_PAGE,
        }
    }

    pub fn settings_from(&self) -> Location {
        self.settings_from.get()
    }

    pub fn orbit_from(&self) -> Location {
        self.orbit_from.get()
    }

    fn leave(&mut self, from: Location, to: Location, cx: &NavCtx) {
        match from.0 {
            Loc::Pages(p, at) => {
                self.pages[p.index()] = Some(SoundPage {
                    chain: cx.chain(p),
                    at,
                })
            }
            Loc::Part(..) => self.mix = MixAt::Home,
            Loc::Fx(_, at) => self.mix = MixAt::Fx(at),
            Loc::Orbit(o) => {
                if o.on(RING_NODE) {
                    self.ring_page = o.page;
                }
                if to.orbit().is_none() {
                    self.orbit = o;
                }
            }
            _ => {}
        }
        if let (Some(o), Some(_)) = (Outside::new(from), to.settings()) {
            self.settings_from = o;
        }
        // Into ORBIT: from here, or from SETTINGS where it was opened.
        if from.orbit().is_none() && to.orbit().is_some() {
            self.orbit_from = Outside::new(from).unwrap_or(self.settings_from);
        }
    }

    /// Part n's pages left, kept by page id if on the same engine; else
    /// its home.
    fn pages_of(&self, p: PartId, cx: &NavCtx) -> PageAt {
        let now = cx.chain(p);
        match self.pages[p.index()] {
            Some(s) if s.chain.engine() == now.engine() => refocus(s.chain.def(), s.at, now.def()),
            _ => now.def().home(),
        }
    }

    /// Part n's mixer from outside it.
    fn mix_entry(&self, n: PartId) -> Loc {
        match self.mix {
            MixAt::Home => mix_loc(n, MIXER_CHANNEL_CHAIN.home()),
            MixAt::Fx(at) => Loc::Fx(n, at),
        }
    }

    /// MENU at the top: where it was pressed.
    fn close_settings(&self, cx: &NavCtx) -> Location {
        self.resolve(self.settings_from, cx)
    }

    /// `o`, a page re-resolved in case the engine changed meanwhile.
    fn resolve(&self, o: Outside, cx: &NavCtx) -> Location {
        match o.0 {
            Loc::Pages(p, _) => Location(Loc::Pages(p, self.pages_of(p, cx))),
            l => Location(l),
        }
    }
}

pub struct NavCtx {
    pub chains: [PartChain; MAX_PARTS],
    /// The reverb's TYPE: REV › MORE 2 shows only under a TYPE with cells
    /// there (HALL's DENSITY).
    pub rev: crate::dsp::reverb::ReverbType,
    /// A Screen's row count.
    pub dyn_rows: u8,
    pub orbit: OrbitCx,
    /// THEME's PAGES: which way SEQ and EDIT step a sub-page (`sub_move`).
    pub pages: Pages,
}

impl NavCtx {
    fn chain(&self, p: PartId) -> PartChain {
        self.chains[p.index()]
    }
}

/// A Part's page chain: its engine's, picked per Sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartChain {
    Algo,
    /// ALGO with a FORMANT page: an operator is a formant or noise band.
    AlgoFormant,
    Modal,
    Tape,
    Va,
}

impl PartChain {
    pub fn of(sound: &Sound) -> PartChain {
        match Self::base(sound.engine()) {
            PartChain::Algo if sound.params.algo.kinds().any_formant() => PartChain::AlgoFormant,
            c => c,
        }
    }

    pub fn base(engine: EngineType) -> PartChain {
        match engine {
            EngineType::Algo => PartChain::Algo,
            EngineType::Modal => PartChain::Modal,
            EngineType::Tape => PartChain::Tape,
            EngineType::Va => PartChain::Va,
        }
    }

    pub fn def(self) -> &'static ChainDef2 {
        match self {
            PartChain::Algo => &block_registry::ALGO_CHAIN,
            PartChain::AlgoFormant => &block_registry::ALGO_FORMANT_CHAIN,
            PartChain::Modal => &MODAL_PLUCK_CHAIN,
            PartChain::Tape => &block_registry::TAPE_CHAIN,
            PartChain::Va => &block_registry::VA_CHAIN,
        }
    }

    pub fn engine(self) -> EngineType {
        match self {
            PartChain::Algo | PartChain::AlgoFormant => EngineType::Algo,
            PartChain::Modal => EngineType::Modal,
            PartChain::Tape => EngineType::Tape,
            PartChain::Va => EngineType::Va,
        }
    }
}

/// An already-decided key: taps act on release (`ui::hold`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKey {
    Part(PartId),
    MixPart(PartId),
    EditPart(PartId),
    Plus,
    Minus,
    Edit,
    SeqTap,
    MenuTap,
    /// Encoder 1 on a list.
    Bar(i8),
    /// SEQ held: into ORBIT.
    SeqHold,
    /// SEQ pressed under MIX: MIDI learn, `UiState`'s; no place moves.
    MixSeq,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Go(Location),
    Act(Act),
    /// EDIT on a Screen row: `UiState` lists it, then goes in.
    Screen(Screen),
    /// EDIT inside a Screen: `UiState` decides; on MANAGE, `column()` says which.
    Run,
    /// An ORBIT key's act: `UiState` runs it.
    Orbit(OrbitAct),
    Stay,
}

impl Location {
    /// Part 1's pages on its engine's home.
    pub fn home(cx: &NavCtx) -> Location {
        let p = PartId::ALL[0];
        Self::part_home(p, cx.chain(p).engine())
    }

    /// Part `p`'s pages on `engine`'s home.
    pub fn part_home(p: PartId, engine: EngineType) -> Location {
        Location(Loc::Pages(p, chain_def_for(engine).home()))
    }

    /// Part `p`'s pages after its chain was `was`: the page kept by id on
    /// the same engine, else home. Other places stay.
    pub fn rechained(self, p: PartId, was: PartChain, cx: &NavCtx) -> Location {
        match self.0 {
            Loc::Pages(q, at) if q == p => {
                let now = cx.chain(p);
                Location(Loc::Pages(
                    p,
                    if was.engine() == now.engine() {
                        refocus(was.def(), at, now.def())
                    } else {
                        now.def().home()
                    },
                ))
            }
            _ => self,
        }
    }

    /// Any page of Part `p`: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pages(p: PartId, at: PageAt) -> Location {
        Location(Loc::Pages(p, at))
    }

    pub fn mixer(p: PartId, m: MixPage) -> Location {
        Location(Loc::Part(p, m))
    }

    pub fn sound(p: PartId) -> Location {
        Self::sound_at(p, Browse::default())
    }

    pub fn sound_at(p: PartId, b: Browse) -> Location {
        Location(Loc::Sound(p, b))
    }

    /// On a Part's mixer or the FX it was entered from.
    pub fn on_mixer(self) -> bool {
        matches!(self.0, Loc::Part(..) | Loc::Fx(..))
    }

    /// A path deeper than the tree allows is cut to its first four rows.
    pub fn settings_at(path: &[u8], row: u8) -> Location {
        debug_assert!(path.len() <= MAX_DEPTH, "{path:?}");
        let path = &path[..path.len().min(MAX_DEPTH)];
        let mut p = [0; MAX_DEPTH];
        p[..path.len()].copy_from_slice(path);
        Location(Loc::Settings(
            SettingsAt {
                path: p,
                depth: path.len() as u8,
                row,
                at: At::List,
            }
            .landed(),
        ))
    }

    /// In SETTINGS, the bar kept on a list of `rows` rows.
    pub fn with_row_within(self, rows: u8) -> Location {
        match self.0 {
            Loc::Settings(s) => Location(Loc::Settings(SettingsAt {
                row: s.row.min(rows.saturating_sub(1)),
                ..s
            })),
            l => Location(l),
        }
    }

    /// In SETTINGS, one level up.
    pub fn up(self) -> Option<Location> {
        let s = self.settings()?.parent()?;
        Some(Location(Loc::Settings(s)))
    }

    /// On MANAGE's commands, its list; anywhere else, itself.
    pub fn manage_list(self) -> Location {
        match self.0 {
            Loc::Settings(
                s @ SettingsAt {
                    at: At::Manage(_), ..
                },
            ) => Location(Loc::Settings(SettingsAt {
                at: At::Manage(Column::Projects),
                ..s
            })),
            l => Location(l),
        }
    }

    pub fn step(self, k: NavKey, cx: &NavCtx, r: &mut Recall) -> Step {
        let s = self.next(k, cx, r);
        self.went(s, cx, r)
    }

    /// `step` for ORBIT's keys; outside ORBIT, nothing.
    pub fn step_orbit(self, k: OrbitKey, cx: &NavCtx, r: &mut Recall) -> Step {
        let s = self.orbit().map_or(Step::Stay, |o| o.step(k, cx, r));
        self.went(s, cx, r)
    }

    /// A `Go` to here is a `Stay`; any other runs `Recall::leave`.
    fn went(self, s: Step, cx: &NavCtx, r: &mut Recall) -> Step {
        match s {
            Step::Go(to) if to == self => Step::Stay,
            Step::Go(to) => {
                r.leave(self, to, cx);
                Step::Go(to)
            }
            s => s,
        }
    }

    fn next(self, k: NavKey, cx: &NavCtx, r: &Recall) -> Step {
        use Loc::*;
        let go = |l: Loc| Step::Go(Location(l));
        match (self.0, k) {
            // ORBIT's keys are `OrbitKey`s: no ladder key moves it.
            (Orbit(_), _) => Step::Stay,
            (Pages(p, _), NavKey::Part(n)) if p == n => go(r.mix_entry(n)),
            (Part(p, _) | Fx(p, _), NavKey::Part(n)) if p == n => go(Pages(n, r.pages_of(n, cx))),
            (_, NavKey::Part(n)) => go(Pages(n, cx.chain(n).def().home())),
            (Part(_, m), NavKey::MixPart(n)) => go(Part(n, m)),
            (Fx(_, at), NavKey::MixPart(n)) => go(Fx(n, at)),
            (_, NavKey::MixPart(n)) => go(r.mix_entry(n)),
            (_, NavKey::EditPart(n)) => go(Sound(n, Browse::default())),
            // `Recall::leave` keeps where it came from.
            (_, NavKey::SeqHold) => go(Orbit(r.orbit)),
            (Settings(s), NavKey::MenuTap) => match s.back() {
                Some(up) => go(Settings(up)),
                None => Step::Go(r.close_settings(cx)),
            },
            (_, NavKey::MenuTap) => Step::Go(Location::settings_at(&[], 0)),
            (Settings(s), k) => s.step(k, cx),
            (Pages(p, at), k) => page_step(cx.chain(p).def(), at, k, cx.pages)
                .map_or(Step::Stay, |at| go(Pages(p, at))),
            (Part(p, m), NavKey::Plus) => go(mix_walk(p, m, 1)),
            (Part(p, m), NavKey::Minus) => go(mix_walk(p, m, -1)),
            (Part(p, _), NavKey::Edit) => go(Sound(p, Browse::default())),
            // The mixer is the top of its chain; the Sound rung is under it.
            (Part(..), NavKey::SeqTap) => Step::Stay,
            (Sound(p, _), NavKey::SeqTap) => go(r.mix_entry(p)),
            (Fx(p, at), NavKey::Minus) if at.node() == FX_FIRST => go(Part(p, MixPage::Sends)),
            (Fx(p, at), k) => page_step(&MIXER_CHANNEL_CHAIN, at, k, cx.pages)
                .map(|to| skip_empty_rev(at, to, sub_move(cx.pages, k), cx.rev))
                .map_or(Step::Stay, |at| go(Fx(p, at))),
            (Sound(p, b), NavKey::Plus) => go(Sound(wrap(p, 1), b)),
            (Sound(p, b), NavKey::Minus) => go(Sound(wrap(p, -1), b)),
            _ => Step::Stay,
        }
    }

    /// The chain page shown; `None` on lists, Screens and the Sound rung.
    pub fn page(self, cx: &NavCtx) -> Option<(&'static ChainDef2, PageAt)> {
        match self.0 {
            Loc::Pages(p, at) => Some((cx.chain(p).def(), at)),
            Loc::Part(_, m) => Some((&MIXER_CHANNEL_CHAIN, m.page())),
            Loc::Fx(_, at) => Some((&MIXER_CHANNEL_CHAIN, at)),
            Loc::Settings(s) => s.at_leaf().map(|l| (l.chain(), l.chain().home())),
            Loc::Orbit(o) => Some((&ORBIT_CHAIN, o.page)),
            Loc::Sound(..) => None,
        }
    }

    /// The Sound rung's Part and browser.
    pub fn browse(self) -> Option<(PartId, Browse)> {
        match self.0 {
            Loc::Sound(p, b) => Some((p, b)),
            _ => None,
        }
    }

    pub fn part(self) -> Option<PartId> {
        match self.0 {
            Loc::Pages(p, _) | Loc::Part(p, _) | Loc::Fx(p, _) | Loc::Sound(p, _) => Some(p),
            Loc::Settings(_) | Loc::Orbit(_) => None,
        }
    }

    pub fn orbit(self) -> Option<OrbitAt> {
        match self.0 {
            Loc::Orbit(o) => Some(o),
            _ => None,
        }
    }

    pub fn settings(self) -> Option<SettingsAt> {
        match self.0 {
            Loc::Settings(s) => Some(s),
            _ => None,
        }
    }

    /// Exhaustive over `Loc`: a new place does not build until it has a rung.
    pub fn rung(self) -> Rung {
        match self.0 {
            Loc::Pages(p, at) => Rung::Pages(p, at),
            Loc::Part(p, m) => Rung::Mixer(p, m),
            Loc::Fx(p, at) => Rung::Fx(p, at),
            Loc::Sound(p, _) => Rung::Sound(p),
            Loc::Settings(s) => Rung::Settings(s),
            Loc::Orbit(o) => Rung::Orbit(o),
        }
    }
}

/// PLUS and MINUS in Part `p`'s mixer: PART, SENDS, then its FX, clamped;
/// other Parts' by MIX+B*n* (owner, 2026-10-02).
fn mix_walk(p: PartId, m: MixPage, d: i8) -> Loc {
    match (m, d > 0) {
        (MixPage::Part, true) => Loc::Part(p, MixPage::Sends),
        (MixPage::Sends, true) => Loc::Fx(p, FX_FIRST_PAGE),
        (_, false) => Loc::Part(p, MixPage::Part),
    }
}

fn wrap(p: PartId, d: i8) -> PartId {
    PartId::ALL[step_wrapped(p.index() as i32, d as i32, MAX_PARTS)]
}

/// `at` moved `delta` round `len` places; 0 with none.
pub(crate) fn step_wrapped(at: i32, delta: i32, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (at + delta).rem_euclid(len as i32) as usize
    }
}

/// `at` moved `delta`, held to `0..len` (`len` at least 1).
pub(crate) fn step_clamped(at: usize, delta: i32, len: usize) -> usize {
    (at as i32 + delta).clamp(0, len as i32 - 1) as usize
}

/// What a key does on a chain's pages. PLUS and MINUS step the node.
/// Under STACK, EDIT is the next sub-page (down) and SEQ the previous (up)
/// (ADR 0081); ROW draws the pages left to right and the panel has EDIT
/// left of SEQ, so there EDIT is the previous and SEQ the next (ADR 0145).
/// The next wraps from the last to the first; the previous stops at the
/// first.
pub fn sub_move(pages: Pages, k: NavKey) -> Option<Move> {
    match (pages, k) {
        (_, NavKey::Plus) => Some(Move::Next),
        (_, NavKey::Minus) => Some(Move::Prev),
        (Pages::Stack, NavKey::Edit) | (Pages::Row, NavKey::SeqTap) => Some(Move::Down),
        (Pages::Stack, NavKey::SeqTap) | (Pages::Row, NavKey::Edit) => Some(Move::Up),
        _ => None,
    }
}

/// `k`'s step on `c` (`sub_move`), clamped along, wrapping down.
fn page_step(c: &ChainDef2, at: PageAt, k: NavKey, pages: Pages) -> Option<PageAt> {
    let m = sub_move(pages, k)?;
    match c.step(at, m) {
        None if m == Move::Down => c.page(at.node() as usize, 0),
        to => to,
    }
}

/// Down onto REV › MORE 2 under a TYPE with no cells there goes on round
/// to REV; up never reaches it (it is the last).
fn skip_empty_rev(
    from: PageAt,
    to: PageAt,
    m: Option<Move>,
    rev: crate::dsp::reverb::ReverbType,
) -> PageAt {
    use crate::ui::reverb_panel::{RevPage, shows};
    let more2 = MIXER_CHANNEL_CHAIN
        .def_at(to)
        .is_some_and(|d| d.id == block_registry::EFX_MORE2.id);
    if more2 && m == Some(Move::Down) && !shows(rev, RevPage::More2) {
        MIXER_CHANNEL_CHAIN
            .page(from.node() as usize, 0)
            .unwrap_or(to)
    } else {
        to
    }
}

/// Part `n`'s mixer place showing `at`: PART, SENDS or an FX page.
fn mix_loc(n: PartId, at: PageAt) -> Loc {
    match at.node() as usize {
        MIXER_PART => Loc::Part(n, MixPage::Part),
        MIXER_HOME => Loc::Part(n, MixPage::Sends),
        _ => Loc::Fx(n, at),
    }
}

/// The static chain definition for an engine.
pub fn chain_def_for(engine: EngineType) -> &'static ChainDef2 {
    PartChain::base(engine).def()
}
