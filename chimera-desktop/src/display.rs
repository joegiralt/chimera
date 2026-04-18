use chimera_hal::{ChimeraDisplay, SCREEN_WIDTH, SCREEN_HEIGHT, FB_SIZE};
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::Pixel;
use minifb::{Key, Window, WindowOptions};

const SCALE: usize = 2;

pub struct DesktopDisplay {
    window: Window,
    fb: Vec<u16>,
    window_buf: Vec<u32>,
}

impl DesktopDisplay {
    pub fn new() -> Self {
        let window = Window::new(
            "Chimera",
            SCREEN_WIDTH as usize * SCALE,
            SCREEN_HEIGHT as usize * SCALE,
            WindowOptions::default(),
        )
        .expect("failed to create window");

        Self {
            window,
            fb: vec![0u16; FB_SIZE],
            window_buf: vec![0u32; FB_SIZE * SCALE * SCALE],
        }
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    pub fn get_keys(&self) -> Vec<Key> {
        self.window.get_keys()
    }

    /// Convert RGB565 framebuffer to scaled RGB888 for minifb
    fn convert_fb(&mut self) {
        let w = SCREEN_WIDTH as usize;
        let sw = w * SCALE;
        for y in 0..SCREEN_HEIGHT as usize {
            for x in 0..w {
                let pixel = self.fb[y * w + x];
                let r = ((pixel >> 11) & 0x1F) as u32;
                let g = ((pixel >> 5) & 0x3F) as u32;
                let b = (pixel & 0x1F) as u32;
                let rgb = (r << 19) | (g << 10) | (b << 3);
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        self.window_buf[(y * SCALE + dy) * sw + x * SCALE + dx] = rgb;
                    }
                }
            }
        }
    }
}

impl DrawTarget for DesktopDisplay {
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

impl OriginDimensions for DesktopDisplay {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

impl ChimeraDisplay for DesktopDisplay {
    fn flush(&mut self) {
        self.convert_fb();
        self.window
            .update_with_buffer(
                &self.window_buf,
                SCREEN_WIDTH as usize * SCALE,
                SCREEN_HEIGHT as usize * SCALE,
            )
            .expect("failed to update window");
    }

    fn pixel_buffer(&mut self) -> &mut [u16] {
        &mut self.fb
    }
}
