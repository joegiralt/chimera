//! The reverb's settings (FX diet spec § Controls); the ring is
//! `dsp::ring`.

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::ring::RingControls;

#[derive(Clone, Copy, Debug)]
pub struct ReverbParams {
    pub time: f32,
    pub damping: f32,
    pub size: f32,
    /// Return level.
    pub mix: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            time: 0.5,
            damping: 0.3,
            size: 0.5,
            mix: 0.0,
        }
    }
}

impl ReverbParams {
    /// Off when the mix is below audibility; the bus skips it then.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    // ParamId(0) was TYPE: retired, never reused (ADR 0009).
    pub const TIME: ParamId = ParamId(1);
    pub const DAMPING: ParamId = ParamId(2);
    pub const SIZE: ParamId = ParamId(3);
    pub const MIX: ParamId = ParamId(4);

    /// What the ring reads.
    pub fn controls(&self) -> RingControls {
        RingControls {
            time: self.time,
            damp: self.damping,
            size: self.size,
            ..RingControls::default()
        }
    }
}

/// The reverb runs outside `Voice`: nothing is modulatable.
pub static REVERB_SPECS: [ParamSpec; 4] = [
    ParamSpec::continuous(1, "TIME", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DAMP", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "SIZE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 31.0, false),
    ParamSpec::continuous(4, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for ReverbParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &REVERB_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::TIME => self.time,
            Self::DAMPING => self.damping,
            Self::SIZE => self.size,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::TIME => self.time = v,
            Self::DAMPING => self.damping = v,
            Self::SIZE => self.size = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
