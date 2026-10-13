//! SETTINGS › PART › MIDI MAP (spec § 8): the active Part's map as rows,
//! and the keys that edit it. Every edit is a Sound edit, so `*` and
//! Revert carry it, and it rests the Part's ramps (indexed by position).

use core::fmt::Write;

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

use super::prompt::{Choice, DeleteAnswer, Prompt};
use super::view::RowLook;
use super::{Ask, Screen, screen_path};
use crate::addr::ParamAddr;
use crate::midi_in::Cc;
use crate::midi_map::{MAP_LEN, MapEntry, MidiMap};
use crate::params::EngineType;
use crate::project::{Line, PartId};
use crate::ui::UiState;
use crate::ui::fmt::FmtBuf;
use crate::ui::nav::{Location, PartChain};
use crate::ui::view::view;

/// A row as the list draws it.
pub struct MapRow {
    /// `CC 74  FREQ`; a dead entry `CC 16  - STRUCT`.
    pub label: FmtBuf,
    /// `0-255`: places of the param's span; min above max inverts.
    pub range: FmtBuf,
    pub look: RowLook,
}

/// `e`, its target's cell label `name`, on a Part playing `engine`:
/// dimmed when that engine doesn't read it. (The font has no dashes.)
pub fn row(e: &MapEntry, name: &str, engine: EngineType) -> MapRow {
    let live = e.target.live(engine);
    let (mut label, mut range) = (FmtBuf::new(), FmtBuf::new());
    let dead = if live { "" } else { "- " };
    let _ = write!(label, "CC {:<3} {dead}{name}", e.cc.get());
    let _ = write!(range, "{}-{}", e.min, e.max);
    MapRow {
        label,
        range,
        look: if live {
            RowLook::Normal
        } else {
            RowLook::Dimmed
        },
    }
}

/// The active Part's map with its rows' cell labels, for the bands.
#[derive(Clone, Copy)]
pub struct MapBand {
    pub map: MidiMap,
    engine: EngineType,
    labels: [&'static str; MAP_LEN],
}

impl MapBand {
    pub fn row(&self, i: usize) -> MapRow {
        row(&self.map.entries()[i], self.labels[i], self.engine)
    }
}

/// B's detents: along 0..=127 past the reserved CCs, stopping at the ends.
fn step_cc(cc: Cc, d: i8) -> Cc {
    let s = i16::from(d.signum());
    let mut n = i16::from(cc.get());
    for _ in 0..d.unsigned_abs() {
        let mut m = n + s;
        while Cc::new(m as u8).is_some_and(Cc::is_reserved) && (0..=127).contains(&m) {
            m += s;
        }
        if !(0..=127).contains(&m) {
            break;
        }
        n = m;
    }
    Cc::new(n as u8).unwrap_or(cc)
}

/// DELETE's panel: `DELETE CC 74?` over the target's cell label, as the
/// entry was when asked.
#[derive(Clone, Copy, Debug)]
pub struct MapDelete {
    pub part: PartId,
    pub entry: MapEntry,
    pub label: &'static str,
}

impl Prompt for MapDelete {
    type Answer = DeleteAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = write!(q, "DELETE CC {}?", self.entry.cc.get());
        let _ = r.write_str(self.label);
    }
}

impl UiState {
    /// A key on the page: B sets the CC, C MIN, D MAX; MIX+MINUS deletes.
    pub(in crate::ui) fn map_input(&mut self, c: &impl Controls) {
        let p = self.active_part;
        let (map, Some(at)) = (self.project.part(p).sound.map, self.loc.settings()) else {
            return;
        };
        let i = at.row() as usize;
        let Some(&e) = map.entries().get(i) else {
            return;
        };
        if c.edges(ButtonId::Mix).down && c.button_state(ButtonId::Minus) == ButtonState::Pressed {
            let label = self.cell_label(e.target.addr());
            let ask = MapDelete {
                part: p,
                entry: e,
                label,
            };
            return self.ask(Ask::MapDelete(ask, Choice::new()));
        }
        let by = |v: u8, id| (i16::from(v) + i16::from(c.encoder_delta(id))).clamp(0, 255) as u8;
        let n = MapEntry {
            cc: step_cc(e.cc, c.encoder_delta(EncoderId::B)),
            min: by(e.min, EncoderId::C),
            max: by(e.max, EncoderId::D),
            ..e
        };
        if n != e {
            let mut m = map;
            m.set(i, n);
            self.set_map(p, m);
        }
    }

    /// The cell label of `a` on the active Part's pages: what learn shows
    /// in the band, full-word and TYPE-aware. A param on no page (LEVEL,
    /// PAN) has the spec's.
    pub fn cell_label(&self, a: ParamAddr) -> &'static str {
        let ctx = self.ctx();
        // The Part's own pages first; a dead entry's are another engine's.
        let own = PartChain::of(&self.project.part(self.active_part).sound);
        let chains = [
            own,
            PartChain::Algo,
            PartChain::Modal,
            PartChain::Tape,
            PartChain::Va,
        ];
        let pages = chains
            .into_iter()
            .flat_map(|c| c.def().blocks)
            .flat_map(|b| core::iter::once(b.def).chain(b.sub_pages.iter().copied()));
        pages
            .flat_map(|def| (0..def.params.len()).map(move |i| view(def, i, &ctx)))
            .find(|v| v.addr() == Some(a))
            .map_or_else(|| a.spec().map_or("??", |s| s.label), |v| v.label())
    }

    /// The active Part's map as the page draws it.
    pub fn map_band(&self) -> MapBand {
        let s = &self.project.part(self.active_part).sound;
        let mut labels = [""; MAP_LEN];
        for (l, e) in labels.iter_mut().zip(s.map.entries()) {
            *l = self.cell_label(e.target.addr());
        }
        MapBand {
            map: s.map,
            engine: s.params.engine(),
            labels,
        }
    }

    /// `m` as Part `p`'s map: a Sound edit, its ramps at rest.
    fn set_map(&mut self, p: PartId, m: MidiMap) {
        self.project.edit_part(p).sound.map = m;
        self.cc.reset_part(p);
    }

    /// DELETE answered: drop the entry (found by target, as the map may
    /// not have moved but the row did), and keep the bar on a row.
    pub(in crate::ui) fn map_deleted(&mut self, p: PartId, e: MapEntry) {
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
