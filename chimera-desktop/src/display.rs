use chimera_core::console::Frame;
use chimera_core::ui::theme_settings::{Palette, ThemeSettings};
use chimera_hal::{ChimeraDisplay, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};
use embedded_graphics_core::Pixel;
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use minifb::{Key, Window, WindowOptions};

const SCALE: usize = 2;

pub struct DesktopDisplay {
    window: Window,
    fb: Box<[u16; FB_SIZE]>,
    window_buf: Vec<u32>,
    /// SETTINGS › PERSONALIZE › THEME: the palette swap, and BRIGHT as a
    /// dimming of the window. GAMMA has no desktop equivalent and is
    /// ignored here.
    palette: Palette,
    bright_pct: u32,
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
            fb: vec![0u16; FB_SIZE].try_into().expect("FB_SIZE pixels"),
            window_buf: vec![0u32; FB_SIZE * SCALE * SCALE],
            palette: Palette::IDENTITY,
            bright_pct: ThemeSettings::DEFAULT.bright.percent() as u32,
        }
    }

    /// Show the framebuffer with `theme`'s colours and brightness.
    pub fn set_theme(&mut self, theme: &ThemeSettings) {
        self.palette = theme.palette();
        self.bright_pct = theme.bright.percent() as u32;
    }

    /// The framebuffer and its palette, as `shot` reads them: BRIGHT is
    /// the window's, not the screen's, so it stays out.
    pub fn frame(&self) -> Frame<'_> {
        Frame {
            fb: &self.fb,
            palette: self.palette,
        }
    }

    /// One framebuffer pixel as window RGB888: palette, then brightness.
    fn to_rgb(&self, pixel: u16) -> u32 {
        let pixel = self.palette.map_raw(pixel);
        let dim = |v: u32| v * self.bright_pct / 100;
        let r = dim((((pixel >> 11) & 0x1F) as u32) << 3);
        let g = dim((((pixel >> 5) & 0x3F) as u32) << 2);
        let b = dim(((pixel & 0x1F) as u32) << 3);
        (r << 16) | (g << 8) | b
    }

    /// Convert framebuffer rows `y_start..y_end` into the scaled window buffer.
    fn convert_rows(&mut self, y_start: usize, y_end: usize) {
        let w = SCREEN_WIDTH as usize;
        let sw = w * SCALE;
        for y in y_start..y_end {
            for x in 0..w {
                let rgb = self.to_rgb(self.fb[y * w + x]);
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        self.window_buf[(y * SCALE + dy) * sw + x * SCALE + dx] = rgb;
                    }
                }
            }
        }
    }

    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    pub fn get_keys(&self) -> Vec<Key> {
        self.window.get_keys()
    }
}

impl DrawTarget for DesktopDisplay {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    /// A fill of the framebuffer, not 76,800 `draw_iter` pixels: the full
    /// render clears through this, every frame the browser is open.
    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        self.fb.fill(RawU16::from(color).into_inner());
        Ok(())
    }

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        chimera_hal::draw_into_fb(&mut self.fb, pixels);
        Ok(())
    }
}

impl OriginDimensions for DesktopDisplay {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

impl ChimeraDisplay for DesktopDisplay {
    fn flush_region(&mut self, y_start: u16, y_end: u16) {
        self.convert_rows(y_start as usize, y_end as usize);
        self.window
            .update_with_buffer(
                &self.window_buf,
                SCREEN_WIDTH as usize * SCALE,
                SCREEN_HEIGHT as usize * SCALE,
            )
            .expect("failed to update window");
    }

    fn pixel_buffer(&mut self) -> &mut [u16] {
        &mut self.fb[..]
    }
}
