//! The card's DMA read path (spec § 4.2): SD multi-block reads over SPI2,
//! DMA2 stream 0 receiving and stream 1 sending 0xFF, run by one state
//! machine in `DMA2_STR0`. It fills the voices' stream rings and, in bench
//! builds, the console's timed reads. Every decision is host-tested
//! (`sd_proto`, `h7_spi`, `serve`, `ring`, `card_bus`); this shell runs
//! them on the registers.
//!
//! A transfer holds the bus as `Stream` from its take to its give. Only a
//! `Held`, made by `Held::take` and spent by `give`, reaches SPI2 and CS,
//! and only a `Held`'s `Staging` reads the DMA's buffer, so no view of it
//! outlives a burst's start.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::{MaybeUninit, offset_of, replace};
use core::ptr::{self, addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU32, Ordering, compiler_fence};

use chimera_core::stream::layout::{Piece, RAW_CHUNK_BLOCKS, RAW_CHUNK_BYTES, deframe};
use chimera_core::stream::ring::{CardSide, Claim};
use chimera_core::stream::serve::{
    Attempt, Blame, ErrorStep, HeldRead, Parked, ReadError, Rot, ServeStep, Strikes, read_failed,
    readable, serve_step,
};
use chimera_core::stream::units::AudioBlocks;
use chimera_fat::h7_spi::{
    self, ABORT, BURST_END, BurstLen, Cr1, DmaEvent, GIVE, Gate, Mbr, Op, POLLED, RegFault, TAKE,
    burst, gate,
};
use chimera_fat::sd::NCR_BYTES;
use chimera_fat::sd_addr::CmdArg;
use chimera_fat::sd_proto::{
    BLOCK_BYTES, CMD12, CMD18, CmdIndex, ProtoError, Reading, SCAN_BYTES, STOP_BYTES, STREAM_BLOCK,
    TOKEN_TIMEOUT_MS, Walk, check_r1, command, stop_response,
};
use chimera_hal::Ms;
use chimera_hal::card_bus::{CardFault, Owner, Quiet, Took, WATCHDOG_MS, Wake};
use chimera_hal::units::{BlockCount, Bytes, BytesPerSec, Lba, Micros};
use cortex_m::asm::dmb;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use crate::controls::now_ms;
use crate::priority::{self, Priority};
use crate::sd::{BUS, CARD_ADDR, SD_FAST_HZ};
use crate::shared::{
    AUDIO_CYCLES, BLOCKS, Cycles, MIDI_CYCLES, STREAM_STATS, Stamp, dwt, registry, rings, stamp,
};

/// The bound on any register poll.
pub const REG_WAIT_US: Micros = Micros::new(50);
/// The polled wait for a card not busy, before CMD18 and after CMD12:
/// about 20 µs at SCK.
const READY_BYTES: u32 = 64;

// Each burst renews the watchdog (`CardBus::renew`), so it bounds one
// burst, not a read whose every block may wait out a token (M2, #761).
const _: () = assert!(WATCHDOG_MS.get() >= 2 * TOKEN_TIMEOUT_MS.get());

// DMAMUX1's channels 8 and 9 are DMA2's streams 0 and 1 (DMA1's are 0-7);
// SPI2's requests (RM0433, the PAC's `DMAREQ_ID_A`).
const RX_MUX: usize = 8;
const TX_MUX: usize = 9;
const SPI2_RX_DMA: u8 = 39;
const SPI2_TX_DMA: u8 = 40;
const RX: usize = 0;
const TX: usize = 1;

/// A raw chunk's blocks as the card clocks them, tokens and CRCs too: the
/// most a fill's bursts land past its start (`Reading::need`, I4).
const BLOCKS_LEN: usize = RAW_CHUNK_BLOCKS as usize * STREAM_BLOCK;

/// The busy drain's area, then the blocks, packed in place as they land:
/// the IRQ de-frames before it starts the next transfer, so one buffer is
/// enough.
#[repr(C, align(32))]
struct DmaBuf {
    scan: [u8; SCAN_BYTES],
    blocks: [u8; BLOCKS_LEN],
}

pub const STAGING_BYTES: usize = size_of::<DmaBuf>();
const SCAN_AT: usize = offset_of!(DmaBuf, scan);
const BLOCKS_AT: usize = offset_of!(DmaBuf, blocks);
const _: () = assert!(SCAN_AT == 0 && BLOCKS_AT == SCAN_BYTES);
// A raw chunk's packed data fits; no burst passes the end (I4).
const _: () = assert!(RAW_CHUNK_BYTES == RAW_CHUNK_BLOCKS as usize * BLOCK_BYTES);
const _: () = assert!(BLOCKS_LEN <= u16::MAX as usize);

// NOLOAD in the non-cacheable DMA region: nothing in it is read before the
// DMA or the CPU writes it. Reached only by address (`staged`) while a
// stream may run; a view of it only through `Staging`.
#[unsafe(link_section = ".ram_d2.dma")]
static mut DMA_BUF: MaybeUninit<DmaBuf> = MaybeUninit::uninit();

/// TX's source, in AXI flash, which DMA2 reaches.
static FF: u8 = 0xFF;

fn staged(at: usize) -> *mut u8 {
    addr_of_mut!(DMA_BUF).cast::<u8>().wrapping_add(at)
}

/// The card IRQ's counters, for `stats`.
pub struct CardStats {
    pub aborts: AtomicU32,
    pub stop_errors: AtomicU32,
    /// The longest entry of `DMA2_STR0`, in µs, audio preemption and all.
    pub irq_us_max: AtomicU32,
    /// The longest entry less the audio's and MIDI DIN's cycles inside it.
    pub irq_own_us_max: AtomicU32,
    /// Card faults set: a read failed past its retry (`serve::on_error`),
    /// a busy that never drained, a stop never sent; and an `init` that
    /// found no SPI divider, or a keep that lost the bus (never, both).
    pub faults: AtomicU32,
    /// Entries of `DMA2_STR0`, pends too.
    pub entries: AtomicU32,
    /// Reads that ended well, and their bursts: a TC entry each.
    pub reads: AtomicU32,
    pub bursts: AtomicU32,
    /// The most bursts one read took.
    pub bursts_max: AtomicU32,
}

pub static STATS: CardStats = CardStats {
    aborts: AtomicU32::new(0),
    stop_errors: AtomicU32::new(0),
    irq_us_max: AtomicU32::new(0),
    irq_own_us_max: AtomicU32::new(0),
    faults: AtomicU32::new(0),
    entries: AtomicU32::new(0),
    reads: AtomicU32::new(0),
    bursts: AtomicU32::new(0),
    bursts_max: AtomicU32::new(0),
};

/// The card IRQ's cycles folded into 64 bits: the UI loop's alone.
#[cfg(feature = "usb-console")]
static IRQ_TOTAL: cortex_m::interrupt::Mutex<
    core::cell::Cell<chimera_core::stream::stats::IrqCycles>,
> = cortex_m::interrupt::Mutex::new(core::cell::Cell::new(
    chimera_core::stream::stats::IrqCycles::new(0),
));

/// The card IRQ's own cycles since boot, its u32 folded in: called every
/// UI pass (`Usb::service`), far inside its 8.9 s wrap.
#[cfg(feature = "usb-console")]
pub fn irq_cycles() -> u64 {
    cortex_m::interrupt::free(|cs| {
        let c = IRQ_TOTAL.borrow(cs);
        let mut f = c.get();
        let t = f.fold(STREAM_STATS.irq_cycles.load(Ordering::Relaxed));
        c.set(f);
        t
    })
}

/// The card IRQ's own cycles and the time when `stats` last read them.
#[cfg(feature = "usb-console")]
#[derive(Clone, Copy)]
pub struct IrqMark {
    cycles: u64,
    at: Ms,
}

#[cfg(feature = "usb-console")]
impl IrqMark {
    pub fn new(now: Ms) -> IrqMark {
        IrqMark {
            cycles: irq_cycles(),
            at: now,
        }
    }
}

/// `stats`' stream lines at `now`: the IRQ's share since `mark`, which
/// moves to now.
#[cfg(feature = "usb-console")]
pub fn tally(
    mark: &mut IrqMark,
    now: Ms,
    cycles_per_us: u32,
) -> chimera_core::console::StreamTally {
    use chimera_core::console::{PerMille, StreamTally};
    use chimera_hal::units::Millis;
    let ld = |a: &AtomicU32| a.load(Ordering::Relaxed);
    let s = &STREAM_STATS;
    let then = replace(mark, IrqMark::new(now));
    let own = mark.cycles.saturating_sub(then.cycles);
    let wall = u64::from(now.since(then.at)) * 1_000 * u64::from(cycles_per_us);
    StreamTally {
        underruns: ld(&s.underruns),
        cold: ld(&s.cold),
        unplanned: ld(&s.unplanned),
        card_faults: ld(&STATS.faults),
        frame_errors: ld(&s.frame_errors),
        unreadable: ld(&s.unreadable),
        held_at_cap: ld(&s.held_at_cap),
        late_refused: ld(&s.late_refused),
        quiet_ms_max: Millis::new(ld(&s.quiet_ms_max)),
        quiets: ld(&s.quiets),
        irq_us_max: Micros::new(ld(&STATS.irq_us_max)),
        irq_own_us_max: Micros::new(ld(&STATS.irq_own_us_max)),
        irq_share: PerMille::of(own, wall),
    }
}

/// Wakes the card IRQ.
pub fn pend() {
    NVIC::pend(pac::Interrupt::DMA2_STR0);
}

/// `fault card`'s fills still to fail a CRC.
#[cfg(feature = "fault-inject")]
static FAIL: AtomicU32 = AtomicU32::new(0);

/// `fault card`: the next `n` fills' transfers fail a CRC at their first
/// burst; two are the retry, then the read fault.
#[cfg(feature = "fault-inject")]
pub fn fail_next(n: u32) {
    FAIL.store(n, Ordering::Release);
}

/// The wants `serve_step` found it can't read: no wake for them.
static PARKED: Parked = Parked::new();

/// A Wanted slot this IRQ can read: its ring names a registry row, and
/// its chunk isn't parked (`serve::readable`).
fn wanted() -> bool {
    filling()
        && rings()
            .zip(registry())
            .is_some_and(|(r, reg)| readable(r, reg, &PARKED).next().is_some())
}

/// No write's quiet: under one, the fill in flight finishes and no new one
/// starts (decision 8).
fn filling() -> bool {
    BUS.quiet() == Quiet::Off
}

/// The UI loop's, each pass (`watch_in`): bank 2's with `bank2`, nothing
/// closed, where no stream plays.
pub fn watch(now: Ms) {
    // Closed with a stream still on the bus (a gate build's erase whose
    // wait ran out): the interrupt's `closed` grounds it and gives the bus
    // back, so the UI's card isn't lost till a reboot.
    #[cfg(feature = "bank2")]
    if crate::bank2::board_door(|d| (d.card.watch)(now)).is_none() && BUS.owner() == Owner::Stream {
        pend();
    }
    #[cfg(not(feature = "bank2"))]
    watch_in(now);
}

/// A transfer stuck past `WATCHDOG_MS` (a TE, a MODF, a lost TC) is asked
/// to abort. A want left with the bus free (a failed fill requeued, or
/// one whose look found the bus the UI's) is woken here; a parked one,
/// whose chunk can't be read, is not.
fn watch_in(now: Ms) {
    if !unstick(now) && BUS.owner() == Owner::Free && serving() && wanted() {
        pend();
    }
}

/// What bank 1 may call of the card interrupt's work (ADR 0175): with
/// `bank2`, `IrqState`'s steps, the reads, the stops and the de-framing
/// are bank 2's (`bank2.x`'s `card_dma` globs). Bank 1 keeps `init`, the
/// handler's shim (its clocks and counters, the flags, `closed`) and the
/// ground and give a closed door still needs.
#[cfg(feature = "bank2")]
#[repr(C)]
pub(crate) struct CardDoor {
    step: fn(&mut IrqState, DmaEvent, Abort),
    watch: fn(Ms),
}

#[cfg(feature = "bank2")]
impl CardDoor {
    pub(crate) const DOOR: CardDoor = CardDoor {
        step: IrqState::step,
        watch: watch_in,
    };
}

/// A stuck transfer asked to abort; true if one was.
fn unstick(now: Ms) -> bool {
    let stuck = BUS.stuck(now);
    if stuck {
        BUS.ask_abort();
        pend();
    }
    stuck
}

/// A card is mounted and not faulted.
fn serving() -> bool {
    CARD_ADDR.get().is_some() && BUS.fault() == CardFault::Clear
}

/// Why a transfer stopped early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    Proto(ProtoError),
    Reg(RegFault),
    /// A poll passed its bound.
    Bound,
    /// The card still busy before CMD18 (`READY_BYTES`).
    Busy,
    /// The watchdog asked.
    Asked,
    /// A DMA transfer error.
    Dma,
    Unmounted,
    /// No command argument for the block (an SDSC card past 4 GiB).
    Range,
}

impl Why {
    /// A failure the protocol doesn't name, which `Rot` can't blame on a
    /// chunk: counted toward `Strikes`. No card, or a block past an SDSC
    /// card's reach, is not the card's.
    fn strikes(self) -> bool {
        match self {
            Why::Reg(_) | Why::Bound | Why::Busy | Why::Asked | Why::Dma => true,
            Why::Proto(_) | Why::Unmounted | Why::Range => false,
        }
    }
}

/// What the IRQ runs on: the SPI clock it set and its bounds.
struct Line {
    mbr: Mbr,
    /// Bytes a second at the real SCK (M5).
    clock: BytesPerSec,
    /// `REG_WAIT_US` in cycles.
    reg_wait: Cycles,
    /// `TOKEN_TIMEOUT_MS` of bytes at SCK: the busy drain's bound.
    drain_bytes: Bytes,
    cycles_per_us: Cycles,
}

impl Line {
    fn us(&self, cycles: Cycles) -> Micros {
        Micros::new(cycles.get() / self.cycles_per_us.get())
    }
}

/// A clock rate in hertz.
#[derive(Clone, Copy)]
pub struct Hz(u32);

impl Hz {
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Polls `ready` for at most `cycles` cycles and `cycles` tries, so a
/// cycle counter that doesn't count still ends it.
fn wait(cycles: Cycles, mut ready: impl FnMut() -> bool) -> bool {
    let t0 = dwt();
    for _ in 0..cycles.get() {
        if ready() {
            return true;
        }
        if dwt().since(t0) > cycles {
            break;
        }
    }
    ready()
}

fn dma2() -> &'static pac::dma1::RegisterBlock {
    // SAFETY: DMA2 is this file's alone (the audio's streams are DMA1's),
    // touched only by `init`, before `DMA2_STR0` is unmasked, and by that
    // handler, which doesn't nest with itself. The block is always mapped.
    unsafe { &*pac::DMA2::ptr() }
}

/// LISR's flags of streams 0 and 1 (LIFCR's bits sit where they do).
const STREAMS_0_1: u32 = 0x0F7D;

/// LISR's flags for streams 0 and 1, read, then cleared: only those read,
/// so a TC landing between the two is seen next entry. FEIF is ignored:
/// spurious in direct mode.
fn events() -> DmaEvent {
    let d = dma2();
    let isr = d.lisr.read();
    let ev = DmaEvent::of(
        isr.tcif0().is_complete(),
        isr.teif0().is_error() || isr.teif1().is_error(),
    );
    // SAFETY: LIFCR's clear bits for flags LISR showed set.
    d.lifcr
        .write(|w| unsafe { w.bits(isr.bits() & STREAMS_0_1) });
    ev
}

fn clear_streams(d: &pac::dma1::RegisterBlock) {
    d.lifcr.write(|w| {
        w.ctcif0()
            .clear()
            .chtif0()
            .clear()
            .cteif0()
            .clear()
            .cdmeif0()
            .clear()
            .cfeif0()
            .clear();
        w.ctcif1()
            .clear()
            .chtif1()
            .clear()
            .cteif1()
            .clear()
            .cdmeif1()
            .clear()
            .cfeif1()
            .clear()
    });
}

/// The bus, held as `Stream` by this handler, both streams off. Made only
/// by `take`, spent only by `give`: the one way to SPI2 and CS.
struct Held {
    /// The HAL's CFG1, put back at the give.
    cfg1: u32,
}

/// TAKE's suspend and SPE off, without its save: a kept bus's CFG1 is
/// already this handler's, and SPE goes off before TSIZE is written.
const REARM: [Op; 3] = [Op::Suspend, Op::ClearFlags, Op::Cr1(Cr1::OFF)];

impl Held {
    /// The card IRQ's take; `None` if the bus is wanted (the give pends)
    /// or the HAL's transaction would not suspend (the bus given back).
    fn take(line: &Line) -> Option<Held> {
        if BUS.take_or_want(now_ms()) == Took::Wanted {
            return None;
        }
        let mut h = Held { cfg1: 0 };
        match h.run(&TAKE, line, 0) {
            Ok(()) => Some(h),
            Err(_) => {
                give(h, line);
                None
            }
        }
    }

    /// Holds on for the next transfer, the watchdog's stamp renewed. No
    /// other taker runs between the give and the take: the UI is thread
    /// mode, the audio never takes and this handler doesn't nest. `None`
    /// if the take failed all the same: the bus isn't this handler's, so
    /// no op touches SPI2 again, and a fault is counted.
    fn keep(self) -> Option<Held> {
        let _ = BUS.give();
        match BUS.take_or_want(now_ms()) {
            Took::Taken => Some(self),
            Took::Wanted => {
                STATS.faults.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    fn spi(&self) -> &'static pac::spi1::RegisterBlock {
        // SAFETY: a `Held` exists only while `BUS.owner() == Owner::Stream`,
        // taken by this handler (`take`), so no other code touches SPI2's
        // registers; the UI takes the bus before any SPI2 access.
        unsafe { &*pac::SPI2::ptr() }
    }

    fn gpioe(&self) -> &'static pac::gpioa::RegisterBlock {
        // SAFETY: PE12 is the card's CS, driven by the UI's `SdSpi` only
        // while it holds the bus; a `Held` means this handler does (as
        // `spi`). BSRR's writes are atomic, and touch only PE12.
        unsafe { &*pac::GPIOE::ptr() }
    }

    /// CS low.
    fn select(&self) {
        self.gpioe().bsrr.write(|w| w.br12().set_bit());
    }

    /// CS high.
    fn deselect(&self) {
        self.gpioe().bsrr.write(|w| w.bs12().set_bit());
    }

    /// One `Op`, one register access (or one bounded wait).
    fn op(&mut self, op: Op, line: &Line, dest: u32) -> Result<(), RegFault> {
        let spi = self.spi();
        let d = dma2();
        let bounded = |ok: bool| {
            if ok {
                Ok(())
            } else {
                Err(RegFault::Timeout(op))
            }
        };
        match op {
            Op::Suspend => {
                if spi.cr1.read().cstart().is_started() {
                    spi.cr1.modify(|_, w| w.csusp().requested());
                    return bounded(wait(line.reg_wait, || spi.sr.read().susp().is_suspended()));
                }
            }
            Op::WaitEot => {
                return bounded(wait(line.reg_wait, || spi.sr.read().eot().is_completed()));
            }
            Op::CheckErrors => {
                let sr = spi.sr.read();
                if sr.ovr().is_overrun() {
                    return Err(RegFault::Ovr);
                }
                if sr.modf().is_fault() {
                    return Err(RegFault::Modf);
                }
            }
            Op::ClearFlags => spi.ifcr.write(|w| {
                w.eotc()
                    .clear()
                    .txtfc()
                    .clear()
                    .suspc()
                    .clear()
                    .ovrc()
                    .clear();
                w.modfc().clear().udrc().clear().tifrec().clear()
            }),
            // SAFETY: `Cr1` holds only ON and OFF, both with SSI (I3).
            Op::Cr1(c) => spi.cr1.write(|w| unsafe { w.bits(c.bits()) }),
            Op::Start => spi.cr1.modify(|_, w| w.cstart().started()),
            Op::AfcntrOn => spi.cfg2.modify(|_, w| w.afcntr().controlled()),
            Op::MasterOn => spi.cfg2.modify(|_, w| w.master().master()),
            Op::SaveCfg1 => self.cfg1 = spi.cfg1.read().bits(),
            Op::SetMbr => spi.cfg1.modify(|_, w| w.mbr().bits(line.mbr.bits())),
            // SAFETY: the value `SaveCfg1` read from this register: TAKE's
            // first op, before any that can fail (`h7_spi::TAKE`).
            Op::RestoreCfg1 => spi.cfg1.write(|w| unsafe { w.bits(self.cfg1) }),
            Op::Tsize(n) => {
                let n = n.map_or(0, BurstLen::as_u16);
                spi.cr2.modify(|_, w| w.tsize().bits(n));
            }
            Op::RxDma(on) => spi.cfg1.modify(|_, w| w.rxdmaen().bit(on)),
            Op::TxDma(on) => spi.cfg1.modify(|_, w| w.txdmaen().bit(on)),
            Op::ClearStreams => clear_streams(d),
            Op::StreamsOn(n) => {
                // Every view of the staging is gone (it borrows the `Held`
                // this burst spends) and the CPU's writes to it are done.
                dmb();
                compiler_fence(Ordering::Release);
                let n = n.as_u16();
                let (rx, tx) = (&d.st[RX], &d.st[TX]);
                // A stream still on ignores NDTR and M0AR: it would run
                // this burst on the last one's address.
                if rx.cr.read().en().is_enabled() || tx.cr.read().en().is_enabled() {
                    return Err(RegFault::StreamOn);
                }
                rx.ndtr.write(|w| w.ndt().bits(n));
                // SAFETY: `dest` is inside `DMA_BUF` with `n` bytes after it
                // (`Dest::of`), in D2 SRAM, which DMA2 reaches; or, armed by
                // a bench's `inject te` alone, DTCM, which it can't: a
                // transfer error that writes nothing (`bench::aim`).
                rx.m0ar.write(|w| unsafe { w.m0a().bits(dest) });
                rx.cr.modify(|_, w| w.en().enabled());
                tx.ndtr.write(|w| w.ndt().bits(n));
                // SAFETY: `FF`, a byte in AXI flash; TX's memory side
                // doesn't increment.
                tx.m0ar
                    .write(|w| unsafe { w.m0a().bits(addr_of!(FF) as u32) });
                tx.cr.modify(|_, w| w.en().enabled());
            }
            Op::StreamsOff => {
                let (rx, tx) = (&d.st[RX], &d.st[TX]);
                rx.cr.modify(|_, w| w.en().disabled());
                tx.cr.modify(|_, w| w.en().disabled());
                return bounded(wait(line.reg_wait, || {
                    rx.cr.read().en().is_disabled() && tx.cr.read().en().is_disabled()
                }));
            }
        }
        Ok(())
    }

    fn run(&mut self, ops: &[Op], line: &Line, dest: u32) -> Result<(), RegFault> {
        h7_spi::run(ops, |o| self.op(o, line, dest))
    }

    /// SPE off, then the endless polled transaction.
    fn polled(&mut self, line: &Line) -> Result<(), Why> {
        self.run(&REARM, line, 0)
            .and_then(|()| self.run(&POLLED, line, 0))
            .map_err(Why::Reg)
    }

    /// One byte each way, each side bounded.
    fn xfer(&self, b: u8, line: &Line) -> Result<u8, Why> {
        let spi = self.spi();
        if !wait(line.reg_wait, || spi.sr.read().txp().is_not_full()) {
            return Err(Why::Bound);
        }
        // SAFETY: TXDR, written a byte at a time as the HAL does (8-bit
        // frames), through its cell.
        unsafe { ptr::write_volatile(UnsafeCell::raw_get(addr_of!(spi.txdr).cast()), b) };
        if !wait(line.reg_wait, || spi.sr.read().rxp().is_not_empty()) {
            return Err(Why::Bound);
        }
        // SAFETY: RXDR, read a byte at a time as the HAL does.
        Ok(unsafe { ptr::read_volatile(addr_of!(spi.rxdr).cast::<u8>()) })
    }

    /// A command, then R1 within NCR.
    fn command(&self, idx: CmdIndex, arg: CmdArg, line: &Line) -> Result<(), Why> {
        for b in command(idx, arg) {
            self.xfer(b, line)?;
        }
        for _ in 0..NCR_BYTES {
            let r1 = self.xfer(0xFF, line)?;
            if r1 & 0x80 == 0 {
                return check_r1(r1).map_err(Why::Proto);
            }
        }
        Err(Why::Proto(ProtoError::Timeout))
    }

    /// DO reads 0xFF within `READY_BYTES`: a card still busy is left to
    /// a later pick, never waited out here.
    fn ready(&self, line: &Line) -> Result<(), Why> {
        for _ in 0..READY_BYTES {
            if self.xfer(0xFF, line)? == 0xFF {
                return Ok(());
            }
        }
        Err(Why::Busy)
    }

    /// CMD12 and its answer; the busy after it drains by DMA. The inner
    /// `Err` is a stop error: the blocks before it stand (I8).
    fn stop(&mut self, line: &Line) -> Result<Result<(), ProtoError>, Why> {
        self.polled(line)?;
        for b in command(CMD12, CmdArg::ZERO) {
            self.xfer(b, line)?;
        }
        let mut r = [0xFF; STOP_BYTES];
        for b in &mut r {
            *b = self.xfer(0xFF, line)?;
        }
        Ok(stop_response(&r))
    }

    /// CS high, then one 0xFF so the card lets DO go.
    fn end(&mut self, line: &Line) {
        self.deselect();
        if self.polled(line).is_ok() {
            let _ = self.xfer(0xFF, line);
        }
    }

    /// The staging, read while both streams are off.
    fn staging(&mut self) -> Staging<'_> {
        // The DMA's writes are done before its TC; the flag was read from
        // the device, then come these normal loads.
        dmb();
        compiler_fence(Ordering::Acquire);
        Staging(PhantomData)
    }

    /// A burst into `dest`.
    fn fly(mut self, dest: Dest, line: &Line) -> Result<Flying, (Flying, Why)> {
        match self.run(&burst(dest.n), line, dest.at) {
            Ok(()) => Ok(Flying(self)),
            Err(e) => Err((Flying(self), Why::Reg(e))),
        }
    }
}

/// A burst in flight: the DMA may write the staging.
struct Flying(Held);

impl Flying {
    /// The burst's end: the streams are off.
    fn land(self, line: &Line) -> Result<Held, (Flying, Why)> {
        let Flying(mut h) = self;
        match h.run(&BURST_END, line, 0) {
            Ok(()) => Ok(h),
            Err(e) => Err((Flying(h), Why::Reg(e))),
        }
    }

    /// Both streams stopped, SPE off, every flag cleared, whatever the
    /// burst had done.
    fn ground(self, line: &Line) -> Held {
        let Flying(mut h) = self;
        let _ = h7_spi::run_all(&ABORT, |o| h.op(o, line, 0));
        h
    }
}

/// GIVE, then the bus; a give that finds it wanted pends. Every op runs
/// past a failed suspend, so CFG1 (MBR, the DMA enables) is always the
/// HAL's again.
fn give(mut h: Held, line: &Line) {
    let _ = h7_spi::run_all(&GIVE, |o| h.op(o, line, 0));
    if BUS.give() == Wake::Pend {
        pend();
    }
}

/// The DMA buffer while both streams are off. Each view covers only bytes
/// the transfer under way wrote, and is formed right after the burst or
/// the read that wrote them.
struct Staging<'h>(PhantomData<&'h mut Held>);

impl Staging<'_> {
    /// The read's bytes up to `upto`, a burst just landed at their end:
    /// the data its bursts packed before it, then the burst.
    fn landed(&mut self, upto: usize) -> Option<&mut [u8]> {
        (upto <= BLOCKS_LEN).then(|| {
            // SAFETY: inside `blocks` (checked above); the streams are off
            // and this borrows the `Held` mutably, so nothing else touches
            // the buffer; this job's bursts (a fill's earlier pieces too)
            // wrote every byte up to `upto` (`advance`'s one use).
            unsafe { core::slice::from_raw_parts_mut(staged(BLOCKS_AT), upto) }
        })
    }

    /// The data of a fill's `blocks`, packed whole by its landings.
    fn chunk(&self, blocks: BlockCount) -> Option<&[u8]> {
        let n = usize::try_from(blocks.bytes().get()).ok()?;
        (n <= BLOCKS_LEN).then(|| {
            // SAFETY: inside `blocks`; every block of the read landed its
            // 512 B there, so all `n` bytes are written; streams off.
            unsafe { core::slice::from_raw_parts(staged(BLOCKS_AT), n) }
        })
    }
}

const SCAN_LEN: BurstLen = match BurstLen::new(SCAN_BYTES) {
    Some(n) => n,
    None => panic!(),
};

/// A burst's target in the staging.
struct Dest {
    at: u32,
    n: BurstLen,
}

impl Dest {
    /// A busy drain's.
    fn scan() -> Dest {
        Dest {
            at: staged(SCAN_AT) as u32,
            n: SCAN_LEN,
        }
    }

    /// The next burst of a read whose data ends at `end`: what it still
    /// needs, landed after the data; `None` past the staging (I4).
    fn of(r: &Reading, kind: &Kind, end: usize) -> Option<Dest> {
        let from = kind.landing(end);
        let need = usize::try_from(r.need().get()).ok()?;
        let n = match kind {
            Kind::Fill(_) => need,
            // A bench read longer than the staging lands it over and over.
            #[cfg(feature = "bench")]
            Kind::Bench => need.min(BLOCKS_LEN),
        };
        // `Reading::need` and `ChunkRead`'s pieces keep a fill inside; this
        // keeps the DMA so.
        (from + n <= BLOCKS_LEN).then_some(())?;
        Some(Dest {
            at: staged(BLOCKS_AT + from) as u32,
            n: BurstLen::new(n)?,
        })
    }
}

/// Whether the handler's entry found an abort asked (`CardBus::ask_abort`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Abort {
    Asked,
    Unasked,
}

/// A slot's fill: one job, however many CMD18s its chunk takes.
struct Fill {
    /// The one `Claim` (Task 5) of the slot.
    claim: Claim,
    /// Its file's registry row, held until the fill drops (#597), and
    /// the chunk's read on it.
    row: HeldRead<'static>,
    /// The piece under way.
    piece: Piece,
    /// Its piece's first read, or the retry (`serve::on_error`).
    attempt: Attempt,
}

/// What a transfer is for.
enum Kind {
    Fill(Fill),
    #[cfg(feature = "bench")]
    Bench,
}

impl Kind {
    /// Where a burst lands, the data so far ending at `end`: a fill's after
    /// it, so a chunk's blocks lie whole for `deframe`; a bench read's,
    /// only CRC-checked, at the start each time.
    fn landing(&self, end: usize) -> usize {
        match self {
            Kind::Fill(_) => end,
            #[cfg(feature = "bench")]
            Kind::Bench => 0,
        }
    }

    /// Where a read's data starts: a piece's after the pieces before it.
    fn start(&self) -> usize {
        match self {
            Kind::Fill(f) => f.piece.at as usize * BLOCK_BYTES,
            #[cfg(feature = "bench")]
            Kind::Bench => 0,
        }
    }
}

/// The bench's answer still owed when the bus is given; nothing outside
/// bench builds.
#[derive(Default)]
struct Owed(#[cfg(feature = "bench")] Option<ReadFail>);

/// A read's end, after its busy drained.
enum Then {
    /// It ended well: the next pick.
    Next {
        done: Done,
        /// Cycles at CMD18.
        t0: Cycles,
    },
    /// An abort: the bus goes back, then the answer.
    Give(Owed),
    /// A fill's piece read: the next piece's CMD18, on the bus held.
    More(Fill),
    /// A piece's transfer failed once: the bus back, then the piece again
    /// on the next entry.
    Retry(Fill),
}

/// What a good read leaves to answer.
enum Done {
    /// The chunk is already in its slot.
    Filled,
    #[cfg(feature = "bench")]
    Bench,
}

enum Phase {
    /// A CMD18's bursts.
    Read {
        walk: Reading,
        /// Cycles at CMD18.
        t0: Cycles,
        kind: Kind,
        /// Where the read's packed data ends, and where the burst's bytes.
        end: usize,
        upto: usize,
    },
    /// After CMD12: a 0xFF burst into `scan` while the card is busy, at
    /// most `left` bytes more.
    Drain { left: u32, then: Then },
}

/// A burst under way.
struct Transfer {
    bus: Flying,
    phase: Phase,
}

enum Job {
    Idle,
    Read(Transfer),
}

struct IrqState {
    side: CardSide<'static>,
    job: Job,
    line: Line,
    /// Bursts of the read under way.
    bursts: u32,
    /// A fill to read again (`Then::Retry`): first at the next look.
    retry: Option<Fill>,
    /// The chunks that didn't de-frame: a card's once, a file's after.
    rot: Rot,
    /// Fills failed in a row for a reason the protocol doesn't name
    /// (`Why::strikes`).
    strikes: Strikes,
}

/// A cell only `DMA2_STR0` opens.
struct IrqOnly<T>(UnsafeCell<MaybeUninit<T>>);

// SAFETY: the value is reached only by `set` and `get`, whose contracts
// keep one access at a time: `set` once by `init` before `DMA2_STR0` is
// unmasked, then `get` at that handler's top, which doesn't nest with
// itself.
unsafe impl<T: Send> Sync for IrqOnly<T> {}

impl<T> IrqOnly<T> {
    const fn new() -> Self {
        Self(UnsafeCell::new(MaybeUninit::uninit()))
    }

    /// # Safety
    /// Once, by `init`, before `DMA2_STR0` is unmasked.
    unsafe fn set(&self, v: T) {
        // SAFETY: per the contract, nothing else reaches the cell yet.
        unsafe { (*self.0.get()).write(v) };
    }

    /// # Safety
    /// Only at the top of `DMA2_STR0`, its one call site, after `set`; the
    /// reference is dropped before the handler returns.
    #[allow(clippy::mut_from_ref)]
    unsafe fn get(&self) -> &mut T {
        // SAFETY: set before the handler could run, and the handler doesn't
        // nest, so this is the one reference.
        unsafe { (*self.0.get()).assume_init_mut() }
    }
}

static IRQ: IrqOnly<IrqState> = IrqOnly::new();

/// DMA2's streams for SPI2, then the IRQ's state, then `DMA2_STR0` at
/// `Priority::CARD`, unmasked last. `spi_ker` is SPI2's kernel clock
/// (`Spi::<SPI2, _>::kernel_clk_unwrap`). False, the IRQ left masked and
/// counted a fault, if no divider brings that clock to `SD_FAST_HZ`: then
/// nothing streams (the boot lends the registry no rows).
#[must_use]
pub fn init(nvic: &mut NVIC, side: CardSide<'static>, spi_ker: Hz, cpu: Hz) -> bool {
    let Some(mbr) = Mbr::fastest(spi_ker.get(), SD_FAST_HZ) else {
        STATS.faults.fetch_add(1, Ordering::Relaxed);
        return false;
    };
    // SAFETY: before `DMA2_STR0` is unmasked, in thread mode, as the audio's
    // DMA1 enable is; DMAMUX1's channels 8 and 9 are this file's alone.
    let (rcc, mux) = unsafe { (&*pac::RCC::ptr(), &*pac::DMAMUX1::ptr()) };
    rcc.ahb1enr.modify(|_, w| w.dma2en().set_bit());
    let _ = rcc.ahb1enr.read();
    // SAFETY: 39 and 40 are SPI2's RX and TX requests in RM0433's DMAMUX1
    // table; channels 8 and 9 are DMA2's streams 0 and 1, this file's alone.
    unsafe {
        mux.ccr[RX_MUX].modify(|_, w| w.dmareq_id().bits(SPI2_RX_DMA));
        mux.ccr[TX_MUX].modify(|_, w| w.dmareq_id().bits(SPI2_TX_DMA));
    }
    let d = dma2();
    // SAFETY: SPI2's block is always mapped; only its data registers'
    // addresses are taken.
    let spi = unsafe { &*pac::SPI2::ptr() };
    let (rx, tx) = (&d.st[RX], &d.st[TX]);
    rx.cr.modify(|_, w| w.en().disabled());
    tx.cr.modify(|_, w| w.en().disabled());
    // SAFETY: the SPI's data registers, which the streams move bytes at.
    unsafe {
        rx.par.write(|w| w.pa().bits(addr_of!(spi.rxdr) as u32));
        tx.par.write(|w| w.pa().bits(addr_of!(spi.txdr) as u32));
    }
    rx.cr.write(|w| {
        w.dir()
            .peripheral_to_memory()
            .minc()
            .incremented()
            .pinc()
            .fixed();
        w.psize().bits8().msize().bits8().pl().high();
        w.tcie().enabled().teie().enabled()
    });
    tx.cr.write(|w| {
        w.dir().memory_to_peripheral().minc().fixed().pinc().fixed();
        w.psize().bits8().msize().bits8().pl().high()
    });
    // Direct mode: no FIFO, so FEIF means nothing.
    rx.fcr.write(|w| w.dmdis().enabled());
    tx.fcr.write(|w| w.dmdis().enabled());
    clear_streams(d);
    let cycles_per_us = Cycles::new((cpu.get() / 1_000_000).max(1));
    let clock = BytesPerSec::spi(mbr.sck_hz(spi_ker.get()));
    let line = Line {
        mbr,
        clock,
        reg_wait: Cycles::new(REG_WAIT_US.get() * cycles_per_us.get()),
        drain_bytes: clock.bytes_in(TOKEN_TIMEOUT_MS),
        cycles_per_us,
    };
    let st = IrqState {
        side,
        job: Job::Idle,
        line,
        bursts: 0,
        retry: None,
        rot: Rot::new(),
        strikes: Strikes::new(),
    };
    // SAFETY: once (`side` exists once: `Rings::sides`), before the unmask.
    unsafe { IRQ.set(st) };
    priority::set_irq(nvic, pac::Interrupt::DMA2_STR0, Priority::CARD);
    // SAFETY: the state is set; the handler touches DMA2, SPI2 and CS only
    // as above.
    unsafe { NVIC::unmask(pac::Interrupt::DMA2_STR0) };
    true
}

/// RX's transfer complete or error, or a pend (a want, a give, an abort):
/// one step of the job. A pend mid-transfer is ignored (I6).
#[interrupt]
fn DMA2_STR0() {
    let enter = clocks();
    STATS.entries.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `IRQ`'s one `get`: `init` set it before unmasking this
    // handler, which doesn't nest with itself.
    let st = unsafe { IRQ.get() };
    let ev = events();
    let asked = if BUS.take_abort() {
        Abort::Asked
    } else {
        Abort::Unasked
    };
    // Bank 2's work through the door; closed, no stream plays.
    #[cfg(feature = "bank2")]
    if crate::bank2::board_door(|d| (d.card.step)(st, ev, asked)).is_none() {
        closed(st);
    }
    #[cfg(not(feature = "bank2"))]
    st.step(ev, asked);
    let leave = clocks();
    let wall = leave.at.since(enter.at);
    let own = wall
        .get()
        .saturating_sub(leave.over.since(enter.over).get());
    // The bench's check is no streaming cost.
    #[cfg(feature = "bench")]
    let own = own.saturating_sub(bench::CHECK_CYCLES.swap(0, Ordering::Relaxed));
    let own_us = st.line.us(Cycles::new(own)).get();
    STATS
        .irq_us_max
        .fetch_max(st.line.us(wall).get(), Ordering::Relaxed);
    STATS.irq_own_us_max.fetch_max(own_us, Ordering::Relaxed);
    // CARD IRQ: the streams' own share of the CPU.
    STREAM_STATS.irq_cycles.fetch_add(own, Ordering::Relaxed);
}

/// The cycle counter and the cycles of the interrupts over this one (the
/// audio's and the MIDI level's, DIN's and USB's) as one pair.
#[inline(always)]
fn clocks() -> Stamp {
    stamp(|| {
        AUDIO_CYCLES
            .load(Ordering::Relaxed)
            .wrapping_add(MIDI_CYCLES.load(Ordering::Relaxed))
    })
}

/// An entry with bank 2 closed: nothing is read. Only a gate build's
/// `bank2 erase` closes a live door, once the bus is free of streams
/// (`close_tape`, which asks an abort first, so bank 2's stop sends CMD12
/// while the door is open). A burst still in flight even then (the IRQ
/// wedged past both waits) is grounded, CS high, the bus given back and
/// its claim requeued, with no CMD12 (bank 2's): the UI's next command
/// finds the card mid-read and recovers it as any failed command. A bench
/// read in flight is answered `Fault`. The flags were cleared (`events`).
#[cfg(feature = "bank2")]
#[inline(never)]
fn closed(st: &mut IrqState) {
    if let Some(f) = st.retry.take() {
        st.side.requeue(f.claim);
    }
    let Job::Read(Transfer { bus, phase }) = replace(&mut st.job, Job::Idle) else {
        return;
    };
    let h = bus.ground(&st.line);
    h.deselect();
    give(h, &st.line);
    match phase {
        Phase::Read {
            kind: Kind::Fill(f),
            ..
        }
        | Phase::Drain {
            then: Then::More(f) | Then::Retry(f),
            ..
        } => st.side.requeue(f.claim),
        // The bench's read, read or stopping: its mailbox answered, so a
        // later read isn't `Busy` for good.
        #[cfg(feature = "bench")]
        Phase::Read {
            kind: Kind::Bench, ..
        }
        | Phase::Drain {
            then: Then::Next {
                done: Done::Bench, ..
            },
            ..
        } => MAIL.answer(Err(ReadFail::Fault)),
        Phase::Drain {
            then: Then::Give(owed),
            ..
        } => settle(owed),
        // A fill read whole: in its slot already.
        Phase::Drain {
            then: Then::Next { .. },
            ..
        } => {}
    }
}

/// The work chosen, claimed with the bus held.
enum Work {
    Fill(Fill),
    /// The bench's mailbox read.
    #[cfg(feature = "bench")]
    Bench(Lba, BlockCount),
    Nothing,
}

impl IrqState {
    fn step(&mut self, ev: DmaEvent, asked: Abort) {
        let in_flight = matches!(self.job, Job::Read(_));
        let g = gate(in_flight, ev);
        if g == Gate::Ignore && asked == Abort::Unasked {
            return;
        }
        // Not in flight, a TE or an ask has nothing to stop: look instead.
        let Job::Read(Transfer { bus, phase }) = replace(&mut self.job, Job::Idle) else {
            return self.look();
        };
        let fail = if asked == Abort::Asked {
            Some(Why::Asked)
        } else if g == Gate::Advance {
            None
        } else {
            Some(Why::Dma)
        };
        match (phase, fail) {
            (Phase::Read { kind, .. }, Some(why)) => {
                let h = bus.ground(&self.line);
                self.abort(h, kind, why);
            }
            // The card was already being stopped: no second CMD12.
            (Phase::Drain { then, .. }, Some(_)) => {
                let h = bus.ground(&self.line);
                self.fault(h, then);
            }
            (
                Phase::Read {
                    walk,
                    t0,
                    kind,
                    end,
                    upto,
                },
                None,
            ) => self.advance(bus, walk, t0, kind, end, upto),
            (Phase::Drain { left, then }, None) => self.drained(bus, left, then),
        }
    }

    /// Nothing in flight (I6): with a want or the bench's read waiting,
    /// the bus taken and the work chosen under it.
    fn look(&mut self) {
        if !serving() {
            // A retry the fault or the unmount overtook: its claim back.
            if let Some(f) = self.retry.take() {
                self.side.requeue(f.claim);
            }
            #[cfg(feature = "bench")]
            if MAIL.claim().is_some() {
                MAIL.answer(Err(if CARD_ADDR.get().is_none() {
                    ReadFail::Unmounted
                } else {
                    ReadFail::Fault
                }));
            }
            return;
        }
        #[cfg(feature = "bench")]
        let waits = self.retry.is_some() || wanted() || MAIL.queued();
        #[cfg(not(feature = "bench"))]
        let waits = self.retry.is_some() || wanted();
        if !waits {
            return;
        }
        if let Some(h) = Held::take(&self.line) {
            match self.work() {
                Work::Nothing => give(h, &self.line),
                work => self.start(h, work),
            }
        } else if let Some(f) = self.retry.take() {
            // Requeued, `watch` wakes it; held, its row would stay held.
            self.side.requeue(f.claim);
        }
    }

    /// With the bus held: a retry first, then the earliest want it can
    /// read (`serve_step`), claimed; else the bench's read.
    fn work(&mut self) -> Work {
        match self.retry.take() {
            Some(f) if serving() && filling() => return Work::Fill(f),
            // A fault since, or a write's quiet: its claim back.
            Some(f) => self.side.requeue(f.claim),
            None => {}
        }
        if !serving() {
            return Work::Nothing;
        }
        if let Some(reg) = registry().filter(|_| filling()) {
            let now = AudioBlocks::new(BLOCKS.load(Ordering::Relaxed));
            if let ServeStep::Read { claimed, row } = serve_step(
                &mut self.side,
                reg,
                now,
                &mut self.rot,
                &PARKED,
                &STREAM_STATS,
            ) {
                match row.read().first() {
                    Some(piece) => {
                        return Work::Fill(Fill {
                            claim: claimed,
                            row,
                            piece,
                            attempt: Attempt::First,
                        });
                    }
                    // `serve_step` read it: never.
                    None => self.side.requeue(claimed),
                }
            }
        }
        #[cfg(feature = "bench")]
        if let Some((lba, n)) = MAIL.claim() {
            return Work::Bench(lba, n);
        }
        Work::Nothing
    }

    /// The bus held: the chosen work's CMD18.
    fn start(&mut self, h: Held, work: Work) {
        let (kind, lba, blocks) = match work {
            Work::Fill(f) => {
                let (lba, blocks) = (f.piece.lba, f.piece.blocks);
                (Kind::Fill(f), lba, blocks)
            }
            #[cfg(feature = "bench")]
            Work::Bench(lba, n) => (Kind::Bench, lba, n),
            Work::Nothing => return give(h, &self.line),
        };
        self.bursts = 0;
        self.begin(h, kind, lba, blocks);
    }

    /// CS low, CMD18 and its R1, then the first token scan. A card still
    /// busy is left for a later pick.
    fn begin(&mut self, mut h: Held, kind: Kind, lba: Lba, blocks: BlockCount) {
        let Some(mode) = CARD_ADDR.get() else {
            return self.drop_job(h, kind, Why::Unmounted);
        };
        let Some(arg) = mode.arg(lba) else {
            return self.drop_job(h, kind, Why::Range);
        };
        if let Err(why) = h.polled(&self.line) {
            return self.drop_job(h, kind, why);
        }
        h.select();
        if let Err(why) = h.ready(&self.line) {
            return self.drop_job(h, kind, why);
        }
        let t0 = dwt();
        match h.command(CMD18, arg, &self.line) {
            Ok(()) => {
                let end = kind.start();
                self.fly(h, Walk::new(blocks), t0, kind, end)
            }
            Err(why) => self.abort(h, kind, why),
        }
    }

    /// The next burst of `walk`, or its stop.
    fn fly(&mut self, h: Held, walk: Walk, t0: Cycles, kind: Kind, end: usize) {
        let walk = match walk {
            Walk::Read(r) => r,
            Walk::Stop => return self.finish(h, t0, kind),
        };
        let Some(dest) = Dest::of(&walk, &kind, end) else {
            return self.abort(h, kind, Why::Bound);
        };
        let upto = kind.landing(end) + dest.n.get();
        BUS.renew(now_ms());
        #[cfg(feature = "bench")]
        let (dest, fault) = bench::aim(dest);
        match h.fly(dest, &self.line) {
            Ok(bus) => {
                self.bursts += 1;
                #[cfg(feature = "bench")]
                bench::strike(fault, &bus.0);
                self.job = Job::Read(Transfer {
                    bus,
                    phase: Phase::Read {
                        walk,
                        t0,
                        kind,
                        end,
                        upto,
                    },
                })
            }
            Err((f, why)) => {
                let h = f.ground(&self.line);
                self.abort(h, kind, why);
            }
        }
    }

    /// A burst landed: its blocks' data packed and CRC-checked.
    fn advance(
        &mut self,
        bus: Flying,
        walk: Reading,
        t0: Cycles,
        kind: Kind,
        end: usize,
        upto: usize,
    ) {
        let mut h = match bus.land(&self.line) {
            Ok(h) => h,
            Err((f, why)) => {
                let h = f.ground(&self.line);
                return self.abort(h, kind, why);
            }
        };
        let from = kind.landing(end);
        let next = match h.staging().landed(upto) {
            Some(buf) => walk.landed(buf, from).map_err(Why::Proto),
            None => Err(Why::Bound),
        };
        #[cfg(feature = "fault-inject")]
        let next = match next {
            Ok(_)
                if matches!(kind, Kind::Fill(_))
                    && FAIL
                        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
                        .is_ok() =>
            {
                Err(Why::Proto(ProtoError::Crc))
            }
            n => n,
        };
        match next {
            Ok((Walk::Read(r), _)) if r.timed_out(self.line.clock) => {
                self.abort(h, kind, Why::Proto(ProtoError::Timeout))
            }
            Ok((walk, end)) => self.fly(h, walk, t0, kind, end),
            Err(why) => self.abort(h, kind, why),
        }
    }

    /// Every block of the read landed and CRC-good. A piece with more
    /// after it: CMD12, the drain, then the next piece. The last: the
    /// chunk goes to its slot (a stop error after keeps it, I8), then
    /// CMD12 and the drain.
    fn finish(&mut self, mut h: Held, t0: Cycles, kind: Kind) {
        let done = match kind {
            Kind::Fill(f) => match f.row.read().after(&f.piece) {
                Some(piece) => {
                    let f = Fill {
                        piece,
                        attempt: Attempt::First,
                        ..f
                    };
                    return self.stop(h, Then::More(f));
                }
                None => {
                    // The row's hold drops after the chunk is in its slot.
                    let filled = self.deliver(&mut h, f);
                    #[cfg(feature = "bench")]
                    if filled {
                        bench::filled(self.line.us(dwt().since(t0)));
                    }
                    #[cfg(not(feature = "bench"))]
                    let _ = filled;
                    Done::Filled
                }
            },
            #[cfg(feature = "bench")]
            Kind::Bench => Done::Bench,
        };
        self.stop(h, Then::Next { done, t0 });
    }

    /// The chunk de-framed into its slot (bench builds check it against
    /// its bed's table, and publish it either way), the guard, then Ready:
    /// true if it went Ready.
    fn deliver(&mut self, h: &mut Held, f: Fill) -> bool {
        let Fill { claim, row, .. } = f;
        let at = row.read();
        let Some(out) = self.side.fill_buf(&claim) else {
            // Stale: the audio cancelled it, and the fill lands nowhere.
            let _ = self.side.fill_done(claim);
            return false;
        };
        let len = usize::try_from(at.packed().get()).unwrap_or(usize::MAX);
        let framed = h
            .staging()
            .chunk(at.blocks())
            .zip(out.get_mut(..len))
            .map(|(raw, out)| {
                let n = deframe(raw, at.skip(), out);
                #[cfg(feature = "bench")]
                if n.is_ok() {
                    bench::check(claim.voice(), claim.chunk(), out);
                }
                n
            });
        let err = match framed {
            Some(Ok(_)) => {
                self.rot.cleared(row.key());
                self.strikes = Strikes::new();
                self.side.write_guard(&claim);
                return self.side.fill_done(claim);
            }
            // A record's tag or length is wrong: the same bytes would come
            // back, so unretried; the card's fault the first time, the
            // file's after (#609).
            Some(Err(_)) => row.frame_error(&mut self.rot),
            // The staging or the slot too short: ours, not the file's, so
            // the fault, never a verdict on the chunk.
            None => ReadError::Frame(Blame::Card),
        };
        failed(err);
        self.side.requeue(claim);
        false
    }

    /// CMD12 and its answer, then the busy drained. A stop that can't be
    /// sent faults the card: no second CMD12.
    fn stop(&mut self, mut h: Held, then: Then) {
        match h.stop(&self.line) {
            Ok(answer) => {
                // After CRC-good data: the blocks stand (I8).
                if answer.is_err() && matches!(then, Then::Next { .. } | Then::More(_)) {
                    STATS.stop_errors.fetch_add(1, Ordering::Relaxed);
                }
                self.drain(h, self.line.drain_bytes.get(), then);
            }
            Err(_) => self.fault(h, then),
        }
    }

    /// The busy, polled for `READY_BYTES` (after a read it is mostly gone
    /// by then), else one 0xFF burst into `scan`, `left` bytes of drain
    /// still allowed.
    /// The bus is polled (`stop` and `drained` leave it so); the poll's
    /// bytes count against `left`.
    fn drain(&mut self, h: Held, left: u32, then: Then) {
        match h.ready(&self.line) {
            Ok(()) => return self.stopped(h, then),
            Err(Why::Busy) if left > READY_BYTES => {}
            Err(_) => return self.fault(h, then),
        }
        let left = left - READY_BYTES;
        BUS.renew(now_ms());
        match h.fly(Dest::scan(), &self.line) {
            Ok(bus) => {
                self.bursts += 1;
                self.job = Job::Read(Transfer {
                    bus,
                    phase: Phase::Drain { left, then },
                })
            }
            Err((f, _)) => {
                let h = f.ground(&self.line);
                self.fault(h, then);
            }
        }
    }

    /// A drain burst landed: the busy polled again.
    fn drained(&mut self, bus: Flying, left: u32, then: Then) {
        let mut h = match bus.land(&self.line) {
            Ok(h) => h,
            Err((f, _)) => {
                let h = f.ground(&self.line);
                return self.fault(h, then);
            }
        };
        if h.polled(&self.line).is_err() {
            return self.fault(h, then);
        }
        let burst = u32::try_from(SCAN_BYTES).unwrap_or(u32::MAX);
        self.drain(h, left.saturating_sub(burst), then);
    }

    /// The card out of busy: CS high, then the read's end.
    fn stopped(&mut self, mut h: Held, then: Then) {
        h.end(&self.line);
        match then {
            Then::Next { done, t0 } => {
                STATS.reads.fetch_add(1, Ordering::Relaxed);
                STATS.bursts.fetch_add(self.bursts, Ordering::Relaxed);
                STATS.bursts_max.fetch_max(self.bursts, Ordering::Relaxed);
                let took = self.line.us(dwt().since(t0));
                match done {
                    Done::Filled => {}
                    #[cfg(feature = "bench")]
                    Done::Bench => MAIL.answer(Ok(took)),
                }
                #[cfg(not(feature = "bench"))]
                let _ = took;
                self.next(h);
            }
            Then::Give(owed) => {
                give(h, &self.line);
                settle(owed);
            }
            Then::More(f) => {
                let (lba, blocks) = (f.piece.lba, f.piece.blocks);
                self.begin(h, Kind::Fill(f), lba, blocks);
            }
            // Never `begin` from here: the retry's CMD18 runs on a fresh
            // entry, so no entry laps the cycle twice (the stack's bound).
            Then::Retry(f) => {
                self.retry = Some(f);
                give(h, &self.line);
                pend();
            }
        }
    }

    /// The card never left busy, or couldn't be stopped: CS high, the
    /// fault set, the bus given. Nothing starts until a mount clears it.
    fn fault(&mut self, mut h: Held, then: Then) {
        h.end(&self.line);
        set_fault();
        give(h, &self.line);
        match then {
            Then::Next { done, .. } => match done {
                Done::Filled => {}
                #[cfg(feature = "bench")]
                Done::Bench => MAIL.answer(Err(ReadFail::Fault)),
            },
            Then::Give(owed) => settle(owed),
            // The whole chunk requeued, once: its one claim moves here.
            Then::More(f) | Then::Retry(f) => self.side.requeue(f.claim),
        }
    }

    /// The bus kept for the next read while any stream wants a fill it
    /// can read (a waiting UI gets it only then: `serve_step`).
    fn next(&mut self, h: Held) {
        match self.work() {
            Work::Nothing => give(h, &self.line),
            work => match h.keep() {
                Some(h) => self.start(h, work),
                None => self.unwork(work),
            },
        }
    }

    /// Work claimed that won't start: its claim back, the bench answered.
    fn unwork(&mut self, work: Work) {
        match work {
            Work::Fill(f) => self.side.requeue(f.claim),
            #[cfg(feature = "bench")]
            Work::Bench(..) => MAIL.answer(Err(ReadFail::Fault)),
            Work::Nothing => {}
        }
    }

    /// No command sent: CS high, the bus back, then the job.
    fn drop_job(&mut self, h: Held, kind: Kind, why: Why) {
        h.deselect();
        give(h, &self.line);
        settle(self.fail_job(kind, why));
    }

    /// The abort (I2): the job's claim requeued, then the card stopped
    /// (CMD12, its answer, the busy drained) and the bus given back. A
    /// requeued want waits for the next wake (a new want, or `watch`), so
    /// a card that keeps failing can't hold this handler in a loop. A
    /// fill's protocol error (C1: CMD18's R1, a token, a CRC16, a token
    /// timeout) is read again once, or blamed (`on_error`): the card
    /// faults for a chunk's first, its chunk is passed over after.
    fn abort(&mut self, h: Held, kind: Kind, why: Why) {
        STATS.aborts.fetch_add(1, Ordering::Relaxed);
        let then = match (kind, why) {
            (Kind::Fill(f), Why::Proto(e)) => {
                match failed(f.row.proto_error(e, f.attempt, &mut self.rot)) {
                    ErrorStep::Retry => Then::Retry(Fill {
                        attempt: Attempt::Retry,
                        ..f
                    }),
                    ErrorStep::Fault | ErrorStep::FrameError | ErrorStep::Rotted => {
                        Then::Give(self.fail_job(Kind::Fill(f), why))
                    }
                }
            }
            (kind, why) => Then::Give(self.fail_job(kind, why)),
        };
        self.stop(h, then);
    }

    /// A fill's claim requeued, the card faulted at its `Strikes`; a
    /// bench read's answer, owed.
    fn fail_job(&mut self, kind: Kind, why: Why) -> Owed {
        match kind {
            Kind::Fill(f) => {
                self.side.requeue(f.claim);
                if why.strikes() && self.strikes.strike() {
                    set_fault();
                }
                Owed::default()
            }
            #[cfg(feature = "bench")]
            Kind::Bench => Owed(Some(match why {
                Why::Proto(e) => ReadFail::Proto(e),
                Why::Unmounted => ReadFail::Unmounted,
                Why::Range => ReadFail::Range,
                _ => ReadFail::Aborted,
            })),
        }
    }
}

/// The read fault: nothing starts until the UI's re-init and mount clear
/// it. Counted once (`counted`).
fn set_fault() {
    let was = BUS.fault();
    BUS.set_fault(CardFault::Read);
    counted(was);
}

/// A fault just set from `was`: counted if it is a new one, so a decided
/// fault whose CMD12 then fails (`abort` → `stop` → `fault`) counts once.
fn counted(was: CardFault) {
    if was == CardFault::Clear && BUS.fault() == CardFault::Read {
        STATS.faults.fetch_add(1, Ordering::Relaxed);
    }
}

/// A fill's read error, decided (`serve::read_failed`, which sets the
/// fault and counts a frame error), its fault counted.
fn failed(err: ReadError) -> ErrorStep {
    let was = BUS.fault();
    let step = read_failed(err, &BUS, &STREAM_STATS);
    counted(was);
    step
}

/// A bench answer owed, given once the bus is.
fn settle(owed: Owed) {
    #[cfg(feature = "bench")]
    if let Owed(Some(f)) = owed {
        MAIL.answer(Err(f));
    }
    #[cfg(not(feature = "bench"))]
    let Owed() = owed;
}

#[cfg(feature = "bench")]
use bench::MAIL;
#[cfg(feature = "bench")]
pub use bench::{
    BenchPort, FILL_RING, ReadFail, beds, bench_port, clear_fault, fill_beds, fill_time, fills,
    inject,
};

/// The console's timed reads (I6): the thread posts one and polls for its
/// answer; the IRQ runs it as a job of its own. The thread never takes the
/// bus, so nothing waits on its own interrupt.
#[cfg(feature = "bench")]
mod bench {
    use core::mem::MaybeUninit;
    use core::ptr::{addr_of, addr_of_mut};
    use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering, fence};

    use crate::shared::dwt;
    use chimera_core::stream::bench::{BENCH, BenchBeds};
    use chimera_core::stream::layout::ChunkIdx;
    use chimera_core::voice_alloc::VoiceIdx;
    use chimera_fat::sd_proto::ProtoError;
    use chimera_hal::card_bus::CardFault;
    use chimera_hal::store::StoreError;
    use chimera_hal::units::{BlockCount, Lba, Micros, Millis};

    use chimera_core::console::Inject;

    use super::{BUS, CARD_ADDR, Dest, Held, pend, unstick};
    use crate::controls::now_ms;
    use crate::sd::SD_IDLE_MS;

    /// Fills that went Ready since boot: also the next index into `FILL_US`.
    static FILLS: AtomicU32 = AtomicU32::new(0);
    /// The fill ring's length: the UI drains it every pass, far more often.
    pub const FILL_RING: u32 = 64;
    /// Each fill's µs, from its last piece's CMD18 to its Ready.
    static FILL_US: [AtomicU32; FILL_RING as usize] =
        [const { AtomicU32::new(0) }; FILL_RING as usize];

    /// The IRQ's, the one writer: a fill went Ready in `us`.
    pub(super) fn filled(us: Micros) {
        let i = FILLS.load(Ordering::Relaxed);
        FILL_US[(i % FILL_RING) as usize].store(us.get(), Ordering::Relaxed);
        FILLS.store(i.wrapping_add(1), Ordering::Release);
    }

    /// Fills since boot.
    pub fn fills() -> u32 {
        FILLS.load(Ordering::Acquire)
    }

    /// Fill `i`'s time; `None` before it lands, or once the IRQ may be
    /// writing its slot again (checked after the read).
    pub fn fill_time(i: u32) -> Option<Micros> {
        let us = FILL_US[(i % FILL_RING) as usize].load(Ordering::Relaxed);
        // As a seqlock's reader: the slot is read before the count is.
        fence(Ordering::Acquire);
        (1..FILL_RING)
            .contains(&FILLS.load(Ordering::Relaxed).wrapping_sub(i))
            .then_some(Micros::new(us))
    }

    /// The fault armed for the next read burst: 0 none, else `Inject`'s.
    static ARMED: AtomicU8 = AtomicU8::new(0);
    const KINDS: [Inject; 3] = [Inject::Abort, Inject::Te, Inject::Modf];
    /// DTCM's base: DMA2 has no path to it, so a burst aimed there is a
    /// transfer error that writes nothing (RM0433 § 2.1).
    const DTCM: u32 = 0x2000_0000;
    /// CR1 with SPE and without SSI: MODF, as SSM = 1 (I3's bench breach).
    const SPE_ONLY: u32 = 1;

    /// B9's: the next read burst gets `i`.
    pub fn inject(i: Inject) {
        let code = KINDS
            .iter()
            .position(|&k| k == i)
            .map_or(0, |p| p as u8 + 1);
        ARMED.store(code, Ordering::Release);
    }

    /// Any fault still armed, dropped.
    pub fn clear_fault() {
        ARMED.store(0, Ordering::Release);
    }

    /// The armed fault, taken by this burst; a TE's burst aims at DTCM.
    pub(super) fn aim(dest: Dest) -> (Dest, Option<Inject>) {
        let fault = match ARMED.swap(0, Ordering::AcqRel) {
            0 => None,
            c => KINDS.get(usize::from(c) - 1).copied(),
        };
        match fault {
            Some(Inject::Te) => (Dest { at: DTCM, ..dest }, fault),
            _ => (dest, fault),
        }
    }

    /// The burst just started: an abort asked, or CR1 without SSI.
    pub(super) fn strike(fault: Option<Inject>, h: &Held) {
        match fault {
            Some(Inject::Abort) => BUS.ask_abort(),
            // SAFETY: CR1's bits as RM0433 lays them; the one write that
            // drops SSI, made only here, armed only by `card dma inject
            // modf`. The bus is this handler's (`Held`).
            Some(Inject::Modf) => h.spi().cr1.write(|w| unsafe { w.bits(SPE_ONLY) }),
            Some(Inject::Te) | None => {}
        }
    }

    /// The beds' cell, written once a boot: Empty → Writing (the UI, by
    /// CAS) → Ready, after which nothing writes it, so the IRQ and the
    /// audio read it freely. A new `BEDS.CHK` needs a reboot.
    const EMPTY: u8 = 0;
    const WRITING: u8 = 1;
    const READY: u8 = 2;
    static BEDS_STATE: AtomicU8 = AtomicU8::new(EMPTY);
    // SAFETY: 8.5 KiB, NOLOAD in D2 beside the UI: garbage at boot, which
    // is sound as it is `MaybeUninit`, reached only by address, and read
    // only once Ready, after `fill_beds` built it in place.
    #[unsafe(link_section = ".ram_d2")]
    static mut BEDS: MaybeUninit<BenchBeds> = MaybeUninit::uninit();

    /// The beds, once `fill_beds` has set them.
    pub fn beds() -> Option<&'static BenchBeds> {
        // SAFETY: Ready only after `fill_beds` wrote the whole value (its
        // `Release`, this `Acquire`), and nothing writes it again.
        (BEDS_STATE.load(Ordering::Acquire) == READY)
            .then(|| unsafe { (*addr_of!(BEDS)).assume_init_ref() })
    }

    /// The UI's, once a boot: the beds built in place, then `fill` sets
    /// them. False if they're set already, or `fill` refuses (the cell
    /// is left Empty for another try).
    pub fn fill_beds(fill: impl FnOnce(&mut BenchBeds) -> bool) -> bool {
        if BEDS_STATE
            .compare_exchange(EMPTY, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        // SAFETY: the CAS lets one caller into Writing, and no reader
        // forms a reference before Ready, so this is the only one.
        let beds = BenchBeds::init_in_place(unsafe { &mut *addr_of_mut!(BEDS) });
        let ok = fill(beds);
        BEDS_STATE.store(if ok { READY } else { EMPTY }, Ordering::Release);
        ok
    }

    /// A fill's de-framed chunk against its bed's table (I7).
    /// `check`'s cycles, which `DMA2_STR0` takes out of its own.
    pub(super) static CHECK_CYCLES: AtomicU32 = AtomicU32::new(0);

    pub(super) fn check(v: VoiceIdx, c: ChunkIdx, bytes: &[u8]) {
        let t0 = dwt();
        if let Some(b) = beds().and_then(|b| b.check(v))
            && !b.verify(c, bytes)
        {
            BENCH.check_failed(v);
        }
        CHECK_CYCLES.fetch_add(dwt().since(t0).get(), Ordering::Relaxed);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ReadFail {
        Proto(ProtoError),
        Unmounted,
        /// No answer within `SD_IDLE_MS`.
        Busy,
        Aborted,
        Fault,
        /// No command argument for the block.
        Range,
    }

    impl From<ReadFail> for StoreError {
        fn from(f: ReadFail) -> StoreError {
            match f {
                ReadFail::Proto(ProtoError::Timeout) => StoreError::Timeout,
                ReadFail::Unmounted => StoreError::NoCard,
                ReadFail::Busy => StoreError::Busy,
                ReadFail::Proto(_) | ReadFail::Aborted | ReadFail::Fault | ReadFail::Range => {
                    StoreError::Io
                }
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    enum State {
        Empty = 0,
        Queued = 1,
        /// The IRQ took it.
        Running = 2,
        Done = 3,
    }

    /// One read at a time: Empty → Queued (the thread) → Running → Done
    /// (the IRQ) → Empty (the thread). The fields beside the state are
    /// written before its `Release` and read after its `Acquire`.
    pub(super) struct Mailbox {
        state: AtomicU8,
        lba: AtomicU32,
        blocks: AtomicU32,
        /// 0 for `Ok`, else the failure; then its µs or byte.
        fail: AtomicU8,
        value: AtomicU32,
    }

    pub(super) static MAIL: Mailbox = Mailbox {
        state: AtomicU8::new(State::Empty as u8),
        lba: AtomicU32::new(0),
        blocks: AtomicU32::new(1),
        fail: AtomicU8::new(0),
        value: AtomicU32::new(0),
    };

    impl Mailbox {
        fn state(&self) -> State {
            match self.state.load(Ordering::Acquire) {
                1 => State::Queued,
                2 => State::Running,
                3 => State::Done,
                _ => State::Empty,
            }
        }

        fn step(&self, from: State, to: State) -> bool {
            self.state
                .compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        }

        /// A read is waiting.
        pub(super) fn queued(&self) -> bool {
            self.state() == State::Queued
        }

        /// The IRQ's: Queued → Running, the request.
        pub(super) fn claim(&self) -> Option<(Lba, BlockCount)> {
            if !self.step(State::Queued, State::Running) {
                return None;
            }
            let n = BlockCount::new(self.blocks.load(Ordering::Relaxed)).unwrap_or(BlockCount::ONE);
            Some((Lba::new(self.lba.load(Ordering::Relaxed)), n))
        }

        /// The IRQ's: Running → Done.
        pub(super) fn answer(&self, r: Result<Micros, ReadFail>) {
            let (fail, value) = match r {
                Ok(us) => (0, us.get()),
                Err(ReadFail::Proto(ProtoError::Crc)) => (1, 0),
                Err(ReadFail::Proto(ProtoError::ErrorToken(b))) => (2, u32::from(b)),
                Err(ReadFail::Proto(ProtoError::Timeout)) => (3, 0),
                Err(ReadFail::Proto(ProtoError::R1(b))) => (4, u32::from(b)),
                Err(ReadFail::Unmounted) => (5, 0),
                Err(ReadFail::Busy) => (6, 0),
                Err(ReadFail::Aborted) => (7, 0),
                Err(ReadFail::Fault) => (8, 0),
                Err(ReadFail::Range) => (9, 0),
            };
            self.fail.store(fail, Ordering::Relaxed);
            self.value.store(value, Ordering::Relaxed);
            self.step(State::Running, State::Done);
        }

        /// The thread's, after `Done`.
        fn result(&self) -> Result<Micros, ReadFail> {
            let v = self.value.load(Ordering::Relaxed);
            let b = u8::try_from(v).unwrap_or(0);
            Err(match self.fail.load(Ordering::Relaxed) {
                0 => return Ok(Micros::new(v)),
                1 => ReadFail::Proto(ProtoError::Crc),
                2 => ReadFail::Proto(ProtoError::ErrorToken(b)),
                3 => ReadFail::Proto(ProtoError::Timeout),
                4 => ReadFail::Proto(ProtoError::R1(b)),
                5 => ReadFail::Unmounted,
                6 => ReadFail::Busy,
                8 => ReadFail::Fault,
                9 => ReadFail::Range,
                _ => ReadFail::Aborted,
            })
        }
    }

    /// The thread's side of the mailbox. Not `Clone`: from `bench_port`,
    /// once.
    pub struct BenchPort(());

    static TAKEN: AtomicBool = AtomicBool::new(false);

    pub fn bench_port() -> Option<BenchPort> {
        (!TAKEN.swap(true, Ordering::AcqRel)).then_some(BenchPort(()))
    }

    impl BenchPort {
        /// `n` blocks from `lba` through the staging, only their CRCs
        /// checked: the µs from CMD18 to the stop.
        pub fn read(&mut self, lba: Lba, n: BlockCount) -> Result<Micros, ReadFail> {
            if CARD_ADDR.get().is_none() {
                return Err(ReadFail::Unmounted);
            }
            if BUS.fault() == CardFault::Read {
                return Err(ReadFail::Fault);
            }
            match MAIL.state() {
                // A read that timed out here still runs: busy until it ends.
                State::Queued | State::Running => return Err(ReadFail::Busy),
                State::Done => MAIL.state.store(State::Empty as u8, Ordering::Release),
                State::Empty => {}
            }
            MAIL.lba.store(lba.get(), Ordering::Relaxed);
            MAIL.blocks.store(n.get(), Ordering::Relaxed);
            MAIL.state.store(State::Queued as u8, Ordering::Release);
            pend();
            let start = now_ms();
            let mut asked = false;
            loop {
                if MAIL.state() == State::Done {
                    let r = MAIL.result();
                    MAIL.state.store(State::Empty as u8, Ordering::Release);
                    return r;
                }
                let now = now_ms();
                // As `SharedCard`'s spin: a stuck transfer is asked once.
                if !asked {
                    asked = unstick(now);
                }
                if now.elapsed_since(start) > Millis::new(SD_IDLE_MS) {
                    // Never started: withdrawn. Running: left to end.
                    MAIL.step(State::Queued, State::Empty);
                    return Err(ReadFail::Busy);
                }
            }
        }
    }
}
