//! The chord ring: a key and up to MAX_SEGS degrees spread over the loop.

use super::MAX_SEGS;
use super::clock::Phase;
use super::ring::Harmony;
use chimera_theory::{Chord, Degree, Key, PitchClass, Scale};

/// Loop length in bars, 1..=8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bars(u8);

impl Bars {
    pub const ONE: Bars = Bars(1);
    pub const MAX: u8 = 8;

    pub const fn new(n: u8) -> Option<Bars> {
        if n >= 1 && n <= Self::MAX {
            Some(Bars(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Chord segments in the loop, 1..=8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Segs(u8);

impl Segs {
    pub const ONE: Segs = Segs(1);

    pub const fn new(n: u8) -> Option<Segs> {
        if n >= 1 && n as usize <= MAX_SEGS {
            Some(Segs(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// `Scale::ALL`'s order, then the user's own set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ScaleKind {
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    HarmonicMinor,
    PentatonicMajor,
    PentatonicMinor,
    Custom,
}

impl ScaleKind {
    pub const ALL: [ScaleKind; 11] = {
        use ScaleKind::*;
        [
            Major,
            Minor,
            Dorian,
            Phrygian,
            Lydian,
            Mixolydian,
            Locrian,
            HarmonicMinor,
            PentatonicMajor,
            PentatonicMinor,
            Custom,
        ]
    };

    pub const fn scale(self, custom: Scale) -> Scale {
        match self {
            ScaleKind::Custom => custom,
            k => Scale::ALL[k as usize],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChordRing {
    pub tonic: PitchClass,
    pub scale: ScaleKind,
    pub custom: Scale,
    pub bars: Bars,
    pub segs: Segs,
    pub degrees: [Degree; MAX_SEGS],
    /// A chord played in over the degree; it wins.
    pub played: [Option<Chord>; MAX_SEGS],
}

impl ChordRing {
    /// C major, one bar, one segment: I.
    pub const NEUTRAL: ChordRing = ChordRing {
        tonic: PitchClass::C,
        scale: ScaleKind::Major,
        custom: Scale::MAJOR,
        bars: Bars::ONE,
        segs: Segs::ONE,
        degrees: [Degree::I; MAX_SEGS],
        played: [None; MAX_SEGS],
    };

    pub const fn key(&self) -> Key {
        Key::new(self.tonic, self.scale.scale(self.custom))
    }
}

/// The segment holding bar `bar_in_loop` at `phase`:
/// ⌊(bar + phase)/bars · segs⌋, exact in integers; a bar past the loop wraps.
pub fn segment_at(bar_in_loop: u32, phase: Phase, bars: Bars, segs: Segs) -> usize {
    let into_loop = ((bar_in_loop as u64) << 32) | phase.get() as u64;
    let loop_len = (bars.get() as u64) << 32;
    (into_loop % loop_len * segs.get() as u64 / loop_len) as usize
}

pub fn harmony(c: &ChordRing, seg: usize) -> Harmony {
    let key = c.key();
    let chord = c.played[seg].unwrap_or_else(|| key.triad(c.degrees[seg]));
    Harmony { key, chord }
}
