//! Bank 2 from bank 1 (ADR 0137): the MPU's view of it, its live reads,
//! its flash controller and the boot's judgement. Only S0-S3
//! (0x08100000..0x0817FFFF) are ever mapped, erased or programmed; MPU
//! region 1 keeps sectors 4-7, the stock bootloader's slots, no-access for
//! good. Only bank 2's own key, control, status and clear registers are
//! named; their values are `chimera_image::regs`'.

use core::sync::atomic::{AtomicU8, Ordering};

use chimera_core::bank2::{Bank2Status, DOOR_CELL};
use chimera_image::regs::{
    CCR_CLEAR, CR_LOCK, CR_START, ERASE_TIMEOUT_MS, KEYS, SR_DBECCERR, WORD_TIMEOUT_MS, busy,
    erase_cr, ms_cycles, program_cr, relock_cr, settle,
};
use chimera_image::{
    BANK2_BASE, BANK2_LEN, Bank2Memory, Bank2Sector, Bank2Word, Flash, FlashFail, ImageRead,
    ReadFail, Verdict,
};
use cortex_m::peripheral::DWT;
use stm32h7xx_hal::pac;

use crate::cache::{BANK2_OPEN_RASR, BANK2_READ_RASR, BANK2_REGION, BANK2_WRITE_RASR, RBAR_VALID};

const _: () = assert!(BANK2_BASE == crate::cache::BANK2_BASE);

/// The board's table in bank 2 (ADR 0175), at `BOARD_DOOR_ADDR`, after the
/// engine's door: what the board's bank 1 calls of its own code there.
#[cfg(feature = "bank2")]
#[repr(C)]
pub(crate) struct BoardDoor {
    magic: u32,
    /// The card interrupt's streaming work.
    #[cfg(not(feature = "sd-probe"))]
    pub(crate) card: crate::card_dma::CardDoor,
}

/// `BoardDoor::magic`.
#[cfg(feature = "bank2")]
const BOARD_MAGIC: u32 = u32::from_le_bytes(*b"B2BD");

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.board")]
#[used]
static BOARD_DOOR: BoardDoor = BoardDoor {
    magic: BOARD_MAGIC,
    #[cfg(not(feature = "sd-probe"))]
    card: crate::card_dma::CardDoor::DOOR,
};

/// `f` on the board's table, `None` closed (see `DoorCell::with`).
#[cfg(feature = "bank2")]
#[inline(always)]
pub(crate) fn board_door<R>(f: impl FnOnce(&BoardDoor) -> R) -> Option<R> {
    use chimera_core::bank2::BOARD_DOOR_ADDR;
    // SAFETY: `bank2.x` links `BOARD_DOOR`, a `BoardDoor`, at its slot,
    // `BOARD_DOOR_ADDR` (its assert).
    unsafe { DOOR_CELL.with_at(BOARD_DOOR_ADDR, f) }
}

/// MPU region 2's state over S0-S3; region 1 (no access) is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mpu2 {
    /// Region 2 off: S0-S3 no-access, as the rest of bank 2.
    Closed,
    /// Read-only, never executed: the judge, the console's `flash`. The
    /// state `cache::init` leaves.
    Read,
    /// Read-write, strongly-ordered: an erase or program.
    Write,
    /// Read-only and executable: the door open.
    Open,
}

impl Mpu2 {
    const fn rasr(self) -> u32 {
        match self {
            Mpu2::Closed => 0,
            Mpu2::Read => BANK2_READ_RASR,
            Mpu2::Write => BANK2_WRITE_RASR,
            Mpu2::Open => BANK2_OPEN_RASR,
        }
    }

    fn from_u8(v: u8) -> Mpu2 {
        match v {
            0 => Mpu2::Closed,
            1 => Mpu2::Read,
            2 => Mpu2::Write,
            _ => Mpu2::Open,
        }
    }

    /// A load from S0-S3 does not fault.
    pub fn readable(self) -> bool {
        self != Mpu2::Closed
    }
}

static STATE: AtomicU8 = AtomicU8::new(Mpu2::Read as u8);

/// Region 2 as last set.
pub fn mpu() -> Mpu2 {
    Mpu2::from_u8(STATE.load(Ordering::Acquire))
}

/// Region 2 to `s`. From the boot or the UI loop only. `Open` is the only
/// cacheable state: into it, the whole D-cache is cleaned and invalidated
/// (set/way) and the whole I-cache invalidated first, so nothing older than
/// the flash is read or run; out of it, the same once region 2 is no longer
/// cacheable, so no line of it (a speculative fill included) outlives it
/// into a write. No cache operation ever names a bank-2 address.
pub fn set_mpu(s: Mpu2) {
    // SAFETY: whole-cache maintenance and the MPU's RBAR/RASR pair. Only the
    // boot and the UI loop call this, never an interrupt; after
    // `cache::init`, `Board` never uses its MPU, and its SCB only for
    // SysTick's priority and to reset, neither of which this touches.
    let mut cp = unsafe { cortex_m::Peripherals::steal() };
    let was = mpu();
    let mut flush = || {
        cp.SCB.clean_invalidate_dcache(&mut cp.CPUID);
        cp.SCB.invalidate_icache();
    };
    if s == Mpu2::Open {
        flush();
    }
    cortex_m::asm::dsb();
    // SAFETY: region 2's base is its size-aligned 0x08100000; its RASR is
    // one of the asserted words; no other region changes.
    unsafe {
        cp.MPU.rbar.write(BANK2_BASE | RBAR_VALID | BANK2_REGION);
        cp.MPU.rasr.write(s.rasr());
    }
    STATE.store(s as u8, Ordering::Release);
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    if was == Mpu2::Open && s != Mpu2::Open {
        flush();
    }
}

fn bank2() -> &'static pac::flash::BANK {
    // SAFETY: FLASH's fixed register block, its bank-2 half: KEYR2, CR2,
    // SR2, CCR2 and FA2R. Only this module writes them, from the boot or
    // the UI loop; the HardFault handler only reads SR2 and FA2R (no read
    // side effects), and no interrupt touches the flash controller.
    unsafe { (*pac::FLASH::ptr()).bank2() }
}

fn cr() -> u32 {
    bank2().cr.read().bits()
}

fn sr() -> u32 {
    bank2().sr.read().bits()
}

/// SR2's flags cleared, so an error an op raises is this op's.
fn clear_flags() {
    // SAFETY: CCR2 only clears SR2's flags, one bit each; it starts,
    // unlocks or erases nothing.
    bank2().ccr.write(|w| unsafe { w.bits(CCR_CLEAR) });
}

/// Bank 2 read live: volatile loads through region 2, uncached in `Read`
/// and `Write`, and in `Open` cached only since the last whole-cache
/// invalidate (nothing writes bank 2 while it is open); a double ECC error
/// in SR2 after them is an `Err`. Bounded to S0-S3, and only while region 2
/// maps them.
pub struct FlashRead;

impl ImageRead for FlashRead {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        let len = u32::try_from(buf.len()).map_err(|_| ReadFail)?;
        let end = off.checked_add(len).ok_or(ReadFail)?;
        if end > BANK2_LEN || !mpu().readable() {
            return Err(ReadFail);
        }
        if len == 0 {
            return Ok(());
        }
        let at = BANK2_BASE + off;
        for (a, b) in (at..).zip(buf.iter_mut()) {
            // SAFETY: `a` is inside S0-S3 (`end <= BANK2_LEN`), which
            // region 2 maps readable (`mpu()`); a load never programs or
            // erases flash.
            *b = unsafe { core::ptr::read_volatile(a as *const u8) };
        }
        if bank2().sr.read().bits() & SR_DBECCERR != 0 {
            return Err(ReadFail);
        }
        Ok(())
    }
}

// SAFETY: `read` loads this chip's bank 2 at `BANK2_BASE + off`, live
// (volatile; region 2 uncached, or cached only since bank 2 last changed). A double ECC error either
// faults (no data returned) or sets SR2.DBECCERR, which is `Err`.
unsafe impl Bank2Memory for FlashRead {}

/// CR2 unlocked; dropping it relocks, whatever happened.
struct Unlocked(());

impl Unlocked {
    /// Bank 2 idle, then KEYR2's key sequence if CR2 is locked.
    fn new() -> Result<Self, FlashFail> {
        if busy(sr()) {
            return Err(FlashFail::Busy);
        }
        if cr() & CR_LOCK != 0 {
            for key in KEYS {
                // SAFETY: KEYR2 takes the two documented keys in order; it
                // unlocks bank 2's CR only, never the option bytes.
                bank2().keyr.write(|w| unsafe { w.bits(key) });
            }
        }
        let u = Unlocked(());
        if cr() & CR_LOCK != 0 {
            return Err(FlashFail::Locked);
        }
        Ok(u)
    }

    /// `v` from `erase_cr` or `program_cr`, or either with START.
    fn set_cr(&self, v: u32) {
        // SAFETY: a CR2 word from `erase_cr` (one of S0-S3) or `program_cr`,
        // maybe with START: neither can hold a bank erase.
        bank2().cr.write(|w| unsafe { w.bits(v) });
        cortex_m::asm::dsb();
    }
}

impl Drop for Unlocked {
    fn drop(&mut self) {
        // SAFETY: sets LOCK, which only a new key sequence clears, and when
        // idle clears PG, SER and START (`relock_cr`).
        bank2()
            .cr
            .write(|w| unsafe { w.bits(relock_cr(cr(), sr())) });
    }
}

/// Polls SR2 until idle, an error flag, or `limit` cycles from `start`.
fn wait(start: u32, limit: u32) -> Result<(), FlashFail> {
    loop {
        let s = sr();
        let late = DWT::cycle_count().wrapping_sub(start) > limit;
        match settle(s, late) {
            Some(f) => return Err(f),
            None if !busy(s) => return Ok(()),
            None => {}
        }
    }
}

/// Bank 2's flash controller over S0-S3, built only by `new`: the door
/// closed and region 2 `Write` (strongly-ordered, uncached), so its reads
/// are live and no cache line holds bank 2 across an erase or program
/// (`set_mpu` invalidates the whole caches out of `Open`). From the boot or
/// the UI loop only; the audio interrupt and the tick keep running from bank 1
/// meanwhile (RM0433 read-while-write), as in the phase 0b spike. Nothing
/// linked into bank 1 reads bank 2 with the door closed.
pub struct FlashBank2 {
    cpu_hz: u32,
    /// The cycle counter moves: every op's timeout can expire.
    clock: bool,
}

impl FlashBank2 {
    pub fn new(cpu_hz: u32) -> Self {
        DOOR_CELL.close();
        set_mpu(Mpu2::Write);
        FlashBank2 {
            cpu_hz,
            clock: crate::clocks::counting(),
        }
    }

    /// `writable`, and a moving cycle counter: without one an op stuck
    /// busy would hang the boot, before the watchdog is armed.
    fn ready(&self) -> Result<(), FlashFail> {
        Self::writable()?;
        if self.clock {
            Ok(())
        } else {
            Err(FlashFail::NoClock)
        }
    }

    /// Still as `new` left it: region 2 `Write`, the door closed. Anything
    /// else (the caller reopened it) refuses the op as `NotWritable`:
    /// nothing erases code a door may run, or stores through a region not
    /// mapped for it.
    fn writable() -> Result<(), FlashFail> {
        if mpu() == Mpu2::Write && !DOOR_CELL.is_open() {
            Ok(())
        } else {
            Err(FlashFail::NotWritable)
        }
    }

    /// The word at `w` as the flash holds it now.
    fn word(w: Bank2Word) -> [u32; 8] {
        let mut got = [0; 8];
        for (a, x) in (w.get()..).step_by(4).zip(got.iter_mut()) {
            // SAFETY: an aligned word inside S0-S3 (`Bank2Word`), region 2
            // `Write` (`writable`); a load never programs or erases.
            *x = unsafe { core::ptr::read_volatile(a as *const u32) };
        }
        got
    }
}

impl ImageRead for FlashBank2 {
    /// `Err` unless still `writable`.
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        Self::writable().map_err(|_| ReadFail)?;
        clear_flags();
        FlashRead.read(off, buf)
    }
}

// SAFETY: `read` is `FlashRead`'s, and only with region 2 `Write`
// (`writable`): strongly-ordered, uncached, live; SR2 cleared first, so
// its DBECCERR is this read's.
unsafe impl Bank2Memory for FlashBank2 {}

// SAFETY: the registers are bank 2's own (`bank2()`); an erase names a
// `Bank2Sector` (S0-S3) through `erase_cr`, a program a `Bank2Word`
// (S0-S3) under `program_cr`; neither can start a bank erase. Each runs
// only with region 2 `Write` and the door closed, and returns `Ok` only
// once SR2 shows the op done with no error flag.
unsafe impl Flash for FlashBank2 {
    /// Clear SR2, SER with `s`'s SNB at ×32, START, then BSY and QW polled
    /// against `ERASE_TIMEOUT_MS`. No retry.
    fn erase(&mut self, s: Bank2Sector) -> Result<(), FlashFail> {
        let start = DWT::cycle_count();
        self.ready()?;
        let u = Unlocked::new()?;
        clear_flags();
        let v = erase_cr(cr(), s);
        u.set_cr(v);
        u.set_cr(v | CR_START);
        wait(start, ms_cycles(ERASE_TIMEOUT_MS, self.cpu_hz))
    }

    /// Refused unless erased; clear SR2, PG at ×32, the eight words, DSB,
    /// then WBNE and QW polled against `WORD_TIMEOUT_MS`.
    fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail> {
        self.ready()?;
        if Self::word(w) != [u32::MAX; 8] {
            return Err(FlashFail::NotErased);
        }
        let u = Unlocked::new()?;
        clear_flags();
        u.set_cr(program_cr(cr()));
        let (words, _) = data.as_chunks::<4>();
        for (a, &b) in (w.get()..).step_by(4).zip(words) {
            let x = u32::from_le_bytes(b);
            // SAFETY: an aligned word of a flash word in S0-S3
            // (`Bank2Word`), region 2 `Write`, CR2.PG set: the store fills
            // bank 2's write buffer, eight make one 256-bit word.
            unsafe { core::ptr::write_volatile(a as *mut u32, x) };
        }
        cortex_m::asm::dsb();
        wait(DWT::cycle_count(), ms_cycles(WORD_TIMEOUT_MS, self.cpu_hz))
    }
}

/// Bank 2 as the boot left it.
pub struct Bank2Boot {
    pub status: Bank2Status,
    /// Flash's verdict, when the boot may install the staged file over it;
    /// `None`: unsealed, or bank 2 marked and not to be installed this boot.
    pub install_over: Option<Verdict>,
}

/// The boot's bank-2 step, before anything reads bank 2: BKP14R's mark
/// first (`boot_action`). A mark erases S0 (erase clears its ECC), then
/// clears the mark, and bank 2 is `Empty` by construction: nothing of it is
/// read that boot, so a word that faults even after its erase can't loop
/// the boot. A failed erase keeps the mark and leaves bank 2 closed. No
/// mark: the judge; on `Good`, region 2 `Open`, the door cell opened and
/// the door checked; SR2's flags cleared after, so a stale DBECCERR can't
/// make a later, unrelated bus fault reset. The cycle counter must already
/// run: it bounds the erase.
#[cfg(feature = "bank2")]
pub fn boot_judge(rtc: &pac::RTC, cpu_hz: u32) -> Bank2Boot {
    use chimera_image::{BootAction, boot_action};

    let closed = |status| Bank2Boot {
        status,
        install_over: None,
    };
    let action = boot_action(crate::dfu::bank2_mark(rtc));
    if action != BootAction::Judge {
        let erased = FlashBank2::new(cpu_hz).erase(Bank2Sector::S0);
        set_mpu(Mpu2::Read);
        if let Err(f) = erased {
            return closed(Bank2Status::EccEraseFailed(f));
        }
        crate::dfu::set_bank2_mark(rtc, 0);
        if crate::bank2_seal().is_none() {
            return closed(Bank2Status::Unsealed);
        }
        return Bank2Boot {
            status: Bank2Status::Empty,
            install_over: action.after_erase(),
        };
    }
    let Some(seal) = crate::bank2_seal() else {
        return closed(Bank2Status::Unsealed);
    };
    let (status, flash) = open_door(&seal);
    clear_flags();
    Bank2Boot {
        status,
        install_over: Some(flash),
    }
}

/// A build without bank 2's section has no seal: it never reads bank 2.
#[cfg(not(feature = "bank2"))]
pub fn boot_judge(_: &pac::RTC, _: u32) -> Bank2Boot {
    Bank2Boot {
        status: Bank2Status::Unsealed,
        install_over: None,
    }
}

/// The HardFault handler's first step: a double ECC error read from S0-S3
/// (a bus fault, SR2.DBECCERR, FA2R in S0-S3) marks BKP14R and resets.
/// Anything else returns, to the handler's halt. Register reads only, no
/// HAL object: safe in a fault handler.
#[cfg(all(feature = "bank2", not(feature = "sd-probe")))]
pub fn reset_on_ecc_fault() {
    // SAFETY: SCB_CFSR, read only; its bits are sticky until written.
    let cfsr = unsafe { (*cortex_m::peripheral::SCB::PTR).cfsr.read() };
    if chimera_image::bank2_ecc_fault(cfsr, sr(), bank2().far.read().bits()) {
        crate::dfu::mark_fault_and_reset();
    }
}

/// Bank 2 judged against `seal`, this build's, and flash's verdict. Region
/// 2 `Read` and the door cell closed first, so every path but open ends
/// there (the console's `flash` still reads S0). On `Good`: region 2
/// `Open`, the cell opened and the door checked once: `probe(1) == 43`,
/// the table's first and last bytes. The boot's and an install's one way
/// in.
#[cfg(feature = "bank2")]
fn open_door(seal: &chimera_image::Seal) -> (Bank2Status, Verdict) {
    use chimera_core::bank2::{DOOR_MAGIC, PROBE_ADDS, TABLE_LEN};
    use chimera_image::Door;

    DOOR_CELL.close();
    set_mpu(Mpu2::Read);
    clear_flags();
    let door = match Door::open(&mut FlashRead, seal) {
        Ok(d) => d,
        Err(v) => return (Bank2Status::from_verdict(&v), v),
    };
    let flash = Verdict::Good(*door.header());
    set_mpu(Mpu2::Open);
    // SAFETY: `door` was opened with `bank2_seal()`, this build's seal.
    let ok = unsafe { DOOR_CELL.open(&door) }
        && DOOR_CELL.with(|d| {
            let last = TABLE_LEN - 1;
            d.magic == DOOR_MAGIC
                && (d.probe)(1) == 1 + PROBE_ADDS
                && d.table[0] == 0xB2
                && d.table[last] == last as u8 ^ 0xB2
        }) == Some(true)
        && board_door(|b| b.magic == BOARD_MAGIC) == Some(true)
        && chimera_core::bank2::ui_door_ok();
    if !ok {
        DOOR_CELL.close();
        set_mpu(Mpu2::Read);
        return (Bank2Status::Corrupt, flash);
    }
    let status = Bank2Status::Open {
        version: *door.version(),
        seal: door.seal().bytes(),
    };
    (status, flash)
}

/// The boot's install, once the card is mounted and before the project
/// loads: only if `flash` could take a staged file, `find_staged` picks one
/// (SYSTEM's, else a `.BK2` dropped in the root) judged `Good` for this
/// build's seal. Then BKP14R `BANK2_INSTALLING`, DON'T POWER OFF,
/// `install`, the door, BKP14R cleared. A fault meanwhile keeps the mark,
/// so the next boot installs nothing. No audio yet and no watchdog (it
/// arms in step 3): nothing to feed. A failure leaves the file for the
/// next boot; so does success (nothing renames it), and a boot with flash
/// `Good` never reads it. `None`: nothing written; else bank 2 as the door
/// then judged it.
#[cfg(feature = "bank2")]
#[inline(never)]
pub fn boot_install(
    store: &mut impl chimera_hal::store::Store,
    display: &mut impl chimera_hal::ChimeraDisplay,
    marker: &crate::dfu::Marker,
    flash: &Verdict,
    cpu_hz: u32,
) -> Option<Bank2Status> {
    let seal = crate::bank2_seal()?;
    let (mut staged, h) = chimera_core::console::find_staged(store, flash, &seal)?;
    marker.bank2_mark(chimera_image::BANK2_INSTALLING);
    let (_, now) = write_staged(&mut staged, display, &h, &seal, cpu_hz);
    marker.bank2_mark(0);
    Some(now)
}

/// `h`, judged `Good` in `staged`, written into bank 2 behind DON'T POWER
/// OFF, then the door: the install's result and bank 2 as the door then
/// judged it. The boot's install and the console's, one copy.
#[cfg(feature = "bank2")]
#[inline(never)]
fn write_staged(
    staged: &mut impl ImageRead,
    display: &mut impl chimera_hal::ChimeraDisplay,
    h: &chimera_image::Header,
    seal: &chimera_image::Seal,
    cpu_hz: u32,
) -> (
    Result<chimera_image::Header, chimera_image::InstallFail>,
    Bank2Status,
) {
    let mut flash = FlashBank2::new(cpu_hz);
    chimera_core::ui::installing::draw(display, h.version.as_str());
    let mut buf = [0u8; 512];
    let mut shown = Shown {
        d: display,
        at: chimera_image::Phase::Erasing,
        #[cfg(feature = "bank2-gate")]
        cpu_hz,
    };
    let r = chimera_image::install_with(&mut flash, staged, seal, &mut buf, &mut shown);
    let now = open_door(seal).0;
    // As the boot's judge: no stale DBECCERR left to read a later bus fault
    // as bank 2's.
    clear_flags();
    (r, now)
}

/// The install's phase under DON'T POWER OFF; a gate build holds at each
/// mark for a hand to pull the power.
#[cfg(feature = "bank2")]
struct Shown<'a, D> {
    d: &'a mut D,
    at: chimera_image::Phase,
    #[cfg(feature = "bank2-gate")]
    cpu_hz: u32,
}

#[cfg(feature = "bank2")]
impl<D: chimera_hal::ChimeraDisplay> chimera_image::Progress for Shown<'_, D> {
    fn phase(&mut self, p: chimera_image::Phase) {
        self.at = p;
        chimera_core::ui::installing::phase(self.d, p);
    }

    fn mark(&mut self, _: chimera_image::Mark) {
        #[cfg(feature = "bank2-gate")]
        gate::hold(self.d, self.at, self.cpu_hz);
    }
}

/// The power-pull aid (`bank2-gate`): never in a shipping build; `just
/// release-bin` refuses a .bin holding `BANK2_GATE` or `PULL NOW`.
#[cfg(feature = "bank2-gate")]
mod gate {
    use chimera_core::ui::installing;
    use chimera_image::Phase;
    use chimera_image::regs::ms_cycles;
    use cortex_m::peripheral::DWT;

    /// The build's marker, which release-bin greps for.
    #[used]
    static BANK2_GATE: [u8; 16] = *b"BANK2-GATE-BUILD";

    /// `PULL NOW 3`, `2`, `1`, a second each, then the phase again. A busy
    /// wait on the cycle counter, interrupts on: the audio plays and the
    /// tick runs. Without a counting DWT, no hold.
    pub fn hold(d: &mut impl chimera_hal::ChimeraDisplay, p: Phase, cpu_hz: u32) {
        core::hint::black_box(&BANK2_GATE);
        if !crate::clocks::counting() {
            return;
        }
        let second = ms_cycles(1000, cpu_hz);
        for s in (1..=3).rev() {
            installing::hold(d, p, s);
            let t = DWT::cycle_count();
            while DWT::cycle_count().wrapping_sub(t) < second {}
        }
        installing::phase(d, p);
    }
}

/// TAPE's voices off before the door closes, waiting for the audio's say
/// (three blocks: the kill, the fade, the report) for `TAPE_OFF_MS`. Timed
/// out, the proxy is silent anyway. Only `bank2 erase` closes a live door.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
fn close_tape(ui: &mut chimera_core::ui::UiState) {
    use crate::controls;
    const TICKS: u32 = TAPE_OFF_MS * controls::CONTROLS_HZ / 1000;
    let t0 = controls::ticks();
    let within = || controls::ticks().wrapping_sub(t0) < TICKS;
    let _ = ui.close_bank2(&chimera_core::project::LOAD_LINK, within);
    // No stream wants a fill now: the burst in flight lands and the card
    // interrupt gives the bus back before its work (bank 2's) goes.
    // Its own `TAPE_OFF_MS`: the audio's wait above may have spent the
    // first.
    #[cfg(not(feature = "sd-probe"))]
    {
        let t1 = controls::ticks();
        while crate::sd::BUS.owner() == chimera_hal::card_bus::Owner::Stream
            && controls::ticks().wrapping_sub(t1) < TICKS
        {
            core::hint::spin_loop();
        }
    }
}

#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
const TAPE_OFF_MS: u32 = 30;

/// `bank2 erase` (`bank2-gate`), from the UI loop before its answer
/// starts: TAPE lets go, the door closes, region 2 `Write`, S0 erased
/// through `FlashBank2`, region 2 `Read`. Erased, bank 2 is `Empty` by
/// construction; a failed erase is judged again.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
#[inline(never)]
pub fn run_erase(
    ui: &mut chimera_core::ui::UiState,
    cpu_hz: u32,
) -> chimera_core::console::Bank2Answer {
    close_tape(ui);
    let r = FlashBank2::new(cpu_hz).erase(Bank2Sector::S0);
    set_mpu(Mpu2::Read);
    let now = match (r, crate::bank2_seal()) {
        (Ok(()), _) => Bank2Status::Empty,
        (Err(_), Some(seal)) => open_door(&seal).0,
        (Err(_), None) => Bank2Status::Unsealed,
    };
    chimera_core::project::LOAD_LINK.open_tape();
    ui.set_bank2(now);
    chimera_core::console::Bank2Answer::Erased(r)
}

/// `bank2 install`, from the UI loop before its answer starts. The staged
/// file is judged first; unless that refuses it (an open bank 2 always
/// refuses: the door is closed here, TAPE silent), region 2 goes `Write`,
/// DON'T POWER OFF goes up and the file is
/// written while the audio plays from bank 1. Then `open_door` judges and
/// checks bank 2 again: open, or region 2 `Read` and the cell closed. The
/// caller redraws the whole screen after.
#[cfg(all(feature = "bank2", feature = "usb-console"))]
#[inline(never)]
pub fn run_install(
    store: &mut impl chimera_hal::store::Store,
    ui: &mut chimera_core::ui::UiState,
    display: &mut impl chimera_hal::ChimeraDisplay,
    cpu_hz: u32,
) -> chimera_core::console::Bank2Answer {
    use chimera_core::console::{
        Bank2Answer, Bank2Audio, Bank2Refusal, CardImage, check_staged, installed,
    };

    let refuse = Bank2Answer::Refused;
    let Some(seal) = crate::bank2_seal() else {
        return refuse(Bank2Refusal::Unsealed);
    };
    if ui.card_pending() {
        return refuse(Bank2Refusal::Busy);
    }
    let vol = match store.mount() {
        Ok(v) => v,
        Err(e) => return refuse(Bank2Refusal::Card(e)),
    };
    let mut staged = CardImage::new(store, vol);
    let verdict = chimera_image::judge(&mut staged, &seal);
    let open = ui.bank2().is_some_and(|b| b.is_open());
    let h = match check_staged(open, &verdict, staged.error()) {
        Ok(h) => h,
        Err(a) => return a,
    };
    let before = audio();
    // `check_staged` refuses a write while open (`Good` there is this seal,
    // so `Already`): TAPE has nothing sounding to let go.
    debug_assert!(!DOOR_CELL.is_open());
    let (r, now) = write_staged(&mut staged, display, &h, &seal, cpu_hz);
    ui.set_bank2(now);
    let after = audio();
    let audio = Bank2Audio {
        overruns: [before[0], after[0]],
        blocks: [before[1], after[1]],
    };
    installed(r, &now, audio)
}

/// The audio's overruns and blocks rendered.
#[cfg(all(feature = "bank2", feature = "usb-console"))]
fn audio() -> [u32; 2] {
    use crate::shared::{BLOCKS, OVERRUNS};
    use core::sync::atomic::Ordering::Relaxed;
    [OVERRUNS.load(Relaxed), BLOCKS.load(Relaxed)]
}
