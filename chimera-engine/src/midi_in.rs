//! MIDI in, split at the source (MIDI map spec § 1): notes, bend, the
//! pedal, channel mode and real-time go to the note queue; every other CC
//! to its own ring, which the UI drains. A turning knob never fills the
//! note queue, and only the UI edits the Sound.
//!
//! `CcRing` is `NoteQueue`'s ring without the note-off reserve: one
//! producer (an ISR or a MIDI thread), one consumer (the UI), the same
//! `Acquire`/`Release` pairing on `head` and `tail` (see `note_queue`).

use core::sync::atomic::{AtomicU32, Ordering};

use chimera_hal::MidiMessage;

use crate::MidiChannel;
use crate::note_queue::InEvent;

/// A controller number, 0..=127.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cc(u8);

/// Bank select (0, 32), RPN/NRPN (6, 38, 96–101), sustain (64) and
/// channel mode (120–127): never mapped (spec § 4).
const RESERVED: [u8; 19] = [
    0, 6, 32, 38, 64, 96, 97, 98, 99, 100, 101, 120, 121, 122, 123, 124, 125, 126, 127,
];

impl Cc {
    pub const fn new(n: u8) -> Option<Self> {
        if n <= 127 { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn is_reserved(self) -> bool {
        RESERVED.contains(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CcEvent {
    pub channel: MidiChannel,
    pub cc: Cc,
    /// 0..=127.
    pub value: u8,
}

impl CcEvent {
    /// Bits 0..4 channel, 4..11 CC, 11..18 value.
    pub fn pack(self) -> u32 {
        self.channel.get() as u32 | ((self.cc.0 as u32) << 4) | (((self.value & 0x7F) as u32) << 11)
    }

    pub fn unpack(bits: u32) -> Option<Self> {
        Some(Self {
            channel: MidiChannel::new((bits & 0xF) as u8)?,
            cc: Cc(((bits >> 4) & 0x7F) as u8),
            value: ((bits >> 11) & 0x7F) as u8,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Routed {
    Queue(InEvent),
    Cc(CcEvent),
    Drop,
}

/// Where a message goes: the note queue takes `InEvent::from_midi`'s set,
/// the ring every other CC (reserved ones too: learn and decode refuse them).
pub fn route(msg: MidiMessage) -> Routed {
    InEvent::from_midi(msg).map_or_else(
        || match msg {
            MidiMessage::ControlChange { channel, cc, value } => {
                Cc::new(cc).map_or(Routed::Drop, |cc| {
                    Routed::Cc(CcEvent {
                        channel,
                        cc,
                        value: value & 0x7F,
                    })
                })
            }
            _ => Routed::Drop,
        },
        Routed::Queue,
    )
}

pub const CC_RING_LEN: usize = 64;
const _: () = assert!(CC_RING_LEN.is_power_of_two());

/// One producer, one consumer; head and tail free-running, the slot
/// `counter % CC_RING_LEN`. A full ring drops the newest and counts it.
pub struct CcRing {
    slots: [AtomicU32; CC_RING_LEN],
    /// Next event to pop (the consumer's only).
    head: AtomicU32,
    /// Next slot to push (the producer's only).
    tail: AtomicU32,
    dropped: AtomicU32,
}

impl Default for CcRing {
    fn default() -> Self {
        Self::new()
    }
}

impl CcRing {
    pub const fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(0) }; CC_RING_LEN],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// Producer side only. `false`: full, dropped. `head` `Acquire` pairs
    /// with `pop`'s `Release`; the `Release` on `tail` publishes the slot.
    pub fn push(&self, ev: CcEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) as usize >= CC_RING_LEN {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.slots[tail as usize % CC_RING_LEN].store(ev.pack(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side only. `tail` `Acquire` pairs with `push`'s `Release`;
    /// the `Release` on `head` frees the slot.
    pub fn pop(&self) -> Option<CcEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let bits = self.slots[head as usize % CC_RING_LEN].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        CcEvent::unpack(bits)
    }

    /// Consumer side: every event waiting, oldest first.
    pub fn drain(&self) -> impl Iterator<Item = CcEvent> + '_ {
        core::iter::from_fn(|| self.pop())
    }

    /// Events dropped because the ring was full.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}
