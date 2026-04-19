//! HC165 shift register control input via SysTick interrupt.
//!
//! PreenFM3 LQFP176: PF0=CLK, PF1=LOAD, PF2=DATA
//! Polled at 500Hz from SysTick ISR (matching PreenFM3).
//! Quadrature decoding matches PreenFM3 Encoders.cpp exactly.

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId, NUM_BUTTONS, NUM_ENCODERS};
use core::sync::atomic::{AtomicBool, AtomicI8, AtomicU32, Ordering};

// GPIOF registers
const GPIOF_IDR: *const u32 = 0x5802_1410 as *const u32;
const GPIOF_BSRR: *mut u32 = 0x5802_1418 as *mut u32;

// SysTick registers (Cortex-M standard)
const SYST_CSR: *mut u32 = 0xE000_E010 as *mut u32;
const SYST_RVR: *mut u32 = 0xE000_E014 as *mut u32;
const SYST_CVR: *mut u32 = 0xE000_E018 as *mut u32;

/// Encoder bit pairs from PreenFM3 (1-indexed pins → 0-indexed masks)
const ENC_BITS: [(u32, u32); 6] = [
    (1 << 16, 1 << 17), (1 << 14, 1 << 15), (1 << 8, 1 << 9),
    (1 << 19, 1 << 18), (1 << 13, 1 << 12), (1 << 11, 1 << 10),
];

/// Button bit masks from PreenFM3 (1-indexed pins → 0-indexed)
const BTN_BITS: [u32; NUM_BUTTONS] = [
    1 << 22, 1 << 20, 1 << 3, 1 << 23, 1 << 2, 1 << 1,
    1 << 21, 1 << 4, 1 << 5, 1 << 6, 1 << 7, 1 << 0,
];

/// N24 quadrature table — 1 count per detent click (vs N12 which gives 2)
const QUAD: [u8; 16] = [0,0,0,0, 0,0,0,1, 0,0,0,2, 0,0,0,0];

// Shared ISR ↔ main state
static READY: AtomicBool = AtomicBool::new(false);
static ENC_DELTA: [AtomicI8; 7] = [
    AtomicI8::new(0), AtomicI8::new(0), AtomicI8::new(0), AtomicI8::new(0),
    AtomicI8::new(0), AtomicI8::new(0), AtomicI8::new(0),
];
static BTN_LATCH: AtomicU32 = AtomicU32::new(0);
static RAW: AtomicU32 = AtomicU32::new(0xFFFFFFFF);
static mut ENC_STATE: [u8; 6] = [0; 6];
static mut ENC_DEBOUNCE: [u8; 6] = [0; 6];
static ISR_TICK: AtomicU32 = AtomicU32::new(0);
static ENC_LAST_EDGE: [AtomicU32; NUM_ENCODERS] = [
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
];

/// Configure SysTick for 500Hz interrupt. Call after clocks are configured.
/// hclk_hz: HCLK frequency in Hz (e.g. 200_000_000 for 200MHz)
pub fn start_systick(hclk_hz: u32) {
    let reload = hclk_hz / 500 - 1; // 500Hz
    unsafe {
        core::ptr::write_volatile(SYST_CSR, 0); // disable
        core::ptr::write_volatile(SYST_RVR, reload);
        core::ptr::write_volatile(SYST_CVR, 0); // clear current
        // Enable counter + interrupt + use processor clock
        core::ptr::write_volatile(SYST_CSR, 0b111);
    }
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

    // Read HC165 via raw register access (same GPIO that diagnostic proved works)
    let bits = unsafe {
        // Latch: LOAD low then high
        core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 17); // PF1 reset (LOAD low)
        cortex_m::asm::delay(100);
        core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 1);  // PF1 set (LOAD high)
        cortex_m::asm::delay(100);

        let mut b: u32 = 0;
        for i in 0..24u32 {
            core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 16); // PF0 reset (CLK low)
            cortex_m::asm::delay(100);
            if core::ptr::read_volatile(GPIOF_IDR) & (1 << 2) != 0 { // PF2 (DATA)
                b |= 1 << i;
            }
            core::ptr::write_volatile(GPIOF_BSRR, 1u32 << 0); // PF0 set (CLK high)
            cortex_m::asm::delay(100);
        }
        b
    };

    RAW.store(bits, Ordering::Relaxed);

    // Buttons: active low → latch
    let mut pressed: u32 = 0;
    for (i, &mask) in BTN_BITS.iter().enumerate() {
        if bits & mask == 0 { pressed |= 1 << i; }
    }
    BTN_LATCH.fetch_or(pressed, Ordering::Relaxed);

    // Encoders: quadrature decode with debounce
    for i in 0..6 {
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
        let latched = BTN_LATCH.swap(0, Ordering::Relaxed);
        for i in 0..NUM_BUTTONS {
            self.btn_cur[i] = latched & (1 << i) != 0;
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

    pub fn raw_bits(&self) -> u32 { RAW.load(Ordering::Relaxed) }

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
        let i = id as usize;
        if i < NUM_ENCODERS { self.enc[i] } else { 0 }
    }

    fn button_state(&self, id: ButtonId) -> ButtonState {
        let i = id as usize;
        if i >= NUM_BUTTONS { return ButtonState::Up; }
        match (self.btn_prev[i], self.btn_cur[i]) {
            (false, true) => ButtonState::Pressed,
            (true, true) => ButtonState::Held,
            (true, false) => ButtonState::Released,
            (false, false) => ButtonState::Up,
        }
    }
}
