mod display;

use chimera_hal::ChimeraDisplay;
use display::DesktopDisplay;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

fn main() {
    let mut display = DesktopDisplay::new();

    // Draw a test pattern: red, green, blue bars
    Rectangle::new(Point::new(0, 0), Size::new(240, 107))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
        .draw(&mut display)
        .unwrap();

    Rectangle::new(Point::new(0, 107), Size::new(240, 106))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::GREEN))
        .draw(&mut display)
        .unwrap();

    Rectangle::new(Point::new(0, 213), Size::new(240, 107))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLUE))
        .draw(&mut display)
        .unwrap();

    display.flush();

    while display.is_open() {
        display.flush();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
