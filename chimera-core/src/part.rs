//! A Part's mix settings (instrument-core spec § Data model): the MIDI
//! channel it listens on, Mono/Poly, the DAC pair it plays out of, level,
//! pan and FX sends. One `Block`, so it gets pages and snap like any other.

use crate::MidiChannel;
use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::fx_bus::FX_SENDS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PartMode {
    /// One voice, retriggered by each note; never stolen.
    Mono = 0,
    /// Voices from the shared pool.
    Poly = 1,
}

impl PartMode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => PartMode::Mono,
            _ => PartMode::Poly,
        }
    }
}

/// One of the three stereo DAC outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DacPair {
    P1 = 0,
    P2 = 1,
    P3 = 2,
}

impl DacPair {
    pub const ALL: [DacPair; crate::hw::DAC_PAIRS] = [DacPair::P1, DacPair::P2, DacPair::P3];

    pub const fn index(self) -> usize {
        self as usize
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => DacPair::P1,
            1 => DacPair::P2,
            _ => DacPair::P3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartParams {
    pub channel: MidiChannel,
    pub mode: PartMode,
    pub output: DacPair,
    /// 0..1
    pub level: f32,
    /// -1 (left) .. 1 (right)
    pub pan: f32,
    /// Chorus, delay, reverb (`FxBus` order), 0..1 each.
    pub sends: [f32; FX_SENDS],
}

impl PartParams {
    pub const CHANNEL: ParamId = ParamId(0);
    pub const MODE: ParamId = ParamId(1);
    pub const OUTPUT: ParamId = ParamId(2);
    pub const LEVEL: ParamId = ParamId(3);
    pub const PAN: ParamId = ParamId(4);
    pub const SEND_CHORUS: ParamId = ParamId(5);
    pub const SEND_DELAY: ParamId = ParamId(6);
    pub const SEND_REVERB: ParamId = ParamId(7);

    /// Part `index` (0-based) listens on channel `index`, Poly, output P1,
    /// level 0.8, centre pan, no sends.
    pub fn for_part(index: usize) -> Self {
        Self {
            channel: MidiChannel::clamped(index as u8),
            mode: PartMode::Poly,
            output: DacPair::P1,
            level: 0.8,
            pan: 0.0,
            sends: [0.0; FX_SENDS],
        }
    }
}

impl Default for PartParams {
    fn default() -> Self {
        Self::for_part(0)
    }
}

/// Nothing here is modulatable: the mixer applies these, not the voice (ADR 0010).
pub static PART_SPECS: [ParamSpec; 8] = [
    ParamSpec::choice(0, "CH", ValFmt::OneBased(15), 15.0, 0.0),
    ParamSpec::choice(1, "MODE", ValFmt::Names(&["MONO", "POLY"]), 1.0, 1.0),
    ParamSpec::choice(2, "OUT", ValFmt::Names(&["P1", "P2", "P3"]), 2.0, 0.0),
    ParamSpec::continuous(3, "LEVEL", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "PAN", ValFmt::Pan, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
    ParamSpec::continuous(5, "CHR", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "DLY", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(7, "REV", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for PartParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &PART_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::CHANNEL => self.channel.get() as f32,
            Self::MODE => self.mode as u8 as f32,
            Self::OUTPUT => self.output as u8 as f32,
            Self::LEVEL => self.level,
            Self::PAN => self.pan,
            Self::SEND_CHORUS => self.sends[0],
            Self::SEND_DELAY => self.sends[1],
            Self::SEND_REVERB => self.sends[2],
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CHANNEL => self.channel = MidiChannel::clamped(v as u8),
            Self::MODE => self.mode = PartMode::from_u8(v as u8),
            Self::OUTPUT => self.output = DacPair::from_u8(v as u8),
            Self::LEVEL => self.level = v,
            Self::PAN => self.pan = v,
            Self::SEND_CHORUS => self.sends[0] = v,
            Self::SEND_DELAY => self.sends[1] = v,
            Self::SEND_REVERB => self.sends[2] = v,
            _ => {}
        }
    }
}
