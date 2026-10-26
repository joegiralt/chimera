//! Bytes the UI queues for DIN OUT (MIDI phase 1 spec § 4.2, § 9): a
//! 256-byte single-producer single-consumer ring. The UI pushes whole
//! messages or nothing; the TX interrupt (or the desktop's out thread)
//! pops bytes. Bank 1's type: the ISR stub and the UI share it.
//!
//! Ordering as `note_queue`'s: the producer's `Relaxed` byte stores are
//! published by its `Release` on `tail`, which the consumer's `Acquire`
//! load pairs with; the consumer frees bytes by its `Release` on `head`,
//! which the producer's `Acquire` load pairs with. Each index has one
//! writer, so each side reads its own back `Relaxed`. A message's bytes
//! are all stored before the one `tail` store that publishes them, so a
//! consumer that pops everything waiting always ends on a message's end.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

pub const OUT_RING_LEN: usize = 256;
const _: () = assert!(OUT_RING_LEN.is_power_of_two());

pub struct OutRing {
    buf: [AtomicU8; OUT_RING_LEN],
    /// Next byte to pop (the consumer's only), free-running.
    head: AtomicU32,
    /// Next byte to push (the producer's only), free-running.
    tail: AtomicU32,
}

impl Default for OutRing {
    fn default() -> Self {
        Self::new()
    }
}

impl OutRing {
    pub const fn new() -> Self {
        Self {
            buf: [const { AtomicU8::new(0) }; OUT_RING_LEN],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    /// Producer side only: all of `msg`, or nothing if it doesn't fit.
    /// Inlined into its callers, bank 2's UI, so none of it is bank 1's.
    #[inline(always)]
    pub fn push_msg(&self, msg: &[u8]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let held = tail.wrapping_sub(self.head.load(Ordering::Acquire)) as usize;
        if OUT_RING_LEN - held < msg.len() {
            return false;
        }
        for (i, &b) in msg.iter().enumerate() {
            let at = tail.wrapping_add(i as u32) as usize % OUT_RING_LEN;
            self.buf[at].store(b, Ordering::Relaxed);
        }
        self.tail
            .store(tail.wrapping_add(msg.len() as u32), Ordering::Release);
        true
    }

    /// Consumer side only: as many waiting bytes as fit `out`, oldest
    /// first; how many.
    pub fn pop_into(&self, out: &mut [u8]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let held = self.tail.load(Ordering::Acquire).wrapping_sub(head) as usize;
        let n = held.min(out.len());
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = self.buf[head.wrapping_add(i as u32) as usize % OUT_RING_LEN]
                .load(Ordering::Relaxed);
        }
        self.head
            .store(head.wrapping_add(n as u32), Ordering::Release);
        n
    }

    /// Nothing waits. Either side; a moment's answer.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
}
