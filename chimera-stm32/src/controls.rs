//! HC165 shift register control input via SysTick interrupt.
//!
//! PreenFM3 LQFP176: PF0=CLK, PF1=LOAD, PF2=DATA
//! Polled at 500Hz from SysTick ISR (matching PreenFM3).
//! Quadrature decoding matches PreenFM3 Encoders.cpp exactly.

use chimera_core::clock_plan::{cycles_for_ns, systick_reload};
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId, NUM_BUTTONS, NUM_ENCODERS};
use core::sync::atomic::{AtomicBool, AtomicI8, AtomicU32, Ordering};
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m::peripheral::{SCB, SYST};

use crate::priority::{self, Priority};

// GPIOF registers
const GPIOF_IDR: *const u32 = 0x5802_1410 as *const u32;
const GPIOF_BSRR: *mut u32 = 0x5802_1418 as *mut u32;

pub const CONTROLS_HZ: u32 = 500;
// The HC165 was clocked with 100-cycle spins at 400 MHz: keep 250 ns at any clock.
const HC165_HALF_PERIOD_NS: u32 = 250;
static HC165_DELAY: AtomicU32 = AtomicU32::new(100);

/// Encoder bit pairs from PreenFM3 (1-indexed pins → 0-indexed masks)
const ENC_BITS: [(u32, u32); NUM_ENCODERS] = [
    (1 << 16, 1 << 17),
    (1 << 14, 1 << 15),
    (1 << 8, 1 << 9),
    (1 << 19, 1 << 18),
    (1 << 13, 1 << 12),
    (1 << 11, 1 << 10),
];

/// Button bit masks from PreenFM3 (1-indexed pins → 0-indexed)
const BTN_BITS: [u32; NUM_BUTTONS] = [
    1 << 22,
    1 << 20,
    1 << 3,
    1 << 23,
    1 << 2,
    1 << 1,
    1 << 21,
    1 << 4,
    1 << 5,
    1 << 6,
    1 << 7,
    1 << 0,
];

/// N24 quadrature table — 1 count per detent click (vs N12 which gives 2)
const QUAD: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0];

// Shared ISR ↔ main state
static READY: AtomicBool = AtomicBool::new(false);
static ENC_DELTA: [AtomicI8; NUM_ENCODERS] = [const { AtomicI8::new(0) }; NUM_ENCODERS];
static BTN_LATCH: AtomicU32 = AtomicU32::new(0);
static mut ENC_STATE: [u8; NUM_ENCODERS] = [0; NUM_ENCODERS];
static mut ENC_DEBOUNCE: [u8; NUM_ENCODERS] = [0; NUM_ENCODERS];
/// Button debounce: tracks how many consecutive ISR ticks a button has been stable.
/// Only latches as pressed after BTN_DEBOUNCE_TICKS consecutive "pressed" reads.
static mut BTN_DEBOUNCE: [u8; NUM_BUTTONS] = [0; NUM_BUTTONS];
static mut BTN_STATE: [bool; NUM_BUTTONS] = [false; NUM_BUTTONS];
const BTN_DEBOUNCE_TICKS: u8 = 3; // 6ms at 500Hz
static ISR_TICK: AtomicU32 = AtomicU32::new(0);
static ENC_LAST_EDGE: [AtomicU32; NUM_ENCODERS] = [const { AtomicU32::new(0) }; NUM_ENCODERS];

/// Arms SysTick at `CONTROLS_HZ`, its priority set first so the first tick
/// cannot run at the reset priority (0, the audio level).
pub fn start_systick(mut syst: SYST, scb: &mut SCB, cpu_hz: u32) {
    HC165_DELAY.store(
        cycles_for_ns(cpu_hz, HC165_HALF_PERIOD_NS),
        Ordering::Relaxed,
    );
    priority::set_systick(scb, Priority::SYSTICK);
    syst.disable_counter();
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(systick_reload(cpu_hz, CONTROLS_HZ));
    syst.clear_current();
    syst.enable_interrupt();
    syst.enable_counter();
}

/// Allow ISR to run. Call after GPIOF is fully configured.
pub fn enable() {
    READY.store(true, Ordering::Release);
}

/// SysTick ISR handler. Reads HC165 and decodes inputs.
pub fn isr_tick() {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    ISR_TICK.fetch_add(1, Ordering::Relaxed);

    let d = HC165_DELAY.load(Ordering::Relaxed);
    // Shift the HC165 chain in: LOAD on PF1, CLK on PF0, DATA on PF2.
    // SAFETY: GPIOF_IDR/BSRR are PF's fixed memory-mapped registers; only
    // this ISR (single, non-reentrant) drives PF0/PF1 and reads PF2.
    let bits = unsafe {
        // Latch: LOAD low then high
        core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 17); // PF1 reset (LOAD low)
        cortex_m::asm::delay(d);
        core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 1); // PF1 set (LOAD high)
        cortex_m::asm::delay(d);

        let mut b: u32 = 0;
        for i in 0..24u32 {
            core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 16); // PF0 reset (CLK low)
            cortex_m::asm::delay(d);
            if core::ptr::read_volatile(GPIOF_IDR) & (1 << 2) != 0 {
                // PF2 (DATA)
                b |= 1 << i;
            }
            core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 0); // PF0 set (CLK high)
            cortex_m::asm::delay(d);
        }
        b
    };

    // Buttons: active low, debounced to stable level
    // SAFETY: only accessed from this ISR
    unsafe {
        let mut debounced: u32 = 0;
        for (i, &mask) in BTN_BITS.iter().enumerate() {
            let raw_pressed = bits & mask == 0;
            if raw_pressed == BTN_STATE[i] {
                // Same as current debounced state — reset counter
                BTN_DEBOUNCE[i] = 0;
            } else {
                // Different from debounced state — count stable ticks
                BTN_DEBOUNCE[i] += 1;
                if BTN_DEBOUNCE[i] >= BTN_DEBOUNCE_TICKS {
                    BTN_STATE[i] = raw_pressed;
                    BTN_DEBOUNCE[i] = 0;
                }
            }
            if BTN_STATE[i] {
                debounced |= 1 << i;
            }
        }
        // Store debounced level (not edge-latched)
        BTN_LATCH.store(debounced, Ordering::Relaxed);
    }

    // Encoders: quadrature decode with debounce
    for i in 0..NUM_ENCODERS {
        // SAFETY: only accessed from this ISR (single-threaded)
        unsafe {
            if ENC_DEBOUNCE[i] > 0 {
                ENC_DEBOUNCE[i] -= 1;
                continue;
            }

            let (bit1_mask, bit2_mask) = ENC_BITS[i];
            let b1: u8 = if bits & bit1_mask == 0 { 1 } else { 0 };
            let b2: u8 = if bits & bit2_mask == 0 { 1 } else { 0 };
            let cur = b1 | (b2 << 1);

            ENC_STATE[i] <<= 2;
            ENC_STATE[i] &= 0x0F;
            ENC_STATE[i] |= cur;

            let action = QUAD[ENC_STATE[i] as usize];
            if action == 1 {
                ENC_DELTA[i].fetch_add(1, Ordering::Relaxed);
                ENC_LAST_EDGE[i].store(ISR_TICK.load(Ordering::Relaxed), Ordering::Relaxed);
                ENC_DEBOUNCE[i] = 2;
            } else if action == 2 {
                ENC_DELTA[i].fetch_add(-1, Ordering::Relaxed);
                ENC_LAST_EDGE[i].store(ISR_TICK.load(Ordering::Relaxed), Ordering::Relaxed);
                ENC_DEBOUNCE[i] = 2;
            }
        }
    }
}

// --- Main-thread interface ---

/// Per-click acceleration: 1, 2, 4, 5, 10, 10, 10...
const ACCEL_CURVE: [i8; 5] = [1, 2, 4, 5, 10];

fn accel_for(burst_pos: u8) -> i8 {
    ACCEL_CURVE.get(burst_pos as usize).copied().unwrap_or(10)
}

pub struct Stm32Controls {
    btn_prev: [bool; NUM_BUTTONS],
    btn_cur: [bool; NUM_BUTTONS],
    enc: [i8; NUM_ENCODERS],
    burst: [u8; NUM_ENCODERS],
}

impl Stm32Controls {
    pub fn new() -> Self {
        Self {
            btn_prev: [false; NUM_BUTTONS],
            btn_cur: [false; NUM_BUTTONS],
            enc: [0; NUM_ENCODERS],
            burst: [0; NUM_ENCODERS],
        }
    }

    /// Read and clear accumulated ISR state. Call once per frame.
    pub fn snapshot(&mut self) {
        self.btn_prev = self.btn_cur;
        let debounced = BTN_LATCH.load(Ordering::Relaxed);
        for (i, cur) in self.btn_cur.iter_mut().enumerate() {
            *cur = debounced & (1 << i) != 0;
        }
        let now = ISR_TICK.load(Ordering::Relaxed);
        for i in 0..NUM_ENCODERS {
            let raw = ENC_DELTA[i].swap(0, Ordering::Relaxed);
            // Reset burst if >150ms (75 ticks at 500Hz) since last edge
            let last = ENC_LAST_EDGE[i].load(Ordering::Relaxed);
            if now.wrapping_sub(last) > 75 {
                self.burst[i] = 0;
            }
            if raw != 0 {
                let dir: i8 = if raw > 0 { 1 } else { -1 };
                let edges = raw.unsigned_abs();
                let mut total: i8 = 0;
                for _ in 0..edges {
                    total = total.saturating_add(dir * accel_for(self.burst[i]));
                    self.burst[i] = self.burst[i].saturating_add(1);
                }
                self.enc[i] = total;
            } else {
                self.enc[i] = 0;
            }
        }
    }

    /// Returns true if any button changed state or any encoder moved this frame.
    pub fn has_activity(&self) -> bool {
        // Any button pressed or just released (need Released event for UI)
        if self.btn_cur.iter().any(|&b| b) || self.btn_prev.iter().any(|&b| b) {
            return true;
        }
        self.enc.iter().any(|&e| e != 0)
    }
}

impl Controls for Stm32Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.enc[id as usize]
    }

    fn button_state(&self, id: ButtonId) -> ButtonState {
        let i = id as usize;
        ButtonState::from_levels(self.btn_prev[i], self.btn_cur[i])
    }
}
