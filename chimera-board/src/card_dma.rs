//! The card's DMA read path (spec § 4.2): SD multi-block reads over SPI2,
//! DMA2 stream 0 receiving and stream 1 sending 0xFF, run by one state
//! machine in `DMA2_STR0`. It fills the voices' stream rings and, in bench
//! builds, the console's timed reads. Every decision is host-tested
//! (`sd_proto`, `h7_spi`, `edf`, `ring`, `card_bus`); this shell runs them
//! on the registers.
//!
//! A transfer holds the bus as `Stream` from its take to its give. Only a
//! `Held`, made by `Held::take` and spent by `give`, reaches SPI2 and CS,
//! and only a `Held`'s `Staging` reads the DMA's buffer, so no view of it
//! outlives a burst's start.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::{MaybeUninit, offset_of, replace};
use core::ops::Range;
use core::ptr::{self, addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU32, Ordering, compiler_fence};

use chimera_core::stream::edf::{Pick, Want, pick, wants};
use chimera_core::stream::layout::{
    ChunkIdx, ChunkRead, Piece, RAW_CHUNK_BLOCKS, RAW_CHUNK_BYTES, deframe,
};
use chimera_core::stream::ring::{CardSide, Claim};
use chimera_core::stream::units::AudioBlocks;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_fat::h7_spi::{
    ABORT, BURST_END, BurstLen, Cr1, DmaEvent, GIVE, Gate, Mbr, Op, POLLED, Placement, RegFault,
    TAKE, burst, gate,
};
use chimera_fat::sd::NCR_BYTES;
use chimera_fat::sd_addr::CmdArg;
use chimera_fat::sd_proto::{
    BLOCK_BYTES, BLOCK_WITH_CRC, CMD12, CMD18, CRC_BYTES, CmdIndex, ProtoError, SCAN_BYTES,
    STOP_BYTES, ScanOutcome, TOKEN_TIMEOUT_MS, Walk, check_r1, command, stop_response,
};
use chimera_hal::Ms;
use chimera_hal::card_bus::{CardFault, Owner, Took, WATCHDOG_MS, Wake};
use chimera_hal::units::{BlockCount, Bytes, BytesPerSec, Lba, Micros};
use cortex_m::asm::dmb;
use cortex_m::peripheral::{DWT, NVIC};
use stm32h7xx_hal::pac::{self, interrupt};

use crate::controls::now_ms;
use crate::priority::{self, Priority};
use crate::sd::{BUS, CARD_ADDR, SD_FAST_HZ};
use crate::shared::{BLOCKS, rings};

/// The bound on any register poll.
pub const REG_WAIT_US: Micros = Micros::new(50);
/// The polled wait for a card not busy before CMD18: about 20 µs at SCK.
const READY_BYTES: u32 = 64;

const _: () = assert!(WATCHDOG_MS.get() >= 2 * TOKEN_TIMEOUT_MS.get());

// DMAMUX1's channels 8 and 9 are DMA2's streams 0 and 1 (DMA1's are 0-7);
// SPI2's requests (RM0433, the PAC's `DMAREQ_ID_A`).
const RX_MUX: usize = 8;
const TX_MUX: usize = 9;
const SPI2_RX_DMA: u8 = 39;
const SPI2_TX_DMA: u8 = 40;
const RX: usize = 0;
const TX: usize = 1;

/// A raw chunk's blocks with the last one's CRC.
const BLOCKS_LEN: usize = RAW_CHUNK_BYTES + CRC_BYTES;
/// A bench read cycles through the staging a raw chunk at a time.
#[cfg(feature = "bench")]
const RAW_MAX: BlockCount = match BlockCount::new(RAW_CHUNK_BLOCKS) {
    Some(n) => n,
    None => panic!(),
};

/// The token-scan area, then the blocks in place: the IRQ de-frames before
/// it starts the next transfer, so one buffer is enough.
#[repr(C, align(32))]
struct DmaBuf {
    scan: [u8; SCAN_BYTES],
    blocks: [u8; BLOCKS_LEN],
}

pub const STAGING_BYTES: usize = size_of::<DmaBuf>();
const SCAN_AT: usize = offset_of!(DmaBuf, scan);
const BLOCKS_AT: usize = offset_of!(DmaBuf, blocks);
const _: () = assert!(SCAN_AT == 0 && BLOCKS_AT == SCAN_BYTES);
// The last block's CRC ends the buffer: no burst passes its end (I4).
const _: () = assert!(BLOCKS_LEN == RAW_CHUNK_BLOCKS as usize * BLOCK_BYTES + CRC_BYTES);
const _: () = assert!((RAW_CHUNK_BLOCKS as usize - 1) * BLOCK_BYTES + BLOCK_WITH_CRC == BLOCKS_LEN);

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
    /// The longest entry of `DMA2_STR0`, in µs.
    pub irq_us_max: AtomicU32,
}

pub static STATS: CardStats = CardStats {
    aborts: AtomicU32::new(0),
    stop_errors: AtomicU32::new(0),
    irq_us_max: AtomicU32::new(0),
};

/// Wakes the card IRQ.
pub fn pend() {
    NVIC::pend(pac::Interrupt::DMA2_STR0);
}

/// Where voice `v`'s chunk `c` lies on the card: a CMD18 a piece, at most
/// a raw chunk's blocks in all (`StreamFile::chunk_read`), so no burst
/// passes the staging's end (I4). Bench builds read stream k's bed; the
/// stream registry (Task 22) answers the rest.
#[cfg(feature = "bench")]
fn locate(v: VoiceIdx, c: ChunkIdx) -> Option<ChunkRead<'static>> {
    bench::bed(v)?.file().chunk_read(c)
}

#[cfg(not(feature = "bench"))]
fn locate(_: VoiceIdx, _: ChunkIdx) -> Option<ChunkRead<'static>> {
    None
}

/// Voice `v` streams from a file this IRQ knows.
fn serves(v: VoiceIdx) -> bool {
    #[cfg(feature = "bench")]
    return bench::bed(v).is_some();
    #[cfg(not(feature = "bench"))]
    {
        let _ = v;
        false
    }
}

/// The Wanted slots this IRQ can serve.
fn servable() -> impl Iterator<Item = Want> {
    rings()
        .into_iter()
        .flat_map(wants)
        .filter(|w| serves(w.voice))
}

/// The UI loop's, each pass. A transfer stuck past `WATCHDOG_MS` (a TE,
/// a MODF, a lost TC) is asked to abort. A want left with the bus free (a
/// failed fill requeued, or a yield whose UI gave up) is woken here.
pub fn watch(now: Ms) {
    if !unstick(now) && BUS.owner() == Owner::Free && serving() && servable().next().is_some() {
        pend();
    }
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

/// What the IRQ runs on: the SPI clock it set and its bounds.
struct Line {
    mbr: Mbr,
    /// Bytes a second at the real SCK (M5).
    clock: BytesPerSec,
    /// `REG_WAIT_US` in cycles.
    reg_wait: u32,
    /// `TOKEN_TIMEOUT_MS` of bytes at SCK: the busy drain's bound.
    drain_bytes: u32,
    cycles_per_us: u32,
}

impl Line {
    fn us(&self, cycles: u32) -> Micros {
        Micros::new(cycles / self.cycles_per_us)
    }
}

/// Polls `ready` for at most `cycles` cycles and `cycles` tries, so a
/// cycle counter that doesn't count still ends it.
fn wait(cycles: u32, mut ready: impl FnMut() -> bool) -> bool {
    let t0 = DWT::cycle_count();
    for _ in 0..cycles {
        if ready() {
            return true;
        }
        if DWT::cycle_count().wrapping_sub(t0) > cycles {
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

/// TAKE's first three: a kept bus's CFG1 is already this handler's, and
/// SPE goes off before TSIZE is written.
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
    /// mode, the audio never takes and this handler doesn't nest.
    fn keep(self) -> Held {
        let _ = BUS.give();
        let took = BUS.take_or_want(now_ms());
        debug_assert_eq!(took, Took::Taken);
        self
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
            Op::SaveCfg1 => self.cfg1 = spi.cfg1.read().bits(),
            Op::SetMbr => spi.cfg1.modify(|_, w| w.mbr().bits(line.mbr.bits())),
            // SAFETY: the value `SaveCfg1` read from this register.
            Op::RestoreCfg1 => spi.cfg1.write(|w| unsafe { w.bits(self.cfg1) }),
            Op::Tsize(n) => {
                let n = n.map_or(0, |n| u16::try_from(n.get()).unwrap_or(0));
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
                let n = u16::try_from(n.get()).unwrap_or(0);
                let (rx, tx) = (&d.st[RX], &d.st[TX]);
                rx.ndtr.write(|w| w.ndt().bits(n));
                // SAFETY: `dest` is inside `DMA_BUF` with `n` bytes after it
                // (`Dest::of`), in D2 SRAM, which DMA2 reaches.
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
        ops.iter().try_for_each(|&o| self.op(o, line, dest))
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
        let _ = h.run(&ABORT, line, 0);
        h
    }
}

/// GIVE, then the bus; a give that finds it wanted pends. Every op runs
/// past a failed suspend, so CFG1 (MBR, the DMA enables) is always the
/// HAL's again.
fn give(mut h: Held, line: &Line) {
    for &o in &GIVE {
        let _ = h.op(o, line, 0);
    }
    if BUS.give() == Wake::Pend {
        pend();
    }
}

/// The DMA buffer while both streams are off. Each view covers only bytes
/// the transfer under way wrote, and is formed right after the burst or
/// the read that wrote them.
struct Staging<'h>(PhantomData<&'h mut Held>);

impl Staging<'_> {
    /// The scan burst just landed: all of `scan`.
    fn scan(&self) -> &[u8; SCAN_BYTES] {
        // SAFETY: the streams are off while this borrows its `Held`, so
        // nothing writes the buffer; the scan burst wrote every byte of
        // `scan` before its TC (`advance`'s one use).
        unsafe { &*staged(SCAN_AT).cast::<[u8; SCAN_BYTES]>() }
    }

    /// A block and its CRC at `off`: its tail copy and its rest burst wrote
    /// them (`advance`'s one use).
    fn block(&self, off: Bytes) -> Option<&[u8; BLOCK_WITH_CRC]> {
        let off = usize::try_from(off.get()).ok()?;
        (off + BLOCK_WITH_CRC <= BLOCKS_LEN).then(|| {
            // SAFETY: inside `blocks` (checked above), streams off, every
            // byte written by this block's tail and rest.
            unsafe { &*staged(BLOCKS_AT + off).cast::<[u8; BLOCK_WITH_CRC]>() }
        })
    }

    /// A token's tail, the block's first bytes, to its place.
    fn copy_tail(&mut self, tail: Range<usize>, off: Bytes) -> Option<()> {
        let off = usize::try_from(off.get()).ok()?;
        let n = tail.len();
        (tail.end <= SCAN_BYTES && off + n <= BLOCKS_LEN).then(|| {
            // SAFETY: both ranges inside their fields (checked above), which
            // don't overlap; streams off, so only this CPU touches them.
            unsafe {
                ptr::copy_nonoverlapping(staged(SCAN_AT + tail.start), staged(BLOCKS_AT + off), n)
            }
        })
    }

    /// The first `blocks` of a read placed whole (`Placement::Chunk`).
    fn chunk(&self, blocks: BlockCount) -> Option<&[u8]> {
        let n = usize::try_from(blocks.bytes().get()).ok()?;
        (n <= BLOCKS_LEN).then(|| {
            // SAFETY: inside `blocks`; every block of the read wrote its
            // 512 B and its CRC, so all `n` bytes are written; streams off.
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
    /// A token scan's, or a busy drain's.
    fn scan() -> Dest {
        Dest {
            at: staged(SCAN_AT) as u32,
            n: SCAN_LEN,
        }
    }

    /// Where `walk`'s next burst lands; `None` at its stop.
    fn of(walk: &Walk, kind: &Kind) -> Option<Dest> {
        let (at, n) = match walk {
            Walk::Scan(_) => return Some(Dest::scan()),
            Walk::Rest(r) => {
                let off = usize::try_from(kind.offset(r.block()).get()).ok()?;
                let have = usize::try_from(r.have().get()).ok()?;
                let n = usize::try_from(r.want().get()).ok()?;
                // Never past `blocks`' end (I4); `ChunkRead`'s pieces and the
                // ring's modulus keep every block inside, this keeps the DMA so.
                (off + have + n <= BLOCKS_LEN).then_some((BLOCKS_AT + off + have, n))?
            }
            Walk::Stop => return None,
        };
        Some(Dest {
            at: staged(at) as u32,
            n: BurstLen::new(n)?,
        })
    }
}

/// A slot's fill: one job, however many CMD18s its chunk takes.
struct Fill {
    /// The one `Claim` (Task 5) of the slot.
    claim: Claim,
    at: ChunkRead<'static>,
    /// The piece under way.
    piece: Piece,
}

/// What a transfer is for.
enum Kind {
    Fill(Fill),
    #[cfg(feature = "bench")]
    Bench,
}

impl Kind {
    /// Where the read's `block` lands: a piece after the pieces before it
    /// (`Placement::Chunk`), so a chunk's blocks lie whole for `deframe`.
    fn offset(&self, block: u32) -> Bytes {
        match self {
            Kind::Fill(f) => Placement::Chunk.offset(f.piece.at.saturating_add(block)),
            #[cfg(feature = "bench")]
            Kind::Bench => Placement::Ring(RAW_MAX).offset(block),
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
        t0: u32,
    },
    /// An abort: the bus goes back, then the answer.
    Give(Owed),
    /// A fill's piece read: the next piece's CMD18, on the bus held.
    More(Fill),
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
        walk: Walk,
        /// Cycles at CMD18.
        t0: u32,
        kind: Kind,
    },
    /// After CMD12: 0xFF bursts into `scan` until one ends in 0xFF, at most
    /// `left` bytes more.
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
/// `Priority::CARD`, unmasked last. `spi_ker_hz` is SPI2's kernel clock
/// (`Spi::<SPI2, _>::kernel_clk_unwrap`).
pub fn init(nvic: &mut NVIC, side: CardSide<'static>, spi_ker_hz: u32, cpu_hz: u32) {
    let Some(mbr) = Mbr::fastest(spi_ker_hz, SD_FAST_HZ) else {
        return;
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
    let cycles_per_us = (cpu_hz / 1_000_000).max(1);
    let line = Line {
        mbr,
        clock: BytesPerSec::spi(mbr.sck_hz(spi_ker_hz)),
        reg_wait: REG_WAIT_US.get() * cycles_per_us,
        drain_bytes: BytesPerSec::spi(mbr.sck_hz(spi_ker_hz))
            .bytes_in(TOKEN_TIMEOUT_MS)
            .get(),
        cycles_per_us,
    };
    let st = IrqState {
        side,
        job: Job::Idle,
        line,
    };
    // SAFETY: once (`side` exists once: `Rings::sides`), before the unmask.
    unsafe { IRQ.set(st) };
    priority::set_irq(nvic, pac::Interrupt::DMA2_STR0, Priority::CARD);
    // SAFETY: the state is set; the handler touches DMA2, SPI2 and CS only
    // as above.
    unsafe { NVIC::unmask(pac::Interrupt::DMA2_STR0) };
}

/// RX's transfer complete or error, or a pend (a want, a give, an abort):
/// one step of the job. A pend mid-transfer is ignored (I6).
#[interrupt]
fn DMA2_STR0() {
    let t = DWT::cycle_count();
    // SAFETY: `IRQ`'s one `get`: `init` set it before unmasking this
    // handler, which doesn't nest with itself.
    let st = unsafe { IRQ.get() };
    st.step(events(), BUS.take_abort());
    let us = st.line.us(DWT::cycle_count().wrapping_sub(t));
    STATS.irq_us_max.fetch_max(us.get(), Ordering::Relaxed);
}

/// The work `pick` chose.
enum Work {
    Fill(Want),
    /// A read in the bench's mailbox, claimed once the bus is held.
    #[cfg(feature = "bench")]
    Bench,
    Yield,
    Nothing,
}

impl IrqState {
    fn step(&mut self, ev: DmaEvent, asked: bool) {
        let in_flight = matches!(self.job, Job::Read(_));
        let g = gate(in_flight, ev);
        if g == Gate::Ignore && !asked {
            return;
        }
        // Not in flight, a TE or an ask has nothing to stop: look instead.
        let Job::Read(Transfer { bus, phase }) = replace(&mut self.job, Job::Idle) else {
            return self.look();
        };
        let fail = if asked {
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
            (Phase::Read { walk, t0, kind }, None) => self.advance(bus, walk, t0, kind),
            (Phase::Drain { left, then }, None) => self.drained(bus, left, then),
        }
    }

    /// Nothing in flight: start the earliest want, else the bench's read.
    fn look(&mut self) {
        let work = self.work();
        if let Work::Yield = work {
            // The waiting UI's give pends this handler again.
            return BUS.want();
        }
        if let Work::Nothing = work {
            return;
        }
        if let Some(h) = Held::take(&self.line) {
            self.start(h, work);
        }
    }

    fn work(&mut self) -> Work {
        if !serving() {
            #[cfg(feature = "bench")]
            if MAIL.claim().is_some() {
                MAIL.answer(Err(if CARD_ADDR.get().is_none() {
                    ReadFail::Unmounted
                } else {
                    ReadFail::Fault
                }));
            }
            return Work::Nothing;
        }
        let now = AudioBlocks::new(BLOCKS.load(Ordering::Relaxed));
        match pick(servable(), now, BUS.ui_wait()) {
            Pick::Fill(w) => Work::Fill(w),
            Pick::YieldToUi => Work::Yield,
            Pick::Idle => {
                #[cfg(feature = "bench")]
                if MAIL.queued() {
                    return Work::Bench;
                }
                Work::Nothing
            }
        }
    }

    /// The bus held: the chosen work's CMD18.
    fn start(&mut self, h: Held, work: Work) {
        let (kind, lba, blocks) = match work {
            Work::Fill(w) => match self.side.claim(w.voice, w.slot) {
                Some(claim) => match locate(claim.voice(), claim.chunk())
                    .and_then(|at| Some((at, at.first()?)))
                {
                    Some((at, piece)) => (
                        Kind::Fill(Fill { claim, at, piece }),
                        piece.lba,
                        piece.blocks,
                    ),
                    // A chunk off the file: left Wanted, no pend, so it
                    // can't loop this handler (`watch` wakes it).
                    None => {
                        self.side.requeue(claim);
                        return give(h, &self.line);
                    }
                },
                // The audio cancelled it since the pick.
                None => return self.release(h),
            },
            #[cfg(feature = "bench")]
            Work::Bench => match MAIL.claim() {
                Some((lba, n)) => (Kind::Bench, lba, n),
                // The thread withdrew it since the pick.
                None => return self.release(h),
            },
            Work::Yield | Work::Nothing => return self.release(h),
        };
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
        let t0 = DWT::cycle_count();
        match h.command(CMD18, arg, &self.line) {
            Ok(()) => self.fly(h, Walk::new(blocks), t0, kind),
            Err(why) => self.abort(h, kind, why),
        }
    }

    /// The next burst of `walk`, or its stop.
    fn fly(&mut self, h: Held, walk: Walk, t0: u32, kind: Kind) {
        let Some(dest) = Dest::of(&walk, &kind) else {
            return self.finish(h, t0, kind);
        };
        match h.fly(dest, &self.line) {
            Ok(bus) => {
                self.job = Job::Read(Transfer {
                    bus,
                    phase: Phase::Read { walk, t0, kind },
                })
            }
            Err((f, why)) => {
                let h = f.ground(&self.line);
                self.abort(h, kind, why);
            }
        }
    }

    /// A burst landed: scan it for the token, or check the block it ended.
    fn advance(&mut self, bus: Flying, walk: Walk, t0: u32, kind: Kind) {
        let mut h = match bus.land(&self.line) {
            Ok(h) => h,
            Err((f, why)) => {
                let h = f.ground(&self.line);
                return self.abort(h, kind, why);
            }
        };
        let next = match walk {
            Walk::Scan(s) => match s.scanned(h.staging().scan()) {
                Err(e) => Err(Why::Proto(e)),
                Ok(ScanOutcome::Waiting(s)) if s.timed_out(self.line.clock) => {
                    Err(Why::Proto(ProtoError::Timeout))
                }
                Ok(ScanOutcome::Waiting(s)) => Ok(Walk::Scan(s)),
                Ok(ScanOutcome::Token { tail, rest }) => {
                    let off = kind.offset(rest.block());
                    match h.staging().copy_tail(tail, off) {
                        Some(()) => Ok(Walk::Rest(rest)),
                        None => Err(Why::Bound),
                    }
                }
            },
            Walk::Rest(r) => match h.staging().block(kind.offset(r.block())) {
                Some(b) => r.landed(b).map_err(Why::Proto),
                None => Err(Why::Bound),
            },
            Walk::Stop => Ok(Walk::Stop),
        };
        match next {
            Ok(walk) => self.fly(h, walk, t0, kind),
            Err(why) => self.abort(h, kind, why),
        }
    }

    /// Every block of the read landed and CRC-good. A piece with more
    /// after it: CMD12, the drain, then the next piece. The last: the
    /// chunk goes to its slot (a stop error after keeps it, I8), then
    /// CMD12 and the drain.
    fn finish(&mut self, mut h: Held, t0: u32, kind: Kind) {
        let done = match kind {
            Kind::Fill(f) => match f.at.after(&f.piece) {
                Some(piece) => return self.stop(h, Then::More(Fill { piece, ..f })),
                None => {
                    self.deliver(&mut h, f.claim, &f.at);
                    Done::Filled
                }
            },
            #[cfg(feature = "bench")]
            Kind::Bench => Done::Bench,
        };
        self.stop(h, Then::Next { done, t0 });
    }

    /// The chunk de-framed into its slot (bench builds check it against
    /// its bed's table, and publish it either way), the guard, then Ready.
    fn deliver(&mut self, h: &mut Held, claim: Claim, at: &ChunkRead<'_>) {
        let Some(out) = self.side.fill_buf(&claim) else {
            // Stale: the audio cancelled it, and the fill lands nowhere.
            let _ = self.side.fill_done(claim);
            return;
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
        if let Some(Ok(_)) = framed {
            self.side.write_guard(&claim);
            let _ = self.side.fill_done(claim);
        } else {
            // A record's tag or length is wrong: a read error.
            self.side.requeue(claim);
        }
    }

    /// CMD12 and its answer, then the busy drained by DMA. A stop that
    /// can't be sent faults the card: no second CMD12.
    fn stop(&mut self, mut h: Held, then: Then) {
        match h.stop(&self.line) {
            Ok(answer) => {
                if answer.is_err() && !matches!(then, Then::Give(_)) {
                    STATS.stop_errors.fetch_add(1, Ordering::Relaxed);
                }
                self.drain(h, self.line.drain_bytes, then);
            }
            Err(_) => self.fault(h, then),
        }
    }

    /// One 0xFF burst into `scan`, `left` bytes of drain still allowed.
    fn drain(&mut self, h: Held, left: u32, then: Then) {
        match h.fly(Dest::scan(), &self.line) {
            Ok(bus) => {
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

    /// A drain burst landed: the card is out of busy if it ended in 0xFF.
    fn drained(&mut self, bus: Flying, left: u32, then: Then) {
        let mut h = match bus.land(&self.line) {
            Ok(h) => h,
            Err((f, _)) => {
                let h = f.ground(&self.line);
                return self.fault(h, then);
            }
        };
        let idle = h.staging().scan()[SCAN_BYTES - 1] == 0xFF;
        let left = left.saturating_sub(u32::try_from(SCAN_BYTES).unwrap_or(u32::MAX));
        if !idle {
            return if left == 0 {
                self.fault(h, then)
            } else {
                self.drain(h, left, then)
            };
        }
        h.end(&self.line);
        match then {
            Then::Next { done, t0 } => {
                let took = self.line.us(DWT::cycle_count().wrapping_sub(t0));
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
        }
    }

    /// The card never left busy, or couldn't be stopped: CS high, the
    /// fault set, the bus given. Nothing starts until a mount clears it.
    fn fault(&mut self, mut h: Held, then: Then) {
        h.end(&self.line);
        BUS.set_fault(CardFault::Read);
        give(h, &self.line);
        match then {
            Then::Next { done, .. } => match done {
                Done::Filled => {}
                #[cfg(feature = "bench")]
                Done::Bench => MAIL.answer(Err(ReadFail::Fault)),
            },
            Then::Give(owed) => settle(owed),
            // The whole chunk requeued, once: its one claim moves here.
            Then::More(f) => self.side.requeue(f.claim),
        }
    }

    /// The bus kept for the next pick, unless a waiting UI isn't to wait.
    fn next(&mut self, h: Held) {
        match self.work() {
            Work::Yield | Work::Nothing => self.release(h),
            work => self.start(h.keep(), work),
        }
    }

    /// The bus given; a want still left pends a look.
    fn release(&mut self, h: Held) {
        give(h, &self.line);
        if serving() && servable().next().is_some() {
            pend();
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
    /// a card that keeps failing can't hold this handler in a loop.
    fn abort(&mut self, h: Held, kind: Kind, why: Why) {
        STATS.aborts.fetch_add(1, Ordering::Relaxed);
        let then = Then::Give(self.fail_job(kind, why));
        self.stop(h, then);
    }

    /// A fill's claim requeued; a bench read's answer, owed.
    fn fail_job(&mut self, kind: Kind, why: Why) -> Owed {
        #[cfg(not(feature = "bench"))]
        let _ = why;
        match kind {
            Kind::Fill(f) => {
                self.side.requeue(f.claim);
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
pub use bench::{BenchPort, ReadFail, beds, bench_port, fill_beds};

/// The console's timed reads (I6): the thread posts one and polls for its
/// answer; the IRQ runs it as a job of its own. The thread never takes the
/// bus, so nothing waits on its own interrupt.
#[cfg(feature = "bench")]
mod bench {
    use core::mem::MaybeUninit;
    use core::ptr::{addr_of, addr_of_mut};
    use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

    use chimera_core::stream::bench::{BENCH, Bed, BenchBeds};
    use chimera_core::stream::layout::ChunkIdx;
    use chimera_core::voice_alloc::VoiceIdx;
    use chimera_fat::sd_proto::ProtoError;
    use chimera_hal::card_bus::CardFault;
    use chimera_hal::store::StoreError;
    use chimera_hal::units::{BlockCount, Lba, Micros, Millis};

    use super::{BUS, CARD_ADDR, pend, unstick};
    use crate::controls::now_ms;
    use crate::sd::SD_IDLE_MS;

    /// The beds' cell, written once a boot: Empty → Writing (the UI, by
    /// CAS) → Ready, after which nothing writes it, so the IRQ and the
    /// audio read it freely. A new `BEDS.CHK` needs a reboot.
    const EMPTY: u8 = 0;
    const WRITING: u8 = 1;
    const READY: u8 = 2;
    static BEDS_STATE: AtomicU8 = AtomicU8::new(EMPTY);
    // 10 KiB, NOLOAD in D2 beside the UI: read only once Ready, after
    // `fill_beds` built it in place.
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

    /// Stream `v`'s bed.
    pub(super) fn bed(v: VoiceIdx) -> Option<&'static Bed> {
        beds()?.beds[v.index()].as_ref()
    }

    /// A fill's de-framed chunk against its bed's table (I7).
    pub(super) fn check(v: VoiceIdx, c: ChunkIdx, bytes: &[u8]) {
        if let Some(b) = bed(v)
            && !b.check().verify(c, bytes)
        {
            BENCH.check_failed(v);
        }
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
