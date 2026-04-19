//! Fast bit-bang SPI using direct register writes.
//! Uses BSRR (bit set/reset register) for single-cycle GPIO toggles
//! instead of the HAL's OutputPin trait which has overhead.

use stm32h7xx_hal::hal::blocking::spi::Write;
use stm32h7xx_hal::hal::digital::v2::OutputPin;

pub struct BitBangSpi<SCK, MOSI> {
    sck: SCK,
    mosi: MOSI,
    // Direct register pointers for fast path
    gpioa_bsrr: *mut u32,
    sck_set: u32,
    sck_reset: u32,
    mosi_set: u32,
    mosi_reset: u32,
}

// SAFETY: GPIO registers are memory-mapped and accessed from a single thread (main loop)
unsafe impl<SCK, MOSI> Send for BitBangSpi<SCK, MOSI> {}

impl<SCK, MOSI> BitBangSpi<SCK, MOSI>
where
    SCK: OutputPin,
    MOSI: OutputPin,
{
    /// Create a new BitBangSpi.
    /// sck_pin and mosi_pin are the pin numbers (e.g. 5 for PA5, 7 for PA7).
    /// gpio_base is the base address of the GPIO port (e.g. 0x5802_0000 for GPIOA).
    pub fn new(sck: SCK, mosi: MOSI, gpio_base: u32, sck_pin: u8, mosi_pin: u8) -> Self {
        let bsrr = (gpio_base + 0x18) as *mut u32;
        Self {
            sck,
            mosi,
            gpioa_bsrr: bsrr,
            sck_set: 1 << sck_pin,
            sck_reset: 1 << (sck_pin + 16),
            mosi_set: 1 << mosi_pin,
            mosi_reset: 1 << (mosi_pin + 16),
        }
    }

    /// Send one byte using direct register writes — extremely fast.
    #[inline(always)]
    fn send_byte_fast(&self, byte: u8) {
        let bsrr = self.gpioa_bsrr;
        for bit in (0..8u8).rev() {
            // SCK low + set MOSI in one write
            let mosi_val = if byte & (1 << bit) != 0 {
                self.mosi_set
            } else {
                self.mosi_reset
            };
            // SAFETY: BSRR is a write-only register, single-threaded access
            unsafe {
                core::ptr::write_volatile(bsrr, self.sck_reset | mosi_val);
                core::ptr::write_volatile(bsrr, self.sck_set);
            }
        }
    }
}

impl<SCK, MOSI> Write<u8> for BitBangSpi<SCK, MOSI>
where
    SCK: OutputPin,
    MOSI: OutputPin,
{
    type Error = core::convert::Infallible;

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        for &byte in words {
            self.send_byte_fast(byte);
        }
        Ok(())
    }
}
