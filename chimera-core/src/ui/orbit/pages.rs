//! ORBIT's pages: ORB (with CUSTOM), RING (RHYTHM, with VOICE and NOTES),
//! CHRD and SCN, a chain of its own that `Loc::Orbit` walks (ADR 0044).

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

/// ORBIT's home and the song's frame: every ring at once. A picks the
/// ring, B to D are KEY, SCALE and BARS (here alone), E mirrors SCN's
/// SCENE, F the tempo (ADR 0079).
pub static ORBIT_ORB: BlockDef = BlockDef {
    id: 78,
    name: "Orbit",
    short: "ORB",
    layout: PageLayout::BigViz,
    viz: VizType::Orbit,
    params: [
        ParamSlot::orbit(OrbitCell::Ring),
        chords(ChordRing::KEY),
        chords(ChordRing::SCALE),
        chords(ChordRing::BARS),
        ParamSlot::orbit(OrbitCell::Scene),
        ParamSlot::param(BlockRef::Tempo, Bpm::BPM),
    ],
};

/// ORB's SCALE slot: EDIT opens CUSTOM with it in focus.
pub const ORB_SCALE: usize = 2;

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

/// The progression, and PLAY: a chord held into a segment. A to C are
/// reserved for chord controls to come (ADR 0079).
pub static ORBIT_CHRD: BlockDef = BlockDef {
    id: 80,
    name: "Chords",
    short: "CHRD",
    layout: PageLayout::CellGrid,
    viz: VizType::OrbitChords,
    params: [
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        chords(ChordRing::SEGS),
        ParamSlot::orbit(OrbitCell::Degree),
        ParamSlot::orbit(OrbitCell::Play),
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

/// The eight scenes: SCENE queues one, SAVE picks where EDIT saves `live`,
/// SWITCH says when a switch lands.
pub static ORBIT_SCN: BlockDef = BlockDef {
    id: 84,
    name: "Scenes",
    short: "SCN",
    layout: PageLayout::CellGrid,
    viz: VizType::OrbitScenes,
    params: [
        ParamSlot::orbit(OrbitCell::Scene),
        ParamSlot::orbit(OrbitCell::Save),
        ParamSlot::orbit(OrbitCell::Switch),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
};

static ORBIT_BLOCKS: [ChainBlock; 4] = [
    ChainBlock::with_subs(&ORBIT_ORB, &[&ORBIT_CUSTOM]),
    ChainBlock {
        map: Some("RING"),
        ..ChainBlock::with_subs(&ORBIT_RHYTHM, &[&ORBIT_VOICE, &ORBIT_NOTES])
    },
    ChainBlock::page(&ORBIT_CHRD),
    ChainBlock::page(&ORBIT_SCN),
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

pub const ORB_NODE: usize = node_of(&ORBIT_ORB);
pub const RING_NODE: usize = node_of(&ORBIT_RHYTHM);
pub const CHRD_NODE: usize = node_of(&ORBIT_CHRD);
pub const SCN_NODE: usize = node_of(&ORBIT_SCN);
