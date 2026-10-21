//! Each filter type's panel as `const` data (filter-types spec § 2–4, layout
//! A of ADR 0155): FLT's MODE or VOICE, FREQ and Q, and FLT › MOD's six
//! cells. Route knobs are views of matrix cells, and the jacks pick which
//! row FM3's and QM's cells are on.

use crate::block::{ParamId, ValFmt};
use crate::dsp::filter::FilterKind;
use crate::modulation::{FixedCol, ModSource};
use crate::params::{EngineType, FilterParams};
use crate::preset::Sound;
use crate::taps::{Jack, from_tap_amount, to_tap_amount};
use crate::ui::mod_grid::RowSource;

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
}

impl PanelTarget {
    /// The cell a route knob views; `None` for a param.
    pub const fn route(self) -> Option<RouteView> {
        match self {
            PanelTarget::Route(s) => Some(RouteView::Freq(s)),
            PanelTarget::Keybd => Some(RouteView::Keybd),
            PanelTarget::JackAmount(j) => Some(RouteView::Jack(j)),
            PanelTarget::Filter(_) => None,
        }
    }
}

/// A route knob: which matrix cell it views (one stored value, two places
/// to edit it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteView {
    /// (source, FREQ): ENV AMT, KEYBOARD.
    Freq(ModSource),
    /// (NOTE, FREQ), stepped.
    Keybd,
    /// (the jack's source, its column): FM3, QM.
    Jack(JackSlot),
}

impl RouteView {
    /// The cell on `s`: a matrix source's, or on ALGO a tap's.
    pub fn cell(self, s: &Sound) -> (RowSource, FixedCol) {
        match self {
            RouteView::Freq(src) => (RowSource::Mod(src.index()), FixedCol::Freq),
            RouteView::Keybd => (RowSource::Mod(ModSource::Note.index()), FixedCol::Freq),
            RouteView::Jack(j) => (j.effective(s).into(), j.col()),
        }
    }

    pub const fn fmt(self) -> ValFmt {
        match self {
            RouteView::Keybd => ValFmt::Keybd,
            RouteView::Freq(_) | RouteView::Jack(_) => ValFmt::Route,
        }
    }
}

/// Cell (`row`, `c`)'s amount on `s`; `None` when it holds no route.
pub fn cell_route(s: &Sound, row: RowSource, c: FixedCol) -> Option<i8> {
    match row {
        RowSource::Mod(i) => {
            (s.mod_state.fixed_present(c) & 1 << i != 0).then(|| s.mod_state.fixed_amount(i, c))
        }
        RowSource::Tap(t) => s.params.taps.get(t, c),
    }
}

fn set_cell(s: &mut Sound, row: RowSource, c: FixedCol, a: i8) {
    match row {
        RowSource::Mod(i) => s.mod_state.set_fixed(i, c, a),
        RowSource::Tap(t) => s.params.taps.set(t, c, a),
    }
}

/// Deletes cell (`row`, `c`)'s route, returning its amount.
fn take_cell(s: &mut Sound, row: RowSource, c: FixedCol) -> Option<i8> {
    let a = cell_route(s, row, c)?;
    match row {
        RowSource::Mod(i) => s.mod_state.clear_fixed(i, c),
        RowSource::Tap(t) => s.params.taps.clear(t, c),
    }
    Some(a)
}

/// The amount in the cell `v` views, 0 when absent.
pub fn route_amount(s: &Sound, v: RouteView) -> i8 {
    let (row, c) = v.cell(s);
    cell_route(s, row, c).unwrap_or(0)
}

/// FM3's or QM's amount: the cell (effective jack, `slot.col()`).
pub fn jack_amount(s: &Sound, slot: JackSlot) -> i8 {
    route_amount(s, RouteView::Jack(slot))
}

/// Writes FM3's or QM's cell, creating its route; no second copy.
pub fn set_jack_amount(s: &mut Sound, slot: JackSlot, a: i8) {
    let (row, c) = RouteView::Jack(slot).cell(s);
    set_cell(s, row, c, a);
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
/// new source's cell in one write, and the jack param follows. Between a
/// route and a tap, FREQ's amount keeps its depth in octaves (spec § 2);
/// Q's is linear both sides. Refuses (and changes nothing) when `new` isn't
/// a choice, is a tap off ALGO, or its cell is occupied.
pub fn repatch(s: &mut Sound, slot: JackSlot, new: Jack) -> bool {
    let old = slot.effective(s);
    if !slot.choices().contains(&new)
        || matches!(new, Jack::Tap(_)) && s.engine() != EngineType::Algo
    {
        return false;
    }
    if old != new {
        let c = slot.col();
        if cell_route(s, new.into(), c).is_some() {
            return false;
        }
        if let Some(a) = take_cell(s, old.into(), c) {
            let a = match (old, new, c) {
                (Jack::Mod(_), Jack::Tap(_), FixedCol::Freq) => to_tap_amount(a),
                (Jack::Tap(_), Jack::Mod(_), FixedCol::Freq) => from_tap_amount(a),
                _ => a,
            };
            set_cell(s, new.into(), c, a);
        }
    }
    slot.set(&mut s.params.filter, new)
}

/// What a jack knob's turn did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JackStep {
    pub moved: bool,
    /// It passed a source whose cell another route holds: the UI says so.
    pub skipped_in_use: bool,
}

/// `delta` detents of a jack knob: each moves the cable to the next choice
/// `repatch` takes, skipping occupied cells, and stops at the list's end.
/// Taps are offered on ALGO only.
pub fn step_jack(s: &mut Sound, slot: JackSlot, delta: i8) -> JackStep {
    let choices = slot.choices();
    let algo = s.engine() == EngineType::Algo;
    let mut at = choices
        .iter()
        .position(|&j| j == slot.effective(s))
        .unwrap_or(0);
    let (mut moved, mut skipped) = (false, false);
    let mut take = |s: &mut Sound, j: Jack| {
        if !(algo || matches!(j, Jack::Mod(_))) {
            return false;
        }
        if repatch(s, slot, j) {
            return true;
        }
        skipped |= cell_route(s, j.into(), slot.col()).is_some();
        false
    };
    for _ in 0..delta.unsigned_abs() {
        let next = if delta > 0 {
            choices[at + 1..]
                .iter()
                .position(|&j| take(s, j))
                .map(|i| at + 1 + i)
        } else {
            choices[..at].iter().rposition(|&j| take(s, j))
        };
        match next {
            Some(i) => (at, moved) = (i, true),
            None => break,
        }
    }
    JackStep {
        moved,
        skipped_in_use: skipped,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelKnob {
    pub target: PanelTarget,
    /// The original panel's word (spec § 2, § 3).
    pub label: &'static str,
}

pub struct KindPanel {
    /// FLT's MODE or VOICE, FREQ and Q (cells 2, 5 and 6).
    pub main: [PanelKnob; 3],
    /// FLT › MOD's first five cells (its F is empty).
    pub mods: [PanelKnob; 5],
    /// The chip's own cell, FLT › FX's F: STABILITY on the 2040, LADDER
    /// and SEM (ADR 0168, 0192); none on CLEAN and DIRTY.
    pub chip: Option<PanelKnob>,
    /// FLT › MOD's F: SEM's OUTPUT, its BANDPASS switch (ADR 0192); empty
    /// on the others.
    pub out: Option<PanelKnob>,
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

/// Two routes, then FREQ MOD · FEEDBACK · RES MOD: every type's FLT › MOD (ADR 0190).
const fn mods(a: PanelKnob, b: PanelKnob) -> [PanelKnob; 5] {
    [
        a,
        b,
        knob_of(PanelTarget::JackAmount(JackSlot::Fm3), "FREQ MOD"),
        filter(FilterParams::FB, "FEEDBACK"),
        knob_of(PanelTarget::JackAmount(JackSlot::Qm), "RES MOD"),
    ]
}

/// FLT › JACK's jacks, every type's, each under its amount's encoder on
/// FLT › MOD (ADR 0190): knobs 10 and 11.
pub static JACKS: [PanelKnob; 2] = [
    filter(FilterParams::FM3_IN, "FREQ SRC"),
    filter(FilterParams::QM_IN, "RES SRC"),
];

/// CLEAN: MODE · FREQ · Q; ENV AMT and KEYBOARD lead FLT › MOD.
pub static CLEAN_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::MODE, "MODE"),
        filter(FilterParams::CUTOFF, "FREQ"),
        filter(FilterParams::RESONANCE, "Q"),
    ],
    mods: mods(
        route(ModSource::Env1, "ENV AMT"),
        route(ModSource::Note, "KEYBOARD"),
    ),
    chip: None,
    out: None,
};

const STABILITY: Option<PanelKnob> = Some(filter(FilterParams::STAB, "STABILITY"));

/// LADDER speaks CLEAN's words (ADR 0104), with STABILITY.
pub static LADDER_PANEL: KindPanel = KindPanel {
    chip: STABILITY,
    ..CLEAN_PANEL
};

/// The 2040 speaks the Prophet's words: VOICE · CUTOFF · RES, and ENV AMT
/// and KEYBOARD lead FLT › MOD. Its one MODE has no
/// cell: KIND's band names it (`kind_note`).
pub static K2040_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::VOICE, "VOICE"),
        filter(FilterParams::CUTOFF, "CUTOFF"),
        filter(FilterParams::RESONANCE, "RES"),
    ],
    mods: mods(
        route(ModSource::Env1, "ENV AMT"),
        knob_of(PanelTarget::Keybd, "KEYBOARD"),
    ),
    chip: STABILITY,
    out: None,
};

/// SEM speaks the SEM-1A's panel: MODE (the LP–notch–HP pot, a knob, not
/// CLEAN's list) · FREQ · RES, ENV AMT, KEYBOARD, STABILITY, and its
/// S504 switch as OUTPUT, MODE or BANDPASS, on FLT › MOD's free F (ADR
/// 0192).
pub static SEM_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::MORPH, "MODE"),
        filter(FilterParams::CUTOFF, "FREQ"),
        filter(FilterParams::RESONANCE, "RES"),
    ],
    mods: CLEAN_PANEL.mods,
    chip: STABILITY,
    out: Some(filter(FilterParams::MODE, "OUTPUT")),
};

pub fn panel(kind: FilterKind) -> &'static KindPanel {
    match kind {
        // DIRTY speaks CLEAN's panel and words (spec § 4): MODE, FREQ, Q.
        FilterKind::Svf | FilterKind::Dirty => &CLEAN_PANEL,
        FilterKind::Ladder => &LADDER_PANEL,
        FilterKind::K2040 => &K2040_PANEL,
        FilterKind::Sem => &SEM_PANEL,
    }
}

/// What KIND's band adds: the 2040's fixed mode, which has no cell.
pub const fn kind_note(kind: FilterKind) -> Option<&'static str> {
    match kind {
        FilterKind::K2040 => Some("4-POLE LP"),
        FilterKind::Svf | FilterKind::Dirty | FilterKind::Ladder | FilterKind::Sem => None,
    }
}

/// Knob `k`: 0–2 FLT's MODE or VOICE, FREQ and Q; 3–7 FLT › MOD's cells;
/// 8 FLT › MOD's F, SEM's OUTPUT; 9 the chip's cell, if the kind has one;
/// 10 and 11 FLT › JACK's jacks.
pub fn knob(kind: FilterKind, k: u8) -> Option<&'static PanelKnob> {
    let p = panel(kind);
    match k {
        0..=2 => Some(&p.main[k as usize]),
        3..=7 => Some(&p.mods[k as usize - 3]),
        8 => p.out.as_ref(),
        9 => p.chip.as_ref(),
        10 | 11 => Some(&JACKS[k as usize - 10]),
        _ => None,
    }
}

/// Whether the kind shows filter parameter `id` (KIND, MODE and LEVEL,
/// FLT › FX's INPUT, always): "not shown, not applied" covers only the
/// filter's own parameters.
pub fn applies(kind: FilterKind, id: ParamId) -> bool {
    let p = panel(kind);
    id == FilterParams::KIND
        || id == FilterParams::MODE
        || id == FilterParams::LEVEL
        || p.main
            .iter()
            .chain(p.mods.iter())
            .chain(p.chip.iter())
            .chain(p.out.iter())
            .chain(JACKS.iter())
            .any(|k| k.target == PanelTarget::Filter(id))
}
