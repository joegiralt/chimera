use crate::addr::{BlockRef, Blocks};
use crate::block::{Block, ParamId, ParamSpec, ValFmt};

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
    ParamSpec::continuous(
        0,
        "CUTOFF",
        ValFmt::Uni,
        20.0,
        20000.0,
        1000.0,
        (20000.0 - 20.0) / 128.0,
        true,
    ),
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
    ParamSpec::continuous(
        0,
        "ATK",
        ValFmt::Uni,
        0.001,
        10.0,
        0.01,
        (10.0 - 0.001) / 128.0,
        true,
    ),
    ParamSpec::continuous(
        1,
        "DEC",
        ValFmt::Uni,
        0.001,
        10.0,
        0.3,
        (10.0 - 0.001) / 128.0,
        true,
    ),
    ParamSpec::continuous(2, "SUS", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, true),
    ParamSpec::continuous(
        3,
        "REL",
        ValFmt::Uni,
        0.001,
        10.0,
        0.3,
        (10.0 - 0.001) / 128.0,
        true,
    ),
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

/// Parameters for one FM operator. Stepped params that are modulatable
/// (`level`, `feedback`) are `f32` so a modulated copy can hold fractional
/// values (the DSP truncates `as u8`); the others keep integer types (plan D18).
#[derive(Clone, Copy, Debug)]
pub struct FmOpParams {
    pub waveform: u8,      // 0–7
    pub coarse: u8,        // 0–63
    pub fine: u8,          // 0–15
    pub level: f32,        // 0–99 (integer steps)
    pub feedback: f32,     // 0–7 (integer steps)
    pub detune: i8,        // -7–7
    pub velocity_sens: u8, // 0–7
    pub attack_rate: u8,   // 0–31
    pub decay1_rate: u8,   // 0–31
    pub decay1_level: u8,  // 0–15
    pub decay2_rate: u8,   // 0–31
    pub release_rate: u8,  // 0–15 (plan D4)
    pub rate_scaling: u8,  // 0–3
}

impl Default for FmOpParams {
    fn default() -> Self {
        Self {
            waveform: 0,
            coarse: 4,
            fine: 0,
            level: 0.0,
            feedback: 0.0,
            detune: 0,
            velocity_sens: 0,
            attack_rate: 31,
            decay1_rate: 0,
            decay1_level: 15,
            decay2_rate: 0,
            release_rate: 15,
            rate_scaling: 0,
        }
    }
}

impl FmOpParams {
    pub const WAVEFORM: ParamId = ParamId(0);
    pub const COARSE: ParamId = ParamId(1);
    pub const FINE: ParamId = ParamId(2);
    pub const LEVEL: ParamId = ParamId(3);
    pub const FEEDBACK: ParamId = ParamId(4);
    pub const DETUNE: ParamId = ParamId(5);
    pub const VELOCITY_SENS: ParamId = ParamId(6);
    pub const ATTACK_RATE: ParamId = ParamId(7);
    pub const DECAY1_RATE: ParamId = ParamId(8);
    pub const DECAY1_LEVEL: ParamId = ParamId(9);
    pub const DECAY2_RATE: ParamId = ParamId(10);
    pub const RELEASE_RATE: ParamId = ParamId(11);
    pub const RATE_SCALING: ParamId = ParamId(12);
}

/// Level and feedback are read every block (`FmOperator::update_live`);
/// waveform is too, but it is a choice. Ratios, detune and the envelope are
/// read only at note-on, so they are not modulatable.
pub static FM_OP_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(0, "WAVE", ValFmt::Int(7), 7.0, 0.0),
    ParamSpec::stepped(1, "CRSE", ValFmt::Int(63), 0.0, 63.0, 4.0, false),
    ParamSpec::stepped(2, "FINE", ValFmt::Int(15), 0.0, 15.0, 0.0, false),
    ParamSpec::stepped(3, "LEVEL", ValFmt::Uni, 0.0, 99.0, 0.0, true),
    ParamSpec::stepped(4, "FDBK", ValFmt::Int(7), 0.0, 7.0, 0.0, true),
    ParamSpec::stepped(5, "DETUN", ValFmt::Bi, -7.0, 7.0, 0.0, false),
    ParamSpec::stepped(6, "V.SNS", ValFmt::Int(7), 0.0, 7.0, 0.0, false),
    ParamSpec::stepped(7, "AR", ValFmt::Int(31), 0.0, 31.0, 31.0, false),
    ParamSpec::stepped(8, "D1R", ValFmt::Int(31), 0.0, 31.0, 0.0, false),
    ParamSpec::stepped(9, "D1L", ValFmt::Int(15), 0.0, 15.0, 15.0, false),
    ParamSpec::stepped(10, "D2R", ValFmt::Int(31), 0.0, 31.0, 0.0, false),
    ParamSpec::stepped(11, "RR", ValFmt::Int(15), 0.0, 15.0, 15.0, false),
    ParamSpec::stepped(12, "RS", ValFmt::Int(3), 0.0, 3.0, 0.0, false),
];

impl Block for FmOpParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FM_OP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVEFORM => self.waveform as f32,
            Self::COARSE => self.coarse as f32,
            Self::FINE => self.fine as f32,
            Self::LEVEL => self.level,
            Self::FEEDBACK => self.feedback,
            Self::DETUNE => self.detune as f32,
            Self::VELOCITY_SENS => self.velocity_sens as f32,
            Self::ATTACK_RATE => self.attack_rate as f32,
            Self::DECAY1_RATE => self.decay1_rate as f32,
            Self::DECAY1_LEVEL => self.decay1_level as f32,
            Self::DECAY2_RATE => self.decay2_rate as f32,
            Self::RELEASE_RATE => self.release_rate as f32,
            Self::RATE_SCALING => self.rate_scaling as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::WAVEFORM => self.waveform = v as u8,
            Self::COARSE => self.coarse = v as u8,
            Self::FINE => self.fine = v as u8,
            Self::LEVEL => self.level = v,
            Self::FEEDBACK => self.feedback = v,
            Self::DETUNE => self.detune = v as i8,
            Self::VELOCITY_SENS => self.velocity_sens = v as u8,
            Self::ATTACK_RATE => self.attack_rate = v as u8,
            Self::DECAY1_RATE => self.decay1_rate = v as u8,
            Self::DECAY1_LEVEL => self.decay1_level = v as u8,
            Self::DECAY2_RATE => self.decay2_rate = v as u8,
            Self::RELEASE_RATE => self.release_rate = v as u8,
            Self::RATE_SCALING => self.rate_scaling = v as u8,
            _ => {}
        }
    }
}

/// Parameters for the 4-operator FM engine.
#[derive(Clone, Copy, Debug)]
pub struct FmParams {
    pub algorithm: u8, // 0–7
    pub operators: [FmOpParams; 4],
}

impl Default for FmParams {
    fn default() -> Self {
        let op0 = FmOpParams {
            level: 99.0,
            ..Default::default()
        };
        Self {
            algorithm: 0,
            operators: [
                op0,
                FmOpParams::default(),
                FmOpParams::default(),
                FmOpParams::default(),
            ],
        }
    }
}

impl FmParams {
    pub const ALGORITHM: ParamId = ParamId(0);
}

/// Engine-level FM params. Operators are separate blocks (`FmOpParams`).
pub static FM_SPECS: [ParamSpec; 1] = [ParamSpec::choice(0, "ALG", ValFmt::OneBased(7), 7.0, 0.0)];

impl Block for FmParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FM_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ALGORITHM => self.algorithm as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        if id == Self::ALGORITHM {
            self.algorithm = v as u8;
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

impl EngineType {
    /// Every engine. Tests iterate this; see `engines_test.rs` for the
    /// exhaustive-match guard that makes a new variant a compile error there.
    pub const ALL: [EngineType; 4] = [
        EngineType::Pizza,
        EngineType::Fm,
        EngineType::Modal,
        EngineType::Va,
    ];
}

/// Voice output stage: level into the mixer and pan.
#[derive(Clone, Copy, Debug)]
pub struct OutParams {
    pub volume: f32,
    pub pan: f32,
}

impl Default for OutParams {
    fn default() -> Self {
        Self {
            volume: 0.8,
            pan: 0.0,
        }
    }
}

impl OutParams {
    pub const VOLUME: ParamId = ParamId(0);
    pub const PAN: ParamId = ParamId(1);
}

/// Volume is read by `Voice`'s VCA every block (newly modulatable); pan is
/// not used by `Voice`.
pub static OUT_SPECS: [ParamSpec; 2] = [
    ParamSpec::continuous(0, "LEVEL", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "PAN", ValFmt::Pan, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
];

impl Block for OutParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &OUT_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::VOLUME => self.volume,
            Self::PAN => self.pan,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::VOLUME => self.volume = v,
            Self::PAN => self.pan = v,
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParamSnapshot {
    /// Private: set only through `for_engine` (and so `Sound::init`, from
    /// `ChainType::engine`) — one source of truth for engine choice (spec §6).
    engine: EngineType,
    pub filter: FilterParams,
    pub drive: DriveParams,
    pub folder: FolderParams,
    pub envelopes: [EnvParams; 3],
    pub pizza: crate::dsp::pizza::PizzaParams,
    pub fm: FmParams,
    pub modal: crate::dsp::modal::ModalParams,
    pub lfo: crate::dsp::lfo::LfoParams,
    pub out: OutParams,
}

impl ParamSnapshot {
    /// Default params for `engine`.
    pub fn for_engine(engine: EngineType) -> Self {
        Self {
            engine,
            ..Self::default()
        }
    }

    pub fn engine(&self) -> EngineType {
        self.engine
    }
}

/// The one exhaustive dispatch from a block address to a Sound's values
/// (spec §2). UI and modulation go through this; DSP reads fields. The FX
/// and the mix settings belong to the Performance and Part, not the Sound.
impl Blocks for ParamSnapshot {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        Some(match b {
            BlockRef::Pizza => &self.pizza,
            BlockRef::Modal => &self.modal,
            BlockRef::Fm => &self.fm,
            BlockRef::FmOp(op) => &self.fm.operators[op.index()],
            BlockRef::Drive => &self.drive,
            BlockRef::Filter => &self.filter,
            BlockRef::Folder => &self.folder,
            BlockRef::AmpEnv => &self.envelopes[0],
            BlockRef::FilterEnv => &self.envelopes[1],
            BlockRef::AuxEnv => &self.envelopes[2],
            BlockRef::Lfo => &self.lfo,
            BlockRef::Out => &self.out,
            BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb | BlockRef::Part => return None,
        })
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        Some(match b {
            BlockRef::Pizza => &mut self.pizza,
            BlockRef::Modal => &mut self.modal,
            BlockRef::Fm => &mut self.fm,
            BlockRef::FmOp(op) => &mut self.fm.operators[op.index()],
            BlockRef::Drive => &mut self.drive,
            BlockRef::Filter => &mut self.filter,
            BlockRef::Folder => &mut self.folder,
            BlockRef::AmpEnv => &mut self.envelopes[0],
            BlockRef::FilterEnv => &mut self.envelopes[1],
            BlockRef::AuxEnv => &mut self.envelopes[2],
            BlockRef::Lfo => &mut self.lfo,
            BlockRef::Out => &mut self.out,
            BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb | BlockRef::Part => return None,
        })
    }
}

impl Default for ParamSnapshot {
    fn default() -> Self {
        Self {
            engine: EngineType::default(),
            filter: FilterParams {
                cutoff: 20000.0, // fully open
                ..Default::default()
            },
            drive: DriveParams::default(),
            folder: FolderParams::default(),
            envelopes: [EnvParams::default(); 3],
            pizza: crate::dsp::pizza::PizzaParams::default(),
            fm: FmParams::default(),
            modal: crate::dsp::modal::ModalParams::default(),
            lfo: crate::dsp::lfo::LfoParams::default(),
            out: OutParams::default(),
        }
    }
}
