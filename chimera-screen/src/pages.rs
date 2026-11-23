// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ed describe`'s page numbering: the console's own walk
//! (`console::ed_page_at`), the Part's pages of the editor's chain, then
//! each pad's on DRUM. `tests/screen.rs` holds it to every page
//! `ed describe` names.

use chimera_core::console::{Schema, ed_page_at};
use chimera_core::dsp::drum::types::PadId;
use chimera_core::params::EngineType;
use chimera_core::ui::block_def::BlockDef;

/// A page number as `ed describe` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageNo(pub u16);

/// Page `n` of `engine`: its def, and the pad it is a page of.
pub fn page_at(engine: EngineType, n: PageNo) -> Option<(&'static BlockDef, Option<PadId>)> {
    ed_page_at(Schema::Engine(engine), usize::from(n.0)).map(|(def, round)| (def, round.pad()))
}
