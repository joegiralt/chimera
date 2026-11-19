//! The pages `ed` walks for an engine: its chain's, the Part's first, then
//! on DRUM each pad's (ADR 0285). One walk, `Pages`, for `describe`,
//! `get`, `set` and `since`, so they can't disagree on what a page is.
//! Its counts are `const`: the page number is a `u8` by a compile-time
//! check, never a silent cast.

use crate::dsp::drum::types::PadId;
use crate::params::EngineType;
use crate::ui::block_def::{BlockDef, ChainDef2, SlotBinding};
use crate::ui::block_registry::{
    ALGO_FORMANT_CHAIN, DRUM_CHAIN, MODAL_PLUCK_CHAIN, TAPE_CHAIN, VA_CHAIN,
};

/// The chain the editor walks for `engine`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.chain"))]
pub const fn chain(engine: EngineType) -> &'static ChainDef2 {
    match engine {
        EngineType::Algo => &ALGO_FORMANT_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
        EngineType::Tape => &TAPE_CHAIN,
        EngineType::Va => &VA_CHAIN,
        EngineType::Drum => &DRUM_CHAIN,
    }
}

/// Whether a cell bound `b` resolves against the pad (DRUM's): a pad's
/// param, its machine's knobs, its filter's panel, its routes. FLT's
/// sub-pages' panel cells count: on DRUM they read the pad's KIND.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.pages.pad_binding")
)]
const fn pad_binding(b: SlotBinding) -> bool {
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

/// `def` is described once a pad on `engine`: on DRUM, a page with a cell
/// on the pad.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.per_pad"))]
pub(super) const fn per_pad(engine: EngineType, def: &BlockDef) -> bool {
    match engine {
        EngineType::Drum => {
            let mut i = 0;
            while i < def.params.len() {
                if pad_binding(def.params[i].binding) {
                    return true;
                }
                i += 1;
            }
            false
        }
        EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => false,
    }
}

/// Flat page `n` of the chain: each node's home, then its sub-pages.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.def_in"))]
const fn def_in(c: &ChainDef2, mut n: usize) -> Option<&'static BlockDef> {
    let mut b = 0;
    while b < c.blocks.len() {
        let node = &c.blocks[b];
        if n == 0 {
            return Some(node.def);
        }
        n -= 1;
        if n < node.sub_pages.len() {
            return Some(node.sub_pages[n]);
        }
        n -= node.sub_pages.len();
        b += 1;
    }
    None
}

/// The chain's pages, `(the Part's, a pad's)`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.counts"))]
const fn counts(engine: EngineType) -> (usize, usize) {
    let c = chain(engine);
    let (mut part, mut pad) = (0, 0);
    let mut n = 0;
    while let Some(def) = def_in(c, n) {
        n += 1;
        if per_pad(engine, def) {
            pad += 1;
        } else {
            part += 1;
        }
    }
    (part, pad)
}

/// Every page of `engine`, pads included.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.total"))]
const fn total(engine: EngineType) -> usize {
    let (part, pad) = counts(engine);
    part + pad * PadId::ALL.len()
}

// A page number is one `u8` on the wire and in `EdArg::Describe`: every
// engine's count fits, checked here, so `pages` never truncates.
const _: () = {
    let mut i = 0;
    while i < EngineType::ALL.len() {
        assert!(total(EngineType::ALL[i]) <= u8::MAX as usize);
        i += 1;
    }
};

/// The pages the editor walks for `engine`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.pages"))]
pub const fn pages(engine: EngineType) -> u8 {
    total(engine) as u8
}

/// A walk of pages in `describe`'s order: the Part's (`pad` `None`), then
/// each pad's, or one pad's round alone.
pub(super) struct Pages {
    engine: EngineType,
    chain: &'static ChainDef2,
    /// The round walked: the Part's, or a pad's.
    round: Option<PadId>,
    /// Stop after this round (one pad's, or the Part's alone).
    only: bool,
    node: usize,
    /// 0: the node's home, then its sub-pages from 1.
    sub: usize,
}

impl Pages {
    /// Every page.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.all"))]
    pub(super) const fn all(engine: EngineType) -> Pages {
        Pages {
            engine,
            chain: chain(engine),
            round: None,
            only: false,
            node: 0,
            sub: 0,
        }
    }

    /// The Part's pages alone.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.part"))]
    pub(super) const fn part(engine: EngineType) -> Pages {
        Pages {
            only: true,
            ..Pages::all(engine)
        }
    }

    /// Pad `p`'s pages alone.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.pad"))]
    pub(super) const fn pad(engine: EngineType, p: PadId) -> Pages {
        Pages {
            round: Some(p),
            only: true,
            ..Pages::all(engine)
        }
    }

    /// The next def of the chain in this round, or `None` at its end.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.next_def"))]
    fn next_def(&mut self) -> Option<&'static BlockDef> {
        let node = self.chain.blocks.get(self.node)?;
        let def = match self.sub {
            0 => node.def,
            k => node.sub_pages.get(k - 1)?,
        };
        self.sub += 1;
        if self.sub > node.sub_pages.len() {
            self.sub = 0;
            self.node += 1;
        }
        Some(def)
    }
}

impl Iterator for Pages {
    type Item = (&'static BlockDef, Option<PadId>);

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.next"))]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.next_def() {
                Some(def) if per_pad(self.engine, def) == self.round.is_some() => {
                    return Some((def, self.round));
                }
                Some(_) => {}
                None => {
                    // The round ends: the next pad's, unless one was asked.
                    if self.only || counts(self.engine).1 == 0 {
                        return None;
                    }
                    self.round = match self.round {
                        None => Some(PadId::ALL[0]),
                        Some(p) if p.index() + 1 == PadId::ALL.len() => return None,
                        Some(p) => Some(p.next()),
                    };
                    self.node = 0;
                    self.sub = 0;
                }
            }
        }
    }
}

/// Page `n`, in `describe`'s order: its def, and the pad it is a page of.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.page_at"))]
pub fn page_at(engine: EngineType, n: usize) -> Option<(&'static BlockDef, Option<PadId>)> {
    Pages::all(engine).nth(n)
}
