//! A scene: everything a loop plays, as plain data; and the song a
//! project keeps, edited through the ORBIT blocks.

use super::chords::{Bars, ChordRing, ScaleKind, Segs};
use super::clock::Bpm;
use super::euclid::{Fill, Steps};
use super::mutate::register_note;
use super::ring::{NoteMode, Octave, Range, Ring, SnapTo};
use super::rng::{Pct, Rng};
use super::runner::OrbitView;
use super::walk::{Pool, Walk};
use super::{MAX_POOL, MAX_SEGS, RINGS, RingIdx};
use crate::Velocity;
use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::modulator::law::Law;
use crate::dsp::modulator::pick;
use crate::project::PartId;
use crate::ui::glyph::FocusGlyph;
use chimera_theory::{Degree, Interval, Key, Note, PitchClass, Scale};

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

/// What a `Target` is, apart from which: its stored kind (spec § 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Part,
}

impl Target {
    pub const fn kind(self) -> TargetKind {
        match self {
            Target::Part(_) => TargetKind::Part,
        }
    }

    /// The Part that plays it.
    pub const fn part(self) -> PartId {
        match self {
            Target::Part(p) => p,
        }
    }
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

/// A ring of the init scene: `hits` in `steps` rotated, walking `pool`
/// (MIDI notes) from `octave`.
const fn ring(steps: u8, hits: u8, rotate: u8, octave: u8, pool: &[u8]) -> Ring {
    let fill = some(Fill::new(some(Steps::new(steps)), hits, rotate));
    let mut r = Ring::new(fill, some(Octave::new(octave)));
    let mut notes = [some(Note::new(0)); MAX_POOL];
    let mut i = 0;
    while i < pool.len() {
        notes[i] = some(Note::new(pool[i]));
        i += 1;
    }
    r.pool = some(Pool::new(notes.split_at(pool.len()).0));
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
    /// The owner's pick at UAT, "sparse groove" (ADR 0073): A minor,
    /// i-VI-VII-i a bar each. Ring 1 the bass on Part 1, each chord's root;
    /// ring 2 the e-piano on Part 2; rings 3 and 4 the pad's two held notes
    /// on Part 3; ring 5 (Part 5) muted, to bring in. Nothing random:
    /// CHANCE 100, no MUTATE.
    pub const fn init() -> Scene {
        let mut r1 = ring(2, 2, 0, 1, &[33, 33, 41, 41, 43, 43, 45, 45]);
        r1.snap = SnapTo::Chord;
        r1.gate = Pct::clamped(90);
        let mut r2 = ring(8, 3, 0, 4, &[69, 72, 76]);
        r2.snap = SnapTo::Chord;
        r2.walk = Walk::PingPong;
        r2.gate = Pct::clamped(70);
        r2.velocity = some(Velocity::new(80));
        let mut r3 = ring(1, 1, 0, 4, &[64]);
        r3.snap = SnapTo::Chord;
        r3.gate = Pct::FULL;
        r3.velocity = some(Velocity::new(90));
        let mut r4 = ring(1, 1, 0, 3, &[57]);
        r4.snap = SnapTo::Chord;
        r4.gate = Pct::FULL;
        r4.velocity = some(Velocity::new(90));
        let mut r5 = ring(16, 9, 1, 5, &[81, 72, 76]);
        r5.gate = Pct::clamped(25);
        r5.mute = true;
        let degrees = {
            let mut d = [Degree::I; MAX_SEGS];
            d[1] = Degree::VI;
            d[2] = Degree::VII;
            d
        };
        Scene {
            chords: ChordRing {
                tonic: PitchClass::A,
                scale: ScaleKind::Minor,
                bars: some(Bars::new(4)),
                segs: some(Segs::new(4)),
                degrees,
                ..ChordRing::NEUTRAL
            },
            rings: [
                on_part(r1, 0),
                on_part(r2, 1),
                on_part(r3, 2),
                on_part(r4, 2),
                on_part(r5, 4),
            ],
            seed: 0x0B17_0001,
        }
    }

    /// CLEAR down on ring `r`: its notes rolled from the scene's seed and
    /// the ring's `notes_rev`, so each turn rolls new ones.
    pub fn scatter(&mut self, r: RingIdx) {
        let key = self.chords.key();
        let s = &mut self.rings[r.get() as usize];
        let seed = self.seed.wrapping_add(u32::from(s.notes_rev) << 16);
        s.scatter(&key, &mut Rng::for_ring(seed, r));
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

/// What a project keeps of ORBIT: the scene being edited and played, its
/// drift absorbed, and the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrbitSong {
    pub live: Scene,
    pub bpm: Bpm,
}

impl OrbitSong {
    /// NEW's: the init scene at 120 BPM.
    pub const NEW: OrbitSong = OrbitSong {
        live: Scene::init(),
        bpm: Bpm::DEFAULT,
    };

    /// Each ring's drift from `v` into `live`, the revs untouched: its hits
    /// while the rhythm is the one the view played, its notes and pool
    /// while the notes are. A ring edited since is left as edited.
    pub fn absorb(&mut self, v: &OrbitView) {
        for (s, r) in self.live.rings.iter_mut().zip(&v.rings) {
            if r.rhythm_rev == s.rhythm_rev
                && let Some(ring) = s.ring.with_mask(r.hits)
            {
                s.ring = ring;
            }
            if r.notes_rev == s.notes_rev {
                s.ring.notes = r.notes;
                s.ring.pool = r.pool;
            }
        }
    }
}

const MAX_STEPS: usize = super::MAX_STEPS as usize;

const PITCH_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const SCALE_NAMES: [&str; 11] = [
    "MAJOR", "MINOR", "DORIAN", "PHRYGIAN", "LYDIAN", "MIXOLYD", "LOCRIAN", "HARM MIN", "PENT MAJ",
    "PENT MIN", "CUSTOM",
];
const OFF_ON: [&str; 2] = ["OFF", "ON"];
const RANGE_NAMES: [&str; 4] = ["0", "1", "2", "3"];
const SNAP_NAMES: [&str; 2] = ["SCALE", "CHORD"];
const MODE_NAMES: [&str; 2] = ["STEP", "POOL"];
/// PING-PONG as `PING-PG`: a cell holds no more.
const WALK_NAMES: [&str; 4] = ["UP", "DOWN", "RANDOM", "PING-PG"];

const _: () = assert!(PITCH_NAMES.len() == PitchClass::ALL.len());
const _: () = assert!(SCALE_NAMES.len() == ScaleKind::ALL.len());
const _: () = assert!(RANGE_NAMES.len() == Range::ALL.len());
const _: () = assert!(SNAP_NAMES.len() == SnapTo::ALL.len());
const _: () = assert!(MODE_NAMES.len() == NoteMode::ALL.len());
const _: () = assert!(WALK_NAMES.len() == Walk::ALL.len());

impl ScaleKind {
    /// As SCALE shows it: `MINOR`, `HARM MIN`.
    pub const fn name(self) -> &'static str {
        SCALE_NAMES[self as usize]
    }
}

const PCT: ValFmt = ValFmt::Law(Law::Pct);

/// ORBIT's params are the project's, never a voice's: none modulatable.
/// STEPS, BARS and SEGS hold their count less one, shown one-based.
pub static RING_SPECS: [ParamSpec; 15] = [
    ParamSpec::stepped(0, "STEPS", ValFmt::OneBased(31), 0.0, 31.0, 15.0, false).ident("STEPS"),
    ParamSpec::stepped(1, "HITS", ValFmt::Int(32), 0.0, 32.0, 4.0, false).ident("HITS"),
    ParamSpec::stepped(2, "ROTATE", ValFmt::Int(31), 0.0, 31.0, 0.0, false).ident("ROTATE"),
    ParamSpec::stepped(3, "CHANCE", PCT, 0.0, 100.0, 100.0, false).ident("CHANCE"),
    ParamSpec::stepped(4, "MUTATE", PCT, 0.0, 100.0, 0.0, false).ident("MUTATE"),
    ParamSpec::stepped(5, "MUTE", ValFmt::Names(&OFF_ON), 0.0, 1.0, 0.0, false)
        .ident("MUTE")
        .glyph(FocusGlyph::Switch),
    ParamSpec::stepped(6, "GATE", PCT, 0.0, 100.0, 50.0, false).ident("GATE"),
    ParamSpec::stepped(7, "VEL", ValFmt::OneBased(126), 1.0, 127.0, 100.0, false).ident("VEL"),
    ParamSpec::stepped(8, "ACCENT", ValFmt::Int(32), 0.0, 32.0, 0.0, false).ident("ACCENT"),
    ParamSpec::stepped(9, "OCTAVE", ValFmt::Int(8), 0.0, 8.0, 4.0, false).ident("OCTAVE"),
    ParamSpec::choice(10, "RANGE", ValFmt::Names(&RANGE_NAMES), 3.0, 0.0).ident("RANGE"),
    ParamSpec::choice(11, "SNAP", ValFmt::Names(&SNAP_NAMES), 1.0, 0.0)
        .ident("SNAP")
        .glyph(FocusGlyph::None),
    ParamSpec::choice(12, "MODE", ValFmt::Names(&MODE_NAMES), 1.0, 1.0)
        .ident("MODE")
        .glyph(FocusGlyph::None),
    ParamSpec::choice(13, "WALK", ValFmt::Names(&WALK_NAMES), 3.0, 0.0)
        .ident("WALK")
        .glyph(FocusGlyph::None),
    ParamSpec::stepped(14, "TARGET", ValFmt::OneBased(5), 0.0, 5.0, 0.0, false).ident("TARGET"),
];

pub static CHORD_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(0, "KEY", ValFmt::Names(&PITCH_NAMES), 11.0, 0.0)
        .ident("KEY")
        .glyph(FocusGlyph::None),
    ParamSpec::choice(1, "SCALE", ValFmt::Names(&SCALE_NAMES), 10.0, 0.0)
        .ident("SCALE")
        .glyph(FocusGlyph::None),
    ParamSpec::stepped(
        2,
        "BARS",
        ValFmt::OneBased(Bars::MAX - 1),
        0.0,
        (Bars::MAX - 1) as f32,
        0.0,
        false,
    )
    .ident("BARS"),
    ParamSpec::stepped(3, "SEGS", ValFmt::OneBased(7), 0.0, 7.0, 0.0, false).ident("SEGS"),
];

pub static TEMPO_SPECS: [ParamSpec; 1] = [ParamSpec::stepped(
    0,
    "BPM",
    ValFmt::Bpm,
    Bpm::MIN as f32,
    Bpm::MAX as f32,
    Bpm::DEFAULT.get() as f32,
    false,
)
.ident("BPM")];

/// `v`'s place in `all`, as a param value.
fn index_of<T: PartialEq>(all: &[T], v: &T) -> f32 {
    all.iter().position(|x| x == v).unwrap_or(0) as f32
}

/// `n` moved `by` semitones, folded back an octave at a time into 0..=127.
fn fold(n: Note, by: i16) -> Note {
    let mut x = n.get() as i16 + by;
    while x > 127 {
        x -= 12;
    }
    while x < 0 {
        x += 12;
    }
    Note::new(x as u8).unwrap_or(n)
}

impl SceneRing {
    pub const STEPS: ParamId = ParamId(0);
    pub const HITS: ParamId = ParamId(1);
    pub const ROTATE: ParamId = ParamId(2);
    pub const CHANCE: ParamId = ParamId(3);
    pub const MUTATE: ParamId = ParamId(4);
    pub const MUTE: ParamId = ParamId(5);
    pub const GATE: ParamId = ParamId(6);
    pub const VEL: ParamId = ParamId(7);
    pub const ACCENT: ParamId = ParamId(8);
    pub const OCTAVE: ParamId = ParamId(9);
    pub const RANGE: ParamId = ParamId(10);
    pub const SNAP: ParamId = ParamId(11);
    pub const MODE: ParamId = ParamId(12);
    pub const WALK: ParamId = ParamId(13);
    pub const TARGET: ParamId = ParamId(14);

    /// A new STEPS, HITS or ROTATE: the hits re-laid, the player told.
    fn refill(&mut self, fill: Fill) {
        if fill != self.ring.fill() {
            self.ring = self.ring.with_fill(fill);
            self.rhythm_rev = self.rhythm_rev.wrapping_add(1);
        }
    }

    /// The notes changed under the player: it takes them, drift and all.
    fn renoted(&mut self) {
        self.notes_rev = self.notes_rev.wrapping_add(1);
    }

    /// What NOTES shows as chips and NOTE edits: the pool's notes, or a
    /// STEP ring's steps.
    pub fn chips(&self) -> usize {
        match self.ring.mode {
            NoteMode::Pool => self.ring.pool.len(),
            NoteMode::Step => self.ring.fill().steps().get() as usize,
        }
    }

    /// Chip `i`'s note.
    pub fn chip(&self, i: usize) -> Note {
        match self.ring.mode {
            NoteMode::Pool => self.ring.pool.get(i),
            NoteMode::Step => self.ring.notes[i % MAX_STEPS],
        }
    }

    /// Chip `i` set to `to`; where it is after, as the pool re-sorts.
    pub fn set_chip(&mut self, i: usize, to: Note) -> usize {
        if self.chip(i) == to {
            return i;
        }
        self.renoted();
        let r = &mut self.ring;
        match r.mode {
            NoteMode::Pool => {
                r.pool = r.pool.with(i, to);
                r.pool.notes().iter().position(|&n| n == to).unwrap_or(i)
            }
            NoteMode::Step => {
                r.notes[i % MAX_STEPS] = to;
                i
            }
        }
    }

    /// CLEAR up: one note, OCTAVE's C; a STEP ring every step at it.
    pub fn clear(&mut self) {
        self.renoted();
        let r = &mut self.ring;
        let c = r.octave.low();
        match r.mode {
            NoteMode::Pool => r.pool = Pool::one(c),
            NoteMode::Step => r.notes = [c; MAX_STEPS],
        }
    }

    /// CLEAR down: a full pool, or every step, of notes of `key` in the
    /// ring's register (`register_note`, as MUTATE draws them).
    pub fn scatter(&mut self, key: &Key, rng: &mut Rng) {
        self.renoted();
        let r = &mut self.ring;
        match r.mode {
            NoteMode::Pool => {
                let pool: [Note; MAX_POOL] = core::array::from_fn(|_| register_note(r, key, rng));
                r.pool = Pool::new(&pool).unwrap_or(r.pool);
            }
            NoteMode::Step => {
                for i in 0..MAX_STEPS {
                    r.notes[i] = register_note(r, key, rng);
                }
            }
        }
    }

    /// A new OCTAVE: the stored notes and pool move with it, whole octaves.
    fn transpose(&mut self, to: Octave) {
        let r = &mut self.ring;
        if to == r.octave {
            return;
        }
        let by = 12 * (to.get() as i16 - r.octave.get() as i16);
        r.octave = to;
        for n in &mut r.notes {
            *n = fold(*n, by);
        }
        let len = r.pool.len();
        let mut moved = [r.pool.get(0); MAX_POOL];
        for (m, n) in moved.iter_mut().zip(r.pool.notes()) {
            *m = fold(*n, by);
        }
        r.pool = Pool::new(&moved[..len]).unwrap_or(r.pool);
        self.renoted();
    }
}

impl Block for SceneRing {
    fn specs(&self) -> &'static [ParamSpec] {
        &RING_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        let r = &self.ring;
        let f = r.fill();
        match id {
            Self::STEPS => (f.steps().get() - 1) as f32,
            Self::HITS => f.hits() as f32,
            Self::ROTATE => f.rotate() as f32,
            Self::CHANCE => r.chance.get() as f32,
            Self::MUTATE => r.mutate.get() as f32,
            Self::MUTE => r.mute as u8 as f32,
            Self::GATE => r.gate.get() as f32,
            Self::VEL => r.velocity.get() as f32,
            Self::ACCENT => r.accent as f32,
            Self::OCTAVE => r.octave.get() as f32,
            Self::RANGE => index_of(&Range::ALL, &r.range),
            Self::SNAP => index_of(&SnapTo::ALL, &r.snap),
            Self::MODE => index_of(&NoteMode::ALL, &r.mode),
            Self::WALK => index_of(&Walk::ALL, &r.walk),
            Self::TARGET => self.target.part().index() as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        let i = v as u8;
        let f = self.ring.fill();
        let r = &mut self.ring;
        match id {
            Self::STEPS => {
                self.refill(f.with_steps(Steps::new(i.saturating_add(1)).unwrap_or(Steps::MAX)))
            }
            Self::HITS => {
                if let Some(fill) = Fill::new(f.steps(), i.min(f.steps().get()), f.rotate()) {
                    self.refill(fill);
                }
            }
            // `Fill::new` takes ROTATE mod STEPS.
            Self::ROTATE => {
                if let Some(fill) = Fill::new(f.steps(), f.hits(), i) {
                    self.refill(fill);
                }
            }
            Self::CHANCE => r.chance = Pct::clamped(i),
            Self::MUTATE => r.mutate = Pct::clamped(i),
            Self::MUTE => r.mute = i != 0,
            Self::GATE => r.gate = Pct::clamped(i),
            Self::VEL => r.velocity = Velocity::new(i.clamp(1, 127)).unwrap_or(Velocity::MAX),
            Self::ACCENT => r.accent = i.min(super::MAX_STEPS),
            Self::OCTAVE => {
                if let Some(o) = Octave::new(i.min(8)) {
                    self.transpose(o);
                }
            }
            Self::RANGE => r.range = pick(&Range::ALL, v),
            Self::SNAP => r.snap = pick(&SnapTo::ALL, v),
            Self::MODE => r.mode = pick(&NoteMode::ALL, v),
            Self::WALK => r.walk = pick(&Walk::ALL, v),
            Self::TARGET => self.target = Target::Part(pick(&PartId::ALL, v)),
            _ => {}
        }
    }

    /// ROTATE wraps both ways at the ring's STEPS; the rest step and clamp
    /// as their specs say.
    fn nudge(&mut self, id: ParamId, delta: i8) {
        let Some(s) = self.spec(id) else { return };
        if id == Self::ROTATE {
            let f = self.ring.fill();
            let to = (f.rotate() as i16 + delta as i16).rem_euclid(f.steps().get() as i16);
            self.write(id, to as f32);
        } else {
            self.set(id, self.get(id) + delta as f32 * s.step);
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        let r = &self.ring;
        match id {
            Self::RANGE => Some(r.range.disk_code()),
            Self::SNAP => Some(r.snap.disk_code()),
            Self::MODE => Some(r.mode.disk_code()),
            Self::WALK => Some(r.walk.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        let r = &self.ring;
        match id {
            Self::RANGE => Some(r.range.disk_ident()),
            Self::SNAP => Some(r.snap.disk_ident()),
            Self::MODE => Some(r.mode.disk_ident()),
            Self::WALK => Some(r.walk.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        let r = &mut self.ring;
        match id {
            Self::RANGE => apply_code(Range::from_disk_code(code), |x| r.range = x),
            Self::SNAP => apply_code(SnapTo::from_disk_code(code), |x| r.snap = x),
            Self::MODE => apply_code(NoteMode::from_disk_code(code), |x| r.mode = x),
            Self::WALK => apply_code(Walk::from_disk_code(code), |x| r.walk = x),
            _ => false,
        }
    }
}

impl ChordRing {
    pub const KEY: ParamId = ParamId(0);
    pub const SCALE: ParamId = ParamId(1);
    pub const BARS: ParamId = ParamId(2);
    pub const SEGS: ParamId = ParamId(3);
}

impl Block for ChordRing {
    fn specs(&self) -> &'static [ParamSpec] {
        &CHORD_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::KEY => index_of(&PitchClass::ALL, &self.tonic),
            Self::SCALE => index_of(&ScaleKind::ALL, &self.scale),
            Self::BARS => (self.bars.get() - 1) as f32,
            Self::SEGS => (self.segs.get() - 1) as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        let n = (v as u8).saturating_add(1);
        match id {
            Self::KEY => self.tonic = pick(&PitchClass::ALL, v),
            Self::SCALE => {
                let to = pick(&ScaleKind::ALL, v);
                // A fresh CUSTOM set starts as the scale picked before it;
                // a set no named scale is, the user's own, stays.
                if to == ScaleKind::Custom
                    && self.scale != ScaleKind::Custom
                    && Scale::ALL.contains(&self.custom)
                {
                    self.custom = self.scale.scale(self.custom);
                }
                self.scale = to;
            }
            Self::BARS => self.bars = Bars::new(n.min(Bars::MAX)).unwrap_or(Bars::ONE),
            Self::SEGS => self.segs = Segs::new(n.min(MAX_SEGS as u8)).unwrap_or(Segs::ONE),
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::KEY => Some(self.tonic.disk_code()),
            Self::SCALE => Some(self.scale.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::KEY => Some(self.tonic.disk_ident()),
            Self::SCALE => Some(self.scale.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::KEY => apply_code(PitchClass::from_disk_code(code), |p| self.tonic = p),
            Self::SCALE => apply_code(ScaleKind::from_disk_code(code), |k| self.scale = k),
            _ => false,
        }
    }
}

impl Bpm {
    pub const BPM: ParamId = ParamId(0);
}

/// The song's tempo as a block (`BlockRef::Tempo`).
impl Block for Bpm {
    fn specs(&self) -> &'static [ParamSpec] {
        &TEMPO_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::BPM => Bpm::get(*self) as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        if id == Self::BPM {
            *self = Bpm::clamped(v as u16);
        }
    }
}

/// The CUSTOM set as an `OrbitScale` record: scene, then the set as `u16`
/// LE, bit n the note n semitones above the root.
pub fn encode_scale(scene: u8, s: Scale) -> [u8; 3] {
    let bits = (0..12).fold(0u16, |m, i| m | (s.contains(Interval::ALL[i]) as u16) << i);
    let [lo, hi] = bits.to_le_bytes();
    [scene, lo, hi]
}

/// An `OrbitScale` payload: the root is set whatever the file says, and
/// bits past the 12 are dropped.
pub fn decode_scale(p: &[u8]) -> Option<(u8, Scale)> {
    let &[scene, lo, hi] = p else { return None };
    let bits = u16::from_le_bytes([lo, hi]);
    let mut set = [Interval::UNISON; 12];
    let mut n = 0;
    for (i, &iv) in Interval::ALL.iter().enumerate() {
        if bits >> i & 1 == 1 {
            set[n] = iv;
            n += 1;
        }
    }
    Some((scene, Scale::custom(&set[..n])))
}
