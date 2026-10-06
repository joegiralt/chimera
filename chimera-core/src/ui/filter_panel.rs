//! Each filter type's panel as `const` data (filter-types spec § 2–4): FLT's
//! knobs 2–6 and FLT › MOD's six cells. Route knobs are views of matrix
//! cells, and the jacks pick which row FM3's and QM's cells are on.

use crate::block::{ParamId, ValFmt};
use crate::dsp::filter::FilterKind;
use crate::modulation::{FixedCol, ModSource};
use crate::params::{EngineType, FilterParams};
use crate::preset::Sound;
use crate::taps::Jack;

/// A jack and the knob that is its cell's amount (spec § 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JackSlot {
    /// FM3 IN → FREQ.
    Fm3,
    /// QM IN → Q.
    Qm,
}

impl JackSlot {
    /// The fixed column the jack's cell is in.
    pub const fn col(self) -> FixedCol {
        match self {
            JackSlot::Fm3 => FixedCol::Freq,
            JackSlot::Qm => FixedCol::Q,
        }
    }

    /// The jack's Filter param.
    pub const fn param(self) -> ParamId {
        match self {
            JackSlot::Fm3 => FilterParams::FM3_IN,
            JackSlot::Qm => FilterParams::QM_IN,
        }
    }

    /// LFO 1 and ENV 3 (owner answer 6); also what a tap acts as off ALGO.
    pub const fn default(self) -> Jack {
        match self {
            JackSlot::Fm3 => Jack::Mod(ModSource::Lfo1),
            JackSlot::Qm => Jack::Mod(ModSource::Env3),
        }
    }

    pub const fn choices(self) -> &'static [Jack] {
        match self {
            JackSlot::Fm3 => &Jack::FM3_CHOICES,
            JackSlot::Qm => &Jack::QM_CHOICES,
        }
    }

    /// The jack whose param `id` is.
    pub fn of(id: ParamId) -> Option<JackSlot> {
        [JackSlot::Fm3, JackSlot::Qm]
            .into_iter()
            .find(|j| j.param() == id)
    }

    fn stored(self, p: &FilterParams) -> Jack {
        match self {
            JackSlot::Fm3 => p.fm3_src(),
            JackSlot::Qm => p.qm_src(),
        }
    }

    fn set(self, p: &mut FilterParams, j: Jack) -> bool {
        match self {
            JackSlot::Fm3 => p.set_fm3_src(j),
            JackSlot::Qm => p.set_qm_src(j),
        }
    }

    /// What the jack acts as on `s` (a tap off ALGO is the default).
    pub fn effective(self, s: &Sound) -> Jack {
        self.stored(&s.params.filter)
            .effective(s.engine(), self.default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTarget {
    /// A Filter parameter.
    Filter(ParamId),
    /// The amount of the matrix cell (source, FREQ).
    Route(ModSource),
    /// NOTE → FREQ, stepped OFF, HALF, FULL (spec § 3).
    Keybd,
    /// The amount of the jack's cell.
    JackAmount(JackSlot),
    /// A Filter parameter shown fixed and dimmed (the 2040's MODE).
    Fixed(ParamId),
}

impl PanelTarget {
    /// The cell a route knob views; `None` for a param.
    pub const fn route(self) -> Option<RouteView> {
        match self {
            PanelTarget::Route(s) => Some(RouteView::Freq(s)),
            PanelTarget::Keybd => Some(RouteView::Keybd),
            PanelTarget::JackAmount(j) => Some(RouteView::Jack(j)),
            PanelTarget::Filter(_) | PanelTarget::Fixed(_) => None,
        }
    }
}

/// A route knob: which matrix cell it views (one stored value, two places
/// to edit it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteView {
    /// (source, FREQ): FM1, FM2, ENV AMT.
    Freq(ModSource),
    /// (NOTE, FREQ), stepped.
    Keybd,
    /// (the jack's source, its column): FM3, QM.
    Jack(JackSlot),
}

impl RouteView {
    /// The cell on `s`; `None` while the jack holds a tap, whose cell is
    /// not a matrix row (Task 13).
    pub fn cell(self, s: &Sound) -> Option<(ModSource, FixedCol)> {
        match self {
            RouteView::Freq(src) => Some((src, FixedCol::Freq)),
            RouteView::Keybd => Some((ModSource::Note, FixedCol::Freq)),
            RouteView::Jack(j) => match j.effective(s) {
                Jack::Mod(src) => Some((src, j.col())),
                Jack::Tap(_) => None,
            },
        }
    }

    pub const fn fmt(self) -> ValFmt {
        match self {
            RouteView::Keybd => ValFmt::Keybd,
            RouteView::Freq(_) | RouteView::Jack(_) => ValFmt::Route,
        }
    }
}

/// The amount in the cell `v` views, 0 when absent.
pub fn route_amount(s: &Sound, v: RouteView) -> i8 {
    v.cell(s)
        .map_or(0, |(src, c)| s.mod_state.fixed_amount(src.index(), c))
}

/// FM3's or QM's amount: the cell (effective jack, `slot.col()`).
pub fn jack_amount(s: &Sound, slot: JackSlot) -> i8 {
    route_amount(s, RouteView::Jack(slot))
}

/// Writes FM3's or QM's cell, creating its route; no second copy.
pub fn set_jack_amount(s: &mut Sound, slot: JackSlot, a: i8) {
    if let Some((src, c)) = RouteView::Jack(slot).cell(s) {
        s.mod_state.set_fixed(src.index(), c, a);
    }
}

/// KEYBD's steps: OFF, HALF, FULL.
const KEYBD_STEPS: [i8; 3] = [0, 64, 127];

/// KEYBD's next step from `amount` in `dir`'s direction; at the end it
/// stays (a negative amount steps up to OFF).
pub fn keybd_step(amount: i8, dir: i8) -> i8 {
    if dir > 0 {
        KEYBD_STEPS
            .into_iter()
            .find(|&s| s > amount)
            .unwrap_or(amount)
    } else if dir < 0 {
        KEYBD_STEPS
            .into_iter()
            .rev()
            .find(|&s| s < amount)
            .unwrap_or(amount)
    } else {
        amount
    }
}

/// Moves `slot`'s cable to `new`: the cell's amount and presence go to the
/// new source's cell in one write, and the jack param follows. Refuses (and
/// changes nothing) when `new` isn't a choice or its cell is occupied.
pub fn repatch(s: &mut Sound, slot: JackSlot, new: Jack) -> bool {
    let old = slot.effective(s);
    if !slot.choices().contains(&new) {
        return false;
    }
    if old != new {
        match (old, new) {
            (Jack::Mod(o), Jack::Mod(n)) => {
                let (c, m) = (slot.col(), &mut s.mod_state);
                if m.fixed_present(c) & 1 << n.index() != 0 {
                    return false;
                }
                if m.fixed_present(c) & 1 << o.index() != 0 {
                    let a = m.fixed_amount(o.index(), c);
                    m.clear_fixed(o.index(), c);
                    m.set_fixed(n.index(), c, a);
                }
            }
            // The tap arms come with `TapRoutes` (Task 13).
            _ => return false,
        }
    }
    slot.set(&mut s.params.filter, new)
}

/// `delta` detents of a jack knob: each moves the cable to the next choice
/// `repatch` takes, skipping occupied cells, and stops at the list's end.
/// Taps are offered on ALGO only. Returns whether the cable moved.
pub fn step_jack(s: &mut Sound, slot: JackSlot, delta: i8) -> bool {
    let choices = slot.choices();
    let algo = s.engine() == EngineType::Algo;
    let mut at = choices
        .iter()
        .position(|&j| j == slot.effective(s))
        .unwrap_or(0);
    let mut moved = false;
    for _ in 0..delta.unsigned_abs() {
        let next = if delta > 0 {
            choices[at + 1..]
                .iter()
                .position(|&j| (algo || matches!(j, Jack::Mod(_))) && repatch(s, slot, j))
                .map(|i| at + 1 + i)
        } else {
            choices[..at]
                .iter()
                .rposition(|&j| (algo || matches!(j, Jack::Mod(_))) && repatch(s, slot, j))
        };
        match next {
            Some(i) => (at, moved) = (i, true),
            None => break,
        }
    }
    moved
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelKnob {
    pub target: PanelTarget,
    /// The original panel's word (spec § 2, § 3).
    pub label: &'static str,
}

pub struct KindPanel {
    /// Knobs 2–6 of FLT.
    pub main: [PanelKnob; 5],
    /// FLT › MOD's six cells.
    pub mods: [Option<PanelKnob>; 6],
}

const fn knob_of(target: PanelTarget, label: &'static str) -> PanelKnob {
    PanelKnob { target, label }
}

const fn filter(id: ParamId, label: &'static str) -> PanelKnob {
    knob_of(PanelTarget::Filter(id), label)
}

const fn route(s: ModSource, label: &'static str) -> PanelKnob {
    knob_of(PanelTarget::Route(s), label)
}

/// FM3 · FM3 IN · QM · QM IN, every type's FLT › MOD cells 2–5.
const fn mods(first: PanelKnob, last: Option<PanelKnob>) -> [Option<PanelKnob>; 6] {
    [
        Some(first),
        Some(knob_of(PanelTarget::JackAmount(JackSlot::Fm3), "FM3")),
        Some(filter(FilterParams::FM3_IN, "FM3 IN")),
        Some(knob_of(PanelTarget::JackAmount(JackSlot::Qm), "QM")),
        Some(filter(FilterParams::QM_IN, "QM IN")),
        last,
    ]
}

/// CLEAN speaks Cascadia: FREQ · Q · MODE · FM1 · FM2; LEVEL leads FLT › MOD.
pub static CLEAN_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::CUTOFF, "FREQ"),
        filter(FilterParams::RESONANCE, "Q"),
        filter(FilterParams::MODE, "MODE"),
        route(ModSource::Env1, "FM1"),
        route(ModSource::Note, "FM2"),
    ],
    mods: mods(filter(FilterParams::LEVEL, "LEVEL"), None),
};

/// The 2040 speaks the Prophet's words: CUTOFF · RES · ENV AMT · KEYBD ·
/// DRIVE (LEVEL under its panel word, spec § 3). VOICE leads FLT › MOD,
/// and its one MODE shows last, fixed.
pub static K2040_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::CUTOFF, "CUTOFF"),
        filter(FilterParams::RESONANCE, "RES"),
        route(ModSource::Env1, "ENV AMT"),
        knob_of(PanelTarget::Keybd, "KEYBD"),
        filter(FilterParams::LEVEL, "DRIVE"),
    ],
    mods: mods(
        filter(FilterParams::VOICE, "VOICE"),
        Some(knob_of(PanelTarget::Fixed(FilterParams::MODE), "MODE")),
    ),
};

pub fn panel(kind: FilterKind) -> &'static KindPanel {
    match kind {
        // DIRTY speaks CLEAN's panel and words (spec § 4).
        FilterKind::Svf | FilterKind::Dirty => &CLEAN_PANEL,
        FilterKind::K2040 => &K2040_PANEL,
    }
}

/// Knob `k`: 0–4 FLT's main row, 5–10 FLT › MOD's cells.
pub fn knob(kind: FilterKind, k: u8) -> Option<&'static PanelKnob> {
    let p = panel(kind);
    match k {
        0..=4 => Some(&p.main[k as usize]),
        5..=10 => p.mods[k as usize - 5].as_ref(),
        _ => None,
    }
}

/// Whether the kind shows filter parameter `id` (KIND and MODE always):
/// "not shown, not applied" covers only the filter's own parameters.
pub fn applies(kind: FilterKind, id: ParamId) -> bool {
    let p = panel(kind);
    id == FilterParams::KIND
        || id == FilterParams::MODE
        || p.main
            .iter()
            .chain(p.mods.iter().flatten())
            .any(|k| k.target == PanelTarget::Filter(id))
}
