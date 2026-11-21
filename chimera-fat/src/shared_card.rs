//! The UI's card behind the card bus (spec § 4.3): every touch of SPI2
//! holds the bus as `Ui`. A wait for it that runs past `take` is `Busy`:
//! the device untouched, never a card fault, never a re-init (C1). A
//! write while streams play quiets them first (decision 8).

use core::cell::Cell;
use core::fmt;

use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, CardFault, QUIET_SPIN_MS, Quiet, Taker, Wake};
use chimera_hal::store::StoreError;
use chimera_hal::units::Millis;
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

    /// `f` on the device, the bus held. Not re-entrant: a `SharedCard`
    /// call inside `f` waits out `take` for the bus `f` holds, then
    /// answers `Busy`.
    pub fn with_device<R>(&self, f: impl FnOnce(&D) -> R) -> Result<R, Busy> {
        let h = self.hold()?;
        Ok(f(h.dev))
    }

    /// Spins for the bus until `take` passes. A stream stuck past the
    /// watchdog is asked to abort, once a spin.
    fn hold(&self) -> Result<UiHold<'_, D>, Busy> {
        let start = (self.hooks.now)();
        let mut asked = false;
        loop {
            let now = (self.hooks.now)();
            if self.bus.try_take(Taker::Ui, now) {
                break Ok(UiHold {
                    dev: &self.dev,
                    bus: self.bus,
                    pend: self.hooks.pend,
                });
            }
            if let Some(h) = self.bus.stuck(now).filter(|_| !asked) {
                self.bus.ask_abort(h);
                (self.hooks.pend)();
                asked = true;
            }
            if now.elapsed_since(start) > self.take {
                break Err(Busy);
            }
        }
    }

    /// Streams playing and no quiet yet: asks one and spins until the
    /// audio holds it, `QUIET_SPIN_MS` at most. Before the hold (C1): a
    /// spin that runs out writes anyway, never `Busy`.
    fn quiet(&self) {
        if self.bus.streams_active().is_none() || self.bus.quiet() != Quiet::Off {
            return;
        }
        let start = (self.hooks.now)();
        self.bus.ask_quiet(start);
        while self.bus.quiet() != Quiet::Held
            && (self.hooks.now)().elapsed_since(start) < QUIET_SPIN_MS
        {
            core::hint::spin_loop();
        }
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

    /// Quiet first; each write keeps the quiet held (`note_write`).
    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Self::Error> {
        if !self.op_busy.get() {
            self.quiet();
        }
        let h = self.op_hold()?;
        let r = h.dev.write(blocks, start).map_err(SharedError::Dev);
        self.bus.note_write((self.hooks.now)());
        r
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

    /// The address mode goes with the card, once the bus is held: a busy
    /// re-init never ran, and the card IRQ keeps reading the card it had.
    fn reinit(&self) -> Result<(), StoreError> {
        let h = self.op_hold()?;
        self.addr.clear();
        h.dev.reinit()
    }

    fn mounted(&self) -> Result<(), StoreError> {
        let h = self.op_hold()?;
        h.dev.mounted()?;
        if let Some(m) = h.dev.addr_mode() {
            self.addr.set(m);
        }
        self.bus.set_fault(CardFault::Clear);
        Ok(())
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

    /// The one `mounted` published: asking the driver may re-acquire, I/O.
    fn addr_mode(&self) -> Option<AddrMode> {
        self.addr.get()
    }

    fn read_fault(&self) -> CardFault {
        self.bus.fault()
    }
}
