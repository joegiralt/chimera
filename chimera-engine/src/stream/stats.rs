// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The streams' counters, for `stats`: atomics any context bumps.

use core::sync::atomic::{AtomicU32, Ordering};

use chimera_hal::units::Millis;

#[derive(Default)]
pub struct StreamStats {
    /// Drop-outs: a cursor that ran out of held frames.
    pub underruns: AtomicU32,
    /// Cold starts: a note-on without a head, or a return from quiet.
    pub cold: AtomicU32,
    /// A tripwire, not the card: a cursor audible with nothing to read, a
    /// jump that skipped `start`. Card read faults are `card_faults`.
    pub unplanned: AtomicU32,
    /// A tripwire for desktop QA: a jump that landed cold while the voice
    /// was still audible. The deck's duck should have faded it first, so
    /// each one may be a step.
    pub cold_loud: AtomicU32,
    /// A held note's reach rose mid-note past what the card could admit:
    /// it plays on at its cap (#595).
    pub held_at_cap: AtomicU32,
    /// A note whose source was moving at note-on, refused once it read.
    pub late_refused: AtomicU32,
    pub frame_errors: AtomicU32,
    /// Reads passed over: a want's chunk its file can't read, left
    /// Wanted and parked, once a want (`serve_step`, `serve::Parked`).
    pub unreadable: AtomicU32,
    /// Writes' quiets, ask to release: the longest, and how many.
    pub quiet_ms_max: AtomicU32,
    pub quiets: AtomicU32,
    /// The card IRQ's own cycles, wrapping every 8.9 s of its time: a
    /// reader folds them (`IrqCycles`). Its longest entry is
    /// `CardStats::irq_own_us_max`.
    pub irq_cycles: AtomicU32,
}

impl StreamStats {
    pub const fn new() -> StreamStats {
        StreamStats {
            underruns: AtomicU32::new(0),
            cold: AtomicU32::new(0),
            unplanned: AtomicU32::new(0),
            cold_loud: AtomicU32::new(0),
            held_at_cap: AtomicU32::new(0),
            late_refused: AtomicU32::new(0),
            frame_errors: AtomicU32::new(0),
            unreadable: AtomicU32::new(0),
            quiet_ms_max: AtomicU32::new(0),
            quiets: AtomicU32::new(0),
            irq_cycles: AtomicU32::new(0),
        }
    }

    /// A quiet settled after `len`.
    pub fn note_quiet(&self, len: Millis) {
        self.quiet_ms_max.fetch_max(len.get(), Ordering::Relaxed);
        self.quiets.fetch_add(1, Ordering::Relaxed);
    }
}

/// The card IRQ's wrapping cycles folded into 64 bits: folded more often
/// than they wrap (every UI pass), no gap between two reads loses one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IrqCycles {
    raw: u32,
    total: u64,
}

impl IrqCycles {
    /// From the counter's value `raw` now.
    pub const fn new(raw: u32) -> IrqCycles {
        IrqCycles { raw, total: 0 }
    }

    /// The counter now `raw`: the cycles since, added. The total.
    pub fn fold(&mut self, raw: u32) -> u64 {
        self.total = self
            .total
            .wrapping_add(u64::from(raw.wrapping_sub(self.raw)));
        self.raw = raw;
        self.total
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}
