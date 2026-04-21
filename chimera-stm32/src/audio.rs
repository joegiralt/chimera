//! SAI1 Block A audio output — test tone bringup.
//!
//! Configures PLL3 for ~48kHz audio clock, sets up SAI1_A as I2S master TX,
//! and provides FIFO polling + data write helpers.
//!
//! PLL3: HSE 8MHz / M=1 * N=46 / P=3 = 122.67 MHz SAI kernel clock
//! SAI1_A: MCKDIV=5 → MCLK=12.27MHz → FS=47917Hz

use stm32h7xx_hal::pac;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::interrupt;

// SAI1 Block A CR1 register address for raw bit manipulation (MCKEN bit 27)
const SAI1_CHA_CR1: *mut u32 = 0x4001_5804 as *mut u32;

// SAI1 Block A data register address: base 0x40015800 + CHA offset 0x04 + DR offset 0x1C
const SAI1_CHA_DR: u32 = 0x4001_5820;

/// DMA audio buffer in RAM_D2 — 256 × i16 = 128 stereo pairs.
/// DMA reads one half while ISR fills the other.
/// Note: RAM_D2 is NOLOAD, so this initializer is not applied by startup code.
/// `prefill_buffer()` must be called before DMA starts.
#[unsafe(link_section = ".ram_d2")]
static mut AUDIO_BUF: [i16; 256] = [0; 256];

/// Phase accumulator for sine generation — only accessed from DMA ISR.
static mut SINE_PHASE: u32 = 0;

/// 256-entry sine lookup table (i32, 50% amplitude).
static SINE_TABLE: [i32; 256] = {
    let mut table = [0i32; 256];
    let mut i = 0;
    while i < 256 {
        let t = i as f64 / 256.0;
        let x = t * 2.0 * 3.14159265358979323846;
        let x = x - (6.28318530717958647692 * ((x / 6.28318530717958647692 + 0.5) as i64 as f64));
        let x2 = x * x;
        let x3 = x2 * x;
        let x5 = x3 * x2;
        let x7 = x5 * x2;
        let x9 = x7 * x2;
        let x11 = x9 * x2;
        let s = x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0 + x9 / 362880.0 - x11 / 39916800.0;
        table[i] = (s * 0.5 * 2147483647.0) as i32;
        i += 1;
    }
    table
};

/// Fill 128 i16 samples (64 stereo pairs) starting at `offset` in AUDIO_BUF.
fn fill_sine_buffer(offset: usize) {
    // SAFETY: only accessed from single non-reentrant ISR (and prefill before DMA starts)
    unsafe {
        let phase_inc: u32 = 39_472_883; // 440 Hz at 47917 Hz
        for i in (0..128).step_by(2) {
            let idx = (SINE_PHASE >> 24) as usize;
            let sample = (SINE_TABLE[idx] >> 16) as i16;
            AUDIO_BUF[offset + i] = sample;     // left
            AUDIO_BUF[offset + i + 1] = sample; // right
            SINE_PHASE = SINE_PHASE.wrapping_add(phase_inc);
        }
    }
}

/// Pre-fill the entire AUDIO_BUF before DMA starts.
pub fn prefill_buffer() {
    fill_sine_buffer(0);
    fill_sine_buffer(128);
}

/// Configure PLL3 to produce the SAI audio clock.
pub fn init_pll3() {
    let rcc = unsafe { &*pac::RCC::ptr() };

    // 1. Enable SAI1 peripheral clock
    rcc.apb2enr.modify(|_, w| w.sai1en().enabled());
    cortex_m::asm::delay(100);

    // 2. Disable PLL3
    rcc.cr.modify(|_, w| w.pll3on().off());
    while rcc.cr.read().pll3rdy().is_ready() {}

    // 3. Set PLL3 input divider: DIVM3 = 1 (preserve DIVM1/DIVM2)
    rcc.pllckselr.modify(|_, w| unsafe { w.divm3().bits(1) });

    // 4. Set PLL3 multiplier and dividers: N=46 (val 45), P=3 (val 2)
    rcc.pll3divr.write(|w| unsafe {
        w.divn3().bits(45)
         .divp3().bits(2)
         .divq3().bits(1)
         .divr3().bits(1)
    });

    // 5. Configure PLL3: wide VCO range, input range 8-16 MHz, enable P output
    rcc.pllcfgr.modify(|_, w| {
        w.pll3vcosel().wide_vco()
         .pll3rge().range8()
         .divp3en().enabled()
    });

    // 6. Enable PLL3
    rcc.cr.modify(|_, w| w.pll3on().on());
    while !rcc.cr.read().pll3rdy().is_ready() {}

    // 7. Set SAI1 clock source to PLL3_P (0b010)
    rcc.d2ccip1r.modify(|_, w| unsafe { w.sai1sel().bits(0b010) });
}

/// Configure SAI1 Block A as I2S master TX, 16-bit stereo.
pub fn init_sai1a() {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    let cha = sai1.cha();

    // Disable SAI before configuration
    cha.cr1.modify(|_, w| w.saien().clear_bit());
    while cha.cr1.read().saien().bit_is_set() {}

    // CR1: Master TX, Free I2S, 16-bit, MCKDIV=5
    cha.cr1.write(|w| unsafe {
        w.mode().bits(0b00)      // Master TX
         .prtcfg().bits(0b00)    // Free protocol (I2S)
         .ds().bits(0b100)       // 16-bit data
         .mckdiv().bits(5)       // MCLK divider
    });

    // Set MCKEN (bit 27) via raw register — not in PAC
    unsafe {
        let cr1 = core::ptr::read_volatile(SAI1_CHA_CR1);
        core::ptr::write_volatile(SAI1_CHA_CR1, cr1 | (1 << 27));
    }

    // CR2: FIFO threshold 1/4, flush FIFO
    cha.cr2.write(|w| unsafe {
        w.fth().bits(0b001)
         .fflush().set_bit()
    });

    // FRCR: 32-bit frame, FS active 16 bits
    cha.frcr.write(|w| unsafe {
        w.frl().bits(31)         // Frame length = 32 bits
         .fsall().bits(15)       // FS active for 16 bits
         .fsdef().set_bit()      // FS is channel identification
         .fspol().clear_bit()    // FS active low
         .fsoff().set_bit()      // FS one bit before first data (I2S standard)
    });

    // SLOTR: 2 slots, both active, 16-bit slot size
    cha.slotr.write(|w| unsafe {
        w.nbslot().bits(1)       // 2 slots (N-1)
         .sloten().bits(0b0011)  // Slots 0 and 1 active
         .slotsz().bits(0b01)    // 16-bit slot size
    });

    // Enable SAI
    cha.cr1.modify(|_, w| w.saien().set_bit());
}

/// Check if the SAI1_A FIFO has room for more data.
#[inline]
pub fn sai_fifo_has_room() -> bool {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    sai1.cha().sr.read().flvl().bits() < 4
}

/// Write a 16-bit sample to the SAI1_A FIFO (packed in lower 16 bits of u32).
#[inline]
pub fn write_sai_data(sample: i16) {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    sai1.cha().dr.write(|w| unsafe { w.data().bits(sample as u16 as u32) });
}
