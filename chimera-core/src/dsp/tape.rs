//! Tape on DAC pair 1 (FX diet spec § Tape). Per side: wow, pre-emphasis,
//! 2× oversampled soft saturation, de-emphasis, head bump and an HF
//! roll-off that darkens with DRIVE, blended in parallel with the dry.
//! MIX 0 is an exact bypass.
//!
//! The DSP (`stage`) is behind the `master-tape` feature, off by default
//! (ADR 0055): the parameters, specs, idents and disk codes stay, so a
//! Performance keeps its TAPE settings either way.

use crate::block::{Block, ParamId, ParamSpec, ValFmt};

#[cfg(feature = "master-tape")]
mod stage;
#[cfg(feature = "master-tape")]
pub use stage::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapeParams {
    pub drive: f32,
    /// 0 darker, 1 brighter.
    pub tone: f32,
    pub wow: f32,
    /// Parallel blend: 0 dry (bypass), 1 all tape.
    pub mix: f32,
}

impl Default for TapeParams {
    fn default() -> Self {
        Self {
            drive: 0.0,
            tone: 0.5,
            wow: 0.0,
            mix: 0.0,
        }
    }
}

impl TapeParams {
    pub const DRIVE: ParamId = ParamId(0);
    pub const TONE: ParamId = ParamId(1);
    pub const WOW: ParamId = ParamId(2);
    pub const MIX: ParamId = ParamId(3);

    /// Off when the mix is below audibility: pair 1 passes untouched.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    /// NaN reads as the default; everything clamps to 0..1.
    pub fn sanitised(&self) -> Self {
        let d = Self::default();
        let f = |v: f32, def: f32| if v.is_nan() { def } else { v.clamp(0.0, 1.0) };
        Self {
            drive: f(self.drive, d.drive),
            tone: f(self.tone, d.tone),
            wow: f(self.wow, d.wow),
            mix: f(self.mix, d.mix),
        }
    }
}

/// The tape runs outside `Voice`: nothing is modulatable.
pub static TAPE_SPECS: [ParamSpec; 4] = [
    ParamSpec::continuous(0, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false)
        .ident("DRIVE"),
    ParamSpec::continuous(1, "TONE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false).ident("TONE"),
    ParamSpec::continuous(2, "WOW", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false).ident("WOW"),
    ParamSpec::continuous(3, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false)
        .ident("MIX")
        .glyph(crate::ui::glyph::FocusGlyph::LevelBar),
];

impl Block for TapeParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &TAPE_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::DRIVE => self.drive,
            Self::TONE => self.tone,
            Self::WOW => self.wow,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::DRIVE => self.drive = v,
            Self::TONE => self.tone = v,
            Self::WOW => self.wow = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
