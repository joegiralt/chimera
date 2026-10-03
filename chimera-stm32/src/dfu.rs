//! Entering ST's ROM DFU loader from the firmware, so `just flash` needs no
//! BOOT0 jumper (USB console spec § Enter DFU from the firmware). `enter`
//! leaves a marker in RTC_BKP0R and resets; `after_reset`, the top of
//! `main`, reads and clears it, and jumps only when `boot::after_reset`
//! says so: the clear read back, after a software reset. BKP1R..3R keep a
//! boot count, who wrote the marker and the last jump's RCC_RSR, for
//! `status`'s `boot` line.

#[cfg(all(not(feature = "sd-probe"), feature = "usb-console"))]
use chimera_core::boot::FROM_CONSOLE;
use chimera_core::boot::UsbRegs;
use chimera_core::boot::{self, BootAction, BootSeen, BootStage, ROM_DFU_BASE};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::boot::{DFU_MAGIC, FROM_MENU, RomDfu};
#[cfg(feature = "usb-console")]
use chimera_core::boot::{UsbState, UsbStep};
use chimera_core::reset::ResetCause;
#[cfg(not(feature = "sd-probe"))]
use chimera_core::ui::settings::replace::said::Said;
use stm32h7xx_hal::pac;

/// DBP read-backs before giving up: it lands in a few cycles.
const DBP_TRIES: u32 = 1_000;

/// The backup registers this module owns.
const MARKER: usize = 0;
const BOOTS: usize = 1;
const FROM: usize = 2;
const JUMP_RSR: usize = 3;
const STAGE: usize = 4;
const USB: usize = 5;
const USB_STEP: usize = 6;
/// BKP7R..12R: `UsbRegs::words`.
const USB_REGS: usize = 7;

/// `after_reset` ran: RTCAPBEN is set, the marker is clear (or stuck, and
/// ignored) and RCC_RSR is read and cleared. Only `after_reset` makes one,
/// and `boot` takes it.
#[must_use]
pub struct Checked(BootSeen);

impl Checked {
    /// What the top of `main` saw.
    pub fn seen(&self) -> BootSeen {
        self.0
    }
}

/// The marker's register, writable: what `enter` needs.
#[cfg(not(feature = "sd-probe"))]
pub struct Marker(pac::RTC);

#[cfg(not(feature = "sd-probe"))]
impl Marker {
    pub fn new(_: Checked, rtc: pac::RTC) -> Marker {
        Marker(rtc)
    }

    /// How far start-up got, for the next boot's `last_stage`.
    pub fn stage(&self, s: BootStage) {
        self.0.bkpr[STAGE].write(|w| w.bkp().bits(s.code()));
    }

    /// How the USB step ended, for the next boot's `last_usb`.
    #[cfg(feature = "usb-console")]
    pub fn usb(&self, s: UsbState) {
        self.0.bkpr[USB].write(|w| w.bkp().bits(s.code()));
    }

    /// How far the USB bring-up got, on try `tries`.
    #[cfg(feature = "usb-console")]
    pub fn usb_step(&self, s: UsbStep, tries: u8) {
        self.0.bkpr[USB_STEP].write(|w| w.bkp().bits(s.code(tries)));
    }

    /// The OTG and PWR registers now, for the next boot's line.
    #[cfg(feature = "usb-console")]
    pub fn usb_regs(&self, r: UsbRegs) {
        for (i, word) in r.words().into_iter().enumerate() {
            self.0.bkpr[USB_REGS + i].write(|w| w.bkp().bits(word));
        }
    }
}

/// Why `enter` may run: a proof, never a flag.
#[cfg(not(feature = "sd-probe"))]
pub enum DfuFrom {
    /// SEQ on OS UPGRADE's `ENTER DFU`.
    Menu(Said<RomDfu>),
    /// The console's `dfu`, its `OK` flushed and drained.
    #[cfg(feature = "usb-console")]
    Console(crate::usb::DfuAsked),
}

/// DBP set and read back, then BKP0R cleared: what it reads back. Not 0 is
/// stuck (DBP never set, or the write did not land), and
/// `boot::after_reset` boots the synth on it. A transient stuck leaves the
/// magic in place, so a later clean reset may enter DFU: accepted, the ROM
/// loader can't brick the unit.
fn clear(pwr: &pac::PWR, rtc: &pac::RTC) -> u32 {
    pwr.cr1.modify(|_, w| w.dbp().set_bit());
    // Without DBP the write is ignored and the read-back says so.
    let _ = (0..DBP_TRIES).any(|_| pwr.cr1.read().dbp().bit_is_set());
    rtc.bkpr[MARKER].write(|w| w.bkp().bits(0));
    rtc.bkpr[MARKER].read().bits()
}

/// The top of `main`, before `boot()` touches a clock or a peripheral:
/// RCC_RSR read and cleared, RTCAPBEN, read BKP0R, clear it, then jump if
/// `boot::after_reset` says so. The marker is cleared whatever it held, so
/// the next reset always plays, and a power-on never jumps.
pub fn after_reset(
    cp: &mut cortex_m::Peripherals,
    rcc: &pac::RCC,
    pwr: &pac::PWR,
    rtc: &pac::RTC,
) -> Checked {
    // RCC_RSR survives the reset it records: cleared here, before the jump
    // too, so the next boot reads only its own reset. RMVF holds the flags
    // at 0 while set, so it is set and then cleared again.
    let rsr = rcc.rsr.read().bits();
    rcc.rsr.modify(|_, w| w.rmvf().set_bit());
    rcc.rsr.modify(|_, w| w.rmvf().clear_bit());
    let dbp = pwr.cr1.read().dbp().bit_is_set();
    rcc.apb4enr.modify(|_, w| w.rtcapben().set_bit());
    // The enable lands before the first RTC access (RM0433 § RCC).
    let _ = rcc.apb4enr.read();
    cortex_m::asm::dsb();
    let marker = rtc.bkpr[MARKER].read().bits();
    let readback = clear(pwr, rtc);
    let boots = rtc.bkpr[BOOTS].read().bits().wrapping_add(1);
    rtc.bkpr[BOOTS].write(|w| w.bkp().bits(boots));
    let action = boot::after_reset(marker, readback, ResetCause::from_rsr(rsr));
    if action == BootAction::RomDfu {
        // The boot record stays as the boot before left it: the DFU detour
        // (OS UPGRADE, flash, leave) is how a cold boot's record is read.
        rtc.bkpr[JUMP_RSR].write(|w| w.bkp().bits(rsr));
        jump(cp)
    }
    let read = |i: usize| rtc.bkpr[i].read().bits();
    let last_stage = read(STAGE);
    let last_usb = read(USB);
    let last_usb_step = read(USB_STEP);
    let last_usb_regs = UsbRegs::from_words(core::array::from_fn(|i| read(USB_REGS + i)));
    // This boot's record starts here, so a step it never reaches reads none.
    rtc.bkpr[STAGE].write(|w| w.bkp().bits(BootStage::Entry.code()));
    for i in [USB, USB_STEP] {
        rtc.bkpr[i].write(|w| w.bkp().bits(0));
    }
    match action {
        BootAction::RomDfu => jump(cp),
        BootAction::Synth => Checked(BootSeen {
            marker,
            readback,
            action,
            rsr,
            dbp,
            boots,
            from: rtc.bkpr[FROM].read().bits(),
            jump_rsr: rtc.bkpr[JUMP_RSR].read().bits(),
            last_stage,
            last_usb,
            last_usb_step,
            last_usb_regs,
        }),
    }
}

/// Into the ROM loader, never back. The marker read back clear, so the
/// next reset plays.
fn jump(cp: &mut cortex_m::Peripherals) -> ! {
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

/// Writes who asked to BKP2R and `DFU_MAGIC` to BKP0R, then
/// `SCB::sys_reset()`.
#[cfg(not(feature = "sd-probe"))]
pub fn enter(m: &Marker, from: DfuFrom) -> ! {
    let tag = match from {
        DfuFrom::Menu(_) => FROM_MENU,
        #[cfg(feature = "usb-console")]
        DfuFrom::Console(_) => FROM_CONSOLE,
    };
    m.0.bkpr[FROM].write(|w| w.bkp().bits(tag));
    m.0.bkpr[MARKER].write(|w| w.bkp().bits(DFU_MAGIC));
    cortex_m::peripheral::SCB::sys_reset()
}
