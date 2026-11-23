// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loading while playing (projects spec § Loading while playing, ADR
//! 0046): a project load bumps the epoch; the audio fades every voice
//! through the old snapshot, acks, and holds the note queues until a
//! snapshot tagged with that epoch arrives.

#[cfg(feature = "bank2-gate")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::instrument::{AudioShared, Instrument};
use crate::orbit::runner::OrbitRunner;

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
    /// A bank-2 write is on: the audio sounds TAPE's voices off.
    #[cfg(feature = "bank2-gate")]
    tape_shut: AtomicBool,
    /// The audio: no TAPE voice is sounding.
    #[cfg(feature = "bank2-gate")]
    tape_quiet: AtomicBool,
}

pub static LOAD_LINK: LoadLink = LoadLink::new();

impl Default for LoadLink {
    #[optimize(size)]
    fn default() -> Self {
        Self::new()
    }
}

impl LoadLink {
    #[optimize(size)]
    pub const fn new() -> Self {
        Self {
            epoch: AtomicU32::new(0),
            ack: AtomicU32::new(0),
            #[cfg(feature = "bank2-gate")]
            tape_shut: AtomicBool::new(false),
            #[cfg(feature = "bank2-gate")]
            tape_quiet: AtomicBool::new(false),
        }
    }

    /// The UI, before bank 2's door closes: TAPE's voices go off, and stay
    /// off until `open_tape`.
    #[cfg(feature = "bank2-gate")]
    #[optimize(size)]
    pub fn shut_tape(&self) {
        self.tape_quiet.store(false, Ordering::Release);
        self.tape_shut.store(true, Ordering::Release);
    }

    /// The UI, once the write is done: TAPE may sound again.
    #[cfg(feature = "bank2-gate")]
    #[optimize(size)]
    pub fn open_tape(&self) {
        self.tape_shut.store(false, Ordering::Release);
    }

    /// The UI: the audio reports no TAPE voice sounding.
    #[cfg(feature = "bank2-gate")]
    #[optimize(size)]
    pub fn tape_quiet(&self) -> bool {
        self.tape_quiet.load(Ordering::Acquire)
    }

    /// The latest epoch: what the UI tags each snapshot with.
    #[optimize(size)]
    pub fn epoch(&self) -> u32 {
        self.epoch.load(Ordering::Acquire)
    }

    /// A new epoch. Only the project load functions call it.
    #[optimize(size)]
    pub(in crate::project) fn bump(&self) -> Swap {
        Swap {
            epoch: self.epoch.fetch_add(1, Ordering::Release).wrapping_add(1),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[optimize(size)]
    pub fn bump_for_test(&self) -> Swap {
        self.bump()
    }

    /// The audio: every voice of epoch `e` is quiet.
    #[optimize(size)]
    pub fn ack(&self, e: u32) {
        self.ack.store(e, Ordering::Release);
    }

    /// The UI: the audio acked epoch `e`.
    #[optimize(size)]
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
    #[optimize(size)]
    pub(in crate::project) fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Spins until the audio acks this epoch or `within` says time is up
    /// (`LOAD_ACK_TIMEOUT_MS`). Either way the UI publishes next.
    #[optimize(size)]
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
    #[optimize(size)]
    fn default() -> Self {
        Self::new()
    }
}

impl LoadGate {
    #[optimize(size)]
    pub const fn new() -> Self {
        Self {
            seen: 0,
            phase: Phase::Open,
        }
    }

    /// One block's step, given the epoch `requested`, the `snapshot`'s
    /// tag and whether every voice is `quiet` (ADR 0046's table).
    #[optimize(size)]
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
    /// to drain. A gate build's `bank2 erase` shuts TAPE's voices and
    /// ORBIT (`orbit`) first, and says quiet once neither sounds (#784).
    #[optimize(size)]
    pub fn before_block(
        &mut self,
        link: &LoadLink,
        inst: &mut Instrument,
        shared: &AudioShared,
        #[cfg_attr(not(feature = "bank2-gate"), allow(unused_variables))] orbit: Option<
            &mut OrbitRunner,
        >,
    ) -> bool {
        #[cfg(feature = "bank2-gate")]
        if link.tape_shut.load(Ordering::Acquire) {
            let stopped = orbit.is_none_or(|o| o.shut(inst, shared));
            inst.sound_off_tape(shared);
            link.tape_quiet
                .store(stopped && !inst.tape_sounding(shared), Ordering::Release);
        } else if let Some(o) = orbit {
            o.reopen();
        }
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
