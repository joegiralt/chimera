//! SETTINGS › MIDI CONFIG's instrument-wide settings: SYNC, who drives
//! ORBIT's transport (ADR 0080). Held by the UI beside the theme and
//! stored in SYSTEM, since it says how the unit sits in the studio, not
//! what a project plays.

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::orbit::clock::SyncMode;

/// Everything MIDI CONFIG sets that SYSTEM keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiSettings {
    pub sync: SyncMode,
}

impl MidiSettings {
    pub const DEFAULT: MidiSettings = MidiSettings {
        sync: SyncMode::DEFAULT,
    };

    pub const SYNC: ParamId = ParamId(0);
}

impl Default for MidiSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

const SYNC_NAMES: [&str; 2] = ["INT", "EXT"];

/// SYNC by index, `SyncMode::ALL`'s order. INT and EXT are two peers,
/// neither off, so the focus shows the word.
pub static MIDI_SPECS: [ParamSpec; 1] =
    [
        ParamSpec::choice(0, "SYNC", ValFmt::Names(&SYNC_NAMES), 1.0, 1.0)
            .ident("SYNC")
            .glyph(crate::ui::glyph::FocusGlyph::None),
    ];

const _: () = assert!(SyncMode::ALL.len() == SYNC_NAMES.len());
const _: () = assert!(matches!(SyncMode::ALL[1], SyncMode::DEFAULT));

impl Block for MidiSettings {
    fn specs(&self) -> &'static [ParamSpec] {
        &MIDI_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::SYNC => self.sync as u8 as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        if id == Self::SYNC {
            self.sync = SyncMode::ALL[(v as usize).min(SyncMode::ALL.len() - 1)];
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
