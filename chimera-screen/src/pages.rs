//! `ed describe`'s page numbering: the Part's pages of the editor's chain
//! (`console::ed_chain`), then each pad's on DRUM. The console's walk
//! (`console::ed::pages`) is private to it; this is its twin, and
//! `tests/pages.rs` holds it to every page `ed describe` names.

use chimera_core::console::{ed_chain, ed_pages};
use chimera_core::dsp::drum::types::PadId;
use chimera_core::params::EngineType;
use chimera_core::ui::block_def::{BlockDef, SlotBinding};

/// A page number as `ed describe` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageNo(pub u16);

/// A cell bound `b` resolves against the pad (`pages::pad_binding`).
fn pad_binding(b: SlotBinding) -> bool {
    match b {
        SlotBinding::Param(a) => a.block.is_pad(),
        SlotBinding::SelectPad
        | SlotBinding::Knob(_)
        | SlotBinding::PadFilterPanel(_)
        | SlotBinding::FilterPanel(_)
        | SlotBinding::SelectRoute
        | SlotBinding::SelectedRoute(_) => true,
        SlotBinding::Empty
        | SlotBinding::SelectedOp(_)
        | SlotBinding::SelectOp
        | SlotBinding::Legacy(_)
        | SlotBinding::FxPanel(_)
        | SlotBinding::EnvPanel(..)
        | SlotBinding::LfoPanel(..)
        | SlotBinding::ModalPanel(..)
        | SlotBinding::OpPanel(..)
        | SlotBinding::Machine(_)
        | SlotBinding::Source
        | SlotBinding::ReverbPanel(..)
        | SlotBinding::SelectedRing(_)
        | SlotBinding::Orbit(_) => false,
    }
}

/// `def` is a pad's page on `engine`.
fn per_pad(engine: EngineType, def: &BlockDef) -> bool {
    engine == EngineType::Drum && def.params.iter().any(|s| pad_binding(s.binding))
}

/// Every page of the chain, each node's home then its sub-pages.
fn defs(engine: EngineType) -> impl Iterator<Item = &'static BlockDef> {
    ed_chain(engine)
        .blocks
        .iter()
        .flat_map(|b| core::iter::once(b.def).chain(b.sub_pages.iter().copied()))
}

/// Page `n` of `engine`: its def, and the pad it is a page of.
pub fn page_at(engine: EngineType, n: PageNo) -> Option<(&'static BlockDef, Option<PadId>)> {
    let n = usize::from(n.0);
    if n >= usize::from(ed_pages(engine)) {
        return None;
    }
    let (pad, part): (Vec<_>, Vec<_>) = defs(engine).partition(|d| per_pad(engine, d));
    if let Some(&d) = part.get(n) {
        return Some((d, None));
    }
    let k = n - part.len();
    let p = *PadId::ALL.get(k / pad.len())?;
    Some((pad[k % pad.len()], Some(p)))
}
