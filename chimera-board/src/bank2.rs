//! Bank 2 from bank 1 (ADR 0137): the MPU's view of it, its live reads,
//! its flash controller and the boot's judgement. Region 2 maps all of it,
//! S0–S7 (0x08100000..0x081FFFFF): the image, with the stock bootloader's
//! two slots in S4–S7 holding our stubs. Nothing is written there until the
//! guard has read each slot as ours, by masked one-word reads. Only bank 2's
//! own key, control, status and clear registers are named; their values are
//! `chimera_image::regs`'.

use core::sync::atomic::{AtomicU8, Ordering};

use chimera_core::bank2::{Bank2Status, DOOR_CELL};
use chimera_image::regs::{
    CCR_CLEAR, CR_LOCK, CR_START, ERASE_TIMEOUT_MS, KEYS, SR_DBECCERR, SR_SNECCERR,
    WORD_TIMEOUT_MS, busy, erase_cr, ms_cycles, program_cr, relock_cr, settle,
};
use chimera_image::slot::{SlotForeign, SlotPlan, SlotRead, Word};
use chimera_image::{
    BANK2_BASE, BANK2_INSTALLING, BANK2_LEN, Bank2Memory, Bank2Sector, Bank2Word, Flash, FlashFail,
    ImageRead, ReadFail, Slot, SlotSet, Verdict,
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
    /// USB MIDI's class: its allocation and the device's poll.
    #[cfg(feature = "midi-usb")]
    pub(crate) usb: crate::usb_midi::UsbDoor,
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
    #[cfg(feature = "midi-usb")]
    usb: crate::usb_midi::UsbDoor::DOOR,
};

/// `BOARD_DOOR`'s slot.
#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `BOARD_DOOR`, a `BoardDoor`, at `BOARD_DOOR_ADDR`
// (its assert, `__bank2_board_door`).
const BOARD_SLOT: chimera_core::bank2::DoorSlot<BoardDoor> =
    unsafe { chimera_core::bank2::DoorSlot::new(chimera_core::bank2::BOARD_DOOR_ADDR) };

/// A slot's stub: its code word, then its tag word (ADR 0272).
#[cfg(feature = "bank2")]
const fn stub(s: chimera_image::Slot) -> [u8; 64] {
    let (code, tag) = (
        chimera_image::slot::code_word(s),
        chimera_image::slot::tag_word(s),
    );
    let mut w = [0u8; 64];
    let mut i = 0;
    while i < 32 {
        w[i] = code[i];
        w[32 + i] = tag[i];
        i += 1;
    }
    w
}

// The slots' stubs and end markers, data in every image: `bank2.x` puts
// them at 0x08180000, 0x081BFFE0, 0x081C0000 and 0x081FFFE0, and
// `just check-slots` reads them back from the .bk2.
#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.slot.mixer")]
#[used]
static SLOT_MIXER: [u8; 64] = stub(chimera_image::Slot::Mixer);

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.slot.mixer_end")]
#[used]
static SLOT_MIXER_END: [u8; 32] = chimera_image::slot::end_marker(chimera_image::Slot::Mixer);

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.slot.edit")]
#[used]
static SLOT_EDIT: [u8; 64] = stub(chimera_image::Slot::Edit);

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.slot.edit_end")]
#[used]
static SLOT_EDIT_END: [u8; 32] = chimera_image::slot::end_marker(chimera_image::Slot::Edit);

/// `f` on the board's table, `None` closed (see `DoorCell::with`).
#[cfg(all(feature = "bank2", not(feature = "sd-probe")))]
#[inline(always)]
pub(crate) fn board_door<R>(f: impl FnOnce(&BoardDoor) -> R) -> Option<R> {
    DOOR_CELL.with_slot(BOARD_SLOT, f)
}

/// MPU region 2's state over S0–S7; region 1 (no access) is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mpu2 {
    /// Region 2 off: bank 2 no-access, region 1's floor.
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

    /// A load from bank 2 does not fault (an MPU fault, not ECC's).
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
/// in SR2 after them is an `Err`. Bounded to bank 2, and only while region
/// 2 maps it.
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
            // SAFETY: `a` is inside bank 2 (`end <= BANK2_LEN`), which
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
        // SAFETY: a CR2 word from `erase_cr` (one of S0–S7) or `program_cr`,
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

/// Bank 2's flash controller over S0–S7, built only by `new`: the door
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
    fn held(w: Bank2Word) -> [u32; 8] {
        let mut got = [0; 8];
        for (a, x) in (w.get()..).step_by(4).zip(got.iter_mut()) {
            // SAFETY: an aligned word inside bank 2 (`Bank2Word`), region 2
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
// `Bank2Sector` (S0–S7) through `erase_cr`, a program a `Bank2Word`
// (inside bank 2) under `program_cr`; neither can start a bank erase. Each runs
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
        if Self::held(w) != [u32::MAX; 8] {
            return Err(FlashFail::NotErased);
        }
        let u = Unlocked::new()?;
        clear_flags();
        u.set_cr(program_cr(cr()));
        let (words, _) = data.as_chunks::<4>();
        for (a, &b) in (w.get()..).step_by(4).zip(words) {
            let x = u32::from_le_bytes(b);
            // SAFETY: an aligned word of a flash word in bank 2
            // (`Bank2Word`), region 2 `Write`, CR2.PG set: the store fills
            // bank 2's write buffer, eight make one 256-bit word.
            unsafe { core::ptr::write_volatile(a as *mut u32, x) };
        }
        cortex_m::asm::dsb();
        wait(DWT::cycle_count(), ms_cycles(WORD_TIMEOUT_MS, self.cpu_hz))
    }
}

/// SCB_CCR.BFHFNMIGN: at FAULTMASK's priority, a precise data bus fault is
/// ignored and its load returns.
const CCR_BFHFNMIGN: u32 = 1 << 8;

/// SR2's ECC flags, read and then cleared: (double, single corrected).
fn clear_ecc() -> (bool, bool) {
    let s = sr();
    // SAFETY: CCR2's two ECC clear bits only.
    bank2()
        .ccr
        .write(|w| unsafe { w.bits(SR_DBECCERR | SR_SNECCERR) });
    (s & SR_DBECCERR != 0, s & SR_SNECCERR != 0)
}

/// The guard's reads (spec § 3.2): one 32-byte word, masked. Region 2 must
/// be `Write` (strongly-ordered, uncached: the load reaches the flash, so
/// its flags are its own). FAULTMASK, then CCR.BFHFNMIGN, the eight loads,
/// BFHFNMIGN cleared, `cpsie f`; only then SR2's ECC flags are read and
/// cleared, so a fault the mask did not swallow (pended, not precise) still
/// finds DBECCERR in the HardFault handler, which resets on the caller's
/// slot mark. The mask spans this one word, never the scan.
impl SlotRead for FlashBank2 {
    fn word(&mut self, addr: u32) -> Word {
        let w = Bank2Word::new(addr).expect("a guard read inside bank 2");
        assert_eq!(mpu(), Mpu2::Write);
        clear_flags();
        // SAFETY: SCB's fixed block; only its CCR is touched, here, from
        // the boot or the UI loop, never an interrupt.
        let scb = unsafe { &*cortex_m::peripheral::SCB::PTR };
        let mut got = [0u32; 8];
        // SAFETY: FAULTMASK and CCR.BFHFNMIGN are set for eight loads of
        // one aligned word inside bank 2 (`Bank2Word`), which region 2 maps
        // `Write`, and cleared again in this block: no other code runs
        // masked. A load never programs or erases. Under them a precise bus
        // fault (a double ECC error) is ignored and the load returns.
        unsafe {
            core::arch::asm!("cpsid f", options(nostack, preserves_flags));
            scb.ccr.modify(|c| c | CCR_BFHFNMIGN);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            for (a, x) in (w.get()..).step_by(4).zip(got.iter_mut()) {
                *x = core::ptr::read_volatile(a as *const u32);
            }
            cortex_m::asm::dsb();
            scb.ccr.modify(|c| c & !CCR_BFHFNMIGN);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            core::arch::asm!("cpsie f", options(nostack, preserves_flags));
            // CPS isn't self-synchronising: a pended fault is taken here,
            // while SR2 still holds its DBECCERR.
            cortex_m::asm::isb();
        }
        let (ecc, corrected) = clear_ecc();
        let mut bytes = [0u8; 32];
        for (b, x) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(got) {
            *b = x.to_le_bytes();
        }
        Word {
            bytes,
            ecc,
            corrected,
        }
    }
}

/// `flash`'s masked reads with BKP14R `guard_marks(ours, s)` written before
/// slot `s`'s first: a fault the mask misses resets into a boot that takes
/// `s` as ours, unread.
struct Marked<'a> {
    rtc: &'a pac::rtc::RegisterBlock,
    flash: &'a mut FlashBank2,
    ours: SlotSet,
    reading: Option<Slot>,
}

impl SlotRead for Marked<'_> {
    fn word(&mut self, addr: u32) -> Word {
        let s = Slot::ALL
            .into_iter()
            .find(|s| (s.base()..=s.marker_addr()).contains(&addr))
            .expect("a guard read inside a slot");
        if self.reading != Some(s) {
            self.reading = Some(s);
            crate::dfu::set_bank2_mark(self.rtc, chimera_image::guard_marks(self.ours, s));
            // The mark lands before the read it covers.
            cortex_m::asm::dsb();
        }
        self.flash.word(addr)
    }
}

/// The slot guard before an install (spec § 3.2), `ours` taken unread (the
/// boot's slot mark). BKP14R: each slot's mark before its reads, then
/// `BANK2_INSTALLING` back for the install. Refused: BKP14R 0 and region 2
/// `Read` (`after_refusal`), so nothing can be written.
#[cfg(feature = "bank2")]
#[cfg_attr(feature = "sd-probe", allow(dead_code))]
pub(crate) fn slot_guard(
    rtc: &pac::rtc::RegisterBlock,
    flash: &mut FlashBank2,
    ours: SlotSet,
) -> Result<SlotPlan, SlotForeign> {
    let mut m = Marked {
        rtc,
        flash,
        ours,
        reading: None,
    };
    let r = chimera_image::slot::guard(&mut m, ours);
    match r {
        Ok(_) => crate::dfu::set_bank2_mark(rtc, BANK2_INSTALLING),
        Err(_) => {
            crate::dfu::set_bank2_mark(rtc, 0);
            after_refusal();
        }
    }
    r
}

/// After the guard refused: region 2 `Read`, so `FlashBank2` refuses every
/// erase and program (`writable`).
#[cfg(feature = "bank2")]
#[cfg_attr(feature = "sd-probe", allow(dead_code))]
pub(crate) fn after_refusal() -> Mpu2 {
    set_mpu(Mpu2::Read);
    mpu()
}

/// Bank 2 as the boot left it.
pub struct Bank2Boot {
    pub status: Bank2Status,
    /// Flash's verdict, when the boot may install the staged file over it;
    /// `None`: unsealed, or bank 2 marked and not to be installed this boot.
    pub install_over: Option<Verdict>,
    /// The slots a slot mark said a guard read faulted in: the install's
    /// guard takes them as ours, unread.
    pub ours: SlotSet,
}

/// The boot's bank-2 step, before anything reads bank 2: BKP14R's mark
/// first (`boot_action`). `BANK2_BAD` or `BANK2_INSTALLING` erases S0
/// (erase clears its ECC), then clears the mark, and bank 2 is `Empty` by
/// construction: nothing of it is read that boot, so a word that faults
/// even after its erase can't loop the boot. A failed erase keeps the mark
/// and leaves bank 2 closed. A slot mark is taken into `ours` and BKP14R
/// cleared before the judge: a judge fault then marks `BANK2_BAD` (the next
/// boot erases S0) instead of repeating this boot, and a boot that installs
/// nothing leaves no stale mark to let a later install skip a refilled
/// slot's guard. Then, as with no mark, the judge; on `Good`, region 2 `Open`, the door cell opened and
/// the door checked; SR2's flags cleared after, so a stale DBECCERR can't
/// make a later, unrelated bus fault reset. The cycle counter must already
/// run: it bounds the erase.
#[cfg(feature = "bank2")]
pub fn boot_judge(rtc: &pac::RTC, cpu_hz: u32) -> Bank2Boot {
    use chimera_image::{BootAction, boot_action};

    let closed = |status| Bank2Boot {
        status,
        install_over: None,
        ours: SlotSet::NONE,
    };
    let action = boot_action(crate::dfu::bank2_mark(rtc));
    let ours = match action {
        BootAction::Judge => SlotSet::NONE,
        BootAction::InstallSlotsOurs(set) => {
            crate::dfu::set_bank2_mark(rtc, 0);
            // Cleared before the judge's first read.
            cortex_m::asm::dsb();
            set
        }
        BootAction::EraseS0ThenJudge | BootAction::EraseS0NoInstall => {
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
                ours: SlotSet::NONE,
            };
        }
    };
    let Some(seal) = crate::bank2_seal() else {
        return closed(Bank2Status::Unsealed);
    };
    let (status, flash) = open_door(&seal);
    clear_flags();
    Bank2Boot {
        status,
        install_over: Some(flash),
        ours,
    }
}

/// A build without bank 2's section has no seal: it never reads bank 2.
#[cfg(not(feature = "bank2"))]
pub fn boot_judge(_: &pac::RTC, _: u32) -> Bank2Boot {
    Bank2Boot {
        status: Bank2Status::Unsealed,
        install_over: None,
        ours: SlotSet::NONE,
    }
}

/// The HardFault handler's first step: a double ECC error read from bank 2
/// (a bus fault, SR2.DBECCERR, FA2R in S0–S7) marks BKP14R and resets.
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
/// `Open`, the door checked once where it lies (`probe(1) == 43`, the
/// table's first and last bytes, the board's and the UI's magics), then
/// the cell opened. The boot's and an install's one way
/// in.
#[cfg(feature = "bank2")]
fn open_door(seal: &chimera_image::Seal) -> (Bank2Status, Verdict) {
    use chimera_core::bank2::{DOOR_MAGIC, Judged, PROBE_ADDS, TABLE_LEN};
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
    let judged = unsafe { Judged::of(&door) };
    // Checked where it lies, then published: the audio and card
    // interrupts, live at an install, call through it only once it passed.
    let checked = judged.and_then(|j| {
        j.check(|j| {
            j.with(|d| {
                let last = TABLE_LEN - 1;
                d.magic == DOOR_MAGIC
                    && d.probe(1) == 1 + PROBE_ADDS
                    && d.table()[0] == 0xB2
                    && d.table()[last] == last as u8 ^ 0xB2
            }) && j.with_slot(BOARD_SLOT, |b| b.magic == BOARD_MAGIC)
                && chimera_core::bank2::ui_door_ok(j)
                && chimera_core::ui::orbit::door::door_ok(j)
                && chimera_core::ui::cc::door_ok(j)
                && chimera_core::bank2::ed_door_ok(j)
        })
    });
    let Some(checked) = checked else {
        set_mpu(Mpu2::Read);
        return (Bank2Status::Corrupt, flash);
    };
    DOOR_CELL.open(checked);
    let status = Bank2Status::Open {
        version: *door.version(),
        seal: door.seal().bytes(),
    };
    (status, flash)
}

/// The boot's install, once the card is mounted and before the project
/// loads: only if `flash` could take a staged file, `find_staged` picks one
/// (SYSTEM's, else a `.BK2` dropped in the root) judged `Good` for this
/// build's seal. Then BKP14R `BANK2_INSTALLING`, region 2 `Write`, the slot
/// guard (`ours` unread; its slot marks; `BANK2_INSTALLING` again), DON'T
/// POWER OFF, `install`, the door, BKP14R cleared. A fault meanwhile keeps
/// the mark, so the next boot installs nothing (or, from a guard read,
/// takes that slot as ours). Refused: NOT INSTALLED for 5 s or until a
/// button is pressed, BKP14R 0, region 2 `Read`, `SlotForeign`. No audio yet and no watchdog (it
/// arms in step 3): nothing to feed. A failure leaves the file for the
/// next boot; so does success (nothing renames it), and a boot with flash
/// `Good` never reads it. `None`: nothing written; else bank 2 as the door
/// then judged it.
// The SD probe halts after its probe: it installs nothing.
#[cfg(all(feature = "bank2", not(feature = "sd-probe")))]
#[inline(never)]
pub fn boot_install(
    store: &mut impl chimera_hal::store::Store,
    display: &mut impl chimera_hal::ChimeraDisplay,
    marker: &crate::dfu::Marker,
    flash: &Verdict,
    ours: SlotSet,
    buttons: chimera_core::board::Hc165Pins,
    cpu_hz: u32,
) -> Option<Bank2Status> {
    let seal = crate::bank2_seal()?;
    let (mut staged, h) = chimera_core::console::find_staged(store, flash, &seal)?;
    marker.bank2_mark(BANK2_INSTALLING);
    let mut bank2 = FlashBank2::new(cpu_hz);
    let plan = match slot_guard(marker.rtc(), &mut bank2, ours) {
        Ok(p) => p,
        Err(SlotForeign(s)) => {
            chimera_core::ui::installing::refused(display, s);
            hold_refusal(buttons, cpu_hz);
            return Some(Bank2Status::SlotForeign(s));
        }
    };
    let (_, now) = write_staged(&mut bank2, &mut staged, display, &h, &seal, &plan);
    marker.bank2_mark(0);
    Some(now)
}

/// How long the boot shows NOT INSTALLED, unless a button is pressed.
#[cfg(all(feature = "bank2", not(feature = "sd-probe")))]
const REFUSAL_MS: u32 = 5_000;

/// NOT INSTALLED held `REFUSAL_MS`, or until a button goes down after all
/// were up (a button still held from power-on doesn't skip it). Before the
/// controls tick: the buttons are read directly, every 10 ms.
#[cfg(all(feature = "bank2", not(feature = "sd-probe")))]
fn hold_refusal(buttons: chimera_core::board::Hc165Pins, cpu_hz: u32) {
    const STEP_MS: u32 = 10;
    let mut up = false;
    for _ in 0..REFUSAL_MS / STEP_MS {
        let down = crate::controls::any_button_down(buttons, cpu_hz);
        if up && down {
            return;
        }
        up |= !down;
        crate::clocks::delay_us(cpu_hz, STEP_MS * 1000);
    }
}

/// `h`, judged `Good` in `staged`, written into bank 2 by `plan` (the
/// guard's) behind DON'T POWER OFF, then the door: the install's result and
/// bank 2 as the door then judged it. The boot's install and the console's,
/// one copy.
#[cfg(feature = "bank2")]
#[cfg_attr(feature = "sd-probe", allow(dead_code))]
#[inline(never)]
fn write_staged(
    flash: &mut FlashBank2,
    staged: &mut impl ImageRead,
    display: &mut impl chimera_hal::ChimeraDisplay,
    h: &chimera_image::Header,
    seal: &chimera_image::Seal,
    plan: &SlotPlan,
) -> (
    Result<chimera_image::Header, chimera_image::InstallFail>,
    Bank2Status,
) {
    chimera_core::ui::installing::draw(display, h.version.as_str());
    let mut buf = [0u8; 512];
    #[cfg(not(feature = "bank2-gate"))]
    let r = {
        let mut shown = Shown {
            d: display,
            at: chimera_image::Phase::Erasing,
        };
        chimera_image::install_with(flash, staged, seal, &mut buf, &mut shown, plan)
    };
    #[cfg(feature = "bank2-gate")]
    let r = gate::install(flash, staged, display, seal, &mut buf, plan);
    let now = open_door(seal).0;
    // As the boot's judge: no stale DBECCERR left to read a later bus fault
    // as bank 2's.
    clear_flags();
    (r, now)
}

/// The install's phase under DON'T POWER OFF (a gate build's is
/// `gate::Shown`).
#[cfg(all(feature = "bank2", not(feature = "bank2-gate")))]
#[cfg_attr(feature = "sd-probe", allow(dead_code))]
struct Shown<'a, D> {
    d: &'a mut D,
    at: chimera_image::Phase,
}

#[cfg(all(feature = "bank2", not(feature = "bank2-gate")))]
impl<D: chimera_hal::ChimeraDisplay> chimera_image::Progress for Shown<'_, D> {
    fn phase(&mut self, p: chimera_image::Phase) {
        self.at = p;
        chimera_core::ui::installing::phase(self.d, p);
    }

    fn mark(&mut self, _: chimera_image::Mark) {}
}

/// The power-pull aid (`bank2-gate`): never in a shipping build; `just
/// release-bin` refuses a .bin holding `BANK2_GATE`, `SLOT_GATE` or
/// `PULL NOW`.
#[cfg(feature = "bank2-gate")]
mod gate {
    use core::cell::RefCell;
    use core::sync::atomic::{AtomicU8, Ordering};

    use chimera_core::console::PullAt;
    use chimera_core::ui::installing;
    use chimera_hal::ChimeraDisplay;
    use chimera_image::regs::ms_cycles;
    use chimera_image::slot::SlotPlan;
    use chimera_image::{
        Bank2Memory, Bank2Sector, Bank2Word, Flash, FlashFail, Header, ImageRead, InstallFail,
        Mark, Phase, Progress, ReadFail, Seal,
    };
    use cortex_m::peripheral::DWT;

    use super::FlashBank2;

    /// The build's marker, which release-bin greps for.
    #[used]
    static BANK2_GATE: [u8; 16] = *b"BANK2-GATE-BUILD";

    /// The slot commands' marker (`bank2 slot-poke`, `erase slots`,
    /// `ecc-read`, `pull`), which `check-no-gate` greps for.
    #[used]
    static SLOT_GATE: [u8; 15] = *b"SLOT-GATE-BUILD";

    /// `bank2 pull`'s point for the next install: 0 none, else
    /// `PullAt::ALL`'s index + 1. RAM only: the boot after a pull holds
    /// nowhere.
    static ARMED: AtomicU8 = AtomicU8::new(0);

    /// The next install holds at `p`.
    pub fn arm(p: PullAt) {
        core::hint::black_box(&SLOT_GATE);
        let i = PullAt::ALL.iter().position(|&a| a == p).unwrap_or(0);
        ARMED.store(i as u8 + 1, Ordering::Relaxed);
    }

    impl FlashBank2 {
        /// `data` programmed at `w` whatever the word holds: no erased
        /// check (`program`'s `held`). Gate 1b only: over a programmed word
        /// its whole point is a double ECC error there, which only an
        /// erase clears.
        pub(super) fn program_over(
            &mut self,
            w: Bank2Word,
            data: &[u8; 32],
        ) -> Result<(), FlashFail> {
            self.ready()?;
            let u = super::Unlocked::new()?;
            super::clear_flags();
            u.set_cr(chimera_image::regs::program_cr(super::cr()));
            let (words, _) = data.as_chunks::<4>();
            for (a, &b) in (w.get()..).step_by(4).zip(words) {
                // SAFETY: as `program`'s stores: an aligned word of a flash
                // word in bank 2 (`Bank2Word`), region 2 `Write`
                // (`ready`), CR2.PG set. The word may already be
                // programmed: corrupting its ECC is this function's
                // purpose, and it touches no other word.
                unsafe { core::ptr::write_volatile(a as *mut u32, u32::from_le_bytes(b)) };
            }
            cortex_m::asm::dsb();
            super::wait(
                DWT::cycle_count(),
                ms_cycles(chimera_image::regs::WORD_TIMEOUT_MS, self.cpu_hz),
            )
        }
    }

    /// The armed point, disarmed.
    fn take() -> Option<PullAt> {
        let i = ARMED.swap(0, Ordering::Relaxed);
        PullAt::ALL.get(usize::from(i).wrapping_sub(1)).copied()
    }

    /// `install_with` as the shipping build's, with the gate's holds: at
    /// each mark (`hold`), and at the armed `bank2 pull` point
    /// (`Pulling`). The screen is shared by the two.
    pub fn install<D: ChimeraDisplay>(
        flash: &mut FlashBank2,
        staged: &mut impl ImageRead,
        display: &mut D,
        seal: &Seal,
        buf: &mut [u8; 512],
        plan: &SlotPlan,
    ) -> Result<Header, InstallFail> {
        let cpu_hz = flash.cpu_hz;
        let d = RefCell::new(display);
        let mut pulling = Pulling {
            flash,
            d: &d,
            armed: take(),
            marker: false,
        };
        let mut shown = Shown {
            d: &d,
            at: Phase::Erasing,
            cpu_hz,
        };
        chimera_image::install_with(&mut pulling, staged, seal, buf, &mut shown, plan)
    }

    /// The phase under DON'T POWER OFF, held at each mark.
    struct Shown<'a, 'd, D> {
        d: &'a RefCell<&'d mut D>,
        at: Phase,
        cpu_hz: u32,
    }

    impl<D: ChimeraDisplay> Progress for Shown<'_, '_, D> {
        fn phase(&mut self, p: Phase) {
            self.at = p;
            installing::phase(&mut **self.d.borrow_mut(), p);
        }

        fn mark(&mut self, _: Mark) {
            hold(&mut **self.d.borrow_mut(), self.at, self.cpu_hz);
        }
    }

    /// `FlashBank2` with `bank2 pull`'s hold: the armed slot's entry
    /// sector, erased after its marker word, is re-erased back to back for
    /// 3 s under PULL NOW; or 3 s held after its tag word. A plan that
    /// erases the entry sector before the marker (a torn tag) or not at
    /// all (read erased) never holds there: no PULL NOW shows.
    struct Pulling<'a, 'd, D> {
        flash: &'a mut FlashBank2,
        d: &'a RefCell<&'d mut D>,
        armed: Option<PullAt>,
        /// The armed slot's marker word programmed this install.
        marker: bool,
    }

    impl<D: ChimeraDisplay> Pulling<'_, '_, D> {
        /// `s` erased again and again until 3 s have passed, PULL NOW
        /// counting down a second at a time; at least once, at most
        /// `TEAR_ERASES` times (the sector's endurance).
        fn tear_erase(&mut self, s: Bank2Sector) -> Result<(), FlashFail> {
            const TEAR_ERASES: u32 = 8;
            let second = ms_cycles(1000, self.flash.cpu_hz);
            let t = DWT::cycle_count();
            let mut shown = 0;
            for _ in 0..TEAR_ERASES {
                let gone = DWT::cycle_count().wrapping_sub(t) / second;
                let left = 3u32.saturating_sub(gone) as u8;
                if left == 0 {
                    break;
                }
                if left != shown {
                    installing::hold(&mut **self.d.borrow_mut(), Phase::Erasing, left);
                    shown = left;
                }
                self.flash.erase(s)?;
            }
            installing::phase(&mut **self.d.borrow_mut(), Phase::Erasing);
            Ok(())
        }
    }

    impl<D> ImageRead for Pulling<'_, '_, D> {
        fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
            self.flash.read(off, buf)
        }
    }

    // SAFETY: `read` is `FlashBank2`'s, unchanged.
    unsafe impl<D> Bank2Memory for Pulling<'_, '_, D> {}

    // SAFETY: every erase and program is `FlashBank2`'s, on the sector or
    // word named; the hold only repeats an erase of the same sector and
    // returns its last result.
    unsafe impl<D: ChimeraDisplay> Flash for Pulling<'_, '_, D> {
        fn erase(&mut self, s: Bank2Sector) -> Result<(), FlashFail> {
            match self.armed {
                Some(p) if p.erase() && self.marker && p.slot().sectors().0 == s => {
                    self.armed = None;
                    self.tear_erase(s)
                }
                _ => self.flash.erase(s),
            }
        }

        fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail> {
            self.flash.program(w, data)?;
            if let Some(p) = self.armed {
                let slot = p.slot();
                if w.get() == slot.marker_addr() {
                    self.marker = true;
                } else if !p.erase() && w.get() == slot.tag_addr() {
                    self.armed = None;
                    hold(
                        &mut **self.d.borrow_mut(),
                        Phase::Erasing,
                        self.flash.cpu_hz,
                    );
                }
            }
            Ok(())
        }
    }

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
    // Still streaming then: the abort asked while the door is open, so
    // bank 2's stop sends CMD12 and drains the card before the door
    // closes; `closed` grounds a burst with no stop only if that too ran
    // out (#761 m12).
    #[cfg(not(feature = "sd-probe"))]
    {
        use crate::sd::BUS;
        use chimera_hal::card_bus::Owner;
        let wait = || {
            let t1 = controls::ticks();
            while BUS.owner() == Owner::Stream && controls::ticks().wrapping_sub(t1) < TICKS {
                core::hint::spin_loop();
            }
        };
        wait();
        if BUS.owner() == Owner::Stream {
            BUS.ask_abort();
            crate::card_dma::pend();
            wait();
        }
    }
}

#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
const TAPE_OFF_MS: u32 = 30;

/// A gate command's flash work, from the UI loop before its answer starts:
/// TAPE lets go, the door closes, region 2 `Write`, `f`, region 2 `Read`.
/// Then bank 2 judged again (`open_door`) unless `judge` says no: the
/// door opens only on this seal's sound image. That judge, as
/// `slot-poke`'s erased check, reads unmasked: a torn word anywhere faults
/// and resets, and the boot's `BANK2_BAD` erase converges (gate only).
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
fn with_door_closed<R>(
    ui: &mut chimera_core::ui::UiState,
    cpu_hz: u32,
    f: impl FnOnce(&mut FlashBank2) -> R,
    judge: impl FnOnce(&R) -> Option<Bank2Status>,
) -> R {
    close_tape(ui);
    let r = f(&mut FlashBank2::new(cpu_hz));
    set_mpu(Mpu2::Read);
    let now = match (judge(&r), crate::bank2_seal()) {
        (Some(s), _) => s,
        (None, Some(seal)) => open_door(&seal).0,
        (None, None) => Bank2Status::Unsealed,
    };
    chimera_core::project::LOAD_LINK.open_tape();
    ui.set_bank2(now);
    r
}

/// `bank2 slot-poke` (`bank2-gate`, spec § 6.3 gate 8): `POKE_WORD`
/// programmed at `POKE_AT`, S5's middle, so the guard reads MIXER's slot
/// foreign. Refused `not erased` unless that word reads erased.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
#[inline(never)]
pub fn run_slot_poke(
    _: &chimera_core::errand::Errand,
    ui: &mut chimera_core::ui::UiState,
    cpu_hz: u32,
) -> chimera_core::console::Bank2Answer {
    use chimera_core::console::{POKE_AT, POKE_WORD};
    let r = with_door_closed(ui, cpu_hz, |f| f.program(POKE_AT, &POKE_WORD), |_| None);
    chimera_core::console::Bank2Answer::SlotPoked(r)
}

/// `bank2 ecc-tear` (`bank2-gate`, gate 1b): `TEAR_WORDS[0]` programmed at
/// `TEAR_AT` (`not erased` from an earlier run is passed over), then
/// `TEAR_WORDS[1]` over it with no erased check, for a double ECC error
/// there. Whether this chip makes one must be verified on it (`ecc-read`
/// at `TEAR_AT`). Bank 2 is not judged after: its unmasked read of that
/// word would fault; it stays closed as `Corrupt`. `bank2 erase slots`
/// clears it.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
#[inline(never)]
pub fn run_ecc_tear(
    _: &chimera_core::errand::Errand,
    ui: &mut chimera_core::ui::UiState,
    cpu_hz: u32,
) -> chimera_core::console::Bank2Answer {
    use chimera_core::console::{TEAR_AT, TEAR_WORDS};
    let r = with_door_closed(
        ui,
        cpu_hz,
        |f| match f.program(TEAR_AT, &TEAR_WORDS[0]) {
            Ok(()) | Err(FlashFail::NotErased) => f.program_over(TEAR_AT, &TEAR_WORDS[1]),
            Err(e) => Err(e),
        },
        |_| Some(Bank2Status::Corrupt),
    );
    chimera_core::console::Bank2Answer::EccTorn(r)
}

/// `bank2 erase slots` (`bank2-gate`, gate 8's clear): S4–S7 erased in
/// order, the first failure stops it.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
#[inline(never)]
pub fn run_erase_slots(
    _: &chimera_core::errand::Errand,
    ui: &mut chimera_core::ui::UiState,
    cpu_hz: u32,
) -> chimera_core::console::Bank2Answer {
    use Bank2Sector::{S4, S5, S6, S7};
    let r = with_door_closed(
        ui,
        cpu_hz,
        |f| {
            [S4, S5, S6, S7]
                .into_iter()
                .try_for_each(|s| f.erase(s).map_err(|e| (e, s)))
        },
        |_| None,
    );
    chimera_core::console::Bank2Answer::SlotsErased(r)
}

/// `bank2 ecc-read <addr>` (`bank2-gate`, gate 1b): the guard's masked
/// read of one word. With DBECCERR set, bank 2 stays closed as `Corrupt`:
/// the judge's unmasked read of that word would fault.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
#[inline(never)]
pub fn run_ecc_read(
    _: &chimera_core::errand::Errand,
    ui: &mut chimera_core::ui::UiState,
    at: Bank2Word,
    cpu_hz: u32,
) -> chimera_core::console::Bank2Answer {
    let w = with_door_closed(
        ui,
        cpu_hz,
        |f| f.word(at.get()),
        |w: &Word| w.ecc.then_some(Bank2Status::Corrupt),
    );
    let mut head = [0u8; 8];
    head.copy_from_slice(&w.bytes[..8]);
    chimera_core::console::Bank2Answer::Ecc { ecc: w.ecc, head }
}

/// `bank2 pull <point>` (`bank2-gate`, gate 6): the next install holds
/// there.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
pub fn run_pull(p: chimera_core::console::PullAt) -> chimera_core::console::Bank2Answer {
    gate::arm(p);
    chimera_core::console::Bank2Answer::Armed(p)
}

/// `bank2 erase` (`bank2-gate`), from the UI loop before its answer
/// starts: TAPE lets go, the door closes, region 2 `Write`, S0 erased
/// through `FlashBank2`, region 2 `Read`. Erased, bank 2 is `Empty` by
/// construction; a failed erase is judged again.
#[cfg(all(feature = "bank2-gate", feature = "usb-console"))]
#[inline(never)]
pub fn run_erase(
    _: &chimera_core::errand::Errand,
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
/// refuses: the door is closed here, TAPE silent), BKP14R
/// `BANK2_INSTALLING`, region 2 `Write` and the slot guard, as the boot's
/// (a slot that isn't ours refuses `SlotForeign`, nothing written, region 2
/// `Read`); then DON'T POWER OFF goes up and the file is written while the
/// audio plays from bank 1, BKP14R cleared after. Then `open_door` judges and
/// checks bank 2 again: open, or region 2 `Read` and the cell closed. The
/// caller redraws the whole screen after.
#[cfg(all(feature = "bank2", feature = "usb-console"))]
#[inline(never)]
pub fn run_install(
    _: &chimera_core::errand::Errand,
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
    // SAFETY: the RTC's fixed register block, BKP14R only: bank 2's mark.
    // Its other writers are the HardFault handler (which resets after) and
    // `Marker::bank2_mark`, called only by the boot's install, long before
    // the UI loop runs this; nothing writes it concurrently.
    let rtc = unsafe { &*pac::RTC::ptr() };
    crate::dfu::set_bank2_mark(rtc, BANK2_INSTALLING);
    let mut bank2 = FlashBank2::new(cpu_hz);
    let plan = match slot_guard(rtc, &mut bank2, SlotSet::NONE) {
        Ok(p) => p,
        Err(SlotForeign(s)) => {
            ui.set_bank2(Bank2Status::SlotForeign(s));
            return refuse(Bank2Refusal::SlotForeign(s));
        }
    };
    let (r, now) = write_staged(&mut bank2, &mut staged, display, &h, &seal, &plan);
    crate::dfu::set_bank2_mark(rtc, 0);
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
