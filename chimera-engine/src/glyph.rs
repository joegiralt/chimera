//! Which gauge a parameter's spec names for its focus band
//! (`ParamSpec::glyph`): the names and the params each composite reads.
//! `chimera_core::ui::glyph` draws them.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::reverb::ReverbParams;

/// The gauge a parameter's focus band shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusGlyph {
    /// 270° arc, from 12:00 when bipolar.
    #[default]
    Arc,
    /// No gauge: the value text gets the whole band (word choices).
    None,
    /// Two-state on/off.
    Switch,
    /// Vertical slider with tick dots.
    LevelBar,
    /// Horizontal crossfader.
    Crossfader,
    /// A note on a three-line staff, by semitones (`StaffNote`).
    Staff,
    /// A small scope tracing one period of the chosen wave's table.
    Wave,
    /// One animated glyph for all of an effect's params (`CompositeId::
    /// params`: the braid's 4, the rings' 7, the cube's 5), drawn from
    /// their set values, never the modulated ones.
    Composite(CompositeId),
}

/// The composites, each shared by its effect's params.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeId {
    /// SIZE, TIME, AMOUNT.
    ReverbCube,
    /// TIME, FEEDBACK, TONE.
    DelayRings,
    /// RATE, DEPTH, MIX; MODE is the strand count.
    ChorusBraid,
}

impl CompositeId {
    /// The params a composite reads, in its inputs' order: every spec that
    /// carries it, and only those (`tests/focus_glyph_test.rs` pins both).
    pub const fn params(self) -> &'static [ParamAddr] {
        match self {
            CompositeId::ChorusBraid => &BRAID_PARAMS,
            CompositeId::DelayRings => &RINGS_PARAMS,
            CompositeId::ReverbCube => &CUBE_PARAMS,
        }
    }
}

impl FocusGlyph {
    pub const ALL: [FocusGlyph; 10] = [
        FocusGlyph::Arc,
        FocusGlyph::None,
        FocusGlyph::Switch,
        FocusGlyph::LevelBar,
        FocusGlyph::Crossfader,
        FocusGlyph::Staff,
        FocusGlyph::Wave,
        FocusGlyph::Composite(CompositeId::ReverbCube),
        FocusGlyph::Composite(CompositeId::DelayRings),
        FocusGlyph::Composite(CompositeId::ChorusBraid),
    ];

    /// Index in `ALL`. Exhaustive: a new variant fails to compile here
    /// until it is listed.
    const fn position(self) -> usize {
        match self {
            FocusGlyph::Arc => 0,
            FocusGlyph::None => 1,
            FocusGlyph::Switch => 2,
            FocusGlyph::LevelBar => 3,
            FocusGlyph::Crossfader => 4,
            FocusGlyph::Staff => 5,
            FocusGlyph::Wave => 6,
            FocusGlyph::Composite(CompositeId::ReverbCube) => 7,
            FocusGlyph::Composite(CompositeId::DelayRings) => 8,
            FocusGlyph::Composite(CompositeId::ChorusBraid) => 9,
        }
    }
}

const _: () = {
    let mut i = 0;
    while i < FocusGlyph::ALL.len() {
        assert!(FocusGlyph::ALL[i].position() == i);
        i += 1;
    }
};

/// The reverb params the cube reads, in `Cube::from_set`'s order.
pub const CUBE_PARAMS: [ParamAddr; 5] = [
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SIZE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::TIME),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DAMPING),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::GRIT),
];

/// The delay params the rings read, in `Rings::from_set`'s order.
pub const RINGS_PARAMS: [ParamAddr; 7] = [
    ParamAddr::new(BlockRef::Delay, DelayParams::TIME_MS),
    ParamAddr::new(BlockRef::Delay, DelayParams::FEEDBACK),
    ParamAddr::new(BlockRef::Delay, DelayParams::TONE),
    ParamAddr::new(BlockRef::Delay, DelayParams::MIX),
    ParamAddr::new(BlockRef::Delay, DelayParams::WOW_FLUTTER),
    ParamAddr::new(BlockRef::Delay, DelayParams::SATURATION),
    ParamAddr::new(BlockRef::Delay, DelayParams::REV_SEND),
];

/// The chorus params the braid reads, in `Braid::from_set`'s order.
pub const BRAID_PARAMS: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Chorus, ChorusParams::MODE),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::RATE),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::DEPTH),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::MIX),
];
