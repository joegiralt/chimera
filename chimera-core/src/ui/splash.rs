// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Boot splash: the Yellow Sign, ground-black on yellow.
//! The sign is Christian Knutsson's design for Pelgrane Press, public domain since 2020 (THIRD_PARTY.md).

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

use super::theme;

/// Side of the square bitmap, in pixels.
const SIZE: usize = 192;

/// Row-major run lengths, ground first, then ink, alternating; a run
/// over 255 is split by a 0 of the other. Baked from the
/// owner's Elektronauts avatar (144 px JPEG, Lanczos to 192, luma < 128) as
/// 1 bit per pixel, then run-length coded (4.6 KB to 1 KB); re-bake from a
/// vector source if it ever needs to be sharper.
static SIGN: &[u8] = include_bytes!("yellow_sign.rle");

/// The avatar's yellow, #f1e579.
const YELLOW: Rgb565 = Rgb565::new(241 >> 3, 229 >> 2, 121 >> 3);

/// The ink's pixels, row-major, as indices into the square.
fn inked() -> impl Iterator<Item = usize> {
    SIGN.iter()
        .scan(0, |at, &run| {
            let from = *at;
            *at += run as usize;
            Some(from..*at)
        })
        .skip(1)
        .step_by(2)
        .flatten()
}

/// Fill the target yellow and centre the sign on it. The ink is `theme::BG`,
/// so the palette's BLACK setting applies to it like any other ground.
pub fn draw<D: DrawTarget<Color = Rgb565>>(d: &mut D) -> Result<(), D::Error> {
    d.clear(YELLOW)?;
    let origin = d.bounding_box().center() - Point::new(SIZE as i32 / 2, SIZE as i32 / 2);
    d.draw_iter(inked().map(|n| {
        let (x, y) = (n % SIZE, n / SIZE);
        Pixel(origin + Point::new(x as i32, y as i32), theme::BG)
    }))?;
    // The build's version and hash under the sign (#406).
    let mut rev = super::fmt::FmtBuf::new();
    let _ = super::about_page::write_build_rev(&mut rev);
    let cx = d.bounding_box().center().x;
    super::draw::text_center(
        d,
        &theme::FONT_LABEL,
        rev.as_str(),
        cx,
        origin.y + SIZE as i32 + 24,
        theme::BG,
        theme::LABEL_TRACKING,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(x: usize, y: usize) -> bool {
        inked().any(|n| n == y * SIZE + x)
    }

    #[test]
    fn the_runs_cover_the_square() {
        let total: usize = SIGN.iter().map(|&r| r as usize).sum();
        assert_eq!(total, SIZE * SIZE);
    }

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
