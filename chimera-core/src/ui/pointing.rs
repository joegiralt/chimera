//! MIDI MAP's pointing (ADR 0288): EDIT on `+ NEW BINDING` or on a Sound
//! row walks the box's own pages; a turn points at a cell, as learn's
//! armed turn does (`learn_turns`), and EDIT binds it there: a new row on
//! the first CC no row uses, or the row's new target, its control, range
//! and echo kept. MENU cancels. Either way MIDI MAP opens again on the
//! row. No controller is needed. Bank 2's, behind `MidiUiDoor`, high in
//! an S4 group (ADR 0272); bank 1 keeps `Learn::Pointing` and the stubs.

use core::fmt::Write;

use super::UiState;
use super::fmt::FmtBuf;
use super::hint::Gesture;
use super::learn::{Aim, AimKind, Learn, LearnNote, Point, PointAct, Pointed, Pointer};
use super::nav::{Location, NavKey, OrbitKey, Step};
use super::settings::{Screen, screen_path};
use chimera_midi::take::Takeover;

use crate::midi_map::{CcKind, Input, LearnCc, MAP_LEN, Mappable, MidiMap};

/// Pointing's words, `|` between: a named static, so its bytes are bank
/// 2's.
static WORDS: [u8; 147] =
    *b"NEW BINDING|NEW TARGET|ALREADY BOUND|EDIT|BINDS|A-F|POINT|MENU|CANCELS|\
+ NEW BINDING|POINT AT A CELL|MAP FULL|DELETE A ROW FIRST|B AND E SET ITS CC";

/// A word's place in `WORDS`.
#[derive(Clone, Copy)]
#[repr(u8)]
enum W {
    NewBinding,
    NewTarget,
    Bound,
    Edit,
    Binds,
    Knobs,
    Point,
    Menu,
    Cancels,
    NewRow,
    PointAt,
    Full,
    DeleteFirst,
    SetCc,
}

/// Word `w`'s text.
#[inline(never)]
fn word(w: W) -> &'static str {
    super::midi::pick(&WORDS, w as u8)
}

/// What pointing says in the value's place, the focused cell labelled
/// `label`. Learn's `note_words` for `LearnNote::Point`.
pub(super) fn say(p: Pointed, label: &str, out: &mut FmtBuf) {
    let w = match p {
        Pointed::Walking(a) | Pointed::Binds(a) => match a {
            AimKind::New => W::NewBinding,
            AimKind::Retarget => W::NewTarget,
        },
        Pointed::Bound => W::Bound,
        Pointed::NotMappable => {
            return super::midi::learn::note_words(Some(LearnNote::NotMappable), label, out);
        }
    };
    let _ = out.write_str(word(w));
}

/// The hint line for `p`: `EDIT BINDS  MENU CANCELS` where EDIT binds,
/// else `A-F POINT  MENU CANCELS`. The door's `point_hint`.
pub fn hint(p: Pointed) -> [Gesture<'static>; 2] {
    let first = match p.binds() {
        true => Gesture(word(W::Edit), word(W::Binds)),
        false => Gesture(word(W::Knobs), word(W::Point)),
    };
    [first, Gesture(word(W::Menu), word(W::Cancels))]
}

/// The first CC no row of `own` or `layer` is on: a new row's control,
/// read `Absolute(Jump)`. Loops, not iterator adaptors: a core instance
/// the inliner leaves out of line is bank 1's (ADR 0175).
fn free_cc(own: &MidiMap, layer: &MidiMap) -> Option<Input> {
    let mut n = 0u8;
    while n <= 127 {
        if let Some(c) = LearnCc::new(n) {
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

impl UiState {
    /// Part `p`'s controller layer, empty before the door first ran.
    fn point_layer(&self, p: crate::project::PartId) -> MidiMap {
        match self.cc.built() {
            Some(m) => *m.ctl.layer(p.index()),
            None => MidiMap::EMPTY,
        }
    }

    /// What `pt` says on the cell focused now.
    fn pointed(&self, pt: Point) -> Pointed {
        if pt.at == Pointer::Walking {
            return Pointed::Walking(pt.aim.kind());
        }
        let Some(t) = self.learn_target() else {
            return Pointed::NotMappable;
        };
        let held = row_on(&self.project.part(pt.part).sound.map, t).is_some();
        match (pt.aim, held) {
            (_, false) => Pointed::Binds(pt.aim.kind()),
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
    /// the active Part's pages, where SETTINGS was opened from if they
    /// were its, else its engine's home, pointing.
    pub(super) fn point_start(&mut self, aim: Aim) {
        let p = self.active_part;
        let cx = self.cx();
        // SETTINGS closed from its top, as MENU would.
        let back = match Location::settings_at(&[], 0).step(NavKey::MenuTap, &cx, &mut self.recall)
        {
            Step::Go(l) if l.part() == Some(p) => l,
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
        });
        self.go(back);
    }

    /// What reaches pointing: a ladder key or a frame's ORBIT keys (MENU
    /// cancels, EDIT binds where the hint says so; `true`, the frame
    /// taken), or a move (pointing goes on, nothing pointed at, through
    /// its Part's places and ORBIT; anywhere else it ends). The door's
    /// `point`.
    pub(in crate::ui) fn point_in(&mut self, a: PointAct<'_>) -> bool {
        let Learn::Pointing(pt) = self.learn else {
            return false;
        };
        let (mut edit, mut menu) = (false, false);
        match a {
            PointAct::Moved(to) => {
                self.learn = match to.orbit().is_some() || to.part() == Some(pt.part) {
                    true => Learn::Pointing(Point {
                        at: Pointer::Walking,
                        ..pt
                    }),
                    false => Learn::Off,
                };
                return false;
            }
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
        if menu {
            self.point_back(pt, None);
            return true;
        }
        if !edit || !self.pointed(pt).binds() {
            return false;
        }
        let Some(t) = self.learn_target() else {
            return false;
        };
        let at = self.point_bind(pt, t);
        self.point_back(pt, at);
        true
    }

    /// `pt`'s bind on `t`: the row it wrote, if any (a row gone since, or
    /// no CC free, writes none).
    fn point_bind(&mut self, pt: Point, t: Mappable) -> Option<usize> {
        let p = pt.part;
        let mut m = self.project.part(p).sound.map;
        let i = match pt.aim {
            Aim::New => {
                let input = free_cc(&m, &self.point_layer(p))?;
                m.learn(input, t).ok()?;
                m.len() - 1
            }
            // Its own target again: nothing to change.
            Aim::Retarget(was) if was == t => return row_on(&m, was),
            Aim::Retarget(was) => {
                let i = row_on(&m, was)?;
                let e = m.entries()[i];
                m.set(i, crate::midi_map::Binding { target: t, ..e });
                i
            }
        };
        self.set_map(p, m);
        Some(i)
    }

    /// Pointing over: MIDI MAP of `pt`'s Part again, the bar on Sound row
    /// `own` if given, else on the row pointing began from.
    fn point_back(&mut self, pt: Point, own: Option<usize>) {
        let p = pt.part;
        self.learn = Learn::Off;
        self.active_part = p;
        let map = self.project.part(p).sound.map;
        let first = 1 + self.point_layer(p).len();
        let own = own.or(match pt.aim {
            Aim::New => None,
            Aim::Retarget(was) => row_on(&map, was),
        });
        // `+ NEW BINDING` is the last row.
        let row = first + own.unwrap_or(map.len());
        self.go(Location::settings_at(
            screen_path(Screen::MidiMap),
            row.min(u8::MAX as usize) as u8,
        ));
    }

    /// `+ NEW BINDING`'s words (ADR 0288): `EDIT POINT AT A CELL`, or a
    /// full map's, dimmed.
    pub(super) fn new_row(&self, full: bool, out: &mut super::settings::midi_map::MapText) {
        use super::settings::midi_map::MapRow;
        use super::settings::view::RowLook;
        out.row = MapRow::New;
        let _ = out.label.write_str(word(W::NewRow));
        let (head, tail) = match full {
            true => (W::Full, W::DeleteFirst),
            false => (W::PointAt, W::SetCc),
        };
        let _ = out.head.write_str(word(head));
        let _ = out.tail.write_str(word(tail));
        if full {
            out.look = RowLook::Dimmed;
        } else {
            let _ = out.control.write_str(word(W::Edit));
            let _ = out.echo.write_str(word(W::PointAt));
        }
        out.key = super::region::settings_key(&[
            out.label.as_str().as_bytes(),
            &[full as u8, MAP_LEN as u8],
        ]);
    }
}
