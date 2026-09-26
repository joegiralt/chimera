//! Notes from the UI/input thread to the audio thread (instrument-core spec
//! § Threading): a fixed 64-event single-producer single-consumer ring.
//! Lock-free and allocation-free; each event is packed into one `AtomicU32`,
//! so there is no `unsafe`. A full queue drops the event and counts it.
//!
//! Ordering: the Cortex-M7 is single-core, so there is no cache-coherency
//! problem to solve here — every access sees the same physical memory.
//! What `Acquire`/`Release` still buys us is a *reordering* barrier, for
//! both the compiler and the core's out-of-order pipeline: without it,
//! the slot write in `push` could be observed after the `tail` bump that
//! advertises it, or the slot read in `pop` could be hoisted past the
//! `head` bump that frees the slot for reuse, and the producer and
//! consumer (an interrupt handler and a foreground loop, or two real
//! cores on other hardware) would disagree about which slot is safe to
//! touch. A `Release` store paired with an `Acquire` load of the *same*
//! atomic forms a synchronizes-with edge, which makes everything the
//! writer did before the `Release` visible to the reader after the
//! matching `Acquire`. Concretely: `push`'s `Relaxed` slot store
//! happens-before `pop`'s `Relaxed` slot load because it is sequenced
//! before `push`'s `Release` store to `tail`, which `pop`'s `Acquire`
//! load of `tail` synchronizes with. The same reasoning runs the other
//! way for `head`, so the producer never reuses a slot the consumer is
//! still reading. `Relaxed` is sufficient for the slot accesses
//! themselves and for each side reading back its *own* index, since only
//! one thread ever writes `head` and only one thread ever writes `tail`.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::{MidiChannel, MidiNote, Velocity};

pub const NOTE_QUEUE_LEN: usize = 64;
const _: () = assert!(NOTE_QUEUE_LEN.is_power_of_two());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    On(Velocity),
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub channel: MidiChannel,
    pub note: MidiNote,
    pub kind: NoteKind,
}

impl NoteEvent {
    /// Bits 0..4 channel, 4..11 note, 11..18 velocity (0 = note-off).
    fn pack(self) -> u32 {
        let vel = match self.kind {
            NoteKind::On(v) => v.get(),
            NoteKind::Off => 0,
        };
        self.channel.get() as u32 | ((self.note.get() as u32) << 4) | ((vel as u32) << 11)
    }

    fn unpack(bits: u32) -> Option<Self> {
        Some(Self {
            channel: MidiChannel::new((bits & 0xF) as u8)?,
            note: MidiNote::new(((bits >> 4) & 0x7F) as u8)?,
            kind: match Velocity::new(((bits >> 11) & 0x7F) as u8) {
                Some(v) => NoteKind::On(v),
                None => NoteKind::Off,
            },
        })
    }
}

/// One producer (`push`) and one consumer (`pop`). Head and tail are
/// free-running counters; the slot is `counter % NOTE_QUEUE_LEN`.
pub struct NoteQueue {
    slots: [AtomicU32; NOTE_QUEUE_LEN],
    /// Next event to pop (written by the consumer only).
    head: AtomicU32,
    /// Next slot to push (written by the producer only).
    tail: AtomicU32,
    dropped: AtomicU32,
}

impl Default for NoteQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(0) }; NOTE_QUEUE_LEN],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// Producer side. `false`: the queue was full and the event was dropped.
    ///
    /// Ordering: `tail` is read back `Relaxed` since only this thread ever
    /// writes it. `head` is `Acquire`, pairing with `pop`'s `Release` store
    /// to `head`, so a slot the consumer just finished reading is visible
    /// as free before we overwrite it. The slot `store` is `Relaxed`; it
    /// is published by the `Release` store to `tail` right after it, which
    /// is what `pop`'s `Acquire` load of `tail` synchronizes with, so the
    /// slot write always happens-before the matching read in `pop`.
    pub fn push(&self, ev: NoteEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) as usize >= NOTE_QUEUE_LEN {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.slots[tail as usize % NOTE_QUEUE_LEN].store(ev.pack(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side (the audio thread).
    ///
    /// Ordering: symmetric with `push`. `head` is read back `Relaxed`
    /// (only this thread writes it); `tail` is `Acquire`, pairing with
    /// `push`'s `Release` store to `tail`, so the slot write in `push` is
    /// guaranteed visible before the `Relaxed` slot load here. The
    /// `Release` store to `head` afterward is what `push`'s `Acquire` load
    /// of `head` pairs with, so the producer never reuses this slot before
    /// this read of it has completed.
    pub fn pop(&self) -> Option<NoteEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let bits = self.slots[head as usize % NOTE_QUEUE_LEN].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        NoteEvent::unpack(bits)
    }

    /// Events dropped because the queue was full.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub const MAX_NOTE_SOURCES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceId<const N: usize>(usize);

impl<const N: usize> SourceId<N> {
    pub const fn new(index: usize) -> Self {
        assert!(index < N, "note source index out of range");
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

/// One `NoteQueue` per note source; each queue has exactly one producer.
/// `drain` empties every queue in source order (source 0 first) so a
/// caller processing note-offs before note-ons within a block sees a
/// deterministic, if not timestamped, order.
pub struct NoteSources<const N: usize> {
    queues: [NoteQueue; N],
}

impl<const N: usize> Default for NoteSources<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> NoteSources<N> {
    pub const fn new() -> Self {
        const {
            assert!(N > 0);
            assert!(N <= MAX_NOTE_SOURCES);
        }
        Self {
            queues: [const { NoteQueue::new() }; N],
        }
    }

    pub fn source(&self, id: SourceId<N>) -> &NoteQueue {
        &self.queues[id.0]
    }

    pub fn drain(&self, mut f: impl FnMut(NoteEvent)) {
        for q in &self.queues {
            while let Some(ev) = q.pop() {
                f(ev);
            }
        }
    }

    pub fn drops(&self) -> [u32; N] {
        core::array::from_fn(|i| self.queues[i].dropped())
    }
}
