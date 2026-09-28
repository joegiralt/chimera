//! IWDG: a reset stops the SAI clocks, so a hung or locked-up audio
//! interrupt, or a halt after a fault, ends in silence and a reboot instead
//! of the DMA looping its last ring.

use core::sync::atomic::Ordering;

use chimera_core::audio_out::{Heartbeat, WATCHDOG_TIMEOUT_MS, iwdg_reload_at_div4};
use stm32h7xx_hal::independent_watchdog::IndependentWatchdog;
use stm32h7xx_hal::{pac, prelude::*};

use crate::audio::dma::BLOCKS;

// The HAL picks the finest prescaler that fits: /4, whose 12-bit reload
// holds the timeout.
const _: () = assert!(iwdg_reload_at_div4(WATCHDOG_TIMEOUT_MS) <= 0xFFF);

/// Before the audio interrupt is unmasked: the block count cannot move yet,
/// so no kick lands while the HAL writes PR and RLR. The IWDG cannot be
/// stopped again; it is frozen while a debugger halts the core.
pub fn start(iwdg: pac::IWDG, dbgmcu: &pac::DBGMCU) {
    dbgmcu.apb4fz1.modify(|_, w| w.dbg_iwdg1().set_bit());
    IndependentWatchdog::new(iwdg).start(WATCHDOG_TIMEOUT_MS.millis());
}

/// From the controls tick, which runs below the audio interrupt: kick only
/// if audio has rendered since the last tick.
pub fn kick_if_audio_alive(heartbeat: &mut Heartbeat) {
    if heartbeat.advanced(BLOCKS.load(Ordering::Relaxed)) {
        // SAFETY: IWDG_KR is write-only and 0xAAAA only reloads the counter.
        // The block count first moves once the audio interrupt is unmasked,
        // after `start` has returned, so this never interleaves with its
        // writes and is then the IWDG's only writer.
        unsafe { (*pac::IWDG::ptr()).kr.write(|w| w.key().reset()) };
    }
}
