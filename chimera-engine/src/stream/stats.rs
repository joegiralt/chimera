//! The streams' counters, for `stats`: atomics any context bumps.

use core::sync::atomic::AtomicU32;

#[derive(Default)]
pub struct StreamStats {
    /// Drop-outs: a cursor that ran out of held frames.
    pub underruns: AtomicU32,
    /// Cold starts: a note-on without a head, or a return from quiet.
    pub cold: AtomicU32,
    pub faults: AtomicU32,
    pub frame_errors: AtomicU32,
    pub quiet_ms_max: AtomicU32,
    pub irq_us_max: AtomicU32,
    pub irq_us_sum: AtomicU32,
}

impl StreamStats {
    pub const fn new() -> StreamStats {
        StreamStats {
            underruns: AtomicU32::new(0),
            cold: AtomicU32::new(0),
            faults: AtomicU32::new(0),
            frame_errors: AtomicU32::new(0),
            quiet_ms_max: AtomicU32::new(0),
            irq_us_max: AtomicU32::new(0),
            irq_us_sum: AtomicU32::new(0),
        }
    }
}
