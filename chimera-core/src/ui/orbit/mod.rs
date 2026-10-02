//! ORBIT's pages: ORB, RING (with VOICE and NOTES) and CHRD (with CUSTOM), a chain
//! of its own that `Loc::Orbit` walks (ADR 0044). Placeholders until
//! Tasks 9–10 give them cells; RING has RHYTHM's. And `OrbitUi`: the
//! transport and what the pages draw of the runner's view.

pub mod cells;

pub use cells::OrbitCell;

use crate::orbit::RINGS;
use crate::orbit::clock::{Bpm, Position};
use crate::orbit::euclid::HitMask;
use crate::orbit::runner::{OrbitShared, OrbitView};
use crate::orbit::scene::{OrbitSong, SceneRing};
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot};
use crate::ui::page::PageLayout;
use chimera_theory::Chord;

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
static ORBIT_RING: BlockDef = BlockDef {
    params: [
        ParamSlot::selected_ring(SceneRing::STEPS),
        ParamSlot::selected_ring(SceneRing::HITS),
        ParamSlot::selected_ring(SceneRing::ROTATE),
        ParamSlot::selected_ring(SceneRing::CHANCE),
        ParamSlot::selected_ring(SceneRing::MUTATE),
        ParamSlot::selected_ring(SceneRing::MUTE),
    ],
    ..placeholder(79, "Ring", "RNG")
};
static ORBIT_CHRD: BlockDef = placeholder(80, "Chords", "CHD");
static ORBIT_CUSTOM: BlockDef = placeholder(81, "Custom", "CUS");
static ORBIT_VOICE: BlockDef = placeholder(82, "Voice", "VOI");
static ORBIT_NOTES: BlockDef = placeholder(83, "Notes", "NOT");

static ORBIT_BLOCKS: [ChainBlock; 3] = [
    ChainBlock::page(&ORBIT_ORB),
    ChainBlock::with_subs(&ORBIT_RING, &[&ORBIT_VOICE, &ORBIT_NOTES]),
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

/// One ring as the pages draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingStatus {
    pub step: u8,
    pub fired: u8,
    pub hits: HitMask,
}

/// What the pages draw of the runner's view: none of its notes or pools,
/// so the UI keeps it whole (`UiState` has a 4 KB guard).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitStatus {
    pub running: bool,
    pub bpm: Bpm,
    pub pos: Position,
    pub beats: u32,
    pub loops: u32,
    pub cued: bool,
    pub seg: u8,
    pub chord: Chord,
    pub rings: [RingStatus; RINGS],
}

impl OrbitStatus {
    pub fn of(v: &OrbitView) -> OrbitStatus {
        OrbitStatus {
            running: v.running,
            bpm: v.bpm,
            pos: v.pos,
            beats: v.beats,
            loops: v.loops,
            cued: v.cued,
            seg: v.seg,
            chord: v.chord,
            rings: v.rings.map(|r| RingStatus {
                step: r.step,
                fired: r.fired,
                hits: r.hits,
            }),
        }
    }
}

/// ORBIT's UI state beside the project: the transport, the CHRD and CUSTOM
/// selections, and the last view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitUi {
    /// CHRD's selected segment.
    pub seg: u8,
    /// CUSTOM's cursor, 0..12 from the tonic.
    pub cursor: u8,
    /// PLAY asked for: what `publish_orbit` sends. Never saved.
    pub run: bool,
    pub scene_rev: u16,
    /// The view's `loops` when the drift was last absorbed.
    pub loops_seen: u32,
    pub status: OrbitStatus,
}

impl OrbitUi {
    pub fn new() -> OrbitUi {
        OrbitUi {
            seg: 0,
            cursor: 0,
            run: false,
            scene_rev: 0,
            loops_seen: 0,
            status: OrbitStatus::of(&OrbitView::default()),
        }
    }

    /// Every field of `out`, every time: a triple buffer's slot comes back
    /// two publishes stale, so a field left alone would flip between
    /// blocks (a false START or STOP).
    pub fn publish(&self, song: &OrbitSong, out: &mut OrbitShared) {
        out.run = self.run;
        out.bpm = song.bpm;
        out.scene_rev = self.scene_rev;
        out.settings = song.live;
    }

    /// Takes the runner's latest view; true when its drift is due in the
    /// song: a loop went by, and no switch is queued. A runner that stopped
    /// itself (a load) clears `run`, once, so one B6 starts it again.
    pub fn see(&mut self, v: &OrbitView) -> bool {
        let was = self.status.running;
        self.status = OrbitStatus::of(v);
        if self.run && was && !v.running {
            self.run = false;
        }
        let due = v.loops != self.loops_seen && !v.cued;
        if due {
            self.loops_seen = v.loops;
        }
        due
    }

    /// B6: PLAY when stopped, STOP when playing.
    pub fn play_stop(&mut self) {
        self.run = !self.status.running;
    }
}

impl Default for OrbitUi {
    fn default() -> Self {
        Self::new()
    }
}
