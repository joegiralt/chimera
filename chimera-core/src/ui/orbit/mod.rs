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
use crate::orbit::channel::{Channel, CmdLog, SceneCmd};
use crate::orbit::chords::{Bars, ChordRing};
use crate::orbit::clock::{Bpm, ClockSource, Phase, Position, SyncMode, step_at};
use crate::orbit::euclid::HitMask;
use crate::orbit::runner::{Capture, Captured, OrbitShared, OrbitView, beats_at};
use crate::orbit::scene::{OrbitSong, Scene, SceneIdx, SceneRing};
use crate::orbit::{RINGS, RingIdx};
use crate::project::Project;
use crate::ui::animation::{AnimatedValue, wrap};
use crate::ui::led::{self, Pulse};
use chimera_theory::Chord;

/// One ring as the pages draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingStatus {
    pub step: u8,
    pub fired: u8,
    pub hits: HitMask,
}

/// What the pages draw of the runner's view: none of its notes or pools,
/// so the UI keeps it whole (`UiState` has a size guard).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitStatus {
    pub running: bool,
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
        let turn = st.pos.phase.turns();
        self.sweep.ease_round(turn, 1.0);
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
        wrap(self.sweep.current(), 1.0)
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
/// and NOTES selections, the commands to the runner, the last view, and
/// what ORB and the header animate of it.
#[derive(Clone, Copy, Debug)]
pub struct OrbitUi {
    /// CHRD's selected segment.
    pub seg: u8,
    /// CUSTOM's cursor, 0..12 from the tonic.
    pub cursor: u8,
    /// NOTES' selected chip: a pool note, or a STEP ring's step.
    pub chip: u8,
    /// What LEARN or CHRD's PLAY armed: the UI's intent until the runner
    /// has applied every command, then the view's `armed`.
    pub capture: Capture,
    /// The view's capture `count` last taken.
    pub took: u8,
    /// SCN's SAVE → n; `None`, the playing scene.
    pub save: Option<SceneIdx>,
    /// The view's `loops`, its low byte, when the drift was last absorbed.
    pub loops_seen: u8,
    /// CLEAR downs, each moving the roll's salt.
    clears: u8,
    /// The commands for the runner, and its ack (ADR 0082).
    chan: Channel,
    /// The project's load: no view from before it is taken.
    epoch: u32,
    pub status: OrbitStatus,
    pub anim: OrbitAnim,
}

impl OrbitUi {
    pub const fn new() -> OrbitUi {
        OrbitUi {
            seg: 0,
            cursor: 0,
            chip: 0,
            capture: Capture::Off,
            took: 0,
            save: None,
            loops_seen: 0,
            clears: 0,
            chan: Channel::new(),
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
        debug_assert!(song.live.untouched(), "an edit not flushed");
        let OrbitShared {
            bpm,
            sync,
            settings,
            next,
            log,
            load,
            synced,
        } = out;
        *bpm = song.bpm;
        *sync = sync_mode;
        *settings = song.live;
        *next = *song.scene(song.queued().unwrap_or(song.current()));
        *log = *self.chan.log();
        *load = self.chan.load();
        *synced = self.chan.synced();
    }

    /// `live`'s edits since the last harvest, into the outbox.
    #[inline(never)]
    fn harvest(&mut self, song: &mut OrbitSong) {
        let (rhythm, notes) = song.live.harvest();
        self.chan.edit(rhythm, notes);
    }

    /// The frame's edits and commands into the log, before its publish.
    pub fn flush(&mut self, song: &mut OrbitSong) {
        self.harvest(song);
        self.chan.flush();
    }

    /// `flush` with no edit to harvest: the caller knows `live` is
    /// untouched, and saves the `&mut` that would re-key the project.
    pub fn flush_log(&mut self) {
        self.chan.flush();
    }

    /// The song's scene intent as the newest scene command: land the
    /// queued slot, or play `live` as the playing one.
    pub fn issue_scene(&mut self, song: &OrbitSong) {
        self.chan.scene(match song.queued() {
            Some(q) => SceneCmd::Cue(q),
            None => SceneCmd::Keep(song.current()),
        });
    }

    /// Capture armed into `to`, anew even if it already was: the runner
    /// drops what was under way.
    pub fn arm(&mut self, to: Capture) {
        self.capture = to;
        self.chan.arm(to);
    }

    pub fn disarm(&mut self) {
        self.arm(Capture::Off);
    }

    /// A segment capture past the progression's SEGS is dropped: SEGS
    /// shrank under it.
    pub fn fit(&mut self, c: &ChordRing) {
        if matches!(self.capture, Capture::Segment(s) if s >= c.segs.get()) {
            self.disarm();
        }
    }

    /// A project was loaded at `epoch`, before its publish: views from
    /// before it are another project's, every command before it is void,
    /// and a capture armed in the last one is dropped. The runner takes
    /// `live` whole, and the playing slot from a `Keep` in the same
    /// publish. Cold: kept out of the card work.
    #[inline(never)]
    pub fn loaded(&mut self, epoch: u32, song: &mut OrbitSong) {
        // The decode's writes: no edits.
        song.live.harvest();
        self.chan.loaded();
        self.chan.scene(SceneCmd::Keep(song.current()));
        self.chan.flush();
        self.epoch = epoch;
        self.capture = Capture::Off;
        self.save = None;
    }

    /// The view's capture, once, for the song to take (`Scene::take`);
    /// `None` with nothing new.
    pub fn take(&mut self, v: &OrbitView) -> Option<Captured> {
        let c = v.captured;
        if c.count == self.took {
            return None;
        }
        self.took = c.count;
        Some(c)
    }

    /// SCENE turned to `s`: queued in the song, and the switch (or, back
    /// on the playing scene, its cancel) sent to the runner, which lands
    /// it on the loop (at once when stopped).
    pub fn queue(&mut self, s: SceneIdx, song: &mut OrbitSong) {
        song.queue(s);
        self.issue_scene(song);
    }

    /// CLEAR down's salt: new on every turn, the log flowing or full.
    /// A fresh UI's first is 0.
    pub fn clear_salt(&mut self) -> u8 {
        let salt = self.clears.wrapping_add(self.chan.log().head().get());
        self.clears = self.clears.wrapping_add(1);
        salt
    }

    /// Every command sent is applied: the view's drift is on top of
    /// every edit, so the pages may draw it. Out of line: each ring's
    /// draw asks.
    #[inline(never)]
    pub fn fresh(&self) -> bool {
        self.chan.idle()
    }

    /// The scene command sent and not yet landed.
    pub fn awaiting(&self) -> Option<SceneCmd> {
        self.chan.awaiting()
    }

    /// The log as the next publish carries it.
    pub fn log(&self) -> &CmdLog {
        self.chan.log()
    }

    /// SCN's SAVE → n: the slot picked, or the playing one.
    pub fn save_slot(&self, song: &OrbitSong) -> SceneIdx {
        self.save.unwrap_or(song.current())
    }

    /// The runner's latest view into the project, in one order (ADR
    /// 0082): the load gate and the ack, the view kept for the pages, a
    /// capture taken into the song, LEARN and PLAY shown as the runner is
    /// armed once every command is applied, the newest scene command landed once
    /// the runner has adopted it, and the drift absorbed once a loop
    /// while every command is applied.
    pub fn sync(&mut self, v: &OrbitView, project: &mut Project) {
        if v.epoch != self.epoch || !self.chan.ack(v.acked) {
            return;
        }
        self.status = OrbitStatus::of(v);
        // An edit since the last flush counts as unapplied.
        if !project.orbit().live.untouched() {
            self.harvest(project.orbit_mut());
        }
        let idle = self.chan.idle();
        if let Some(c) = self.take(v) {
            let song = project.orbit_mut();
            song.live.take(&c);
            self.harvest(song);
        }
        // Every arm applied: the runner's armed target is the truth, a
        // capture taken or a re-arm since included.
        if idle {
            self.capture = v.armed;
        }
        if let Some(c) = self.chan.awaiting()
            && idle
            && v.cue.is_none()
        {
            if let SceneCmd::Cue(n) = c {
                project.orbit_mut().land(n);
                // SAVE defaults to the playing scene again.
                self.save = None;
            }
            self.chan.landed(c);
        }
        // `idle()` again, not step 3's: a capture taken above is an edit
        // the runner hasn't applied, and drift would overwrite it.
        if self.chan.synced() && self.chan.idle() && v.loops as u8 != self.loops_seen {
            self.loops_seen = v.loops as u8;
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
        self.chan.flip();
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
