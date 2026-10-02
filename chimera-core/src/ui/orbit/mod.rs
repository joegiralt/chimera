//! ORBIT's pages (`pages`), its own cells (`cells`) and how they are drawn
//! (`draw`); and `OrbitUi`: the transport and what the pages draw of the
//! runner's view.

pub mod cells;
pub mod draw;
pub mod pages;

pub use cells::OrbitCell;
pub use pages::{
    CHRD_NODE, ORBIT_CHAIN, ORBIT_CHRD, ORBIT_CUSTOM, ORBIT_ORB, ORBIT_RHYTHM, RING_NODE,
};

use crate::block::Block;
use crate::orbit::clock::{Bpm, Phase, Position, step_at};
use crate::orbit::euclid::HitMask;
use crate::orbit::runner::{OrbitShared, OrbitView, beats_at};
use crate::orbit::scene::{OrbitSong, Scene, SceneRing};
use crate::orbit::{RINGS, RingIdx};
use crate::project::Project;
use crate::ui::animation::AnimatedValue;
use crate::ui::nav::OrbitAct;
use chimera_theory::Chord;

/// One ring as the pages draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingStatus {
    pub step: u8,
    pub fired: u8,
    pub hits: HitMask,
    /// The rhythm these hits were played under: an edit not yet played
    /// draws from the scene.
    pub rhythm_rev: u8,
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
            rhythm_rev: 0,
        }; RINGS];
        let mut i = 0;
        while i < RINGS {
            let r = &scene.rings[i].ring;
            rings[i].step = step_at(Phase::ZERO, r.fill().steps());
            rings[i].hits = r.hits_mask();
            rings[i].rhythm_rev = scene.rings[i].rhythm_rev;
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
                rhythm_rev: r.rhythm_rev,
            }),
        }
    }

    /// Beats since the start, from the position.
    pub const fn beats(&self) -> u32 {
        beats_at(self.pos)
    }
}

/// A pulse's fill: full on its first tick, gone by its third.
const PULSE_FULL: u8 = 255;
/// What a pulse loses each tick after its first: 255, 127, 0.
const PULSE_FADE: u8 = 128;

/// What ORB and the header animate from the runner's view, stepped once a
/// `UiTick`: the sweep, the beat dot and each ring's flash.
#[derive(Clone, Copy, Debug)]
pub struct OrbitAnim {
    /// The bar's phase, 0..1 turns, unwrapped so it eases the short way.
    sweep: AnimatedValue,
    /// The beat last pulsed; `None` while stopped.
    beat: Option<u32>,
    dot: u8,
    /// Each ring's `fired` last seen, and its flash.
    fired: [u8; RINGS],
    flash: [u8; RINGS],
}

impl OrbitAnim {
    const fn new() -> OrbitAnim {
        OrbitAnim {
            sweep: AnimatedValue::new(0.0).with_speed(0.6),
            beat: None,
            dot: 0,
            fired: [0; RINGS],
            flash: [0; RINGS],
        }
    }

    /// One `UiTick`: a new beat fills the dot, a ring that fired flashes,
    /// and what was lit fades; the sweep eases toward the phase.
    fn tick(&mut self, st: &OrbitStatus) {
        let fade = |v: &mut u8| *v = v.saturating_sub(PULSE_FADE);
        fade(&mut self.dot);
        self.flash.iter_mut().for_each(fade);
        let turn = st.pos.phase.get() as f32 / 4_294_967_296.0;
        self.sweep
            .set_target(draw::nearest(self.sweep.target(), turn, 1.0));
        self.sweep.update();
        if self.sweep.is_settled() {
            self.sweep.snap(draw::wrap(self.sweep.target(), 1.0));
        }
        if !st.running {
            self.beat = None;
            self.dot = 0;
            self.flash = [0; RINGS];
            self.fired = st.rings.map(|r| r.fired);
            return;
        }
        if self.beat != Some(st.beats()) {
            self.beat = Some(st.beats());
            self.dot = PULSE_FULL;
        }
        for (i, r) in st.rings.iter().enumerate() {
            if r.fired != self.fired[i] {
                self.fired[i] = r.fired;
                self.flash[i] = PULSE_FULL;
            }
        }
    }

    /// The sweep's turn, 0..1 from the top, eased.
    pub fn sweep(&self) -> f32 {
        draw::wrap(self.sweep.current(), 1.0)
    }

    /// Ring `r`'s flash, 0..=255.
    pub fn flash(&self, r: RingIdx) -> u8 {
        self.flash[r.get() as usize]
    }

    /// The beat dot's fill while running, 0..=255; `None` stopped.
    pub fn dot(&self) -> Option<u8> {
        self.beat.map(|_| self.dot)
    }
}

/// ORBIT's UI state beside the project: the transport, the CHRD and CUSTOM
/// selections, the last view, and what ORB and the header animate of it.
#[derive(Clone, Copy, Debug)]
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
    pub anim: OrbitAnim,
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
            anim: OrbitAnim::new(),
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

    /// One `UiTick` of ORB's and the header's animation.
    pub fn tick(&mut self) {
        self.anim.tick(&self.status);
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
