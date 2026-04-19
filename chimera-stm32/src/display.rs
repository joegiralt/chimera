//! ILI9341 display driver via SPI1.
//! 240x320 RGB565, framebuffer in D1 AXI-SRAM.

use chimera_hal::{ChimeraDisplay, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::Pixel;
use stm32h7xx_hal::gpio::{Output, PushPull};
use stm32h7xx_hal::hal::digital::v2::OutputPin;
use stm32h7xx_hal::spi::Spi;

type SpiType = Spi<stm32h7xx_hal::pac::SPI1, stm32h7xx_hal::spi::Enabled>;
type DcPin = stm32h7xx_hal::gpio::PD8<Output<PushPull>>;
type ResetPin = stm32h7xx_hal::gpio::PD9<Output<PushPull>>;
type CsPin = stm32h7xx_hal::gpio::PD10<Output<PushPull>>;

pub struct Stm32Display {
    spi: SpiType,
    dc: DcPin,
    reset: ResetPin,
    cs: CsPin,
    fb: [u16; FB_SIZE],
}

impl Stm32Display {
    pub fn new(spi: SpiType, dc: DcPin, reset: ResetPin, cs: CsPin) -> Self {
        Self {
            spi,
            dc,
            reset,
            cs,
            fb: [0u16; FB_SIZE],
        }
    }

    /// Send a command byte to the ILI9341.
    fn cmd(&mut self, cmd: u8) {
        let _ = self.dc.set_low(); // command mode
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[cmd]);
        let _ = self.cs.set_high();
    }

    /// Send data bytes to the ILI9341.
    fn data(&mut self, data: &[u8]) {
        let _ = self.dc.set_high(); // data mode
        let _ = self.cs.set_low();
        let _ = self.spi.write(data);
        let _ = self.cs.set_high();
    }

    /// Send command followed by data.
    fn cmd_data(&mut self, cmd: u8, data: &[u8]) {
        self.cmd(cmd);
        self.data(data);
    }

    /// Initialize the ILI9341 display.
    /// Sequence from PreenFM3 ili9341.c
    pub fn init(&mut self) {
        // Hardware reset
        let _ = self.reset.set_low();
        cortex_m::asm::delay(480_000 * 10); // ~10ms
        let _ = self.reset.set_high();
        cortex_m::asm::delay(480_000 * 120); // ~120ms

        // Software reset
        self.cmd(0x01);
        cortex_m::asm::delay(480_000 * 10);

        // Power control
        self.cmd_data(0xCB, &[0x39, 0x2C, 0x00, 0x34, 0x02]);
        self.cmd_data(0xCF, &[0x00, 0xC1, 0x30]);
        self.cmd_data(0xE8, &[0x85, 0x00, 0x78]);
        self.cmd_data(0xEA, &[0x00, 0x00]);
        self.cmd_data(0xED, &[0x64, 0x03, 0x12, 0x81]);
        self.cmd_data(0xF7, &[0x20]);
        self.cmd_data(0xC0, &[0x23]); // VRH
        self.cmd_data(0xC1, &[0x10]); // SAP/BT
        self.cmd_data(0xC5, &[0x3E, 0x28]); // VCM control
        self.cmd_data(0xC7, &[0x86]); // VCM control 2

        // Memory access control
        self.cmd_data(0x36, &[0x48]); // MX | BGR

        // Pixel format: 16-bit RGB565
        self.cmd_data(0x3A, &[0x55]);

        // Frame rate
        self.cmd_data(0xB1, &[0x00, 0x18]);

        // Display function control
        self.cmd_data(0xB6, &[0x08, 0x82, 0x27]);

        // Gamma
        self.cmd_data(0xF2, &[0x00]); // 3Gamma disable
        self.cmd_data(0x26, &[0x01]); // Gamma curve 1

        // Sleep out
        self.cmd(0x11);
        cortex_m::asm::delay(480_000 * 120);

        // Display on
        self.cmd(0x29);
    }

    /// Set the drawing window for a full-screen write.
    fn set_window(&mut self) {
        // Column address set (0 to 239)
        self.cmd_data(0x2A, &[0x00, 0x00, 0x00, 0xEF]);
        // Row address set (0 to 319)
        self.cmd_data(0x2B, &[0x00, 0x00, 0x01, 0x3F]);
        // Memory write
        self.cmd(0x2C);
    }
}

impl DrawTarget for Stm32Display {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(point, color) in pixels {
            let x = point.x;
            let y = point.y;
            if x >= 0 && x < SCREEN_WIDTH as i32 && y >= 0 && y < SCREEN_HEIGHT as i32 {
                let idx = (y as usize) * (SCREEN_WIDTH as usize) + (x as usize);
                self.fb[idx] = RawU16::from(color).into_inner();
            }
        }
        Ok(())
    }
}

impl OriginDimensions for Stm32Display {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

impl ChimeraDisplay for Stm32Display {
    fn flush(&mut self) {
        self.set_window();

        // Send framebuffer as RGB565 over SPI
        // TODO: use DMA for non-blocking transfer
        let _ = self.dc.set_high();
        let _ = self.cs.set_low();

        // Send in chunks (SPI has limited buffer)
        for chunk in self.fb.chunks(256) {
            let mut bytes = [0u8; 512];
            for (i, &pixel) in chunk.iter().enumerate() {
                bytes[i * 2] = (pixel >> 8) as u8;
                bytes[i * 2 + 1] = pixel as u8;
            }
            let _ = self.spi.write(&bytes[..chunk.len() * 2]);
        }

        let _ = self.cs.set_high();
    }

    fn pixel_buffer(&mut self) -> &mut [u16] {
        &mut self.fb
    }
}
