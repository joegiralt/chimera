//! Loading while playing (projects spec § Loading while playing, ADR
//! 0046): a project load bumps the epoch; the audio fades every voice
//! through the old snapshot, acks, and holds the note queues until a
//! snapshot tagged with that epoch arrives.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::instrument::{AudioShared, Instrument};

/// How long the UI waits for the ack before it publishes anyway.
pub const LOAD_ACK_TIMEOUT_MS: u32 = 10;

/// The epoch the UI asks for and the last one the audio acked. Beside the
/// `AudioShared` triple buffer, not in it: the audio must see a new epoch
/// before the snapshot that carries it is published.
///
/// Ordering: only these counters ride the atomics; the snapshot itself
/// goes through the triple buffer's own release and acquire. Release on
/// the writes and Acquire on the reads keep each side's earlier work
/// (the parse, the kill) ordered before what the other side sees.
pub struct LoadLink {
    epoch: AtomicU32,
    ack: AtomicU32,
}

pub static LOAD_LINK: LoadLink = LoadLink::new();

impl Default for LoadLink {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadLink {
    pub const fn new() -> Self {
        Self {
            epoch: AtomicU32::new(0),
            ack: AtomicU32::new(0),
        }
    }

    /// The latest epoch: what the UI tags each snapshot with.
    pub fn epoch(&self) -> u32 {
        self.epoch.load(Ordering::Acquire)
    }

    /// A new epoch. Only the project load functions call it.
    pub(in crate::project) fn bump(&self) -> Swap {
        Swap {
            epoch: self.epoch.fetch_add(1, Ordering::Release).wrapping_add(1),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn bump_for_test(&self) -> Swap {
        self.bump()
    }

    /// The audio: every voice of epoch `e` is quiet.
    pub fn ack(&self, e: u32) {
        self.ack.store(e, Ordering::Release);
    }

    /// The UI: the audio acked epoch `e`.
    pub fn acked(&self, e: u32) -> bool {
        self.ack.load(Ordering::Acquire) == e
    }
}

/// An epoch bumped and not yet settled: the UI publishes after `settle`.
#[must_use]
#[derive(Debug)]
pub struct Swap {
    epoch: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    Acked,
    TimedOut,
}

impl Swap {
    /// Spins until the audio acks this epoch or `within` says time is up
    /// (`LOAD_ACK_TIMEOUT_MS`). Either way the UI publishes next.
    pub fn settle(self, link: &LoadLink, mut within: impl FnMut() -> bool) -> Settled {
        while !link.acked(self.epoch) && within() {
            core::hint::spin_loop();
        }
        if link.acked(self.epoch) {
            Settled::Acked
        } else {
            Settled::TimedOut
        }
    }
}

/// What the audio does this block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GateStep {
    /// Kill every voice (a new epoch).
    pub kill: bool,
    /// Drain the note queues.
    pub drain: bool,
    /// Ack this epoch.
    pub ack: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Draining: the snapshot carries the epoch.
    Open,
    /// Killed, the voices fading.
    Fading,
    /// Acked, waiting for the epoch's snapshot.
    Waiting,
}

/// The audio's side of a load: one `before_block` per callback.
pub struct LoadGate {
    seen: u32,
    phase: Phase,
}

impl Default for LoadGate {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadGate {
    pub const fn new() -> Self {
        Self {
            seen: 0,
            phase: Phase::Open,
        }
    }

    /// One block's step, given the epoch `requested`, the `snapshot`'s
    /// tag and whether every voice is `quiet` (ADR 0046's table).
    pub fn step(&mut self, requested: u32, snapshot: u32, quiet: bool) -> GateStep {
        let mut out = GateStep::default();
        if requested != self.seen {
            self.seen = requested;
            out.kill = true;
            // Published already: the UI timed out, or boot.
            self.phase = if snapshot == self.seen {
                Phase::Open
            } else {
                Phase::Fading
            };
            out.drain = self.phase == Phase::Open;
            return out;
        }
        match self.phase {
            Phase::Open => out.drain = true,
            Phase::Fading | Phase::Waiting if snapshot == self.seen => {
                self.phase = Phase::Open;
                out.drain = true;
            }
            Phase::Fading if quiet => {
                self.phase = Phase::Waiting;
                out.ack = Some(self.seen);
            }
            Phase::Fading | Phase::Waiting => {}
        }
        out
    }

    /// Before the drain: kills and acks as `step` says. Returns whether
    /// to drain.
    pub fn before_block(
        &mut self,
        link: &LoadLink,
        inst: &mut Instrument,
        shared: &AudioShared,
    ) -> bool {
        let s = self.step(link.epoch(), shared.epoch, inst.quiet());
        if s.kill {
            inst.kill_all();
        }
        if let Some(e) = s.ack {
            link.ack(e);
        }
        s.drain
    }
}
