//! Music theory as const data: pitch classes, intervals, scales, chords,
//! snapping and names. No heap, no dependencies.
#![no_std]

use core::fmt::{self, Display, Formatter, Write};
use core::hash::{Hash, Hasher};
use core::mem::size_of;
use core::ops::{Add, Sub};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PitchClass {
    C,
    Cs,
    D,
    Ds,
    E,
    F,
    Fs,
    G,
    Gs,
    A,
    As,
    B,
}

impl PitchClass {
    pub const ALL: [PitchClass; 12] = {
        use PitchClass::*;
        [C, Cs, D, Ds, E, F, Fs, G, Gs, A, As, B]
    };

    const fn wrap(semis: u8) -> Self {
        Self::ALL[(semis % 12) as usize]
    }

    const fn bit(self) -> u16 {
        1 << self as u16
    }

    pub const fn up(self, i: Interval) -> Self {
        Self::wrap(self as u8 + i.0)
    }

    /// The interval from `below` up to `self`.
    pub const fn above(self, below: PitchClass) -> Interval {
        Interval((self as u8 + 12 - below as u8) % 12)
    }
}

impl Add<Interval> for PitchClass {
    type Output = PitchClass;
    fn add(self, i: Interval) -> PitchClass {
        self.up(i)
    }
}

impl Sub for PitchClass {
    type Output = Interval;
    fn sub(self, below: PitchClass) -> Interval {
        self.above(below)
    }
}

const PC_NAMES: [[u8; 2]; 12] = [
    *b"C ", *b"C#", *b"D ", *b"D#", *b"E ", *b"F ", *b"F#", *b"G ", *b"G#", *b"A ", *b"A#", *b"B ",
];

fn write_ascii(f: &mut Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    bytes
        .iter()
        .take_while(|&&b| b != b' ' && b != 0)
        .try_for_each(|&b| f.write_char(b as char))
}

impl Display for PitchClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_ascii(f, &PC_NAMES[*self as usize])
    }
}

/// An interval folded into one octave, 0..=11 semitones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Interval(u8);

impl Interval {
    pub const UNISON: Interval = Interval(0);
    pub const MINOR_SECOND: Interval = Interval(1);
    pub const MAJOR_SECOND: Interval = Interval(2);
    pub const MINOR_THIRD: Interval = Interval(3);
    pub const MAJOR_THIRD: Interval = Interval(4);
    pub const FOURTH: Interval = Interval(5);
    pub const TRITONE: Interval = Interval(6);
    pub const FIFTH: Interval = Interval(7);
    pub const MINOR_SIXTH: Interval = Interval(8);
    pub const MAJOR_SIXTH: Interval = Interval(9);
    pub const MINOR_SEVENTH: Interval = Interval(10);
    pub const MAJOR_SEVENTH: Interval = Interval(11);
    pub const ALL: [Interval; 12] = {
        let mut a = [Interval(0); 12];
        let mut i = 0;
        while i < 12 {
            a[i] = Interval(i as u8);
            i += 1;
        }
        a
    };

    /// Any signed distance, folded into the octave.
    pub const fn from_semitones(s: i32) -> Interval {
        Interval(s.rem_euclid(12) as u8)
    }

    pub const fn semitones(self) -> u8 {
        self.0
    }
}

/// A MIDI note, 0..=127. C4 = 60.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Note(u8);

impl Note {
    pub const MIN: Note = Note(0);
    pub const MAX: Note = Note(127);

    pub const fn new(n: u8) -> Option<Note> {
        if n <= 127 { Some(Note(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub const fn pc(self) -> PitchClass {
        PitchClass::wrap(self.0)
    }

    /// `semitones` up (or down, if negative); None off either end.
    pub const fn transpose(self, semitones: i16) -> Option<Note> {
        let n = self.0 as i16 + semitones;
        if n >= 0 && n <= 127 {
            Some(Note(n as u8))
        } else {
            None
        }
    }
}

impl Display for Note {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.pc(), self.0 as i8 / 12 - 1)
    }
}

/// `set` (bit 0 = root) moved so bit 0 lands on `root`.
const fn rotate(set: u16, root: PitchClass) -> u16 {
    let r = root as u16;
    ((set << r) | (set >> (12 - r))) & 0xFFF
}

/// `set` seen from `root`: bit 0 becomes `root`'s.
const fn relative(set: u16, root: PitchClass) -> u16 {
    let r = (12 - root as u16) % 12;
    ((set << r) | (set >> (12 - r))) & 0xFFF
}

/// Set bits of `set` below bit `below`.
const fn rank(set: u16, below: u8) -> usize {
    (set & ((1 << below) - 1)).count_ones() as usize
}

/// The pitch-class set of a scale relative to its root; bit 0, the root,
/// is always set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scale(u16);

impl Scale {
    /// Any set of notes above the root; the root is always in.
    pub const fn custom(steps: &[Interval]) -> Scale {
        let mut m = 1;
        let mut i = 0;
        while i < steps.len() {
            m |= 1 << steps[i].0;
            i += 1;
        }
        Scale(m)
    }

    /// The set with `i` flipped; the root stays.
    pub const fn toggle(self, i: Interval) -> Scale {
        Scale((self.0 ^ (1 << i.0)) | 1)
    }

    pub const MAJOR: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MAJOR_SECOND,
            I::MAJOR_THIRD,
            I::FOURTH,
            I::FIFTH,
            I::MAJOR_SIXTH,
            I::MAJOR_SEVENTH,
        ])
    };
    pub const MINOR: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MAJOR_SECOND,
            I::MINOR_THIRD,
            I::FOURTH,
            I::FIFTH,
            I::MINOR_SIXTH,
            I::MINOR_SEVENTH,
        ])
    };
    pub const DORIAN: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MAJOR_SECOND,
            I::MINOR_THIRD,
            I::FOURTH,
            I::FIFTH,
            I::MAJOR_SIXTH,
            I::MINOR_SEVENTH,
        ])
    };
    pub const PHRYGIAN: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MINOR_SECOND,
            I::MINOR_THIRD,
            I::FOURTH,
            I::FIFTH,
            I::MINOR_SIXTH,
            I::MINOR_SEVENTH,
        ])
    };
    pub const LYDIAN: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MAJOR_SECOND,
            I::MAJOR_THIRD,
            I::TRITONE,
            I::FIFTH,
            I::MAJOR_SIXTH,
            I::MAJOR_SEVENTH,
        ])
    };
    pub const MIXOLYDIAN: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MAJOR_SECOND,
            I::MAJOR_THIRD,
            I::FOURTH,
            I::FIFTH,
            I::MAJOR_SIXTH,
            I::MINOR_SEVENTH,
        ])
    };
    pub const LOCRIAN: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MINOR_SECOND,
            I::MINOR_THIRD,
            I::FOURTH,
            I::TRITONE,
            I::MINOR_SIXTH,
            I::MINOR_SEVENTH,
        ])
    };
    pub const HARMONIC_MINOR: Scale = {
        use Interval as I;
        Scale::custom(&[
            I::MAJOR_SECOND,
            I::MINOR_THIRD,
            I::FOURTH,
            I::FIFTH,
            I::MINOR_SIXTH,
            I::MAJOR_SEVENTH,
        ])
    };
    pub const PENTATONIC_MAJOR: Scale = {
        use Interval as I;
        Scale::custom(&[I::MAJOR_SECOND, I::MAJOR_THIRD, I::FIFTH, I::MAJOR_SIXTH])
    };
    pub const PENTATONIC_MINOR: Scale = {
        use Interval as I;
        Scale::custom(&[I::MINOR_THIRD, I::FOURTH, I::FIFTH, I::MINOR_SEVENTH])
    };

    pub const ALL: [Scale; 10] = [
        Scale::MAJOR,
        Scale::MINOR,
        Scale::DORIAN,
        Scale::PHRYGIAN,
        Scale::LYDIAN,
        Scale::MIXOLYDIAN,
        Scale::LOCRIAN,
        Scale::HARMONIC_MINOR,
        Scale::PENTATONIC_MAJOR,
        Scale::PENTATONIC_MINOR,
    ];

    pub const fn contains(self, i: Interval) -> bool {
        self.0 & (1 << i.0) != 0
    }

    /// Notes in the set, 1..=12.
    pub const fn degree_count(self) -> usize {
        self.0.count_ones() as usize
    }

    /// The `index`th note's degree, counting from 0; None past the set.
    pub const fn degree(self, index: usize) -> Option<Degree> {
        if index < self.degree_count() {
            Some(Degree(index as u8))
        } else {
            None
        }
    }

    /// Every degree of the set, from the root up.
    pub fn degrees(self) -> impl ExactSizeIterator<Item = Degree> {
        Degree::ALL.into_iter().take(self.degree_count())
    }

    /// The degree of the note `i` above the root, if it is in the set.
    const fn degree_of(self, i: Interval) -> Option<Degree> {
        if self.contains(i) {
            Some(Degree(rank(self.0, i.0) as u8))
        } else {
            None
        }
    }

    /// Semitones of the `k`th note, `k` wrapping within the set.
    const fn nth(self, k: usize) -> u8 {
        let mut k = k % self.degree_count();
        let mut s = 0;
        loop {
            if self.0 & (1 << s) != 0 {
                if k == 0 {
                    return s;
                }
                k -= 1;
            }
            s += 1;
        }
    }

    /// Every other note from `d`'s, as a mask relative to `d`'s note.
    const fn stack(self, d: Degree, st: Stack) -> (Interval, u16) {
        let d = d.0 as usize;
        let root = self.nth(d);
        let mut m = 0;
        let mut j = 0;
        while j < st.tones() {
            let s = self.nth(d + 2 * j);
            m |= 1 << ((s + 12 - root) % 12);
            j += 1;
        }
        (Interval(root), m)
    }
}

const SCALE_NAMES: [[u8; 8]; 10] = [
    *b"MAJOR   ",
    *b"MINOR   ",
    *b"DORIAN  ",
    *b"PHRYGIAN",
    *b"LYDIAN  ",
    *b"MIXOLYD ",
    *b"LOCRIAN ",
    *b"HARM MIN",
    *b"PENT MAJ",
    *b"PENT MIN",
];

impl Display for Scale {
    /// A named scale's name, else "CUSTOM".
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match Scale::ALL.iter().position(|s| s == self) {
            // Names hold one space at most, never at the start of a word.
            Some(i) => f.write_str(
                core::str::from_utf8(&SCALE_NAMES[i])
                    .map_err(|_| fmt::Error)?
                    .trim_end(),
            ),
            None => f.write_str("CUSTOM"),
        }
    }
}

/// A step of a scale, counting from its root: I..=XII, so a degree
/// reaches every note of any set. Degrees past a smaller set's size wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Degree(u8);

impl Degree {
    pub const I: Degree = Degree(0);
    pub const II: Degree = Degree(1);
    pub const III: Degree = Degree(2);
    pub const IV: Degree = Degree(3);
    pub const V: Degree = Degree(4);
    pub const VI: Degree = Degree(5);
    pub const VII: Degree = Degree(6);
    pub const ALL: [Degree; 12] = {
        let mut a = [Degree(0); 12];
        let mut i = 0;
        while i < 12 {
            a[i] = Degree(i as u8);
            i += 1;
        }
        a
    };

    /// Counting from 0.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

const NUMERALS: [[u8; 4]; 12] = [
    *b"I   ", *b"II  ", *b"III ", *b"IV  ", *b"V   ", *b"VI  ", *b"VII ", *b"VIII", *b"IX  ",
    *b"X   ", *b"XI  ", *b"XII ",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stack {
    Triad,
    Seventh,
}

impl Stack {
    const fn tones(self) -> usize {
        match self {
            Stack::Triad => 3,
            Stack::Seventh => 4,
        }
    }
}

/// A tonic and a scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub tonic: PitchClass,
    pub scale: Scale,
}

impl Key {
    pub const fn new(tonic: PitchClass, scale: Scale) -> Key {
        Key { tonic, scale }
    }

    const fn mask(self) -> u16 {
        rotate(self.scale.0, self.tonic)
    }

    pub const fn contains(self, pc: PitchClass) -> bool {
        self.mask() & pc.bit() != 0
    }

    pub const fn degree_count(self) -> usize {
        self.scale.degree_count()
    }

    /// Stacks every other note of the set from the degree's note; degrees
    /// wrap in smaller sets. The chord keeps exactly the stacked tones,
    /// whether or not they name a quality.
    pub const fn chord(self, d: Degree, st: Stack) -> Chord {
        let (root, m) = self.scale.stack(d, st);
        let root = self.tonic.up(root);
        Chord::built(root, root, PcSet(rotate(m, root)))
    }

    pub const fn triad(self, d: Degree) -> Chord {
        self.chord(d, Stack::Triad)
    }

    /// The stack as a roman numeral from its label: "iv", "V7", "viio/7";
    /// inversions in figured bass, "i6", "I64"; "IV?" when no quality
    /// matches exactly.
    pub const fn degree_label(self, d: Degree, st: Stack) -> DegreeLabel {
        let c = self.chord(d, st);
        let l = c.label();
        if !l.exact {
            return DegreeLabel {
                degree: d,
                shape: None,
            };
        }
        let degree = match self.scale.degree_of(l.root.above(self.tonic)) {
            Some(r) => r,
            None => d,
        };
        let inversion = rank(relative(c.tones.0, l.root), l.bass.above(l.root).0);
        DegreeLabel {
            degree,
            shape: Some((l.quality, inversion as u8)),
        }
    }
}

struct QualityDef {
    mask: u16,
    name: [u8; 6],
    /// Numeral marks either side of the figure: "o" in "viio7", "sus4"
    /// in "V7sus4".
    pre: [u8; 4],
    post: [u8; 4],
}

const fn pad<const N: usize>(s: &[u8]) -> [u8; N] {
    let mut a = [0; N];
    let mut i = 0;
    while i < s.len() {
        a[i] = s[i];
        i += 1;
    }
    a
}

const fn q(steps: &[Interval], name: &[u8], pre: &[u8], post: &[u8]) -> QualityDef {
    QualityDef {
        mask: Scale::custom(steps).0,
        name: pad(name),
        pre: pad(pre),
        post: pad(post),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Quality {
    Maj,
    Min,
    Dim,
    Aug,
    Sus2,
    Sus4,
    Maj6,
    Min6,
    Dom7,
    Maj7,
    Min7,
    HalfDim7,
    Dim7,
    MinMaj7,
    AugMaj7,
    Dom7Sus4,
}

const QUALITIES: [QualityDef; 16] = {
    use Interval as I;
    let (m3, m3b, p5) = (I::MAJOR_THIRD, I::MINOR_THIRD, I::FIFTH);
    [
        q(&[m3, p5], b"", b"", b""),
        q(&[m3b, p5], b"m", b"", b""),
        q(&[m3b, I::TRITONE], b"dim", b"o", b""),
        q(&[m3, I::MINOR_SIXTH], b"aug", b"+", b""),
        q(&[I::MAJOR_SECOND, p5], b"sus2", b"", b"sus2"),
        q(&[I::FOURTH, p5], b"sus4", b"", b"sus4"),
        q(&[m3, p5, I::MAJOR_SIXTH], b"6", b"", b"add6"),
        q(&[m3b, p5, I::MAJOR_SIXTH], b"m6", b"", b"add6"),
        q(&[m3, p5, I::MINOR_SEVENTH], b"7", b"", b""),
        q(&[m3, p5, I::MAJOR_SEVENTH], b"maj7", b"maj", b""),
        q(&[m3b, p5, I::MINOR_SEVENTH], b"m7", b"", b""),
        q(&[m3b, I::TRITONE, I::MINOR_SEVENTH], b"m7b5", b"o/", b""),
        q(&[m3b, I::TRITONE, I::MAJOR_SIXTH], b"dim7", b"o", b""),
        q(&[m3b, p5, I::MAJOR_SEVENTH], b"mMaj7", b"maj", b""),
        q(
            &[m3, I::MINOR_SIXTH, I::MAJOR_SEVENTH],
            b"maj7#5",
            b"+maj",
            b"",
        ),
        q(&[I::FOURTH, p5, I::MINOR_SEVENTH], b"7sus4", b"", b"sus4"),
    ]
};

impl Quality {
    pub const ALL: [Quality; 16] = {
        use Quality::*;
        [
            Maj, Min, Dim, Aug, Sus2, Sus4, Maj6, Min6, Dom7, Maj7, Min7, HalfDim7, Dim7, MinMaj7,
            AugMaj7, Dom7Sus4,
        ]
    };

    const fn def(self) -> &'static QualityDef {
        &QUALITIES[self as usize]
    }

    const fn mask(self) -> u16 {
        self.def().mask
    }

    /// The quality whose tones are exactly `set` (absolute) read from `root`.
    const fn exact(set: u16, root: PitchClass) -> Option<Quality> {
        let m = relative(set, root);
        let mut i = 0;
        while i < Self::ALL.len() {
            if Self::ALL[i].mask() == m {
                return Some(Self::ALL[i]);
            }
            i += 1;
        }
        None
    }

    /// Aug and dim7 read the same from every tone.
    const fn is_symmetric(self) -> bool {
        matches!(self, Quality::Aug | Quality::Dim7)
    }

    /// The best quality for a root-relative set and its rank: 2 per shared
    /// tone, less 1 per missing or extra one, doubled, plus 1 if it isn't
    /// symmetric, so aug and dim7 lose ties. Equal ranks go to table order.
    const fn nearest(m: u16) -> (Quality, i8) {
        let mut best = (Quality::Maj, i8::MIN);
        let mut i = 0;
        while i < Self::ALL.len() {
            let q = Self::ALL[i].mask();
            let score = 2 * (q & m).count_ones() as i8
                - (q & !m).count_ones() as i8
                - (m & !q & 0xFFF).count_ones() as i8;
            let rank = 2 * score + !Self::ALL[i].is_symmetric() as i8;
            if rank > best.1 {
                best = (Self::ALL[i], rank);
            }
            i += 1;
        }
        best
    }

    /// Minor third and no major third: a lower-case numeral.
    const fn is_minor(self) -> bool {
        let m = self.mask();
        m & (1 << 3) != 0 && m & (1 << 4) == 0
    }

    /// Four tones with a seventh, not an added sixth: figured 7, 65, 43, 42.
    const fn is_seventh(self) -> bool {
        let m = self.mask();
        m.count_ones() == 4 && !(m & (1 << 7) != 0 && m & (1 << 9) != 0)
    }

    /// Root-relative tones, ascending, root first.
    pub const fn tones(self) -> Tones {
        Tones {
            mask: self.mask(),
            next: 0,
        }
    }
}

/// Iterator over a set's intervals, ascending.
#[derive(Clone, Copy, Debug)]
pub struct Tones {
    mask: u16,
    next: u8,
}

impl Iterator for Tones {
    type Item = Interval;
    fn next(&mut self) -> Option<Interval> {
        while self.next < 12 {
            let s = self.next;
            self.next += 1;
            if self.mask & (1 << s) != 0 {
                return Some(Interval(s));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = (self.mask >> self.next).count_ones() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Tones {}

/// A set of pitch classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PcSet(u16);

impl PcSet {
    pub const fn of(pcs: &[PitchClass]) -> PcSet {
        let mut m = 0;
        let mut i = 0;
        while i < pcs.len() {
            m |= pcs[i].bit();
            i += 1;
        }
        PcSet(m)
    }

    pub const fn contains(self, pc: PitchClass) -> bool {
        self.0 & pc.bit() != 0
    }

    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Ascending from C.
    pub fn iter(self) -> impl ExactSizeIterator<Item = PitchClass> {
        Tones {
            mask: self.0,
            next: 0,
        }
        .map(|i| PitchClass::C + i)
    }
}

/// Exactly the tones played or stacked, and the lowest one (`bass`),
/// always among them. Two chords are equal when their tones and bass are.
/// The name is a label worked out from the tones: see [`ChordLabel`].
#[derive(Clone, Copy, Debug)]
pub struct Chord {
    /// The note it was built from (a stacked degree, a quality's root, the
    /// played bass): the label tries it first as the root.
    built_on: PitchClass,
    bass: PitchClass,
    tones: PcSet,
}

impl PartialEq for Chord {
    fn eq(&self, other: &Chord) -> bool {
        (self.tones, self.bass) == (other.tones, other.bass)
    }
}

impl Eq for Chord {}

impl Hash for Chord {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (self.tones, self.bass).hash(h)
    }
}

impl Chord {
    /// `built_on` and `bass` are forced into the set.
    const fn built(built_on: PitchClass, bass: PitchClass, tones: PcSet) -> Chord {
        Chord {
            built_on,
            bass,
            tones: PcSet(tones.0 | built_on.bit() | bass.bit()),
        }
    }

    /// The quality's tones on `root`, in root position.
    pub const fn new(root: PitchClass, quality: Quality) -> Chord {
        Chord::built(root, root, PcSet(rotate(quality.mask(), root)))
    }

    /// The same tones over another of them; None if `bass` isn't one.
    pub const fn over(self, bass: PitchClass) -> Option<Chord> {
        if self.tones.contains(bass) {
            Some(Chord { bass, ..self })
        } else {
            None
        }
    }

    /// A played chord: its pitch classes over the lowest note.
    /// None under two classes.
    pub fn from_notes(notes: &[Note]) -> Option<Chord> {
        let bass = notes.iter().min()?.pc();
        let played = notes.iter().fold(0u16, |m, n| m | n.pc().bit());
        (played.count_ones() >= 2).then_some(Chord::built(bass, bass, PcSet(played)))
    }

    /// The label's root.
    pub const fn root(self) -> PitchClass {
        self.label().root
    }

    pub const fn bass(self) -> PitchClass {
        self.bass
    }

    pub const fn tones(self) -> PcSet {
        self.tones
    }

    pub const fn contains(self, pc: PitchClass) -> bool {
        self.tones.contains(pc)
    }

    /// Candidate roots in naming order: the note it was built on, the
    /// bass, then the other tones rising from the first.
    const fn candidate(self, k: usize) -> PitchClass {
        match k {
            0 => self.built_on,
            1 => self.bass,
            _ => self.built_on.up(Interval::ALL[k - 1]),
        }
    }

    /// Tries every tone as the root for an exact quality, in
    /// [`candidate`](Self::candidate) order. With none, the nearest quality
    /// over every tone, marked approximate.
    pub const fn label(self) -> ChordLabel {
        let mut k = 0;
        while k < 13 {
            let r = self.candidate(k);
            if self.tones.contains(r)
                && let Some(q) = Quality::exact(self.tones.0, r)
            {
                return ChordLabel::new(r, q, self.bass, true);
            }
            k += 1;
        }
        let mut best = (self.built_on, Quality::Maj, i8::MIN);
        let mut k = 0;
        while k < 13 {
            let r = self.candidate(k);
            if self.tones.contains(r) {
                let (q, rank) = Quality::nearest(relative(self.tones.0, r));
                if rank > best.2 {
                    best = (r, q, rank);
                }
            }
            k += 1;
        }
        ChordLabel::new(best.0, best.1, self.bass, false)
    }
}

impl Display for Chord {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.label().fmt(f)
    }
}

/// A chord's name: "Am", "Am/C", or "Csus2?" when no quality matches the
/// tones exactly and this is the nearest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChordLabel {
    root: PitchClass,
    quality: Quality,
    bass: PitchClass,
    exact: bool,
}

impl ChordLabel {
    const fn new(root: PitchClass, quality: Quality, bass: PitchClass, exact: bool) -> Self {
        ChordLabel {
            root,
            quality,
            bass,
            exact,
        }
    }

    pub const fn root(self) -> PitchClass {
        self.root
    }

    pub const fn quality(self) -> Quality {
        self.quality
    }

    /// Shown after a slash when it isn't the root.
    pub const fn bass(self) -> PitchClass {
        self.bass
    }

    pub const fn is_exact(self) -> bool {
        self.exact
    }
}

impl Display for ChordLabel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root)?;
        write_ascii(f, &self.quality.def().name)?;
        if self.bass != self.root {
            write!(f, "/{}", self.bass)?;
        }
        if !self.exact {
            f.write_char('?')?;
        }
        Ok(())
    }
}

/// A stacked degree as a roman numeral: the degree of the label's root,
/// its quality and a figured-bass inversion; or the stacked degree and
/// "?" when no quality matches exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DegreeLabel {
    degree: Degree,
    /// The quality and the bass's place among its tones, 0..=3.
    shape: Option<(Quality, u8)>,
}

impl Display for DegreeLabel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let minor = matches!(self.shape, Some((q, _)) if q.is_minor());
        NUMERALS[self.degree.0 as usize]
            .iter()
            .take_while(|&&b| b != b' ')
            .try_for_each(|&b| {
                f.write_char(if minor { b.to_ascii_lowercase() } else { b } as char)
            })?;
        let Some((q, inversion)) = self.shape else {
            return f.write_char('?');
        };
        let figure = match (q.def().mask.count_ones(), inversion) {
            (4, 0) if q.is_seventh() => "7",
            (3, 1) => "6",
            (3, 2) => "64",
            (4, 1) => "65",
            (4, 2) => "43",
            (4, 3) => "42",
            _ => "",
        };
        write_ascii(f, &q.def().pre)?;
        f.write_str(figure)?;
        write_ascii(f, &q.def().post)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapTo {
    Scale(Key),
    Chord(Chord),
}

impl SnapTo {
    const fn mask(self) -> u16 {
        match self {
            SnapTo::Scale(s) => s.mask(),
            SnapTo::Chord(c) => c.tones.0,
        }
    }

    pub const fn contains(self, pc: PitchClass) -> bool {
        self.mask() & pc.bit() != 0
    }
}

/// The nearest note in `to`, staying inside 0..=127; a tie goes down.
/// A chord snaps to exactly its tones.
pub const fn snap(note: Note, to: SnapTo) -> Note {
    let set = to.mask();
    let n = note.0 as i16;
    let mut d = 0;
    // Every set holds a pitch class, so one side hits within 11.
    while d < 12 {
        let (a, b) = (n - d, n + d);
        if a >= 0 && set & (1 << (a % 12)) != 0 {
            return Note(a as u8);
        }
        if b <= 127 && set & (1 << (b % 12)) != 0 {
            return Note(b as u8);
        }
        d += 1;
    }
    note
}

/// Flash held by every table above.
pub const TABLE_BYTES: usize = size_of::<[PitchClass; 12]>()
    + size_of::<[[u8; 2]; 12]>()
    + size_of::<[Interval; 12]>()
    + size_of::<[Scale; 10]>()
    + size_of::<[[u8; 8]; 10]>()
    + size_of::<[Degree; 12]>()
    + size_of::<[[u8; 4]; 12]>()
    + size_of::<[QualityDef; 16]>()
    + size_of::<[Quality; 16]>();

const _: () = {
    let mut i = 0;
    while i < Scale::ALL.len() {
        let s = Scale::ALL[i];
        assert!(s.contains(Interval::UNISON));
        let n = s.degree_count();
        assert!(n == 5 || n == 7);
        // Named heptatonic scales stack to exact qualities.
        let mut d = 0;
        while n == 7 && d < 7 {
            let deg = Degree::ALL[d];
            assert!(Quality::exact(s.stack(deg, Stack::Triad).1, PitchClass::C).is_some());
            assert!(Quality::exact(s.stack(deg, Stack::Seventh).1, PitchClass::C).is_some());
            d += 1;
        }
        i += 1;
    }
    let mut i = 0;
    while i < QUALITIES.len() {
        let m = QUALITIES[i].mask;
        assert!(m & 1 != 0);
        assert!(m.count_ones() >= 3 && m.count_ones() <= 4);
        assert!(Quality::ALL[i] as usize == i);
        let mut j = i + 1;
        while j < QUALITIES.len() {
            assert!(QUALITIES[j].mask != m, "qualities are distinct");
            j += 1;
        }
        i += 1;
    }
    assert!(TABLE_BYTES <= 512);
};
