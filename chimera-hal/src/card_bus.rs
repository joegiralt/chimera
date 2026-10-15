//! The card bus word (spec § 4.3): who owns SPI2 and the card, the UI or
//! the streaming IRQ, and the flags they pass each other. Atomics only,
//! so the UI, the audio and the card IRQ share one `static`.
//!
//! Wakeups, with none lost:
//! 1. The audio pends `DMA2_STR0` on every new want, whoever owns the bus.
//! 2. The IRQ takes only through `take_or_want`; on `Wanted` it returns.
//! 3. Whoever gives pends on `Wake::Pend`; the IRQ, after giving, re-scans
//!    the wants and pends if any is left.

#[cfg(not(loom))]
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::Ms;
use crate::units::{Millis, StreamCount, UiWait};

/// A write holds the streams quiet until this long passes without one.
pub const QUIET_HOLD_MS: Millis = Millis::new(20);
/// A `Stream` hold longer than this is stuck: the UI asks an abort.
pub const WATCHDOG_MS: Millis = Millis::new(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Owner {
    Free = 0,
    Ui = 1,
    Stream = 2,
}

impl Owner {
    const fn from_bits(b: u8) -> Owner {
        match b {
            1 => Owner::Ui,
            2 => Owner::Stream,
            _ => Owner::Free,
        }
    }
}

/// Who may take the bus: never `Free`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taker {
    Ui,
    Stream,
}

impl From<Taker> for Owner {
    fn from(t: Taker) -> Owner {
        match t {
            Taker::Ui => Owner::Ui,
            Taker::Stream => Owner::Stream,
        }
    }
}

/// The card IRQ's take: the bus, or a want the next give answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Took {
    Taken,
    Wanted,
}

/// A give's answer: `Pend` the card IRQ, someone wanted the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    Pend,
    Quiet,
}

/// The streams' quiet for a card write: asked by the writer, held by the
/// audio once its streamed voices are faded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Quiet {
    Off = 0,
    Asked = 1,
    Held = 2,
}

impl Quiet {
    const fn from_bits(b: u8) -> Quiet {
        match b {
            1 => Quiet::Asked,
            2 => Quiet::Held,
            _ => Quiet::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CardFault {
    Clear = 0,
    Read = 1,
}

pub struct CardBus {
    owner: AtomicU8,
    /// A `Stream` take's `Ms`, for the watchdog.
    since: AtomicU32,
    ui_waiting: AtomicBool,
    wanted: AtomicBool,
    abort: AtomicBool,
    quiet: AtomicU8,
    /// The last write's `Ms`, for `settle`.
    wrote: AtomicU32,
    fault: AtomicU8,
    streams_active: AtomicU8,
}

macro_rules! fresh {
    () => {
        CardBus {
            owner: AtomicU8::new(Owner::Free as u8),
            since: AtomicU32::new(0),
            ui_waiting: AtomicBool::new(false),
            wanted: AtomicBool::new(false),
            abort: AtomicBool::new(false),
            quiet: AtomicU8::new(Quiet::Off as u8),
            wrote: AtomicU32::new(0),
            fault: AtomicU8::new(CardFault::Clear as u8),
            streams_active: AtomicU8::new(0),
        }
    };
}

#[cfg(not(loom))]
impl Default for CardBus {
    fn default() -> Self {
        Self::new()
    }
}

impl CardBus {
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        fresh!()
    }

    #[cfg(loom)]
    pub fn new() -> Self {
        fresh!()
    }

    /// Free → `t`. A `Stream` take stamps `now` first, published by the
    /// CAS's release: only the IRQ takes `Stream`, so a failed take
    /// overwrites no live stamp.
    pub fn try_take(&self, t: Taker, now: Ms) -> bool {
        self.take(t, now, Ordering::AcqRel, Ordering::Relaxed)
    }

    fn take(&self, t: Taker, now: Ms, ok: Ordering, fail: Ordering) -> bool {
        if t == Taker::Stream {
            self.since.store(now.0, Ordering::Relaxed);
        }
        self.owner
            .compare_exchange(Owner::Free as u8, Owner::from(t) as u8, ok, fail)
            .is_ok()
    }

    /// The card IRQ's only take. A failed CAS stores `wanted`, then
    /// retries once; the store and the retry are `SeqCst`, against
    /// `give`'s two, so neither side's read can pass its own write.
    pub fn take_or_want(&self, now: Ms) -> Took {
        if self.try_take(Taker::Stream, now) {
            return Took::Taken;
        }
        self.wanted.store(true, Ordering::SeqCst);
        if self.take(Taker::Stream, now, Ordering::SeqCst, Ordering::SeqCst) {
            Took::Taken
        } else {
            Took::Wanted
        }
    }

    /// Stores `Free` first, then swaps `wanted` out; `Pend` if it was
    /// set. Together with `take_or_want`'s want-then-retry they lose no
    /// wakeup: a want the swap misses landed after `Free` was stored, so
    /// the retry takes the bus. Both sides are `SeqCst` (a `Release`
    /// store could pass the swap's read: store buffering).
    pub fn give(&self) -> Wake {
        self.owner.store(Owner::Free as u8, Ordering::SeqCst);
        if self.wanted.swap(false, Ordering::SeqCst) {
            Wake::Pend
        } else {
            Wake::Quiet
        }
    }

    pub fn owner(&self) -> Owner {
        Owner::from_bits(self.owner.load(Ordering::Acquire))
    }

    /// The bus is `Stream`'s and has been for over `WATCHDOG_MS`.
    pub fn stuck(&self, now: Ms) -> bool {
        self.owner() == Owner::Stream
            && now.elapsed_since(Ms(self.since.load(Ordering::Relaxed))) > WATCHDOG_MS
    }

    pub fn ask_abort(&self) {
        self.abort.store(true, Ordering::Release);
    }

    /// The IRQ's read of an abort: once per ask.
    pub fn take_abort(&self) -> bool {
        self.abort.swap(false, Ordering::AcqRel)
    }

    pub fn set_ui_wait(&self, w: UiWait) {
        self.ui_waiting
            .store(w == UiWait::Waiting, Ordering::Release);
    }

    pub fn ui_wait(&self) -> UiWait {
        if self.ui_waiting.load(Ordering::Acquire) {
            UiWait::Waiting
        } else {
            UiWait::Idle
        }
    }

    /// Off → Asked; otherwise nothing.
    pub fn ask_quiet(&self) {
        self.quiet_step(Quiet::Off, Quiet::Asked);
    }

    /// Asked → Held; otherwise nothing.
    pub fn answer_quiet(&self) {
        self.quiet_step(Quiet::Asked, Quiet::Held);
    }

    pub fn quiet(&self) -> Quiet {
        Quiet::from_bits(self.quiet.load(Ordering::Acquire))
    }

    pub fn note_write(&self, now: Ms) {
        self.wrote.store(now.0, Ordering::Release);
    }

    /// Held → Off once `QUIET_HOLD_MS` passes without a write; true if it
    /// did.
    pub fn settle(&self, now: Ms) -> bool {
        self.quiet() == Quiet::Held
            && now.elapsed_since(Ms(self.wrote.load(Ordering::Acquire))) >= QUIET_HOLD_MS
            && self.quiet_step(Quiet::Held, Quiet::Off)
    }

    fn quiet_step(&self, from: Quiet, to: Quiet) -> bool {
        self.quiet
            .compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    pub fn set_fault(&self, f: CardFault) {
        self.fault.store(f as u8, Ordering::Release);
    }

    pub fn fault(&self) -> CardFault {
        match self.fault.load(Ordering::Acquire) {
            0 => CardFault::Clear,
            _ => CardFault::Read,
        }
    }

    pub fn set_streams_active(&self, c: Option<StreamCount>) {
        self.streams_active
            .store(StreamCount::to_bits(c), Ordering::Relaxed);
    }

    pub fn streams_active(&self) -> Option<StreamCount> {
        StreamCount::from_bits(self.streams_active.load(Ordering::Relaxed))
    }
}
