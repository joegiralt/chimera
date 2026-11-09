//! The instrument-wide settings SYSTEM keeps beside the theme: MIDI
//! CONFIG's SYNC, who drives ORBIT's transport (ADR 0080), SETTINGS >
//! TUNING's TUNE, the A4 reference as cents off 440 Hz, SYSTEM > PUSH
//! ENCODERS' PUSH, whether the front panel's encoders push (ADR 0140), and
//! MIDI CONFIG's ECHO, DIN OUT's master switch (spec § 8.4), and MIDI
//! CONFIG › CONTROL's MIXER, SEQUENCER and AUTO channels (ADR 0254).
//! They say how the unit sits in the studio, not what a project plays.
//! (One block, so the file keeps its `MIDI` record.)

use crate::MidiChannel;
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
    /// PUSH and ECHO (`push`, `echo`).
    pub switches: Switches,
    /// MIXER, SEQUENCER and AUTO (`Control`), five bits each from bit 0:
    /// the channel plus one, 0 OFF (ADR 0254). Packed: a wider
    /// `MidiSettings` grew SYSTEM's bank-1 copies.
    pub control: u16,
}

/// MIDI CONFIG › CONTROL's channels (ADR 0254), `MidiSettings::control`'s
/// order: the precedence among them when two share a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Mixer = 0,
    Sequencer = 1,
    Auto = 2,
}

impl Control {
    pub const ALL: [Control; 3] = [Control::Mixer, Control::Sequencer, Control::Auto];
}

/// SYSTEM's two switches in one byte: PUSH, the control board has push
/// encoders, read them (ADR 0140); ECHO, DIN OUT sends value echo and
/// identity replies (spec § 8.4). One byte keeps `MidiSettings` at three,
/// so SYSTEM's copies in bank 1 keep their shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches(u8);

impl Switches {
    const PUSH: u8 = 1;
    const ECHO: u8 = 2;

    const fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    const fn with(self, bit: u8, on: bool) -> Self {
        Self(if on { self.0 | bit } else { self.0 & !bit })
    }
}

impl MidiSettings {
    pub const DEFAULT: MidiSettings = MidiSettings {
        sync: SyncMode::DEFAULT,
        tune: 0,
        switches: Switches(Switches::ECHO),
        control: 0,
    };

    pub const SYNC: ParamId = ParamId(0);
    pub const TUNE: ParamId = ParamId(1);
    pub const PUSH: ParamId = ParamId(2);
    pub const ECHO: ParamId = ParamId(3);
    // 4 is PROGRAM IN's (#891), 5-8 MPE's (#896).
    pub const MIXER: ParamId = ParamId(9);
    pub const SEQUENCER: ParamId = ParamId(10);
    pub const AUTO: ParamId = ParamId(11);
    pub const CONTROL: [ParamId; 3] = [Self::MIXER, Self::SEQUENCER, Self::AUTO];

    /// `c`'s channel; `None`: OFF.
    pub fn channel_of(&self, c: Control) -> Option<MidiChannel> {
        self.raw(c as u8).checked_sub(1).and_then(MidiChannel::new)
    }

    /// CONTROL channel `i`'s stored value: 0 OFF, else the channel plus one.
    pub const fn raw(&self, i: u8) -> u8 {
        (self.control >> (5 * i) & 31) as u8
    }

    /// The control board has push encoders: read them (ADR 0140).
    pub const fn push(&self) -> bool {
        self.switches.has(Switches::PUSH)
    }

    /// DIN OUT sends: value echo and identity replies (spec § 8.4).
    pub const fn echo(&self) -> bool {
        self.switches.has(Switches::ECHO)
    }

    pub const fn with_push(mut self, on: bool) -> Self {
        self.switches = self.switches.with(Switches::PUSH, on);
        self
    }

    pub const fn with_echo(mut self, on: bool) -> Self {
        self.switches = self.switches.with(Switches::ECHO, on);
        self
    }

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

/// A CONTROL channel: OFF, then 1 to 16.
static CONTROL_NAMES: [&str; 17] = [
    "OFF", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];

/// One CONTROL channel's spec: OFF by default, and in a SYSTEM from
/// before it (ADR 0254).
const fn control(id: u8, label: &'static str, ident: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Names(&CONTROL_NAMES),
        0.0,
        16.0,
        0.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident(ident)
}

/// SYNC by index, `SyncMode::ALL`'s order; `SYNC_NAMES` too
/// (`midi_sync_page_test::sync_value_names_and_order_agree`). INT and EXT are two peers,
/// neither off, so the focus shows the word.
pub static MIDI_SPECS: [ParamSpec; 7] = [
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
    // A SYSTEM from before ECHO has none: ON, the default (spec § 8.4).
    ParamSpec::stepped(
        3,
        "ECHO",
        ValFmt::Names(&OFF_ON),
        0.0,
        1.0,
        1.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("ECHO")
    .glyph(crate::glyph::FocusGlyph::Switch),
    control(9, "MIXER", "MIXER_CH"),
    control(10, "SEQUENCER", "SEQ_CH").short("SEQ"),
    control(11, "AUTO", "AUTO_CH"),
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
            Self::PUSH => f32::from(self.push()),
            Self::ECHO => f32::from(self.echo()),
            ParamId(i @ 9..=11) => f32::from(self.raw(i - 9)),
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::SYNC => self.sync = SyncMode::ALL[(v as usize).min(SyncMode::ALL.len() - 1)],
            Self::TUNE => self.tune = crate::dsp::clampf(v, -100.0, 100.0) as i8,
            Self::PUSH => *self = self.with_push(v as u8 != 0),
            Self::ECHO => *self = self.with_echo(v as u8 != 0),
            ParamId(i @ 9..=11) => {
                let at = 5 * (i - 9);
                let v = u16::from((v as u8).min(16));
                self.control = self.control & !(31 << at) | v << at;
            }
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
