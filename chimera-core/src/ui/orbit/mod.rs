//! ORBIT's pages: ORB, RING (with VOICE and NOTES) and CHRD (with CUSTOM), a chain
//! of its own that `Loc::Orbit` walks (ADR 0044). Placeholders until
//! Tasks 9–10 give them cells; RING has RHYTHM's. And `OrbitUi`: the
//! transport and what the pages draw of the runner's view.

pub mod cells;

pub use cells::OrbitCell;

use crate::block::Block;
use crate::orbit::RINGS;
use crate::orbit::clock::{Bpm, Phase, Position, step_at};
use crate::orbit::euclid::HitMask;
use crate::orbit::runner::{OrbitShared, OrbitView, beats_at};
use crate::orbit::scene::{OrbitSong, Scene, SceneRing};
use crate::project::Project;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot};
use crate::ui::nav::OrbitAct;
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
    /// The load the view's runner is on.
    pub epoch: u32,
    pub bpm: Bpm,
    pub pos: Position,
    pub loops: u32,
    pub cued: bool,
    pub seg: u8,
    pub chord: Chord,
    pub rings: [RingStatus; RINGS],
}

impl OrbitStatus {
    /// The init scene, stopped at the top: the runner's first view.
    pub const IDLE: OrbitStatus = OrbitStatus::stopped(&Scene::init());

    const fn stopped(scene: &Scene) -> OrbitStatus {
        let c = &scene.chords;
        let chord = match c.played[0] {
            Some(ch) => ch,
            None => c.key().triad(c.degrees[0]),
        };
        let mut rings = [RingStatus {
            step: 0,
            fired: 0,
            hits: scene.rings[0].ring.hits_mask(),
        }; RINGS];
        let mut i = 0;
        while i < RINGS {
            let r = &scene.rings[i].ring;
            rings[i].step = step_at(Phase::ZERO, r.fill().steps());
            rings[i].hits = r.hits_mask();
            i += 1;
        }
        OrbitStatus {
            running: false,
            epoch: 0,
            bpm: Bpm::DEFAULT,
            pos: Position::ZERO,
            loops: 0,
            cued: false,
            seg: 0,
            chord,
            rings,
        }
    }

    pub fn of(v: &OrbitView) -> OrbitStatus {
        OrbitStatus {
            running: v.running,
            epoch: v.epoch,
            bpm: v.bpm,
            pos: v.pos,
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

    /// Beats since the start, from the position.
    pub const fn beats(&self) -> u32 {
        beats_at(self.pos)
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
    /// The project's load: only a view on it brings drift.
    pub epoch: u32,
    pub status: OrbitStatus,
}

impl OrbitUi {
    pub const fn new() -> OrbitUi {
        OrbitUi {
            seg: 0,
            cursor: 0,
            run: false,
            scene_rev: 0,
            loops_seen: 0,
            epoch: 0,
            status: OrbitStatus::IDLE,
        }
    }

    /// Every field of `out`, every time: a triple buffer's slot comes back
    /// two publishes stale, so a field left alone would flip between
    /// blocks (a false START or STOP). The destructure fails to compile
    /// when `OrbitShared` gains a field.
    pub fn publish(&self, song: &OrbitSong, out: &mut OrbitShared) {
        let OrbitShared {
            run,
            bpm,
            scene_rev,
            settings,
        } = out;
        *run = self.run;
        *bpm = song.bpm;
        *scene_rev = self.scene_rev;
        *settings = song.live;
    }

    /// A project was loaded at `epoch`: views from before it are another
    /// project's.
    pub fn loaded(&mut self, epoch: u32) {
        self.epoch = epoch;
    }

    /// Takes the runner's latest view; true when its drift is due in the
    /// song: a view on this project's load, a loop gone by, and no switch
    /// queued. A runner that stopped itself clears `run`, once, so one B6
    /// starts it again: seen stopping, or on a new load, which stops it
    /// even before it was seen running.
    pub fn see(&mut self, v: &OrbitView) -> bool {
        let was = self.status;
        self.status = OrbitStatus::of(v);
        if self.run && !v.running && (was.running || v.epoch != was.epoch) {
            self.run = false;
        }
        let due = v.epoch == self.epoch && v.loops != self.loops_seen && !v.cued;
        if due {
            self.loops_seen = v.loops;
        }
        due
    }

    /// What an ORBIT key asks. Only a mute touches the project (and so
    /// marks it); the transport is the UI's.
    pub fn act(&mut self, a: OrbitAct, project: &mut Project) {
        match a {
            // B6: PLAY when stopped, STOP when playing.
            OrbitAct::PlayStop => self.run = !self.status.running,
            OrbitAct::Mute(r) => {
                let ring = &mut project.orbit_mut().live.rings[r.get() as usize];
                let was = ring.get(SceneRing::MUTE) != 0.0;
                ring.set(SceneRing::MUTE, f32::from(!was));
            }
            // Task 16.
            OrbitAct::SaveScene => {}
        }
    }
}

impl Default for OrbitUi {
    fn default() -> Self {
        Self::new()
    }
}
