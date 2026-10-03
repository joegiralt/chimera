//! ORBIT's pages (`pages`), its own cells (`cells`) and how they are drawn
//! (`draw`); and `OrbitUi`: the transport and what the pages draw of the
//! runner's view.

pub mod cells;
pub mod draw;
pub mod pages;

pub use cells::OrbitCell;
pub use pages::{
    CHRD_NODE, ORB_NODE, ORBIT_CHAIN, ORBIT_CHRD, ORBIT_CUSTOM, ORBIT_NOTES, ORBIT_ORB,
    ORBIT_RHYTHM, ORBIT_SCN, ORBIT_VOICE, RING_NODE, SCN_NODE,
};

use crate::block::Block;
use crate::orbit::chords::{Bars, ChordRing};
use crate::orbit::clock::{Bpm, ClockSource, Phase, Position, SyncMode, step_at};
use crate::orbit::euclid::HitMask;
use crate::orbit::runner::{Capture, Captured, OrbitShared, OrbitView, beats_at};
use crate::orbit::scene::{OrbitSong, Refused, Scene, SceneIdx, SceneRing};
use crate::orbit::{RINGS, RingIdx};
use crate::project::{Confirmed, Project, SceneSlot};
use crate::ui::animation::AnimatedValue;
use crate::ui::led::{self, Pulse};
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
    pub source: ClockSource,
    /// The internal BPM, or the MIDI clock's measured one.
    pub bpm: Bpm,
    pub pos: Position,
    /// The playing scene's BARS: its loop.
    pub bars: Bars,
    pub loops: u32,
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
            source: ClockSource::Internal,
            bpm: Bpm::DEFAULT,
            pos: Position::ZERO,
            bars: c.bars,
            loops: 0,
            seg: 0,
            chord,
            rings,
        }
    }

    pub fn of(v: &OrbitView) -> OrbitStatus {
        OrbitStatus {
            running: v.running,
            epoch: v.epoch,
            source: v.source,
            bpm: v.bpm,
            pos: v.pos,
            bars: v.bars,
            loops: v.loops,
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
    fn tick(&mut self, st: &OrbitStatus) -> Option<Pulse> {
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
            return None;
        }
        let mut pulse = None;
        if self.beat != Some(st.beats()) {
            self.beat = Some(st.beats());
            self.dot = PULSE_FULL;
            pulse = Some(Pulse::of(st.beats()));
        }
        for (i, r) in st.rings.iter().enumerate() {
            if r.fired != self.fired[i] {
                self.fired[i] = r.fired;
                self.flash[i] = PULSE_FULL;
            }
        }
        pulse
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

/// ORBIT's UI state beside the project: the transport, the CHRD, CUSTOM
/// and NOTES selections, the last view, and what ORB and the header animate of it.
#[derive(Clone, Copy, Debug)]
pub struct OrbitUi {
    /// CHRD's selected segment.
    pub seg: u8,
    /// CUSTOM's cursor, 0..12 from the tonic.
    pub cursor: u8,
    /// NOTES' selected chip: a pool note, or a STEP ring's step.
    pub chip: u8,
    /// Bumped by every B6 press: what `publish_orbit` sends, and the
    /// runner toggles on each press (ADR 0080). Never saved.
    pub presses: u8,
    /// What LEARN or CHRD's PLAY armed; `Off` once a capture is taken.
    pub capture: Capture,
    /// Bumped by every arming, published with `capture`.
    pub arming: u8,
    /// The view's capture `count` last taken.
    pub took: u16,
    /// Bumped by every switch, published with the scene: the runner
    /// adopts each once, and a view reporting it lands it.
    pub scene_rev: u16,
    /// SCN's SAVE → n; `None`, the playing scene.
    pub save: Option<SceneIdx>,
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
            chip: 0,
            presses: 0,
            capture: Capture::Off,
            arming: 0,
            took: 0,
            scene_rev: 0,
            save: None,
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
    pub fn publish(&self, song: &OrbitSong, sync_mode: SyncMode, out: &mut OrbitShared) {
        let OrbitShared {
            presses,
            bpm,
            sync,
            scene_rev,
            scene,
            settings,
            queued,
            next,
            capture,
            arming,
        } = out;
        *presses = self.presses;
        *bpm = song.bpm;
        *sync = sync_mode;
        *scene_rev = self.scene_rev;
        *scene = song.current();
        *settings = song.live;
        *queued = song.queued();
        *next = *song.scene(song.queued().unwrap_or(song.current()));
        *capture = self.capture;
        *arming = self.arming;
    }

    /// Capture armed into `to`, anew even if it already was.
    pub fn arm(&mut self, to: Capture) {
        self.capture = to;
        self.arming = self.arming.wrapping_add(1);
    }

    /// A segment capture past the progression's SEGS is dropped: SEGS
    /// shrank under it.
    pub fn fit(&mut self, c: &ChordRing) {
        if matches!(self.capture, Capture::Segment(s) if s >= c.segs.get()) {
            self.capture = Capture::Off;
        }
    }

    /// A project was loaded at `epoch`: views from before it are another
    /// project's, and a capture armed in the last one is dropped.
    pub fn loaded(&mut self, epoch: u32) {
        self.epoch = epoch;
        self.capture = Capture::Off;
        self.save = None;
    }

    /// The view's capture, once, for the song to take (`Scene::take`),
    /// and capture disarmed; `None` with nothing new, or played before a
    /// load: on the old project, or while the card was read.
    pub fn take(&mut self, v: &OrbitView) -> Option<Captured> {
        let c = v.captured;
        if c.count == self.took {
            return None;
        }
        self.took = c.count;
        if c.epoch != self.epoch {
            return None;
        }
        if self.capture == c.target {
            self.capture = Capture::Off;
        }
        Some(c)
    }

    /// Takes the runner's latest view; true when its drift is due in the
    /// song: a view on this project's load and a loop gone by (`absorb`
    /// takes it from the playing scene only).
    pub fn see(&mut self, v: &OrbitView) -> bool {
        self.status = OrbitStatus::of(v);
        let due = v.epoch == self.epoch && v.loops != self.loops_seen;
        if due {
            self.loops_seen = v.loops;
        }
        due
    }

    /// SCENE turned to `s`: queued in the song, and a new rev for the
    /// runner, which lands it on the loop (at once when stopped); `s` the
    /// playing scene cancels a waiting switch.
    pub fn queue(&mut self, s: SceneIdx, song: &mut OrbitSong) {
        song.queue(s);
        self.scene_rev = self.scene_rev.wrapping_add(1);
    }

    /// SETTINGS › ORBIT's CLEAR, answered, into the song; a re-landing it
    /// queues gets a new rev.
    pub fn clear_scene(
        &mut self,
        yes: Confirmed<SceneSlot>,
        song: &mut OrbitSong,
    ) -> Result<(), Refused> {
        let was = song.queued();
        song.clear_scene(yes)?;
        self.requeued(was, song);
        Ok(())
    }

    /// SETTINGS › ORBIT's RELOAD, answered: the playing slot lands again,
    /// under a new rev.
    pub fn reload_scene(
        &mut self,
        yes: Confirmed<SceneSlot>,
        song: &mut OrbitSong,
    ) -> Result<(), Refused> {
        let was = song.queued();
        song.reload_scene(yes)?;
        self.requeued(was, song);
        Ok(())
    }

    /// The song's queue moved from `was`: a new rev for the runner.
    fn requeued(&mut self, was: Option<SceneIdx>, song: &OrbitSong) {
        if song.queued() != was {
            self.scene_rev = self.scene_rev.wrapping_add(1);
        }
    }

    /// SCN's SAVE → n: the slot picked, or the playing one.
    pub fn save_slot(&self, song: &OrbitSong) -> SceneIdx {
        self.save.unwrap_or(song.current())
    }

    /// The runner's latest view into the project: a capture taken into the
    /// song, a switch landed once the view reports this rev adopted, the
    /// view kept for the pages, and its drift absorbed once a loop.
    pub fn sync(&mut self, v: &OrbitView, project: &mut Project) {
        if let Some(c) = self.take(v) {
            project.orbit_mut().live.take(&c);
        }
        if project.orbit().queued().is_some()
            && v.epoch == self.epoch
            && v.scene_rev == self.scene_rev
        {
            project.orbit_mut().land(v.playing);
            // SAVE defaults to the playing scene again.
            self.save = None;
        }
        if self.see(v) {
            project.orbit_mut().absorb(v);
        }
    }

    /// One `UiTick` of ORB's and the header's animation; the beat that
    /// began in it, for the LED.
    pub fn tick(&mut self) -> Option<led::Pulse> {
        self.anim.tick(&self.status)
    }

    /// B6: PLAY when stopped, STOP when playing, as the runner is when
    /// the press reaches it, whoever started it (ADR 0080). The transport
    /// is never the project's.
    pub fn play_stop(&mut self) {
        self.presses = self.presses.wrapping_add(1);
    }

    /// MIX + B*n*: ring `r`'s MUTE, in the project (so marked).
    pub fn mute(r: RingIdx, project: &mut Project) {
        let ring = &mut project.orbit_mut().live.rings[r.get() as usize];
        let was = ring.get(SceneRing::MUTE) != 0.0;
        ring.set(SceneRing::MUTE, f32::from(!was));
    }
}

impl Default for OrbitUi {
    fn default() -> Self {
        Self::new()
    }
}
