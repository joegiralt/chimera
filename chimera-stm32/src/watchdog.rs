//! IWDG: a reset stops the SAI clocks, so a hung or locked-up audio
//! interrupt, or a halt after a fault, ends in silence and a reboot instead
//! of the DMA looping its last ring.

use core::sync::atomic::Ordering;

use chimera_core::audio_out::{Heartbeat, watchdog_timeout_ms};
use chimera_core::hw::{BLOCK_SIZE, SAMPLE_RATE};
use stm32h7xx_hal::independent_watchdog::IndependentWatchdog;
use stm32h7xx_hal::{pac, prelude::*};

use crate::audio::dma::BLOCKS;
use crate::controls::CONTROLS_HZ;

const TIMEOUT_MS: u32 = watchdog_timeout_ms(CONTROLS_HZ, BLOCK_SIZE as u32, SAMPLE_RATE);

/// Once the audio interrupt runs. The IWDG cannot be stopped again; it is
/// frozen while a debugger halts the core.
pub fn start(iwdg: pac::IWDG, dbgmcu: &pac::DBGMCU) {
    dbgmcu.apb4fz1.modify(|_, w| w.dbg_iwdg1().set_bit());
    IndependentWatchdog::new(iwdg).start(TIMEOUT_MS.millis());
}

/// From the controls tick, which runs below the audio interrupt: kick only
/// if audio has rendered since the last tick.
pub fn kick_if_audio_alive(heartbeat: &mut Heartbeat) {
    if heartbeat.advanced(BLOCKS.load(Ordering::Relaxed)) {
        // SAFETY: IWDG_KR is write-only and 0xAAAA only reloads the counter;
        // `start` has taken the peripheral and no longer touches it, so this
        // is its only writer.
        unsafe { (*pac::IWDG::ptr()).kr.write(|w| w.key().reset()) };
    }
}
