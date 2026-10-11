//! THEME as the panel shows it: the accent's and ground's colours, the
//! backlight's PWM duty and the ILI9341's gamma tables. The values and
//! their disk codes are the engine's (`chimera_engine::theme_settings`).
//!
//! The renderer keeps drawing the canonical palette (`theme::ACCENT`,
//! `ACCENT_SOFT`, `BG`); the display shell swaps those three colours for the
//! chosen ones as it pushes pixels out (`Palette::map`), so TEAL with BLACK 0
//! is the identity and every screen golden stays bit-identical. BRIGHT and
//! GAMMA are hardware: the shell turns them into PWM duty and ILI9341
//! commands.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::prelude::RgbColor;

pub use chimera_engine::theme_settings::*;

use crate::ui::theme;

/// PWM compare value for BRIGHT on a timer whose full scale is `max_duty`.
/// In u32: `max_duty * pct` overflows u16 (TIM1 at 20 kHz counts to 12000).
pub const fn duty(b: Bright, max_duty: u16) -> u16 {
    (max_duty as u32 * b.percent() as u32 / 100) as u16
}

/// Positive (0xE0) and negative (0xE1) gamma correction, 15 bytes each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GammaTables {
    pub positive: [u8; 15],
    pub negative: [u8; 15],
}

/// ILI9341 datasheet V1.11, § 8.1 command list (p. 86–87) and § 8.3.24–25:
/// the E0h/E1h reset defaults. The chip has no "restore gamma" command
/// (§ 15.1 resets only the GC0 curve select), so PANEL sends these.
const PANEL_TABLES: GammaTables = GammaTables {
    positive: [
        0x08, 0x0E, 0x12, 0x05, 0x03, 0x09, 0x47, 0x86, 0x2B, 0x0B, 0x04, 0x00, 0x00, 0x00, 0x00,
    ],
    negative: [
        0x08, 0x1A, 0x20, 0x07, 0x0E, 0x05, 0x3A, 0x8A, 0x40, 0x04, 0x18, 0x0F, 0x3F, 0x3F, 0x0F,
    ],
};

/// Adafruit_ILI9341's init tables (as in commit 7293a02).
const PUNCH_TABLES: GammaTables = GammaTables {
    positive: [
        0x0F, 0x31, 0x2B, 0x0C, 0x0E, 0x08, 0x4E, 0xF1, 0x37, 0x07, 0x10, 0x03, 0x0E, 0x09, 0x00,
    ],
    negative: [
        0x00, 0x0E, 0x14, 0x03, 0x11, 0x07, 0x31, 0xC1, 0x48, 0x08, 0x0F, 0x0C, 0x31, 0x36, 0x0F,
    ],
};

const SOFT_TABLES: GammaTables = GammaTables {
    positive: midpoint(&PANEL_TABLES.positive, &PUNCH_TABLES.positive),
    negative: midpoint(&PANEL_TABLES.negative, &PUNCH_TABLES.negative),
};

/// Each register field halfway (rounded down) between `a` and `b`. Every
/// byte is one voltage field except the 8th, which packs two 4-bit fields
/// (VP36|VP27, VN36|VN27), so its nibbles are averaged apart.
pub const fn midpoint(a: &[u8; 15], b: &[u8; 15]) -> [u8; 15] {
    let mut out = [0u8; 15];
    let mut i = 0;
    while i < 15 {
        out[i] = if i == 7 {
            let hi = ((a[i] >> 4) + (b[i] >> 4)) / 2;
            let lo = ((a[i] & 0x0F) + (b[i] & 0x0F)) / 2;
            hi << 4 | lo
        } else {
            ((a[i] as u16 + b[i] as u16) / 2) as u8
        };
        i += 1;
    }
    out
}

/// GAMMA's 0xE0/0xE1 tables.
pub const fn tables(g: Gamma) -> &'static GammaTables {
    match g {
        Gamma::Panel => &PANEL_TABLES,
        Gamma::Soft => &SOFT_TABLES,
        Gamma::Punch => &PUNCH_TABLES,
    }
}

/// The ground `#0a0b0d` the soft accent mixes over.
const GROUND_HEX: u32 = 0x0a0b0d;

/// The accent's `#rrggbb`: mid-light and moderately saturated like the
/// teal, so each reads on the dark ground without blooming on the TN panel.
pub const fn accent_hex(a: Accent) -> u32 {
    match a {
        Accent::Teal => 0x7fd4c8,
        Accent::Amber => 0xe8b45c,
        Accent::Rose => 0xe89aae,
        Accent::Lime => 0xb4d86a,
        Accent::Ice => 0xa4c4f0,
    }
}

pub const fn accent_color(a: Accent) -> Rgb565 {
    rgb565(accent_hex(a))
}

/// The accent at 12 % over the ground: fill under a viz line.
pub fn accent_soft(a: Accent) -> Rgb565 {
    let (a, g) = (accent_hex(a), GROUND_HEX);
    let mix = |s: u32| {
        let (a, g) = ((a >> s) & 0xFF, (g >> s) & 0xFF);
        (a * 12 + g * 88 + 50) / 100
    };
    rgb565(mix(16) << 16 | mix(8) << 8 | mix(0))
}

/// `#rrggbb` to RGB565 by truncation, as the palette consts were made.
pub const fn rgb565(hex: u32) -> Rgb565 {
    Rgb565::new(
        ((hex >> 16) as u8) >> 3,
        ((hex >> 8) as u8) >> 2,
        (hex as u8) >> 3,
    )
}

/// The ground at BLACK's lift; 0 is `theme::BG` (1, 2, 1).
pub const fn ground(b: Black) -> Rgb565 {
    let g = (2 + b.get()) as u8;
    Rgb565::new(g / 2, g, g / 2)
}

/// Contrast the light-mode accent, WARN and ALERT must keep against the
/// ground: WCAG's 3:1 for UI glyphs, with a margin (x10) because the check
/// uses gamma 2.0, which reads a dark colour lighter than sRGB does.
const LIGHT_CONTRAST_X10: u32 = 32;

/// The lightest ground's weakest case: BLACK +4 in light mode, `#e7e7e7`
/// (28, 57, 28 in RGB565). The light colours are checked against it, so
/// they hold on every BLACK; a test pins it to `ground_light`.
const LIGHT_FLOOR: u32 = 231;

/// Gamma-2 luminance, 0..=65025 (white).
const fn luma(r: u32, g: u32, b: u32) -> u32 {
    (2126 * r * r + 7152 * g * g + 722 * b * b) / 10_000
}

/// `hex` scaled toward black, in 1/128ths, until it holds
/// `LIGHT_CONTRAST_X10` against `LIGHT_FLOOR`; unchanged if it already does.
const fn darken(hex: u32) -> u32 {
    const K: u32 = 3251; // 0.05 of white's luminance
    let (r, g, b) = (hex >> 16 & 0xFF, hex >> 8 & 0xFF, hex & 0xFF);
    let lb = luma(LIGHT_FLOOR, LIGHT_FLOOR, LIGHT_FLOOR) + K;
    let mut s = 256;
    while s > 0
        && lb * 10 < LIGHT_CONTRAST_X10 * (luma((r * s) >> 8, (g * s) >> 8, (b * s) >> 8) + K)
    {
        s -= 2;
    }
    ((r * s) >> 8) << 16 | ((g * s) >> 8) << 8 | ((b * s) >> 8)
}

/// 12 % of `hex` over white: the fill under a viz line, light mode (BLACK's
/// few levels do not show in it).
const fn mix12(c: u32) -> u32 {
    (c * 12 + 255 * 88 + 50) / 100
}

const fn soft_over_white(hex: u32) -> u32 {
    mix12(hex >> 16 & 0xFF) << 16 | mix12(hex >> 8 & 0xFF) << 8 | mix12(hex & 0xFF)
}

const LIGHT_HEX: [u32; 5] = [
    darken(accent_hex(Accent::Teal)),
    darken(accent_hex(Accent::Amber)),
    darken(accent_hex(Accent::Rose)),
    darken(accent_hex(Accent::Lime)),
    darken(accent_hex(Accent::Ice)),
];

/// The five accents in light mode: each pick, darkened only as far as it
/// takes to hold 3:1, computed at compile time, and the soft fill.
static ACCENTS_LIGHT: [Rgb565; 5] = [
    rgb565(LIGHT_HEX[0]),
    rgb565(LIGHT_HEX[1]),
    rgb565(LIGHT_HEX[2]),
    rgb565(LIGHT_HEX[3]),
    rgb565(LIGHT_HEX[4]),
];
static SOFTS_LIGHT: [Rgb565; 5] = [
    rgb565(soft_over_white(LIGHT_HEX[0])),
    rgb565(soft_over_white(LIGHT_HEX[1])),
    rgb565(soft_over_white(LIGHT_HEX[2])),
    rgb565(soft_over_white(LIGHT_HEX[3])),
    rgb565(soft_over_white(LIGHT_HEX[4])),
];

/// WARN (`#f88000`) and ALERT (`#f80000`) in light mode, by the same rule.
pub const WARN_LIGHT: Rgb565 = rgb565(darken(0xf88000));
pub const ALERT_LIGHT: Rgb565 = rgb565(darken(0xf80000));

/// INVERT's ground: the dark ground at the same BLACK, bit-flipped. BLACK
/// −2 is white; each step takes a little more off it.
pub fn ground_light(b: Black) -> Rgb565 {
    from_raw(!raw(ground(b)))
}

/// The accent in light mode.
pub fn accent_light(a: Accent) -> Rgb565 {
    ACCENTS_LIGHT[a as usize]
}

/// The colours a theme replaces, in the display shell. Dark mode swaps
/// three canonical colours; light mode (`invert`) flips every other colour's
/// luminance and swaps the signal colours too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub accent: Rgb565,
    pub accent_soft: Rgb565,
    pub bg: Rgb565,
    pub invert: bool,
}

impl Palette {
    /// TEAL, BLACK 0, dark: maps every colour to itself.
    pub const IDENTITY: Palette = Palette {
        accent: theme::ACCENT,
        accent_soft: theme::ACCENT_SOFT,
        bg: theme::BG,
        invert: false,
    };

    /// The colour to show for a canonical palette colour `c`.
    pub fn map(&self, c: Rgb565) -> Rgb565 {
        from_raw(self.map_raw(raw(c)))
    }

    /// `map` on one raw framebuffer pixel (tests, the desktop). Flushes use
    /// `map_into`, which branches on INVERT once per call, not per pixel.
    #[inline(never)]
    pub fn map_raw(&self, px: u16) -> u16 {
        if self.invert {
            self.light_raw(px)
        } else {
            self.dark_raw(px)
        }
    }

    /// `src` through the palette as big-endian bytes into `dst` (two a pixel).
    /// INVERT off costs the three compares it always did.
    pub fn map_into(&self, src: &[u16], dst: &mut [u8]) {
        let put = |d: &mut [u8; 2], v: u16| *d = v.to_be_bytes();
        if self.invert {
            for (d, &p) in dst.as_chunks_mut::<2>().0.iter_mut().zip(src) {
                put(d, self.light_raw(p));
            }
        } else {
            for (d, &p) in dst.as_chunks_mut::<2>().0.iter_mut().zip(src) {
                put(d, self.dark_raw(p));
            }
        }
    }

    #[inline]
    fn dark_raw(&self, px: u16) -> u16 {
        if px == raw(theme::ACCENT) {
            raw(self.accent)
        } else if px == raw(theme::ACCENT_SOFT) {
            raw(self.accent_soft)
        } else if px == raw(theme::BG) {
            raw(self.bg)
        } else {
            px
        }
    }

    /// Light mode's `map_raw`: every colour but the swapped five is flipped.
    #[inline]
    fn light_raw(&self, px: u16) -> u16 {
        if px == raw(theme::BG) {
            raw(self.bg)
        } else if px == raw(theme::ACCENT) {
            raw(self.accent)
        } else if px == raw(theme::ACCENT_SOFT) {
            raw(self.accent_soft)
        } else if px == raw(theme::WARN) {
            raw(WARN_LIGHT)
        } else if px == raw(theme::ALERT) {
            raw(ALERT_LIGHT)
        } else {
            !px
        }
    }
}

#[inline]
fn raw(c: Rgb565) -> u16 {
    RawU16::from(c).into_inner()
}

#[inline]
fn from_raw(px: u16) -> Rgb565 {
    RawU16::new(px).into()
}

/// The colours the display shows for ACCENT, ACCENT_SOFT and BG (and, in
/// light mode, WARN and ALERT). The soft accent rides the ground: in dark
/// mode it moves with BLACK by the ground's own step.
#[inline(never)]
pub fn palette(t: &ThemeSettings) -> Palette {
    if t.invert {
        let bg = ground_light(t.black);
        let accent = accent_light(t.accent);
        return Palette {
            accent,
            accent_soft: SOFTS_LIGHT[t.accent as usize],
            bg,
            invert: true,
        };
    }
    let (g0, g) = (ground(Black::ZERO), ground(t.black));
    let s = accent_soft(t.accent);
    let ch = |v: u8, d0: u8, d: u8, max: i16| (v as i16 + d as i16 - d0 as i16).clamp(0, max);
    Palette {
        accent: accent_color(t.accent),
        accent_soft: Rgb565::new(
            ch(s.r(), g0.r(), g.r(), 31) as u8,
            ch(s.g(), g0.g(), g.g(), 63) as u8,
            ch(s.b(), g0.b(), g.b(), 31) as u8,
        ),
        bg: g,
        invert: false,
    }
}

/// The palette for the boot splash: `t` with light mode off, so the Yellow
/// Sign stays yellow with a dark ink (ADR 0126).
pub fn splash_palette(t: &ThemeSettings) -> Palette {
    palette(&ThemeSettings {
        invert: false,
        ..*t
    })
}
