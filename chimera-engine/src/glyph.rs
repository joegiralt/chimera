//! Which gauge a parameter's spec names for its focus band
//! (`ParamSpec::glyph`): the names and the params each composite reads.
//! `chimera_core::ui::glyph` draws them.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::reverb::ReverbParams;
use crate::dsp::tape_engine::TapeEngineParams as T;
use crate::dsp::va::VaParams;

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
    /// params`: the braid's 4, the rings' 7, the cube's 6), drawn from
    /// their set values, never the modulated ones.
    Composite(CompositeId),
    /// VA's own gauges, drawn from the VA block's set values (ADR 0129).
    Va(VaGlyph),
    /// TAPE's machine glyph (ADR 0173): every machine cell's, drawn from
    /// `TAPE_GLYPH_PARAMS`' set values; still on MODEL and MODE.
    Tape,
}

/// VA's gauges: what the oscillators and the operator do to the sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaGlyph {
    /// A scope of one oscillator's naive wave at its WAVE and SHAPE.
    Wave(VaOsc),
    /// OSC 1 MODE OSC 2: the operator's output over two of OSC 1's periods.
    Mode,
    /// The crossfader, its ends named by MODE.
    Blend,
}

/// One of VA's two oscillators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaOsc {
    One,
    Two,
}

impl VaOsc {
    /// Its WAVE and SHAPE.
    pub const fn params(self) -> [ParamAddr; 2] {
        let (w, s) = match self {
            VaOsc::One => (VaParams::WAVE1, VaParams::SHAPE1),
            VaOsc::Two => (VaParams::WAVE2, VaParams::SHAPE2),
        };
        [
            ParamAddr::new(BlockRef::Va, w),
            ParamAddr::new(BlockRef::Va, s),
        ]
    }
}

/// What MODE's glyph reads, in `VaMix::from_set`'s order: MODE, AMOUNT,
/// then each oscillator's WAVE and SHAPE. BLEND's reads the first two.
pub const VA_MIX_PARAMS: [ParamAddr; 6] = {
    let [w1, s1] = VaOsc::One.params();
    let [w2, s2] = VaOsc::Two.params();
    [
        ParamAddr::new(BlockRef::Va, VaParams::MODE),
        ParamAddr::new(BlockRef::Va, VaParams::AMOUNT),
        w1,
        s1,
        w2,
        s2,
    ]
};

/// The composites, each shared by its effect's params.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeId {
    /// The reverb's knobs: the cube under QUAD (ADR 0067, 0085).
    Reverb,
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
            CompositeId::Reverb => &REVERB_PARAMS,
        }
    }
}

impl FocusGlyph {
    pub const ALL: [FocusGlyph; 15] = [
        FocusGlyph::Arc,
        FocusGlyph::None,
        FocusGlyph::Switch,
        FocusGlyph::LevelBar,
        FocusGlyph::Crossfader,
        FocusGlyph::Staff,
        FocusGlyph::Wave,
        FocusGlyph::Composite(CompositeId::Reverb),
        FocusGlyph::Composite(CompositeId::DelayRings),
        FocusGlyph::Composite(CompositeId::ChorusBraid),
        FocusGlyph::Va(VaGlyph::Wave(VaOsc::One)),
        FocusGlyph::Va(VaGlyph::Wave(VaOsc::Two)),
        FocusGlyph::Va(VaGlyph::Mode),
        FocusGlyph::Va(VaGlyph::Blend),
        FocusGlyph::Tape,
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
            FocusGlyph::Composite(CompositeId::Reverb) => 7,
            FocusGlyph::Composite(CompositeId::DelayRings) => 8,
            FocusGlyph::Composite(CompositeId::ChorusBraid) => 9,
            FocusGlyph::Va(VaGlyph::Wave(VaOsc::One)) => 10,
            FocusGlyph::Va(VaGlyph::Wave(VaOsc::Two)) => 11,
            FocusGlyph::Va(VaGlyph::Mode) => 12,
            FocusGlyph::Va(VaGlyph::Blend) => 13,
            FocusGlyph::Tape => 14,
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
pub const CUBE_PARAMS: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SIZE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::TIME),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DAMPING),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SMEAR),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::RATE),
];

/// Every reverb param that shows the composite: the cube's six, then the
/// slots only the other TYPEs read (their glyph lands with them).
pub const REVERB_PARAMS: [ParamAddr; 11] = [
    CUBE_PARAMS[0],
    CUBE_PARAMS[1],
    CUBE_PARAMS[2],
    CUBE_PARAMS[3],
    CUBE_PARAMS[4],
    CUBE_PARAMS[5],
    ParamAddr::new(BlockRef::Reverb, ReverbParams::TONE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DIFFUSE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::PREDELAY),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::SHAPE),
    ParamAddr::new(BlockRef::Reverb, ReverbParams::DENSITY),
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

/// What TAPE's machine glyph reads, in `TapeGauge::from_set`'s order:
/// MODEL, MODE, START, END, POSITION, MOTION, SIZE, DRAG, TAPE, AGE, XFADE.
pub const TAPE_GLYPH_PARAMS: [ParamAddr; 11] = {
    const fn t(id: crate::block::ParamId) -> ParamAddr {
        ParamAddr::new(BlockRef::TapeEngine, id)
    }
    [
        t(T::MODEL),
        t(T::MODE),
        t(T::START),
        t(T::END),
        t(T::POSITION),
        t(T::MOTION),
        t(T::SIZE),
        t(T::DRAG),
        t(T::TAPE),
        t(T::AGE),
        t(T::XFADE),
    ]
};
