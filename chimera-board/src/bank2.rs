//! Bank 2 from bank 1 (ADR 0137): the MPU's view of it, its live reads and
//! the boot's judgement. Only S0-S3 (0x08100000..0x0817FFFF) are ever
//! mapped; MPU region 1 keeps sectors 4-7, the stock bootloader's slots,
//! no-access for good. Phase 1 writes nothing.

use core::sync::atomic::{AtomicU8, Ordering};

use chimera_core::bank2::Bank2Status;
use chimera_image::{BANK2_BASE, BANK2_LEN, Bank2Memory, ImageRead, ReadFail};
use stm32h7xx_hal::pac;

use crate::cache::{BANK2_OPEN_RASR, BANK2_READ_RASR, BANK2_REGION, BANK2_WRITE_RASR, RBAR_VALID};

const _: () = assert!(BANK2_BASE == crate::cache::BANK2_BASE);

/// FLASH_SR2.DBECCERR: a read hit a double ECC error.
const SR_DBECCERR: u32 = 1 << 26;
/// FLASH_CCR2's clear bits for SR2's EOP (16) and error flags (17-19,
/// 21-26) (RM0433 FLASH_CCR).
const CCR_CLEAR: u32 = 1 << 16 | 0b111 << 17 | 0x3f << 21;
const _: () = assert!(CCR_CLEAR == 0x07EF_0000);

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

/// Region 2 to `s`. From the boot or the UI loop only. Into `Open`, the
/// whole D-cache is cleaned and invalidated (set/way) and the whole
/// I-cache invalidated first, so nothing older than the flash is read or
/// run; no cache operation ever names a bank-2 address.
pub fn set_mpu(s: Mpu2) {
    // SAFETY: whole-cache maintenance and the MPU's RBAR/RASR pair. Only the
    // boot and the UI loop call this, never an interrupt; after
    // `cache::init`, `Board` never uses its MPU, and its SCB only for
    // SysTick's priority and to reset, neither of which this touches.
    let mut cp = unsafe { cortex_m::Peripherals::steal() };
    if s == Mpu2::Open {
        cp.SCB.clean_invalidate_dcache(&mut cp.CPUID);
        cp.SCB.invalidate_icache();
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
}

fn bank2() -> &'static pac::flash::BANK {
    // SAFETY: FLASH's fixed register block; here SR2 is read and CCR2
    // written, bank 2's own, nothing else.
    unsafe { (*pac::FLASH::ptr()).bank2() }
}

/// SR2's flags cleared, so an error a read raises is this read's.
#[cfg_attr(not(feature = "bank2"), allow(dead_code))]
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

/// Bank 2 judged against this build's seal; on `Good`, region 2 `Open`,
/// the door cell opened and the door checked once: `probe(1) == 43`, the
/// table's first and last bytes. Anything less leaves region 2 `Read`
/// (the console's `flash` still reads S0) and the cell closed.
#[cfg(feature = "bank2")]
pub fn boot_judge() -> Bank2Status {
    use chimera_core::bank2::{DOOR_CELL, DOOR_MAGIC, PROBE_ADDS, TABLE_LEN};
    use chimera_image::Door;

    let Some(seal) = crate::bank2_seal() else {
        return Bank2Status::Unsealed;
    };
    clear_flags();
    let door = match Door::open(&mut FlashRead, &seal) {
        Ok(d) => d,
        Err(v) => return Bank2Status::from_verdict(&v),
    };
    set_mpu(Mpu2::Open);
    // SAFETY: `door` was opened with `bank2_seal()`, this build's seal.
    let ok = unsafe { DOOR_CELL.open(&door) }
        && DOOR_CELL.with(|d| {
            let last = TABLE_LEN - 1;
            d.magic == DOOR_MAGIC
                && (d.probe)(1) == 1 + PROBE_ADDS
                && d.table[0] == 0xB2
                && d.table[last] == last as u8 ^ 0xB2
        }) == Some(true);
    if !ok {
        DOOR_CELL.close();
        set_mpu(Mpu2::Read);
        return Bank2Status::Corrupt;
    }
    Bank2Status::Open {
        version: *door.version(),
        seal: door.seal().bytes(),
    }
}

/// A build without bank 2's section has no seal: it never opens it.
#[cfg(not(feature = "bank2"))]
pub fn boot_judge() -> Bank2Status {
    Bank2Status::Unsealed
}
