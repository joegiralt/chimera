//! Focus glyphs: the gauge at the right of the focus band, hand-assigned
//! per parameter on its spec (`ParamSpec::glyph`), ARC by default.

use crate::block::ValFmt;

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
    pub fn gauge(self, value: f32, fmt: ValFmt) -> Gauge {
        let bipolar = fmt.is_bipolar();
        match self {
            FocusGlyph::None => Gauge::None,
            FocusGlyph::Switch => Gauge::Switch { on: value },
            FocusGlyph::LevelBar => Gauge::LevelBar {
                value,
                ticks: level_ticks(fmt),
            },
            FocusGlyph::Arc
            | FocusGlyph::Crossfader
            | FocusGlyph::Composite(CompositeId::ReverbCube)
            | FocusGlyph::Composite(CompositeId::DelayRings)
            | FocusGlyph::Composite(CompositeId::ChorusBraid) => Gauge::Arc { value, bipolar },
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
            Gauge::Arc { .. } | Gauge::None | Gauge::Switch { .. } | Gauge::LevelBar { .. } => {
                false
            }
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
