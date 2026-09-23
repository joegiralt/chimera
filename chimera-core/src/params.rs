use crate::block::{Block, ParamId, ParamSpec, ValFmt};

/// A single parameter value with range clamping
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl Param {
    pub const fn new(min: f32, max: f32, default: f32) -> Self {
        Self {
            value: default,
            min,
            max,
            default,
        }
    }

    pub fn set(&mut self, v: f32) {
        self.value = if v < self.min {
            self.min
        } else if v > self.max {
            self.max
        } else {
            v
        };
    }

    pub fn nudge(&mut self, delta: f32) {
        self.set(self.value + delta);
    }

    /// Normalized 0.0..1.0
    pub fn normalized(&self) -> f32 {
        (self.value - self.min) / (self.max - self.min)
    }

    /// Set from normalized 0.0..1.0
    pub fn set_normalized(&mut self, n: f32) {
        self.set(self.min + n * (self.max - self.min));
    }

    /// Returns the current value.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Apply a modulation offset scaled by the param's range.
    pub fn apply_mod_offset(&mut self, offset: f32) {
        self.value = (self.value + offset * (self.max - self.min))
            .clamp(self.min, self.max);
    }

    /// Snap to next coarse point in the given direction.
    /// `snap_points` are in normalized space (0..1), must be sorted ascending.
    pub fn snap_to(&mut self, delta: i8, snap_points: &[f32]) {
        let n = self.normalized();
        if delta > 0 {
            // Find first snap point above current (with small epsilon)
            for &sp in snap_points {
                if sp > n + 0.005 {
                    self.set_normalized(sp);
                    return;
                }
            }
            // Already at or above max snap point
            self.set_normalized(1.0);
        } else {
            // Find last snap point below current
            for &sp in snap_points.iter().rev() {
                if sp < n - 0.005 {
                    self.set_normalized(sp);
                    return;
                }
            }
            // Already at or below min snap point
            self.set_normalized(0.0);
        }
    }
}

/// Parameters for one voice's filter
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    pub fm_amount: f32,
    pub env_amount: f32,
    pub key_track: f32,
    pub mode: u8,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            drive: 0.0,
            fm_amount: 0.0,
            env_amount: 0.0,
            key_track: 0.0,
            mode: 2, // LP4
        }
    }
}

impl FilterParams {
    pub const CUTOFF: ParamId = ParamId(0);
    pub const RESONANCE: ParamId = ParamId(1);
    pub const DRIVE: ParamId = ParamId(2);
    pub const FM_AMOUNT: ParamId = ParamId(3);
    pub const ENV_AMOUNT: ParamId = ParamId(4);
    pub const KEY_TRACK: ParamId = ParamId(5);
}

/// Cutoff, resonance and drive are read by `Voice` every block. FM amount,
/// env amount and key track are never read (spec § Current state).
/// `mode` has no spec (not on any page; plan D16).
pub static FILTER_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "CUTOFF", ValFmt::Uni, 20.0, 20000.0, 1000.0, (20000.0 - 20.0) / 128.0, true),
    ParamSpec::continuous(1, "RESO", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(3, "FM", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "ENV", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
    ParamSpec::continuous(5, "TRACK", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for FilterParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FILTER_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::CUTOFF => self.cutoff,
            Self::RESONANCE => self.resonance,
            Self::DRIVE => self.drive,
            Self::FM_AMOUNT => self.fm_amount,
            Self::ENV_AMOUNT => self.env_amount,
            Self::KEY_TRACK => self.key_track,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CUTOFF => self.cutoff = v,
            Self::RESONANCE => self.resonance = v,
            Self::DRIVE => self.drive = v,
            Self::FM_AMOUNT => self.fm_amount = v,
            Self::ENV_AMOUNT => self.env_amount = v,
            Self::KEY_TRACK => self.key_track = v,
            _ => {}
        }
    }
}

/// Parameters for one envelope
#[derive(Clone, Copy, Debug)]
pub struct EnvParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub level: f32,
    pub vel_sens: f32,
}

impl Default for EnvParams {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.3,
            sustain: 0.7,
            release: 0.3,
            level: 1.0,
            vel_sens: 0.5,
        }
    }
}

impl EnvParams {
    pub const ATTACK: ParamId = ParamId(0);
    pub const DECAY: ParamId = ParamId(1);
    pub const SUSTAIN: ParamId = ParamId(2);
    pub const RELEASE: ParamId = ParamId(3);
    pub const LEVEL: ParamId = ParamId(4);
    pub const VEL_SENS: ParamId = ParamId(5);
}

/// Shared by all three envelopes. A/D/S/R are read by `Voice` every block
/// for the amp envelope (`envelopes[0]`); level and vel_sens are never read.
/// `envelopes[1..2]` are never read at all — `ParamAddr::modulatable`
/// excludes them (plan D7).
pub static ENV_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "ATK", ValFmt::Uni, 0.001, 10.0, 0.01, (10.0 - 0.001) / 128.0, true),
    ParamSpec::continuous(1, "DEC", ValFmt::Uni, 0.001, 10.0, 0.3, (10.0 - 0.001) / 128.0, true),
    ParamSpec::continuous(2, "SUS", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, true),
    ParamSpec::continuous(3, "REL", ValFmt::Uni, 0.001, 10.0, 0.3, (10.0 - 0.001) / 128.0, true),
    ParamSpec::continuous(4, "LEVEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "VEL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
];

impl Block for EnvParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ENV_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ATTACK => self.attack,
            Self::DECAY => self.decay,
            Self::SUSTAIN => self.sustain,
            Self::RELEASE => self.release,
            Self::LEVEL => self.level,
            Self::VEL_SENS => self.vel_sens,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::ATTACK => self.attack = v,
            Self::DECAY => self.decay = v,
            Self::SUSTAIN => self.sustain = v,
            Self::RELEASE => self.release = v,
            Self::LEVEL => self.level = v,
            Self::VEL_SENS => self.vel_sens = v,
            _ => {}
        }
    }
}

/// Parameters for pre-filter drive stage
#[derive(Clone, Copy, Debug)]
pub struct DriveParams {
    pub drive: f32,
    pub tone: f32,
    pub mix: f32,
}

impl Default for DriveParams {
    fn default() -> Self {
        Self {
            drive: 0.0,
            tone: 0.5,
            mix: 1.0,
        }
    }
}

impl DriveParams {
    pub const DRIVE: ParamId = ParamId(0);
    pub const TONE: ParamId = ParamId(1);
    pub const MIX: ParamId = ParamId(2);
}

/// All three are read by `Voice` every block from the modulated copy.
pub static DRIVE_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "TONE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "MIX", ValFmt::Bi, 0.0, 1.0, 1.0, 1.0 / 128.0, true),
];

impl Block for DriveParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &DRIVE_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::DRIVE => self.drive,
            Self::TONE => self.tone,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::DRIVE => self.drive = v,
            Self::TONE => self.tone = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}

/// Parameters for post-filter wavefolder
#[derive(Clone, Copy, Debug)]
pub struct FolderParams {
    pub fold: f32,
    pub symmetry: f32,
    pub mix: f32,
}

impl Default for FolderParams {
    fn default() -> Self {
        Self {
            fold: 0.0,
            symmetry: 0.5,
            mix: 0.5,
        }
    }
}

impl FolderParams {
    pub const FOLD: ParamId = ParamId(0);
    pub const SYMMETRY: ParamId = ParamId(1);
    pub const MIX: ParamId = ParamId(2);
}

/// All three are read by `Voice` every block from the modulated copy.
pub static FOLDER_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "FOLD", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "SYM", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "MIX", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
];

impl Block for FolderParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FOLDER_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::FOLD => self.fold,
            Self::SYMMETRY => self.symmetry,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::FOLD => self.fold = v,
            Self::SYMMETRY => self.symmetry = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}

/// Parameters for one FM operator.
#[derive(Clone, Copy, Debug)]
pub struct FmOpParams {
    pub waveform: Param,       // 0.0–7.0 (integer steps)
    pub coarse: Param,         // 0.0–63.0 (integer steps)
    pub fine: Param,           // 0.0–15.0 (integer steps)
    pub level: Param,          // 0.0–99.0 (integer steps)
    pub feedback: Param,       // 0.0–7.0 (integer steps)
    pub detune: Param,         // -7.0–7.0 (integer steps)
    pub velocity_sens: Param,  // 0.0–7.0 (integer steps)
    pub attack_rate: Param,    // 0.0–31.0 (integer steps)
    pub decay1_rate: Param,    // 0.0–31.0 (integer steps)
    pub decay1_level: Param,   // 0.0–15.0 (integer steps)
    pub decay2_rate: Param,    // 0.0–31.0 (integer steps)
    pub release_rate: Param,   // 1.0–15.0 (integer steps)
    pub rate_scaling: Param,   // 0.0–3.0 (integer steps)
}

impl Default for FmOpParams {
    fn default() -> Self {
        Self {
            waveform: Param::new(0.0, 7.0, 0.0),
            coarse: Param::new(0.0, 63.0, 4.0),
            fine: Param::new(0.0, 15.0, 0.0),
            level: Param::new(0.0, 99.0, 0.0),
            feedback: Param::new(0.0, 7.0, 0.0),
            detune: Param::new(-7.0, 7.0, 0.0),
            velocity_sens: Param::new(0.0, 7.0, 0.0),
            attack_rate: Param::new(0.0, 31.0, 31.0),
            decay1_rate: Param::new(0.0, 31.0, 0.0),
            decay1_level: Param::new(0.0, 15.0, 15.0),
            decay2_rate: Param::new(0.0, 31.0, 0.0),
            release_rate: Param::new(1.0, 15.0, 15.0),
            rate_scaling: Param::new(0.0, 3.0, 0.0),
        }
    }
}

/// Parameters for the 4-operator FM engine.
#[derive(Clone, Copy, Debug)]
pub struct FmParams {
    pub algorithm: Param,           // 0.0–7.0 (integer steps)
    pub operators: [FmOpParams; 4],
}

impl Default for FmParams {
    fn default() -> Self {
        let mut op0 = FmOpParams::default();
        op0.level = Param::new(0.0, 99.0, 99.0);
        Self {
            algorithm: Param::new(0.0, 7.0, 0.0),
            operators: [
                op0,
                FmOpParams::default(),
                FmOpParams::default(),
                FmOpParams::default(),
            ],
        }
    }
}

/// Which synthesis engine is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EngineType {
    #[default]
    Pizza = 0,
    Fm = 1,
    Modal = 2,
    Va = 3,
}

#[derive(Clone, Debug)]
pub struct ParamSnapshot {
    pub engine: EngineType,
    pub filter: FilterParams,
    pub drive: DriveParams,
    pub folder: FolderParams,
    pub envelopes: [EnvParams; 3],
    pub pizza: crate::dsp::pizza::PizzaParams,
    pub fm: FmParams,
    pub modal: crate::dsp::modal::ModalParams,
    pub reverb: crate::dsp::reverb::ReverbParams,
    pub delay: crate::dsp::delay::DelayParams,
    pub chorus: crate::dsp::chorus::ChorusParams,
    pub lfo: crate::dsp::lfo::LfoParams,
    pub volume: Param,
    pub pan: Param,
}

impl Default for ParamSnapshot {
    fn default() -> Self {
        Self {
            engine: EngineType::default(),
            filter: {
                let mut f = FilterParams::default();
                f.cutoff = 20000.0; // fully open
                f
            },
            drive: DriveParams::default(),
            folder: FolderParams::default(),
            envelopes: [EnvParams::default(); 3],
            pizza: crate::dsp::pizza::PizzaParams::default(),
            fm: FmParams::default(),
            modal: crate::dsp::modal::ModalParams::default(),
            delay: crate::dsp::delay::DelayParams::default(),
            chorus: crate::dsp::chorus::ChorusParams::default(),
            reverb: crate::dsp::reverb::ReverbParams {
                reverb_type: 0,
                time: 0.5,
                damping: 0.3,
                size: 0.5,
                mix: 0.0,
            },
            lfo: crate::dsp::lfo::LfoParams::default(),
            volume: Param::new(0.0, 1.0, 0.8),
            pan: Param::new(-1.0, 1.0, 0.0),
        }
    }
}
