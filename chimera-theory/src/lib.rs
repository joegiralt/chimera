//! Music theory as const data: pitch classes, intervals, scales, chords,
//! snapping and names. No heap, no dependencies.
#![no_std]

use core::fmt::{self, Display, Formatter, Write};
use core::mem::size_of;
use core::ops::{Add, Sub};

/// Which way a note halfway between two allowed pitches snaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tie {
    Up,
    Down,
}

/// The ORBIT spec's rule (§ 5.1).
pub const TIE: Tie = Tie::Down;

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

/// The pitch-class set of a scale relative to its root; bit 0, the root,
/// is always set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Scale(u16);

impl Scale {
    /// Any set of up to 12 notes; bit n is n semitones above the root.
    /// The root is forced on, bits above 11 are dropped.
    pub const fn custom(mask: u16) -> Scale {
        Scale((mask & 0xFFF) | 1)
    }

    /// The set with `i` flipped; the root stays.
    pub const fn toggle(self, i: Interval) -> Scale {
        Scale::custom(self.0 ^ (1 << i.0) | 1)
    }

    pub const fn of(steps: &[Interval]) -> Scale {
        let mut m = 1;
        let mut i = 0;
        while i < steps.len() {
            m |= 1 << steps[i].0;
            i += 1;
        }
        Scale(m)
    }

    pub const MAJOR: Scale = {
        use Interval as I;
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[
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
        Scale::of(&[I::MAJOR_SECOND, I::MAJOR_THIRD, I::FIFTH, I::MAJOR_SIXTH])
    };
    pub const PENTATONIC_MINOR: Scale = {
        use Interval as I;
        Scale::of(&[I::MINOR_THIRD, I::FOURTH, I::FIFTH, I::MINOR_SEVENTH])
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
        let src = self;
        let root = src.nth(d as usize);
        let mut m = 0;
        let mut j = 0;
        while j < st.tones() {
            let s = src.nth(d as usize + 2 * j);
            m |= 1 << ((s + 12 - root) % 12);
            j += 1;
        }
        (Interval(root), m)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Degree {
    I,
    II,
    III,
    IV,
    V,
    VI,
    VII,
}

impl Degree {
    pub const ALL: [Degree; 7] = {
        use Degree::*;
        [I, II, III, IV, V, VI, VII]
    };
}

const NUMERALS: [[u8; 3]; 7] = [
    *b"I  ", *b"II ", *b"III", *b"IV ", *b"V  ", *b"VI ", *b"VII",
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
    /// wrap in sets under 7 notes. Stacks that name no quality take the
    /// nearest one.
    pub const fn chord(self, d: Degree, st: Stack) -> Chord {
        let (root, m) = self.scale.stack(d, st);
        Chord::new(self.tonic.up(root), Quality::nearest(m).0)
    }

    pub const fn triad(self, d: Degree) -> Chord {
        self.chord(d, Stack::Triad)
    }

    /// "iv", "V7", "iio": case from the quality.
    pub const fn degree_label(self, d: Degree, st: Stack) -> DegreeLabel {
        DegreeLabel {
            degree: d,
            quality: self.chord(d, st).quality,
        }
    }
}

struct QualityDef {
    mask: u16,
    name: [u8; 6],
    numeral: [u8; 5],
}

const fn q(steps: &[Interval], name: &[u8], numeral: &[u8]) -> QualityDef {
    let mut n = [0; 6];
    let mut i = 0;
    while i < name.len() {
        n[i] = name[i];
        i += 1;
    }
    let mut r = [0; 5];
    let mut i = 0;
    while i < numeral.len() {
        r[i] = numeral[i];
        i += 1;
    }
    QualityDef {
        mask: Scale::of(steps).0,
        name: n,
        numeral: r,
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
        q(&[m3, p5], b"", b""),
        q(&[m3b, p5], b"m", b""),
        q(&[m3b, I::TRITONE], b"dim", b"o"),
        q(&[m3, I::MINOR_SIXTH], b"aug", b"+"),
        q(&[I::MAJOR_SECOND, p5], b"sus2", b"sus2"),
        q(&[I::FOURTH, p5], b"sus4", b"sus4"),
        q(&[m3, p5, I::MAJOR_SIXTH], b"6", b"6"),
        q(&[m3b, p5, I::MAJOR_SIXTH], b"m6", b"6"),
        q(&[m3, p5, I::MINOR_SEVENTH], b"7", b"7"),
        q(&[m3, p5, I::MAJOR_SEVENTH], b"maj7", b"maj7"),
        q(&[m3b, p5, I::MINOR_SEVENTH], b"m7", b"7"),
        q(&[m3b, I::TRITONE, I::MINOR_SEVENTH], b"m7b5", b"7b5"),
        q(&[m3b, I::TRITONE, I::MAJOR_SIXTH], b"dim7", b"o7"),
        q(&[m3b, p5, I::MAJOR_SEVENTH], b"mMaj7", b"maj7"),
        q(&[m3, I::MINOR_SIXTH, I::MAJOR_SEVENTH], b"maj7#5", b"+maj7"),
        q(&[I::FOURTH, p5, I::MINOR_SEVENTH], b"7sus4", b"7sus4"),
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

    /// The best-scoring quality for a root-relative set: 2 per shared tone,
    /// less 1 per missing or extra one; ties go to table order.
    /// Provisional.
    const fn nearest(m: u16) -> (Quality, i8) {
        let mut best = (Quality::Maj, i8::MIN);
        let mut i = 0;
        while i < Self::ALL.len() {
            let q = Self::ALL[i].mask();
            let score = 2 * (q & m).count_ones() as i8
                - (q & !m).count_ones() as i8
                - (m & !q & 0xFFF).count_ones() as i8;
            if score > best.1 {
                best = (Self::ALL[i], score);
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

    /// Root-relative tones, ascending, root first.
    pub const fn tones(self) -> Tones {
        Tones {
            mask: self.mask(),
            next: 0,
        }
    }
}

/// Iterator over a quality's intervals.
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

/// A root and a quality; the tones are derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    root: PitchClass,
    quality: Quality,
}

impl Chord {
    pub const fn new(root: PitchClass, quality: Quality) -> Chord {
        Chord { root, quality }
    }

    pub const fn root(self) -> PitchClass {
        self.root
    }

    pub const fn quality(self) -> Quality {
        self.quality
    }

    const fn mask(self) -> u16 {
        rotate(self.quality.mask(), self.root)
    }

    pub fn tones(self) -> impl ExactSizeIterator<Item = PitchClass> {
        self.quality.tones().map(move |i| self.root + i)
    }

    pub const fn contains(self, pc: PitchClass) -> bool {
        self.mask() & pc.bit() != 0
    }

    /// A played chord: rooted on the lowest note, with the nearest
    /// quality to its pitch classes. None under two classes.
    pub fn from_notes(notes: &[Note]) -> Option<Chord> {
        let root = notes.iter().min()?.pc();
        let played = notes.iter().fold(0u16, |m, n| m | n.pc().bit());
        if played.count_ones() < 2 {
            return None;
        }
        Some(Chord::new(root, Quality::nearest(relative(played, root)).0))
    }
}

impl Display for Chord {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root)?;
        write_ascii(f, &self.quality.def().name)
    }
}

/// A degree with its quality, shown as a roman numeral.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DegreeLabel {
    pub degree: Degree,
    pub quality: Quality,
}

impl Display for DegreeLabel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let minor = self.quality.is_minor();
        NUMERALS[self.degree as usize]
            .iter()
            .take_while(|&&b| b != b' ')
            .try_for_each(|&b| {
                f.write_char(if minor { b.to_ascii_lowercase() } else { b } as char)
            })?;
        write_ascii(f, &self.quality.def().numeral)
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
            SnapTo::Chord(c) => c.mask(),
        }
    }

    pub const fn contains(self, pc: PitchClass) -> bool {
        self.mask() & pc.bit() != 0
    }
}

/// The nearest note in `to`, ties by `TIE`, staying inside 0..=127.
pub const fn snap(note: Note, to: SnapTo) -> Note {
    let set = to.mask();
    let n = note.0 as i16;
    let mut d = 0;
    // Every set holds a pitch class, so one side hits within 11.
    while d < 12 {
        let (a, b) = match TIE {
            Tie::Up => (n + d, n - d),
            Tie::Down => (n - d, n + d),
        };
        if a >= 0 && a <= 127 && set & (1 << (a % 12)) != 0 {
            return Note(a as u8);
        }
        if b >= 0 && b <= 127 && set & (1 << (b % 12)) != 0 {
            return Note(b as u8);
        }
        d += 1;
    }
    note
}

/// Flash held by the tables above.
pub const TABLE_BYTES: usize = size_of::<[QualityDef; 16]>()
    + size_of::<[Scale; 10]>()
    + size_of::<[[u8; 2]; 12]>()
    + size_of::<[[u8; 3]; 7]>();

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
            assert!(Quality::nearest(s.stack(deg, Stack::Triad).1).1 == 6);
            assert!(Quality::nearest(s.stack(deg, Stack::Seventh).1).1 == 8);
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
