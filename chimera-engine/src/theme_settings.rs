//! SETTINGS › PERSONALIZE › THEME: backlight, panel gamma, accent and
//! ground (ADR 0033), light mode (ADR 0126), whether focus bands show gesture hints (ADR
//! 0086), how the chain map draws sub-pages (ADR 0145), and the text size
//! (ADR 0154), and whether a live hit moves KIT's pad (ADR 0268).
//!
//! The values, their disk codes and the THEME block; what they look like
//! (colours, PWM duty, the panel's gamma tables) is the UI's
//! (`chimera_core::ui::theme_settings`). Nothing here touches the audio
//! thread.
//!
//! Stored in SYSTEM (`storage::SystemSettings`); with no card, no file or an
//! error, boot takes the owner's pick, 70 / PUNCH / TEAL / −2.

use crate::block::{
    Block, Change, DiskCode, Motion, OFF_ON, ParamId, ParamSpec, ValFmt, apply_code,
};

/// BRIGHT's and BLACK's values, by step index: their disk idents and their
/// shown names, one copy.
const BRIGHT_IDENTS: [&str; 19] = [
    "10", "15", "20", "25", "30", "35", "40", "45", "50", "55", "60", "65", "70", "75", "80", "85",
    "90", "95", "100",
];
const BLACK_IDENTS: [&str; 7] = ["-2", "-1", "0", "+1", "+2", "+3", "+4"];

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

    fn disk_ident(self) -> &'static str {
        BRIGHT_IDENTS[self.index() as usize]
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

    fn disk_ident(self) -> &'static str {
        match self {
            Gamma::Panel => "PANEL",
            Gamma::Soft => "SOFT",
            Gamma::Punch => "PUNCH",
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

impl Gamma {
    pub const ALL: [Gamma; 3] = [Gamma::Panel, Gamma::Soft, Gamma::Punch];
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

    fn disk_ident(self) -> &'static str {
        match self {
            Accent::Teal => "TEAL",
            Accent::Amber => "AMBER",
            Accent::Rose => "ROSE",
            Accent::Lime => "LIME",
            Accent::Ice => "ICE",
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

impl Accent {
    pub const ALL: [Accent; 5] = [
        Accent::Teal,
        Accent::Amber,
        Accent::Rose,
        Accent::Lime,
        Accent::Ice,
    ];
}

/// How the chain map shows a node's sub-pages (ADR 0145).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pages {
    /// A line under the nodes, left to right, windowed round the current.
    Row,
    /// Hung under the pill as a list, the nearest shown.
    Stack,
}

impl DiskCode for Pages {
    fn disk_code(self) -> u8 {
        match self {
            Pages::Row => 0,
            Pages::Stack => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Pages::Row => "ROW",
            Pages::Stack => "STACK",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Pages::Row),
            1 => Some(Pages::Stack),
            _ => None,
        }
    }
}

impl Pages {
    pub const ALL: [Pages; 2] = [Pages::Row, Pages::Stack];
}

/// How large the reading text draws (ADR 0154).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    /// The faces the screens were drawn in.
    Normal,
    /// The cells, the focus label, the hint and the lists a size up,
    /// where each fits.
    Large,
}

impl DiskCode for TextSize {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        TEXT_NAMES[self as usize]
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        TextSize::ALL.get(c as usize).copied()
    }
}

impl TextSize {
    pub const ALL: [TextSize; 2] = [TextSize::Normal, TextSize::Large];
}

/// Whether a live hit on the shown Part selects its pad (ADR 0268). Disk
/// codes are frozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadFollow {
    /// Never: only MIX + MINUS / PLUS and the PAD cell move it.
    Off,
    /// Once the player has left the DRUM pages' encoders alone a while.
    Idle,
    /// Every hit.
    Always,
}

impl DiskCode for PadFollow {
    fn disk_code(self) -> u8 {
        self as u8
    }

    fn disk_ident(self) -> &'static str {
        FOLLOW_NAMES[self as usize]
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        PadFollow::ALL.get(c as usize).copied()
    }
}

impl PadFollow {
    pub const ALL: [PadFollow; 3] = [PadFollow::Off, PadFollow::Idle, PadFollow::Always];
}

/// Which of ALGO's views OSC opens on (ADR 0300): every page is there
/// either way. Disk codes are frozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlgoFirst {
    /// One parameter across the six operators, then each operator's page.
    Parameter,
    /// Each operator's own page, then the parameter pages.
    Operator,
}

impl DiskCode for AlgoFirst {
    fn disk_code(self) -> u8 {
        match self {
            AlgoFirst::Parameter => 0,
            AlgoFirst::Operator => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            AlgoFirst::Parameter => "PARAMETER",
            AlgoFirst::Operator => "OPERATOR",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(AlgoFirst::Parameter),
            1 => Some(AlgoFirst::Operator),
            _ => None,
        }
    }
}

impl AlgoFirst {
    pub const ALL: [AlgoFirst; 2] = [AlgoFirst::Parameter, AlgoFirst::Operator];
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
}

/// The code is the lift itself as a two's-complement byte, −2..=4.
impl DiskCode for Black {
    fn disk_code(self) -> u8 {
        self.0 as u8
    }

    fn disk_ident(self) -> &'static str {
        BLACK_IDENTS[(self.0 - Self::MIN) as usize]
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        let v = c as i8;
        (Self::MIN..=Self::MAX).contains(&v).then_some(Black(v))
    }
}

/// Idle time before the screensaver, by step index (OFF, then seconds).
/// Disk codes are the indices: frozen, new steps append (ADR 0147).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saver(u8);

impl Saver {
    const SECS: [u16; 9] = [0, 5, 7, 10, 30, 60, 120, 300, 600];
    pub const OFF: Saver = Saver(0);
    pub const DEFAULT: Saver = Saver(7);

    /// Step `i`, held to the last.
    pub const fn from_index(i: u8) -> Saver {
        Saver(if (i as usize) < Self::SECS.len() {
            i
        } else {
            Self::SECS.len() as u8 - 1
        })
    }

    pub const fn index(self) -> u8 {
        self.0
    }

    /// Idle time in ms; `None` for OFF.
    pub const fn ms(self) -> Option<u32> {
        match Self::SECS[self.0 as usize] {
            0 => None,
            s => Some(s as u32 * 1000),
        }
    }
}

/// Everything SETTINGS › PERSONALIZE › THEME sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeSettings {
    pub bright: Bright,
    pub gamma: Gamma,
    pub accent: Accent,
    pub black: Black,
    /// Gesture hints in the focus bands; off, a band reads its cell alone.
    pub hints: bool,
    /// Light mode: white ground, black ink; BLACK then takes the edge off
    /// the white.
    pub invert: bool,
    /// Idle time before the oscilloscope screensaver.
    pub saver: Saver,
    /// The chain map's sub-pages: a row or a stack.
    pub pages: Pages,
    /// The reading text's size.
    pub text: TextSize,
    /// Whether a live hit moves KIT's pad.
    pub pad_follow: PadFollow,
    /// Which of ALGO's views OSC opens on.
    pub algo_first: AlgoFirst,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ThemeSettings {
    /// Boot state without a SYSTEM file: the owner's pick, 70 %, PUNCH, TEAL, −2.
    pub const DEFAULT: ThemeSettings = ThemeSettings {
        bright: Bright::DEFAULT,
        gamma: Gamma::Punch,
        accent: Accent::Teal,
        black: Black::DEFAULT,
        hints: true,
        invert: false,
        saver: Saver::DEFAULT,
        pages: Pages::Row,
        text: TextSize::Normal,
        pad_follow: PadFollow::Idle,
        algo_first: AlgoFirst::Parameter,
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
    pub const HINTS: ParamId = ParamId(4);
    pub const INVERT: ParamId = ParamId(5);
    pub const SAVER: ParamId = ParamId(6);
    pub const PAGES: ParamId = ParamId(7);
    pub const TEXT: ParamId = ParamId(8);
    pub const FOLLOW: ParamId = ParamId(9);
    pub const ALGO_FIRST: ParamId = ParamId(10);
}

const GAMMA_NAMES: [&str; 3] = ["PANEL", "SOFT", "PUNCH"];
const ACCENT_NAMES: [&str; 5] = ["TEAL", "AMBER", "ROSE", "LIME", "ICE"];
const SAVER_NAMES: [&str; 9] = [
    "OFF", "5 S", "7 S", "10 S", "30 S", "60 S", "2 MIN", "5 MIN", "10 MIN",
];
const _: () = assert!(SAVER_NAMES.len() == Saver::SECS.len());
const PAGES_NAMES: [&str; 2] = ["ROW", "STACK"];
const TEXT_NAMES: [&str; 2] = ["NORMAL", "LARGE"];
const FOLLOW_NAMES: [&str; 3] = ["OFF", "IDLE", "ALWAYS"];
/// As the cell and the band show them: PARAMETER is too wide for the band.
const ALGO_FIRST_NAMES: [&str; 2] = ["PARAMS", "OPS"];

const _: () = assert!(BRIGHT_IDENTS.len() == THEME_SPECS[0].max as usize + 1);
const _: () = assert!(BLACK_IDENTS.len() == THEME_SPECS[3].max as usize + 1);

/// Choices by index: BRIGHT 0..=18 (10..100 %), BLACK 0..=6 (−2..+4). The
/// defaults are the owner's pick: 70, PUNCH, TEAL, −2.
pub static THEME_SPECS: [ParamSpec; 11] = [
    ParamSpec::choice(
        0,
        "BRIGHT",
        ValFmt::Names(&BRIGHT_IDENTS),
        18.0,
        12.0,
        Change::Snap,
    )
    .ident("BRIGHT"),
    ParamSpec::choice(
        1,
        "GAMMA",
        ValFmt::Names(&GAMMA_NAMES),
        2.0,
        2.0,
        Change::Snap,
    )
    .ident("GAMMA")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        2,
        "ACCENT",
        ValFmt::Names(&ACCENT_NAMES),
        4.0,
        0.0,
        Change::Snap,
    )
    .ident("ACCENT")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        3,
        "BLACK",
        ValFmt::Names(&BLACK_IDENTS),
        6.0,
        0.0,
        Change::Snap,
    )
    .ident("BLACK"),
    // A SYSTEM from before HINTS has none: ON.
    ParamSpec::stepped(
        4,
        "HINTS",
        ValFmt::Names(&OFF_ON),
        0.0,
        1.0,
        1.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("HINTS")
    .glyph(crate::glyph::FocusGlyph::Switch),
    // A SYSTEM from before INVERT has none: OFF.
    ParamSpec::stepped(
        5,
        "INVERT",
        ValFmt::Names(&OFF_ON),
        0.0,
        1.0,
        0.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("INVERT")
    .glyph(crate::glyph::FocusGlyph::Switch),
    // A SYSTEM from before SAVER has none: 5 MIN.
    ParamSpec::stepped(
        6,
        "SAVER",
        ValFmt::Names(&SAVER_NAMES),
        0.0,
        8.0,
        7.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("SAVER")
    .glyph(crate::glyph::FocusGlyph::None),
    // A SYSTEM from before PAGES has none: ROW.
    ParamSpec::choice(
        7,
        "PAGES",
        ValFmt::Names(&PAGES_NAMES),
        1.0,
        0.0,
        Change::Snap,
    )
    .ident("PAGES")
    .glyph(crate::glyph::FocusGlyph::None),
    // A SYSTEM from before TEXT has none: NORMAL.
    ParamSpec::choice(
        8,
        "SIZE",
        ValFmt::Names(&TEXT_NAMES),
        1.0,
        0.0,
        Change::Snap,
    )
    .ident("TEXT")
    .glyph(crate::glyph::FocusGlyph::None),
    // A SYSTEM from before PAD FOLLOW has none: IDLE.
    ParamSpec::choice(
        9,
        "FOLLOW",
        ValFmt::Names(&FOLLOW_NAMES),
        2.0,
        1.0,
        Change::Snap,
    )
    .ident("FOLLOW")
    .glyph(crate::glyph::FocusGlyph::None),
    // A SYSTEM from before ALGO PAGES has none: PARAMETER.
    ParamSpec::choice(
        10,
        "FIRST",
        ValFmt::Names(&ALGO_FIRST_NAMES),
        1.0,
        0.0,
        Change::Snap,
    )
    .ident("ALGO_FIRST")
    .glyph(crate::glyph::FocusGlyph::None),
];

impl Block for ThemeSettings {
    fn specs(&self) -> &'static [ParamSpec] {
        &THEME_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::BRIGHT => self.bright.index() as f32,
            Self::GAMMA => self.gamma as u8 as f32,
            Self::ACCENT => self.accent as u8 as f32,
            Self::BLACK => (self.black.0 - Black::MIN) as f32,
            Self::HINTS => f32::from(self.hints),
            Self::INVERT => f32::from(self.invert),
            Self::SAVER => self.saver.index() as f32,
            Self::PAGES => self.pages as u8 as f32,
            Self::TEXT => self.text as u8 as f32,
            Self::FOLLOW => self.pad_follow as u8 as f32,
            Self::ALGO_FIRST => f32::from(self.algo_first.disk_code()),
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        let i = v as u8;
        match id {
            Self::BRIGHT => self.bright = Bright::from_index(i),
            Self::GAMMA => self.gamma = Gamma::ALL[(i as usize).min(Gamma::ALL.len() - 1)],
            Self::ACCENT => self.accent = Accent::ALL[(i as usize).min(Accent::ALL.len() - 1)],
            Self::BLACK => self.black = Black::new(Black::MIN.saturating_add(i.min(127) as i8)),
            Self::HINTS => self.hints = i != 0,
            Self::INVERT => self.invert = i != 0,
            Self::SAVER => self.saver = Saver::from_index(i),
            Self::PAGES => self.pages = Pages::ALL[(i as usize).min(Pages::ALL.len() - 1)],
            Self::TEXT => self.text = TextSize::ALL[(i as usize).min(TextSize::ALL.len() - 1)],
            Self::FOLLOW => {
                self.pad_follow = PadFollow::ALL[(i as usize).min(PadFollow::ALL.len() - 1)]
            }
            Self::ALGO_FIRST => {
                self.algo_first = AlgoFirst::ALL[(i as usize).min(AlgoFirst::ALL.len() - 1)]
            }
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::BRIGHT => Some(self.bright.disk_code()),
            Self::GAMMA => Some(self.gamma.disk_code()),
            Self::ACCENT => Some(self.accent.disk_code()),
            Self::BLACK => Some(self.black.disk_code()),
            Self::PAGES => Some(self.pages.disk_code()),
            Self::TEXT => Some(self.text.disk_code()),
            Self::FOLLOW => Some(self.pad_follow.disk_code()),
            Self::ALGO_FIRST => Some(self.algo_first.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::BRIGHT => Some(self.bright.disk_ident()),
            Self::GAMMA => Some(self.gamma.disk_ident()),
            Self::ACCENT => Some(self.accent.disk_ident()),
            Self::BLACK => Some(self.black.disk_ident()),
            Self::PAGES => Some(self.pages.disk_ident()),
            Self::TEXT => Some(self.text.disk_ident()),
            Self::FOLLOW => Some(self.pad_follow.disk_ident()),
            Self::ALGO_FIRST => Some(self.algo_first.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::BRIGHT => apply_code(Bright::from_disk_code(code), |b| self.bright = b),
            Self::GAMMA => apply_code(Gamma::from_disk_code(code), |g| self.gamma = g),
            Self::ACCENT => apply_code(Accent::from_disk_code(code), |a| self.accent = a),
            Self::BLACK => apply_code(Black::from_disk_code(code), |b| self.black = b),
            Self::PAGES => apply_code(Pages::from_disk_code(code), |p| self.pages = p),
            Self::TEXT => apply_code(TextSize::from_disk_code(code), |t| self.text = t),
            Self::FOLLOW => apply_code(PadFollow::from_disk_code(code), |f| self.pad_follow = f),
            Self::ALGO_FIRST => {
                apply_code(AlgoFirst::from_disk_code(code), |a| self.algo_first = a)
            }
            _ => false,
        }
    }
}
