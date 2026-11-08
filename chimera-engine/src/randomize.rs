//! SETTINGS › PART › RANDOMIZE (#809, ADR 0232): four amounts, as stock
//! PreenFM3's randomizer has, and a seed. Turning any of them rolls the
//! active Part's Sound (`kern::roll`, bank 2's): the same seed, amounts and
//! Sound always roll the same result.
//!
//! Held by the UI, never stored: a session's pick, like a knob's place.

pub mod kern;

use crate::block::{Block, Change, Motion, ParamId, ParamSpec, ValFmt};

/// How far one dimension moves, 0 (OFF: left as it is) to 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount(u8);

impl Amount {
    pub const OFF: Amount = Amount(0);
    pub const MAX: u8 = 3;

    pub const fn new(v: u8) -> Self {
        Amount(if v > Self::MAX { Self::MAX } else { v })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The roll's seed, 0..=255: what the page shows and a turn steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed(u8);

impl Seed {
    pub const fn new(v: u8) -> Self {
        Seed(v)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The page's cells: OSC, ENV, TONE and MOD (stock's OPER, ENV TIME, IM
/// and MODL), then SEED.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Randomize {
    pub osc: Amount,
    pub env: Amount,
    pub tone: Amount,
    pub mods: Amount,
    pub seed: Seed,
}

impl Default for Randomize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Randomize {
    /// RICH oscillators, ANY envelope, BRIGHT tone, SUBTLE motion.
    pub const DEFAULT: Randomize = Randomize {
        osc: Amount(2),
        env: Amount(3),
        tone: Amount(2),
        mods: Amount(1),
        seed: Seed(0),
    };

    pub const OSC: ParamId = ParamId(0);
    pub const ENV: ParamId = ParamId(1);
    pub const TONE: ParamId = ParamId(2);
    pub const MOD: ParamId = ParamId(3);
    pub const SEED: ParamId = ParamId(4);
}

const OSC_NAMES: [&str; 4] = ["OFF", "PURE", "RICH", "WILD"];
/// Stock's families: 1 percussive, 2 slow, 3 anything.
const ENV_NAMES: [&str; 4] = ["OFF", "PLUCK", "PAD", "ANY"];
const TONE_NAMES: [&str; 4] = ["OFF", "SOFT", "BRIGHT", "WILD"];
const MOD_NAMES: [&str; 4] = ["OFF", "SUBTLE", "MOVING", "WILD"];

const fn amount(id: u8, label: &'static str, names: &'static [&'static str], d: f32) -> ParamSpec {
    ParamSpec::choice(id, label, ValFmt::Names(names), 3.0, d, Change::Snap)
        .ident(label)
        .glyph(crate::glyph::FocusGlyph::None)
}

pub static RANDOM_SPECS: [ParamSpec; 5] = [
    amount(0, "OSC", &OSC_NAMES, 2.0),
    amount(1, "ENV", &ENV_NAMES, 3.0),
    amount(2, "TONE", &TONE_NAMES, 2.0),
    amount(3, "MOD", &MOD_NAMES, 1.0),
    ParamSpec::stepped(
        4,
        "SEED",
        ValFmt::Int(u8::MAX),
        0.0,
        u8::MAX as f32,
        0.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("SEED"),
];

impl Block for Randomize {
    fn specs(&self) -> &'static [ParamSpec] {
        &RANDOM_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::OSC => self.osc.get() as f32,
            Self::ENV => self.env.get() as f32,
            Self::TONE => self.tone.get() as f32,
            Self::MOD => self.mods.get() as f32,
            Self::SEED => self.seed.get() as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        let a = Amount::new(v as u8);
        match id {
            Self::OSC => self.osc = a,
            Self::ENV => self.env = a,
            Self::TONE => self.tone = a,
            Self::MOD => self.mods = a,
            Self::SEED => self.seed = Seed::new(v as u8),
            _ => {}
        }
    }
}
