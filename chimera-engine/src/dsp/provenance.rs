// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a technique comes from (ADR 0114): each DSP module declares a
//! `PROVENANCE` and a `PATENT` list so the sourcing is auditable.

/// The source of an algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// A published method, re-derived from the paper.
    Textbook { cite: &'static str },
    /// A model of a named circuit.
    CircuitModel {
        circuit: &'static str,
        refs: &'static str,
        check: &'static str,
    },
    /// Written here.
    Original { author: &'static str },
}

/// Whether a freedom-to-operate check was done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Patent {
    NotChecked,
    Checked,
}

/// A technique and its patent status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatentLine {
    pub technique: &'static str,
    pub patent: Patent,
}
