//! Focus glyphs: the gauge at the right of the focus band, hand-assigned
//! per parameter on its spec (`ParamSpec::glyph`), ARC by default.

use crate::addr::{BlockRef, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::chorus::ChorusParams;

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
    /// One animated glyph for up to three params of an effect, drawn from
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

/// What the focus band draws, with its inputs: built glyphs only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gauge {
    /// `value` 0..1, from 12:00 when `bipolar`.
    Arc { value: f32, bipolar: bool },
    /// No gauge: the value text has the whole band.
    None,
    /// A two-state toggle: `on` 0 off, 1 on, eased between.
    Switch { on: f32 },
    /// A vertical slider, `value` 0..1 rising from the bottom, beside
    /// `ticks` dots: one per step for a few steps, else 8.
    LevelBar { value: f32, ticks: u8 },
    /// A horizontal crossfader, `value` 0 left .. 1 right, from the set
    /// value, never the modulated one.
    Crossfader { value: f32 },
    /// The chorus braid (`CompositeId::ChorusBraid`).
    Braid(Braid),
}

/// The chorus params the braid reads, in `Braid::from_set`'s order.
pub const BRAID_PARAMS: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Chorus, ChorusParams::MODE),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::RATE),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::DEPTH),
    ParamAddr::new(BlockRef::Chorus, ChorusParams::MIX),
];

/// The braid's param in focus, emphasised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BraidPart {
    Mode,
    Rate,
    Depth,
    Mix,
}

impl BraidPart {
    /// In `BRAID_PARAMS`' order.
    pub const ALL: [BraidPart; 4] = [
        BraidPart::Mode,
        BraidPart::Rate,
        BraidPart::Depth,
        BraidPart::Mix,
    ];
}

/// The chorus's voices as strands twisting round a dry centre line, from
/// the chorus's set values (normalized) and the UI clock's `frame`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Braid {
    /// MODE: 0 OFF, 1 I, 2 II, 3 I+II.
    pub mode: u8,
    /// How fast the twist travels.
    pub rate: f32,
    /// How far the strands swing apart.
    pub depth: f32,
    /// How bright the strands are against the dry line.
    pub mix: f32,
    pub focus: Option<BraidPart>,
    pub frame: u32,
}

impl Braid {
    /// From `BRAID_PARAMS`' set values, normalized.
    pub fn from_set(set: [f32; 4], focus: Option<BraidPart>, frame: u32) -> Self {
        Self {
            mode: libm::roundf(set[0].clamp(0.0, 1.0) * 3.0) as u8,
            rate: set[1].clamp(0.0, 1.0),
            depth: set[2].clamp(0.0, 1.0),
            mix: set[3].clamp(0.0, 1.0),
            focus,
            frame,
        }
    }

    /// OFF none (the dry line alone), I and II two, I+II three.
    pub const fn strands(&self) -> usize {
        match self.mode {
            0 => 0,
            1 | 2 => 2,
            _ => 3,
        }
    }

    /// Twists along the box: II's are tighter.
    pub const fn turns(&self) -> f32 {
        if self.mode == 2 { 2.5 } else { 1.5 }
    }

    /// The twist's phase at `frame`, radians in 0..τ: RATE sets how fast
    /// it travels (0.16 to 1.6 Hz at 20 fps); II travels faster. In f64,
    /// so it stays smooth however long the clock has run.
    pub fn twist(&self, frame: u32) -> f32 {
        let per_frame = 0.05 + 0.45 * self.rate as f64;
        let speed = if self.mode == 2 { 1.6 } else { 1.0 };
        libm::fmod(frame as f64 * per_frame * speed, core::f64::consts::TAU) as f32
    }
}

impl FocusGlyph {
    pub const ALL: [FocusGlyph; 8] = [
        FocusGlyph::Arc,
        FocusGlyph::None,
        FocusGlyph::Switch,
        FocusGlyph::LevelBar,
        FocusGlyph::Crossfader,
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
            FocusGlyph::Composite(CompositeId::ReverbCube) => 5,
            FocusGlyph::Composite(CompositeId::DelayRings) => 6,
            FocusGlyph::Composite(CompositeId::ChorusBraid) => 7,
        }
    }

    /// The gauge the focus band draws for this glyph at the focused slot's
    /// `value` in format `fmt`. The one place a glyph not built yet stands
    /// in as ARC; each glyph's story gives it its own `Gauge`.
    /// A built composite's gauge comes from `composite`, which reads its
    /// params' set values.
    pub fn gauge(
        self,
        value: f32,
        fmt: ValFmt,
        composite: impl FnOnce(CompositeId) -> Gauge,
    ) -> Gauge {
        let bipolar = fmt.is_bipolar();
        match self {
            FocusGlyph::None => Gauge::None,
            FocusGlyph::Switch => Gauge::Switch { on: value },
            FocusGlyph::LevelBar => Gauge::LevelBar {
                value,
                ticks: level_ticks(fmt),
            },
            FocusGlyph::Crossfader => Gauge::Crossfader { value },
            FocusGlyph::Composite(CompositeId::ChorusBraid) => composite(CompositeId::ChorusBraid),
            FocusGlyph::Arc
            | FocusGlyph::Composite(CompositeId::ReverbCube)
            | FocusGlyph::Composite(CompositeId::DelayRings) => Gauge::Arc { value, bipolar },
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

impl Gauge {
    /// Moves on its own: its band redraws every frame while shown.
    pub const fn animates(&self) -> bool {
        match self {
            Gauge::Arc { .. }
            | Gauge::None
            | Gauge::Switch { .. }
            | Gauge::LevelBar { .. }
            | Gauge::Crossfader { .. } => false,
            Gauge::Braid(_) => true,
        }
    }
}

/// A level bar's ticks for `fmt`: one per step when it has 2 to 9, else 8.
fn level_ticks(fmt: ValFmt) -> u8 {
    let steps = fmt.max_int().saturating_add(1);
    if fmt.is_discrete() && (2..=9).contains(&steps) {
        steps
    } else {
        8
    }
}

/// The focus band's dirty-key share of the UI clock: the frame while the
/// glyph animates, else constant.
pub const fn anim_key(animates: bool, frame: u32) -> u32 {
    if animates { frame } else { 0 }
}
