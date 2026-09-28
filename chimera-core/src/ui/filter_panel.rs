//! Each filter KIND's panel as `const` data (spec § 6): FLT's knobs 2–6
//! and FLT › MODE's extras. A kind's row lands with its model.

use crate::block::ParamId;
use crate::dsp::filter::FilterKind;
use crate::modulation::ModSource;
use crate::params::FilterParams;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTarget {
    /// A Filter parameter.
    Filter(ParamId),
    /// The amount of the matrix route `source → CUTOFF`.
    Route(ModSource),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelKnob {
    pub target: PanelTarget,
    /// The original panel's word (spec § 6).
    pub label: &'static str,
}

pub struct KindPanel {
    /// Knobs 2–6 of FLT.
    pub main: [PanelKnob; 5],
    /// Slots 2–3 of FLT › MODE.
    pub extras: [Option<PanelKnob>; 2],
}

const fn filter(id: ParamId, label: &'static str) -> PanelKnob {
    PanelKnob {
        target: PanelTarget::Filter(id),
        label,
    }
}

const fn route(s: ModSource, label: &'static str) -> PanelKnob {
    PanelKnob {
        target: PanelTarget::Route(s),
        label,
    }
}

/// SVF: CUTOFF · RES · MODE · ENV · KEY; extras DRIVE and LFO.
pub static SVF_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::CUTOFF, "CUTOFF"),
        filter(FilterParams::RESONANCE, "RES"),
        filter(FilterParams::MODE, "MODE"),
        route(ModSource::Env1, "ENV"),
        route(ModSource::Note, "KEY"),
    ],
    extras: [
        Some(filter(FilterParams::DRIVE, "DRIVE")),
        Some(route(ModSource::Lfo1, "LFO")),
    ],
};

pub fn panel(kind: FilterKind) -> &'static KindPanel {
    match kind {
        FilterKind::Svf => &SVF_PANEL,
    }
}

/// Knob `k`: 0–4 the main row, 5–6 the extras.
pub fn knob(kind: FilterKind, k: u8) -> Option<&'static PanelKnob> {
    let p = panel(kind);
    match k {
        0..=4 => Some(&p.main[k as usize]),
        5 | 6 => p.extras[k as usize - 5].as_ref(),
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
            .chain(p.extras.iter().flatten())
            .any(|k| k.target == PanelTarget::Filter(id))
}
