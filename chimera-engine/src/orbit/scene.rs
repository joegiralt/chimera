//! A scene: everything a loop plays, as plain data; and the song a
//! project keeps, edited through the ORBIT blocks.

use super::channel::RingMask;
use super::chords::{Bars, ChordRing, ScaleKind, Segs};
use super::clock::Bpm;
use super::euclid::{Fill, Steps};
use super::mutate::register_note;
use super::ring::{NoteMode, Octave, Range, Ring, SnapTo};
use super::rng::{Pct, Rng};
use super::runner::{Capture, Captured, OrbitView};
use super::walk::{Pool, Walk};
use super::{MAX_POOL, MAX_SEGS, RINGS, RingIdx};
use crate::Velocity;
use crate::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use crate::block::{Block, DiskCode, OFF_ON, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::modulator::law::Law;
use crate::dsp::modulator::pick;
use crate::glyph::FocusGlyph;
use crate::in_place::uninit_at;
use crate::project::{Confirmed, PartId, SceneAsk, SceneSlot};
use chimera_theory::{Chord, Degree, Interval, Key, Note, PitchClass, Scale};
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

pub const SCENES: usize = 8;

crate::project::bounded_index!(
    /// A scene slot, 0..8.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    SceneIdx < SCENES
);

impl SceneIdx {
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A set of scene slots, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SceneSet(u8);

const _: () = assert!(SCENES <= u8::BITS as usize);

impl SceneSet {
    pub const EMPTY: SceneSet = SceneSet(0);

    pub const fn with(self, n: SceneIdx) -> SceneSet {
        SceneSet(self.0 | 1 << n.0)
    }

    pub const fn without(self, n: SceneIdx) -> SceneSet {
        SceneSet(self.0 & !(1 << n.0))
    }

    pub const fn contains(self, n: SceneIdx) -> bool {
        self.0 & 1 << n.0 != 0
    }

    /// Bit n slot n.
    pub const fn bits(self) -> u8 {
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

/// What an edit changed in a ring since the UI last sent its edits: its
/// rhythm (the hits), its notes (and pool), or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Touched(u8);

impl Touched {
    pub const NONE: Touched = Touched(0);
    pub const RHYTHM: Touched = Touched(1);
    pub const NOTES: Touched = Touched(2);

    pub const fn contains(self, t: Touched) -> bool {
        self.0 & t.0 == t.0
    }

    pub const fn with(self, t: Touched) -> Touched {
        Touched(self.0 | t.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SceneRing {
    pub ring: Ring,
    pub target: Target,
    /// Set when the rhythm or the notes change under the player's feet,
    /// until the UI sends the edit (ADR 0082). Run-time only: never saved,
    /// and no part of equality.
    pub touched: Touched,
}

/// Equal as a project keeps it: `touched` is the runner's handshake, so a
/// scene loaded or reverted is the scene saved.
impl PartialEq for SceneRing {
    fn eq(&self, o: &Self) -> bool {
        self.ring == o.ring && self.target == o.target
    }
}

impl Eq for SceneRing {}

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
        touched: Touched::NONE,
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
    /// `salt`, which the UI moves on each turn, so each rolls new ones.
    pub fn scatter(&mut self, r: RingIdx, salt: u8) {
        let key = self.chords.key();
        let seed = self.seed.wrapping_add(u32::from(salt) << 16);
        self.rings[r.get() as usize].scatter(&key, &mut Rng::for_ring(seed, r));
    }

    /// The rings touched since the last harvest, as the edits to send:
    /// (rhythm, notes). Clears them.
    pub fn harvest(&mut self) -> (RingMask, RingMask) {
        let (mut rhythm, mut notes) = (RingMask::EMPTY, RingMask::EMPTY);
        for (r, s) in RingIdx::ALL.into_iter().zip(&mut self.rings) {
            if s.touched.contains(Touched::RHYTHM) {
                rhythm = rhythm.with(r);
            }
            if s.touched.contains(Touched::NOTES) {
                notes = notes.with(r);
            }
            s.touched = Touched::NONE;
        }
        (rhythm, notes)
    }

    /// No edit waits to be harvested.
    pub fn untouched(&self) -> bool {
        self.rings.iter().all(|s| s.touched == Touched::NONE)
    }

    /// A capture into its target (spec § 5.3): a segment's chord over its
    /// lowest note, or a ring's notes.
    pub fn take(&mut self, c: &Captured) {
        match c.target {
            Capture::Off => {}
            Capture::Segment(s) => {
                // None past SEGS: it shrank under the capture.
                let segs = self.chords.segs.get() as usize;
                if let (Some(p), Some(ch)) = (
                    self.chords.played[..segs].get_mut(s as usize),
                    Chord::from_notes(c.notes()),
                ) {
                    *p = Some(ch);
                }
            }
            Capture::Ring(r) => self.rings[r.get() as usize].take(c.notes()),
        }
    }

    /// `neutral()`'s, compared out of line: one copy of `==` in flash.
    #[inline(never)]
    pub fn is_neutral(&self) -> bool {
        *self == NEUTRAL
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

/// The neutral scene, once in flash: a load's base and `saved`'s measure.
pub static NEUTRAL: Scene = Scene::neutral();

/// What a project keeps of ORBIT: the eight scenes, the one playing and
/// any queued, the scene being edited and played (`live`, its drift
/// absorbed), and the tempo.
///
/// Each slot is its scene's working copy: the user's edits, never the
/// drift (ADR 0083). The slots are written only here: `edit` into the
/// playing one, `save_to`, and a confirmed `overwrite_scene` or
/// `clear_scene`, each moving the slot's rev, which is what a
/// `Confirmed<SceneSlot>` witnesses.
#[derive(Clone, Copy, Debug)]
pub struct OrbitSong {
    /// The playing scene: what the pages show and the runner plays, its
    /// slot plus the drift. A switch replaces it with the slot when it
    /// lands (ADR 0077, 0083). Private: `edit` is the one way to change
    /// it, and writes the slot too.
    live: Scene,
    pub bpm: Bpm,
    scenes: [Scene; SCENES],
    /// Moves on every write to its slot; run-time only, never saved.
    slot_revs: [u16; SCENES],
    /// The slots holding a scene, not the neutral one. Kept on each
    /// write, so no frame compares whole scenes.
    saved: SceneSet,
    current: SceneIdx,
    /// A switch waiting on the runner: it lands on the loop.
    queued: Option<SceneIdx>,
    /// A CLEAR or RELOAD waits to land the playing slot again: SCENE
    /// turned away and back keeps it queued. Run-time only.
    reland_due: bool,
}

crate::in_place::field_list!(OrbitSong => OrbitSong { live, bpm, scenes, slot_revs, saved, current, queued, reland_due });

/// Equal as a project keeps it: the slots' revs are run-time only, and
/// `saved` follows from the slots.
impl PartialEq for OrbitSong {
    fn eq(&self, o: &Self) -> bool {
        self.live == o.live
            && self.bpm == o.bpm
            && self.scenes == o.scenes
            && self.current == o.current
            && self.queued == o.queued
    }
}

impl Eq for OrbitSong {}

/// NEW's slot `i`: the init scene first, the rest neutral.
fn new_scene(i: usize) -> Scene {
    if i == 0 { Scene::init() } else { NEUTRAL }
}

/// A save or an overwrite refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A switch is queued: what lands stays what was queued, and a save
    /// waits for it.
    SwitchPending,
    /// The slot was written since its prompt opened.
    SlotChanged,
}

/// What SCN's SAVE did.
#[derive(Debug)]
pub enum SceneSave {
    Saved(SceneIdx),
    /// Another saved scene: it asks first.
    Ask(SceneAsk),
}

impl OrbitSong {
    /// NEW's: the init scene in slot 1, playing, the rest neutral, at 120
    /// BPM.
    pub const NEW: OrbitSong = OrbitSong {
        live: Scene::init(),
        bpm: Bpm::DEFAULT,
        scenes: {
            let mut s = [Scene::neutral(); SCENES];
            s[0] = Scene::init();
            s
        },
        slot_revs: [0; SCENES],
        saved: SceneSet::EMPTY.with(SceneIdx(0)),
        current: SceneIdx(0),
        queued: None,
        reland_due: false,
    };

    /// NEW (`NEW`'s value), built in place a scene at a time: the firmware
    /// keeps no nine-scene constant in flash.
    pub fn init_in_place(slot: &mut MaybeUninit<OrbitSong>) -> &mut OrbitSong {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every field, and each of the
        // scenes, is written once before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).live).write(Scene::init());
            addr_of_mut!((*p).bpm).write(Bpm::DEFAULT);
            let scenes = addr_of_mut!((*p).scenes).cast::<Scene>();
            for i in 0..SCENES {
                scenes.add(i).write(new_scene(i));
            }
            addr_of_mut!((*p).slot_revs).write([0; SCENES]);
            addr_of_mut!((*p).saved).write(SceneSet::EMPTY.with(SceneIdx(0)));
            addr_of_mut!((*p).current).write(SceneIdx(0));
            addr_of_mut!((*p).queued).write(None);
            addr_of_mut!((*p).reland_due).write(false);
            slot.assume_init_mut()
        }
    }

    /// Back to NEW in place. Every slot is written, so each one's rev
    /// moves: a confirmation from before refuses.
    pub fn reset(&mut self) {
        let revs = self.slot_revs;
        // SAFETY: `self` is valid, aligned and unaliased, and `OrbitSong`
        // is `Copy`: overwriting it drops nothing.
        Self::init_in_place(unsafe { uninit_at(self) });
        self.slot_revs = revs.map(|r| r.wrapping_add(1));
    }

    /// The slots holding a scene: the rest are neutral.
    pub fn saved(&self) -> SceneSet {
        self.saved
    }

    pub fn scene(&self, n: SceneIdx) -> &Scene {
        &self.scenes[n.0 as usize]
    }

    /// The scene playing, as the runner last reported landing it.
    pub fn current(&self) -> SceneIdx {
        self.current
    }

    pub fn queued(&self) -> Option<SceneIdx> {
        self.queued
    }

    /// Slot `n`'s writes so far, wrapping: a `Confirmed<SceneSlot>`'s witness.
    pub(crate) fn slot_rev(&self, n: SceneIdx) -> u16 {
        self.slot_revs[n.0 as usize]
    }

    /// SCENE turned to `s`: queued, to land on the loop; `live` stays the
    /// playing scene till then. Back to the playing scene cancels it,
    /// unless a CLEAR or RELOAD is due: that stays queued, so `live` and
    /// the slot meet again.
    pub fn queue(&mut self, s: SceneIdx) {
        self.queued = (s != self.current || self.reland_due).then_some(s);
    }

    /// The runner adopted `s` (the view says so): it plays, and `live`
    /// becomes a copy of its slot, its edits kept and its drift restarted.
    /// Landing the playing slot again, every ring is marked touched: the
    /// runner took the slot a moment before, and an edit since goes with
    /// them.
    pub fn land(&mut self, s: SceneIdx) {
        if self.queued == Some(s) {
            self.live = self.scenes[s.0 as usize];
            if s == self.current {
                for r in &mut self.live.rings {
                    r.touched = Touched::RHYTHM.with(Touched::NOTES);
                }
            }
            self.current = s;
            self.queued = None;
            self.reland_due = false;
        }
    }

    /// A capture into the scene it was played in (ADR 0083): its slot, and
    /// `live` while that scene plays. Into another slot its touched marks
    /// stay with it: the runner may have adopted that slot already, so they
    /// go out when it lands.
    pub fn capture(&mut self, c: &Captured) {
        if c.scene == self.current {
            self.edit(&mut |s| s.take(c));
        } else {
            self.scenes[c.scene.0 as usize].take(c);
            self.moved(c.scene);
        }
    }

    /// An edit to the playing scene: `f` on its slot, then on `live`, so
    /// a switch away and back keeps it (ADR 0083). While a switch waits,
    /// still the playing scene's. Out of line, `f` behind a `dyn`: one
    /// copy in flash.
    #[inline(never)]
    pub fn edit(&mut self, f: &mut dyn FnMut(&mut Scene)) {
        let n = self.current;
        f(&mut self.scenes[n.0 as usize]);
        self.wrote(n);
        f(&mut self.live);
    }

    /// A scene param's encoder turned by `delta` (snapped, with MIX):
    /// an `edit`, so on `live` and its slot alike.
    #[inline(never)]
    pub fn turn(&mut self, a: ParamAddr, delta: i8, snap: bool) {
        self.edit(&mut |s| {
            if let Some(b) = s.block_mut(a.block) {
                if snap {
                    b.snap(a.param, delta);
                } else {
                    b.nudge(a.param, delta);
                }
            }
        });
    }

    /// The playing scene as it plays: its slot plus the drift.
    pub fn live(&self) -> &Scene {
        &self.live
    }

    /// `live`'s edits since the last harvest (`Scene::harvest`).
    pub fn harvest(&mut self) -> (RingMask, RingMask) {
        self.live.harvest()
    }

    /// `live` past its slot: a test's drift, or a `live` that differs
    /// from its slot as a load can leave it. Never the firmware's.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn live_mut_for_test(&mut self) -> &mut Scene {
        &mut self.live
    }

    /// `live` over slot `n`.
    fn write(&mut self, n: SceneIdx) {
        self.put(n, self.live);
    }

    /// `s` into slot `n`, its rev moved and `saved` kept.
    fn put(&mut self, n: SceneIdx, s: Scene) {
        self.scenes[n.0 as usize] = s;
        self.wrote(n);
    }

    /// Slot `n` written: `moved`, and its touched marks dropped, as they
    /// are `live`'s to send.
    fn wrote(&mut self, n: SceneIdx) {
        self.scenes[n.0 as usize].harvest();
        self.moved(n);
    }

    /// Slot `n` changed: its rev moved, `saved` kept.
    fn moved(&mut self, n: SceneIdx) {
        let i = n.0 as usize;
        self.slot_revs[i] = self.slot_revs[i].wrapping_add(1);
        self.saved = if self.scenes[i].is_neutral() {
            self.saved.without(n)
        } else {
            self.saved.with(n)
        };
    }

    /// `live` differs from the playing slot: drift since the scene last
    /// landed (or an edit to `live` alone), which RELOAD restarts.
    pub fn edited(&self) -> bool {
        self.live != self.scenes[self.current.0 as usize]
    }

    /// The playing slot queued to land again, like a switch: `live` becomes
    /// its copy on the loop.
    fn reland(&mut self) {
        self.queued = Some(self.current);
        self.reland_due = true;
    }

    /// SETTINGS › ORBIT's CLEAR, answered: the slot it asked about to
    /// neutral, unless written since. The playing one also lands again, so
    /// `live` goes neutral on the loop. Refused while a switch waits: what
    /// lands stays what was queued, and the prompt's "on the loop" holds.
    pub fn clear_scene(&mut self, yes: Confirmed<SceneSlot>) -> Result<(), Refused> {
        if self.queued.is_some() {
            return Err(Refused::SwitchPending);
        }
        if !yes.holds(self) {
            return Err(Refused::SlotChanged);
        }
        let n = yes.target().index();
        self.put(n, NEUTRAL);
        if n == self.current {
            self.reland();
        }
        Ok(())
    }

    /// SETTINGS › ORBIT's RELOAD FROM PROJ, answered: `live` back to the
    /// playing slot, queued like a switch, so the scene restarts on the
    /// loop from its edits, its drift dropped (ADR 0083). Refused while a
    /// switch waits, and for a slot written since, or no longer playing.
    pub fn reload_scene(&mut self, yes: Confirmed<SceneSlot>) -> Result<(), Refused> {
        if self.queued.is_some() {
            return Err(Refused::SwitchPending);
        }
        if !yes.holds(self) || yes.target().index() != self.current {
            return Err(Refused::SlotChanged);
        }
        self.reland();
        Ok(())
    }

    /// SCN's SAVE → `n`: at once over the playing slot or a neutral one;
    /// over another saved scene it asks.
    /// Only ever refused as `SwitchPending`.
    pub fn save_to(&mut self, n: SceneIdx) -> Result<SceneSave, Refused> {
        if self.queued.is_some() {
            return Err(Refused::SwitchPending);
        }
        if n != self.current && self.saved.contains(n) {
            return Ok(SceneSave::Ask(SceneAsk::of(self, n)));
        }
        self.write(n);
        Ok(SceneSave::Saved(n))
    }

    /// SAVE OVER, answered: `live` over the slot it asked about, unless the
    /// slot was written since.
    pub fn overwrite_scene(&mut self, yes: Confirmed<SceneSlot>) -> Result<(), Refused> {
        if self.queued.is_some() {
            return Err(Refused::SwitchPending);
        }
        if !yes.holds(self) {
            return Err(Refused::SlotChanged);
        }
        self.write(yes.target().index());
        Ok(())
    }

    /// Each ring's drift from `v` into `live`, never its slot: its hits,
    /// notes and pool, nothing marked touched. `OrbitUi::sync` gates it: only a view of the
    /// scene `live` is, with every edit applied (ADR 0082).
    pub fn absorb(&mut self, v: &OrbitView) {
        for (s, r) in self.live.rings.iter_mut().zip(&v.rings) {
            if let Some(ring) = s.ring.with_mask(r.hits) {
                s.ring = ring;
            }
            s.ring.notes = r.notes;
            s.ring.pool = r.pool;
        }
    }

    /// What a save writes as playing, and as `live`: while a switch waits,
    /// the queued slot, as if it had landed (Decisions).
    pub fn landing(&self) -> (SceneIdx, &Scene) {
        match self.queued {
            Some(q) => (q, &self.scenes[q.0 as usize]),
            None => (self.current, &self.live),
        }
    }

    /// A load's base (spec § 8): every slot and `live` neutral, 120 BPM,
    /// slot 1 playing, nothing queued. Every slot is written, so each
    /// one's rev moves: no confirmation from before the load holds after.
    pub(crate) fn clear_for_load(&mut self) {
        for (s, rev) in self.scenes.iter_mut().zip(&mut self.slot_revs) {
            *s = NEUTRAL;
            *rev = rev.wrapping_add(1);
        }
        self.saved = SceneSet::EMPTY;
        self.live = NEUTRAL;
        self.bpm = Bpm::DEFAULT;
        self.current = SceneIdx(0);
        self.queued = None;
        self.reland_due = false;
    }

    /// Scene `i` as a project file numbers it: the slots 0–7, `live` 8.
    /// A load's writes: `loaded` keeps `saved` once they are done.
    pub(crate) fn stored_mut(&mut self, i: u8) -> Option<&mut Scene> {
        match SceneIdx::new(i) {
            Some(n) => Some(&mut self.scenes[n.0 as usize]),
            None => (i == STORED_LIVE).then_some(&mut self.live),
        }
    }

    /// Scene `i` as a project file numbers it.
    pub(crate) fn stored(&self, i: u8) -> Option<&Scene> {
        match SceneIdx::new(i) {
            Some(n) => Some(&self.scenes[n.0 as usize]),
            None => (i == STORED_LIVE).then_some(&self.live),
        }
    }

    /// The loaded file's playing scene.
    pub(crate) fn set_current(&mut self, n: SceneIdx) {
        self.current = n;
    }

    /// A load done: `saved` from the slots as read.
    pub(crate) fn loaded(&mut self) {
        self.saved = SceneIdx::ALL
            .into_iter()
            .filter(|n| !self.scenes[n.0 as usize].is_neutral())
            .fold(SceneSet::EMPTY, SceneSet::with);
    }
}

/// `live`'s number in a project file, after the eight slots (spec § 8).
pub const STORED_LIVE: u8 = SCENES as u8;

/// A scene's blocks, as an `OrbitBlock` record reaches them.
impl BlockRead for Scene {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        match b {
            BlockRef::OrbitChords => Some(&self.chords),
            BlockRef::OrbitRing(r) => Some(&self.rings[r.get() as usize]),
            _ => None,
        }
    }
}

impl Blocks for Scene {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::OrbitChords => Some(&mut self.chords),
            BlockRef::OrbitRing(r) => Some(&mut self.rings[r.get() as usize]),
            _ => None,
        }
    }
}

const MAX_STEPS: usize = super::MAX_STEPS as usize;

/// The theory's names for the named scales, in `ScaleKind`'s order, then
/// CUSTOM.
const SCALE_NAMES: [&str; 11] = {
    let mut n = ["CUSTOM"; 11];
    let mut i = 0;
    while i < chimera_theory::SCALE_NAMES.len() {
        n[i] = chimera_theory::SCALE_NAMES[i];
        i += 1;
    }
    n
};
const RANGE_NAMES: [&str; 4] = ["0", "1", "2", "3"];
const SNAP_NAMES: [&str; 2] = ["SCALE", "CHORD"];
const MODE_NAMES: [&str; 2] = ["STEP", "POOL"];
/// PING-PONG as `PING-PG`: a cell holds no more.
const WALK_NAMES: [&str; 4] = ["UP", "DOWN", "RANDOM", "PING-PG"];

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
    ParamSpec::choice(
        0,
        "KEY",
        ValFmt::Names(&chimera_theory::PITCH_NAMES),
        11.0,
        0.0,
    )
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
            self.touched = self.touched.with(Touched::RHYTHM);
        }
    }

    /// The notes changed under the player: it takes them, drift and all.
    fn renoted(&mut self) {
        self.touched = self.touched.with(Touched::NOTES);
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

    /// Notes played in: a POOL ring's pool, sorted; a STEP ring's hits
    /// in the order played, each rest after a hit at its note (the steps
    /// before the first, the last hit's). OCTAVE becomes the lowest's, the
    /// notes left where they were played.
    pub fn take(&mut self, played: &[Note]) {
        let Some(&low) = played.iter().min() else {
            return;
        };
        self.renoted();
        let r = &mut self.ring;
        r.octave = Octave::of(low);
        match r.mode {
            NoteMode::Pool => r.pool = Pool::new(played).unwrap_or(r.pool),
            NoteMode::Step => {
                let mask = r.hits_mask();
                let at = |k: usize| played[k % played.len()];
                let mut note = at((mask.hits() as usize).saturating_sub(1));
                let steps = mask.steps().get() as usize;
                for (i, n) in r.notes[..steps].iter_mut().enumerate() {
                    if let Some(k) = mask.hit_index(i) {
                        note = at(k);
                    }
                    *n = note;
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

/// RANGE, SNAP, MODE and WALK, the ring's enum params: `$m!(Type, field)`
/// for the one `$id` names, `None` for any other. One list for `get`,
/// `write` and the disk code's three.
macro_rules! ring_enum {
    ($id:expr, $m:ident) => {
        match $id {
            SceneRing::RANGE => Some($m!(Range, range)),
            SceneRing::SNAP => Some($m!(SnapTo, snap)),
            SceneRing::MODE => Some($m!(NoteMode, mode)),
            SceneRing::WALK => Some($m!(Walk, walk)),
            _ => None,
        }
    };
}

impl Block for SceneRing {
    fn specs(&self) -> &'static [ParamSpec] {
        &RING_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        let r = &self.ring;
        macro_rules! at {
            ($t:ident, $f:ident) => {
                index_of(&$t::ALL, &r.$f)
            };
        }
        if let Some(v) = ring_enum!(id, at) {
            return v;
        }
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
            Self::ACCENT => r.accent() as f32,
            Self::OCTAVE => r.octave.get() as f32,
            Self::TARGET => self.target.part().index() as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        let r = &mut self.ring;
        macro_rules! put {
            ($t:ident, $f:ident) => {
                r.$f = pick(&$t::ALL, v)
            };
        }
        if ring_enum!(id, put).is_some() {
            return;
        }
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
            Self::ACCENT => *r = r.with_accent(i.min(super::MAX_STEPS)),
            Self::OCTAVE => {
                if let Some(o) = Octave::new(i.min(8)) {
                    self.transpose(o);
                }
            }
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
            self.write(id, s.nudged(self.get(id), delta));
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        let r = &self.ring;
        macro_rules! code {
            ($t:ident, $f:ident) => {
                r.$f.disk_code()
            };
        }
        ring_enum!(id, code)
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        let r = &self.ring;
        macro_rules! ident {
            ($t:ident, $f:ident) => {
                r.$f.disk_ident()
            };
        }
        ring_enum!(id, ident)
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        let r = &mut self.ring;
        macro_rules! set {
            ($t:ident, $f:ident) => {
                apply_code($t::from_disk_code(code), |x| r.$f = x)
            };
        }
        ring_enum!(id, set).unwrap_or(false)
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
