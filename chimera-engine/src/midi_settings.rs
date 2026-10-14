//! The instrument-wide settings SYSTEM keeps beside the theme: MIDI
//! CONFIG's SYNC, who drives ORBIT's transport (ADR 0080), SETTINGS >
//! TUNING's TUNE, the A4 reference as cents off 440 Hz, and SYSTEM > PUSH
//! ENCODERS' PUSH, whether the front panel's encoders push (ADR 0140).
//! They say how the unit sits in the studio, not what a project plays.
//! (One block, so the file keeps its `MIDI` record.)

use crate::block::{
    Block, Change, DiskCode, Motion, OFF_ON, ParamId, ParamSpec, ValFmt, apply_code,
};
use crate::orbit::clock::SyncMode;

/// Everything MIDI CONFIG sets that SYSTEM keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiSettings {
    pub sync: SyncMode,
    /// Cents, -100..=100: A4 = 440 Hz × 2^(tune / 1200). Whole cents.
    pub tune: i8,
    /// The control board has push encoders: read them (ADR 0140).
    pub push: bool,
}

impl MidiSettings {
    pub const DEFAULT: MidiSettings = MidiSettings {
        sync: SyncMode::DEFAULT,
        tune: 0,
        push: false,
    };

    pub const SYNC: ParamId = ParamId(0);
    pub const TUNE: ParamId = ParamId(1);
    pub const PUSH: ParamId = ParamId(2);

    /// TUNE in semitones, for the pitch path.
    pub fn tune_semitones(&self) -> f32 {
        f32::from(self.tune) / 100.0
    }
}

impl Default for MidiSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

const SYNC_NAMES: [&str; 2] = ["INT", "EXT"];

/// SYNC by index, `SyncMode::ALL`'s order; `SYNC_NAMES` too
/// (`midi_sync_page_test::sync_value_names_and_order_agree`). INT and EXT are two peers,
/// neither off, so the focus shows the word.
pub static MIDI_SPECS: [ParamSpec; 3] = [
    ParamSpec::choice(
        0,
        "SYNC",
        ValFmt::Names(&SYNC_NAMES),
        1.0,
        1.0,
        Change::Snap,
    )
    .ident("SYNC")
    .glyph(crate::glyph::FocusGlyph::None),
    // Not modulatable: a route would detune the whole unit.
    ParamSpec::stepped(
        1,
        "TUNE",
        ValFmt::Signed(100),
        -100.0,
        100.0,
        0.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("TUNE"),
    // A SYSTEM from before PUSH has none: OFF, the 24-bit read.
    ParamSpec::stepped(
        2,
        "PUSH",
        ValFmt::Names(&OFF_ON),
        0.0,
        1.0,
        0.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("PUSH")
    .glyph(crate::glyph::FocusGlyph::Switch),
];

const _: () = assert!(SyncMode::ALL.len() == SYNC_NAMES.len());
// `get` reads the discriminant as the index: `ALL` keeps declaration order.
const _: () = {
    let mut i = 0;
    while i < SyncMode::ALL.len() {
        assert!(SyncMode::ALL[i] as usize == i);
        i += 1;
    }
};
const _: () = assert!(matches!(SyncMode::ALL[1], SyncMode::DEFAULT));

impl Block for MidiSettings {
    fn specs(&self) -> &'static [ParamSpec] {
        &MIDI_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::SYNC => self.sync as u8 as f32,
            Self::TUNE => f32::from(self.tune),
            Self::PUSH => f32::from(self.push),
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::SYNC => self.sync = SyncMode::ALL[(v as usize).min(SyncMode::ALL.len() - 1)],
            Self::TUNE => self.tune = crate::dsp::clampf(v, -100.0, 100.0) as i8,
            Self::PUSH => self.push = v as u8 != 0,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::SYNC).then(|| self.sync.disk_code())
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        (id == Self::SYNC).then(|| self.sync.disk_ident())
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::SYNC => apply_code(SyncMode::from_disk_code(code), |s| self.sync = s),
            _ => false,
        }
    }
}
