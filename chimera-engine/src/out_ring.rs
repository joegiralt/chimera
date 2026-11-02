//! Bytes the UI queues for DIN OUT or USB's IN (MIDI phase 1 spec § 4.2,
//! § 9): a 256-byte single-producer single-consumer ring. The UI pushes
//! whole messages or nothing; the TX interrupt, USB's poll (or the
//! desktop's out thread) pops bytes. Bank 1's type: the ISR stub and the
//! UI share it.
//!
//! Ordering as `note_queue`'s: the producer's `Relaxed` byte stores are
//! published by its `Release` on `tail`, which the consumer's `Acquire`
//! load pairs with; the consumer frees bytes by its `Release` on `head`,
//! which the producer's `Acquire` load pairs with. Each index has one
//! writer, so each side reads its own back `Relaxed`. A message's bytes
//! are all stored before the one `tail` store that publishes them, so a
//! consumer that pops everything waiting always ends on a message's end.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use crate::midi_in::Port;

pub const OUT_RING_LEN: usize = 256;
const _: () = assert!(OUT_RING_LEN.is_power_of_two());

pub struct OutRing {
    buf: [AtomicU8; OUT_RING_LEN],
    /// Next byte to pop (the consumer's only), free-running.
    head: AtomicU32,
    /// Next byte to push (the producer's only), free-running.
    tail: AtomicU32,
    /// Messages the producer dropped, till the consumer takes the count.
    dropped: AtomicU32,
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
            dropped: AtomicU32::new(0),
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

    /// Consumer side only: the oldest waiting byte. Inlined into its
    /// caller, bank 2's USB poll, so `pop_into` stays the ISR's alone.
    #[inline(always)]
    pub fn pop(&self) -> Option<u8> {
        let head = self.head.load(Ordering::Relaxed);
        if self.tail.load(Ordering::Acquire) == head {
            return None;
        }
        let b = self.buf[head as usize % OUT_RING_LEN].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(b)
    }

    /// Producer side: `n` messages dropped.
    #[inline(always)]
    pub fn lose(&self, n: u32) {
        self.dropped.fetch_add(n, Ordering::Relaxed);
    }

    /// Consumer side: the messages dropped since it last asked.
    #[inline(always)]
    pub fn take_dropped(&self) -> u32 {
        self.dropped.swap(0, Ordering::Relaxed)
    }

    /// Consumer side: everything waiting dropped unread.
    #[inline(always)]
    pub fn clear(&self) {
        self.head
            .store(self.tail.load(Ordering::Acquire), Ordering::Release);
    }

    /// Nothing waits. Either side; a moment's answer.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
}

/// MIDI out's rings, one a `Port`: DIN's, and USB's where it is built.
#[derive(Clone, Copy)]
pub struct OutPorts<'a> {
    pub din: &'a OutRing,
    pub usb: Option<&'a OutRing>,
}

impl<'a> OutPorts<'a> {
    pub const fn new(din: &'a OutRing, usb: &'a OutRing) -> Self {
        Self {
            din,
            usb: Some(usb),
        }
    }

    /// DIN's alone.
    pub const fn din(din: &'a OutRing) -> Self {
        Self { din, usb: None }
    }

    /// One out pass over the ports (`Tee`).
    #[inline(always)]
    pub fn tee(&self) -> Tee<'a> {
        Tee {
            din: self.din,
            usb: self.usb,
            broken: false,
        }
    }
}

/// One out pass. DIN's ring decides what goes: a message it has no room
/// for waits for a later pass. Each sent is copied to USB's, never holding
/// DIN back; a copy that doesn't fit is lost and counted on USB's ring, and
/// so is the rest of the pass, whose running status would read wrong.
/// Inlined into its caller, bank 2's echo.
pub struct Tee<'a> {
    din: &'a OutRing,
    usb: Option<&'a OutRing>,
    broken: bool,
}

impl Tee<'_> {
    /// `msg` out of every port; false: DIN had no room, nothing went.
    #[inline(always)]
    pub fn send(&mut self, msg: &[u8]) -> bool {
        if !self.din.push_msg(msg) {
            return false;
        }
        self.copy(msg);
        true
    }

    /// `msg` out of `port` alone (an identity reply); false: DIN's had no
    /// room.
    #[inline(always)]
    pub fn send_to(&mut self, port: Port, msg: &[u8]) -> bool {
        match port {
            Port::Din => self.din.push_msg(msg),
            Port::Usb => {
                self.copy(msg);
                true
            }
        }
    }

    #[inline(always)]
    fn copy(&mut self, msg: &[u8]) {
        if let Some(u) = self.usb
            && (self.broken || !u.push_msg(msg))
        {
            self.broken = true;
            u.lose(1);
        }
    }
}
