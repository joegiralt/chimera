//! Chimera bootloader — minimal jump to firmware at 0x08020000.
//! Replaces the PreenFM3 bootloader to give firmware a clean peripheral state.

#![no_std]
#![no_main]

use core::ops::RangeInclusive;

use cortex_m_rt::entry;
use panic_halt as _;

/// Firmware vector table in flash (chimera-stm32/memory.x).
const FIRMWARE_ADDR: u32 = 0x0802_0000;
/// The firmware's FLASH region: 896K from `FIRMWARE_ADDR` (chimera-stm32/memory.x).
const FIRMWARE_CODE: RangeInclusive<u32> = FIRMWARE_ADDR..=FIRMWARE_ADDR + 896 * 1024 - 2;
/// An initial SP is a full-descending stack's top: the end of DTCM or of AXI
/// SRAM, or anywhere inside them (RM0433 memory map).
const DTCM: RangeInclusive<u32> = 0x2000_0000 + 8..=0x2002_0000;
const AXI_SRAM: RangeInclusive<u32> = 0x2400_0000 + 8..=0x2408_0000;

const SCB_VTOR: *mut u32 = 0xE000_ED08 as *mut u32;

/// A vector table worth jumping to: an 8-byte-aligned SP (AAPCS) in RAM and
/// a Thumb reset handler in the firmware's flash. Erased flash reads
/// 0xFFFF_FFFF for both, and fails.
const fn vectors_valid(sp: u32, reset: u32) -> bool {
    let sp_ok = sp.is_multiple_of(8)
        && ((sp >= *DTCM.start() && sp <= *DTCM.end())
            || (sp >= *AXI_SRAM.start() && sp <= *AXI_SRAM.end()));
    let pc = reset & !1;
    let reset_ok = reset & 1 == 1 && pc >= *FIRMWARE_CODE.start() && pc <= *FIRMWARE_CODE.end();
    sp_ok && reset_ok
}

// The first case is today's chimera.bin.
const _: () = {
    assert!(vectors_valid(0x2000_FF00, 0x0802_0299));
    assert!(vectors_valid(0x2408_0000, 0x080F_FFFF));
    assert!(!vectors_valid(0xFFFF_FFFF, 0xFFFF_FFFF));
    assert!(!vectors_valid(0x2001_0000, 0x0802_0298));
    assert!(!vectors_valid(0x2001_0004, 0x0802_0299));
    assert!(!vectors_valid(0x2001_0000, 0x0801_FFFF));
    assert!(!vectors_valid(0x2002_0008, 0x0802_0299));
};

#[entry]
fn main() -> ! {
    let table = FIRMWARE_ADDR as *const u32;
    // SAFETY: the first two words of the firmware's flash sector; flash is
    // always readable, erased or not.
    let (sp, reset) = unsafe { (table.read_volatile(), table.add(1).read_volatile()) };
    if !vectors_valid(sp, reset) {
        panic!("no firmware at 0x0802_0000");
    }
    // SAFETY: SCB->VTOR, the Cortex-M7's aligned vector-table register;
    // FIRMWARE_ADDR is 128-byte aligned as VTOR requires, and no exception
    // is enabled here to be taken through it early.
    unsafe { SCB_VTOR.write_volatile(FIRMWARE_ADDR) };
    // SAFETY: `vectors_valid` checked the SP lies in RAM and the reset
    // vector in the firmware's flash; `bootload` sets MSP and branches in
    // one asm sequence, so no Rust code runs on the new stack.
    unsafe { cortex_m::asm::bootload(table) }
}
