#[path = "common/image.rs"]
mod image;

use chimera_fat::sd_addr::{AddrMode, CardAddr};
use chimera_fat::shared_card::{Hooks, SharedCard, SharedError};
use chimera_fat::{FatStore, Medium};
use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, CardFault, Owner, Taker, WATCHDOG_MS};
use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};
use chimera_hal::units::{Millis, UiWait};
use core::cell::{Cell, RefCell};
use core::ops::ControlFlow;
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx};
use image::{DiskError, RamDisk, fat16};

const TAKE: Millis = Millis::new(600);

thread_local! {
    /// The stepping clock: each `now` is 1 ms past the last.
    static CLOCK: Cell<u32> = const { Cell::new(0) };
    /// The stream's take, at this ms.
    static TAKE_AT: Cell<Option<(u32, &'static CardBus)>> = const { Cell::new(None) };
    /// The stream's give, at this ms.
    static GIVE_AT: Cell<Option<(u32, &'static CardBus)>> = const { Cell::new(None) };
    static PENDS: Cell<u32> = const { Cell::new(0) };
    /// Every device call and the stream's give, with the bus's owner then.
    static LOG: RefCell<Vec<(&'static str, Owner)>> = const { RefCell::new(Vec::new()) };
}

/// Tests may share a thread: each starts from a fresh clock and log.
fn fresh() {
    CLOCK.set(0);
    TAKE_AT.set(None);
    GIVE_AT.set(None);
    PENDS.set(0);
    LOG.with_borrow_mut(Vec::clear);
}

fn now() -> Ms {
    let t = CLOCK.get();
    CLOCK.set(t + 1);
    if let Some((at, bus)) = TAKE_AT.get()
        && t == at
    {
        assert!(bus.try_take(Taker::Stream, Ms(t)));
    }
    if let Some((at, bus)) = GIVE_AT.get()
        && t == at
    {
        bus.give();
        note("give", bus);
    }
    Ms(t)
}

fn pend() {
    PENDS.set(PENDS.get() + 1);
}

fn note(name: &'static str, bus: &CardBus) {
    LOG.with_borrow_mut(|l| l.push((name, bus.owner())));
}

fn log() -> Vec<(&'static str, Owner)> {
    LOG.with_borrow(Clone::clone)
}

/// A card that logs every call by name; `want`: each read marks the bus
/// wanted, as the card IRQ's take does.
struct Fake {
    disk: RamDisk,
    bus: &'static CardBus,
    want: bool,
}

impl BlockDevice for Fake {
    type Error = DiskError;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), DiskError> {
        note("read", self.bus);
        if self.want {
            self.bus.take_or_want(Ms(0));
        }
        self.disk.read(blocks, start)
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), DiskError> {
        note("write", self.bus);
        self.disk.write(blocks, start)
    }

    fn num_blocks(&self) -> Result<BlockCount, DiskError> {
        note("num_blocks", self.bus);
        self.disk.num_blocks()
    }
}

impl Medium for Fake {
    fn start_op(&self) {
        note("start_op", self.bus);
    }

    fn reinit(&self) {
        note("reinit", self.bus);
    }

    fn mounted(&self) {
        note("mounted", self.bus);
    }

    fn addr_mode(&self) -> Option<AddrMode> {
        Some(AddrMode::Block)
    }
}

fn leak<T>(t: T) -> &'static T {
    Box::leak(Box::new(t))
}

fn card(want: bool) -> (SharedCard<Fake>, &'static CardBus, &'static CardAddr) {
    fresh();
    let (bus, addr) = (leak(CardBus::new()), leak(CardAddr::new()));
    let fake = Fake {
        disk: fat16(16_384, 7),
        bus,
        want,
    };
    let hooks = Hooks { pend, now };
    (SharedCard::new(fake, bus, addr, hooks, TAKE), bus, addr)
}

fn read(c: &SharedCard<Fake>) -> Result<(), SharedError<DiskError>> {
    c.read(&mut [Block::new()], BlockIdx(0))
}

#[test]
fn ui_call_takes_and_gives_the_bus() {
    let (c, bus, _) = card(false);
    assert_eq!(read(&c), Ok(()));
    assert_eq!(log(), [("read", Owner::Ui)]);
    assert_eq!(bus.owner(), Owner::Free);
    assert_eq!(PENDS.get(), 0);
}

#[test]
fn ui_call_pends_when_wanted() {
    let (c, bus, _) = card(true);
    assert_eq!(read(&c), Ok(()));
    assert_eq!(bus.owner(), Owner::Free);
    assert_eq!(PENDS.get(), 1);
}

#[test]
fn ui_waits_for_stream_then_proceeds() {
    let (c, bus, _) = card(false);
    assert!(bus.try_take(Taker::Stream, Ms(0)));
    GIVE_AT.set(Some((5, bus)));
    assert_eq!(read(&c), Ok(()));
    assert_eq!(log(), [("give", Owner::Free), ("read", Owner::Ui)]);
    assert_eq!(bus.ui_wait(), UiWait::Idle);
}

#[test]
fn ui_times_out_busy_untouched() {
    let (c, bus, _) = card(false);
    assert!(bus.try_take(Taker::Stream, Ms(0)));
    assert_eq!(read(&c), Err(SharedError::Busy));
    assert!(CLOCK.get() > TAKE.get());
    assert_eq!(log(), []);
    assert_eq!(bus.owner(), Owner::Stream);
    assert_eq!(bus.ui_wait(), UiWait::Idle);
}

struct Drain;

impl ReadSink for Drain {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, _bytes: &[u8]) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}

#[test]
fn fat_store_over_a_held_bus_never_reinits() {
    let (c, bus, _) = card(false);
    let mut store = FatStore::new(c);
    let vol = store.mount().unwrap();
    LOG.with_borrow_mut(Vec::clear);
    assert!(bus.try_take(Taker::Stream, Ms(CLOCK.get())));
    assert_eq!(store.mount(), Err(StoreError::Busy));
    let file = FileName::new(Dir::Chimera, b"TEST", b"BIN").unwrap();
    assert_eq!(store.read(vol, file, &mut Drain), Err(StoreError::Busy));
    assert_eq!(log(), []);
}

#[test]
fn a_mount_the_stream_interrupts_is_busy_not_reinit() {
    let (c, bus, _) = card(false);
    let mut store = FatStore::new(c);
    // start_op and the MBR's read take two ms each; the boot sector's
    // read waits out `take`, and the stream gives after it.
    TAKE_AT.set(Some((4, bus)));
    GIVE_AT.set(Some((TAKE.get() + 100, bus)));
    assert_eq!(store.mount(), Err(StoreError::Busy));
    let calls: Vec<_> = log().into_iter().map(|(name, _)| name).collect();
    assert_eq!(calls, ["start_op", "read"]);
}

#[test]
fn every_medium_io_call_takes_the_bus() {
    let (c, bus, _) = card(false);
    assert!(bus.try_take(Taker::Stream, Ms(0)));
    c.start_op();
    c.reinit();
    c.mounted();
    assert!(c.with_device(|_| ()).is_err());
    assert_eq!(log(), []);
    bus.give();
    c.start_op();
    c.reinit();
    c.mounted();
    assert_eq!(c.with_device(|_| bus.owner()).ok(), Some(Owner::Ui));
    assert_eq!(
        log(),
        [
            ("start_op", Owner::Ui),
            ("reinit", Owner::Ui),
            ("mounted", Owner::Ui)
        ]
    );
    assert_eq!(bus.owner(), Owner::Free);
}

#[test]
fn mounted_publishes_the_card_addr_and_reinit_clears_it() {
    let (c, bus, addr) = card(false);
    bus.set_fault(CardFault::Read);
    c.mounted();
    assert_eq!(addr.get(), Some(AddrMode::Block));
    assert_eq!(bus.fault(), CardFault::Clear);
    c.reinit();
    assert_eq!(addr.get(), None);
}

#[test]
fn a_stuck_stream_is_asked_to_abort() {
    const _: () = assert!(TAKE.get() > WATCHDOG_MS.get());
    let (c, bus, _) = card(false);
    assert!(bus.try_take(Taker::Stream, Ms(0)));
    assert_eq!(read(&c), Err(SharedError::Busy));
    assert!(bus.take_abort());
    assert_eq!(PENDS.get(), 1);
}
