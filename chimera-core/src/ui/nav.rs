//! Where the UI is, and where each key takes it: one `Location` and a pure
//! `step` (ADR 0044, amended by ADR 0066).

use crate::hw::MAX_PARTS;
use crate::orbit::{RINGS, RingIdx};
use crate::params::EngineType;
use crate::project::PartId;
use crate::ui::block_def::{ChainDef2, Move};
use crate::ui::block_registry::{
    self, ALGO_CHAIN, CHORUS, MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART, MODAL_PLUCK_CHAIN,
};
use crate::ui::orbit::{ORB_NODE, ORBIT_CHAIN, RING_NODE, SCN_NODE};
use crate::ui::settings::leaves::OnePage;
use crate::ui::settings::{Act, Kind, MANAGE_COMMANDS, PART_ROW, Row, Screen, row_at};

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
/// SETTINGS › PART, where SEQ on the mixer and the Sound rung goes.
const PART_SETTINGS: [u8; 1] = [PART_ROW];

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

    /// MENU: MANAGE's commands back to its list, else one level up with the
    /// bar on the row just left.
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
        let wrap = |i: u8, n: usize, d: i32| {
            if n == 0 {
                0
            } else {
                (i as i32 + d).rem_euclid(n as i32) as u8
            }
        };
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
            // One page: no key steps it.
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
            (_, NavKey::Edit, _) if self.at == At::Manage(Column::Projects) && cx.dyn_rows > 0 => {
                go(SettingsAt {
                    at: At::Manage(Column::Command(0)),
                    ..self
                })
            }
            (Some(Kind::Screen(_)), _, Some(d)) => row(cx.dyn_rows as usize, d),
            (_, NavKey::SeqTap, _) if self.at == At::Manage(Column::Projects) => Step::Stay,
            (Some(Kind::Screen(_)), NavKey::SeqTap, _) => Step::Run,
            (Some(Kind::List(rs)), NavKey::Edit, _) => match under_bar(rs) {
                Some(Kind::List(_) | Kind::Leaf(_)) => self.child().map_or(Step::Stay, go),
                Some(Kind::Screen(s)) => Step::Screen(s),
                Some(Kind::Act(a)) => Step::Act(a),
                _ => Step::Stay,
            },
            (Some(Kind::List(rs)), NavKey::SeqTap, _) => match under_bar(rs) {
                Some(Kind::Act(a)) => Step::Act(a),
                Some(Kind::Screen(s)) => Step::Screen(s),
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
            OrbitKey::SeqTap => Step::Orbit(OrbitAct::SaveScene),
            OrbitKey::Plus => page(ORBIT_CHAIN.step(self.page, Move::Next)),
            OrbitKey::Minus => page(ORBIT_CHAIN.step(self.page, Move::Prev)),
            OrbitKey::Edit if self.on(RING_NODE) => {
                let n = ORBIT_CHAIN.blocks[RING_NODE].sub_page_count().max(1);
                page(ORBIT_CHAIN.page(RING_NODE, (sub + 1) % n))
            }
            // EDIT is down (#316): ORB's one sub-page, CUSTOM, opens while
            // SCALE is CUSTOM and in focus; from it EDIT wraps back to ORB,
            // even with SCALE named under it (a load, a scene switch).
            OrbitKey::Edit if self.on(ORB_NODE) && sub == 1 => page(ORBIT_CHAIN.page(ORB_NODE, 0)),
            OrbitKey::Edit if self.on(ORB_NODE) && cx.orbit.custom => page(Some(CUSTOM_PAGE)),
            // SCN's commit: SAVE → n.
            OrbitKey::Edit if self.on(SCN_NODE) => Step::Orbit(OrbitAct::SaveTo),
            OrbitKey::Edit => Step::Stay,
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
    /// SEQ: `live` over the playing scene.
    SaveScene,
    /// EDIT on SCN: `live` to SAVE's slot, asking over another saved one.
    SaveTo,
}

/// What ORBIT's keys read from the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitCx {
    /// Each ring's Part: where a held B*n* crosses to.
    pub targets: [PartId; RINGS],
    /// SCALE is CUSTOM and ORB's focus is on it: EDIT on ORB opens CUSTOM.
    pub custom: bool,
}

impl OrbitCx {
    /// Ring *n* plays Part *n*; SCALE isn't CUSTOM.
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
        custom: false,
    };
}

/// Where `Recall` starts, before anything is left: re-resolved on use.
const PLACEHOLDER: Loc = Loc::Pages(PartId::ALL[0], ALGO_CHAIN.home());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location(Loc);

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
    engine: EngineType,
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
                    engine: cx.engine(p),
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

    /// Part n's pages left, if on the same engine; else its home.
    fn pages_of(&self, p: PartId, cx: &NavCtx) -> PageAt {
        match self.pages[p.index()] {
            Some(s) if s.engine == cx.engine(p) => s.at,
            _ => chain_def_for(cx.engine(p)).home(),
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
    pub engines: [EngineType; MAX_PARTS],
    /// A Screen's row count.
    pub dyn_rows: u8,
    pub orbit: OrbitCx,
}

impl NavCtx {
    fn engine(&self, p: PartId) -> EngineType {
        self.engines[p.index()]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Go(Location),
    Act(Act),
    /// EDIT or SEQ on a Screen row: `UiState` lists it, then goes in.
    Screen(Screen),
    /// SEQ inside a Screen: `UiState` decides; on MANAGE, `column()` says which.
    Run,
    /// An ORBIT key's act: `UiState` runs it.
    Orbit(OrbitAct),
    Stay,
}

impl Location {
    /// Part 1's pages on its engine's home.
    pub fn home(cx: &NavCtx) -> Location {
        let p = PartId::ALL[0];
        Self::part_home(p, cx.engine(p))
    }

    /// Part `p`'s pages on `engine`'s home.
    pub fn part_home(p: PartId, engine: EngineType) -> Location {
        Location(Loc::Pages(p, chain_def_for(engine).home()))
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
            (_, NavKey::Part(n)) => go(Pages(n, chain_def_for(cx.engine(n)).home())),
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
            (Pages(p, at), k) => page_step(chain_def_for(cx.engine(p)), at, k)
                .map_or(Step::Stay, |at| go(Pages(p, at))),
            (Part(p, m), NavKey::Plus) => go(mix_walk(p, m, 1)),
            (Part(p, m), NavKey::Minus) => go(mix_walk(p, m, -1)),
            (Part(p, _), NavKey::Edit) => go(Sound(p, Browse::default())),
            (Part(..) | Fx(..) | Sound(..), NavKey::SeqTap) => {
                Step::Go(Location::settings_at(&PART_SETTINGS, 0))
            }
            (Fx(p, at), NavKey::Minus) if at.node() == FX_FIRST => go(Part(p, MixPage::Sends)),
            (Fx(p, at), k) => {
                page_step(&MIXER_CHANNEL_CHAIN, at, k).map_or(Step::Stay, |at| go(Fx(p, at)))
            }
            (Sound(p, b), NavKey::Plus) => go(Sound(wrap(p, 1), b)),
            (Sound(p, b), NavKey::Minus) => go(Sound(wrap(p, -1), b)),
            _ => Step::Stay,
        }
    }

    /// The chain page shown; `None` on lists, Screens and the Sound rung.
    pub fn page(self, cx: &NavCtx) -> Option<(&'static ChainDef2, PageAt)> {
        match self.0 {
            Loc::Pages(p, at) => Some((chain_def_for(cx.engine(p)), at)),
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
    let n = (p.index() as i32 + d as i32).rem_euclid(MAX_PARTS as i32);
    PartId::ALL[n as usize]
}

/// PLUS and MINUS step the node, clamped; EDIT is sub-page down, SEQ up.
fn page_step(c: &ChainDef2, at: PageAt, k: NavKey) -> Option<PageAt> {
    let m = match k {
        NavKey::Plus => Move::Next,
        NavKey::Minus => Move::Prev,
        NavKey::Edit => Move::Down,
        NavKey::SeqTap => Move::Up,
        _ => return None,
    };
    c.step(at, m)
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
    match engine {
        EngineType::Algo => &block_registry::ALGO_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
    }
}
