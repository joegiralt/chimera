use crate::addr::{BlockRef, Blocks};
use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::filter::{FilterMode, SVF_MODE_NAMES, SVF_MODES};
use crate::dsp::modulator::{EnvSpeed, EnvType, FuncMode, FuncParams, HoldPos, pick};

/// Parameters for one voice's filter.
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    /// Private: `set_mode` keeps it in the SVF's list (spec § 7).
    mode: FilterMode,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            drive: 0.0,
            mode: FilterMode::Lp24,
        }
    }
}

impl FilterParams {
    pub const CUTOFF: ParamId = ParamId(0);
    pub const RESONANCE: ParamId = ParamId(1);
    pub const DRIVE: ParamId = ParamId(2);
    // 3 (FM), 4 (ENV) and 5 (KEY) are retired, never reused (ADR 0009).
    pub const MODE: ParamId = ParamId(7);

    pub fn mode(&self) -> FilterMode {
        self.mode
    }

    /// Sets `m` if the SVF has it; returns whether it did.
    pub fn set_mode(&mut self, m: FilterMode) -> bool {
        let ok = SVF_MODES.contains(&m);
        if ok {
            self.mode = m;
        }
        ok
    }
}

/// Every one read by `Voice` per block; MODE is an Enum, so not modulatable.
pub static FILTER_SPECS: [ParamSpec; 4] = [
    ParamSpec::continuous(
        0,
        "CUTOFF",
        ValFmt::Uni,
        20.0,
        20000.0,
        1000.0,
        (20000.0 - 20.0) / 128.0,
        true,
    )
    .octaves(crate::dsp::filter::CUTOFF_OCTAVES),
    ParamSpec::continuous(1, "RESO", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::choice(7, "MODE", ValFmt::Names(&SVF_MODE_NAMES), 7.0, 0.0),
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
            Self::MODE => SVF_MODES.iter().position(|&m| m == self.mode).unwrap_or(0) as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CUTOFF => self.cutoff = v,
            Self::RESONANCE => self.resonance = v,
            Self::DRIVE => self.drive = v,
            Self::MODE => {
                self.set_mode(SVF_MODES[(v.max(0.0) as usize).min(SVF_MODES.len() - 1)]);
            }
            _ => {}
        }
    }
}

/// One ENV slot's parameters (spec § Data model).
#[derive(Clone, Copy, Debug)]
pub struct EnvParams {
    /// A, D, R and H: positions 0..1 on SPEED's exponential ranges.
    pub attack: f32,
    pub decay: f32,
    pub release: f32,
    pub hold: f32,
    /// S: a level, 0..1.
    pub sustain: f32,
    /// LEVEL destination's stored value; the peak comes from its routes.
    pub level: f32,
    /// Unread and off the pages (#112).
    pub vel_sens: f32,
    pub env_type: EnvType,
    pub speed: EnvSpeed,
    pub hold_pos: HoldPos,
    /// TIME destination's stored 0.
    pub time: f32,
    /// Envelope B's MODE, FORM, RISE, FALL and SHAPE.
    pub func: FuncParams,
}

impl Default for EnvParams {
    /// Today's times at MED: A 10 ms, D and R 300 ms, S 0.7; H 0.001 ms.
    fn default() -> Self {
        Self {
            attack: 0.189,
            decay: 0.559,
            release: 0.559,
            hold: 0.0,
            sustain: 0.7,
            level: 1.0,
            vel_sens: 0.5,
            env_type: EnvType::A,
            speed: EnvSpeed::Med,
            hold_pos: HoldPos::Ahdsr,
            time: 0.0,
            func: FuncParams::ENV,
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
    pub const HOLD: ParamId = ParamId(6);
    pub const TYPE: ParamId = ParamId(7);
    pub const SPEED: ParamId = ParamId(8);
    pub const HOLD_POS: ParamId = ParamId(9);
    pub const TIME: ParamId = ParamId(10);
    pub const MODE: ParamId = ParamId(11);
    pub const FORM: ParamId = ParamId(12);
    pub const RISE: ParamId = ParamId(13);
    pub const FALL: ParamId = ParamId(14);
    pub const SHAPE: ParamId = ParamId(15);
}

/// Positions and levels, per block. LEVEL and TIME are hidden destinations
/// (Task 9 of the filter-routing plan makes them modulatable).
pub static ENV_SPECS: [ParamSpec; 16] = [
    ParamSpec::continuous(0, "ATK", ValFmt::Uni, 0.0, 1.0, 0.189, 1.0 / 128.0, false),
    ParamSpec::continuous(1, "DEC", ValFmt::Uni, 0.0, 1.0, 0.559, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "SUS", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "REL", ValFmt::Uni, 0.0, 1.0, 0.559, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "LEVEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "VEL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "H", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::choice(7, "TYPE", ValFmt::Names(&["A", "B"]), 1.0, 0.0),
    ParamSpec::choice(
        8,
        "SPEED",
        ValFmt::Names(&["FAST", "MED", "SLOW"]),
        2.0,
        1.0,
    ),
    ParamSpec::choice(
        9,
        "HOLD",
        ValFmt::Names(&["OFF", "AHDSR", "GATE EXT"]),
        2.0,
        1.0,
    ),
    ParamSpec::continuous(10, "TIME", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
    ParamSpec::choice(
        11,
        "MODE",
        ValFmt::Names(&["ENV", "LFO", "BURST"]),
        2.0,
        0.0,
    ),
    ParamSpec::choice(12, "FORM", ValFmt::Names(&["AD", "AHR", "CYCLE"]), 2.0, 0.0),
    ParamSpec::continuous(13, "RISE", ValFmt::Uni, 0.0, 1.0, 0.206, 1.0 / 128.0, false),
    ParamSpec::continuous(14, "FALL", ValFmt::Uni, 0.0, 1.0, 0.640, 1.0 / 128.0, false),
    ParamSpec::continuous(15, "SHAPE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
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
            Self::HOLD => self.hold,
            Self::TYPE => self.env_type as u8 as f32,
            Self::SPEED => self.speed as u8 as f32,
            Self::HOLD_POS => self.hold_pos as u8 as f32,
            Self::TIME => self.time,
            Self::MODE => self.func.mode as u8 as f32,
            Self::FORM => self.func.form_index(),
            Self::RISE => self.func.rise,
            Self::FALL => self.func.fall,
            Self::SHAPE => self.func.shape,
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
            Self::HOLD => self.hold = v,
            Self::TYPE => self.env_type = pick(&EnvType::ALL, v),
            Self::SPEED => self.speed = pick(&EnvSpeed::ALL, v),
            Self::HOLD_POS => self.hold_pos = pick(&HoldPos::ALL, v),
            Self::TIME => self.time = v,
            Self::MODE => self.func.mode = pick(&FuncMode::ALL, v),
            Self::FORM => self.func.set_form_index(v),
            Self::RISE => self.func.rise = v,
            Self::FALL => self.func.fall = v,
            Self::SHAPE => self.func.shape = v,
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

    /// Short display label for the engine and its chain.
    pub fn label(self) -> &'static str {
        match self {
            EngineType::Algo => "Algo",
            EngineType::Modal => "Modal",
        }
    }
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
    /// Private: set only through `for_engine` (and so `Sound::init`) — one
    /// source of truth for engine choice (spec §6).
    engine: EngineType,
    pub filter: FilterParams,
    pub drive: DriveParams,
    pub folder: FolderParams,
    pub envelopes: [EnvParams; 3],
    pub algo: crate::dsp::algo::params::AlgoParams,
    pub modal: crate::dsp::modal::ModalParams,
    pub lfos: [crate::dsp::lfo::LfoParams; 3],
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
            BlockRef::Env(s) => &self.envelopes[s.index()],
            BlockRef::Lfo(s) => &self.lfos[s.index()],
            BlockRef::Out => &self.out,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::Theme => return None,
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
            BlockRef::Env(s) => &mut self.envelopes[s.index()],
            BlockRef::Lfo(s) => &mut self.lfos[s.index()],
            BlockRef::Out => &mut self.out,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::Theme => return None,
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
            envelopes: [
                EnvParams::default(),
                EnvParams::default(),
                EnvParams {
                    env_type: EnvType::B,
                    ..EnvParams::default()
                },
            ],
            algo: crate::dsp::algo::params::AlgoParams::default(),
            modal: crate::dsp::modal::ModalParams::default(),
            lfos: [crate::dsp::lfo::LfoParams::default(); 3],
            out: OutParams::default(),
        }
    }
}
