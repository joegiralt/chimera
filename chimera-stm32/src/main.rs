#![no_std]
#![no_main]

mod audio;
mod controls;
mod display;
mod midi;

use cortex_m_rt::entry;
use panic_halt as _;
use stm32h7xx_hal::{pac, prelude::*, spi};
use stm32h7xx_hal::hal::blocking::spi::Write;

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();

    // Default clocks
    let pwr = dp.PWR.constrain();
    let pwrcfg = pwr.freeze();
    let rcc = dp.RCC.constrain();
    let ccdr = rcc.freeze(pwrcfg, &dp.SYSCFG);

    // GPIO
    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);

    let mut led = gpioe.pe1.into_push_pull_output();
    let mut dc = gpiod.pd8.into_push_pull_output();
    let mut reset = gpiod.pd9.into_push_pull_output();
    let mut cs = gpiod.pd10.into_push_pull_output();

    // TFT Backlight on PE11 — just set HIGH (no PWM needed for test)
    let mut backlight = gpioe.pe11.into_push_pull_output();
    backlight.set_high();

    // SPI1
    let mut spi = dp.SPI1.spi(
        (
            gpioa.pa5.into_alternate::<5>(),
            gpioa.pa6.into_alternate::<5>(),
            gpioa.pa7.into_alternate::<5>(),
        ),
        spi::MODE_0,
        2.MHz(),
        ccdr.peripheral.SPI1,
        &ccdr.clocks,
    );

    led.set_high(); // alive

    // === ILI9341 Init (matching PreenFM3 exactly) ===

    // Reset sequence: CS high first, then select after reset
    cs.set_high();  // unselect
    reset.set_low();
    cortex_m::asm::delay(5_000_000);
    cs.set_low();   // select — CS stays low from here
    reset.set_high();
    cortex_m::asm::delay(30_000_000);

    // Software reset
    dc.set_low();
    let _ = spi.write(&[0x01]);
    cortex_m::asm::delay(5_000_000);

    // Power control A
    dc.set_low();
    let _ = spi.write(&[0xCB]);
    dc.set_high();
    let _ = spi.write(&[0x39, 0x2C, 0x00, 0x34, 0x02]);

    // Power control B
    dc.set_low();
    let _ = spi.write(&[0xCF]);
    dc.set_high();
    let _ = spi.write(&[0x00, 0xC1, 0x30]);

    // Driver timing A
    dc.set_low();
    let _ = spi.write(&[0xE8]);
    dc.set_high();
    let _ = spi.write(&[0x85, 0x00, 0x78]);

    // Driver timing B
    dc.set_low();
    let _ = spi.write(&[0xEA]);
    dc.set_high();
    let _ = spi.write(&[0x00, 0x00]);

    // Power on sequence
    dc.set_low();
    let _ = spi.write(&[0xED]);
    dc.set_high();
    let _ = spi.write(&[0x64, 0x03, 0x12, 0x81]);

    // Pump ratio
    dc.set_low();
    let _ = spi.write(&[0xF7]);
    dc.set_high();
    let _ = spi.write(&[0x20]);

    // Power control VRH
    dc.set_low();
    let _ = spi.write(&[0xC0]);
    dc.set_high();
    let _ = spi.write(&[0x23]);

    // Power control SAP/BT
    dc.set_low();
    let _ = spi.write(&[0xC1]);
    dc.set_high();
    let _ = spi.write(&[0x10]);

    // VCM control
    dc.set_low();
    let _ = spi.write(&[0xC5]);
    dc.set_high();
    let _ = spi.write(&[0x3E, 0x28]);

    // VCM control 2
    dc.set_low();
    let _ = spi.write(&[0xC7]);
    dc.set_high();
    let _ = spi.write(&[0x86]);

    // MADCTL
    dc.set_low();
    let _ = spi.write(&[0x36]);
    dc.set_high();
    let _ = spi.write(&[0x48]);

    // Pixel format 16-bit
    dc.set_low();
    let _ = spi.write(&[0x3A]);
    dc.set_high();
    let _ = spi.write(&[0x55]);

    // Frame rate
    dc.set_low();
    let _ = spi.write(&[0xB1]);
    dc.set_high();
    let _ = spi.write(&[0x00, 0x18]);

    // Display function
    dc.set_low();
    let _ = spi.write(&[0xB6]);
    dc.set_high();
    let _ = spi.write(&[0x08, 0x82, 0x27]);

    // Gamma disable
    dc.set_low();
    let _ = spi.write(&[0xF2]);
    dc.set_high();
    let _ = spi.write(&[0x00]);

    // Gamma curve 1
    dc.set_low();
    let _ = spi.write(&[0x26]);
    dc.set_high();
    let _ = spi.write(&[0x01]);

    // Sleep out
    dc.set_low();
    let _ = spi.write(&[0x11]);
    cortex_m::asm::delay(60_000_000);

    // Display on
    dc.set_low();
    let _ = spi.write(&[0x29]);
    cortex_m::asm::delay(5_000_000);

    // CS stays low (PreenFM3 behavior)

    led.set_low();
    cortex_m::asm::delay(5_000_000);
    led.set_high(); // LED on = init done

    // === Fill screen with cyan ===

    // Column address set 0-239
    dc.set_low();
    let _ = spi.write(&[0x2A]);
    dc.set_high();
    let _ = spi.write(&[0x00, 0x00, 0x00, 0xEF]);

    // Row address set 0-319
    dc.set_low();
    let _ = spi.write(&[0x2B]);
    dc.set_high();
    let _ = spi.write(&[0x00, 0x00, 0x01, 0x3F]);

    // Memory write
    dc.set_low();
    let _ = spi.write(&[0x2C]);
    dc.set_high();

    // Test 1: Invert display — if this works, commands are getting through
    dc.set_low();
    let _ = spi.write(&[0x21]); // Display inversion ON
    cortex_m::asm::delay(5_000_000);

    // Test 2: Write pixels
    dc.set_low();
    let _ = spi.write(&[0x2A]); // Column address
    dc.set_high();
    let _ = spi.write(&[0x00, 0x00, 0x00, 0xEF]);

    dc.set_low();
    let _ = spi.write(&[0x2B]); // Row address
    dc.set_high();
    let _ = spi.write(&[0x00, 0x00, 0x01, 0x3F]);

    dc.set_low();
    let _ = spi.write(&[0x2C]); // Memory write

    // Send red pixels (0xF800) — small test area
    dc.set_high();
    let red = [0xF8u8, 0x00];
    for _ in 0..1000 {
        let _ = spi.write(&red);
    }

    // Slow blink = done
    loop {
        led.set_high();
        cortex_m::asm::delay(16_000_000);
        led.set_low();
        cortex_m::asm::delay(16_000_000);
    }
}
