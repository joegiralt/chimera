//! ILI9341 display driver via SPI1.
//! 240x320 RGB565, framebuffer in AXI-SRAM.

use chimera_hal::{ChimeraDisplay, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::Pixel;
use stm32h7xx_hal::hal::blocking::spi::Write;
use stm32h7xx_hal::hal::digital::v2::OutputPin;

pub struct Stm32Display<SPI, DC, RST, CS> {
    spi: SPI,
    dc: DC,
    reset: RST,
    cs: CS,
    fb: [u16; FB_SIZE],
}

impl<SPI, DC, RST, CS> Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, cs: CS) -> Self {
        Self {
            spi,
            dc,
            reset,
            cs,
            fb: [0u16; FB_SIZE],
        }
    }

    fn cmd(&mut self, cmd: u8) {
        let _ = self.dc.set_low();
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[cmd]);
        let _ = self.cs.set_high();
    }

    fn data_bytes(&mut self, data: &[u8]) {
        let _ = self.dc.set_high();
        let _ = self.cs.set_low();
        let _ = self.spi.write(data);
        let _ = self.cs.set_high();
    }

    fn cmd_data(&mut self, cmd: u8, data: &[u8]) {
        self.cmd(cmd);
        self.data_bytes(data);
    }

    /// ILI9341 init sequence (from PreenFM3 ili9341.c)
    pub fn init(&mut self) {
        // Hardware reset
        let _ = self.reset.set_low();
        cortex_m::asm::delay(5_000_000); // ~10ms at 480MHz
        let _ = self.reset.set_high();
        cortex_m::asm::delay(60_000_000); // ~120ms

        self.cmd(0x01); // Software reset
        cortex_m::asm::delay(5_000_000);

        // Power control
        self.cmd_data(0xCB, &[0x39, 0x2C, 0x00, 0x34, 0x02]);
        self.cmd_data(0xCF, &[0x00, 0xC1, 0x30]);
        self.cmd_data(0xE8, &[0x85, 0x00, 0x78]);
        self.cmd_data(0xEA, &[0x00, 0x00]);
        self.cmd_data(0xED, &[0x64, 0x03, 0x12, 0x81]);
        self.cmd_data(0xF7, &[0x20]);
        self.cmd_data(0xC0, &[0x23]);
        self.cmd_data(0xC1, &[0x10]);
        self.cmd_data(0xC5, &[0x3E, 0x28]);
        self.cmd_data(0xC7, &[0x86]);
        self.cmd_data(0x36, &[0x48]); // MADCTL: MX | BGR
        self.cmd_data(0x3A, &[0x55]); // 16-bit RGB565
        self.cmd_data(0xB1, &[0x00, 0x18]); // Frame rate
        self.cmd_data(0xB6, &[0x08, 0x82, 0x27]); // Display function
        self.cmd_data(0xF2, &[0x00]); // Gamma disable
        self.cmd_data(0x26, &[0x01]); // Gamma curve 1

        self.cmd(0x11); // Sleep out
        cortex_m::asm::delay(60_000_000);
        self.cmd(0x29); // Display on
    }

    fn set_window(&mut self) {
        self.cmd_data(0x2A, &[0x00, 0x00, 0x00, 0xEF]); // Column: 0-239
        self.cmd_data(0x2B, &[0x00, 0x00, 0x01, 0x3F]); // Row: 0-319
        self.cmd(0x2C); // Memory write
    }
}

impl<SPI, DC, RST, CS> DrawTarget for Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
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

impl<SPI, DC, RST, CS> OriginDimensions for Stm32Display<SPI, DC, RST, CS> {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

impl<SPI, DC, RST, CS> ChimeraDisplay for Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    fn flush(&mut self) {
        self.set_window();
        let _ = self.dc.set_high();
        let _ = self.cs.set_low();

        // Send framebuffer as RGB565 big-endian bytes
        // Send in 512-byte chunks (256 pixels)
        let mut bytes = [0u8; 512];
        for chunk in self.fb.chunks(256) {
            for (i, &pixel) in chunk.iter().enumerate() {
                bytes[i * 2] = (pixel >> 8) as u8;     // high byte first
                bytes[i * 2 + 1] = pixel as u8;
            }
            let _ = self.spi.write(&bytes[..chunk.len() * 2]);
        }

        let _ = self.cs.set_high();
    }

    fn flush_region(&mut self, y_start: u16, y_end: u16) {
        // Column address: 0-239
        self.cmd_data(0x2A, &[0x00, 0x00, 0x00, 0xEF]);
        // Row address: y_start to y_end-1
        let ys = y_start.to_be_bytes();
        let ye = (y_end - 1).to_be_bytes();
        self.cmd_data(0x2B, &[ys[0], ys[1], ye[0], ye[1]]);
        self.cmd(0x2C); // RAMWR

        let _ = self.dc.set_high();
        let _ = self.cs.set_low();

        let start = y_start as usize * SCREEN_WIDTH as usize;
        let end = y_end as usize * SCREEN_WIDTH as usize;
        let mut bytes = [0u8; 512];
        for chunk in self.fb[start..end].chunks(256) {
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

