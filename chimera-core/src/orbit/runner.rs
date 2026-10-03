//! `OrbitRunner`: ORBIT's audio-thread shell. It owns the clock, the
//! transport and the working copy of the playing scene, and each block
//! turns them into note events for the `Instrument` (spec § 4).

use super::chords::{Bars, harmony};
use super::clock::{
    Bpm, ClockSource, InternalClock, MidiClock, Position, SyncMode, WINDOW, advance_cap, step_at,
};
use super::engine::{Events, OrbitKind, OrbitState, Span, block};
use super::euclid::HitMask;
use super::ring::{Harmony, NoteMode};
use super::scene::{Scene, SceneIdx};
use super::walk::Pool;
use super::{MAX_POOL, MAX_STEPS, RINGS, RingIdx};
use crate::MidiNote;
use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::{AudioShared, Instrument, NoteOrigin, NoteSet};
use crate::note_queue::{NoteEvent, NoteKind, RealTime};
use crate::triple::TripleBuffer;
use chimera_theory::{Chord, Note};
use core::mem::size_of;
use core::num::NonZeroU32;

/// What the UI asks of ORBIT, published with the Parts (ADR 0021).
#[derive(Clone, Debug)]
pub struct OrbitShared {
    /// Bumped on every B6 press: each press starts a stopped ORBIT or
    /// stops a playing one (ADR 0080). A count, so presses between two
    /// runner reads never alias.
    pub presses: u8,
    pub bpm: Bpm,
    /// MIDI CONFIG's SYNC, from SYSTEM (ADR 0080).
    pub sync: SyncMode,
    /// Bumped on every switch, re-landing and cancel; the runner acts on
    /// each rev once (ADR 0077).
    pub scene_rev: u16,
    /// The playing scene, which `settings` is: the knobs come from it.
    pub scene: SceneIdx,
    pub settings: Scene,
    /// A switch waiting on the loop, and the slot it brings (`next`),
    /// adopted whole at the boundary; the playing slot itself lands again.
    /// `next` means nothing without it.
    pub queued: Option<SceneIdx>,
    pub next: Scene,
    /// What the keys play into (spec § 5.3).
    pub capture: Capture,
    /// Bumped on every arming, the same target again included: the runner
    /// arms on its change (ADR 0075).
    pub arming: u8,
}

impl OrbitShared {
    /// What the newest rev brings: the queued slot, or `settings` itself.
    fn incoming(&self) -> (SceneIdx, &Scene) {
        match self.queued {
            Some(s) => (s, &self.next),
            None => (self.scene, &self.settings),
        }
    }
}

impl Default for OrbitShared {
    fn default() -> Self {
        OrbitShared {
            presses: 0,
            bpm: Bpm::DEFAULT,
            sync: SyncMode::DEFAULT,
            scene_rev: 0,
            scene: SceneIdx::ALL[0],
            settings: Scene::init(),
            queued: None,
            next: Scene::init(),
            capture: Capture::Off,
            arming: 0,
        }
    }
}

/// What a capture is armed into: NOTES' LEARN arms a ring, CHRD's PLAY a
/// segment (spec § 5.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Capture {
    #[default]
    Off,
    Ring(RingIdx),
    Segment(u8),
}

/// The runner's last capture, as the view reports it: the UI takes each
/// new `count` into its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Captured {
    pub count: u16,
    pub target: Capture,
    /// The arming it was taken under: a newer one stays armed in the UI.
    pub arming: u8,
    /// In the order played, each once (a STEP ring's may repeat).
    pub notes: [Note; MAX_POOL],
    pub len: u8,
    /// The project load it was played on: another's is never taken.
    pub epoch: u32,
}

impl Captured {
    const NONE: Captured = Captured {
        count: 0,
        target: Capture::Off,
        arming: 0,
        notes: [Note::MIN; MAX_POOL],
        len: 0,
        epoch: 0,
    };

    pub fn notes(&self) -> &[Note] {
        &self.notes[..self.len as usize]
    }
}

/// A capture under way: the notes since the first key-down.
#[derive(Clone, Copy, Debug)]
struct Take {
    /// The arming last acted on: a new one arms, so a slot still armed
    /// after a capture never takes a second, and an arming in the frame
    /// a capture is taken is never lost.
    arming: u8,
    /// What the keys play into; `Off` once taken, until re-armed.
    armed: Capture,
    /// Keys down since the first, by MIDI note: the last one up ends it.
    held: NoteSet,
    notes: [Note; MAX_POOL],
    len: u8,
    done: Captured,
}

impl Take {
    const fn new() -> Take {
        Take {
            arming: 0,
            armed: Capture::Off,
            held: NoteSet::EMPTY,
            notes: [Note::MIN; MAX_POOL],
            len: 0,
            done: Captured::NONE,
        }
    }

    /// The UI's target and arming, as published: a new arming arms and
    /// starts over; the target gone `Off` disarms.
    fn arm(&mut self, to: Capture, arming: u8) {
        if arming != self.arming {
            self.arming = arming;
            self.armed = to;
            self.restart();
        } else if to == Capture::Off && self.armed != Capture::Off {
            self.armed = Capture::Off;
            self.restart();
        }
    }

    fn restart(&mut self) {
        (self.held, self.len) = (NoteSet::EMPTY, 0);
    }

    /// A load disarms (ADR 0075): its publish still carries the old
    /// arming, and keys held through it end nothing.
    fn forget(&mut self) {
        self.armed = Capture::Off;
        self.restart();
    }

    fn played(&self) -> &[Note] {
        &self.notes[..self.len as usize]
    }

    /// The notes taken: a segment's only if they make a chord, else it
    /// stays armed for the next. `epoch` is the load it was played on.
    fn finish(&mut self, epoch: u32) {
        if matches!(self.armed, Capture::Segment(_)) && Chord::from_notes(self.played()).is_none() {
            return self.restart();
        }
        self.done = Captured {
            count: self.done.count.wrapping_add(1),
            target: self.armed,
            arming: self.arming,
            notes: self.notes,
            len: self.len,
            epoch,
        };
        self.armed = Capture::Off;
        self.restart();
    }
}

/// One ring as the pages draw it: the working copy, drift and all.
#[derive(Clone, Copy, Debug)]
pub struct RingView {
    pub step: u8,
    /// Counts the ring's note-ons; the dots flash on a change.
    pub fired: u8,
    pub hits: HitMask,
    pub pool: Pool,
    pub notes: [Note; MAX_STEPS as usize],
    /// The revs these hits and notes were played under: `absorb` keeps
    /// an edit the runner hasn't seen yet.
    pub rhythm_rev: u8,
    pub notes_rev: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct OrbitView {
    pub running: bool,
    /// The project load the runner is on (`AudioShared.epoch`).
    pub epoch: u32,
    pub source: ClockSource,
    /// The internal BPM, or the MIDI clock's measured one.
    pub bpm: Bpm,
    pub pos: Position,
    /// The playing scene's loop, so a page can say where in it `pos` is.
    pub bars: Bars,
    pub beats: u32,
    pub loops: u32,
    /// The scene adopted last, and the switch it adopted (`scene_rev`):
    /// the UI lands a switch on its own rev only, never a stale view's.
    pub playing: SceneIdx,
    pub scene_rev: u16,
    /// A switch waiting on the loop boundary.
    pub queued: Option<SceneIdx>,
    pub seg: u8,
    pub chord: Chord,
    pub rings: [RingView; RINGS],
    pub captured: Captured,
}

/// Beats in a bar (4/4, spec § 6.2) and the shift taking a phase to a beat.
pub(crate) const BEATS_PER_BAR: u32 = 4;
const BEAT_SHIFT: u32 = 32 - BEATS_PER_BAR.trailing_zeros();

/// Beats from the start to `pos`.
pub const fn beats_at(pos: Position) -> u32 {
    pos.bar
        .wrapping_mul(BEATS_PER_BAR)
        .wrapping_add(pos.phase.get() >> BEAT_SHIFT)
}

impl OrbitView {
    /// The view of `scene` stopped at `pos`.
    fn of(scene: &Scene, pos: Position) -> OrbitView {
        OrbitView {
            running: false,
            epoch: 0,
            source: ClockSource::Internal,
            bpm: Bpm::DEFAULT,
            pos,
            bars: scene.chords.bars,
            beats: beats_at(pos),
            loops: 0,
            playing: SceneIdx::ALL[0],
            scene_rev: 0,
            queued: None,
            seg: 0,
            chord: harmony(&scene.chords, 0).chord,
            rings: core::array::from_fn(|i| {
                let s = &scene.rings[i];
                let r = &s.ring;
                RingView {
                    step: step_at(pos.phase, r.fill().steps()),
                    fired: 0,
                    hits: r.hits_mask(),
                    pool: r.pool,
                    notes: r.notes,
                    rhythm_rev: s.rhythm_rev,
                    notes_rev: s.notes_rev,
                }
            }),
            captured: Captured::NONE,
        }
    }
}

impl Default for OrbitView {
    fn default() -> Self {
        OrbitView::of(&Scene::init(), Position::ZERO)
    }
}

/// The report's triple buffer, to leak and `split` like the scope's.
pub fn orbit_buffer() -> TripleBuffer<OrbitView> {
    TripleBuffer::new(
        OrbitView::default(),
        OrbitView::default(),
        OrbitView::default(),
    )
}

/// A ring's revs as last copied from the settings.
type Revs = (u8, u8);

pub struct OrbitRunner {
    clock: InternalClock,
    midi: MidiClock,
    /// The clock that moved the last block.
    source: ClockSource,
    /// SYNC as of the last block: INT hears no real-time (ADR 0080).
    sync: SyncMode,
    /// A MIDI Start to act on next block: offs, then bar 0.
    ext_start: bool,
    /// The DAW's last Start or Continue (true) or Stop (false), to act on
    /// next block: the transport up or down.
    ext_play: Option<bool>,
    /// The previous window's end: the next one starts here.
    edge: Position,
    state: OrbitState,
    working: Scene,
    revs: [Revs; RINGS],
    /// The switch adopted last, and its scene.
    scene_rev: u16,
    playing: SceneIdx,
    /// `presses` as of the last block.
    presses_seen: u8,
    /// Whether ORBIT plays: B6 toggles it, the DAW's Start and Continue
    /// set it and its Stop clears it (ADR 0080).
    transport: bool,
    /// A switch waiting on the boundary: the scene it brings.
    cue: Option<SceneIdx>,
    epoch: u32,
    /// The sample at the start of the next block.
    sample: u64,
    sample_rate: NonZeroU32,
    loops: u32,
    fired: [u8; RINGS],
    take: Take,
    /// The harmony last handed to the `Instrument`: a change re-snaps.
    harmony: Option<Harmony>,
}

const _: () = assert!(size_of::<OrbitRunner>() <= 1024);

fn revs_of(s: &Scene) -> [Revs; RINGS] {
    core::array::from_fn(|i| (s.rings[i].rhythm_rev, s.rings[i].notes_rev))
}

impl OrbitRunner {
    pub fn new(sample_rate: NonZeroU32) -> Self {
        let working = Scene::init();
        OrbitRunner {
            clock: InternalClock::start(0, Bpm::DEFAULT, sample_rate),
            midi: MidiClock::new(sample_rate),
            source: ClockSource::Internal,
            sync: SyncMode::DEFAULT,
            ext_start: false,
            ext_play: None,
            edge: Position::ZERO,
            state: OrbitState::start(&working),
            revs: revs_of(&working),
            working,
            scene_rev: 0,
            playing: SceneIdx::ALL[0],
            presses_seen: 0,
            transport: false,
            cue: None,
            epoch: 0,
            sample: 0,
            sample_rate,
            loops: 0,
            fired: [0; RINGS],
            take: Take::new(),
            harmony: None,
        }
    }

    /// What the newest rev brings, whole, as the playing scene.
    fn adopt(&mut self, shared: &AudioShared) {
        let (s, scene) = shared.orbit.incoming();
        self.working = *scene;
        self.revs = revs_of(&self.working);
        self.scene_rev = shared.orbit.scene_rev;
        self.playing = s;
        self.cue = None;
    }

    /// The knobs from `settings`; a ring's hits and notes only if their
    /// rev moved, so MUTATE's drift stands otherwise.
    fn copy_knobs(&mut self, settings: &Scene) {
        let w = &mut self.working;
        w.chords = settings.chords;
        w.seed = settings.seed;
        for (i, s) in settings.rings.iter().enumerate() {
            let (rhythm, notes) = self.revs[i];
            let mut sr = *s;
            let old = w.rings[i].ring;
            if s.rhythm_rev == rhythm {
                // `None`: the fill changed with no rev, so the fresh one stands.
                sr.ring = sr.ring.with_mask(old.hits_mask()).unwrap_or(sr.ring);
            }
            if s.notes_rev == notes {
                sr.ring.notes = old.notes;
                sr.ring.pool = old.pool;
            }
            w.rings[i] = sr;
            self.revs[i] = (s.rhythm_rev, s.notes_rev);
        }
    }

    fn send(inst: &mut Instrument, shared: &AudioShared, ev: &Events, fired: &mut [u8; RINGS]) {
        for e in ev.iter() {
            let Some(note) = MidiNote::new(e.note.get()) else {
                continue;
            };
            let kind = match e.kind {
                OrbitKind::On(v) => {
                    let n = &mut fired[e.ring.get() as usize];
                    *n = n.wrapping_add(1);
                    NoteKind::On(v)
                }
                OrbitKind::Off => NoteKind::Off,
            };
            let ev = NoteEvent {
                channel: e.channel,
                note,
                kind,
            };
            inst.handle_from(ev, shared, NoteOrigin::Orbit(e.ring));
        }
    }

    /// The sample the next block starts at: the `now` of the real-time
    /// messages drained before it.
    pub fn now(&self) -> u64 {
        self.sample
    }

    /// A MIDI real-time message from source `src`, drained at `now` (§ 6.1).
    /// Only the leading clock's are taken, and none under SYNC INT. Start,
    /// Continue and Stop act at the next block, which sends their offs.
    pub fn realtime(&mut self, src: usize, rt: RealTime, now: u64) {
        if self.sync == SyncMode::Int {
            return;
        }
        // It came during the block before its drain: half a block, on average.
        let now = now.saturating_sub(WINDOW);
        let led = self.midi.position(now).is_some();
        match rt {
            // A clock that led nothing before it (a new leader, one back
            // from silence, or a stopped DAW's) counts on from where ORBIT
            // is. A stopped DAW's stays unled; Continue rebases anyway.
            RealTime::Clock => {
                if self.midi.tick(src, now) && !led {
                    self.midi.rebase(self.edge);
                }
            }
            RealTime::Start => {
                if self.midi.start(src, now) {
                    self.ext_start = true;
                    self.ext_play = Some(true);
                }
            }
            // On from where ORBIT is, which B6 may have moved since Stop.
            RealTime::Continue => {
                if self.midi.resume(src, now) {
                    self.midi.rebase(self.edge);
                    self.ext_play = Some(true);
                }
            }
            RealTime::Stop => {
                if self.midi.stop(src, now) {
                    self.ext_play = Some(false);
                }
            }
        }
    }

    /// A live note, drained before it goes to the `Instrument`: into an
    /// armed capture (spec § 5.3), from any source and channel. It runs
    /// from the first key-down to the last key-up, at most `MAX_POOL`
    /// notes, each once; a STEP ring's takes one a key-down, repeats too,
    /// and ends when its hits are filled.
    pub fn live(&mut self, ev: NoteEvent) {
        let step = match self.take.armed {
            Capture::Off => return,
            Capture::Segment(_) => None,
            Capture::Ring(r) => {
                let ring = &self.working.rings[r.get() as usize].ring;
                (ring.mode == NoteMode::Step)
                    .then(|| ring.hits_mask().hits().clamp(1, MAX_POOL as u8))
            }
        };
        let epoch = self.epoch;
        let t = &mut self.take;
        let Some(note) = Note::new(ev.note.get()) else {
            return;
        };
        match ev.kind {
            NoteKind::On(_) => {
                t.held = t.held.with(ev.note);
                let fresh = step.is_some() || !t.played().contains(&note);
                if fresh && (t.len as usize) < MAX_POOL {
                    t.notes[t.len as usize] = note;
                    t.len += 1;
                }
                if step.is_some_and(|n| t.len >= n) {
                    t.finish(epoch);
                }
            }
            NoteKind::Off => {
                if !t.held.contains(ev.note) {
                    return;
                }
                t.held = t.held.without(ev.note);
                if t.held.is_empty() && step.is_none() {
                    t.finish(epoch);
                }
            }
        }
    }

    /// Spec § 4 steps 2-5 for one block: what it sent the `Instrument`.
    /// The shells drop it; the tests read each block's events from it.
    /// Step 5 hands the `Instrument` the harmony its live keys snap to,
    /// `None` while stopped, and re-snaps held keys whenever it differs
    /// from the last one handed (spec § 5.2): a segment change, a landed
    /// scene, a start or a MIDI Start back to bar 0, a chord played in.
    pub fn block(&mut self, inst: &mut Instrument, shared: &AudioShared) -> Events {
        self.take.arm(shared.orbit.capture, shared.orbit.arming);
        let sent = self.advance(shared);
        Self::send(inst, shared, &sent, &mut self.fired);
        let h = self.transport.then(|| self.state.harmony(&self.working));
        inst.set_harmony(h);
        if h.is_some() && h != self.harmony {
            inst.resnap(shared);
        }
        self.harmony = h;
        sent
    }

    /// One block's transport, clock and steps.
    fn advance(&mut self, shared: &AudioShared) -> Events {
        let o = &shared.orbit;
        let t = self.sample;
        self.sample += BLOCK_SIZE as u64;

        if shared.epoch != self.epoch {
            // A load killed every voice: nothing to release, and no edge.
            // It is the player's newer act, so the DAW's pending Start,
            // Continue or Stop is dropped: its next Start plays the load.
            self.epoch = shared.epoch;
            (self.ext_start, self.ext_play) = (false, None);
            self.state.forget();
            self.take.forget();
            self.transport = false;
            self.presses_seen = o.presses;
            self.adopt(shared);
            return Events::new();
        }

        if o.sync != self.sync {
            // Either way the other mode's clock is forgotten: INT never
            // hears it, and EXT takes the next one up from the edge.
            self.sync = o.sync;
            self.midi = MidiClock::new(self.sample_rate);
            (self.ext_start, self.ext_play) = (false, None);
        }

        // The DAW's Start or Stop first ends what ORBIT holds; Start goes
        // back to bar 0.
        let was = self.transport;
        let reset = core::mem::take(&mut self.ext_start);
        let daw = self.ext_play.take();
        let mut sent = Events::new();
        if reset || daw == Some(false) && was {
            sent = self.state.stop();
        }
        if daw == Some(false) {
            self.transport = false;
        }

        let midi = self.midi.position(t + WINDOW);
        // Every press toggles: an odd count since the last block flips it.
        let pressed = o.presses.wrapping_sub(self.presses_seen) & 1 == 1;
        self.presses_seen = o.presses;
        // B6 toggles the transport as it was before the DAW's word, so B6
        // and a Stop in one block both stop.
        let play = match (pressed, daw) {
            (true, _) => !was,
            (false, Some(p)) => p,
            (false, None) => was,
        };
        if play && !self.transport {
            self.adopt(shared);
            if midi.is_some() {
                // Joins the external clock in phase: the dice from the
                // seed, the position kept (§ 6.3).
                self.state.restart_dice(&self.working);
            } else {
                // No external clock: ORBIT's own, from bar 0.
                self.clock = InternalClock::start(t, o.bpm, self.sample_rate);
                self.edge = Position::ZERO;
                self.state = OrbitState::start(&self.working);
                self.loops = 0;
            }
            self.transport = true;
        } else if !play && self.transport {
            sent = sent.then(&self.state.stop());
            self.transport = false;
        }
        if reset {
            self.edge = Position::ZERO;
            self.state = OrbitState::start(&self.working);
            self.loops = 0;
        }

        if o.scene_rev != self.scene_rev {
            // A queued slot lands, the playing one again too (CLEAR or
            // RELOAD); back to the scene playing with nothing queued is a
            // cancel, nothing to adopt. Its rev isn't taken: `scene_rev`
            // stays the adopted scene's, which the UI's drift is gated on.
            let (s, _) = o.incoming();
            self.cue = (o.queued.is_some() || s != self.playing).then_some(s);
        }
        if self.cue.is_some() && !self.transport {
            self.adopt(shared);
        }
        // Only the playing scene's knobs: a waiting switch's are its own,
        // and once the UI's switch is adopted its `settings` are a frame
        // behind until its view lands it.
        let unseen = o.queued.is_some() && o.scene_rev == self.scene_rev;
        if o.scene == self.playing && !unseen {
            self.copy_knobs(&o.settings);
        }

        let sounding = self.transport;
        let (to, from_sample, bar_len) = match midi {
            Some(at) => {
                self.source = ClockSource::Midi;
                (at, t.saturating_sub(WINDOW), self.midi.bar_len())
            }
            None => {
                if !sounding {
                    // Held where ORBIT is, to go on from the next edge.
                    self.clock.set_bpm(t + WINDOW, self.edge, o.bpm);
                    self.source = ClockSource::Internal;
                    return sent;
                }
                let from_sample = t.saturating_sub(WINDOW).max(self.clock.anchor());
                // The fallback, or a tempo change: on from the edge.
                if self.source == ClockSource::Midi || o.bpm != self.clock.bpm() {
                    self.clock.set_bpm(from_sample, self.edge, o.bpm);
                }
                self.source = ClockSource::Internal;
                let to = self.clock.position(t + WINDOW);
                (to, from_sample, self.clock.bar_len())
            }
        };
        // Never back, and never more than the cap ahead: a jump is caught
        // up over the next blocks, a step boundary per ring per window.
        let cap = self
            .edge
            .units()
            .saturating_add(advance_cap(self.sample_rate));
        let to = to.clamp(self.edge, Position::from_units(cap));
        if !sounding {
            // The external clock moves on without ORBIT, so B6 joins in phase.
            self.edge = to;
            return sent;
        }

        let span = Span {
            from: self.edge,
            to,
            from_sample,
            to_sample: t + WINDOW,
            bar_len,
        };
        let channels = core::array::from_fn::<_, MAX_PARTS, _>(|i| shared.parts[i].mix.channel);
        let cue = self.cue.map(|_| o.incoming().1);
        let ev = block(&mut self.state, &mut self.working, cue, span, &channels);
        if ev.adopted() {
            self.revs = revs_of(&self.working);
            self.scene_rev = o.scene_rev;
            self.playing = o.incoming().0;
            self.cue = None;
        }
        self.loops += ev.boundary() as u32;
        self.edge = to;
        sent.then(&ev)
    }

    /// Each ring's off still to send, a held ORBIT voice's: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pending(&self) -> [Option<super::engine::PendingOff>; RINGS] {
        self.state.pending()
    }

    pub fn view(&self) -> OrbitView {
        let mut v = OrbitView::of(&self.working, self.edge);
        v.running = self.transport;
        v.epoch = self.epoch;
        v.source = self.source;
        v.bpm = match self.source {
            ClockSource::Internal => self.clock.bpm(),
            ClockSource::Midi => self.midi.bpm(),
        };
        v.loops = self.loops;
        v.captured = self.take.done;
        v.playing = self.playing;
        v.scene_rev = self.scene_rev;
        v.queued = self.cue;
        v.seg = self.state.seg();
        v.chord = self.state.harmony(&self.working).chord;
        for ((r, f), (rhythm, notes)) in v.rings.iter_mut().zip(self.fired).zip(self.revs) {
            r.fired = f;
            (r.rhythm_rev, r.notes_rev) = (rhythm, notes);
        }
        v
    }
}
