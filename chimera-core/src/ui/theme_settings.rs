//! System › Theme: backlight, panel gamma, accent and ground (ADR 0033).
//!
//! The renderer keeps drawing the canonical palette (`theme::ACCENT`,
//! `ACCENT_SOFT`, `BG`); the display shell swaps those three colours for the
//! chosen ones as it pushes pixels out (`Palette::map`), so TEAL with BLACK 0
//! is the identity and every screen golden stays bit-identical. BRIGHT and
//! GAMMA are hardware: the shell turns them into PWM duty and ILI9341
//! commands. Nothing here touches the audio thread.
//!
//! No storage yet: the settings reset at boot to the owner's pick, 70 / PUNCH / TEAL / −2.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::prelude::RgbColor;

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::ui::theme;

/// Backlight duty, percent: 10..=100 in steps of 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bright(u8);

impl Bright {
    pub const MIN: u8 = 10;
    pub const MAX: u8 = 100;
    pub const STEP: u8 = 5;
    pub const DEFAULT: Bright = Bright(70);

    /// `pct` clamped to 10..=100 and rounded to the nearest step.
    pub const fn new(pct: u8) -> Self {
        let p = if pct < Self::MIN {
            Self::MIN
        } else if pct > Self::MAX {
            Self::MAX
        } else {
            pct
        };
        Bright((p + Self::STEP / 2) / Self::STEP * Self::STEP)
    }

    pub const fn percent(self) -> u8 {
        self.0
    }

    /// PWM compare value for a timer whose full scale is `max_duty`. In u32:
    /// `max_duty * pct` overflows u16 (TIM1 at 20 kHz counts to 12000).
    pub const fn duty(self, max_duty: u16) -> u16 {
        (max_duty as u32 * self.0 as u32 / 100) as u16
    }

    /// Step index 0..=18 (the param's value).
    const fn index(self) -> u8 {
        (self.0 - Self::MIN) / Self::STEP
    }

    const fn from_index(i: u8) -> Self {
        Self::new(Self::MIN.saturating_add(i.saturating_mul(Self::STEP)))
    }
}

/// The code is the percent itself, 10..=100 in steps of 5.
impl DiskCode for Bright {
    fn disk_code(self) -> u8 {
        self.0
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        let on_step =
            (Self::MIN..=Self::MAX).contains(&c) && (c - Self::MIN).is_multiple_of(Self::STEP);
        on_step.then_some(Bright(c))
    }
}

/// Panel gamma: which 0xE0/0xE1 tables the ILI9341 gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gamma {
    /// The panel's power-on tables: the look the PreenFM3 init leaves.
    Panel,
    /// Halfway between PANEL and PUNCH, field by field.
    Soft,
    /// Adafruit's tables: deeper blacks, more contrast, a muted teal.
    Punch,
}

impl DiskCode for Gamma {
    fn disk_code(self) -> u8 {
        match self {
            Gamma::Panel => 0,
            Gamma::Soft => 1,
            Gamma::Punch => 2,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Gamma::Panel),
            1 => Some(Gamma::Soft),
            2 => Some(Gamma::Punch),
            _ => None,
        }
    }
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

impl Gamma {
    pub const ALL: [Gamma; 3] = [Gamma::Panel, Gamma::Soft, Gamma::Punch];

    pub const fn tables(self) -> &'static GammaTables {
        match self {
            Gamma::Panel => &PANEL_TABLES,
            Gamma::Soft => &SOFT_TABLES,
            Gamma::Punch => &PUNCH_TABLES,
        }
    }
}

/// The one accent colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
    Teal,
    Amber,
    Rose,
    Lime,
    Ice,
}

impl DiskCode for Accent {
    fn disk_code(self) -> u8 {
        match self {
            Accent::Teal => 0,
            Accent::Amber => 1,
            Accent::Rose => 2,
            Accent::Lime => 3,
            Accent::Ice => 4,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Accent::Teal),
            1 => Some(Accent::Amber),
            2 => Some(Accent::Rose),
            3 => Some(Accent::Lime),
            4 => Some(Accent::Ice),
            _ => None,
        }
    }
}

/// The ground `#0a0b0d` the soft accent mixes over.
const GROUND_HEX: u32 = 0x0a0b0d;

impl Accent {
    pub const ALL: [Accent; 5] = [
        Accent::Teal,
        Accent::Amber,
        Accent::Rose,
        Accent::Lime,
        Accent::Ice,
    ];

    /// `#rrggbb`: mid-light and moderately saturated like the teal, so each
    /// reads on the dark ground without blooming on the TN panel.
    pub const fn hex(self) -> u32 {
        match self {
            Accent::Teal => 0x7fd4c8,
            Accent::Amber => 0xe8b45c,
            Accent::Rose => 0xe89aae,
            Accent::Lime => 0xb4d86a,
            Accent::Ice => 0xa4c4f0,
        }
    }

    pub const fn color(self) -> Rgb565 {
        rgb565(self.hex())
    }

    /// The accent at 12 % over the ground: fill under a viz line.
    pub fn soft(self) -> Rgb565 {
        let (a, g) = (self.hex(), GROUND_HEX);
        let mix = |s: u32| {
            let (a, g) = ((a >> s) & 0xFF, (g >> s) & 0xFF);
            (a * 12 + g * 88 + 50) / 100
        };
        rgb565(mix(16) << 16 | mix(8) << 8 | mix(0))
    }
}

/// `#rrggbb` to RGB565 by truncation, as the palette consts were made.
pub const fn rgb565(hex: u32) -> Rgb565 {
    Rgb565::new(
        ((hex >> 16) as u8) >> 3,
        ((hex >> 8) as u8) >> 2,
        (hex as u8) >> 3,
    )
}

/// Ground lift in RGB565 levels, −2..=4: green (6-bit) moves one level per
/// step, red and blue follow at half that, so the ground stays neutral and
/// every step differs (−2 is pure black; +4 is `#181818`, still well below
/// FAINT).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Black(i8);

impl Black {
    pub const MIN: i8 = -2;
    pub const MAX: i8 = 4;
    pub const DEFAULT: Black = Black(-2);
    /// The renderer's own ground.
    pub const ZERO: Black = Black(0);

    pub const fn new(v: i8) -> Self {
        Black(if v < Self::MIN {
            Self::MIN
        } else if v > Self::MAX {
            Self::MAX
        } else {
            v
        })
    }

    pub const fn get(self) -> i8 {
        self.0
    }

    /// The ground at this lift; 0 is `theme::BG` (1, 2, 1).
    pub const fn ground(self) -> Rgb565 {
        let g = (2 + self.0) as u8;
        Rgb565::new(g / 2, g, g / 2)
    }
}

/// The code is the lift itself as a two's-complement byte, −2..=4.
impl DiskCode for Black {
    fn disk_code(self) -> u8 {
        self.0 as u8
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        let v = c as i8;
        (Self::MIN..=Self::MAX).contains(&v).then_some(Black(v))
    }
}

/// Everything System › Theme sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeSettings {
    pub bright: Bright,
    pub gamma: Gamma,
    pub accent: Accent,
    pub black: Black,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
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

impl ThemeSettings {
    /// Boot state (no storage yet): the owner's pick, 70 %, PUNCH, TEAL, −2.
    pub const DEFAULT: ThemeSettings = ThemeSettings {
        bright: Bright::DEFAULT,
        gamma: Gamma::Punch,
        accent: Accent::Teal,
        black: Black::DEFAULT,
    };

    /// TEAL at ground 0: the palette the renderer draws, unchanged.
    pub const NEUTRAL: ThemeSettings = ThemeSettings {
        black: Black::ZERO,
        ..ThemeSettings::DEFAULT
    };

    pub const BRIGHT: ParamId = ParamId(0);
    pub const GAMMA: ParamId = ParamId(1);
    pub const ACCENT: ParamId = ParamId(2);
    pub const BLACK: ParamId = ParamId(3);

    /// The colours the display shows for ACCENT, ACCENT_SOFT and BG. The soft
    /// accent rides the ground: it moves with BLACK by the ground's own step.
    pub fn palette(&self) -> Palette {
        let (g0, g) = (Black::ZERO.ground(), self.black.ground());
        let s = self.accent.soft();
        let ch = |v: u8, d0: u8, d: u8, max: i16| (v as i16 + d as i16 - d0 as i16).clamp(0, max);
        Palette {
            accent: self.accent.color(),
            accent_soft: Rgb565::new(
                ch(s.r(), g0.r(), g.r(), 31) as u8,
                ch(s.g(), g0.g(), g.g(), 63) as u8,
                ch(s.b(), g0.b(), g.b(), 31) as u8,
            ),
            bg: g,
        }
    }
}

const BRIGHT_NAMES: [&str; 19] = [
    "10", "15", "20", "25", "30", "35", "40", "45", "50", "55", "60", "65", "70", "75", "80", "85",
    "90", "95", "100",
];
const GAMMA_NAMES: [&str; 3] = ["PANEL", "SOFT", "PUNCH"];
const ACCENT_NAMES: [&str; 5] = ["TEAL", "AMBER", "ROSE", "LIME", "ICE"];
const BLACK_NAMES: [&str; 7] = ["-2", "-1", "0", "+1", "+2", "+3", "+4"];

/// Choices by index: BRIGHT 0..=18 (10..100 %), BLACK 0..=6 (−2..+4). The
/// defaults are the owner's pick: 70, PUNCH, TEAL, −2.
pub static THEME_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(0, "BRIGHT", ValFmt::Names(&BRIGHT_NAMES), 18.0, 12.0),
    ParamSpec::choice(1, "GAMMA", ValFmt::Names(&GAMMA_NAMES), 2.0, 2.0),
    ParamSpec::choice(2, "ACCENT", ValFmt::Names(&ACCENT_NAMES), 4.0, 0.0),
    ParamSpec::choice(3, "BLACK", ValFmt::Names(&BLACK_NAMES), 6.0, 0.0),
];

impl Block for ThemeSettings {
    fn specs(&self) -> &'static [ParamSpec] {
        &THEME_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::BRIGHT => self.bright.index() as f32,
            Self::GAMMA => self.gamma as u8 as f32,
            Self::ACCENT => self.accent as u8 as f32,
            Self::BLACK => (self.black.0 - Black::MIN) as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        let i = v as u8;
        match id {
            Self::BRIGHT => self.bright = Bright::from_index(i),
            Self::GAMMA => self.gamma = Gamma::ALL[(i as usize).min(Gamma::ALL.len() - 1)],
            Self::ACCENT => self.accent = Accent::ALL[(i as usize).min(Accent::ALL.len() - 1)],
            Self::BLACK => self.black = Black::new(Black::MIN.saturating_add(i.min(127) as i8)),
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::BRIGHT => Some(self.bright.disk_code()),
            Self::GAMMA => Some(self.gamma.disk_code()),
            Self::ACCENT => Some(self.accent.disk_code()),
            Self::BLACK => Some(self.black.disk_code()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::BRIGHT => apply_code(Bright::from_disk_code(code), |b| self.bright = b),
            Self::GAMMA => apply_code(Gamma::from_disk_code(code), |g| self.gamma = g),
            Self::ACCENT => apply_code(Accent::from_disk_code(code), |a| self.accent = a),
            Self::BLACK => apply_code(Black::from_disk_code(code), |b| self.black = b),
            _ => false,
        }
    }
}
