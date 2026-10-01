//! Focus glyphs: the gauge at the right of the focus band, hand-assigned
//! per parameter on its spec (`ParamSpec::glyph`), ARC by default.

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

/// A glyph the focus band can draw today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    Arc,
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

    /// What the focus band draws for this glyph: one not built yet stands
    /// in as ARC. Each glyph's story moves it to its own `Drawn`.
    pub const fn drawn(self) -> Drawn {
        match self {
            FocusGlyph::Arc
            | FocusGlyph::None
            | FocusGlyph::Switch
            | FocusGlyph::LevelBar
            | FocusGlyph::Crossfader
            | FocusGlyph::Composite(_) => Drawn::Arc,
        }
    }
}

impl Drawn {
    /// Moves on its own: its band redraws every frame while shown.
    pub const fn animates(self) -> bool {
        match self {
            Drawn::Arc => false,
        }
    }
}

/// The focus band's dirty-key share of the UI clock: the frame while the
/// glyph animates, else constant.
pub const fn anim_key(animates: bool, frame: u32) -> u32 {
    if animates { frame } else { 0 }
}
