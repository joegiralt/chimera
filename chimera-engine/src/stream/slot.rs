// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! One slot of a stream's ring: a state word the audio and the card move
//! by compare-and-swap, and the deadline the audio wants it filled by.
//!
//! The audio moves a slot out of Empty (`want`) and back into it
//! (`release`, `cancel`), and is the only side that changes its
//! generation. The card (through `ring::CardSide`) moves Wanted to
//! Filling, Filling to Ready or back to Wanted, never touching the
//! generation. So a fill can only land under the generation it claimed.
//!
//! Ordering:
//! - **want:** the deadline `Relaxed`, then the word `Release`. A Wanted
//!   word read beside a newer deadline only reorders EDF; it never hands
//!   out bytes (the claim and every later step check the generation).
//! - **claim:** CAS `Acquire`: the audio's reads of the slot's last fill
//!   happened before the `want` it claims, so the card's writes come after.
//! - **fill_done:** CAS `Release`; `seen` loads `Acquire`, so a Ready
//!   seen is a Ready whose bytes are written.
//! - **release:** `Release`, so the audio's reads come before the card's
//!   next claim of the slot. **cancel** is `AcqRel` for the same reason.

#[cfg(not(loom))]
use core::sync::atomic::{AtomicU32, Ordering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicU32, Ordering};

use super::layout::ChunkIdx;
use super::units::{AudioBlocks, Generation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Empty,
    Wanted {
        chunk: ChunkIdx,
        generation: Generation,
        deadline: AudioBlocks,
    },
    Filling {
        chunk: ChunkIdx,
        generation: Generation,
    },
    Ready {
        chunk: ChunkIdx,
        generation: Generation,
    },
}

// The word: state in the top 2 bits, the generation, then the chunk.
const CHUNK_BITS: u32 = 24;
const GEN_SHIFT: u32 = CHUNK_BITS;
const STATE_SHIFT: u32 = GEN_SHIFT + Generation::BITS;
const CHUNK_MASK: u32 = (1 << CHUNK_BITS) - 1;
const _: () = assert!(2 + Generation::BITS + 24 == 32);
const _: () = assert!(ChunkIdx::MAX.get() == CHUNK_MASK);

/// A word's state, without its fields. Empty is 0, so a zeroed slot is
/// Empty at generation 0.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Empty = 0,
    Wanted = 1,
    Filling = 2,
    Ready = 3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Word {
    tag: Tag,
    generation: Generation,
    chunk: ChunkIdx,
}

impl Word {
    fn pack(self) -> u32 {
        (self.tag as u32) << STATE_SHIFT | self.generation.bits() << GEN_SHIFT | self.chunk.get()
    }

    fn unpack(w: u32) -> Word {
        let tag = match w >> STATE_SHIFT {
            0 => Tag::Empty,
            1 => Tag::Wanted,
            2 => Tag::Filling,
            _ => Tag::Ready,
        };
        let chunk = match ChunkIdx::new(w & CHUNK_MASK) {
            Some(c) => c,
            // `CHUNK_MASK` is `ChunkIdx::MAX` (asserted above).
            None => ChunkIdx::MAX,
        };
        Word {
            tag,
            generation: Generation::from_bits(w >> GEN_SHIFT),
            chunk,
        }
    }

    fn with(self, tag: Tag) -> Word {
        Word { tag, ..self }
    }
}

pub struct Slot {
    word: AtomicU32,
    deadline: AtomicU32,
}

#[cfg(not(loom))]
impl Default for Slot {
    fn default() -> Self {
        Self::new()
    }
}

impl Slot {
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Slot {
            word: AtomicU32::new(0),
            deadline: AtomicU32::new(0),
        }
    }

    /// loom's atomics aren't `const`.
    #[cfg(loom)]
    pub fn new() -> Self {
        Slot {
            word: AtomicU32::new(0),
            deadline: AtomicU32::new(0),
        }
    }

    fn load(&self) -> Word {
        Word::unpack(self.word.load(Ordering::Acquire))
    }

    fn swap(&self, from: Word, to: Word, ok: Ordering) -> bool {
        self.word
            .compare_exchange(from.pack(), to.pack(), ok, Ordering::Relaxed)
            .is_ok()
    }

    pub fn seen(&self) -> SlotState {
        let Word {
            tag,
            generation,
            chunk,
        } = self.load();
        match tag {
            Tag::Empty => SlotState::Empty,
            Tag::Wanted => {
                let deadline = AudioBlocks::new(self.deadline.load(Ordering::Relaxed));
                SlotState::Wanted {
                    chunk,
                    generation,
                    deadline,
                }
            }
            Tag::Filling => SlotState::Filling { chunk, generation },
            Tag::Ready => SlotState::Ready { chunk, generation },
        }
    }

    /// Empty → Wanted under the next generation; `None` if not Empty.
    pub fn want(&self, chunk: ChunkIdx, deadline: AudioBlocks) -> Option<Generation> {
        let w = self.load();
        if w.tag != Tag::Empty {
            return None;
        }
        let generation = w.generation.next();
        self.deadline.store(deadline.get(), Ordering::Relaxed);
        let to = Word {
            tag: Tag::Wanted,
            generation,
            chunk,
        };
        self.swap(w, to, Ordering::Release).then_some(generation)
    }

    /// Ready → Empty, keeping the generation.
    pub fn release(&self) {
        let w = self.load();
        if w.tag == Tag::Ready {
            self.swap(w, w.with(Tag::Empty), Ordering::Release);
        }
    }

    /// Whatever it holds → Empty under the next generation, so a fill in
    /// flight lands nowhere. Wait-free: the card changes only the state,
    /// never the generation, so the swap needs no retry.
    pub fn cancel(&self) {
        let w = self.load();
        if w.tag != Tag::Empty {
            let to = Word {
                tag: Tag::Empty,
                generation: w.generation.next(),
                chunk: w.chunk,
            };
            self.word.swap(to.pack(), Ordering::AcqRel);
        }
    }

    /// Card: Wanted → Filling.
    pub(super) fn claim(&self) -> Option<(ChunkIdx, Generation)> {
        let w = self.load();
        (w.tag == Tag::Wanted && self.swap(w, w.with(Tag::Filling), Ordering::Acquire))
            .then_some((w.chunk, w.generation))
    }

    /// Card: still Filling under this claim.
    pub(super) fn filling(&self, chunk: ChunkIdx, generation: Generation) -> bool {
        self.load()
            == Word {
                tag: Tag::Filling,
                generation,
                chunk,
            }
    }

    /// Card: Filling → `to`, under this claim; false if it moved on.
    pub(super) fn finish(&self, chunk: ChunkIdx, generation: Generation, to: Finish) -> bool {
        let from = Word {
            tag: Tag::Filling,
            generation,
            chunk,
        };
        let tag = match to {
            Finish::Ready => Tag::Ready,
            Finish::Requeue => Tag::Wanted,
        };
        self.swap(from, from.with(tag), Ordering::Release)
    }

    /// Ready under `generation`: its chunk.
    pub(super) fn ready(&self, generation: Generation) -> Option<ChunkIdx> {
        let w = self.load();
        (w.tag == Tag::Ready && w.generation == generation).then_some(w.chunk)
    }

    /// Ready, whatever the generation: its chunk.
    pub(super) fn ready_chunk(&self) -> Option<ChunkIdx> {
        let w = self.load();
        (w.tag == Tag::Ready).then_some(w.chunk)
    }
}

pub(super) enum Finish {
    Ready,
    Requeue,
}
