//! IWDG: a reset stops the SAI clocks, so a hung or locked-up audio
//! interrupt, or a halt after a fault, ends in silence and a reboot instead
//! of the DMA looping its last ring.

use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::audio_out::{
    Heartbeat, LIVE_WAIT_MS, LSI_MAX_HZ, LSI_NOMINAL_HZ, LiveCheck, WATCHDOG_TIMEOUT_MS,
    iwdg_reload_at_div4, kick_gap_us,
};
use chimera_core::boot::wait_until;
use chimera_core::hw::{BLOCK_SIZE, SAMPLE_RATE};
use cortex_m::peripheral::{DWT, SCB};
use stm32h7xx_hal::independent_watchdog::IndependentWatchdog;
use stm32h7xx_hal::{pac, prelude::*};

use crate::audio::dma::BLOCKS;
use crate::controls::CONTROLS_HZ;

// The timeout, shortest at the LSI's fastest, outlasts the longest healthy
// gap between kicks at this tick rate and block size.
const _: () = assert!(
    (WATCHDOG_TIMEOUT_MS as u64 * 1000 * LSI_NOMINAL_HZ as u64 / LSI_MAX_HZ as u64)
        > kick_gap_us(CONTROLS_HZ, BLOCK_SIZE as u32, SAMPLE_RATE) as u64
);

// The HAL picks the finest prescaler that fits: /4, whose 12-bit reload
// holds the timeout.
const _: () = assert!(iwdg_reload_at_div4(WATCHDOG_TIMEOUT_MS) <= 0xFFF);

/// Set once `start` has returned: the tick kicks only then, so no kick
/// lands while the HAL writes KR, PR and RLR.
static ARMED: AtomicBool = AtomicBool::new(false);

/// The audio renders and the controls tick runs, so the kicks are live.
/// Only `await_live` makes one, and `start` takes it: the watchdog can't
/// be armed over a start-up step with no kicks behind it.
#[must_use]
pub struct Live(());

/// Waits until audio blocks and controls ticks both move (`LiveCheck`).
/// After `LIVE_WAIT_MS` without, it resets, as the watchdog would.
pub fn await_live(cpu_hz: u32) -> Live {
    let check = LiveCheck::start(BLOCKS.load(Ordering::Relaxed), crate::controls::ticks());
    let live = wait_until(LIVE_WAIT_MS * (cpu_hz / 1000), DWT::cycle_count, || {
        check.live(BLOCKS.load(Ordering::Relaxed), crate::controls::ticks())
    });
    if !live {
        SCB::sys_reset();
    }
    Live(())
}

/// The last start-up step: the IWDG cannot be stopped again (a reset
/// does), and it is frozen while a debugger halts the core.
pub fn start(iwdg: pac::IWDG, dbgmcu: &pac::DBGMCU, _live: Live) {
    dbgmcu.apb4fz1.modify(|_, w| w.dbg_iwdg1().set_bit());
    IndependentWatchdog::new(iwdg).start(WATCHDOG_TIMEOUT_MS.millis());
    ARMED.store(true, Ordering::Release);
}

/// From the controls tick, which runs below the audio interrupt: once
/// armed, kick only if audio has rendered since the last tick.
pub fn kick_if_audio_alive(heartbeat: &mut Heartbeat) {
    let moved = heartbeat.advanced(BLOCKS.load(Ordering::Relaxed));
    if moved && ARMED.load(Ordering::Acquire) {
        // SAFETY: IWDG_KR is write-only and 0xAAAA only reloads the counter.
        // ARMED is set after `start` has returned, so this never interleaves
        // with its writes and is then the IWDG's only writer.
        unsafe { (*pac::IWDG::ptr()).kr.write(|w| w.key().reset()) };
    }
}
