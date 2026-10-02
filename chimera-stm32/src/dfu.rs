//! Entering ST's ROM DFU loader from the firmware, so `just flash` needs no
//! BOOT0 jumper (USB console spec § Enter DFU from the firmware). `enter`
//! leaves a marker and resets; `after_reset`, the top of `main`, reads and
//! clears it and jumps when `boot::after_reset` says so.

use chimera_core::boot::{self, BootAction, ROM_DFU_BASE};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::boot::{DFU_MAGIC, RomDfu};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::ui::settings::replace::said::Said;
use stm32h7xx_hal::pac;

/// A word a system reset keeps.
#[derive(Clone, Copy)]
enum Marker {
    /// RTC_BKP0R (0x5800_4050): the backup domain. RTCAPBEN clocks it,
    /// DBP unlocks it for writing.
    Bkp0,
    /// A word of D3 SRAM4: neither the stock bootloader's nor Chimera's
    /// linker script puts anything in RAM_D3.
    #[allow(dead_code, reason = "U10's fallback, if BKP0R is clobbered")]
    Sram4(u32),
}

/// The marker: RTC_BKP0R. Fallback if U10 finds it clobbered: the last
/// word of D3 SRAM4, `Marker::Sram4(SRAM4_LAST)`.
const MARKER: Marker = Marker::Bkp0;

/// 0x3800_FFFC: SRAM4 is 64 KB at 0x3800_0000 (RM0433).
#[allow(dead_code, reason = "U10's fallback, if BKP0R is clobbered")]
const SRAM4_LAST: u32 = 0x3800_FFFC;
const _: () = assert!(SRAM4_LAST.is_multiple_of(4) && SRAM4_LAST + 4 == 0x3800_0000 + 64 * 1024);

fn read(rtc: &pac::RTC) -> u32 {
    match MARKER {
        Marker::Bkp0 => rtc.bkpr[0].read().bits(),
        // SAFETY: an aligned word of SRAM4, which is always clocked and
        // which nothing else of Chimera's maps.
        Marker::Sram4(at) => unsafe { core::ptr::read_volatile(at as *const u32) },
    }
}

fn write(rtc: &pac::RTC, v: u32) {
    match MARKER {
        Marker::Bkp0 => rtc.bkpr[0].write(|w| w.bkp().bits(v)),
        // SAFETY: as `read`.
        Marker::Sram4(at) => unsafe { core::ptr::write_volatile(at as *mut u32, v) },
    }
}

/// Why `enter` may run: a proof, never a flag.
#[cfg(not(feature = "sd-probe"))]
pub enum DfuFrom {
    /// SEQ on OS UPGRADE's `ENTER DFU`.
    Menu(Said<RomDfu>),
    /// The console's `dfu`, its `OK` flushed.
    #[cfg(feature = "usb-console")]
    Console(crate::usb::DfuAsked),
}

/// The top of `main`, before `boot()` touches a clock or a peripheral:
/// enable RTCAPBEN, read BKP0R, set DBP, clear BKP0R, then jump on `RomDfu`.
/// The marker is cleared whatever it held, so the next reset always plays.
pub fn after_reset(cp: &mut cortex_m::Peripherals, rcc: &pac::RCC, pwr: &pac::PWR, rtc: &pac::RTC) {
    rcc.apb4enr.modify(|_, w| w.rtcapben().set_bit());
    let m = read(rtc);
    pwr.cr1.modify(|_, w| w.dbp().set_bit());
    while pwr.cr1.read().dbp().bit_is_clear() {}
    write(rtc, 0);
    match boot::after_reset(m) {
        BootAction::Synth => {}
        BootAction::RomDfu => {
            // The stock bootloader's `HAL_SuspendTick` leaves it counting.
            cp.SYST.disable_interrupt();
            cp.SYST.disable_counter();
            // SAFETY: ROM_DFU_BASE is the vector table of ST's system
            // memory bootloader (AN2606, STM32H74x/75x), immutable ROM: its
            // first word is a valid initial MSP, its second a valid reset
            // vector. This runs before any clock, peripheral or interrupt
            // of Chimera's is set up, on the state the stock bootloader's
            // `MX_Deinit` left, as the ROM expects after a reset, and
            // never returns, so nothing of Chimera's runs on the new stack.
            unsafe {
                cp.SCB.vtor.write(ROM_DFU_BASE);
                cortex_m::asm::bootload(ROM_DFU_BASE as *const u32)
            }
        }
    }
}

/// Writes `DFU_MAGIC` to the marker, then `SCB::sys_reset()`. RTCAPBEN and
/// DBP are already set (`after_reset`, the HAL's PWR `freeze`).
#[cfg(not(feature = "sd-probe"))]
pub fn enter(rtc: &pac::RTC, from: DfuFrom) -> ! {
    let _proof = from;
    write(rtc, DFU_MAGIC);
    cortex_m::peripheral::SCB::sys_reset()
}
