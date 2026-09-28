//! Chimera bootloader — minimal jump to firmware at 0x08020000, an
//! alternative to the stock PreenFM3 bootloader that the unit keeps today.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;

/// Firmware vector table in flash (chimera-stm32/memory.x).
const FIRMWARE_ADDR: u32 = 0x0802_0000;
/// Flash bank 1 ends here (RM0433 memory map); the firmware's FLASH region
/// runs up to it.
const FLASH_BANK1_END: u32 = 0x0810_0000;
/// DTCM and AXI SRAM (RM0433 memory map), as [base, end).
const DTCM: (u32, u32) = (0x2000_0000, 0x2002_0000);
const AXI_SRAM: (u32, u32) = (0x2400_0000, 0x2408_0000);

const SCB_VTOR: *mut u32 = 0xE000_ED08 as *mut u32;
// PE1 is the LED, as in chimera-stm32's panic handler (RM0433 register map).
const RCC_AHB4ENR: *mut u32 = 0x5802_44E0 as *mut u32;
const GPIOEEN: u32 = 1 << 4;
const GPIOE_MODER: *mut u32 = 0x5802_1000 as *mut u32;
const GPIOE_BSRR: *mut u32 = 0x5802_1018 as *mut u32;
const PE1: u32 = 1;

/// An initial SP for a full-descending stack: 8-byte aligned (AAPCS), above
/// a RAM region's base (the first push goes below it) and at most its end.
const fn sp_valid(sp: u32, (base, end): (u32, u32)) -> bool {
    sp.is_multiple_of(8) && sp > base && sp <= end
}

/// A vector table worth jumping to: an SP in RAM and a Thumb reset handler
/// in the firmware's flash. Erased flash reads 0xFFFF_FFFF for both, and
/// fails.
const fn vectors_valid(sp: u32, reset: u32) -> bool {
    let pc = reset & !1;
    (sp_valid(sp, DTCM) || sp_valid(sp, AXI_SRAM))
        && reset & 1 == 1
        && pc >= FIRMWARE_ADDR
        && pc < FLASH_BANK1_END
}

// The first case is today's chimera.bin.
const _: () = {
    assert!(vectors_valid(0x2000_FF00, 0x0802_0299));
    assert!(vectors_valid(0x2408_0000, 0x080F_FFFF));
    assert!(!vectors_valid(0xFFFF_FFFF, 0xFFFF_FFFF));
    assert!(!vectors_valid(0x2001_0000, 0x0802_0298));
    assert!(!vectors_valid(0x2001_0004, 0x0802_0299));
    assert!(!vectors_valid(0x2001_0000, 0x0801_FFFF));
    assert!(!vectors_valid(0x2001_0000, 0x0810_0001));
    assert!(!vectors_valid(0x2000_0000, 0x0802_0299));
    assert!(!vectors_valid(0x2002_0008, 0x0802_0299));
};

/// No firmware: light the LED and stop, so a blank unit is not a dark one.
fn no_firmware() -> ! {
    // SAFETY: RCC_AHB4ENR, GPIOE_MODER and GPIOE_BSRR are those registers'
    // fixed addresses; nothing else runs, and setting GPIOEEN, PE1's mode
    // bits to output (01) and its BSRR set bit touch nothing else.
    unsafe {
        RCC_AHB4ENR.write_volatile(RCC_AHB4ENR.read_volatile() | GPIOEEN);
        let _ = RCC_AHB4ENR.read_volatile();
        let moder = GPIOE_MODER.read_volatile() & !(0b11 << (2 * PE1));
        GPIOE_MODER.write_volatile(moder | 0b01 << (2 * PE1));
        GPIOE_BSRR.write_volatile(1 << PE1);
    }
    loop {
        cortex_m::asm::wfi();
    }
}

#[entry]
fn main() -> ! {
    let table = FIRMWARE_ADDR as *const u32;
    // SAFETY: the first two words of the firmware's flash sector; flash is
    // always readable, erased or not.
    let (sp, reset) = unsafe { (table.read_volatile(), table.add(1).read_volatile()) };
    if !vectors_valid(sp, reset) {
        no_firmware();
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
