//! The display and controls glue both targets share (#97).

use chimera_hal::{ButtonState, FB_SIZE, SCREEN_WIDTH, draw_into_fb};
use embedded_graphics_core::Pixel;
use embedded_graphics_core::geometry::Point;
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};

#[test]
fn button_state_follows_the_two_levels() {
    assert_eq!(ButtonState::from_levels(false, true), ButtonState::Pressed);
    assert_eq!(ButtonState::from_levels(true, true), ButtonState::Held);
    assert_eq!(ButtonState::from_levels(true, false), ButtonState::Released);
    assert_eq!(ButtonState::from_levels(false, false), ButtonState::Up);
}

#[test]
fn pixels_land_row_major_and_off_screen_ones_drop() {
    let mut fb = vec![0u16; FB_SIZE];
    let red = RawU16::from(Rgb565::new(31, 0, 0)).into_inner();
    draw_into_fb(
        &mut fb,
        [
            Pixel(Point::new(3, 2), Rgb565::new(31, 0, 0)),
            Pixel(Point::new(-1, 0), Rgb565::new(31, 0, 0)),
            Pixel(Point::new(240, 0), Rgb565::new(31, 0, 0)),
            Pixel(Point::new(0, 320), Rgb565::new(31, 0, 0)),
        ],
    );
    assert_eq!(fb[2 * SCREEN_WIDTH as usize + 3], red);
    assert_eq!(fb.iter().filter(|&&p| p != 0).count(), 1);
}
