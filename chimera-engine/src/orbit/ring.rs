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

    /// The octave `n` is in, held to 0..=8.
    pub const fn of(n: Note) -> Octave {
        let o = (n.get() / 12).saturating_sub(1);
        Octave(if o > 8 { 8 } else { o })
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

impl Harmony {
    /// `to`'s set at this moment: the key's scale or the chord's tones.
    pub const fn target(self, to: SnapTo) -> chimera_theory::SnapTo {
        match to {
            SnapTo::Scale => chimera_theory::SnapTo::Scale(self.key),
            SnapTo::Chord => chimera_theory::SnapTo::Chord(self.chord),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ring {
    fill: Fill,
    /// The realised hits; MUTATE drifts it, a new fill re-lays it.
    hits_mask: HitMask,
    /// ACCENT's fill over the hits, kept with them.
    accented: HitMask,
    pub chance: Pct,
    pub mutate: Pct,
    pub mute: bool,
    pub gate: Pct,
    pub velocity: Velocity,
    accent: u8,
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
            accented: accents(fill.mask(), 0),
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

    /// How many of the hits ACCENT spreads over them.
    pub const fn accent(&self) -> u8 {
        self.accent
    }

    pub const fn with_accent(mut self, accent: u8) -> Ring {
        self.accent = accent;
        self.with_hits(self.hits_mask)
    }

    /// The accents laid over `mask`: the ring's own, kept, when `mask` is
    /// its hits; else laid now (a runner's drifted hits).
    pub const fn accents_on(&self, mask: HitMask) -> HitMask {
        if mask.bits() == self.hits_mask.bits()
            && mask.steps().get() == self.hits_mask.steps().get()
        {
            self.accented
        } else {
            accents(mask, self.accent)
        }
    }

    /// `m` as the hits, the accents laid over them.
    const fn with_hits(mut self, m: HitMask) -> Ring {
        self.hits_mask = m;
        self.accented = accents(m, self.accent);
        self
    }

    /// A new STEPS, HITS or ROTATE: re-lays the hits, dropping MUTATE's
    /// nudges.
    pub const fn with_fill(mut self, fill: Fill) -> Ring {
        self.fill = fill;
        self.with_hits(fill.mask())
    }

    /// `s`'s knobs over `self`; its hits only with `rhythm`, its notes and
    /// pool only with `notes`, so drift stands otherwise. Kept hits are
    /// laid again only when STEPS, HITS, ROTATE or ACCENT moved: as
    /// `s.with_mask(self's hits)`, or `s`'s hits where they no longer fit.
    pub(crate) fn take_knobs(&mut self, s: &Ring, rhythm: bool, notes: bool) {
        let &Ring {
            fill,
            hits_mask: _,
            accented: _,
            chance,
            mutate,
            mute,
            gate,
            velocity,
            accent,
            octave,
            range,
            snap,
            mode,
            walk,
            notes: _,
            pool: _,
        } = s;
        let relay = rhythm || fill != self.fill || accent != self.accent;
        if relay {
            let laid = if rhythm {
                *s
            } else {
                s.with_mask(self.hits_mask).unwrap_or(*s)
            };
            (self.hits_mask, self.accented) = (laid.hits_mask, laid.accented);
        }
        if notes {
            (self.notes, self.pool) = (s.notes, s.pool);
        }
        (self.fill, self.chance, self.mutate, self.mute, self.gate) =
            (fill, chance, mutate, mute, gate);
        (self.velocity, self.accent, self.octave, self.range) = (velocity, accent, octave, range);
        (self.snap, self.mode, self.walk) = (snap, mode, walk);
    }

    /// `m` as the hits, if it keeps STEPS and the hit count: a nudge.
    pub fn with_mask(self, m: HitMask) -> Option<Ring> {
        let cur = self.hits_mask;
        if m.steps() != cur.steps() || m.hits() != cur.hits() {
            return None;
        }
        Some(self.with_hits(m))
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
        let note = snap(raw, harmony.target(self.snap));
        let accent = self.accented.is_hit(i);
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

    /// NOTES' "COMES ROUND IN": hits until the walk and the hits line up
    /// again, `lcm(period, hits)`; 0 with no hits. A walk of L notes
    /// (the pool over RANGE + 1 octaves) comes round in L going UP or
    /// DOWN, in 2(L − 1) PING-PONG (its ends once), and never RANDOM:
    /// `None` (ADR 0074 amends spec § 3.4).
    pub fn comes_round_in(&self) -> Option<u32> {
        let len = self.pool.len() as u32 * (self.range.octaves() as u32 + 1);
        let walk = match self.walk {
            Walk::Up | Walk::Down => len,
            Walk::PingPong => (2 * (len - 1)).max(1),
            Walk::Random => return None,
        };
        let hits = self.hits_mask.hits() as u32;
        let gcd = |mut a: u32, mut b: u32| {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        };
        Some(if hits == 0 {
            0
        } else {
            walk / gcd(walk, hits) * hits
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
                    self = self.with_hits(h);
                }
            }
        }
        self
    }
}
