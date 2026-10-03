//! `block`: one audio block's worth of ORBIT, as a pure function of the
//! state, the scene and the window it is handed.

use super::chords::{harmony, segment_at};
use super::clock::{Phase, Position, step_at, step_start};
use super::mutate::{Mutation, mutate};
use super::ring::Harmony;
use super::rng::Rng;
use super::scene::{Scene, Target};
use super::walk::WalkState;
use super::{MAX_STEPS, RINGS, RingIdx};
use crate::Velocity;
use crate::hw::MAX_PARTS;
use chimera_hal::MidiChannel;
use chimera_theory::Note;

/// An off for a note already held, and the next ring step's on.
pub const EVENTS_PER_RING: usize = 2;

/// A note-off for later, with the channel its On went out on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingOff {
    pub channel: MidiChannel,
    pub note: Note,
    pub at: u64,
}

/// A gate is at least this many samples: a floor on note length, one or two
/// blocks once quantised to the block grid.
const MIN_GATE: u64 = 96;

#[derive(Clone, Copy, Debug)]
pub struct OrbitState {
    walks: [WalkState; RINGS],
    rngs: [Rng; RINGS],
    pending: [Option<PendingOff>; RINGS],
    /// No MUTATE at this loop's boundary: the loop is new.
    fresh: bool,
    seg: u8,
}

impl OrbitState {
    pub fn start(scene: &Scene) -> OrbitState {
        let mut st = OrbitState {
            walks: [WalkState::START; RINGS],
            rngs: [Rng::new(0); RINGS],
            pending: [None; RINGS],
            fresh: true,
            seg: 0,
        };
        st.restart_dice(scene);
        st
    }

    /// Walks and dice from the seed again, nothing else: a B6 start under
    /// a leading MIDI clock.
    pub fn restart_dice(&mut self, scene: &Scene) {
        self.walks = [WalkState::START; RINGS];
        self.rngs = RingIdx::ALL.map(|r| Rng::for_ring(scene.seed, r));
    }

    /// Every pending off, now, and nothing else.
    pub fn stop(&mut self) -> Events {
        let mut ev = Events::new();
        for r in RingIdx::ALL {
            if let Some(p) = self.pending[r.get() as usize].take() {
                ev.push(r, p.channel, p.note, OrbitKind::Off);
            }
        }
        ev
    }

    /// The voices were killed: their offs have nothing to end.
    pub fn forget(&mut self) {
        self.pending = [None; RINGS];
    }

    pub fn harmony(&self, scene: &Scene) -> Harmony {
        harmony(&scene.chords, self.seg as usize)
    }

    pub fn seg(&self) -> u8 {
        self.seg
    }

    /// Each ring's off still to send: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pending(&self) -> [Option<PendingOff>; RINGS] {
        self.pending
    }
}

/// A block's window on the song and on the sample clock.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub from: Position,
    pub to: Position,
    pub from_sample: u64,
    pub to_sample: u64,
    pub bar_len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitKind {
    On(Velocity),
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitEvent {
    pub ring: RingIdx,
    pub channel: MidiChannel,
    pub note: Note,
    pub kind: OrbitKind,
}

#[derive(Clone, Copy, Debug)]
pub struct Events {
    ev: [Option<OrbitEvent>; EVENTS_PER_RING * RINGS],
    n: usize,
    boundary: bool,
    adopted: bool,
    seg_changed: bool,
}

impl Events {
    pub(super) fn new() -> Events {
        Events {
            ev: [None; EVENTS_PER_RING * RINGS],
            n: 0,
            boundary: false,
            adopted: false,
            seg_changed: false,
        }
    }

    fn push(&mut self, ring: RingIdx, channel: MidiChannel, note: Note, kind: OrbitKind) {
        // A ring sounds at most one step per block, so this never fills.
        if let Some(slot) = self.ev.get_mut(self.n) {
            *slot = Some(OrbitEvent {
                ring,
                channel,
                note,
                kind,
            });
            self.n += 1;
        }
    }

    /// These, then `later`'s, with `later`'s flags: a block's STOP offs
    /// before its window. A ring's off is in one or the other, so a ring
    /// still sends at most `EVENTS_PER_RING`.
    pub(super) fn then(mut self, later: &Events) -> Events {
        for e in later.ev[..later.n].iter().flatten() {
            self.push(e.ring, e.channel, e.note, e.kind);
        }
        Events {
            boundary: later.boundary,
            adopted: later.adopted,
            seg_changed: later.seg_changed,
            ..self
        }
    }

    /// Offs first, then Ons; each in ring order.
    pub fn iter(&self) -> impl Iterator<Item = OrbitEvent> + '_ {
        let all = || self.ev[..self.n].iter().flatten().copied();
        all()
            .filter(|e| e.kind == OrbitKind::Off)
            .chain(all().filter(|e| e.kind != OrbitKind::Off))
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The window held a loop boundary.
    pub fn boundary(&self) -> bool {
        self.boundary
    }

    /// A cued scene was copied in at it.
    pub fn adopted(&self) -> bool {
        self.adopted
    }

    pub fn seg_changed(&self) -> bool {
        self.seg_changed
    }
}

/// Phase units from `base` to `p`.
fn units(p: Position, base: Position) -> u64 {
    (((p.bar - base.bar) as u64) << 32)
        .wrapping_add(p.phase.get() as u64)
        .wrapping_sub(base.phase.get() as u64)
}

fn to_ge(span: Span) -> bool {
    span.to >= span.from
}

/// Everything ORBIT does for the window `span`: offs due, the loop
/// boundary, then each ring's step. Steps land by linear interpolation of
/// the window, which holds under an internal or a leading external clock.
///
/// A window is shorter than one step of any ring (clock.rs's const assert
/// and the runner's advance cap). So a ring plays at most one step, the
/// events never overflow, and no step before the loop boundary shares the
/// window with it.
pub fn block(
    st: &mut OrbitState,
    scene: &mut Scene,
    cue: Option<&Scene>,
    span: Span,
    channels: &[MidiChannel; MAX_PARTS],
) -> Events {
    debug_assert!(to_ge(span) && units(span.to, span.from) < (1 << 32) / MAX_STEPS as u64);
    let mut ev = Events::new();
    let Span { from, to, .. } = span;

    for r in RingIdx::ALL {
        let slot = &mut st.pending[r.get() as usize];
        if let Some(p) = slot.filter(|p| p.at < span.to_sample) {
            *slot = None;
            ev.push(r, p.channel, p.note, OrbitKind::Off);
        }
    }

    let bars = scene.chords.bars.get() as u32;
    let line = |b: u32| Position {
        bar: b,
        phase: Phase::ZERO,
    };
    if (from.bar..=to.bar).any(|b| b % bars == 0 && line(b) >= from && line(b) < to) {
        ev.boundary = true;
        if let Some(c) = cue {
            *scene = *c;
            st.restart_dice(scene);
            ev.adopted = true;
        } else if !st.fresh {
            let h = harmony(&scene.chords, 0);
            for (i, sr) in scene.rings.iter_mut().enumerate() {
                if let Some(m) = mutate(&sr.ring, &h, &mut st.rngs[i]) {
                    match m {
                        Mutation::Swap { .. } => sr.notes_rev = sr.notes_rev.wrapping_add(1),
                        Mutation::Nudge { .. } => sr.rhythm_rev = sr.rhythm_rev.wrapping_add(1),
                    }
                    sr.ring = sr.ring.apply(m);
                }
            }
        }
        st.fresh = false;
    }

    let total = units(to, from).max(1);
    let elapsed = span.to_sample.saturating_sub(span.from_sample);
    // A window is under a step long, so it sees a segment or two: each
    // one's chord is built once, not per step.
    let mut held: Option<(usize, Harmony)> = None;
    for r in RingIdx::ALL {
        let i = r.get() as usize;
        let ring = &scene.rings[i].ring;
        let n = ring.fill().steps();
        let step_len = (span.bar_len / n.get() as u32) as u64;
        for bar in from.bar..=to.bar {
            let first = if bar == from.bar {
                let k = step_at(from.phase, n);
                k + (step_start(k, n) < from.phase) as u8
            } else {
                0
            };
            for k in first..n.get() {
                let at = Position {
                    bar,
                    phase: step_start(k, n),
                };
                if at >= to {
                    break;
                }
                let seg = segment_at(bar, at.phase, scene.chords.bars, scene.chords.segs);
                let h = match held {
                    Some((s, h)) if s == seg => h,
                    _ => held.insert((seg, harmony(&scene.chords, seg))).1,
                };
                let Some(out) = ring.play(k as usize, &mut st.walks[i], &h, &mut st.rngs[i]) else {
                    continue;
                };
                if let Some(p) = st.pending[i].take() {
                    ev.push(r, p.channel, p.note, OrbitKind::Off);
                }
                let Target::Part(part) = scene.rings[i].target;
                let channel = channels[part.index()];
                let sample = span.from_sample + units(at, from) * elapsed / total;
                let gate = step_len * out.gate_fraction.get() as u64 / 100;
                st.pending[i] = Some(PendingOff {
                    channel,
                    note: out.note,
                    at: (sample + gate).max(sample + MIN_GATE),
                });
                ev.push(r, channel, out.note, OrbitKind::On(out.velocity));
            }
        }
    }

    let seg = segment_at(to.bar, to.phase, scene.chords.bars, scene.chords.segs) as u8;
    ev.seg_changed = seg != st.seg;
    st.seg = seg;
    ev
}
