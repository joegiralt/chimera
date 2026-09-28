//! Boot splash: the Yellow Sign, ground-black on yellow.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

use super::theme;

/// Side of the square bitmap, in pixels.
const SIZE: usize = 192;

/// 1 bit per pixel, row-major, MSB first, set = ink. ponytail: baked from
/// the owner's Elektronauts avatar (144 px JPEG, Lanczos to 192, luma < 128);
/// re-bake from a vector source if it ever needs to be sharper.
static SIGN: &[u8; SIZE * SIZE / 8] = include_bytes!("yellow_sign.bin");

/// The avatar's yellow, #f1e579.
const YELLOW: Rgb565 = Rgb565::new(241 >> 3, 229 >> 2, 121 >> 3);

fn ink(x: usize, y: usize) -> bool {
    let n = y * SIZE + x;
    SIGN[n / 8] & (0x80 >> (n % 8)) != 0
}

/// Fill the target yellow and centre the sign on it. The ink is `theme::BG`,
/// so the palette's BLACK setting applies to it like any other ground.
pub fn draw<D: DrawTarget<Color = Rgb565>>(d: &mut D) -> Result<(), D::Error> {
    d.clear(YELLOW)?;
    let origin = d.bounding_box().center() - Point::new(SIZE as i32 / 2, SIZE as i32 / 2);
    d.draw_iter(
        (0..SIZE * SIZE)
            .map(|n| (n % SIZE, n / SIZE))
            .filter(|&(x, y)| ink(x, y))
            .map(|(x, y)| Pixel(origin + Point::new(x as i32, y as i32), theme::BG)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_ground_and_the_ring_is_ink() {
        assert!(!ink(0, 0) && !ink(SIZE - 1, SIZE - 1));
        assert!(
            !ink(SIZE / 2, SIZE / 2 - 60),
            "inside the ring, above the glyph"
        );
        assert!((0..SIZE / 8).any(|y| ink(SIZE / 2, y)), "the ring's top");
    }
}
