// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI MAP's pointing (ADR 0288): EDIT on `+ NEW BINDING` or on a Sound
//! row walks the box's own pages; a turn points at a cell, as learn's
//! armed turn does (`learn_turns`), and EDIT binds it there: a new row on
//! the first CC no row and no built-in control uses, or the row's new
//! target, its control, range and echo kept. MENU, tapped or held,
//! cancels. Either way MIDI MAP opens again on the row. No controller is
//! needed. Bank 2's, behind `MidiUiDoor`, high in an S4 group (ADR 0272);
//! bank 1 keeps `Learn::Pointing` and the stubs.

use core::fmt::Write;

use chimera_midi::take::Takeover;

use chimera_hal::ALL_ENCODERS;

use super::UiState;
use super::fmt::FmtBuf;
use super::hint::{Gesture, Hint};
use super::learn::{Aim, AimKind, Cell, Learn, LearnNote, Point, PointAct, Pointed, Pointer, Took};
use super::nav::{Location, NavKey, OrbitKey, Step};
use super::page::PageLayout;
use super::settings::midi_map::{FRAME_ROWS, MapRow, MapText};
use super::settings::view::RowLook;
use super::settings::{Screen, screen_path};
use crate::midi_map::builtin::BUILT_IN;
use crate::midi_map::{Binding, CcKind, Input, LearnCc, MAP_LEN, Mappable, MidiMap};
use crate::project::{Line, PartId};

/// Pointing's words, `|` between: a named static, so its bytes are bank
/// 2's.
static WORDS: [u8; 148] = *b"NEW BINDING|NEW TARGET|ALREADY BOUND|EDIT|\
+ NEW BINDING|POINT AT A CELL|MAP FULL|DELETE A ROW FIRST|ITS CC IS SET ON ITS ROW|\
BINDING GONE|NO FREE CC";

/// A word's place in `WORDS`.
#[derive(Clone, Copy)]
#[repr(u8)]
enum W {
    NewBinding,
    NewTarget,
    Bound,
    Edit,
    NewRow,
    PointAt,
    Full,
    DeleteFirst,
    SetOnRow,
    Gone,
    NoCc,
}

/// Word `w`'s text.
#[inline(never)]
fn word(w: W) -> &'static str {
    super::midi::pick(&WORDS, w as u8)
}

/// The hints' words: byte statics, so they link into bank 2 with this
/// module; `text` checks them as UTF-8 when built (a const `from_utf8`
/// that panics at compile time, intentionally, as `part_voices`' do).
static EDIT_B: [u8; 4] = *b"EDIT";
static BINDS_B: [u8; 5] = *b"BINDS";
static AF_B: [u8; 3] = *b"A-F";
static POINT_B: [u8; 5] = *b"POINT";
static MENU_B: [u8; 4] = *b"MENU";
static CANCELS_B: [u8; 7] = *b"CANCELS";
static NOT_B: [u8; 12] = *b"NOT MAPPABLE";
static BOUND_B: [u8; 13] = *b"ALREADY BOUND";
static NONE_B: [u8; 0] = [];

const fn text(b: &'static [u8]) -> &'static str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(_) => panic!("the words are UTF-8"),
    }
}

const CANCEL: Gesture<'static> = Gesture(text(&MENU_B), text(&CANCELS_B));
static BINDS: [Gesture<'static>; 2] = [Gesture(text(&EDIT_B), text(&BINDS_B)), CANCEL];
static WALKS: [Gesture<'static>; 2] = [Gesture(text(&AF_B), text(&POINT_B)), CANCEL];
static NOT: [Gesture<'static>; 2] = [Gesture(text(&NOT_B), text(&NONE_B)), CANCEL];
static BOUND: [Gesture<'static>; 2] = [Gesture(text(&BOUND_B), text(&NONE_B)), CANCEL];

/// What pointing says in the value's place, the focused cell labelled
/// `label`. Learn's `note_words` for `LearnNote::Point`.
pub(super) fn say(p: Pointed, label: &str, out: &mut FmtBuf) {
    let w = match p {
        Pointed::Walking(AimKind::New) | Pointed::Binds(AimKind::New) => W::NewBinding,
        Pointed::Walking(AimKind::Retarget) | Pointed::Binds(AimKind::Retarget) => W::NewTarget,
        Pointed::Bound => W::Bound,
        Pointed::NotMappable => {
            return super::midi::learn::note_words(Some(LearnNote::NotMappable), label, out);
        }
    };
    let _ = out.write_str(word(w));
}

/// The hint line for `p`: `EDIT BINDS  MENU CANCELS` where EDIT binds,
/// `A-F POINT  MENU CANCELS` before a cell is pointed at, else why not
/// (MOD MATRIX's routes, a cell another row holds). The door's
/// `point_hint`; the renderer puts it over every hint a page shows, and in
/// a BigViz status line's place.
pub fn hint(p: Pointed) -> Hint {
    match p {
        Pointed::Binds(_) => &BINDS,
        Pointed::Walking(_) => &WALKS,
        Pointed::NotMappable => &NOT,
        Pointed::Bound => &BOUND,
    }
}

/// CC `n` as a new row's, if it may be: a binding may be learned on it
/// (`LearnCc`), and it is neither the mod wheel nor a control the built-in
/// layer answers (`BUILT_IN`, ADR 0263).
fn free_number(n: u8) -> Option<LearnCc> {
    if n == 1 {
        return None;
    }
    for &b in &BUILT_IN {
        if b == n {
            return None;
        }
    }
    LearnCc::new(n)
}

/// The first such CC no row of `own` or `layer` is on: a new row's
/// control, read `Absolute(Jump)`. Loops, not iterator adaptors: a core
/// instance the inliner leaves out of line is bank 1's (ADR 0175).
fn free_cc(own: &MidiMap, layer: &MidiMap) -> Option<Input> {
    let mut n = 0u8;
    while n <= 127 {
        if let Some(c) = free_number(n) {
            let i = Input::Cc(c, CcKind::Absolute(Takeover::Jump));
            if !on(own, &i) && !on(layer, &i) {
                return Some(i);
            }
        }
        n += 1;
    }
    None
}

/// Whether a row of `m` is on `i`'s control.
fn on(m: &MidiMap, i: &Input) -> bool {
    for e in m.entries() {
        if e.input.same_control(i) {
            return true;
        }
    }
    false
}

/// The row of `m` on target `t`.
fn row_on(m: &MidiMap, t: Mappable) -> Option<usize> {
    for (i, e) in m.entries().iter().enumerate() {
        if e.target == t {
            return Some(i);
        }
    }
    None
}

/// A MIDI MAP row, as the bar's place: the pick, the layer's rows, the
/// Sound's, `+ NEW BINDING`. At most `FRAME_ROWS + 2 * MAP_LEN`, so a byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MapRowAt(u8);

const _: () = assert!(FRAME_ROWS + 2 * MAP_LEN <= u8::MAX as usize);

impl MapRowAt {
    /// Sound row `own` under a layer `layer`; past the Sound's rows,
    /// `+ NEW BINDING`. Each count is a map's, at most `MAP_LEN`.
    fn of(layer: &MidiMap, own: usize) -> MapRowAt {
        MapRowAt((1 + layer.len() + own.min(MAP_LEN)) as u8)
    }
}

impl UiState {
    /// Part `p`'s controller layer, empty before the door first ran.
    fn point_layer(&self, p: PartId) -> MidiMap {
        match self.cc.built() {
            Some(m) => *m.ctl.layer(p.index()),
            None => MidiMap::EMPTY,
        }
    }

    /// The cell `pt` points at, if it is still the one focused on the page
    /// shown: after a rechained page or a moved focus, nothing is.
    fn pointed_cell(&self, pt: Point) -> Option<Cell> {
        match pt.at {
            Pointer::Walking => None,
            Pointer::At(c) => {
                let focused = ALL_ENCODERS.get(self.focused_slot());
                (c.page == self.page_def().id && focused == Some(&c.slot)).then_some(c)
            }
        }
    }

    /// What `pt` says on the page shown.
    fn pointed(&self, pt: Point) -> Pointed {
        // MOD MATRIX's routes are no cell a control may drive.
        if self.page_def().layout == PageLayout::Matrix {
            return Pointed::NotMappable;
        }
        if self.pointed_cell(pt).is_none() {
            return Pointed::Walking(pt.aim.kind());
        }
        let Some(t) = self.learn_target() else {
            return Pointed::NotMappable;
        };
        let held = row_on(&self.project.part(pt.part).sound.map, t).is_some();
        match (pt.aim, held) {
            (Aim::New | Aim::Retarget(_), false) => Pointed::Binds(pt.aim.kind()),
            // Its own target: EDIT binds it again, nothing changes.
            (Aim::Retarget(was), true) if was == t => Pointed::Binds(AimKind::Retarget),
            (Aim::New | Aim::Retarget(_), true) => Pointed::Bound,
        }
    }

    /// Pointing's words now. The door's `point_say`.
    pub(in crate::ui) fn point_say_in(&self) -> Option<Pointed> {
        match self.learn {
            Learn::Pointing(pt) => Some(self.pointed(pt)),
            Learn::Off | Learn::Armed { .. } | Learn::Sensing { .. } => None,
        }
    }

    /// EDIT on MIDI MAP's `+ NEW BINDING` (`New`) or a Sound row: out to
    /// the active Part's pages or mixer, where SETTINGS was opened from if
    /// they were its, else its engine's home, pointing.
    pub(super) fn point_start(&mut self, aim: Aim) {
        let p = self.active_part;
        let cx = self.cx();
        // SETTINGS closed from its top, as MENU would.
        let back = match Location::settings_at(&[], 0).step(NavKey::MenuTap, &cx, &mut self.recall)
        {
            Step::Go(l) if l.ladder_part() == Some(p) => l,
            Step::Go(_)
            | Step::Act(_)
            | Step::Screen(_)
            | Step::Run
            | Step::Orbit(_)
            | Step::Stay => Location::part_home(p, self.project.part(p).sound.params.engine()),
        };
        self.learn = Learn::Pointing(Point {
            part: p,
            aim,
            at: Pointer::Walking,
            loads: self.project.part(p).loads(),
        });
        self.go(back);
    }

    /// What reaches pointing: a ladder key or a frame's ORBIT keys (MENU
    /// cancels, EDIT binds where the hint says so), MENU held (it
    /// cancels), or a move (pointing goes on, nothing pointed at, through
    /// its Part's pages, mixer and FX and through ORBIT; the Sound rung,
    /// whose EDIT loads, and anywhere else end it). The door's `point`.
    #[optimize(speed)]
    pub(in crate::ui) fn point_in(&mut self, a: PointAct<'_>) -> Took {
        let Learn::Pointing(pt) = self.learn else {
            return Took::Walk;
        };
        // A load since pointing began ends it: its map is another.
        if self.project.part(pt.part).loads() != pt.loads {
            self.learn = Learn::Off;
            return Took::Walk;
        }
        let (mut edit, mut menu) = (false, false);
        match a {
            PointAct::Moved(to) => {
                self.learn = match to.orbit().is_some() || to.ladder_part() == Some(pt.part) {
                    true => Learn::Pointing(Point {
                        at: Pointer::Walking,
                        ..pt
                    }),
                    false => Learn::Off,
                };
                return Took::Walk;
            }
            PointAct::MenuHeld => menu = true,
            PointAct::Nav(k) => (edit, menu) = (k == NavKey::Edit, k == NavKey::MenuTap),
            PointAct::Orbit(keys) => {
                for k in keys {
                    match k {
                        Some(OrbitKey::Edit) => edit = true,
                        Some(OrbitKey::MenuTap) => menu = true,
                        Some(
                            OrbitKey::Ring(_)
                            | OrbitKey::Cross(_)
                            | OrbitKey::Mute(_)
                            | OrbitKey::PlayStop
                            | OrbitKey::Plus
                            | OrbitKey::Minus
                            | OrbitKey::SeqTap,
                        )
                        | None => {}
                    }
                }
            }
        }
        // MENU cancels; EDIT binds where the hint says so. One call back to
        // MIDI MAP: two are what the machine outliner shares into bank 1
        // (ADR 0175).
        let at = match (menu, edit) {
            (true, _) => None,
            (false, false) => return Took::Walk,
            (false, true) => {
                let target = match self.pointed(pt).binds() {
                    true => self.learn_target(),
                    false => None,
                };
                let Some(t) = target else {
                    return Took::Walk;
                };
                match self.point_bind(pt, t) {
                    Ok(i) => Some(i),
                    // Said, not silent: the row went, or no CC is free.
                    Err(w) => {
                        self.error(Line::new(word(w)));
                        None
                    }
                }
            }
        };
        self.point_back(pt, at);
        Took::Frame
    }

    /// `pt`'s bind on `t`: the Sound row it wrote, or why none was.
    fn point_bind(&mut self, pt: Point, t: Mappable) -> Result<usize, W> {
        let p = pt.part;
        let mut m = self.project.part(p).sound.map;
        let i = match pt.aim {
            Aim::New => {
                let input = free_cc(&m, &self.point_layer(p)).ok_or(W::NoCc)?;
                if m.learn(input, t).is_err() {
                    return Err(W::Full);
                }
                m.len() - 1
            }
            // Its own target again: nothing to change.
            Aim::Retarget(was) if was == t => return row_on(&m, was).ok_or(W::Gone),
            Aim::Retarget(was) => {
                let i = row_on(&m, was).ok_or(W::Gone)?;
                let e = m.entries()[i];
                m.set(i, Binding { target: t, ..e });
                i
            }
        };
        self.set_map(p, m);
        Ok(i)
    }

    /// Pointing over: MIDI MAP of `pt`'s Part again, the bar on Sound row
    /// `own` if given, else on the row pointing began from.
    fn point_back(&mut self, pt: Point, own: Option<usize>) {
        let p = pt.part;
        self.learn = Learn::Off;
        self.active_part = p;
        let map = self.project.part(p).sound.map;
        let own = match (own, pt.aim) {
            (Some(i), Aim::New | Aim::Retarget(_)) => Some(i),
            (None, Aim::New) => None,
            (None, Aim::Retarget(was)) => row_on(&map, was),
        };
        // Past the Sound's rows: `+ NEW BINDING`.
        let at = MapRowAt::of(&self.point_layer(p), own.unwrap_or(map.len()));
        self.go(Location::settings_at(screen_path(Screen::MidiMap), at.0));
    }

    /// `+ NEW BINDING`'s words (ADR 0288): `EDIT POINT AT A CELL`, or a
    /// full map's, dimmed.
    pub(super) fn new_row(&self, full: bool, out: &mut MapText) {
        out.row = MapRow::New;
        let _ = out.label.write_str(word(W::NewRow));
        let (head, tail) = match full {
            true => (W::Full, W::DeleteFirst),
            false => (W::PointAt, W::SetOnRow),
        };
        let _ = out.head.write_str(word(head));
        let _ = out.tail.write_str(word(tail));
        match full {
            true => out.look = RowLook::Dimmed,
            false => {
                let _ = out.control.write_str(word(W::Edit));
                let _ = out.echo.write_str(word(W::PointAt));
            }
        }
        out.key = super::region::settings_key(&[
            out.label.as_str().as_bytes(),
            &[full as u8, MAP_LEN as u8],
        ]);
    }
}
