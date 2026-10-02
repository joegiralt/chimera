//! A scene: everything a loop plays, as plain data.

use super::chords::{ChordRing, ScaleKind, Segs};
use super::euclid::{Fill, Steps};
use super::ring::{Octave, Range, Ring, SnapTo};
use super::rng::Pct;
use super::walk::{Pool, Walk};
use super::{MAX_SEGS, RINGS};
use crate::project::PartId;
use chimera_theory::{Degree, Note, PitchClass};

pub const SCENES: usize = 8;

/// A scene slot, 0..8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneIdx(u8);

impl SceneIdx {
    pub const ALL: [SceneIdx; SCENES] = {
        let mut a = [SceneIdx(0); SCENES];
        let mut i = 0;
        while i < SCENES {
            a[i] = SceneIdx(i as u8);
            i += 1;
        }
        a
    };

    pub const fn new(i: u8) -> Option<SceneIdx> {
        if (i as usize) < SCENES {
            Some(SceneIdx(i))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Where a ring's notes go. Kit keys are Slice 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Part(PartId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneRing {
    pub ring: Ring,
    pub target: Target,
    /// Bumped when the rhythm or the notes change under the player's feet.
    pub rhythm_rev: u8,
    pub notes_rev: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene {
    pub chords: ChordRing,
    pub rings: [SceneRing; RINGS],
    pub seed: u32,
}

const fn some<T: Copy>(o: Option<T>) -> T {
    match o {
        Some(v) => v,
        None => panic!(),
    }
}

/// Ring `n` of the init scene: `hits` in `steps` rotated, on A C E of
/// `octave`.
const fn ring(steps: u8, hits: u8, rotate: u8, octave: u8) -> Ring {
    let oct = some(Octave::new(octave));
    let c = oct.low().get();
    let fill = some(Fill::new(some(Steps::new(steps)), hits, rotate));
    let mut r = Ring::new(fill, oct);
    let (a, e) = (some(Note::new(c + 9)), some(Note::new(c + 4)));
    r.pool = some(Pool::new(&[a, oct.low(), e]));
    r
}

const fn on_part(ring: Ring, n: usize) -> SceneRing {
    SceneRing {
        ring,
        target: Target::Part(PartId::ALL[n]),
        rhythm_rev: 0,
        notes_rev: 0,
    }
}

impl Scene {
    /// A minor, i-iv-v-i over one bar; five rings on Parts 1..=5.
    pub const fn init() -> Scene {
        let mut r1 = ring(4, 4, 0, 2);
        r1.snap = SnapTo::Chord;
        r1.gate = Pct::clamped(80);
        let mut r2 = ring(3, 3, 0, 3);
        r2.snap = SnapTo::Chord;
        r2.walk = Walk::PingPong;
        let mut r3 = ring(13, 5, 2, 4);
        r3.gate = Pct::clamped(30);
        r3.walk = Walk::Random;
        r3.mutate = Pct::clamped(20);
        let mut r4 = ring(8, 3, 0, 5);
        r4.gate = Pct::clamped(30);
        r4.mute = true;
        let mut r5 = ring(16, 9, 1, 5);
        r5.gate = Pct::clamped(25);
        r5.range = Range::One;
        r5.chance = Pct::clamped(70);
        r5.accent = 3;
        r5.mutate = Pct::clamped(30);
        let mut degrees = [Degree::I; MAX_SEGS];
        degrees[1] = Degree::IV;
        degrees[2] = Degree::V;
        Scene {
            chords: ChordRing {
                tonic: PitchClass::A,
                scale: ScaleKind::Minor,
                segs: some(Segs::new(4)),
                degrees,
                ..ChordRing::NEUTRAL
            },
            rings: [
                on_part(r1, 0),
                on_part(r2, 1),
                on_part(r3, 2),
                on_part(r4, 3),
                on_part(r5, 4),
            ],
            seed: 0x0B17_0001,
        }
    }

    /// Nothing sounds: every ring muted, C major, one chord.
    pub const fn neutral() -> Scene {
        let oct = some(Octave::new(4));
        let mut r = Ring::new(some(Fill::new(some(Steps::new(1)), 1, 0)), oct);
        r.mute = true;
        Scene {
            chords: ChordRing::NEUTRAL,
            rings: [
                on_part(r, 0),
                on_part(r, 1),
                on_part(r, 2),
                on_part(r, 3),
                on_part(r, 4),
            ],
            seed: 0x0B17_0001,
        }
    }
}
