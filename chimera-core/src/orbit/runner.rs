//! `OrbitRunner`: ORBIT's audio-thread shell. It owns the clock, the
//! transport and the working copy of the playing scene, and each block
//! turns them into note events for the `Instrument` (spec § 4).

use super::chords::harmony;
use super::clock::{
    Bpm, ClockSource, InternalClock, MidiClock, Position, WINDOW, advance_cap, step_at,
};
use super::engine::{Events, OrbitKind, OrbitState, PendingOff, Span, block};
use super::euclid::HitMask;
use super::ring::NoteMode;
use super::scene::Scene;
use super::walk::Pool;
use super::{MAX_POOL, MAX_STEPS, RINGS, RingIdx};
use crate::MidiNote;
use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::{AudioShared, Instrument, NoteOrigin};
use crate::note_queue::{NoteEvent, NoteKind, RealTime};
use crate::triple::TripleBuffer;
use chimera_theory::{Chord, Note};
use core::mem::size_of;
use core::num::NonZeroU32;

/// What the UI asks of ORBIT, published with the Parts (ADR 0021).
#[derive(Clone, Debug)]
pub struct OrbitShared {
    pub run: bool,
    pub bpm: Bpm,
    /// Bumped when a different scene is queued; the runner adopts
    /// `settings` whole at the next loop boundary.
    pub scene_rev: u16,
    pub settings: Scene,
    /// What the keys play into (spec § 5.3).
    pub capture: Capture,
}

impl Default for OrbitShared {
    fn default() -> Self {
        OrbitShared {
            run: false,
            bpm: Bpm::DEFAULT,
            scene_rev: 0,
            settings: Scene::init(),
            capture: Capture::Off,
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
    /// In the order played, each once (a STEP ring's may repeat).
    pub notes: [Note; MAX_POOL],
    pub len: u8,
}

impl Captured {
    const NONE: Captured = Captured {
        count: 0,
        target: Capture::Off,
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
    /// The target as last published: arming is its change, so a slot
    /// still armed after a capture never takes a second.
    seen: Capture,
    /// What the keys play into; `Off` once taken, until re-armed.
    armed: Capture,
    /// Keys down since the first, by MIDI note: the last one up ends it.
    held: u128,
    notes: [Note; MAX_POOL],
    len: u8,
    done: Captured,
}

impl Take {
    const fn new() -> Take {
        Take {
            seen: Capture::Off,
            armed: Capture::Off,
            held: 0,
            notes: [Note::MIN; MAX_POOL],
            len: 0,
            done: Captured::NONE,
        }
    }

    /// The UI's target, as published: a change re-arms and starts over.
    fn arm(&mut self, to: Capture) {
        if to != self.seen {
            self.seen = to;
            self.armed = to;
            self.restart();
        }
    }

    fn restart(&mut self) {
        (self.held, self.len) = (0, 0);
    }

    fn played(&self) -> &[Note] {
        &self.notes[..self.len as usize]
    }

    /// The notes taken: a segment's only if they make a chord, else it
    /// stays armed for the next.
    fn finish(&mut self) {
        if matches!(self.armed, Capture::Segment(_)) && Chord::from_notes(self.played()).is_none() {
            return self.restart();
        }
        self.done = Captured {
            count: self.done.count.wrapping_add(1),
            target: self.armed,
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
    pub beats: u32,
    pub loops: u32,
    pub cued: bool,
    pub seg: u8,
    pub chord: Chord,
    pub rings: [RingView; RINGS],
    pub captured: Captured,
}

/// Beats in a bar (4/4, spec § 6.2) and the shift taking a phase to a beat.
const BEATS_PER_BAR: u32 = 4;
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
            beats: beats_at(pos),
            loops: 0,
            cued: false,
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
    /// The external transport: set by MIDI Start or Continue, a new
    /// leader's first Clock, or B6 with no clock leading; cleared by Stop.
    /// The fallback alone keeps it.
    ext_playing: bool,
    /// A MIDI Start to act on next block: offs, then bar 0.
    ext_start: bool,
    /// A MIDI Stop to act on next block: offs, and the transport down.
    ext_stop: bool,
    /// The previous window's end: the next one starts here.
    edge: Position,
    state: OrbitState,
    working: Scene,
    revs: [Revs; RINGS],
    scene_rev: u16,
    /// `run` as of the last block: transport acts on its edges only.
    run_seen: bool,
    transport: bool,
    cue_pending: bool,
    epoch: u32,
    /// The sample at the start of the next block.
    sample: u64,
    sample_rate: NonZeroU32,
    loops: u32,
    fired: [u8; RINGS],
    take: Take,
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
            ext_playing: true,
            ext_start: false,
            ext_stop: false,
            edge: Position::ZERO,
            state: OrbitState::start(&working),
            revs: revs_of(&working),
            working,
            scene_rev: 0,
            run_seen: false,
            transport: false,
            cue_pending: false,
            epoch: 0,
            sample: 0,
            sample_rate,
            loops: 0,
            fired: [0; RINGS],
            take: Take::new(),
        }
    }

    /// `settings` whole, as the playing scene.
    fn adopt(&mut self, shared: &AudioShared) {
        self.working = shared.orbit.settings;
        self.revs = revs_of(&self.working);
        self.scene_rev = shared.orbit.scene_rev;
        self.cue_pending = false;
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
    /// Only the leading clock's are taken. Start and Stop act at the next
    /// block, which sends their offs.
    pub fn realtime(&mut self, src: usize, rt: RealTime, now: u64) {
        // It came during the block before its drain: half a block, on average.
        let now = now.saturating_sub(WINDOW);
        let led = self.midi.position(now).is_some();
        match rt {
            // A clock taken up with no Start carries on from where ORBIT
            // is, and plays: its first Clock is a Continue.
            RealTime::Clock => {
                if self.midi.tick(src, now) && !led {
                    self.midi.rebase(self.edge);
                    self.ext_playing = true;
                }
            }
            RealTime::Start => {
                if self.midi.start(src, now) {
                    self.ext_playing = true;
                    self.ext_start = true;
                }
            }
            RealTime::Continue => self.ext_playing |= self.midi.resume(src, now),
            RealTime::Stop => {
                if self.midi.stop(src, now) {
                    self.ext_playing = false;
                    self.ext_stop = true;
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
        let t = &mut self.take;
        let Some(note) = Note::new(ev.note.get()) else {
            return;
        };
        let bit = 1u128 << note.get();
        match ev.kind {
            NoteKind::On(_) => {
                t.held |= bit;
                let fresh = step.is_some() || !t.played().contains(&note);
                if fresh && (t.len as usize) < MAX_POOL {
                    t.notes[t.len as usize] = note;
                    t.len += 1;
                }
                if step.is_some_and(|n| t.len >= n) {
                    t.finish();
                }
            }
            NoteKind::Off => {
                if t.held & bit == 0 {
                    return;
                }
                t.held &= !bit;
                if t.held == 0 && step.is_none() {
                    t.finish();
                }
            }
        }
    }

    /// Spec § 4 steps 2-5 for one block: what it sent the `Instrument`.
    /// The shells drop it; the tests read each block's events from it.
    pub fn block(&mut self, inst: &mut Instrument, shared: &AudioShared) -> Events {
        self.take.arm(shared.orbit.capture);
        let sent = self.advance(shared);
        Self::send(inst, shared, &sent, &mut self.fired);
        sent
    }

    /// One block's transport, clock and steps.
    fn advance(&mut self, shared: &AudioShared) -> Events {
        let o = &shared.orbit;
        let t = self.sample;
        self.sample += BLOCK_SIZE as u64;

        if shared.epoch != self.epoch {
            // A load killed every voice: nothing to release, and no edge.
            self.epoch = shared.epoch;
            self.state.forget();
            self.transport = false;
            self.run_seen = o.run;
            self.adopt(shared);
            return Events::new();
        }

        // A MIDI Start or Stop first ends what ORBIT holds. Stop drops the
        // transport with no edge (Decisions); Start goes back to bar 0.
        let mut sent = Events::new();
        if self.ext_start || self.ext_stop {
            sent = self.state.stop();
        }
        if core::mem::take(&mut self.ext_stop) {
            self.transport = false;
        }
        if core::mem::take(&mut self.ext_start) {
            self.edge = Position::ZERO;
            self.state = OrbitState::start(&self.working);
            self.loops = 0;
        }

        let midi = self.midi.position(t + WINDOW);
        let (up, down) = (o.run && !self.run_seen, !o.run && self.run_seen);
        self.run_seen = o.run;
        if up && !self.transport {
            self.adopt(shared);
            if midi.is_some() {
                // Joins the external clock in phase: the dice from the
                // seed, the position kept (§ 6.3).
                self.state.restart_dice(&self.working);
            } else {
                // B6 with no external clock: nothing outside to wait for.
                self.ext_playing = true;
                self.clock = InternalClock::start(t, o.bpm, self.sample_rate);
                self.edge = Position::ZERO;
                self.state = OrbitState::start(&self.working);
                self.loops = 0;
            }
            self.transport = true;
        } else if down && self.transport {
            sent = sent.then(&self.state.stop());
            self.transport = false;
        }

        if o.scene_rev != self.scene_rev {
            self.cue_pending = true;
        }
        if self.cue_pending && !self.transport {
            self.adopt(shared);
        }
        if !self.cue_pending {
            self.copy_knobs(&o.settings);
        }

        let sounding = self.transport && self.ext_playing;
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
        let cue = self.cue_pending.then_some(&o.settings);
        let ev = block(&mut self.state, &mut self.working, cue, span, &channels);
        if ev.adopted() {
            self.revs = revs_of(&self.working);
            self.scene_rev = o.scene_rev;
            self.cue_pending = false;
        }
        self.loops += ev.boundary() as u32;
        self.edge = to;
        sent.then(&ev)
    }

    /// Each ring's off still to send, a held ORBIT voice's: for the tests.
    pub fn pending(&self) -> [Option<PendingOff>; RINGS] {
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
        v.cued = self.cue_pending;
        v.seg = self.state.seg();
        v.chord = self.state.harmony(&self.working).chord;
        for ((r, f), (rhythm, notes)) in v.rings.iter_mut().zip(self.fired).zip(self.revs) {
            r.fired = f;
            (r.rhythm_rev, r.notes_rev) = (rhythm, notes);
        }
        v
    }
}
