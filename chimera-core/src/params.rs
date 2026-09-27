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

/// Which synthesis engine is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EngineType {
    #[default]
    Algo = 0,
    Modal = 1,
}

impl EngineType {
    /// Every engine. Tests iterate this; see `engines_test.rs` for the
    /// exhaustive-match guard that makes a new variant a compile error there.
    pub const ALL: [EngineType; 2] = [EngineType::Algo, EngineType::Modal];
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
    pub algo: crate::dsp::algo::params::AlgoParams,
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
            BlockRef::Modal => &self.modal,
            BlockRef::Algo => &self.algo,
            BlockRef::AlgoOp(op) => &self.algo.ops[op.index()],
            BlockRef::Drive => &self.drive,
            BlockRef::Filter => &self.filter,
            BlockRef::Folder => &self.folder,
            BlockRef::AmpEnv => &self.envelopes[0],
            BlockRef::FilterEnv => &self.envelopes[1],
            BlockRef::AuxEnv => &self.envelopes[2],
            BlockRef::Lfo => &self.lfo,
            BlockRef::Out => &self.out,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Part => return None,
        })
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        Some(match b {
            BlockRef::Modal => &mut self.modal,
            BlockRef::Algo => &mut self.algo,
            BlockRef::AlgoOp(op) => &mut self.algo.ops[op.index()],
            BlockRef::Drive => &mut self.drive,
            BlockRef::Filter => &mut self.filter,
            BlockRef::Folder => &mut self.folder,
            BlockRef::AmpEnv => &mut self.envelopes[0],
            BlockRef::FilterEnv => &mut self.envelopes[1],
            BlockRef::AuxEnv => &mut self.envelopes[2],
            BlockRef::Lfo => &mut self.lfo,
            BlockRef::Out => &mut self.out,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Part => return None,
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
            algo: crate::dsp::algo::params::AlgoParams::default(),
            modal: crate::dsp::modal::ModalParams::default(),
            lfo: crate::dsp::lfo::LfoParams::default(),
            out: OutParams::default(),
        }
    }
}
