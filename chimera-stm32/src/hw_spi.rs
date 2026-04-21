//! Hardware SPI1 driver matching PreenFM3's exact HAL_SPI_Transmit sequence.
//! Key: SPI is disabled/re-enabled for each transfer. TSIZE set before CSTART.

use stm32h7xx_hal::hal::blocking::spi::Write;
use stm32h7xx_hal::pac;

const SPI1_TXDR: *mut u8 = 0x4001_3020 as *mut u8;

pub struct HwSpi;

impl HwSpi {
    pub fn init() -> Self {
        // SAFETY: single-threaded init
        let rcc = unsafe { &*pac::RCC::ptr() };
        let spi1 = unsafe { &*pac::SPI1::ptr() };

        // SPI1 kernel clock = PLL3_P (122.67 MHz, already running for SAI)
        // PreenFM3 uses PLL2, but we don't have PLL2 configured.
        // PLL3_P is available. With prescaler /4 = ~30 MHz.
        rcc.d2ccip1r.modify(|_, w| w.spi123sel().pll3_p());

        // Enable SPI1 peripheral clock
        rcc.apb2enr.modify(|_, w| w.spi1en().enabled());
        cortex_m::asm::delay(1000);

        // Ensure SPI is disabled
        spi1.cr1.write(|w| w);
        cortex_m::asm::delay(100);

        // CFG1: 8-bit data, prescaler /4 (~30 MHz), FIFO threshold = 12 data (like PreenFM3)
        spi1.cfg1.write(|w| unsafe {
            w.dsize().bits(7)     // 8-bit (N-1)
             .mbr().bits(0b001)   // prescaler /4
             .fthlv().bits(0b011) // FIFO threshold: 12 data frames
        });

        // CFG2: master, full-duplex, Mode 0, MSB first, software NSS, NSSP pulse, AFCNTR
        spi1.cfg2.write(|w| {
            w.master().master()
             .ssm().enabled()
             .ssoe().disabled()
             .afcntr().controlled()  // keep AF control when SPE=0
             .cpol().idle_low()
             .cpha().first_edge()
             .lsbfrst().msbfirst()
             .ssom().not_asserted()  // SS not asserted between frames
        });

        // Leave SPI disabled — it gets enabled per-transfer (matching HAL behavior)
        // Just set SSI high so master mode works
        spi1.cr1.write(|w| w.ssi().set_bit());

        Self
    }
}

impl Write<u8> for HwSpi {
    type Error = core::convert::Infallible;

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        if words.is_empty() { return Ok(()); }

        let spi1 = unsafe { &*pac::SPI1::ptr() };

        // Split into 65535-byte chunks (TSIZE max)
        for chunk in words.chunks(65535) {
            let len = chunk.len() as u16;

            // 1. Set TSIZE (number of data frames to transfer)
            spi1.cr2.write(|w| w.tsize().bits(len));

            // 2. Enable SPI (SPE = 1), keep SSI high
            spi1.cr1.modify(|_, w| w.spe().set_bit());

            // 3. Start transfer (CSTART = 1)
            spi1.cr1.modify(|_, w| w.cstart().set_bit());

            // 4. Write data bytes
            for &byte in chunk {
                // Wait for TXP (FIFO has room)
                while !spi1.sr.read().txp().bit_is_set() {}
                // SAFETY: 8-bit write to SPI1 TXDR
                unsafe { core::ptr::write_volatile(SPI1_TXDR, byte); }
            }

            // 5. Wait for EOT (end of transfer)
            while !spi1.sr.read().eot().bit_is_set() {}

            // 6. Clear flags (matching HAL_SPI_CloseTransfer)
            spi1.ifcr.write(|w| {
                w.eotc().set_bit()   // clear EOT
                 .txtfc().set_bit()  // clear TXTF
            });

            // 7. Disable SPI (matching HAL — re-enabled next transfer)
            spi1.cr1.modify(|_, w| w.spe().clear_bit());
        }

        Ok(())
    }
}
