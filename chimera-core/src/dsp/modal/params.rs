//! Modal's parameters and their specs.

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};

// ── Modal Params ────────────────────────────────────────────────────

/// Resonator model selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResonatorMode {
    String = 0,      // KS+ (body, stiffness, position, ensemble)
    Modal = 1,       // SVF bandpass bank (Rings-style)
    Bowed = 2,       // Sustained bow friction
    Sympathetic = 3, // Multiple resonating strings (Rings-style)
}

impl DiskCode for ResonatorMode {
    fn disk_code(self) -> u8 {
        match self {
            ResonatorMode::String => 0,
            ResonatorMode::Modal => 1,
            ResonatorMode::Bowed => 2,
            ResonatorMode::Sympathetic => 3,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(ResonatorMode::String),
            1 => Some(ResonatorMode::Modal),
            2 => Some(ResonatorMode::Bowed),
            3 => Some(ResonatorMode::Sympathetic),
            _ => None,
        }
    }
}

impl ResonatorMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 4 {
            0 => ResonatorMode::String,
            1 => ResonatorMode::Modal,
            2 => ResonatorMode::Bowed,
            _ => ResonatorMode::Sympathetic,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModalParams {
    pub mode: ResonatorMode,
    pub excite: f32,
    pub decay: f32,
    pub brightness: f32,
    pub inharm: f32,   // Modal: stiffness. String: not used.
    pub position: f32, // Modal: excitation position. String: pluck position.
    pub note: f32,
    pub num_modes: u8,
    // String (KS+) params
    pub ks_excitation: u8, // 0=noise, 1=click, 2=bright, 3=dark
    pub ks_color: f32,     // excitation brightness
    pub ks_body: f32,      // body resonance (half-delay comb)
    pub ks_stiffness: f32, // allpass dispersion (bell character)
    pub ks_feedback: f32,  // sustain boost
    pub ks_ens_rate: f32,  // ensemble LFO rate
    pub ks_ens_depth: f32, // ensemble detuning depth
    pub ks_ens_mix: f32,   // ensemble dry/wet
    // Bowed params
    pub bow_velocity: f32,
    pub bow_force: f32,
}

impl Default for ModalParams {
    fn default() -> Self {
        Self {
            mode: ResonatorMode::String,
            excite: 0.8,
            decay: 0.3,
            brightness: 0.7,
            inharm: 0.25,
            position: 0.0, // bridge position
            note: 60.0,
            num_modes: 32,
            ks_excitation: 0, // noise
            ks_color: 0.8,
            ks_body: 0.3,
            ks_stiffness: 0.0,
            ks_feedback: 0.2,
            ks_ens_rate: 0.3,
            ks_ens_depth: 0.0,
            ks_ens_mix: 0.0,
            bow_velocity: 0.5,
            bow_force: 0.5,
        }
    }
}

impl ModalParams {
    pub const MODE: ParamId = ParamId(0);
    pub const EXCITE: ParamId = ParamId(1);
    pub const DECAY: ParamId = ParamId(2);
    pub const BRIGHTNESS: ParamId = ParamId(3);
    pub const POSITION: ParamId = ParamId(4);
    pub const INHARM: ParamId = ParamId(5);
    pub const KS_BODY: ParamId = ParamId(6);
    pub const KS_STIFFNESS: ParamId = ParamId(7);
    pub const KS_FEEDBACK: ParamId = ParamId(8);
    pub const KS_ENS_DEPTH: ParamId = ParamId(9);
    pub const KS_ENS_RATE: ParamId = ParamId(10);
    pub const KS_ENS_MIX: ParamId = ParamId(11);
}

/// Modal params are read at note-on (or by the engine from the unmodulated
/// snapshot), never from `Voice`'s modulated copy: none are modulatable.
/// Only UI-bound params have specs (plan D16). MODE max 3 is plan D3.
pub static MODAL_SPECS: [ParamSpec; 12] = [
    ParamSpec::choice(0, "MODE", ValFmt::Int(3), 3.0, 0.0),
    ParamSpec::continuous(1, "EXCITE", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DECAY", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "BRIGHT", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "POS", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "INHARM", ValFmt::Uni, 0.0, 1.0, 0.25, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "BODY", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(7, "STIFF", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(8, "FDBK", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false),
    ParamSpec::continuous(9, "E.DPT", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(10, "E.RAT", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(11, "E.MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for ModalParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &MODAL_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as u8 as f32,
            Self::EXCITE => self.excite,
            Self::DECAY => self.decay,
            Self::BRIGHTNESS => self.brightness,
            Self::POSITION => self.position,
            Self::INHARM => self.inharm,
            Self::KS_BODY => self.ks_body,
            Self::KS_STIFFNESS => self.ks_stiffness,
            Self::KS_FEEDBACK => self.ks_feedback,
            Self::KS_ENS_DEPTH => self.ks_ens_depth,
            Self::KS_ENS_RATE => self.ks_ens_rate,
            Self::KS_ENS_MIX => self.ks_ens_mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODE => self.mode = ResonatorMode::from_u8(v as u8),
            Self::EXCITE => self.excite = v,
            Self::DECAY => self.decay = v,
            Self::BRIGHTNESS => self.brightness = v,
            Self::POSITION => self.position = v,
            Self::INHARM => self.inharm = v,
            Self::KS_BODY => self.ks_body = v,
            Self::KS_STIFFNESS => self.ks_stiffness = v,
            Self::KS_FEEDBACK => self.ks_feedback = v,
            Self::KS_ENS_DEPTH => self.ks_ens_depth = v,
            Self::KS_ENS_RATE => self.ks_ens_rate = v,
            Self::KS_ENS_MIX => self.ks_ens_mix = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::MODE).then(|| self.mode.disk_code())
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::MODE && apply_code(ResonatorMode::from_disk_code(code), |m| self.mode = m)
    }
}
