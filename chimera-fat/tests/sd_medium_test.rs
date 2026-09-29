use chimera_fat::{BusPhase, FatStore, FixedTime, Medium, SdBus};
use chimera_hal::store::{Store, StoreError};
use core::cell::{Cell, RefCell};
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::{ErrorKind, ErrorType, Operation, SpiDevice};
use embedded_sdmmc::sdcard::spi::AcquireOpts;
use embedded_sdmmc::{SdCard, SdCardError};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Wake,
    SetPhase(BusPhase),
    StartOp,
}

/// What the bus and the delay saw.
#[derive(Default)]
struct Log {
    calls: RefCell<Vec<Call>>,
    transactions: Cell<u32>,
    delays: Cell<u32>,
}

/// An empty slot: MISO floats high (0xFF), and the deadline passes after
/// `limit` transactions, failing every transaction until the next
/// `start_op`, as `SdSpi`'s does.
struct FakeBus {
    log: Rc<Log>,
    phase: BusPhase,
    limit: u32,
    since_start: u32,
    timed_out: bool,
}

impl FakeBus {
    fn new(log: &Rc<Log>, limit: u32) -> Self {
        FakeBus {
            log: log.clone(),
            phase: BusPhase::Acquire,
            limit,
            since_start: 0,
            timed_out: false,
        }
    }
}

impl ErrorType for FakeBus {
    type Error = ErrorKind;
}

impl SpiDevice<u8> for FakeBus {
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), ErrorKind> {
        self.log.transactions.set(self.log.transactions.get() + 1);
        self.since_start += 1;
        self.timed_out |= self.since_start > self.limit;
        if self.timed_out {
            return Err(ErrorKind::Other);
        }
        for op in ops {
            match op {
                Operation::Read(b) | Operation::TransferInPlace(b) | Operation::Transfer(b, _) => {
                    b.fill(0xFF)
                }
                Operation::Write(_) | Operation::DelayNs(_) => {}
            }
        }
        Ok(())
    }
}

impl SdBus for FakeBus {
    fn wake(&mut self) {
        self.log.calls.borrow_mut().push(Call::Wake);
    }

    fn set_phase(&mut self, p: BusPhase) {
        self.log.calls.borrow_mut().push(Call::SetPhase(p));
        self.phase = p;
    }

    fn phase(&self) -> BusPhase {
        self.phase
    }

    fn start_op(&mut self) {
        self.log.calls.borrow_mut().push(Call::StartOp);
        (self.since_start, self.timed_out) = (0, false);
    }

    fn timed_out(&self) -> bool {
        self.timed_out
    }
}

struct NoDelay(Rc<Log>);

impl DelayNs for NoDelay {
    fn delay_ns(&mut self, _ns: u32) {
        self.0.delays.set(self.0.delays.get() + 1);
    }
}

fn card(log: &Rc<Log>, limit: u32) -> SdCard<FakeBus, NoDelay> {
    let opts = AcquireOpts {
        acquire_retries: 3,
        use_crc: true,
    };
    SdCard::new_with_options(FakeBus::new(log, limit), NoDelay(log.clone()), opts)
}

/// Review Focus 3: no card is `NoCard` within the acquire deadline, not
/// `Io` after the library's own retries (10 000 polls per CMD0).
#[test]
fn no_card_is_no_card() {
    const LIMIT: u32 = 50;
    let log = Rc::new(Log::default());
    let mut s = FatStore::new(card(&log, LIMIT), FixedTime);
    assert_eq!(s.mount(), Err(StoreError::NoCard));
    // Two acquires (the mount retries once), each cut off one transaction
    // past the deadline, plus the read the library does after it.
    assert!(
        log.transactions.get() <= 2 * (LIMIT + 2),
        "{} transactions",
        log.transactions.get()
    );
    let calls = log.calls.borrow();
    assert_eq!(
        calls[..4],
        [
            Call::StartOp,
            Call::SetPhase(BusPhase::Acquire),
            Call::Wake,
            Call::StartOp
        ]
    );
    assert!(!calls.contains(&Call::SetPhase(BusPhase::Data)));
}

#[test]
fn classify_table() {
    let log = Rc::new(Log::default());
    let sd = card(&log, u32::MAX);
    let with = |phase, timed_out, e: SdCardError| {
        sd.spi(|b| (b.phase, b.timed_out) = (phase, timed_out));
        sd.classify(&e)
    };
    use BusPhase::{Acquire, Data};
    assert_eq!(
        with(Data, false, SdCardError::CardNotFound),
        StoreError::NoCard
    );
    assert_eq!(
        with(Acquire, true, SdCardError::Transport),
        StoreError::NoCard
    );
    assert_eq!(
        with(Data, true, SdCardError::Transport),
        StoreError::Timeout
    );
    assert_eq!(
        with(Data, true, SdCardError::TimeoutReadBuffer),
        StoreError::Timeout
    );
    assert_eq!(with(Acquire, false, SdCardError::Transport), StoreError::Io);
    assert_eq!(with(Data, false, SdCardError::Transport), StoreError::Io);
    assert_eq!(
        with(Data, false, SdCardError::CrcError(1, 2)),
        StoreError::Io
    );
    assert_eq!(
        with(Data, true, SdCardError::CrcError(1, 2)),
        StoreError::Io
    );
}

#[test]
fn fault_is_the_deadline() {
    let log = Rc::new(Log::default());
    let sd = card(&log, u32::MAX);
    assert_eq!(sd.fault(), None);
    sd.spi(|b| b.timed_out = true);
    assert_eq!(sd.fault(), Some(StoreError::NoCard));
    sd.spi(|b| b.phase = BusPhase::Data);
    assert_eq!(sd.fault(), Some(StoreError::Timeout));
}

#[test]
fn reinit_wakes_at_init_clock() {
    let log = Rc::new(Log::default());
    let sd = card(&log, u32::MAX);
    sd.reinit();
    assert_eq!(
        *log.calls.borrow(),
        [Call::SetPhase(BusPhase::Acquire), Call::Wake]
    );
    log.calls.borrow_mut().clear();
    sd.mounted();
    assert_eq!(*log.calls.borrow(), [Call::SetPhase(BusPhase::Data)]);
}
