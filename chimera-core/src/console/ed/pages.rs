//! The pages `ed` walks for a schema: its shared pages first, then each
//! round's (each pad's on DRUM, each Part's on the mixer and SETTINGS:
//! ADR 0285, 0289).
//! One walk, `Pages`, for `describe`, `get`, `set` and `since`, so they
//! can't disagree on what a page is. Its counts are `const`: the page
//! number is a `u8` by a compile-time check, never a silent cast.

use super::schema::{Round, RoundKind, Schema};
use crate::dsp::drum::types::PadId;
use crate::params::EngineType;
use crate::project::PartId;
use crate::ui::block_def::{BlockDef, ChainDef2};

/// The chain the editor walks for `engine`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.chain"))]
pub const fn chain(engine: EngineType) -> &'static ChainDef2 {
    super::schema::engine_chain(engine)
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

/// Flat page `n` of `s`'s chain or leaves, rounds aside.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.def_of"))]
const fn def_of(s: Schema, n: usize) -> Option<&'static BlockDef> {
    match s.chain() {
        Some(c) => def_in(c, n),
        None => {
            let l = s.leaves();
            if n < l.len() { Some(l[n].def) } else { None }
        }
    }
}

/// The schema's pages, `(shared, a round's)`: the matrix has one page, its
/// own.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.counts"))]
const fn counts(s: Schema) -> (usize, usize) {
    if let Schema::Matrix(_) = s {
        return (1, 0);
    }
    let (mut shared, mut round) = (0, 0);
    let mut n = 0;
    while let Some(def) = def_of(s, n) {
        n += 1;
        if s.in_round(def) {
            round += 1;
        } else {
            shared += 1;
        }
    }
    (shared, round)
}

/// How many rounds a schema's round pages go: each pad, each Part.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.rounds_of"))]
const fn rounds_of(k: RoundKind) -> usize {
    match k {
        RoundKind::None => 0,
        RoundKind::Pads => PadId::ALL.len(),
        RoundKind::Parts => PartId::ALL.len(),
    }
}

/// Every page of `s`, every round's included.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.total"))]
const fn total(s: Schema) -> usize {
    let (shared, round) = counts(s);
    shared + round * rounds_of(s.rounds())
}

// A page number is one `u8` on the wire and in `EdArg::Describe`: every
// schema's count fits, checked here, so `pages` never truncates.
const _: () = {
    let mut i = 0;
    while i < Schema::ALL.len() {
        assert!(total(Schema::ALL[i]) <= u8::MAX as usize);
        i += 1;
    }
};

/// The pages the editor walks for `engine`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.pages"))]
pub const fn pages(engine: EngineType) -> u8 {
    schema_pages(Schema::Engine(engine))
}

/// The pages the editor walks for `s`.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.pages.schema_pages")
)]
pub const fn schema_pages(s: Schema) -> u8 {
    total(s) as u8
}

/// Round `i` of kind `k`, after the shared pages.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.round_at"))]
fn round_at(k: RoundKind, i: usize) -> Option<Round> {
    match k {
        RoundKind::None => None,
        RoundKind::Pads => PadId::ALL.get(i).map(|&p| Round::Pad(p)),
        RoundKind::Parts => PartId::ALL.get(i).map(|&p| Round::Part(p)),
    }
}

/// A walk of pages in `describe`'s order: the shared pages, then each
/// round's, or one round alone.
pub(super) struct Pages {
    schema: Schema,
    round: Round,
    /// The next round to walk when this one ends: `None` walking one
    /// round alone.
    next: Option<usize>,
    node: usize,
    /// 0: the node's home, then its sub-pages from 1.
    sub: usize,
}

impl Pages {
    /// Every page.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.all"))]
    pub(super) const fn all(s: Schema) -> Pages {
        Pages {
            schema: s,
            round: Round::Shared,
            next: Some(0),
            node: 0,
            sub: 0,
        }
    }

    /// Round `r`'s pages alone (the shared pages for `Round::Shared`).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.round"))]
    pub(super) const fn round(s: Schema, r: Round) -> Pages {
        Pages {
            round: r,
            next: None,
            ..Pages::all(s)
        }
    }

    /// The next def of the chain (or the leaves) in this round, or `None`
    /// at its end.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.next_def"))]
    fn next_def(&mut self) -> Option<&'static BlockDef> {
        let Some(c) = self.schema.chain() else {
            let leaf = self.schema.leaves().get(self.node)?;
            self.node += 1;
            return Some(leaf.def);
        };
        let node = c.blocks.get(self.node)?;
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
    type Item = (&'static BlockDef, Round);

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.next"))]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let shared = matches!(self.round, Round::Shared);
            match self.next_def() {
                Some(def) if self.schema.in_round(def) != shared => {
                    return Some((def, self.round));
                }
                Some(_) => {}
                None => {
                    // The round ends: the next, unless one was asked.
                    let i = self.next?;
                    self.round = round_at(self.schema.rounds(), i)?;
                    self.next = Some(i + 1);
                    self.node = 0;
                    self.sub = 0;
                }
            }
        }
    }
}

/// Page `n` of `s`, in `describe`'s order: its def, and the round it is
/// a page of.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.pages.page_at"))]
pub fn page_at(s: Schema, n: usize) -> Option<(&'static BlockDef, Round)> {
    Pages::all(s).nth(n)
}
