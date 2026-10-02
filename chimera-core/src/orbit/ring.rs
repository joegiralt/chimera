//! A ring: one Euclidean voice and the pure function that plays it.

use super::MAX_STEPS;
use super::euclid::{Fill, HitMask, accents};
use super::mutate::Mutation;
use super::rng::{Pct, Rng};
use super::walk::{Pool, Walk, WalkState, walk_next};
use crate::Velocity;
use chimera_theory::{Chord, Key, Note, snap};

/// OCTAVE, 0..=8: octave n starts at C of octave n, MIDI 12·(n + 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Octave(u8);

impl Octave {
    pub const fn new(o: u8) -> Option<Octave> {
        if o <= 8 { Some(Octave(o)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Its C.
    pub const fn low(self) -> Note {
        match Note::new(12 * (self.0 + 1)) {
            Some(n) => n,
            None => Note::MAX,
        }
    }
}

/// RANGE: extra octaves for the pool walk and for mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Range {
    Zero,
    One,
    Two,
    Three,
}

impl Range {
    pub const ALL: [Range; 4] = [Range::Zero, Range::One, Range::Two, Range::Three];

    pub const fn octaves(self) -> u8 {
        self as u8
    }
}

/// Which set a ring's notes snap to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapTo {
    Scale,
    Chord,
}

impl SnapTo {
    pub const ALL: [SnapTo; 2] = [SnapTo::Scale, SnapTo::Chord];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteMode {
    /// One note per step.
    Step,
    /// The pool, walked on every hit.
    Pool,
}

impl NoteMode {
    pub const ALL: [NoteMode; 2] = [NoteMode::Step, NoteMode::Pool];
}

/// What the rings play against at this moment of the loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Harmony {
    pub key: Key,
    pub chord: Chord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ring {
    fill: Fill,
    /// The realised hits; MUTATE drifts it, a new fill re-lays it.
    hits_mask: HitMask,
    pub chance: Pct,
    pub mutate: Pct,
    pub mute: bool,
    pub gate: Pct,
    pub velocity: Velocity,
    pub accent: u8,
    pub octave: Octave,
    pub range: Range,
    pub snap: SnapTo,
    pub mode: NoteMode,
    pub walk: Walk,
    /// STEP: one per step.
    pub notes: [Note; MAX_STEPS as usize],
    pub pool: Pool,
}

/// A sounded hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteOut {
    pub note: Note,
    pub velocity: Velocity,
    /// Of one step.
    pub gate_fraction: Pct,
    pub accent: bool,
}

/// Added to VEL on accented hits, capped at 127 (the VOICE mockup).
pub const ACCENT_BOOST: u8 = 30;

impl Ring {
    /// `fill` laid out; the rest as the init scene's defaults.
    pub const fn new(fill: Fill, octave: Octave) -> Ring {
        let c = octave.low();
        Ring {
            fill,
            hits_mask: fill.mask(),
            chance: Pct::FULL,
            mutate: Pct::ZERO,
            mute: false,
            gate: Pct::clamped(50),
            velocity: Velocity::DEFAULT,
            accent: 0,
            octave,
            range: Range::Zero,
            snap: SnapTo::Scale,
            mode: NoteMode::Pool,
            walk: Walk::Up,
            notes: [c; MAX_STEPS as usize],
            pool: Pool::one(c),
        }
    }

    pub const fn fill(&self) -> Fill {
        self.fill
    }

    pub const fn hits_mask(&self) -> HitMask {
        self.hits_mask
    }

    /// A new STEPS, HITS or ROTATE: re-lays the hits, dropping MUTATE's
    /// nudges.
    pub const fn with_fill(mut self, fill: Fill) -> Ring {
        self.fill = fill;
        self.hits_mask = fill.mask();
        self
    }

    /// `m` as the hits, if it keeps STEPS and the hit count: a nudge.
    pub fn with_mask(mut self, m: HitMask) -> Option<Ring> {
        let cur = self.hits_mask;
        if m.steps() != cur.steps() || m.hits() != cur.hits() {
            return None;
        }
        self.hits_mask = m;
        Some(self)
    }

    /// Step `i` of the loop (wrapping each bar). A hit draws CHANCE and
    /// walks the pool whether or not it sounds, so a miss or a mute keeps
    /// the phase.
    pub fn play(
        &self,
        i: usize,
        walk: &mut WalkState,
        harmony: &Harmony,
        rng: &mut Rng,
    ) -> Option<NoteOut> {
        let mask = self.hits_mask;
        if !mask.is_hit(i) {
            return None;
        }
        let fired = rng.chance(self.chance);
        let raw = match self.mode {
            NoteMode::Step => self.notes[i % mask.steps().get() as usize],
            NoteMode::Pool => walk_next(self, walk, rng),
        };
        if !fired || self.mute {
            return None;
        }
        let note = match self.snap {
            SnapTo::Scale => snap(raw, chimera_theory::SnapTo::Scale(harmony.key)),
            SnapTo::Chord => snap(raw, chimera_theory::SnapTo::Chord(harmony.chord)),
        };
        let accent = accents(mask, self.accent).is_hit(i);
        let velocity = if accent {
            let v = self.velocity.get().saturating_add(ACCENT_BOOST).min(127);
            Velocity::new(v).unwrap_or(Velocity::MAX)
        } else {
            self.velocity
        };
        Some(NoteOut {
            note,
            velocity,
            gate_fraction: self.gate,
            accent,
        })
    }

    /// The ring after `m`.
    pub fn apply(mut self, m: Mutation) -> Ring {
        match m {
            Mutation::Swap { index, note } => match self.mode {
                NoteMode::Pool => self.pool = self.pool.with(index, note),
                NoteMode::Step => self.notes[index % MAX_STEPS as usize] = note,
            },
            Mutation::Nudge { from, to } => {
                if let Some(h) = self.hits_mask.moved(from, to) {
                    self.hits_mask = h;
                }
            }
        }
        self
    }
}
