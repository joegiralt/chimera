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

/// The three canonical colours a theme replaces, in the display shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub accent: Rgb565,
    pub accent_soft: Rgb565,
    pub bg: Rgb565,
}

impl Palette {
    /// TEAL, BLACK 0: maps every colour to itself.
    pub const IDENTITY: Palette = Palette {
        accent: theme::ACCENT,
        accent_soft: theme::ACCENT_SOFT,
        bg: theme::BG,
    };

    /// The colour to show for a canonical palette colour `c`.
    pub fn map(&self, c: Rgb565) -> Rgb565 {
        RawU16::new(self.map_raw(RawU16::from(c).into_inner())).into()
    }

    /// `map` on a raw framebuffer pixel.
    #[inline]
    pub fn map_raw(&self, px: u16) -> u16 {
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
}

#[inline]
fn raw(c: Rgb565) -> u16 {
    RawU16::from(c).into_inner()
}

/// The colours the display shows for ACCENT, ACCENT_SOFT and BG. The soft
/// accent rides the ground: it moves with BLACK by the ground's own step.
pub fn palette(t: &ThemeSettings) -> Palette {
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
    }
}
