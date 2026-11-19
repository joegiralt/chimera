//! SETTINGS › PART › MIDI MAP (spec § 8.2, ADR 0226): the active Part's
//! controller map on row 0, its layer's rows, then its Sound's own rows,
//! and the keys that edit them. A Sound row's edit is a Sound edit, so `*`
//! and Revert carry it, and it rests the Part's ramps (indexed by
//! position). The rows, their words and the keys are bank 2's
//! (`ui::midi::page`, behind `MidiUiDoor`); here are the bands' reads and
//! the stubs.

use core::fmt::Write;

use chimera_hal::{ALL_ENCODERS, ButtonId, ButtonState, Controls};

use super::prompt::{DeleteAnswer, Prompt};
use super::view::RowLook;
use super::{Screen, screen_path};
use crate::dsp::voice_fx::params::FxType;
use crate::midi_map::{Binding, MidiMap};
use crate::name::SoundName;
use crate::project::{Line, PartId};
use crate::ui::UiState;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{Gesture, Hint};
use crate::ui::midi::layer::CtlJob;
use crate::ui::mod_grid;
use crate::ui::nav::Location;

/// The target's spec label, or the FX type's word for its pot.
pub fn name(e: &Binding, fx: FxType) -> &'static str {
    let a = e.target.addr();
    mod_grid::fx_cell(a, fx)
        .map(|c| c.label)
        .or_else(|| a.spec().map(|s| s.label))
        .unwrap_or("??")
}

/// Which of the page's rows (ADR 0226).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapRow {
    /// Row 0: the Part's controller map, B picks it, EDIT saves.
    Pick,
    /// The controller layer's: edited, or dropped, in the map on the card.
    Layer,
    /// The Sound's own.
    #[default]
    Own,
    /// The last: `+ NEW BINDING`, EDIT points at its target (ADR 0288).
    New,
}

/// A row's words as MIDI MAP shows them (spec § 8.2), written whole by
/// bank 2 (`ui::midi::page`, behind the door); closed, empty.
pub struct MapText {
    pub row: MapRow,
    /// Dimmed: the Part's engine doesn't read it, or (a layer row) a Sound
    /// row holds its control.
    pub look: RowLook,
    /// `CC 74  CUTOFF`; a dead entry `CC 16  - STRUCT`.
    pub label: FmtBuf,
    /// The input's short form, right-aligned on the row: `JUMP`.
    pub word: FmtBuf,
    /// The input spelled out: `RELATIVE · TWOS COMPLEMENT`.
    pub head: FmtBuf,
    /// What follows it, if anything: `+1 = 1  -1 = 127`.
    pub tail: FmtBuf,
    /// The span's places, `0`-`255`.
    pub min: FmtBuf,
    pub max: FmtBuf,
    /// `OFF` / `ON`.
    pub echo: FmtBuf,
    /// `CC 74`, `CC 2+34`, `NRPN 10:3`, `PRESSURE`: DELETE's question.
    pub control: FmtBuf,
    /// An NRPN or pressure: B can't step it. Row 0: a map is picked.
    pub fixed: bool,
    /// All of the above as one number, for the list's region key.
    pub key: u32,
}

impl Default for MapText {
    fn default() -> Self {
        Self {
            row: MapRow::Own,
            look: RowLook::Normal,
            label: FmtBuf::default(),
            word: FmtBuf::default(),
            head: FmtBuf::default(),
            tail: FmtBuf::default(),
            min: FmtBuf::default(),
            max: FmtBuf::default(),
            echo: FmtBuf::default(),
            control: FmtBuf::default(),
            fixed: false,
            key: 0,
        }
    }
}

impl MapText {
    /// MIN, MAX and ECHO beside their keys: `C MIN 0  D MAX 255  F ECHO OFF`.
    /// Row 0: KEEP MAP's, `MIX + PLUS KEEP MAP OFF`, its words bank 2's
    /// (ADR 0253).
    pub fn values(&self) -> [Gesture<'_>; 3] {
        // `+ NEW BINDING`: `EDIT POINT AT A CELL`, its words bank 2's.
        if matches!(self.row, MapRow::Pick | MapRow::New) {
            let none = Gesture("", "");
            return [
                Gesture(self.control.as_str(), self.echo.as_str()),
                none,
                none,
            ];
        }
        [
            Gesture("C MIN", self.min.as_str()),
            Gesture("D MAX", self.max.as_str()),
            Gesture("F ECHO", self.echo.as_str()),
        ]
    }

    /// The keys' hint, under the values: a Sound row's EDIT points at its
    /// new target (ADR 0288), a layer row's does nothing.
    pub fn hint(&self) -> Hint {
        match (self.row, self.fixed) {
            (MapRow::Pick, true) => PICK_MAP,
            (MapRow::Pick, false) => PICK,
            (MapRow::Layer, true) => LAYER_FIXED,
            (MapRow::Layer, false) => LAYER,
            (MapRow::Own, true) => FIXED,
            (MapRow::Own, false) => KEYS,
            (MapRow::New, _) => crate::ui::hint::NONE,
        }
    }
}

/// `e`'s words, its target `live` on the Part's engine.
fn text(e: &Binding, fx: FxType, live: bool) -> MapText {
    let mut t = MapText::default();
    crate::ui::cc::map_text(e, fx, live, &mut t);
    t
}

/// A Sound row: B and E, under the values (spec § 8.2's hint line, ADR
/// 0086); EDIT points at a new target (ADR 0288).
pub const KEYS: Hint = &[
    Gesture("B", "CONTROL"),
    Gesture("E", "INPUT"),
    Gesture("EDIT", "TARGET"),
];
/// A Sound row on an NRPN or pressure: B can't step it.
pub const FIXED: Hint = &[
    Gesture("B", "FIXED"),
    Gesture("E", "INPUT"),
    Gesture("EDIT", "TARGET"),
];
/// A controller layer's row: B and E edit it in its map; EDIT does
/// nothing there (ADR 0288).
pub const LAYER: Hint = &[Gesture("B", "CONTROL"), Gesture("E", "INPUT")];
/// A layer's row on an NRPN or pressure.
pub const LAYER_FIXED: Hint = &[Gesture("B", "RELEARN TO CHANGE"), Gesture("E", "INPUT")];
/// Row 0: B picks the map, EDIT saves the Part's rows into one (ADR 0226).
pub const PICK: Hint = &[Gesture("B", "PICK"), Gesture("EDIT", "SAVE ROWS TO MAP")];
/// Row 0 with a map picked: MIX+MINUS deletes it; MIX+EDIT's rename is on
/// the tail line (ADR 0255).
pub const PICK_MAP: Hint = &[Gesture("EDIT", "SAVE"), Gesture("MIX+MINUS", "DELETE")];

/// The rows MIDI MAP always has: the pick and `+ NEW BINDING`.
pub const FRAME_ROWS: usize = 2;

/// A pass's keys on MIDI MAP, as `map_in` reads them: MIX down, and
/// MINUS's, PLUS's and EDIT's presses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct MapKeys(u8);

/// One of MIDI MAP's keys in a `MapKeys` set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapKey {
    /// MIX down.
    Mix,
    /// MINUS pressed.
    Minus,
    /// PLUS pressed.
    Plus,
    /// EDIT pressed.
    Edit,
}

impl MapKey {
    #[inline(always)]
    const fn bit(self) -> u8 {
        match self {
            MapKey::Mix => 1,
            MapKey::Minus => 2,
            MapKey::Plus => 4,
            MapKey::Edit => 8,
        }
    }
}

impl MapKeys {
    pub const NONE: MapKeys = MapKeys(0);

    /// These keys and `k`.
    #[inline(always)]
    pub const fn with(self, k: MapKey) -> MapKeys {
        MapKeys(self.0 | k.bit())
    }

    #[inline(always)]
    const fn has(self, k: MapKey) -> bool {
        self.0 & k.bit() != 0
    }

    /// MIX + MINUS: DELETE.
    #[inline(always)]
    pub const fn delete(self) -> bool {
        self.has(MapKey::Mix) && self.has(MapKey::Minus)
    }

    /// MIX + PLUS: KEEP MAP on row 0.
    #[inline(always)]
    pub const fn keep(self) -> bool {
        self.has(MapKey::Mix) && self.has(MapKey::Plus)
    }

    /// EDIT without MIX: pointing (ADR 0288).
    #[inline(always)]
    pub const fn point(self) -> bool {
        self.has(MapKey::Edit) && !self.has(MapKey::Mix)
    }
}

/// The active Part's MIDI MAP for the bands: its rows are bank 2's.
#[derive(Clone, Copy)]
pub struct MapBand<'a> {
    ui: &'a UiState,
    /// Row 0, the layer's rows, the Sound's and `+ NEW BINDING`.
    pub rows: usize,
    /// No layer row and no Sound row: the page says how to learn one.
    pub empty: bool,
    /// Bank 2 answers: closed, the page reads `NEEDS UPDATE`.
    pub open: bool,
}

impl MapBand<'_> {
    /// Row `i`'s words and look.
    pub fn text(&self, i: usize) -> MapText {
        let mut t = MapText::default();
        crate::ui::cc::map_row(self.ui, i, &mut t);
        t
    }

    /// Row `i`'s look.
    pub fn look(&self, i: usize) -> RowLook {
        self.text(i).look
    }
}

/// DELETE's panel: `DELETE CC 74?` over the target's cell label, as the
/// entry was when asked; or `DELETE MAP?` over the map's name.
#[derive(Clone, Copy, Debug)]
pub struct MapDelete {
    pub part: PartId,
    /// The row; `None`, the Part's whole map (`map`).
    pub entry: Option<Binding>,
    pub label: &'static str,
    /// A layer row: dropped from the Part's controller map on the card.
    pub layer: bool,
    pub map: Option<SoundName>,
}

impl Prompt for MapDelete {
    type Answer = DeleteAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let Some(e) = self.entry else {
            let _ = q.write_str("DELETE MAP?");
            let _ = r.write_str(self.map.as_ref().map_or("", |n| n.as_str()));
            return;
        };
        let t = text(&e, FxType::default(), true);
        let _ = q.write_str("DELETE ");
        let _ = q.write_str(t.control.as_str());
        let _ = q.write_char('?');
        let _ = r.write_str(self.label);
    }
}

impl UiState {
    /// A key on the page: A moves the bar (`NavKey::Bar`), B the control,
    /// C MIN, D MAX, E the input, F the echo; MIX+MINUS deletes, MIX + PLUS
    /// on row 0 is KEEP MAP; EDIT on a Sound row or `+ NEW BINDING` points
    /// (ADR 0288). Bank 2's (`map_in`), from MIX down and MINUS's, PLUS's
    /// and EDIT's presses (`MapKeys`); closed, nothing.
    #[inline(never)]
    pub(in crate::ui) fn map_input(&mut self, c: &impl Controls) {
        let d = ALL_ENCODERS.map(|e| c.encoder_delta(e));
        let mut keys = MapKeys::NONE;
        if c.edges(ButtonId::Mix).down {
            keys = keys.with(MapKey::Mix);
        }
        for (b, k) in [
            (ButtonId::Minus, MapKey::Minus),
            (ButtonId::Plus, MapKey::Plus),
            (ButtonId::Edit, MapKey::Edit),
        ] {
            if c.button_state(b) == ButtonState::Pressed {
                keys = keys.with(k);
            }
        }
        self.map_keys(d, keys);
    }

    /// The active Part's MIDI MAP as the page draws it.
    pub fn map_band(&self) -> MapBand<'_> {
        let rows = self.map_rows();
        MapBand {
            ui: self,
            rows,
            empty: rows <= FRAME_ROWS,
            open: crate::ui::cc::map_open(),
        }
    }

    /// `m` as Part `p`'s map: a Sound edit, its ramps at rest.
    pub(in crate::ui) fn set_map(&mut self, p: PartId, m: MidiMap) {
        self.project.edit_part(p).sound.map = m;
        self.cc.reset_part(p);
    }

    /// DELETE answered: drop the entry (found by target, as the map may
    /// not have moved but the row did), and keep the bar on a row.
    #[inline(never)]
    pub(in crate::ui) fn map_deleted(&mut self, d: MapDelete) {
        let p = d.part;
        let Some(e) = d.entry else {
            if let Some(n) = d.map {
                self.ctl_ask(CtlJob::Delete(p, n));
            }
            return;
        };
        if d.layer {
            return self.ctl_ask(CtlJob::Drop(p, e));
        }
        let mut m = self.project.part(p).sound.map;
        let Some(i) = m.entries().iter().position(|x| x.target == e.target) else {
            return;
        };
        m.remove(i);
        let at = self.loc.settings().map_or(0, |s| s.row() as usize);
        self.set_map(p, m);
        let row = at.min(self.map_rows().saturating_sub(1)) as u8;
        self.go(Location::settings_at(screen_path(Screen::MidiMap), row));
    }
}
