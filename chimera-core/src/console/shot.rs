//! `shot`: the framebuffer streamed through the palette, a row at a time.

use super::Colours;
use crate::ui::theme_settings::Palette;
use chimera_hal::{FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};

/// The host stopped taking bytes for STALL_MS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stalled;

/// Where answers go.
pub trait Out {
    fn put(&mut self, bytes: &[u8]) -> Result<(), Stalled>;
}

/// The screen as the display holds it, and the palette it flushes through.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub fb: &'a [u16; FB_SIZE],
    pub palette: Palette,
}

pub const SHOT_HEADER: &str = "SHOT 240 320 rgb565be 153600\n";

const W: usize = SCREEN_WIDTH as usize;
const H: usize = SCREEN_HEIGHT as usize;

/// Whether `h` is `SHOT <w> <h> rgb565be <bytes>\n`, compared in const context.
const fn header_is(h: &[u8], w: usize, ht: usize, bytes: usize) -> bool {
    const fn lit(h: &[u8], i: usize, s: &[u8]) -> Option<usize> {
        let mut k = 0;
        while k < s.len() {
            if i + k >= h.len() || h[i + k] != s[k] {
                return None;
            }
            k += 1;
        }
        Some(i + s.len())
    }
    const fn num(h: &[u8], i: usize, n: usize) -> Option<usize> {
        let mut div = 1;
        while n / div >= 10 {
            div *= 10;
        }
        let mut i = i;
        let mut rest = n;
        while div > 0 {
            if i >= h.len() || h[i] != b'0' + (rest / div) as u8 {
                return None;
            }
            rest %= div;
            div /= 10;
            i += 1;
        }
        Some(i)
    }
    macro_rules! step {
        ($e:expr) => {
            match $e {
                Some(i) => i,
                None => return false,
            }
        };
    }
    let i = step!(lit(h, 0, b"SHOT "));
    let i = step!(num(h, i, w));
    let i = step!(lit(h, i, b" "));
    let i = step!(num(h, i, ht));
    let i = step!(lit(h, i, b" rgb565be "));
    let i = step!(num(h, i, bytes));
    let i = step!(lit(h, i, b"\n"));
    i == h.len()
}
const _: () = assert!(header_is(SHOT_HEADER.as_bytes(), W, H, FB_SIZE * 2));

/// The header, the body row by row (one 480-byte row on the stack at a time), then `OK`.
pub fn write_shot(f: Frame<'_>, c: Colours, out: &mut impl Out) -> Result<(), Stalled> {
    out.put(SHOT_HEADER.as_bytes())?;
    let mut row = [[0u8; 2]; W];
    for src in f.fb.as_chunks::<W>().0 {
        for (dst, &px) in row.iter_mut().zip(src) {
            *dst = match c {
                Colours::Theme => f.palette.map_raw(px),
                Colours::Raw => px,
            }
            .to_be_bytes();
        }
        out.put(row.as_flattened())?;
    }
    out.put(b"OK\n")
}
