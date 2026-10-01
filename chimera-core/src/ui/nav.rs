//! Where the UI is, and where each key takes it: one `Location` and a pure
//! `step` (ADR 0044, amended by ADR 0066).

use crate::hw::MAX_PARTS;
use crate::params::EngineType;
use crate::project::PartId;
use crate::ui::block_def::ChainDef2;
use crate::ui::block_registry::{
    self, CHORUS, MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART, MODAL_1, MODAL_PLUCK_CHAIN,
};
use crate::ui::settings::{Act, Kind, Screen, row_at};

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
const PART_SETTINGS: [u8; 1] = [1];

/// RES, Modal's home (owner, 2026-10-01: ADR 0066).
const MODAL_HOME: u8 = {
    let b = MODAL_PLUCK_CHAIN.blocks;
    let mut i = 0;
    while b[i].def.id != MODAL_1.id {
        i += 1;
    }
    i as u8
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageAt {
    pub node: u8,
    pub sub: u8,
}

impl PageAt {
    const ZERO: PageAt = PageAt { node: 0, sub: 0 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixPage {
    Part,
    Sends,
}

impl MixPage {
    fn node(self) -> u8 {
        match self {
            MixPage::Part => MIXER_PART as u8,
            MixPage::Sends => MIXER_HOME as u8,
        }
    }
}

/// A place in SETTINGS: a list with its bar on `row`, a Screen, or a leaf
/// on `page`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsAt {
    path: [u8; MAX_DEPTH],
    depth: u8,
    row: u8,
    /// (0,0) on lists.
    page: PageAt,
}

impl SettingsAt {
    pub fn path(&self) -> &[u8] {
        &self.path[..self.depth as usize]
    }

    pub fn row(&self) -> u8 {
        self.row
    }

    pub fn page(&self) -> PageAt {
        self.page
    }

    pub fn at_leaf(&self) -> Option<&'static ChainDef2> {
        match self.kind()? {
            Kind::Leaf(c) => Some(c),
            _ => None,
        }
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
        Some(SettingsAt {
            path,
            depth: self.depth + 1,
            row: 0,
            page: PageAt::ZERO,
        })
    }

    /// One level up, the bar on the row just left.
    fn parent(self) -> Option<SettingsAt> {
        let d = self.depth.checked_sub(1)?;
        let mut path = self.path;
        // Zero past `depth`, so equal places compare equal.
        path[d as usize] = 0;
        Some(SettingsAt {
            path,
            depth: d,
            row: self.path[d as usize],
            page: PageAt::ZERO,
        })
    }

    fn step(self, k: NavKey, cx: &NavCtx) -> Step {
        let go =
            |s: Option<SettingsAt>| s.map_or(Step::Stay, |s| Step::Go(Location(Loc::Settings(s))));
        let bar = |n: usize, d: i32| {
            let row = if n == 0 {
                0
            } else {
                (self.row as i32 + d).rem_euclid(n as i32) as u8
            };
            go(Some(SettingsAt { row, ..self }))
        };
        let delta = match k {
            NavKey::Bar(d) => Some(d as i32),
            NavKey::Plus => Some(1),
            NavKey::Minus => Some(-1),
            _ => None,
        };
        match (self.kind(), k, delta) {
            (Some(Kind::Leaf(c)), k, _) => {
                go(page_step(c, self.page, k).map(|page| SettingsAt { page, ..self }))
            }
            (Some(Kind::List(rs)), _, Some(d)) => bar(rs.len(), d),
            (Some(Kind::Screen(_)), _, Some(d)) => bar(cx.dyn_rows as usize, d),
            (Some(Kind::Screen(_)), NavKey::SeqTap, _) => Step::Run,
            (Some(Kind::List(rs)), NavKey::Edit, _) => {
                match rs.get(self.row as usize).map(|r| r.kind) {
                    Some(Kind::List(_) | Kind::Leaf(_)) => go(self.child()),
                    Some(Kind::Screen(s)) => Step::Screen(s),
                    _ => Step::Stay,
                }
            }
            (Some(Kind::List(rs)), NavKey::SeqTap, _) => {
                match rs.get(self.row as usize).map(|r| r.kind) {
                    Some(Kind::Act(a)) => Step::Act(a),
                    _ => Step::Stay,
                }
            }
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
    /// The shared FX, `node` from CHORUS on the mixer chain.
    Fx(PageAt),
    /// The Sound rung; Task 8 adds the browser's cursor.
    Sound(PartId),
    Settings(SettingsAt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location(Loc);

/// A `Location` outside SETTINGS, where MENU at the top returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outside(Loc);

impl Outside {
    pub fn new(l: Location) -> Option<Outside> {
        match l.0 {
            Loc::Settings(_) => None,
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
    mix: MixPage,
    settings_from: Outside,
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
            mix: MixPage::Sends,
            settings_from: Outside(Location::HOME.0),
        }
    }

    pub fn settings_from(&self) -> Location {
        self.settings_from.get()
    }

    fn leave(&mut self, from: Location, to: Location, cx: &NavCtx) {
        match from.0 {
            Loc::Pages(p, at) => {
                self.pages[p.index()] = Some(SoundPage {
                    engine: cx.engine(p),
                    at,
                })
            }
            Loc::Part(_, m) => self.mix = m,
            _ => {}
        }
        if let (Some(o), Some(_)) = (Outside::new(from), to.settings()) {
            self.settings_from = o;
        }
    }

    /// Part n's pages from its own mixer: the page left, on the same engine.
    fn pages_of(&self, p: PartId, cx: &NavCtx) -> PageAt {
        match self.pages[p.index()] {
            Some(s) if s.engine == cx.engine(p) => s.at,
            _ => home(cx.engine(p)),
        }
    }
}

pub struct NavCtx {
    pub engines: [EngineType; MAX_PARTS],
    /// A Screen's row count.
    pub dyn_rows: u8,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Go(Location),
    Act(Act),
    /// EDIT on a Screen row: `UiState` lists it, then goes in.
    Screen(Screen),
    /// SEQ on a Screen's row: `UiState` decides.
    Run,
    Stay,
}

impl Location {
    /// Part 1's pages at (0,0): its engine's home at boot, when it is Algo.
    pub const HOME: Location = Location(Loc::Pages(PartId::ALL[0], PageAt::ZERO));

    pub fn pages(p: PartId, at: PageAt) -> Location {
        Location(Loc::Pages(p, at))
    }

    pub fn mixer(p: PartId, m: MixPage) -> Location {
        Location(Loc::Part(p, m))
    }

    pub fn sound(p: PartId) -> Location {
        Location(Loc::Sound(p))
    }

    /// Panics on a path deeper than the tree allows.
    pub fn settings_at(path: &[u8], row: u8) -> Location {
        let mut p = [0; MAX_DEPTH];
        p[..path.len()].copy_from_slice(path);
        Location(Loc::Settings(SettingsAt {
            path: p,
            depth: path.len() as u8,
            row,
            page: PageAt::ZERO,
        }))
    }

    pub fn step(self, k: NavKey, cx: &NavCtx, r: &mut Recall) -> Step {
        match self.next(k, cx, r) {
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
        let pages_home = |n: PartId| go(Pages(n, home(cx.engine(n))));
        match (self.0, k) {
            (Pages(p, _), NavKey::Part(n)) if p == n => go(Part(n, outside_mix(r.mix))),
            (Part(p, _), NavKey::Part(n)) if p == n => go(Pages(n, r.pages_of(n, cx))),
            (_, NavKey::Part(n)) => pages_home(n),
            (Part(_, m), NavKey::MixPart(n)) => go(Part(n, m)),
            (Fx(_), NavKey::MixPart(n)) => go(Part(n, r.mix)),
            (_, NavKey::MixPart(n)) => go(Part(n, outside_mix(r.mix))),
            (_, NavKey::EditPart(n)) => go(Sound(n)),
            (Settings(s), NavKey::MenuTap) => match s.parent() {
                Some(up) => go(Settings(up)),
                None => Step::Go(r.settings_from()),
            },
            (_, NavKey::MenuTap) => Step::Go(Location::settings_at(&[], 0)),
            (Settings(s), k) => s.step(k, cx),
            (Pages(p, at), k) => page_step(chain_def_for(cx.engine(p)), at, k)
                .map_or(Step::Stay, |at| go(Pages(p, at))),
            (Part(p, m), NavKey::Plus) => go(mix_walk(p, m, 1)),
            (Part(p, m), NavKey::Minus) => go(mix_walk(p, m, -1)),
            (Part(p, _), NavKey::Edit) => go(Sound(p)),
            (Part(..) | Sound(_), NavKey::SeqTap) => {
                Step::Go(Location::settings_at(&PART_SETTINGS, 0))
            }
            (Fx(at), NavKey::Minus) if at.node == FX_FIRST => {
                go(Part(PartId::ALL[MAX_PARTS - 1], MixPage::Sends))
            }
            (Fx(at), k) => {
                page_step(&MIXER_CHANNEL_CHAIN, at, k).map_or(Step::Stay, |at| go(Fx(at)))
            }
            (Sound(p), NavKey::Plus) => go(Sound(wrap(p, 1))),
            (Sound(p), NavKey::Minus) => go(Sound(wrap(p, -1))),
            _ => Step::Stay,
        }
    }

    /// The chain page shown; `None` on lists, Screens and the Sound rung.
    pub fn page(self, cx: &NavCtx) -> Option<(&'static ChainDef2, PageAt)> {
        match self.0 {
            Loc::Pages(p, at) => Some((chain_def_for(cx.engine(p)), at)),
            Loc::Part(_, m) => Some((
                &MIXER_CHANNEL_CHAIN,
                PageAt {
                    node: m.node(),
                    sub: 0,
                },
            )),
            Loc::Fx(at) => Some((&MIXER_CHANNEL_CHAIN, at)),
            Loc::Settings(s) => s.at_leaf().map(|c| (c, s.page)),
            Loc::Sound(_) => None,
        }
    }

    pub fn part(self) -> Option<PartId> {
        match self.0 {
            Loc::Pages(p, _) | Loc::Part(p, _) | Loc::Sound(p) => Some(p),
            Loc::Fx(_) | Loc::Settings(_) => None,
        }
    }

    pub fn settings(self) -> Option<SettingsAt> {
        match self.0 {
            Loc::Settings(s) => Some(s),
            _ => None,
        }
    }
}

/// Into the mixer from outside it: a remembered PART opens SENDS (ADR 0057).
fn outside_mix(m: MixPage) -> MixPage {
    match m {
        MixPage::Part => MixPage::Sends,
        m => m,
    }
}

/// PLUS and MINUS on a Part's rung: PART, SENDS, the next Part's PART… and
/// after Part 6's SENDS, the FX (owner, 2026-10-01).
fn mix_walk(p: PartId, m: MixPage, d: i8) -> Loc {
    let i = (p.index() * 2 + (m == MixPage::Sends) as usize) as i32 + d as i32;
    match u8::try_from(i / 2).ok().and_then(PartId::new) {
        _ if i < 0 => Loc::Part(p, m),
        Some(q) => Loc::Part(
            q,
            if i % 2 == 0 {
                MixPage::Part
            } else {
                MixPage::Sends
            },
        ),
        None => Loc::Fx(PageAt {
            node: FX_FIRST,
            sub: 0,
        }),
    }
}

fn wrap(p: PartId, d: i8) -> PartId {
    let n = (p.index() as i32 + d as i32).rem_euclid(MAX_PARTS as i32);
    PartId::ALL[n as usize]
}

/// PLUS and MINUS step the node, clamped; EDIT is sub-page down, SEQ up.
fn page_step(c: &ChainDef2, at: PageAt, k: NavKey) -> Option<PageAt> {
    let subs = c
        .block_at(at.node as usize)
        .map_or(0, |b| b.sub_page_count());
    match k {
        NavKey::Plus if (at.node as usize) + 1 < c.len() => Some(PageAt {
            node: at.node + 1,
            sub: 0,
        }),
        NavKey::Minus if at.node > 0 => Some(PageAt {
            node: at.node - 1,
            sub: 0,
        }),
        NavKey::Edit if (at.sub as usize) + 1 < subs => Some(PageAt {
            sub: at.sub + 1,
            ..at
        }),
        NavKey::SeqTap if at.sub > 0 => Some(PageAt {
            sub: at.sub - 1,
            ..at
        }),
        _ => None,
    }
}

/// Where an engine's pages open (owner, 2026-10-01: ADR 0066).
pub fn home(e: EngineType) -> PageAt {
    let node = match e {
        EngineType::Algo => 0,
        EngineType::Modal => MODAL_HOME,
    };
    PageAt { node, sub: 0 }
}

/// The static chain definition for an engine.
pub fn chain_def_for(engine: EngineType) -> &'static ChainDef2 {
    match engine {
        EngineType::Algo => &block_registry::ALGO_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
    }
}
