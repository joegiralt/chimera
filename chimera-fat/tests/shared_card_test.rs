#[path = "common/image.rs"]
mod image;

use chimera_core::stream::stats::StreamStats;
use chimera_fat::sd_addr::{AddrMode, CardAddr};
use chimera_fat::shared_card::{Hooks, SharedCard, SharedError};
use chimera_fat::{FatStore, Medium};
use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, CardFault, Owner, Quiet, Taker, WATCHDOG_MS};
use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};
use chimera_hal::units::{Millis, StreamCount, UiWait};
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
    /// The audio's quiet answer, at this ms.
    static ANSWER_AT: Cell<Option<(u32, &'static CardBus)>> = const { Cell::new(None) };
    /// The bus's quiet at each device write.
    static WROTE: RefCell<Vec<Quiet>> = const { RefCell::new(Vec::new()) };
    static PENDS: Cell<u32> = const { Cell::new(0) };
    /// Every device call and the stream's give, with the bus's owner then.
    static LOG: RefCell<Vec<(&'static str, Owner)>> = const { RefCell::new(Vec::new()) };
}

/// Tests may share a thread: each starts from a fresh clock and log.
fn fresh() {
    CLOCK.set(0);
    TAKE_AT.set(None);
    GIVE_AT.set(None);
    ANSWER_AT.set(None);
    WROTE.with_borrow_mut(Vec::clear);
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
    if let Some((at, bus)) = ANSWER_AT.get()
        && t == at
    {
        bus.answer_quiet(Ms(t));
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
        WROTE.with_borrow_mut(|w| w.push(self.bus.quiet()));
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

    fn reinit(&self) -> Result<(), StoreError> {
        note("reinit", self.bus);
        Ok(())
    }

    fn mounted(&self) -> Result<(), StoreError> {
        note("mounted", self.bus);
        Ok(())
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

fn write(c: &SharedCard<Fake>) -> Result<(), SharedError<DiskError>> {
    c.write(&[Block::new()], BlockIdx(0))
}

fn wrote() -> Vec<Quiet> {
    WROTE.with_borrow(Clone::clone)
}

/// Decision 8: with streams playing, a write asks them quiet and waits for
/// the audio to hold it.
#[test]
fn write_quiets_when_streams_active() {
    let (c, bus, _) = card(false);
    bus.set_streams_active(StreamCount::new(2));
    ANSWER_AT.set(Some((12, bus)));
    assert_eq!(write(&c), Ok(()));
    assert_eq!(wrote(), [Quiet::Held]);
    assert!(CLOCK.get() > 12);
    assert_eq!(bus.owner(), Owner::Free);
}

#[test]
fn write_without_streams_does_not_quiet() {
    let (c, bus, _) = card(false);
    assert_eq!(write(&c), Ok(()));
    assert_eq!(wrote(), [Quiet::Off]);
    assert_eq!(bus.quiet(), Quiet::Off);
    assert!(CLOCK.get() < 5, "no spin: {}", CLOCK.get());
}

/// The audio never answers: the write runs after 50 ms, never `Busy`
/// (C1), and the quiet's length, once it settles, reaches the stats.
#[test]
fn quiet_spin_gives_up_after_50ms() {
    let (c, bus, _) = card(false);
    bus.set_streams_active(StreamCount::new(1));
    assert_eq!(write(&c), Ok(()));
    assert_eq!(wrote(), [Quiet::Asked]);
    let spun = CLOCK.get();
    assert!((50..TAKE.get()).contains(&spun), "{spun}");
    // A late answer: held, then settled 20 ms after it.
    bus.answer_quiet(Ms(90));
    assert_eq!(bus.settle(Ms(109)), None);
    let len = bus.settle(Ms(110)).expect("settles");
    assert!(len >= Millis::new(50), "{len:?}");
    let stats = StreamStats::new();
    stats.note_quiet(len);
    assert_eq!(
        stats
            .quiet_ms_max
            .load(core::sync::atomic::Ordering::Relaxed),
        len.get()
    );
    assert_eq!(stats.quiets.load(core::sync::atomic::Ordering::Relaxed), 1);
}

/// A save's many writes, reads between: one ask, one spin, the quiet held
/// throughout; it settles once 20 ms pass without a write.
#[test]
fn quiet_holds_across_a_burst_and_settles() {
    let (c, bus, _) = card(false);
    bus.set_streams_active(StreamCount::new(1));
    ANSWER_AT.set(Some((5, bus)));
    assert_eq!(write(&c), Ok(()));
    for _ in 0..8 {
        assert_eq!(read(&c), Ok(()));
        let t = CLOCK.get();
        assert_eq!(write(&c), Ok(()));
        assert!(CLOCK.get() - t < 10, "no second spin");
        assert_eq!(bus.settle(Ms(CLOCK.get())), None, "mid-burst");
    }
    assert_eq!(wrote(), [Quiet::Held; 9]);
    let last = CLOCK.get();
    assert_eq!(bus.settle(Ms(last + 10)), None);
    let len = bus.settle(Ms(last + 20)).expect("settles");
    assert_eq!(len, Millis::new(last + 20), "from the ask at 0");
    assert_eq!(bus.quiet(), Quiet::Off);
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
fn a_stream_between_identify_and_mounted_makes_the_mount_busy() {
    let (c, bus, addr) = card(false);
    let mut store = FatStore::new(c);
    // start_op, the MBR's read and the boot sector's read take two ms
    // each: the stream takes the bus as `mounted` asks for it.
    TAKE_AT.set(Some((6, bus)));
    assert_eq!(store.mount(), Err(StoreError::Busy));
    assert_eq!(addr.get(), None);
    let calls: Vec<_> = log().into_iter().map(|(name, _)| name).collect();
    assert_eq!(calls, ["start_op", "read", "read"]);
    bus.give();
    assert!(store.mount().is_ok());
    assert_eq!(addr.get(), Some(AddrMode::Block));
}

/// The card IRQ's read fault reaches the store; its recovery re-inits
/// under the bus, then mounts, which clears it.
#[test]
fn recover_reinits_under_the_bus_and_clears_the_fault() {
    let (c, bus, _) = card(false);
    let mut store = FatStore::new(c);
    bus.set_fault(CardFault::Read);
    assert_eq!(store.read_fault(), CardFault::Read);
    assert!(store.recover().is_ok());
    assert_eq!(store.read_fault(), CardFault::Clear);
    let calls: Vec<_> = log().into_iter().take(3).collect();
    assert_eq!(
        calls,
        [
            ("start_op", Owner::Ui),
            ("reinit", Owner::Ui),
            ("start_op", Owner::Ui)
        ]
    );
}

#[test]
fn every_medium_io_call_takes_the_bus() {
    let (c, bus, _) = card(false);
    assert!(bus.try_take(Taker::Stream, Ms(0)));
    // Before `start_op`: each waits out the bus itself.
    assert_eq!(c.reinit(), Err(StoreError::Busy));
    assert_eq!(c.mounted(), Err(StoreError::Busy));
    c.start_op();
    assert!(c.with_device(|_| ()).is_err());
    assert_eq!(log(), []);
    bus.give();
    c.start_op();
    assert_eq!(c.reinit(), Ok(()));
    assert_eq!(c.mounted(), Ok(()));
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
    assert_eq!(c.mounted(), Ok(()));
    assert_eq!(addr.get(), Some(AddrMode::Block));
    assert_eq!(bus.fault(), CardFault::Clear);
    assert_eq!(c.reinit(), Ok(()));
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
