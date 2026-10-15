//! The stream rings in SRAM4: two slots a voice, each a chunk of packed
//! samples behind a few guard frames copied from the chunk before, so a
//! cursor's interpolation reads across the seam from one slot.
//!
//! Two sides, each handed out once (`Rings::sides`): the audio's, which
//! wants, cancels and reads, and the card's, the one writer of slot bytes.
//! A fill needs a `Claim`, a read a `Ready`; both are checked against the
//! slot word's generation, so neither side touches bytes the other owns.

use core::cell::UnsafeCell;
#[cfg(not(loom))]
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use chimera_hal::units::Bytes;
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::layout::{CHUNK_BYTES, CHUNK_FRAMES, ChunkIdx};
use super::slot::{Finish, Slot, SlotState};
use super::units::{AudioBlocks, Frames, Generation, Packing, SlotIdx};
use crate::hw::MAX_VOICES;
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
}

pub struct Ring {
    slots: [Slot; 2],
    bytes: [UnsafeCell<[u8; SLOT_BYTES]>; 2],
    guard: [AtomicU8; 2],
}

// SAFETY: `bytes` are written only through the one `CardSide` (`sides`
// hands it out once), under a `Claim` whose slot is still Filling under
// its generation (`fill_buf`, `write_guard` check it); a slot leaves
// Filling only by the card's own `&mut self` calls or the audio's cancel,
// and nothing reads a Filling or Empty slot. They are read only through a
// `Ready`, which exists only while its slot is Ready under its own
// generation, and borrows the block's `AudioTurn`: the card never moves a
// Ready slot, and the audio's `release` and `cancel` need `&mut AudioTurn`.
// The `Release` publish and `Acquire` load of the slot word order the
// bytes (slot.rs). `write_guard` reads the other slot while it is Ready,
// where only reads happen.
unsafe impl Sync for Ring {}

impl Ring {
    #[cfg(not(loom))]
    const fn new() -> Ring {
        Ring {
            slots: [const { Slot::new() }; 2],
            bytes: [const { UnsafeCell::new([0; SLOT_BYTES]) }; 2],
            guard: [const { AtomicU8::new(0) }; 2],
        }
    }

    #[cfg(loom)]
    fn new() -> Ring {
        Ring {
            slots: core::array::from_fn(|_| Slot::new()),
            bytes: core::array::from_fn(|_| UnsafeCell::new([0; SLOT_BYTES])),
            guard: core::array::from_fn(|_| AtomicU8::new(0)),
        }
    }
}

/// A ring a voice. All-zero bytes are a valid `Rings`: every slot Empty
/// at generation 0, Unguarded, its sides not yet taken. The board builds
/// it so, in place in SRAM4.
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

    /// The audio's side and the card's, once.
    pub fn sides(&self) -> Option<(AudioSide<'_>, CardSide<'_>)> {
        (!self.taken.swap(true, Ordering::AcqRel))
            .then_some((AudioSide { rings: self }, CardSide { rings: self }))
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

    fn slot(&self, v: VoiceIdx, s: SlotIdx) -> &Slot {
        &self.rings[v.index()].slots[s.index()]
    }
}

/// The audio's side: one `AudioTurn` a block.
pub struct AudioSide<'r> {
    rings: &'r Rings,
}

impl AudioSide<'_> {
    pub fn turn(&mut self) -> AudioTurn<'_> {
        AudioTurn { rings: self.rings }
    }
}

/// One audio block's access to the rings.
pub struct AudioTurn<'s> {
    rings: &'s Rings,
}

impl<'s> AudioTurn<'s> {
    pub fn seen(&self, v: VoiceIdx, s: SlotIdx) -> SlotState {
        self.rings.slot(v, s).seen()
    }

    /// Empty → Wanted: the generation its fill will carry.
    pub fn want(
        &self,
        v: VoiceIdx,
        s: SlotIdx,
        c: ChunkIdx,
        by: AudioBlocks,
    ) -> Option<Generation> {
        self.rings.slot(v, s).want(c, by)
    }

    /// Ready → Empty. Not while a `Ready` lives:
    ///
    /// ```
    /// use chimera_engine::stream::layout::ChunkIdx;
    /// use chimera_engine::stream::ring::Rings;
    /// use chimera_engine::stream::units::{AudioBlocks, SlotIdx};
    /// use chimera_engine::voice_alloc::VoiceIdx;
    /// static RINGS: Rings = Rings::new();
    /// let (v, s) = (VoiceIdx::ALL[0], SlotIdx::A);
    /// let (mut audio, mut card) = RINGS.sides().unwrap();
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
    /// static RINGS: Rings = Rings::new();
    /// let (v, s) = (VoiceIdx::ALL[0], SlotIdx::A);
    /// let (mut audio, _card) = RINGS.sides().unwrap();
    /// let mut turn = audio.turn();
    /// let g = turn.want(v, s, ChunkIdx::new(0).unwrap(), AudioBlocks::new(9)).unwrap();
    /// let ready = turn.ready(v, s, g);
    /// turn.release(v, s);
    /// drop(ready);
    /// ```
    pub fn release(&mut self, v: VoiceIdx, s: SlotIdx) {
        self.rings.slot(v, s).release();
    }

    /// Whatever the slot holds → Empty, under a new generation: a fill in
    /// flight is dropped.
    pub fn cancel(&mut self, v: VoiceIdx, s: SlotIdx) {
        self.rings.slot(v, s).cancel();
    }

    /// The slot's samples, if it is Ready under `g`.
    pub fn ready<'t>(&'t self, v: VoiceIdx, s: SlotIdx, g: Generation) -> Option<Ready<'t>> {
        let ring = &self.rings.rings[v.index()];
        let chunk = ring.slots[s.index()].ready(g)?;
        let guard = Guard::from_bits(ring.guard[s.index()].load(Ordering::Relaxed));
        // SAFETY: the slot is Ready under `g` (loaded `Acquire` above), so
        // the card wrote these bytes before its `Release` and won't again
        // until this turn's `release` or `cancel`, which the borrow of
        // `self` keeps out while the `Ready` lives (`Ring`'s `Sync`).
        let bytes = unsafe { &*ring.bytes[s.index()].get() };
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
    bytes: &'a [u8; SLOT_BYTES],
}

impl Ready<'_> {
    pub fn chunk(&self) -> ChunkIdx {
        self.chunk
    }

    pub fn guard(&self) -> Guard {
        self.guard
    }

    /// Frame `i` of the chunk: from -4 when Guarded (0 when not) to the
    /// chunk's last frame; `None` outside.
    pub fn sample(&self, i: i32) -> Option<i16> {
        let first = match self.guard {
            Guard::Guarded => 0,
            Guard::Unguarded => GUARD_FRAMES.get(),
        };
        let at = u32::try_from(i.checked_add_unsigned(GUARD_FRAMES.get())?).ok()?;
        if at < first || at >= GUARD_FRAMES.get() + CHUNK_FRAMES.get() {
            return None;
        }
        // Under `GUARD_FRAMES + CHUNK_FRAMES`: a pair in `bytes`.
        let (even, odd) = unpack12(self.bytes, at as usize / 2);
        Some(if at.is_multiple_of(2) { even } else { odd })
    }
}

/// A slot the card took to fill. Not `Clone`: one fill a claim.
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
    rings: &'r Rings,
}

impl CardSide<'_> {
    /// Wanted → Filling.
    pub fn claim(&mut self, v: VoiceIdx, s: SlotIdx) -> Option<Claim> {
        let (chunk, generation) = self.rings.slot(v, s).claim()?;
        // Before `fill_done`'s `Release`; a write_guard may set it.
        self.rings.rings[v.index()].guard[s.index()].store(0, Ordering::Relaxed);
        Some(Claim {
            voice: v,
            slot: s,
            chunk,
            generation,
        })
    }

    fn filling(&self, c: &Claim) -> bool {
        self.rings
            .slot(c.voice, c.slot)
            .filling(c.chunk, c.generation)
    }

    /// The chunk area after the guard; `None` for a stale claim. One at a
    /// time:
    ///
    /// ```compile_fail,E0499
    /// use chimera_engine::stream::layout::ChunkIdx;
    /// use chimera_engine::stream::ring::Rings;
    /// use chimera_engine::stream::units::{AudioBlocks, SlotIdx};
    /// use chimera_engine::voice_alloc::VoiceIdx;
    /// static RINGS: Rings = Rings::new();
    /// let v = VoiceIdx::ALL[0];
    /// let (mut audio, mut card) = RINGS.sides().unwrap();
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
        let cell = &self.rings.rings[c.voice.index()].bytes[c.slot.index()];
        // SAFETY: the slot is Filling under `c`'s generation, so no `Ready`
        // reads it, and this is the one `CardSide`, whose `&mut self` this
        // slice borrows: no other slice of any slot lives (`Ring`'s `Sync`).
        let bytes = unsafe { &mut *cell.get() };
        Some(&mut bytes[GUARD..])
    }

    /// The other slot's last frames ahead of this chunk, when it is Ready
    /// with the chunk before; else Unguarded.
    pub fn write_guard(&mut self, c: &Claim) {
        if !self.filling(c) {
            return;
        }
        let ring = &self.rings.rings[c.voice.index()];
        let other = c.slot.other();
        let before = ring.slots[other.index()]
            .ready_chunk()
            .and_then(ChunkIdx::next);
        if before != Some(c.chunk) {
            return;
        }
        // SAFETY: `c`'s slot is Filling under its claim, so only this
        // `&mut self` touches it; the other slot is Ready, where nothing
        // writes (`Ring`'s `Sync`). Two cells: the borrows don't overlap.
        let (to, from) = unsafe {
            (
                &mut *ring.bytes[c.slot.index()].get(),
                &*ring.bytes[other.index()].get(),
            )
        };
        to[..GUARD].copy_from_slice(&from[SLOT_BYTES - GUARD..]);
        ring.guard[c.slot.index()].store(1, Ordering::Relaxed);
    }

    /// Filling → Ready; false if the fill was dropped (cancelled).
    pub fn fill_done(&mut self, c: Claim) -> bool {
        self.rings
            .slot(c.voice, c.slot)
            .finish(c.chunk, c.generation, Finish::Ready)
    }

    /// Filling → Wanted, same generation: a fault, to try again.
    pub fn requeue(&mut self, c: Claim) {
        self.rings
            .slot(c.voice, c.slot)
            .finish(c.chunk, c.generation, Finish::Requeue);
    }
}
