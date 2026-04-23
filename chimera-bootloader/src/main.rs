//! Chimera bootloader — minimal jump to firmware at 0x08020000.
//! Replaces the PreenFM3 bootloader to give firmware a clean peripheral state.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;

/// Firmware entry point in flash.
const FIRMWARE_ADDR: u32 = 0x0802_0000;

#[entry]
fn main() -> ! {
    // Read firmware's vector table
    let sp = unsafe { core::ptr::read_volatile(FIRMWARE_ADDR as *const u32) };
    let reset = unsafe { core::ptr::read_volatile((FIRMWARE_ADDR + 4) as *const u32) };

    unsafe {
        // Set vector table to firmware's location
        core::ptr::write_volatile(0xE000_ED08 as *mut u32, FIRMWARE_ADDR);

        // Set main stack pointer to firmware's initial SP
        cortex_m::register::msp::write(sp);

        // Jump to firmware's reset handler — never returns
        let jump: extern "C" fn() -> ! = core::mem::transmute(reset);
        jump();
    }
}
