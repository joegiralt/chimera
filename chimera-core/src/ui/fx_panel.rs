// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each FX TYPE's pots as `const` data (ADR 0155, `filter_panel`'s
//! pattern): the words FLT › FX's P1–P3 cells show under that type.
//! TYPE, INPUT and MIX sit on the page outside the panel.

use crate::block::{ParamId, ValFmt};
use crate::dsp::voice_fx::params::{FxType, VoiceFxParams as X};

/// A pot under a type: its word, and the short a matrix column and a cell
/// too narrow for the word show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxCell {
    pub id: ParamId,
    pub label: &'static str,
    pub short: &'static str,
    pub fmt: ValFmt,
}

const fn cell(id: ParamId, label: &'static str, short: &'static str, fmt: ValFmt) -> FxCell {
    FxCell {
        id,
        label,
        short,
        fmt,
    }
}

static SOFT: [FxCell; 3] = [
    cell(X::P1, "DRIVE", "DRV", ValFmt::Uni),
    // TONE moves the clip's pre and post gains: noon is flat.
    cell(X::P2, "TONE", "TONE", ValFmt::Bi),
    cell(X::P3, "LEVEL", "LVL", ValFmt::Uni),
];

static FUZZ: [FxCell; 3] = [
    cell(X::P1, "SUSTAIN", "SUS", ValFmt::Uni),
    // The Muff's tone stack: LP to HP, the mid scoop at noon.
    cell(X::P2, "TONE", "TONE", ValFmt::Bi),
    cell(X::P3, "VOLUME", "VOL", ValFmt::Uni),
];

static FOLD: [FxCell; 3] = [
    cell(X::P1, "FOLD", "FOLD", ValFmt::Uni),
    cell(X::P2, "SYMMETRY", "SYM", ValFmt::Bi),
    cell(X::P3, "LEVEL", "LVL", ValFmt::Uni),
];

/// `t`'s three pots; OFF has none (its cells read dashes).
pub fn cells(t: FxType) -> Option<&'static [FxCell; 3]> {
    match t {
        FxType::Off => None,
        FxType::Soft => Some(&SOFT),
        FxType::Fuzz => Some(&FUZZ),
        FxType::Fold => Some(&FOLD),
    }
}

/// The pot `id` is under `t`.
pub fn cell_of(t: FxType, id: ParamId) -> Option<&'static FxCell> {
    cells(t)?.iter().find(|c| c.id == id)
}
