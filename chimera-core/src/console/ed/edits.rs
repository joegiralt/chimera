// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ed set <addr> <value>`: the value resolved here, bank 2's, queued for
//! the shell's UI pass, which applies it as a knob turn (`UiState::edit_to`,
//! ADR 0112's glide): never a snap, never a zipper. A pad's lands on that
//! pad, whichever the box has selected (ADR 0285); a Part's on that Part,
//! whichever the box shows (ADR 0289).

use core::fmt::{self, Write};
use core::mem::MaybeUninit;

use super::addr::{EdBlock, GlobalBlock};
use super::lit::{HERE, lit, quoted};
use super::project::ProjectCmd;
use super::route::{RouteCell, RouteDest, RouteTo};
use super::schema::Hashes;
use super::value::{engine_of, find};
use super::{EdAddr, EdValue, OptIndex};
use crate::addr::BlockRead;
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::OpKind;
use crate::project::Line;
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::ed_cue::{LoadMark, Pass, RouteAt};
use crate::ui::page::ValFmt;
use crate::ui::{ed_cue, held, kit_groups, view};
use chimera_engine::params::EngineType;

/// Sets waiting for the next UI pass. A lap of a browser's sliders is a
/// handful; more than this is the host outrunning the pass.
pub const CAP: usize = 16;

/// What a set was taken for, so it lands there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Taken {
    /// The followed Part's Sound (or kit, or a pad), on `engine`: a Part
    /// that changed engine since drops it.
    Sound { part: PartId, engine: EngineType },
    /// Part `n`'s mix or sends, whichever Part the box shows.
    Part(PartId),
    /// A bus: the mixer's, the same from any Part.
    Bus,
    /// THEME or MIDI CONFIG: the box's, the same from any Part.
    Global,
    /// RANDOMIZE on Part `n`, the followed and active one: a turn rolls
    /// the active Part's Sound (ADR 0232), so a box that has moved to
    /// another Part since drops it.
    Roll(PartId),
    /// ORBIT's: the song's, the same from any Part, through
    /// `OrbitSong::edit` (ADR 0082, 0083), never a Part's blocks.
    Orbit,
}

impl Taken {
    /// What a set of `at` is taken for while following `part` on
    /// `engine`; `None` for a block no set reaches yet.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.of"))]
    fn of(at: EdBlock, part: PartId, engine: EngineType) -> Option<Taken> {
        match at {
            EdBlock::Sound(_) | EdBlock::Pad(..) => Some(Taken::Sound { part, engine }),
            EdBlock::Part(n, _) => Some(Taken::Part(n)),
            EdBlock::Bus(_) => Some(Taken::Bus),
            EdBlock::Global(GlobalBlock::Theme | GlobalBlock::Midi) => Some(Taken::Global),
            EdBlock::Global(GlobalBlock::Random) => Some(Taken::Roll(part)),
            EdBlock::Orbit(_) => Some(Taken::Orbit),
        }
    }

    /// The Part it lands on now; `None`: it is dropped.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.lands"))]
    fn lands(self, ui: &UiState) -> Option<PartId> {
        match self {
            Taken::Sound { part, engine } => {
                (ui.project().part(part).sound.engine() == engine).then_some(part)
            }
            Taken::Part(n) => Some(n),
            Taken::Bus | Taken::Global | Taken::Orbit => Some(ui.active_part),
            Taken::Roll(n) => (ui.active_part == n).then_some(n),
        }
    }
}

#[derive(Clone, Copy)]
struct Pending {
    taken: Taken,
    addr: EdAddr,
    to: To,
}

impl Pending {
    /// A slot nothing is queued in (`len` says so).
    const SPARE: Pending = Pending {
        taken: Taken::Bus,
        addr: EdAddr::SPARE,
        to: To::Num(0.0),
    };
}

/// A set's value: a choice's option, or a continuous param's number.
#[derive(Clone, Copy, Debug, PartialEq)]
enum To {
    Opt(OptIndex),
    Num(f32),
}

impl To {
    /// As the cell stores it.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.stored"))]
    fn stored(self) -> f32 {
        match self {
            To::Opt(i) => f32::from(i.get()),
            To::Num(v) => v,
        }
    }
}

/// Commands queued beside the sets (ADR 0289): a lap of a patch bay's
/// cables is a handful.
pub const ACTS: usize = 8;

/// A command queued for the UI pass, after the sets.
#[derive(Clone, Copy)]
enum Act {
    /// `ed route`: kept from pass to pass until the cell holds its amount
    /// (a step a pass, never a jump), or the box leaves the Part it was
    /// taken for, or a Sound or a project is loaded under it (`mark`): the
    /// act was that Sound's. `to` `None` removes the route.
    Route {
        taken: Taken,
        at: RouteAt,
        to: Option<i8>,
        mark: LoadMark,
    },
    /// `ed projects`: kept until the card is free to list (BUSY).
    Projects,
    /// `ed project …`: kept until the card is free; checked again then.
    Project(ProjectCmd),
    /// `ed map …`, `ed maps`, `ed learn`: checked again in the pass, a
    /// listing kept until no map job waits, learn behind a load.
    #[cfg(feature = "midi-map")]
    Map(super::map::MapAct),
    /// `ed orbit`, `ed scene`, `ed chord|scale|step|pool`: checked again
    /// in the pass; the transport kept until every command before it is
    /// applied.
    Orbit(super::orbit::OrbitCmd),
}

/// What `EdState::push` made of a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Queued,
    Full,
}

/// The refusals `ed since` says at most: the newest.
pub const REFUSED: usize = 4;

/// A set the box refused at its turn, on its Part, and why: the toast's
/// words.
#[derive(Clone, Copy)]
struct Refused {
    part: PartId,
    addr: EdAddr,
    why: Line,
}

/// The turns refused since `ed since` last said them, oldest first: the
/// newest `REFUSED`. Uninit past `len`, so `EdState::new`, built in bank 1
/// (`take_ed`), writes one byte for them; shifted by hand, as core's rotate
/// would link into bank 1.
pub(super) struct Refusals {
    items: [MaybeUninit<Refused>; REFUSED],
    len: u8,
}

impl Refusals {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.edits.refusals_new")
    )]
    pub(super) const fn new() -> Refusals {
        Refusals {
            items: [MaybeUninit::uninit(); REFUSED],
            len: 0,
        }
    }

    /// `addr` on `part` refused for `why`: the oldest dropped when full.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.edits.refusals_push")
    )]
    fn push(&mut self, part: PartId, addr: EdAddr, why: Line) {
        let mut at = usize::from(self.len);
        if at == REFUSED {
            let mut i = 1;
            while i < REFUSED {
                self.items[i - 1] = self.items[i];
                i += 1;
            }
            at = REFUSED - 1;
        }
        self.items[at] = MaybeUninit::new(Refused { part, addr, why });
        self.len = (at + 1) as u8;
    }

    /// `refused <ADDR> "<why>"`, one line each of `part`'s: an address
    /// names no Part, so another's would read as this one's.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.edits.refusals_write")
    )]
    pub(super) fn write(&self, part: PartId, w: &mut dyn Write) -> fmt::Result {
        let mut i = 0;
        while i < usize::from(self.len) {
            // SAFETY: `push` writes every item below `len` before raising
            // it, and `len` never passes `REFUSED`.
            let r = unsafe { self.items[i].assume_init_ref() };
            if r.part == part {
                w.write_str(REFUSED_HEAD.get())?;
                r.addr.write(w)?;
                quoted(w, r.why.as_str())?;
            }
            i += 1;
        }
        Ok(())
    }

    /// Said, or no longer the followed Part's: none left.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.edits.refusals_clear")
    )]
    pub(super) fn clear(&mut self) {
        self.len = 0;
    }
}

/// What the console keeps for `ed`, which the shell owns: the sets taken
/// and not yet applied by the UI pass (as `Hands` queues `Gesture`s, a fixed
/// array), the turns refused since `ed since` last said so, and its copy of
/// what it last reported.
pub struct EdState {
    items: [Pending; CAP],
    len: u8,
    /// `ed part`'s Part, for the same pass: the last asked wins.
    part: Option<PartId>,
    /// The turns refused, for `ed since`'s `refused` lines.
    pub(super) refused: Refusals,
    /// `ed since`'s copy and number (`since.rs`).
    pub(super) since: super::EdSince,
    /// `ed schema`'s hashes, made once a boot.
    pub(super) hashes: Hashes,
    /// Commands for the UI pass, uninit past `acts_len`: `EdState::new`,
    /// which bank 1 builds in place, writes one byte for them.
    acts: [MaybeUninit<Act>; ACTS],
    acts_len: u8,
}

impl EdState {
    /// Sets the queue holds.
    pub const CAP: usize = CAP;

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.new"))]
    pub const fn new() -> Self {
        EdState {
            items: [Pending::SPARE; CAP],
            len: 0,
            part: None,
            refused: Refusals::new(),
            since: super::EdSince::new(),
            hashes: Hashes::new(),
            acts: [MaybeUninit::uninit(); ACTS],
            acts_len: 0,
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.push"))]
    fn push(&mut self, taken: Taken, addr: EdAddr, to: To) -> Pushed {
        match self.items.get_mut(usize::from(self.len)) {
            None => Pushed::Full,
            Some(slot) => {
                *slot = Pending { taken, addr, to };
                self.len += 1;
                Pushed::Queued
            }
        }
    }

    /// The acts queued, oldest first.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.acts"))]
    fn acts(&self) -> &[MaybeUninit<Act>] {
        self.acts.get(..usize::from(self.acts_len)).unwrap_or(&[])
    }

    /// Act `i`, queued.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.act"))]
    fn act(&self, i: usize) -> Option<Act> {
        // SAFETY: `acts()` is the first `acts_len` slots, each written by
        // `push_act` before `acts_len` counted it.
        self.acts().get(i).map(|a| unsafe { a.assume_init() })
    }

    /// `ed route` queued: a route already queued on the same cell,
    /// wherever it is among the acts, takes the new amount (and the new
    /// mark), so a drag keeps one act and a cell never has two pulling it
    /// apart.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.edits.push_route")
    )]
    fn push_route(&mut self, taken: Taken, at: RouteAt, to: Option<i8>, mark: LoadMark) -> Pushed {
        let new = Act::Route {
            taken,
            at,
            to,
            mark,
        };
        let mut i = 0;
        while let Some(b) = self.act(i) {
            if let Act::Route {
                taken: t, at: a, ..
            } = b
                && t == taken
                && a == at
            {
                if let Some(slot) = self.acts.get_mut(i) {
                    slot.write(new);
                }
                return Pushed::Queued;
            }
            i += 1;
        }
        self.push_act(new)
    }

    /// `ed map set` queued: one on the same row and field takes the new
    /// value, so a drag keeps one act; any other after the rest.
    #[cfg(feature = "midi-map")]
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.push_map"))]
    fn push_map(&mut self, a: super::map::MapAct) -> Pushed {
        use super::map::MapAct;
        if let MapAct::Set {
            part,
            row,
            was,
            field,
        } = a
        {
            let mut i = 0;
            while let Some(b) = self.act(i) {
                if let Act::Map(MapAct::Set {
                    part: p,
                    row: r,
                    was: v,
                    field: f,
                }) = b
                    && p == part
                    && r == row
                    && v == was
                    && core::mem::discriminant(&f) == core::mem::discriminant(&field)
                {
                    if let Some(slot) = self.acts.get_mut(i) {
                        slot.write(Act::Map(a));
                    }
                    return Pushed::Queued;
                }
                i += 1;
            }
        }
        self.push_act(Act::Map(a))
    }

    /// `a` queued after the rest.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.push_act"))]
    fn push_act(&mut self, a: Act) -> Pushed {
        match self.acts.get_mut(usize::from(self.acts_len)) {
            None => Pushed::Full,
            Some(slot) => {
                slot.write(a);
                self.acts_len += 1;
                Pushed::Queued
            }
        }
    }

    /// Every act, oldest first, on the Part it was taken for while the box
    /// still edits it; an unfinished route stays for the next pass, unless
    /// its Sound was loaded over.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.edits.apply_acts")
    )]
    fn apply_acts(&mut self, ui: &mut UiState) {
        let (mut i, mut kept) = (0, 0);
        while let Some(a) = self.act(i) {
            i += 1;
            let pass = match a {
                Act::Route {
                    taken,
                    at,
                    to,
                    mark,
                } => match taken.lands(ui) {
                    Some(p) if p == ui.active_part && ed_cue::load_mark(ui, p) == mark => {
                        ed_cue::route(ui, at, to)
                    }
                    Some(_) | None => Pass::Done,
                },
                Act::Projects => ed_cue::card(ui, None),
                Act::Project(c) => ed_cue::card(ui, Some(c)),
                #[cfg(feature = "midi-map")]
                Act::Map(m) => super::map::apply(ui, m),
                Act::Orbit(c) => ed_cue::orbit(ui, c),
            };
            if let Pass::Wait = pass
                && let Some(slot) = self.acts.get_mut(kept)
            {
                slot.write(a);
                kept += 1;
            }
        }
        self.acts_len = kept as u8;
    }

    /// Follow `p` on the next UI pass, as its key would.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.follow"))]
    pub fn follow(&mut self, p: PartId) {
        self.part = Some(p);
    }

    /// Every queued set, oldest first, as a knob turn on the Part it was
    /// taken for (a Part that has changed engine since drops its Sound's
    /// sets), then the Part to follow: the sets were taken for theirs.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.apply"))]
    pub fn apply(&mut self, ui: &mut UiState) {
        let mut i = 0;
        while i < usize::from(self.len) {
            if let Some(&p) = self.items.get(i)
                && let Some(at) = p.taken.lands(ui)
                && let Some(why) = turn(ui, at, p)
            {
                self.refused.push(at, p.addr, why);
            }
            i += 1;
        }
        self.len = 0;
        self.apply_acts(ui);
        if let Some(p) = self.part.take() {
            ui.follow_part(p);
        }
        // The pass runs after the hand's input and its flush: ORBIT's
        // edits go to the runner's log here, before the frame's publish
        // (ADR 0082). Nothing touched, nothing sent.
        ui.flush_orbit();
    }
}

/// `p` as a knob turn. A pad's is the selected pad's turn with its pad
/// selected for it, and the box's selection put back: the box never sees
/// it move, and the edit takes the knob's whole path (ADR 0280's CHOKE
/// check with it). The refusal said on the box, if it was refused.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.turn"))]
fn turn(ui: &mut UiState, at: PartId, p: Pending) -> Option<Line> {
    let (a, pad) = (p.addr.param(), p.addr.pad());
    if let Taken::Orbit = p.taken {
        // Dimmed now (BPM under a MIDI clock): inert, as on the box.
        if !dimmed(ui, p.addr) {
            ed_cue::orbit_set(ui, a, p.to.stored());
        }
        return None;
    }
    if let Some(why) = refusal(ui, at, p) {
        // To the value it has: no change, the hold kept, nothing said.
        if unchanged(ui, at, p) {
            return None;
        }
        ed_cue::refused(ui, at, a, pad, why);
        return Some(why);
    }
    let to = p.to.stored();
    match pad {
        None => {
            ui.edit_to(at, a, to);
        }
        Some(n) => {
            let was = ui.project().selected_pad(at);
            ui.project_mut().select_pad(at, n);
            ui.edit_to(at, a, to);
            ui.project_mut().select_pad(at, was);
        }
    }
    ed_cue::landed(ui, at, a, pad);
    None
}

/// `a`'s cell is drawn dimmed and inert where a page shows it: BPM while a
/// MIDI clock leads (`view::is_dimmed`).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.dimmed"))]
fn dimmed(ui: &UiState, a: EdAddr) -> bool {
    let own = ui.cell_own();
    match find(ui, &own, a) {
        Some((def, i, ctx)) => view::is_dimmed(
            &view::view(def, i, &ctx),
            &ui.project().part(ui.active_part).sound,
            &ctx,
        ),
        None => false,
    }
}

/// `p` sets its Sound cell on Part `at` to the value stored there already.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.unchanged"))]
fn unchanged(ui: &UiState, at: PartId, p: Pending) -> bool {
    let a = p.addr.param();
    let params = &ui.project().part(at).sound.params;
    params
        .block(a.block)
        .is_some_and(|b| b.get(a.param) == p.to.stored())
}

/// Why the box refuses `p` before its write, as its toast reads: TAPE's
/// MODEL or DIRECTION onto what a streamed source can't play (the turn's
/// and a CC's check), or an operator kind or algorithm past the formant
/// budget (a CC's words), or a pad's CHOKE onto a 9th voice group
/// (kit_groups' words, ADR 0280); `None`: the write goes ahead.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.refusal"))]
fn refusal(ui: &UiState, at: PartId, p: Pending) -> Option<Line> {
    // Each cell refused here is a choice: a number never reaches one.
    let To::Opt(i) = p.to else {
        return None;
    };
    let a = p.addr.param();
    let sound = &ui.project().part(at).sound;
    if a == held::CHOKE
        && let Some(n) = p.addr.pad()
    {
        return kit_groups::refuses(ui.project().kit(at), n, p.to.stored());
    }
    let src = ui.streams(at);
    if let Some(t) = held::TapeTurn::of(a, p.to.stored()) {
        return held::Pick::Tape(t).why(sound, src).map(held::reason);
    }
    let pick = match held::applies(a, false)? {
        held::Cell::Wave(op) => held::Pick::Wave(op, OpKind::clamped(i.get())),
        held::Cell::Alg(side) => held::Pick::Alg(side, AlgoId::clamped(i.get())),
    };
    pick.why(sound, src).map(|_| ed_cue::formant_full())
}

/// Why `ed set` can't take a value: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetRefusal {
    /// `#n` past the choice's last option.
    NoSuchOption,
    /// A name or `#n` on a continuous param.
    TakesANumber,
    /// A word that names none of the choice's options.
    TakesAName,
}

impl SetRefusal {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.words"))]
    pub fn words(self) -> &'static str {
        match self {
            SetRefusal::NoSuchOption => NO_SUCH_OPTION.get(),
            SetRefusal::TakesANumber => TAKES_A_NUMBER.get(),
            SetRefusal::TakesAName => TAKES_A_NAME.get(),
        }
    }
}

/// Why `EdState::stored_set` won't take a stored value.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoredRefusal {
    /// `describe` lists no such address for the followed Part's engine.
    NotHere,
    /// NaN or an infinity.
    NotFinite,
    /// What `ed set` says of it.
    Set(SetRefusal),
}

#[cfg(any(test, feature = "test-support"))]
impl EdState {
    /// What `ed set <addr>` stores for the stored value `v`, sent as the
    /// cell's own kind says (a choice's `#n`, else the number): checked as
    /// `set` checks it, nothing queued. For a mirror of the unit's screens
    /// (chimera-screen), which writes it itself.
    pub fn stored_set(&mut self, ui: &UiState, addr: EdAddr, v: f32) -> Result<f32, StoredRefusal> {
        if !v.is_finite() {
            return Err(StoredRefusal::NotFinite);
        }
        if !self.since.lists(engine_of(ui), addr) {
            return Err(StoredRefusal::NotHere);
        }
        let val = match fmt_of(ui, addr).and_then(super::describe::names) {
            Some(_) if v.fract() == 0.0 && (0.0..=255.0).contains(&v) => {
                EdValue::Index(OptIndex::new(v as u8))
            }
            Some(_) => return Err(StoredRefusal::Set(SetRefusal::NoSuchOption)),
            None => EdValue::Num(v, super::super::Word::new(b"")),
        };
        resolve(ui, addr, val)
            .map(To::stored)
            .map_err(StoredRefusal::Set)
    }
}

impl Default for EdState {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.default"))]
    fn default() -> Self {
        Self::new()
    }
}

/// `a`'s format as the Part shows it now: its cell's where a page shows
/// it, else its spec's (as `ed get` reads it).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.fmt_of"))]
fn fmt_of(ui: &UiState, a: EdAddr) -> Option<ValFmt> {
    let own = ui.cell_own();
    match find(ui, &own, a) {
        Some((def, i, ctx)) => Some(view::view(def, i, &ctx).fmt()),
        None => Some(a.param().spec()?.fmt),
    }
}

/// The stored value `val` means for `a`, or why not: a choice's name (its
/// own, matched exactly, the typed word kept), its option number, or a
/// number.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.resolve"))]
fn resolve(ui: &UiState, a: EdAddr, val: EdValue) -> Result<To, SetRefusal> {
    let names = fmt_of(ui, a).and_then(super::describe::names);
    match (val, names) {
        (EdValue::Index(n), Some(names)) => match names.get(usize::from(n.get())) {
            Some(_) => Ok(To::Opt(n)),
            None => Err(SetRefusal::NoSuchOption),
        },
        (EdValue::Index(_), None) => Err(SetRefusal::TakesANumber),
        (EdValue::Num(_, word) | EdValue::Name(word), Some(names)) => {
            let mut i = 0;
            while i < names.len() {
                if names[i].as_bytes() == word.as_bytes() {
                    return u8::try_from(i)
                        .map(|n| To::Opt(OptIndex::new(n)))
                        .map_err(|_| SetRefusal::NoSuchOption);
                }
                i += 1;
            }
            Err(SetRefusal::TakesAName)
        }
        (EdValue::Num(n, _), None) => Ok(To::Num(n)),
        (EdValue::Name(_), None) => Err(SetRefusal::TakesANumber),
    }
}

/// `ed route`'s answer, `OK queued` or `ERR`: on the followed Part's
/// engine, a tap's cell only on ALGO.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.route"))]
pub(super) fn route(
    c: RouteCell,
    to: RouteTo,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let Some(q) = q else {
        return w.write_str(ROUTE_UNBUILT.get());
    };
    let engine = engine_of(ui);
    if !c.on(engine) {
        w.write_str(ROUTE_HAS_NO.get())?;
        c.write(w)?;
        return w.write_str(HERE.get());
    }
    let at = match c {
        RouteCell::Matrix {
            src,
            dest: RouteDest::Fixed(f),
        } => RouteAt::Fixed(src.row(), f),
        RouteCell::Matrix {
            src,
            dest: RouteDest::Reg(a),
        } => RouteAt::Reg(src.row(), a.param()),
        RouteCell::Jack(j) => RouteAt::Jack(j),
    };
    let to = match to {
        RouteTo::Amount(a) => Some(a.get()),
        RouteTo::Off => None,
    };
    let taken = Taken::Sound {
        part: ui.active_part,
        engine,
    };
    let mark = ed_cue::load_mark(ui, ui.active_part);
    match q.push_route(taken, at, to, mark) {
        Pushed::Full => w.write_str(BUSY.get()),
        Pushed::Queued => w.write_str(QUEUED.get()),
    }
}

/// `ed projects`' answer: one re-list queued, however often asked.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.projects"))]
pub(super) fn projects(q: Option<&mut EdState>, w: &mut dyn Write) -> fmt::Result {
    let Some(q) = q else {
        return w.write_str(PROJECTS_UNBUILT.get());
    };
    let mut i = 0;
    while let Some(a) = q.act(i) {
        if let Act::Projects = a {
            return w.write_str(QUEUED.get());
        }
        i += 1;
    }
    queued(q.push_act(Act::Projects), w)
}

/// `ed project …`'s answer: what the box would refuse now is said at
/// once; the rest is queued, and checked again in the UI pass.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.project"))]
pub(super) fn project(
    c: ProjectCmd,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let Some(q) = q else {
        return w.write_str(PROJECT_UNBUILT.get());
    };
    match super::project::check(c, ui) {
        Err(why) => why.write(w),
        Ok(()) => queued(q.push_act(Act::Project(c)), w),
    }
}

/// `ed map …`'s answer: what the box would refuse now is said at once;
/// the rest is queued, and checked again in the UI pass.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.map"))]
pub(super) fn map(
    c: super::map::MapCmd,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let Some(q) = q else {
        return w.write_str(MAP_UNBUILT.get());
    };
    match super::map::check(c, ui) {
        Err(why) => why.write(w),
        Ok(a) => queued(q.push_map(a), w),
    }
}

/// `ed maps`' answer: one listing queued, however often asked.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.maps"))]
pub(super) fn maps(q: Option<&mut EdState>, w: &mut dyn Write) -> fmt::Result {
    let Some(q) = q else {
        return w.write_str(MAP_UNBUILT.get());
    };
    let mut i = 0;
    while let Some(a) = q.act(i) {
        if let Act::Map(super::map::MapAct::List) = a {
            return w.write_str(QUEUED.get());
        }
        i += 1;
    }
    queued(q.push_act(Act::Map(super::map::MapAct::List)), w)
}

/// `ed learn`'s answer: an address `describe` lists for the followed Part
/// on a Part's own page or its mixer, a knob a control may drive.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.learn"))]
pub(super) fn learn(
    c: super::map::LearnCmd,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let Some(q) = q else {
        return w.write_str(MAP_UNBUILT.get());
    };
    let listed = match c {
        super::map::LearnCmd::At(a) => q.since.lists(engine_of(ui), a),
        super::map::LearnCmd::Off => true,
    };
    match super::map::learn_check(c, ui, listed) {
        Err(why) => why.write(w),
        Ok(a) => queued(q.push_map(a), w),
    }
}

/// `ed orbit|scene|chord|scale|step|pool …`'s answer: what the box would
/// refuse now is said at once; the rest is queued, and checked again in
/// the UI pass.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.orbit"))]
pub(super) fn orbit(
    c: super::orbit::OrbitCmd,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let Some(q) = q else {
        return super::orbit::NotNow::Unbuilt.write(w);
    };
    match super::orbit::check(c, ui) {
        Err(why) => why.write(w),
        Ok(()) => queued(q.push_act(Act::Orbit(c)), w),
    }
}

/// `OK queued`, or `ERR busy` past the queue.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.queued"))]
fn queued(p: Pushed, w: &mut dyn Write) -> fmt::Result {
    match p {
        Pushed::Full => w.write_str(BUSY.get()),
        Pushed::Queued => w.write_str(QUEUED.get()),
    }
}

/// The answer, `OK queued` or `ERR`: whole, for `answer_in`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.set"))]
pub(super) fn set(
    addr: EdAddr,
    val: EdValue,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let part = ui.active_part;
    let engine = engine_of(ui);
    let Some(q) = q else {
        return w.write_str(SET_UNBUILT.get());
    };
    let taken = Taken::of(addr.at(), part, engine);
    let Some(taken) = taken.filter(|_| q.since.lists(engine, addr)) else {
        w.write_str(SET_HAS_NO.get())?;
        addr.write(w)?;
        return w.write_str(HERE.get());
    };
    if let Taken::Orbit = taken
        && dimmed(ui, addr)
    {
        w.write_str(SET.get())?;
        addr.write(w)?;
        return w.write_str(DIMMED.get());
    }
    let to = match resolve(ui, addr, val) {
        Ok(to) => to,
        Err(why) => {
            w.write_str(SET.get())?;
            addr.write(w)?;
            w.write_char(' ')?;
            w.write_str(why.words())?;
            return w.write_char('\n');
        }
    };
    match q.push(taken, addr, to) {
        Pushed::Full => w.write_str(BUSY.get()),
        Pushed::Queued => w.write_str(QUEUED.get()),
    }
}

lit!(
    /// A refused set's line's head.
    REFUSED_HEAD = "refused "
);
lit!(NO_SUCH_OPTION = "has no such option");
lit!(
    /// A set of a dimmed cell's tail: BPM while a MIDI clock leads.
    DIMMED = " is dimmed: a MIDI clock leads\n"
);
lit!(TAKES_A_NUMBER = "takes a number");
lit!(TAKES_A_NAME = "takes one of its names");
lit!(ROUTE_UNBUILT = "ERR ed route is not in this build\n");
lit!(PROJECTS_UNBUILT = "ERR ed projects is not in this build\n");
lit!(PROJECT_UNBUILT = "ERR ed project is not in this build\n");
#[cfg(feature = "midi-map")]
lit!(MAP_UNBUILT = "ERR ed map is not in this build\n");
lit!(ROUTE_HAS_NO = "ERR ed route has no ");
lit!(SET_UNBUILT = "ERR ed set is not in this build\n");
lit!(SET_HAS_NO = "ERR ed set has no ");
lit!(
    /// A set's refusal's head, before its address and why.
    SET = "ERR ed set "
);
lit!(
    /// The queue is full.
    BUSY = "ERR busy\n"
);
lit!(
    /// A set or route taken, for the UI pass.
    QUEUED = "OK queued\n"
);
