//! SETTINGS › PART › MIDI MAP (spec § 8.2): the active Part's map as rows,
//! and the keys that edit it. Every edit is a Sound edit, so `*` and
//! Revert carry it, and it rests the Part's ramps (indexed by position).
//! The words and the keys are bank 2's (`ui::midi::page`, behind
//! `MidiUiDoor`); here are the bands' reads and the stub.

use core::fmt::Write;

use chimera_hal::{ALL_ENCODERS, ButtonId, ButtonState, Controls};

use super::prompt::{DeleteAnswer, Prompt};
use super::view::RowLook;
use super::{Screen, screen_path};
use crate::dsp::voice_fx::params::FxType;
use crate::midi_map::{Binding, MidiMap};
use crate::params::EngineType;
use crate::project::{Line, PartId};
use crate::ui::UiState;
use crate::ui::fmt::FmtBuf;
use crate::ui::hint::{Gesture, Hint};
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

/// A binding's words as MIDI MAP shows them (spec § 8.2), written whole
/// by bank 2 (`ui::midi::page::text`, behind the door); closed, empty.
#[derive(Default)]
pub struct MapText {
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
    /// An NRPN or pressure: B can't step it.
    pub fixed: bool,
    /// All of the above as one number, for the list's region key.
    pub key: u32,
}

impl MapText {
    /// MIN, MAX and ECHO beside their keys: `C MIN 0  D MAX 255  F ECHO OFF`.
    pub fn values(&self) -> [Gesture<'_>; 3] {
        [
            Gesture("C MIN", self.min.as_str()),
            Gesture("D MAX", self.max.as_str()),
            Gesture("F ECHO", self.echo.as_str()),
        ]
    }

    /// B's and E's hint, under the values.
    pub fn hint(&self) -> Hint {
        if self.fixed { FIXED } else { KEYS }
    }
}

/// `e`'s words, its target `live` on the Part's engine.
fn text(e: &Binding, fx: FxType, live: bool) -> MapText {
    let mut t = MapText::default();
    crate::ui::cc::map_text(e, fx, live, &mut t);
    t
}

/// B and E, under the values (spec § 8.2's hint line, ADR 0086).
pub const KEYS: Hint = &[Gesture("B", "CONTROL"), Gesture("E", "INPUT")];
/// An NRPN or pressure control is fixed: B can't step it.
pub const FIXED: Hint = &[Gesture("B", "RELEARN TO CHANGE"), Gesture("E", "INPUT")];

/// The active Part's map for the bands.
#[derive(Clone, Copy)]
pub struct MapBand {
    pub map: MidiMap,
    engine: EngineType,
    fx: FxType,
    /// Bank 2 answers: closed, the page reads `NEEDS UPDATE`.
    pub open: bool,
}

impl MapBand {
    /// Row `i`'s words.
    pub fn text(&self, i: usize) -> MapText {
        let e = &self.map.entries()[i];
        text(e, self.fx, e.target.live(self.engine))
    }

    /// Row `i`'s look: dimmed when the Part's engine doesn't read it.
    pub fn look(&self, i: usize) -> RowLook {
        if self.map.entries()[i].target.live(self.engine) {
            RowLook::Normal
        } else {
            RowLook::Dimmed
        }
    }
}

/// DELETE's panel: `DELETE CC 74?` over the target's cell label, as the
/// entry was when asked.
#[derive(Clone, Copy, Debug)]
pub struct MapDelete {
    pub part: PartId,
    pub entry: Binding,
    pub label: &'static str,
}

impl Prompt for MapDelete {
    type Answer = DeleteAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let t = text(&self.entry, FxType::default(), true);
        let _ = q.write_str("DELETE ");
        let _ = q.write_str(t.control.as_str());
        let _ = q.write_char('?');
        let _ = r.write_str(self.label);
    }
}

impl UiState {
    /// A key on the page: A moves the bar (`NavKey::Bar`), B the control,
    /// C MIN, D MAX, E the input, F the echo; MIX+MINUS deletes. Bank 2's
    /// (`map_in`); closed, nothing.
    #[inline(never)]
    pub(in crate::ui) fn map_input(&mut self, c: &impl Controls) {
        let d = ALL_ENCODERS.map(|e| c.encoder_delta(e));
        let delete =
            c.edges(ButtonId::Mix).down && c.button_state(ButtonId::Minus) == ButtonState::Pressed;
        self.map_keys(d, delete);
    }

    /// The active Part's map as the page draws it.
    pub fn map_band(&self) -> MapBand {
        let s = &self.project.part(self.active_part).sound;
        MapBand {
            map: s.map,
            engine: s.params.engine(),
            fx: s.params.fx.ty(),
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
    pub(in crate::ui) fn map_deleted(&mut self, p: PartId, e: Binding) {
        let mut m = self.project.part(p).sound.map;
        let Some(i) = m.entries().iter().position(|x| x.target == e.target) else {
            return;
        };
        m.remove(i);
        let at = self.loc.settings().map_or(0, |s| s.row() as usize);
        self.set_map(p, m);
        let row = at.min(m.len().saturating_sub(1)) as u8;
        self.go(Location::settings_at(screen_path(Screen::MidiMap), row));
    }
}
