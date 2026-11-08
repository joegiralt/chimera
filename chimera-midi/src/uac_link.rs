//! What the audio interrupt, the USB interrupt and the class's bank-1
//! shims share for USB audio (ADR 0248): the frame ring and the streaming
//! interface's alt setting. Never globbed into bank 2: the audio
//! interrupt pushes and the class's `UsbClass` shims run from bank 1.

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use usb_device::class_prelude::InterfaceNumber;

/// A single-producer, single-consumer ring of stereo frames: the audio
/// interrupt pushes a block, the USB interrupt pops a packet. Lock-free and
/// wait-free both sides; `N` a power of two. Its slots are atomics so it
/// needs no `unsafe`: a relaxed load or store is a plain one on the M7.
pub struct Ring<const N: usize> {
    slots: [[AtomicI32; 2]; N],
    /// Frames pushed, wrapping: the producer's.
    head: AtomicU32,
    /// Frames popped, wrapping: the consumer's.
    tail: AtomicU32,
}

impl<const N: usize> Default for Ring<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Ring<N> {
    pub const fn new() -> Self {
        const { assert!(N.is_power_of_two() && N <= 1 << 16) };
        Ring {
            slots: [const { [AtomicI32::new(0), AtomicI32::new(0)] }; N],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    /// The producer's `n` frames, frame `i` from `f(i)`, all or none: a
    /// ring without room for them refuses the block (`false`). Inlined
    /// into the audio interrupt.
    #[inline(always)]
    pub fn push(&self, n: usize, mut f: impl FnMut(usize) -> [i32; 2]) -> bool {
        let h = self.head.load(Ordering::Relaxed);
        let used = h.wrapping_sub(self.tail.load(Ordering::Acquire)) as usize;
        if N - used.min(N) < n {
            return false;
        }
        for i in 0..n {
            let s = &self.slots[(h as usize).wrapping_add(i) & (N - 1)];
            let [l, r] = f(i);
            s[0].store(l, Ordering::Relaxed);
            s[1].store(r, Ordering::Relaxed);
        }
        self.head.store(h.wrapping_add(n as u32), Ordering::Release);
        true
    }

    /// Frames waiting, as the consumer sees them.
    #[inline(always)]
    pub fn len(&self) -> usize {
        let h = self.head.load(Ordering::Acquire);
        (h.wrapping_sub(self.tail.load(Ordering::Relaxed)) as usize).min(N)
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The consumer's next `n` frames (no more than wait) to `f`, oldest
    /// first; the count taken.
    #[inline(always)]
    pub fn pop(&self, n: usize, mut f: impl FnMut(usize, [i32; 2])) -> usize {
        let n = n.min(self.len());
        let t = self.tail.load(Ordering::Relaxed);
        for i in 0..n {
            let s = &self.slots[(t as usize).wrapping_add(i) & (N - 1)];
            f(
                i,
                [s[0].load(Ordering::Relaxed), s[1].load(Ordering::Relaxed)],
            );
        }
        self.tail.store(t.wrapping_add(n as u32), Ordering::Release);
        n
    }

    /// The consumer drops `n` frames unread (no more than wait).
    #[inline(always)]
    pub fn skip(&self, n: usize) -> usize {
        self.pop(n, |_, _| {})
    }
}

/// The streaming interface's alt setting: 0 stopped, 1 the stream. Its
/// `get`, `set` and `reset` are the class's `UsbClass` methods, as
/// usb-device asks every class in turn: `None` or `false` for any other
/// interface, so MIDI's and AudioControl's alt 0 still falls to the
/// device's default.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Alt {
    iface: InterfaceNumber,
    alt: u8,
}

impl Alt {
    #[inline(always)]
    pub const fn new(iface: InterfaceNumber) -> Alt {
        Alt { iface, alt: 0 }
    }

    #[inline(always)]
    pub fn iface(&self) -> InterfaceNumber {
        self.iface
    }

    /// The host has the stream on.
    #[inline(always)]
    pub fn streaming(&self) -> bool {
        self.alt == 1
    }

    #[inline(always)]
    pub fn get(&self, iface: InterfaceNumber) -> Option<u8> {
        (iface == self.iface).then_some(self.alt)
    }

    /// Alt 0 or 1 of this interface taken; any other refused, so the
    /// device stalls it.
    #[inline(always)]
    pub fn set(&mut self, iface: InterfaceNumber, alt: u8) -> bool {
        let ok = iface == self.iface && alt <= 1;
        if ok {
            self.alt = alt;
        }
        ok
    }

    /// A bus reset: stopped.
    #[inline(always)]
    pub fn reset(&mut self) {
        self.alt = 0;
    }
}
