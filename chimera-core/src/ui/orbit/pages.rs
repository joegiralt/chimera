//! ORBIT's pages: ORB, RING (RHYTHM, with VOICE and NOTES) and CHRD (with
//! CUSTOM), a chain of its own that `Loc::Orbit` walks (ADR 0044).

use super::OrbitCell;
use crate::addr::BlockRef;
use crate::orbit::chords::ChordRing;
use crate::orbit::clock::Bpm;
use crate::orbit::scene::SceneRing;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::page::PageLayout;

const fn chords(id: crate::block::ParamId) -> ParamSlot {
    ParamSlot::param(BlockRef::OrbitChords, id)
}

/// ORBIT's home: every ring at once. A picks the ring, F the tempo.
pub static ORBIT_ORB: BlockDef = BlockDef {
    id: 78,
    name: "Orbit",
    short: "ORB",
    layout: PageLayout::BigViz,
    viz: VizType::Orbit,
    params: [
        ParamSlot::orbit(OrbitCell::Ring),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::param(BlockRef::Tempo, Bpm::BPM),
    ],
};

/// The selected ring's Euclidean fill and its odds.
pub static ORBIT_RHYTHM: BlockDef = BlockDef {
    id: 79,
    name: "Rhythm",
    short: "RHYTHM",
    layout: PageLayout::CellGrid,
    viz: VizType::OrbitRhythm,
    params: [
        ParamSlot::selected_ring(SceneRing::STEPS),
        ParamSlot::selected_ring(SceneRing::HITS),
        ParamSlot::selected_ring(SceneRing::ROTATE),
        ParamSlot::selected_ring(SceneRing::CHANCE),
        ParamSlot::selected_ring(SceneRing::MUTATE),
        ParamSlot::selected_ring(SceneRing::MUTE),
    ],
};

/// The key and the progression; PLAY is Task 14's.
pub static ORBIT_CHRD: BlockDef = BlockDef {
    id: 80,
    name: "Chords",
    short: "CHRD",
    layout: PageLayout::CellGrid,
    viz: VizType::OrbitChords,
    params: [
        chords(ChordRing::KEY),
        chords(ChordRing::SCALE),
        chords(ChordRing::BARS),
        chords(ChordRing::SEGS),
        ParamSlot::orbit(OrbitCell::Degree),
        ParamSlot::EMPTY,
    ],
};

/// The CUSTOM set round its circle, shown while SCALE is CUSTOM.
pub static ORBIT_CUSTOM: BlockDef = BlockDef {
    id: 81,
    name: "Custom",
    short: "CUSTOM",
    layout: PageLayout::BigViz,
    viz: VizType::OrbitCircle,
    params: [
        ParamSlot::orbit(OrbitCell::Cursor),
        ParamSlot::orbit(OrbitCell::Set),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
};

/// How the selected ring's hits sound: their length, their force, and
/// where in pitch they sit.
pub static ORBIT_VOICE: BlockDef = BlockDef {
    id: 82,
    name: "Voice",
    short: "VOICE",
    layout: PageLayout::CellGrid,
    viz: VizType::OrbitVoice,
    params: [
        ParamSlot::selected_ring(SceneRing::GATE),
        ParamSlot::selected_ring(SceneRing::VEL),
        ParamSlot::selected_ring(SceneRing::ACCENT),
        ParamSlot::selected_ring(SceneRing::OCTAVE),
        ParamSlot::selected_ring(SceneRing::RANGE),
        ParamSlot::selected_ring(SceneRing::SNAP),
    ],
};

/// The selected ring's notes, as a pool walked or one a step, and the
/// Part they play.
pub static ORBIT_NOTES: BlockDef = BlockDef {
    id: 83,
    name: "Notes",
    short: "NOTES",
    layout: PageLayout::CellGrid,
    viz: VizType::OrbitNotes,
    params: [
        ParamSlot::selected_ring(SceneRing::MODE),
        ParamSlot::selected_ring(SceneRing::WALK),
        ParamSlot::selected_ring(SceneRing::TARGET),
        ParamSlot::orbit(OrbitCell::Learn),
        ParamSlot::orbit(OrbitCell::Clear),
        ParamSlot::orbit(OrbitCell::Note),
    ],
};

static ORBIT_BLOCKS: [ChainBlock; 3] = [
    ChainBlock::page(&ORBIT_ORB),
    ChainBlock {
        map: Some("RING"),
        ..ChainBlock::with_subs(&ORBIT_RHYTHM, &[&ORBIT_VOICE, &ORBIT_NOTES])
    },
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

pub const RING_NODE: usize = node_of(&ORBIT_RHYTHM);
pub const CHRD_NODE: usize = node_of(&ORBIT_CHRD);
