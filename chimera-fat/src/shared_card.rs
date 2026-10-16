//! The UI's card behind the card bus (spec § 4.3): every touch of SPI2
//! holds the bus as `Ui`. A wait for it that runs past `take` is `Busy`:
//! the device untouched, never a card fault, never a re-init (C1).

use core::cell::Cell;
use core::fmt;

use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, CardFault, Taker, Wake};
use chimera_hal::store::StoreError;
use chimera_hal::units::{Millis, UiWait};
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};

use crate::sd_addr::{AddrMode, CardAddr};
use crate::store::Medium;

/// The UI's wait for the bus ran out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedError<E> {
    Busy,
    Dev(E),
}

impl<E> From<Busy> for SharedError<E> {
    fn from(_: Busy) -> Self {
        SharedError::Busy
    }
}

impl From<Busy> for StoreError {
    fn from(_: Busy) -> Self {
        StoreError::Busy
    }
}

impl<E: fmt::Display> fmt::Display for SharedError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SharedError::Busy => f.write_str("card bus busy"),
            SharedError::Dev(e) => e.fmt(f),
        }
    }
}

impl<E: core::error::Error> core::error::Error for SharedError<E> {}

/// The board's: pend the card IRQ, and the ms clock.
#[derive(Clone, Copy)]
pub struct Hooks {
    pub pend: fn(),
    pub now: fn() -> Ms,
}

/// The bus held as `Ui`, and the one way to the device: made only by
/// `SharedCard::hold`. Dropping it gives the bus, and pends the card IRQ
/// if the bus was wanted.
pub struct UiHold<'a, D> {
    dev: &'a D,
    bus: &'a CardBus,
    pend: fn(),
}

impl<D> Drop for UiHold<'_, D> {
    fn drop(&mut self) {
        if self.bus.give() == Wake::Pend {
            (self.pend)();
        }
    }
}

pub struct SharedCard<D> {
    dev: D,
    bus: &'static CardBus,
    addr: &'static CardAddr,
    hooks: Hooks,
    take: Millis,
    /// This operation's `start_op` found the bus busy: every call until
    /// the next `start_op` answers `Busy`.
    op_busy: Cell<bool>,
}

impl<D: Medium> SharedCard<D> {
    pub fn new(
        dev: D,
        bus: &'static CardBus,
        addr: &'static CardAddr,
        hooks: Hooks,
        take: Millis,
    ) -> Self {
        Self {
            dev,
            bus,
            addr,
            hooks,
            take,
            op_busy: Cell::new(false),
        }
    }

    /// `f` on the device, the bus held.
    pub fn with_device<R>(&self, f: impl FnOnce(&D) -> R) -> Result<R, Busy> {
        let h = self.hold()?;
        Ok(f(h.dev))
    }

    /// Spins for the bus until `take` passes. A stream stuck past the
    /// watchdog is asked to abort, once a spin.
    fn hold(&self) -> Result<UiHold<'_, D>, Busy> {
        let start = (self.hooks.now)();
        self.bus.set_ui_wait(UiWait::Waiting);
        let mut asked = false;
        let held = loop {
            let now = (self.hooks.now)();
            if self.bus.try_take(Taker::Ui, now) {
                break Ok(UiHold {
                    dev: &self.dev,
                    bus: self.bus,
                    pend: self.hooks.pend,
                });
            }
            if !asked && self.bus.stuck(now) {
                self.bus.ask_abort();
                (self.hooks.pend)();
                asked = true;
            }
            if now.elapsed_since(start) > self.take {
                break Err(Busy);
            }
        };
        self.bus.set_ui_wait(UiWait::Idle);
        held
    }

    /// `hold`, unless this operation already found the bus busy.
    fn op_hold(&self) -> Result<UiHold<'_, D>, Busy> {
        if self.op_busy.get() {
            return Err(Busy);
        }
        self.hold()
    }
}

impl<D: Medium> BlockDevice for SharedCard<D> {
    type Error = SharedError<D::Error>;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Self::Error> {
        let h = self.op_hold()?;
        h.dev.read(blocks, start).map_err(SharedError::Dev)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        let h = self.op_hold()?;
        h.dev.write(blocks, start).map_err(SharedError::Dev)
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        let h = self.op_hold()?;
        h.dev.num_blocks().map_err(SharedError::Dev)
    }
}

impl<D: Medium> Medium for SharedCard<D> {
    /// An acquire's CMD0 is I/O: under a hold, or the operation is busy.
    fn start_op(&self) {
        self.op_busy.set(false);
        match self.hold() {
            Ok(h) => h.dev.start_op(),
            Err(Busy) => self.op_busy.set(true),
        }
    }

    /// The address mode goes with the card. A busy bus skips the re-init:
    /// the next operation's error asks it again.
    fn reinit(&self) {
        self.addr.clear();
        if let Ok(h) = self.op_hold() {
            h.dev.reinit();
        }
    }

    fn mounted(&self) {
        if let Ok(h) = self.op_hold() {
            h.dev.mounted();
            if let Some(m) = h.dev.addr_mode() {
                self.addr.set(m);
            }
            self.bus.set_fault(CardFault::Clear);
        }
    }

    fn cap(&self, ms: Option<u32>) {
        self.dev.cap(ms);
    }

    fn classify(&self, e: &Self::Error) -> StoreError {
        match e {
            SharedError::Busy => StoreError::Busy,
            SharedError::Dev(e) => self.dev.classify(e),
        }
    }

    fn elapsed_ms(&self) -> u32 {
        self.dev.elapsed_ms()
    }

    fn addr_mode(&self) -> Option<AddrMode> {
        self.dev.addr_mode()
    }
}
