//! Entering ST's ROM DFU loader from the firmware, so `just flash` needs no
//! BOOT0 jumper (USB console spec § Enter DFU from the firmware). `enter`
//! leaves a marker in RTC_BKP0R and resets; `after_reset`, the top of
//! `main`, reads and clears it, and jumps only when `boot::after_reset`
//! says so and the clear read back.

use chimera_core::boot::{self, BootAction, ROM_DFU_BASE};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::boot::{DFU_MAGIC, RomDfu};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::ui::settings::replace::said::Said;
use stm32h7xx_hal::pac;

/// DBP read-backs before giving up: it lands in a few cycles.
const DBP_TRIES: u32 = 1_000;

/// `after_reset` ran: RTCAPBEN is set and the marker is clear (or stuck,
/// and ignored). Only `after_reset` makes one, and `boot` takes it.
#[must_use]
pub struct Checked(());

/// The marker's register, writable: what `enter` needs.
#[cfg(not(feature = "sd-probe"))]
pub struct Marker(pac::RTC);

#[cfg(not(feature = "sd-probe"))]
impl Marker {
    pub fn new(_: Checked, rtc: pac::RTC) -> Marker {
        Marker(rtc)
    }
}

/// BKP0R reads back 0.
struct Cleared(());

/// BKP0R would not clear (DBP never set, or the write did not land): the
/// synth boots, so a stuck marker can't trap the unit in DFU.
struct Stuck;

/// Why `enter` may run: a proof, never a flag.
#[cfg(not(feature = "sd-probe"))]
pub enum DfuFrom {
    /// SEQ on OS UPGRADE's `ENTER DFU`.
    Menu(Said<RomDfu>),
    /// The console's `dfu`, its `OK` flushed and drained.
    #[cfg(feature = "usb-console")]
    Console(crate::usb::DfuAsked),
}

/// DBP set and read back, then BKP0R cleared and read back.
fn clear(pwr: &pac::PWR, rtc: &pac::RTC) -> Result<Cleared, Stuck> {
    pwr.cr1.modify(|_, w| w.dbp().set_bit());
    if !(0..DBP_TRIES).any(|_| pwr.cr1.read().dbp().bit_is_set()) {
        return Err(Stuck);
    }
    rtc.bkpr[0].write(|w| w.bkp().bits(0));
    match rtc.bkpr[0].read().bits() {
        0 => Ok(Cleared(())),
        _ => Err(Stuck),
    }
}

/// The top of `main`, before `boot()` touches a clock or a peripheral:
/// RTCAPBEN, read BKP0R, clear it, then jump on `RomDfu` if it cleared.
/// The marker is cleared whatever it held, so the next reset always plays.
pub fn after_reset(
    cp: &mut cortex_m::Peripherals,
    rcc: &pac::RCC,
    pwr: &pac::PWR,
    rtc: &pac::RTC,
) -> Checked {
    rcc.apb4enr.modify(|_, w| w.rtcapben().set_bit());
    // The enable lands before the first RTC access (RM0433 § RCC).
    let _ = rcc.apb4enr.read();
    cortex_m::asm::dsb();
    let m = rtc.bkpr[0].read().bits();
    match (boot::after_reset(m), clear(pwr, rtc)) {
        (BootAction::RomDfu, Ok(c)) => jump(cp, c),
        (BootAction::Synth, _) | (BootAction::RomDfu, Err(Stuck)) => Checked(()),
    }
}

/// Into the ROM loader, never back. `Cleared`: the next reset plays.
fn jump(cp: &mut cortex_m::Peripherals, _: Cleared) -> ! {
    // The stock bootloader's `HAL_SuspendTick` leaves it counting.
    cp.SYST.disable_interrupt();
    cp.SYST.disable_counter();
    // SAFETY: ROM_DFU_BASE is the vector table of ST's system memory
    // bootloader (AN2606, STM32H74x/75x), immutable ROM: its first word is
    // a valid initial MSP, its second a valid reset vector. What the jump
    // relies on, as after a reset:
    // - PRIMASK clear, as at reset (Chimera's own interrupts need it too);
    // - every NVIC line disabled and none pending (cleared here), SysTick
    //   stopped (above), and nothing else of Chimera's set up yet;
    // - the caches and the MPU as the stock bootloader left them, which
    //   is how it starts any image;
    // - RTCAPBEN and DBP set: harmless, the loader touches neither.
    // It never returns, so nothing of Chimera's runs on the ROM's stack.
    unsafe {
        for i in 0..8 {
            cp.NVIC.icer[i].write(u32::MAX);
            cp.NVIC.icpr[i].write(u32::MAX);
        }
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        cp.SCB.vtor.write(ROM_DFU_BASE);
        cortex_m::asm::bootload(ROM_DFU_BASE as *const u32)
    }
}

/// Writes `DFU_MAGIC` to BKP0R, then `SCB::sys_reset()`.
#[cfg(not(feature = "sd-probe"))]
pub fn enter(m: &Marker, _from: DfuFrom) -> ! {
    m.0.bkpr[0].write(|w| w.bkp().bits(DFU_MAGIC));
    cortex_m::peripheral::SCB::sys_reset()
}
