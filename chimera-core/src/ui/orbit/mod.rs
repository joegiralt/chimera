//! ORBIT's pages: ORB, RING and CHRD (with its CUSTOM sub-page), a chain
//! of its own that `Loc::Orbit` walks (ADR 0044). Placeholders until
//! Tasks 9–10 give them cells.

use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot};
use crate::ui::page::PageLayout;

const fn placeholder(id: u16, name: &'static str, short: &'static str) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: crate::ui::block_def::VizType::None,
        params: [ParamSlot::EMPTY; 6],
    }
}

pub static ORBIT_ORB: BlockDef = placeholder(78, "Orbit", "ORB");
static ORBIT_RING: BlockDef = placeholder(79, "Ring", "RNG");
static ORBIT_CHRD: BlockDef = placeholder(80, "Chords", "CHD");
static ORBIT_CUSTOM: BlockDef = placeholder(81, "Custom", "CUS");

static ORBIT_BLOCKS: [ChainBlock; 3] = [
    ChainBlock::page(&ORBIT_ORB),
    ChainBlock::page(&ORBIT_RING),
    ChainBlock::with_subs(&ORBIT_CHRD, &[&ORBIT_CUSTOM]),
];

pub static ORBIT_CHAIN: ChainDef2 =
    ChainDef2::new("Orbit", &ORBIT_BLOCKS, &[]).with_home_def(&ORBIT_ORB);

/// The node whose home is `def`: a chain without it fails the build.
const fn node_of(def: &BlockDef) -> usize {
    let mut i = 0;
    loop {
        assert!(i < ORBIT_BLOCKS.len(), "not an ORBIT node");
        if ORBIT_BLOCKS[i].def.id == def.id {
            break i;
        }
        i += 1;
    }
}

pub const RING_NODE: usize = node_of(&ORBIT_RING);
pub const CHRD_NODE: usize = node_of(&ORBIT_CHRD);
