//! `OrbitRunner`: ORBIT's audio-thread shell. It owns the clock, the
//! transport and the working copy of the playing scene, and each block
//! turns them into note events for the `Instrument` (spec § 4).

use super::chords::harmony;
use super::clock::{Bpm, InternalClock, Position, step_at};
use super::engine::{OrbitKind, OrbitState, Span, block};
use super::euclid::HitMask;
use super::scene::Scene;
use super::walk::Pool;
use super::{MAX_STEPS, RINGS};
use crate::MidiNote;
use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::{AudioShared, Instrument, NoteOrigin};
use crate::note_queue::{NoteEvent, NoteKind};
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
}

impl Default for OrbitShared {
    fn default() -> Self {
        OrbitShared {
            run: false,
            bpm: Bpm::DEFAULT,
            scene_rev: 0,
            settings: Scene::init(),
        }
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

#[derive(Clone, Copy, Debug)]
pub struct OrbitView {
    pub running: bool,
    pub bpm: Bpm,
    pub pos: Position,
    pub beats: u32,
    pub loops: u32,
    pub cued: bool,
    pub seg: u8,
    pub chord: Chord,
    pub rings: [RingView; RINGS],
}

/// Beats in a bar (4/4, spec § 6.2) and the shift taking a phase to a beat.
const BEATS_PER_BAR: u32 = 4;
const BEAT_SHIFT: u32 = 32 - BEATS_PER_BAR.trailing_zeros();

impl OrbitView {
    /// The view of `scene` stopped at `pos`.
    fn of(scene: &Scene, pos: Position) -> OrbitView {
        OrbitView {
            running: false,
            bpm: Bpm::DEFAULT,
            pos,
            beats: pos
                .bar
                .wrapping_mul(BEATS_PER_BAR)
                .wrapping_add(pos.phase.get() >> BEAT_SHIFT),
            loops: 0,
            cued: false,
            seg: 0,
            chord: harmony(&scene.chords, 0).chord,
            rings: core::array::from_fn(|i| {
                let r = &scene.rings[i].ring;
                RingView {
                    step: step_at(pos.phase, r.fill().steps()),
                    fired: 0,
                    hits: r.hits_mask(),
                    pool: r.pool,
                    notes: r.notes,
                }
            }),
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

    fn send(
        inst: &mut Instrument,
        shared: &AudioShared,
        ev: impl Iterator<Item = super::engine::OrbitEvent>,
        fired: &mut [u8; RINGS],
    ) {
        for e in ev {
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

    /// Spec § 4 steps 2-5 for one block.
    pub fn block(&mut self, inst: &mut Instrument, shared: &AudioShared) {
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
            return;
        }

        let (up, down) = (o.run && !self.run_seen, !o.run && self.run_seen);
        self.run_seen = o.run;
        if up && !self.transport {
            self.adopt(shared);
            self.clock = InternalClock::start(t, o.bpm, self.sample_rate);
            self.edge = Position::ZERO;
            self.state = OrbitState::start(&self.working);
            self.transport = true;
            self.loops = 0;
        } else if down && self.transport {
            let off = self.state.stop();
            Self::send(inst, shared, off.iter(), &mut self.fired);
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
        if !self.transport {
            return;
        }

        let from_sample = t.saturating_sub(32).max(self.clock.anchor());
        if o.bpm != self.clock.bpm() {
            self.clock.set_bpm(from_sample, self.edge, o.bpm);
        }
        let to = self.clock.position(t + 32);
        let span = Span {
            from: self.edge,
            to,
            from_sample,
            to_sample: t + 32,
            bar_len: self.clock.bar_len(),
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
        Self::send(inst, shared, ev.iter(), &mut self.fired);
    }

    pub fn view(&self) -> OrbitView {
        let mut v = OrbitView::of(&self.working, self.edge);
        v.running = self.transport;
        v.bpm = self.clock.bpm();
        v.loops = self.loops;
        v.cued = self.cue_pending;
        v.seg = self.state.seg();
        v.chord = self.state.harmony(&self.working).chord;
        for (r, f) in v.rings.iter_mut().zip(self.fired) {
            r.fired = f;
        }
        v
    }
}
