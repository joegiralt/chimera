//! Hardware SPI1 driver via direct PAC register access.
//! STM32H7 SPI requires TSIZE set per transfer and CSTART for each transaction.

use stm32h7xx_hal::hal::blocking::spi::Write;
use stm32h7xx_hal::pac;

// SPI1 register addresses
const SPI1_BASE: u32 = 0x4001_3000;
const SPI1_TXDR: *mut u8 = (SPI1_BASE + 0x20) as *mut u8;

/// Hardware SPI1 driver. Pins must be configured as AF5 before use.
pub struct HwSpi;

impl HwSpi {
    /// Initialize SPI1 hardware. Pins must already be configured as AF5.
    pub fn init() -> Self {
        // SAFETY: single-threaded init, SPI1 peripheral access
        let rcc = unsafe { &*pac::RCC::ptr() };
        let spi1 = unsafe { &*pac::SPI1::ptr() };

        // Enable SPI1 clock on APB2
        rcc.apb2enr.modify(|_, w| w.spi1en().enabled());
        cortex_m::asm::delay(100);

        // Disable SPI before configuration
        spi1.cr1.write(|w| w);  // clear all CR1 bits
        while spi1.cr1.read().spe().bit_is_set() {}

        // CFG1: 8-bit data, prescaler /16 (~6.25 MHz), FIFO threshold = 1 byte
        spi1.cfg1.write(|w| unsafe {
            w.dsize().bits(7)     // 8-bit data (N-1)
             .mbr().bits(0b011)   // prescaler /16
             .fthlv().bits(0)     // FIFO threshold: 1 data
        });

        // CFG2: master, full-duplex, CPOL=0, CPHA=0, MSB first, SW NSS, AFCNTR
        spi1.cfg2.write(|w| {
            w.master().master()
             .ssm().enabled()       // software slave select
             .ssoe().disabled()     // no HW SS output
             .afcntr().controlled() // keep AF control during disable
             .cpol().idle_low()
             .cpha().first_edge()
             .lsbfrst().msbfirst()
        });

        // CR1: set SSI high (required for master with SSM), then enable
        spi1.cr1.write(|w| w.ssi().set_bit());
        spi1.cr1.modify(|_, w| w.spe().set_bit());

        Self
    }

    /// Transfer a block of bytes. Sets TSIZE, starts transfer, writes bytes, waits for completion.
    fn transfer_bytes(&self, data: &[u8]) {
        if data.is_empty() { return; }

        let spi1 = unsafe { &*pac::SPI1::ptr() };

        // Set transfer size
        let len = data.len().min(65535) as u16;
        spi1.cr2.write(|w| w.tsize().bits(len));

        // Start the transfer
        spi1.cr1.modify(|_, w| w.cstart().set_bit());

        // Write each byte
        for &byte in data {
            // Wait for TXP (TX FIFO not full)
            while !spi1.sr.read().txp().bit_is_set() {}
            // SAFETY: 8-bit write to SPI1 TXDR
            unsafe { core::ptr::write_volatile(SPI1_TXDR, byte); }
        }

        // Wait for end of transfer (EOT)
        while !spi1.sr.read().eot().bit_is_set() {}

        // Clear EOT flag
        spi1.ifcr.write(|w| w.eotc().set_bit());
    }
}

impl Write<u8> for HwSpi {
    type Error = core::convert::Infallible;

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        // STM32H7 SPI has max TSIZE of 65535. Split larger transfers.
        for chunk in words.chunks(65535) {
            self.transfer_bytes(chunk);
        }
        Ok(())
    }
}
