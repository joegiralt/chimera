//! The stream rings: three slots a voice, each a chunk of packed samples
//! behind a few guard frames copied from the chunk before, so a cursor's
//! interpolation reads across the seam from one slot. Three slots, so a
//! read late by up to a chunk lands while the head still plays the one
//! before.
//!
//! `Rings` (SRAM4) holds every slot's word and guard flag and slots A and
//! B's bytes; slot C's bytes are `ThirdSlots`, which SRAM4 has no room
//! for (the board keeps them in D2).
//!
//! Two sides, each handed out once (`Rings::sides`): the audio's, which
//! wants, cancels and reads, and the card's, the one writer of slot bytes.
//! A fill needs a `Claim`, a read a `Ready`; both are checked against the
//! slot word's generation, so neither side touches bytes the other owns.

use core::cell::UnsafeCell;
use core::ptr;
#[cfg(not(loom))]
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use chimera_hal::units::Bytes;
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::layout::{CHUNK_BYTES, ChunkIdx};
use super::slot::{Finish, Slot, SlotState};
use super::units::{AudioBlocks, Frames, Generation, Packing, SlotIdx};
use crate::hw::MAX_VOICES;
use crate::project::SourceId;
use crate::storage::twelve::unpack12;
use crate::voice_alloc::VoiceIdx;

/// Frames of the chunk before, ahead of a slot's own.
pub const GUARD_FRAMES: Frames = Frames::new(4);
pub const GUARD_BYTES: Bytes = GUARD_FRAMES.to_bytes(Packing::Twelve);
/// The guard, then a chunk (packed, not framed).
pub const SLOT_BYTES: usize = match GUARD_BYTES.checked_add(CHUNK_BYTES) {
    Some(b) => b.get() as usize,
    None => panic!(),
};
const GUARD: usize = GUARD_BYTES.get() as usize;
// A whole number of 12-bit pairs, so the chunk starts on one.
const _: () = assert!(GUARD_FRAMES.get().is_multiple_of(2));
const _: () = assert!(SLOT_BYTES == 4_038);
// 0x3800_FFFC stays free for ADR 0068's possible marker word (M7).
#[cfg(not(loom))]
const _: () = assert!(size_of::<Rings>() <= crate::hw::SRAM4 - 4);

/// Whether a Ready slot's guard holds the chunk before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    Guarded,
    Unguarded,
}

impl Guard {
    // Unguarded is 0: what a zeroed or freshly claimed slot says.
    const fn from_bits(b: u8) -> Guard {
        if b == 1 {
            Guard::Guarded
        } else {
            Guard::Unguarded
        }
    }

    const fn bits(self) -> u8 {
        match self {
            Guard::Guarded => 1,
            Guard::Unguarded => 0,
        }
    }
}

/// A source id in an atomic byte: its index + 1, 0 none.
pub(super) struct AtomicTag(AtomicU8);

impl AtomicTag {
    #[cfg(not(loom))]
    const fn none() -> AtomicTag {
        AtomicTag(AtomicU8::new(0))
    }

    #[cfg(loom)]
    fn none() -> AtomicTag {
        AtomicTag(AtomicU8::new(0))
    }

    /// `id`'s tag: never 0, nor `u8::MAX`.
    pub(super) const fn encode(id: SourceId) -> u8 {
        id.index() as u8 + 1
    }

    pub(super) fn decode(t: u8) -> Option<SourceId> {
        SourceId::new(t.checked_sub(1)?)
    }

    fn load(&self, o: Ordering) -> Option<SourceId> {
        Self::decode(self.0.load(o))
    }

    fn store(&self, id: SourceId, o: Ordering) {
        self.0.store(Self::encode(id), o);
    }
}

type SlotCell = UnsafeCell<[u8; SLOT_BYTES]>;

/// Slots a ring.
pub const SLOTS: usize = SlotIdx::ALL.len();

pub struct Ring {
    slots: [Slot; SLOTS],
    /// Slots A and B's; C's are in `ThirdSlots`.
    bytes: [SlotCell; 2],
    guard: [AtomicU8; SLOTS],
    /// The source its wants read.
    source: AtomicTag,
}

// Every id + 1 fits the byte, 0 left for none.
const _: () = assert!(SourceId::ALL.len() < u8::MAX as usize);

/// Each voice's slot C bytes. All-zero bytes are valid, not yet taken.
pub struct ThirdSlots {
    bytes: [SlotCell; MAX_VOICES],
    taken: AtomicBool,
}

// SAFETY: as `Ring`'s: the cells are reached only through the one pair of
// sides `Rings::sides` hands out with them (`taken` lets one pair).
unsafe impl Sync for ThirdSlots {}

#[cfg(not(loom))]
impl Default for ThirdSlots {
    fn default() -> Self {
        Self::new()
    }
}

impl ThirdSlots {
    #[cfg(not(loom))]
    pub const fn new() -> ThirdSlots {
        ThirdSlots {
            bytes: [const { UnsafeCell::new([0; SLOT_BYTES]) }; MAX_VOICES],
            taken: AtomicBool::new(false),
        }
    }

    #[cfg(loom)]
    pub fn new() -> ThirdSlots {
        ThirdSlots {
            bytes: core::array::from_fn(|_| UnsafeCell::new([0; SLOT_BYTES])),
            taken: AtomicBool::new(false),
        }
    }
}

// SAFETY: `bytes` (and `ThirdSlots`') are written only through the one
// `CardSide` (`sides` hands it out once, with both flags), under a `Claim`
// whose slot is still Filling under its generation (`fill_buf`,
// `write_guard` check it); a slot leaves Filling only by the card's own
// `&mut self` calls or the audio's cancel, and nothing reads a Filling or
// Empty slot. They are read only through a `Ready`, which exists only
// while its slot is Ready under its own generation, and borrows the
// block's `AudioTurn`: the card never moves a Ready slot, and the audio's
// `release` and `cancel` need `&mut AudioTurn`. The `Release` publish and
// `Acquire` load of the slot word order the bytes (slot.rs).
// `write_guard` reads another slot while it is Ready, where only reads
// happen.
unsafe impl Sync for Ring {}

impl Ring {
    #[cfg(not(loom))]
    const fn new() -> Ring {
        Ring {
            slots: [const { Slot::new() }; SLOTS],
            bytes: [const { UnsafeCell::new([0; SLOT_BYTES]) }; 2],
            guard: [const { AtomicU8::new(0) }; SLOTS],
            source: AtomicTag::none(),
        }
    }

    #[cfg(loom)]
    fn new() -> Ring {
        Ring {
            slots: core::array::from_fn(|_| Slot::new()),
            bytes: core::array::from_fn(|_| UnsafeCell::new([0; SLOT_BYTES])),
            guard: core::array::from_fn(|_| AtomicU8::new(0)),
            source: AtomicTag::none(),
        }
    }
}

/// A ring a voice. All-zero bytes are a valid `Rings`: every slot Empty
/// at generation 0, Unguarded, its sides not yet taken. The board builds
/// it so, in place in SRAM4, and its `ThirdSlots` in D2.
pub struct Rings {
    rings: [Ring; MAX_VOICES],
    taken: AtomicBool,
}

#[cfg(not(loom))]
impl Default for Rings {
    fn default() -> Self {
        Self::new()
    }
}

impl Rings {
    #[cfg(not(loom))]
    pub const fn new() -> Rings {
        Rings {
            rings: [const { Ring::new() }; MAX_VOICES],
            taken: AtomicBool::new(false),
        }
    }

    /// loom's atomics aren't `const`.
    #[cfg(loom)]
    pub fn new() -> Rings {
        Rings {
            rings: core::array::from_fn(|_| Ring::new()),
            taken: AtomicBool::new(false),
        }
    }

    /// The audio's side and the card's, once, over these rings and
    /// `third`; `None` if either was taken, the other then left as it was.
    pub fn sides<'r>(&'r self, third: &'r ThirdSlots) -> Option<(AudioSide<'r>, CardSide<'r>)> {
        if self.taken.swap(true, Ordering::AcqRel) {
            return None;
        }
        if third.taken.swap(true, Ordering::AcqRel) {
            self.taken.store(false, Ordering::Release);
            return None;
        }
        let at = Store { rings: self, third };
        Some((
            AudioSide { at },
            CardSide {
                at,
                live: [[false; SlotIdx::ALL.len()]; MAX_VOICES],
            },
        ))
    }

    /// Every Wanted slot and its deadline, for EDF.
    pub fn wants(&self) -> impl Iterator<Item = (VoiceIdx, SlotIdx, AudioBlocks)> + '_ {
        VoiceIdx::ALL.into_iter().flat_map(move |v| {
            SlotIdx::ALL
                .into_iter()
                .filter_map(move |s| match self.slot(v, s).seen() {
                    SlotState::Wanted { deadline, .. } => Some((v, s, deadline)),
                    _ => None,
                })
        })
    }

    /// Slot `s` of voice `v`'s state, as the card sees it.
    pub fn seen(&self, v: VoiceIdx, s: SlotIdx) -> SlotState {
        self.slot(v, s).seen()
    }

    /// The source voice `v`'s wants read (`AudioTurn::stream`). Read
    /// after a claim, whose `Acquire` saw the want's `Release`: the
    /// source its note set before it.
    pub fn source(&self, v: VoiceIdx) -> Option<SourceId> {
        self.rings[v.index()].source.load(Ordering::Relaxed)
    }

    fn slot(&self, v: VoiceIdx, s: SlotIdx) -> &Slot {
        &self.rings[v.index()].slots[s.index()]
    }

    fn guard(&self, v: VoiceIdx, s: SlotIdx) -> &AtomicU8 {
        &self.rings[v.index()].guard[s.index()]
    }
}

/// The rings and their third slots, as both sides reach them.
#[derive(Clone, Copy)]
struct Store<'r> {
    rings: &'r Rings,
    third: &'r ThirdSlots,
}

impl<'r> Store<'r> {
    fn slot(self, v: VoiceIdx, s: SlotIdx) -> &'r Slot {
        self.rings.slot(v, s)
    }

    fn guard(self, v: VoiceIdx, s: SlotIdx) -> &'r AtomicU8 {
        self.rings.guard(v, s)
    }

    fn cell(self, v: VoiceIdx, s: SlotIdx) -> &'r SlotCell {
        let near = &self.rings.rings[v.index()].bytes;
        match s {
            SlotIdx::A => &near[0],
            SlotIdx::B => &near[1],
            SlotIdx::C => &self.third.bytes[v.index()],
        }
    }
}

/// The audio's side: one `AudioTurn` a block.
pub struct AudioSide<'r> {
    at: Store<'r>,
}

impl AudioSide<'_> {
    pub fn turn(&mut self) -> AudioTurn<'_> {
        AudioTurn { at: self.at }
    }
}

/// One audio block's access to the rings.
pub struct AudioTurn<'s> {
    at: Store<'s>,
}

impl<'s> AudioTurn<'s> {
    /// Voice `v`'s wants read `id` from here on: set at a note's start,
    /// its last note's slots cancelled, before its wants (whose `Release`
    /// publishes it).
    pub fn stream(&self, v: VoiceIdx, id: SourceId) {
        self.at.rings.rings[v.index()]
            .source
            .store(id, Ordering::Relaxed);
    }

    pub fn seen(&self, v: VoiceIdx, s: SlotIdx) -> SlotState {
        self.at.slot(v, s).seen()
    }

    /// Empty → Wanted: the generation its fill will carry.
    pub fn want(
        &self,
        v: VoiceIdx,
        s: SlotIdx,
        c: ChunkIdx,
        by: AudioBlocks,
    ) -> Option<Generation> {
        self.at.slot(v, s).want(c, by)
    }

    /// Ready → Empty. Not while a `Ready` lives:
    ///
    /// ```
    /// use chimera_engine::stream::layout::ChunkIdx;
    /// use chimera_engine::stream::ring::Rings;
    /// use chimera_engine::stream::units::{AudioBlocks, SlotIdx};
    /// use chimera_engine::voice_alloc::VoiceIdx;
    /// use chimera_engine::stream::ring::ThirdSlots;
    /// static RINGS: Rings = Rings::new();
    /// static THIRD: ThirdSlots = ThirdSlots::new();
    /// let (v, s) = (VoiceIdx::ALL[0], SlotIdx::A);
    /// let (mut audio, mut card) = RINGS.sides(&THIRD).unwrap();
    /// let mut turn = audio.turn();
    /// let g = turn.want(v, s, ChunkIdx::new(0).unwrap(), AudioBlocks::new(9)).unwrap();
    /// let claim = card.claim(v, s).unwrap();
    /// card.fill_buf(&claim).unwrap()[0] = 0x12;
    /// assert!(card.fill_done(claim));
    /// let ready = turn.ready(v, s, g).unwrap();
    /// assert_eq!(ready.sample(0), Some(0x12));
    /// turn.release(v, s);
    /// ```
    ///
    /// ```compile_fail,E0502
    /// use chimera_engine::stream::layout::ChunkIdx;
    /// use chimera_engine::stream::ring::Rings;
    /// use chimera_engine::stream::units::{AudioBlocks, SlotIdx};
    /// use chimera_engine::voice_alloc::VoiceIdx;
    /// use chimera_engine::stream::ring::ThirdSlots;
    /// static RINGS: Rings = Rings::new();
    /// static THIRD: ThirdSlots = ThirdSlots::new();
    /// let (v, s) = (VoiceIdx::ALL[0], SlotIdx::A);
    /// let (mut audio, _card) = RINGS.sides(&THIRD).unwrap();
    /// let mut turn = audio.turn();
    /// let g = turn.want(v, s, ChunkIdx::new(0).unwrap(), AudioBlocks::new(9)).unwrap();
    /// let ready = turn.ready(v, s, g);
    /// turn.release(v, s);
    /// drop(ready);
    /// ```
    pub fn release(&mut self, v: VoiceIdx, s: SlotIdx) {
        self.at.slot(v, s).release();
    }

    /// Whatever the slot holds → Empty, under a new generation: a fill in
    /// flight is dropped.
    pub fn cancel(&mut self, v: VoiceIdx, s: SlotIdx) {
        self.at.slot(v, s).cancel();
    }

    /// The slot's samples, if it is Ready under `g`.
    pub fn ready<'t>(&'t self, v: VoiceIdx, s: SlotIdx, g: Generation) -> Option<Ready<'t>> {
        let chunk = self.at.slot(v, s).ready(g)?;
        // `Acquire`: a guard the card patched in later (`fill_done`) is
        // written whole when seen.
        let guard = Guard::from_bits(self.at.guard(v, s).load(Ordering::Acquire));
        let from = match guard {
            Guard::Guarded => 0,
            Guard::Unguarded => GUARD,
        };
        // SAFETY: the slot is Ready under `g` (loaded `Acquire` above), so
        // the card wrote these bytes before its `Release` and won't again
        // until this turn's `release` or `cancel`, which the borrow of
        // `self` keeps out while the `Ready` lives (`Ring`'s `Sync`). An
        // Unguarded view leaves the guard out: the one place the card may
        // still write a Ready slot (`fill_done`'s patch).
        let bytes = unsafe {
            &*ptr::slice_from_raw_parts(
                self.at.cell(v, s).get().cast::<u8>().add(from),
                SLOT_BYTES - from,
            )
        };
        Some(Ready {
            chunk,
            guard,
            bytes,
        })
    }
}

/// A Ready slot's chunk, for one block.
pub struct Ready<'a> {
    chunk: ChunkIdx,
    guard: Guard,
    /// From the guard when Guarded, else from the chunk.
    bytes: &'a [u8],
}

impl<'a> Ready<'a> {
    /// The frames `sample` reads, packed, and the first's index in the
    /// chunk: from −4 when Guarded, 0 when not. A pair each 3 B, so a
    /// reader decodes them as a row's samples. The one raw-byte path out of
    /// a slot, and like `sample` only under a want's generation (`ready`).
    pub fn packed(&self) -> (i32, &'a [u8]) {
        match self.guard {
            Guard::Guarded => (-(GUARD_FRAMES.get() as i32), self.bytes),
            Guard::Unguarded => (0, self.bytes),
        }
    }

    pub fn chunk(&self) -> ChunkIdx {
        self.chunk
    }

    pub fn guard(&self) -> Guard {
        self.guard
    }

    /// Frame `i` of the chunk: from -4 when Guarded (0 when not) to the
    /// chunk's last frame; `None` outside.
    pub fn sample(&self, i: i32) -> Option<i16> {
        let (first, _) = self.packed();
        let at = u32::try_from(i.checked_sub(first)?).ok()?;
        if at as usize >= self.bytes.len() / 3 * 2 {
            return None;
        }
        // The first frame starts a pair (`GUARD_FRAMES` is even).
        let (even, odd) = unpack12(self.bytes, at as usize / 2);
        Some(if at.is_multiple_of(2) { even } else { odd })
    }
}

/// A slot the card took to fill. Not `Clone`: one fill a claim. The card
/// holds at most one `Claim` per slot at a time (`CardSide::claim` refuses
/// a second until `fill_done` or `requeue`): the 6-bit generation wraps
/// after 64 cancels, so two live claims on one slot could pass a stale
/// check.
#[must_use = "finish with fill_done or requeue"]
pub struct Claim {
    voice: VoiceIdx,
    slot: SlotIdx,
    chunk: ChunkIdx,
    generation: Generation,
}

impl Claim {
    pub fn chunk(&self) -> ChunkIdx {
        self.chunk
    }

    pub fn voice(&self) -> VoiceIdx {
        self.voice
    }

    pub fn slot(&self) -> SlotIdx {
        self.slot
    }
}

/// The card's side: the one writer of slot bytes.
pub struct CardSide<'r> {
    at: Store<'r>,
    live: [[bool; SlotIdx::ALL.len()]; MAX_VOICES],
}

impl<'r> CardSide<'r> {
    /// The rings it fills, for their wants and sources.
    pub fn rings(&self) -> &'r Rings {
        self.at.rings
    }

    /// Wanted → Filling.
    pub fn claim(&mut self, v: VoiceIdx, s: SlotIdx) -> Option<Claim> {
        if self.live[v.index()][s.index()] {
            return None;
        }
        let (chunk, generation) = self.at.slot(v, s).claim()?;
        // Before `fill_done`'s `Release`; a write_guard may set it.
        self.at
            .guard(v, s)
            .store(Guard::Unguarded.bits(), Ordering::Relaxed);
        self.live[v.index()][s.index()] = true;
        Some(Claim {
            voice: v,
            slot: s,
            chunk,
            generation,
        })
    }

    fn filling(&self, c: &Claim) -> bool {
        self.at.slot(c.voice, c.slot).filling(c.chunk, c.generation)
    }

    /// The chunk area after the guard; `None` for a stale claim. One at a
    /// time:
    ///
    /// ```compile_fail,E0499
    /// use chimera_engine::stream::layout::ChunkIdx;
    /// use chimera_engine::stream::ring::Rings;
    /// use chimera_engine::stream::units::{AudioBlocks, SlotIdx};
    /// use chimera_engine::voice_alloc::VoiceIdx;
    /// use chimera_engine::stream::ring::ThirdSlots;
    /// static RINGS: Rings = Rings::new();
    /// static THIRD: ThirdSlots = ThirdSlots::new();
    /// let v = VoiceIdx::ALL[0];
    /// let (mut audio, mut card) = RINGS.sides(&THIRD).unwrap();
    /// let turn = audio.turn();
    /// let at = AudioBlocks::new(9);
    /// turn.want(v, SlotIdx::A, ChunkIdx::new(0).unwrap(), at).unwrap();
    /// turn.want(v, SlotIdx::B, ChunkIdx::new(1).unwrap(), at).unwrap();
    /// let (a, b) = (card.claim(v, SlotIdx::A).unwrap(), card.claim(v, SlotIdx::B).unwrap());
    /// let one = card.fill_buf(&a).unwrap();
    /// let two = card.fill_buf(&b).unwrap();
    /// one[0] = two[0];
    /// ```
    pub fn fill_buf(&mut self, c: &Claim) -> Option<&mut [u8]> {
        if !self.filling(c) {
            return None;
        }
        let cell = self.at.cell(c.voice, c.slot);
        // SAFETY: the slot is Filling under `c`'s generation, so no `Ready`
        // reads it, and this is the one `CardSide`, whose `&mut self` this
        // slice borrows: no other slice of any slot lives (`Ring`'s `Sync`).
        let bytes = unsafe { &mut *cell.get() };
        Some(&mut bytes[GUARD..])
    }

    /// The last frames of another slot Ready with the chunk before, ahead
    /// of this chunk; else Unguarded.
    pub fn write_guard(&mut self, c: &Claim) {
        if !self.filling(c) {
            return;
        }
        let at = self.at;
        let Some(before) = SlotIdx::ALL.into_iter().find(|&s| {
            s != c.slot
                && at.slot(c.voice, s).ready_chunk().and_then(ChunkIdx::next) == Some(c.chunk)
        }) else {
            return;
        };
        // SAFETY: `c`'s slot is Filling under its claim, so only this
        // `&mut self` touches it. The audio may release or cancel `before`
        // meanwhile, but slot bytes are written only by the one `CardSide`,
        // which is inside this call: `before`'s bytes can't change under
        // the copy (`Ring`'s `Sync`). Two slots, two cells: the borrows
        // don't overlap.
        let (to, from) = unsafe {
            (
                &mut *at.cell(c.voice, c.slot).get(),
                &*at.cell(c.voice, before).get(),
            )
        };
        to[..GUARD].copy_from_slice(&from[SLOT_BYTES - GUARD..]);
        at.guard(c.voice, c.slot)
            .store(Guard::Guarded.bits(), Ordering::Relaxed);
    }

    /// Filling → Ready; false if the fill was dropped (cancelled). The
    /// chunk after it, Ready already and Unguarded (it landed first: a
    /// requeue), takes its guard from it (#581).
    pub fn fill_done(&mut self, c: Claim) -> bool {
        self.live[c.voice.index()][c.slot.index()] = false;
        let done = self
            .at
            .slot(c.voice, c.slot)
            .finish(c.chunk, c.generation, Finish::Ready);
        if done {
            self.guard_next(&c);
        }
        done
    }

    /// `c`'s last frames ahead of the chunk after it, where that is Ready
    /// and Unguarded.
    fn guard_next(&mut self, c: &Claim) {
        let at = self.at;
        let Some(next) = SlotIdx::ALL.into_iter().find(|&s| {
            s != c.slot
                && c.chunk
                    .next()
                    .is_some_and(|n| at.slot(c.voice, s).ready_chunk() == Some(n))
                && Guard::from_bits(at.guard(c.voice, s).load(Ordering::Relaxed))
                    == Guard::Unguarded
        }) else {
            return;
        };
        // SAFETY: slot bytes are written only by the one `CardSide`,
        // inside this call. `next` is Ready and Unguarded, so every `Ready`
        // of it leaves its guard out (`AudioTurn::ready`): the write to
        // those bytes overlaps no view. `c`'s slot is only read. Two
        // slots, two cells.
        let (to, from) = unsafe {
            (
                &mut *ptr::slice_from_raw_parts_mut(
                    at.cell(c.voice, next).get().cast::<u8>(),
                    GUARD,
                ),
                &*at.cell(c.voice, c.slot).get(),
            )
        };
        to.copy_from_slice(&from[SLOT_BYTES - GUARD..]);
        // The bytes before the flag: `ready` loads it `Acquire`.
        at.guard(c.voice, next)
            .store(Guard::Guarded.bits(), Ordering::Release);
    }

    /// Filling → Wanted, same generation: a fault, to try again.
    pub fn requeue(&mut self, c: Claim) {
        self.live[c.voice.index()][c.slot.index()] = false;
        self.at
            .slot(c.voice, c.slot)
            .finish(c.chunk, c.generation, Finish::Requeue);
    }
}
