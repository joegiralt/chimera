//! `OrbitRunner`: ORBIT's audio-thread shell. It owns the clock, the
//! transport and the working copy of the playing scene, and each block
//! turns them into note events for the `Instrument` (spec § 4).

use super::channel::{Batch, Cmd, CmdLog, RingMask, SceneCmd, Seq, receive};
use super::chords::{Bars, harmony};
use super::clock::{
    Bpm, ClockSource, InternalClock, MidiClock, Position, SyncMode, WINDOW, advance_cap, step_at,
};
use super::engine::{Events, OrbitKind, OrbitState, Span, block};
use super::euclid::HitMask;
use super::ring::{Harmony, NoteMode, Ring};
use super::scene::{Scene, SceneIdx, SceneRing};
use super::walk::Pool;
use super::{MAX_POOL, MAX_STEPS, RINGS, RingIdx};
use crate::MidiNote;
use crate::dsp::rev::sync::Tempo;
use crate::hw::{BLOCK_SIZE, Cost, MAX_PARTS};
use crate::instrument::{AudioShared, Instrument, NoteOrigin, NoteSet};
use crate::note_queue::{NoteEvent, NoteKind, RealTime};
use crate::source_table::GateView;
use crate::triple::TripleBuffer;
use chimera_theory::{Chord, Note};
use core::mem::size_of;
use core::num::NonZeroU32;

/// What the UI asks of ORBIT, published with the Parts (ADR 0021): its
/// levels, and its commands in one log (ADR 0082).
#[derive(Clone, Debug)]
pub struct OrbitShared {
    pub bpm: Bpm,
    /// MIDI CONFIG's SYNC, from SYSTEM (ADR 0080).
    pub sync: SyncMode,
    /// `live`: the knobs, the fields an edit names, and what `Keep` plays.
    pub settings: Scene,
    /// The queued slot, else the playing one: what a `Cue` lands.
    pub next: Scene,
    /// The UI's commands, each applied once.
    pub log: CmdLog,
    /// The seq a project load took: every command up to it is void.
    pub load: Seq,
    /// The UI has landed its newest scene command: `settings` is the
    /// base of what plays.
    pub synced: bool,
}

impl OrbitShared {
    /// `c` into the log as the UI's flush puts it; its seq.
    pub fn push(&mut self, c: Cmd) -> Seq {
        self.log.push(c.into())
    }
}

impl Default for OrbitShared {
    fn default() -> Self {
        OrbitShared {
            bpm: Bpm::DEFAULT,
            sync: SyncMode::DEFAULT,
            settings: Scene::init(),
            next: Scene::init(),
            log: CmdLog::EMPTY,
            load: Seq::ZERO,
            synced: true,
        }
    }
}

/// A project load as the shells publish one, for rigs with no UI: a new
/// epoch, and the seq it voids every command up to.
#[cfg(any(test, feature = "test-support"))]
pub fn fake_load(shared: &mut AudioShared) {
    shared.epoch = shared.epoch.wrapping_add(1);
    shared.orbit.load = shared.orbit.log.reserve();
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
    pub count: u8,
    pub target: Capture,
    /// The scene playing when the take ended: the one it goes into, as a
    /// recording goes into the pattern it was made in (ADR 0083).
    pub scene: SceneIdx,
    /// In the order played, each once (a STEP ring's may repeat).
    pub notes: [Note; MAX_POOL],
    pub len: u8,
}

impl Captured {
    const NONE: Captured = Captured {
        count: 0,
        target: Capture::Off,
        scene: SceneIdx::ALL[0],
        notes: [Note::MIN; MAX_POOL],
        len: 0,
    };

    pub fn notes(&self) -> &[Note] {
        &self.notes[..self.len as usize]
    }
}

/// A capture under way: the notes since the first key-down.
#[derive(Clone, Copy, Debug)]
struct Take {
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
            armed: Capture::Off,
            held: NoteSet::EMPTY,
            notes: [Note::MIN; MAX_POOL],
            len: 0,
            done: Captured::NONE,
        }
    }

    /// An `Arm` or `Disarm` (ADR 0082), or a load's disarm (ADR 0075):
    /// what was under way is dropped, so keys held through it end nothing.
    fn set(&mut self, to: Capture) {
        self.armed = to;
        self.restart();
    }

    fn restart(&mut self) {
        (self.held, self.len) = (NoteSet::EMPTY, 0);
    }

    fn played(&self) -> &[Note] {
        &self.notes[..self.len as usize]
    }

    /// The notes taken, into `scene`: a segment's only if they make a
    /// chord, else it stays armed for the next.
    fn finish(&mut self, scene: SceneIdx) {
        if matches!(self.armed, Capture::Segment(_)) && Chord::from_notes(self.played()).is_none() {
            return self.restart();
        }
        self.done = Captured {
            count: self.done.count.wrapping_add(1),
            target: self.armed,
            scene,
            notes: self.notes,
            len: self.len,
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
}

impl RingView {
    /// `r` at `pos`, as the view reports it. Out of line: the default
    /// view and every publish share it.
    #[inline(never)]
    fn of(r: &Ring, pos: Position, fired: u8) -> RingView {
        RingView {
            step: step_at(pos.phase, r.fill().steps()),
            fired,
            hits: r.hits_mask(),
            pool: r.pool,
            notes: r.notes,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OrbitView {
    pub running: bool,
    /// The project load the runner is on (`AudioShared.epoch`): the UI
    /// takes no view from before its own (ADR 0082's load gate).
    pub epoch: u32,
    /// The newest command applied.
    pub acked: Seq,
    pub source: ClockSource,
    /// The internal BPM, or the MIDI clock's measured one.
    pub bpm: Bpm,
    pub pos: Position,
    /// The playing scene's loop, so a page can say where in it `pos` is.
    pub bars: Bars,
    pub beats: u32,
    pub loops: u32,
    /// The scene adopted last.
    pub playing: SceneIdx,
    /// A scene waiting on the loop boundary: with every command applied,
    /// `None` means the newest scene command is adopted.
    pub cue: Option<SceneIdx>,
    /// What the keys play into: the newest arm command's target, `Off`
    /// once its capture is taken.
    pub armed: Capture,
    pub seg: u8,
    pub chord: Chord,
    pub rings: [RingView; RINGS],
    pub captured: Captured,
}

/// Beats in a bar (4/4, spec § 6.2) and the shift taking a phase to a beat.
pub const BEATS_PER_BAR: u32 = 4;
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
            acked: Seq::ZERO,
            source: ClockSource::Internal,
            bpm: Bpm::DEFAULT,
            pos,
            bars: scene.chords.bars,
            beats: beats_at(pos),
            loops: 0,
            playing: SceneIdx::ALL[0],
            cue: None,
            armed: Capture::Off,
            seg: 0,
            chord: harmony(&scene.chords, 0).chord,
            rings: core::array::from_fn(|i| RingView::of(&scene.rings[i].ring, pos, 0)),
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

/// A scene waiting on the boundary, as slot n: the slot itself (`next`),
/// or `live` (`settings`), when the UI turned back from a switch it never
/// saw adopted.
#[derive(Clone, Copy, Debug)]
enum Incoming {
    Slot(SceneIdx),
    Live(SceneIdx),
}

impl Incoming {
    fn scene(self) -> SceneIdx {
        match self {
            Incoming::Slot(n) | Incoming::Live(n) => n,
        }
    }

    fn of(self, o: &OrbitShared) -> &Scene {
        match self {
            Incoming::Slot(_) => &o.next,
            Incoming::Live(_) => &o.settings,
        }
    }
}

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
    playing: SceneIdx,
    /// The newest command applied.
    applied: Seq,
    /// `working`'s base is the UI's `live`: knobs and edits copy from
    /// `settings`. The UI's `synced` sets it; only a slot's adoption
    /// clears it (`live`'s keeps it).
    base_landed: bool,
    /// Whether ORBIT plays: B6 toggles it, the DAW's Start and Continue
    /// set it and its Stop clears it (ADR 0080).
    transport: bool,
    /// A scene waiting on the boundary.
    cue: Option<Incoming>,
    epoch: u32,
    /// The sample at the start of the next block.
    sample: u64,
    sample_rate: NonZeroU32,
    loops: u32,
    fired: [u8; RINGS],
    take: Take,
    /// The harmony last handed to the `Instrument`: a change re-snaps.
    harmony: Option<Harmony>,
    /// The external tempo the reverb follows, a beat in samples, with the
    /// BPM set when it was taken (`follow`).
    ext: Option<(u32, Bpm)>,
}

const _: () = assert!(size_of::<OrbitRunner>() <= 1024);

impl OrbitRunner {
    /// Reserved from the voice budget while ORBIT plays, as `FxBus::COST`
    /// is (ADR 0091): its own worst block, the clock, rings, MUTATE,
    /// harmony, re-snap and view, without the notes it hands the
    /// `Instrument` (`Instrument::BURST` bills those, as anyone's), over
    /// the block's samples. Re-measured with `Instrument::BURST`: their sum
    /// must cover ORBIT's cold first block (65,387 at 785a028a).
    pub const COST: Cost = Cost(171); // worst block less its notes' handling, 10,913, the worst over five runs (STRESS keys first), measured 2026-10-04/05, bench ORBIT PHASES, rev V at 480 MHz, 06dd8707 to 785a028a; / 64, rounded up

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
            working,
            playing: SceneIdx::ALL[0],
            applied: Seq::ZERO,
            base_landed: true,
            transport: false,
            cue: None,
            epoch: 0,
            sample: 0,
            sample_rate,
            loops: 0,
            fired: [0; RINGS],
            take: Take::new(),
            harmony: None,
            ext: None,
        }
    }

    /// `inc`, whole, as the playing scene.
    fn adopt(&mut self, inc: Incoming, o: &OrbitShared) {
        self.working = *inc.of(o);
        self.playing = inc.scene();
        self.cue = None;
        self.base_landed = Self::lands_live(inc);
    }

    /// Whether `working`'s base is the UI's `live` once `inc` is adopted:
    /// `live` itself, yes, so its edits keep coming; a slot, not until the
    /// UI says it has landed it.
    fn lands_live(inc: Incoming) -> bool {
        matches!(inc, Incoming::Live(_))
    }

    /// The UI's commands since the last block (ADR 0082): a scene command
    /// cues, or cancels a cue; the edit masks go to the knob copy.
    fn commands(&mut self, o: &OrbitShared) -> Batch {
        let b = receive(&o.log, &mut self.applied);
        match b.scene {
            Some(SceneCmd::Cue(n)) => self.cue = Some(Incoming::Slot(n)),
            // In sync, `live` already plays as slot n: nothing to land.
            Some(SceneCmd::Keep(n)) if self.base_landed => {
                self.cue = None;
                self.playing = n;
            }
            Some(SceneCmd::Keep(n)) => self.cue = Some(Incoming::Live(n)),
            None => {}
        }
        b
    }

    /// The knobs from `settings`; a ring's hits, notes and pool only where
    /// an edit names them, so MUTATE's drift stands otherwise.
    fn copy_knobs(&mut self, settings: &Scene, rhythm: RingMask, notes: RingMask) {
        let w = &mut self.working;
        w.chords = settings.chords;
        w.seed = settings.seed;
        for (r, (w, s)) in RingIdx::ALL
            .into_iter()
            .zip(w.rings.iter_mut().zip(&settings.rings))
        {
            // A new field fails to compile here until it is copied or left.
            let SceneRing {
                ring,
                target,
                touched: _,
            } = s;
            w.target = *target;
            w.ring
                .take_knobs(ring, rhythm.contains(r), notes.contains(r));
        }
    }

    fn send(
        inst: &mut Instrument,
        (shared, src): (&AudioShared, &GateView<'_>),
        ev: &Events,
        fired: &mut [u8; RINGS],
    ) {
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
            #[cfg(feature = "probe")]
            let t0 = crate::probe::now();
            inst.handle_from_with(ev, shared, NoteOrigin::Orbit(e.ring), src);
            #[cfg(feature = "probe")]
            crate::probe::max(6, crate::probe::now().wrapping_sub(t0));
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
        let led = self.midi.leading(now);
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
        let playing = self.playing;
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
                    t.finish(playing);
                }
            }
            NoteKind::Off => {
                if !t.held.contains(ev.note) {
                    return;
                }
                t.held = t.held.without(ev.note);
                if t.held.is_empty() && step.is_none() {
                    t.finish(playing);
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn block(&mut self, inst: &mut Instrument, shared: &AudioShared) -> Events {
        self.block_with(inst, shared, &GateView::empty())
    }

    /// `block`, TAPE's note-ons reading through the block's `src`: what
    /// the shells run.
    pub fn block_with(
        &mut self,
        inst: &mut Instrument,
        shared: &AudioShared,
        src: &GateView<'_>,
    ) -> Events {
        #[cfg(feature = "probe")]
        crate::probe::begin();
        let sent = self.advance(shared);
        // Before the notes: while ORBIT plays, its reserve judges them too
        // (ADR 0091).
        let h = self.transport.then(|| self.state.harmony(&self.working));
        inst.set_harmony(h);
        #[cfg(feature = "probe")]
        crate::probe::stamp(7);
        Self::send(inst, (shared, src), &sent, &mut self.fired);
        #[cfg(feature = "probe")]
        crate::probe::stamp(8);
        if h.is_some() && h != self.harmony {
            inst.resnap(shared);
        }
        self.harmony = h;
        #[cfg(feature = "probe")]
        crate::probe::stamp(9);
        // After the last stamp: no phase bills it.
        self.follow(shared.orbit.bpm);
        inst.set_tempo(self.tempo());
        sent
    }

    /// Which tempo the reverb follows. Under EXT, the leader's held beat
    /// whenever it is heard and settled, playing or stopped (a DAW's Stop
    /// with its Clock running keeps it). Through a dropout or a fresh
    /// clock's first beat the last external tempo holds, as an Elektron
    /// box keeps the last one it heard, until ORBIT plays its own clock,
    /// the BPM is edited, or SYNC goes INT.
    fn follow(&mut self, bpm: Bpm) {
        let own = self.transport && self.source == ClockSource::Internal;
        if self.sync == SyncMode::Int || own {
            self.ext = None;
        } else if let Some(b) = self.midi.beat().filter(|_| self.midi.heard(self.sample)) {
            self.ext = Some((b, bpm));
        } else if self.ext.is_some_and(|(_, at)| at != bpm) {
            self.ext = None;
        }
    }

    /// The tempo the reverb takes this block: the external beat `follow`
    /// holds, else the internal BPM.
    pub fn tempo(&self) -> Tempo {
        match self.ext {
            Some((beat, _)) => Tempo::from_beat(beat, self.sample_rate.get()),
            None => Tempo::from_bpm(self.clock.bpm().get() as f32),
        }
    }

    /// A project load, at `epoch`: it killed every voice, so nothing to
    /// release, and no edge. It is the player's newer act, so the DAW's
    /// pending Start, Continue or Stop is dropped: its next Start plays
    /// the load. Cold: out of `advance`.
    #[inline(never)]
    fn load(&mut self, epoch: u32, o: &OrbitShared) {
        self.epoch = epoch;
        (self.ext_start, self.ext_play) = (false, None);
        self.state.forget();
        self.take.set(Capture::Off);
        // A capture ended on the old project is taken into nothing.
        (self.take.done.target, self.take.done.len) = (Capture::Off, 0);
        self.transport = false;
        // The loaded `live`, whole, and every command up to the load void.
        self.working = o.settings;
        self.cue = None;
        self.base_landed = true;
        self.applied = o.load;
    }

    /// One block's transport, clock and steps.
    fn advance(&mut self, shared: &AudioShared) -> Events {
        let o = &shared.orbit;
        let t = self.sample;
        self.sample += BLOCK_SIZE as u64;

        if shared.epoch != self.epoch {
            self.load(shared.epoch, o);
        }
        // After a load, the commands past it, applied as any others (spec
        // § 3): its `Keep` names the playing slot, and a B6 pressed after
        // it plays the loaded project.
        let edits = self.commands(o);
        if let Some(to) = edits.arm {
            self.take.set(to);
        }
        if o.synced {
            self.base_landed = true;
        }

        if o.sync != self.sync {
            // Either way the other mode's clock is forgotten: INT never
            // hears it, and EXT takes the next one up from the edge.
            self.sync = o.sync;
            self.midi = MidiClock::new(self.sample_rate);
            (self.ext_start, self.ext_play) = (false, None);
        }
        #[cfg(feature = "probe")]
        crate::probe::stamp(1);

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
        // An odd number of `TogglePlay`s since the last block flips it.
        let pressed = edits.flip;
        // B6 toggles the transport as it was before the DAW's word, so B6
        // and a Stop in one block both stop.
        let play = match (pressed, daw) {
            (true, _) => !was,
            (false, Some(p)) => p,
            (false, None) => was,
        };
        if play && !self.transport {
            // The waiting scene, else `live`; out of sync with nothing
            // waiting, what plays already (the UI is landing it).
            let base = self.base_landed.then_some(Incoming::Live(self.playing));
            if let Some(inc) = self.cue.or(base) {
                self.adopt(inc, o);
            }
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

        if let Some(inc) = self.cue
            && !self.transport
        {
            self.adopt(inc, o);
        }
        // Only onto the UI's `live`: once a scene is adopted, `settings`
        // is another scene's until the UI lands it.
        if self.base_landed {
            self.copy_knobs(&o.settings, edits.rhythm, edits.notes);
        }
        #[cfg(feature = "probe")]
        crate::probe::stamp(2);

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
        let cue = self.cue.map(|c| c.of(o));
        #[cfg(feature = "probe")]
        crate::probe::stamp(3);
        let ev = block(&mut self.state, &mut self.working, cue, span, &channels);
        if let Some(inc) = self.cue
            && ev.adopted()
        {
            self.playing = inc.scene();
            self.cue = None;
            self.base_landed = Self::lands_live(inc);
        }
        self.loops += ev.boundary() as u32;
        self.edge = to;
        sent.then(&ev)
    }

    /// The scene as it plays, drift and all: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn working(&self) -> &Scene {
        &self.working
    }

    /// Each ring's off still to send, a held ORBIT voice's: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pending(&self) -> [Option<super::engine::PendingOff>; RINGS] {
        self.state.pending()
    }

    /// The view, built.
    pub fn view(&self) -> OrbitView {
        let mut v = OrbitView::default();
        self.fill_view(&mut v);
        v
    }

    /// The view, written into `v` in place: every field, every time, as
    /// a triple buffer's slot comes back two publishes stale. The
    /// destructure fails to compile when `OrbitView` gains a field.
    pub fn fill_view(&self, v: &mut OrbitView) {
        let OrbitView {
            running,
            epoch,
            acked,
            source,
            bpm,
            pos,
            bars,
            beats,
            loops,
            playing,
            cue,
            armed,
            seg,
            chord,
            rings,
            captured,
        } = v;
        let w = &self.working;
        *running = self.transport;
        *epoch = self.epoch;
        *acked = self.applied;
        *source = self.source;
        *bpm = match self.source {
            ClockSource::Internal => self.clock.bpm(),
            ClockSource::Midi => self.midi.bpm(),
        };
        *pos = self.edge;
        *bars = w.chords.bars;
        *beats = beats_at(self.edge);
        *loops = self.loops;
        *playing = self.playing;
        *cue = self.cue.map(Incoming::scene);
        *armed = self.take.armed;
        *seg = self.state.seg();
        // Running, the block's; stopped, built here.
        *chord = self.harmony.unwrap_or_else(|| self.state.harmony(w)).chord;
        for ((rv, s), &fired) in rings.iter_mut().zip(&w.rings).zip(&self.fired) {
            *rv = RingView::of(&s.ring, self.edge, fired);
        }
        *captured = self.take.done;
    }
}
