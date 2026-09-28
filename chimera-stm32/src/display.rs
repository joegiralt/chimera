//! ILI9341 display driver via SPI1.
//! 240x320 RGB565, framebuffer in static BSS (too large for stack).

use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::ui::theme_settings::{GammaTables, Palette};
use chimera_hal::{ChimeraDisplay, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};
use embedded_graphics_core::Pixel;
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use stm32h7xx_hal::hal::blocking::spi::Write;
use stm32h7xx_hal::hal::digital::v2::OutputPin;

static mut FRAMEBUFFER: [u16; FB_SIZE] = [0u16; FB_SIZE];
static FRAMEBUFFER_TAKEN: AtomicBool = AtomicBool::new(false);

/// The framebuffer, once: `Stm32Display` owns it from then on.
pub fn take_framebuffer() -> Option<&'static mut [u16; FB_SIZE]> {
    if FRAMEBUFFER_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `FRAMEBUFFER` ever made.
    Some(unsafe { &mut *addr_of_mut!(FRAMEBUFFER) })
}

pub struct Stm32Display<SPI, DC, RST, CS> {
    spi: SPI,
    dc: DC,
    reset: RST,
    cs: CS,
    fb: &'static mut [u16; FB_SIZE],
    /// System › Theme's colours, swapped in as pixels go out; the
    /// framebuffer keeps the canonical palette.
    palette: Palette,
}

impl<SPI, DC, RST, CS> Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, cs: CS, fb: &'static mut [u16; FB_SIZE]) -> Self {
        Self {
            spi,
            dc,
            reset,
            cs,
            fb,
            palette: Palette::IDENTITY,
        }
    }

    /// Show the framebuffer through `palette` from the next flush on.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Positive (E0h) and negative (E1h) gamma correction, live.
    pub fn set_gamma(&mut self, t: &GammaTables) {
        self.cmd_data(0xE0, &t.positive);
        self.cmd_data(0xE1, &t.negative);
    }

    /// Push framebuffer pixels `start..end` (already windowed) through the palette.
    fn write_pixels(&mut self, start: usize, end: usize) {
        let _ = self.dc.set_high();
        let _ = self.cs.set_low();
        let mut bytes = [0u8; 512];
        for chunk in self.fb[start..end].chunks(256) {
            for (i, &pixel) in chunk.iter().enumerate() {
                let [hi, lo] = self.palette.map_raw(pixel).to_be_bytes();
                bytes[i * 2] = hi;
                bytes[i * 2 + 1] = lo;
            }
            let _ = self.spi.write(&bytes[..chunk.len() * 2]);
        }
        let _ = self.cs.set_high();
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

    /// ILI9341 init sequence (from PreenFM3 ili9341.c). Leaves gamma at the
    /// panel's reset tables (THEME's PANEL); the caller pushes the boot
    /// theme's actual gamma right after.
    pub fn init(&mut self, cpu_hz: u32) {
        let _ = self.reset.set_low();
        crate::clocks::delay_us(cpu_hz, 12_500);
        let _ = self.reset.set_high();
        crate::clocks::delay_us(cpu_hz, 150_000);

        self.cmd(0x01);
        crate::clocks::delay_us(cpu_hz, 12_500);

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
        self.cmd_data(0xB1, &[0x00, 0x18]);
        self.cmd_data(0xB6, &[0x08, 0x82, 0x27]);
        self.cmd_data(0xF2, &[0x00]);
        self.cmd_data(0x26, &[0x01]);
        self.cmd(0x11);
        crate::clocks::delay_us(cpu_hz, 150_000);
        self.cmd(0x29);
    }

    fn set_window(&mut self) {
        self.cmd_data(0x2A, &[0x00, 0x00, 0x00, 0xEF]);
        self.cmd_data(0x2B, &[0x00, 0x00, 0x01, 0x3F]);
        self.cmd(0x2C);
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

    /// A fill of the framebuffer, not 76,800 `draw_iter` pixels.
    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        self.fb.fill(RawU16::from(color).into_inner());
        Ok(())
    }

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
        self.write_pixels(0, FB_SIZE);
    }

    fn flush_region(&mut self, y_start: u16, y_end: u16) {
        self.cmd_data(0x2A, &[0x00, 0x00, 0x00, 0xEF]);
        let ys = y_start.to_be_bytes();
        let ye = (y_end - 1).to_be_bytes();
        self.cmd_data(0x2B, &[ys[0], ys[1], ye[0], ye[1]]);
        self.cmd(0x2C);
        self.write_pixels(
            y_start as usize * SCREEN_WIDTH as usize,
            y_end as usize * SCREEN_WIDTH as usize,
        );
    }

    fn pixel_buffer(&mut self) -> &mut [u16] {
        self.fb
    }
}
