use crate::addr::{BlockRef, Blocks};
use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::filter::{FilterKind, FilterMode, KIND_NAMES, SVF_MODE_NAMES};
use crate::dsp::modulator::{
    EnvForm, EnvSpeed, EnvType, FuncMode, FuncParams, HoldPos, LfoForm, pick,
};

/// Parameters for one voice's filter.
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    /// Private: kept consistent with mode through set_kind and set_mode.
    kind: FilterKind,
    /// Private: `set_mode` keeps it in the kind's list (spec § 7).
    mode: FilterMode,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            drive: 0.0,
            kind: FilterKind::Svf,
            mode: FilterMode::Lp24,
        }
    }
}

impl FilterParams {
    pub const CUTOFF: ParamId = ParamId(0);
    pub const RESONANCE: ParamId = ParamId(1);
    pub const DRIVE: ParamId = ParamId(2);
    // 3 (FM), 4 (ENV) and 5 (KEY) are retired, never reused (ADR 0009).
    pub const KIND: ParamId = ParamId(6);
    pub const MODE: ParamId = ParamId(7);

    pub fn mode(&self) -> FilterMode {
        self.mode
    }

    pub fn kind(&self) -> FilterKind {
        self.kind
    }

    /// Change KIND (spec § 7): MODE stays if the new kind has it.
    pub fn set_kind(&mut self, k: FilterKind) {
        *self = crate::dsp::filter::kind_change(*self, k);
    }

    /// Only `kind_change` calls this; MODE is fixed up there.
    pub(crate) fn set_kind_raw(&mut self, k: FilterKind) {
        self.kind = k;
    }

    /// Sets `m` if the kind has it; returns whether it did.
    pub fn set_mode(&mut self, m: FilterMode) -> bool {
        let ok = self.kind.modes().contains(&m);
        if ok {
            self.mode = m;
        }
        ok
    }
}

/// Every one read by `Voice` per block; MODE is an Enum, so not modulatable.
pub static FILTER_SPECS: [ParamSpec; 5] = [
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
    .ident("CUTOFF")
    .octaves(crate::dsp::filter::CUTOFF_OCTAVES)
    .short("CUT"),
    ParamSpec::continuous(1, "RESO", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true).ident("RESO"),
    ParamSpec::continuous(2, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true).ident("DRIVE"),
    // A choice among the one built kind.
    ParamSpec::choice(6, "KIND", ValFmt::Names(&KIND_NAMES), 0.0, 0.0).ident("KIND"),
    ParamSpec::choice(7, "MODE", ValFmt::Names(&SVF_MODE_NAMES), 7.0, 0.0).ident("MODE"),
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
            Self::KIND => FilterKind::BUILT
                .iter()
                .position(|&k| k == self.kind)
                .unwrap_or(0) as f32,
            Self::MODE => self
                .kind
                .modes()
                .iter()
                .position(|&m| m == self.mode)
                .unwrap_or(0) as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CUTOFF => self.cutoff = v,
            Self::RESONANCE => self.resonance = v,
            Self::DRIVE => self.drive = v,
            Self::KIND => self.set_kind(FilterKind::from_index(v)),
            Self::MODE => {
                let m = self.kind.modes();
                self.set_mode(m[(v.max(0.0) as usize).min(m.len() - 1)]);
            }
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::KIND => Some(self.kind.disk_code()),
            Self::MODE => Some(self.mode.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::KIND => Some(self.kind.disk_ident()),
            Self::MODE => Some(self.mode.disk_ident()),
            _ => None,
        }
    }

    /// MODE's code is the mode's own, and it is refused if the KIND lacks it.
    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::KIND => apply_code(FilterKind::from_disk_code(code), |k| self.set_kind(k)),
            Self::MODE => FilterMode::from_disk_code(code).is_some_and(|m| self.set_mode(m)),
            _ => false,
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
    /// The three FORMs, one per MODE and each stored on its own, so decoding
    /// never depends on MODE and a MODE's remembered FORM survives a reload.
    /// `FORM` is the live view of the current MODE's.
    pub const FORM_ENV: ParamId = ParamId(16);
    pub const FORM_LFO: ParamId = ParamId(17);
    pub const FORM_BURST: ParamId = ParamId(18);
}

/// Positions and levels, per block. LEVEL and TIME (hidden, primed from the
/// stage cells), RISE, FALL and SHAPE are modulatable.
pub static ENV_SPECS: [ParamSpec; 19] = [
    ParamSpec::continuous(0, "ATK", ValFmt::Uni, 0.0, 1.0, 0.189, 1.0 / 128.0, false).ident("ATK"),
    ParamSpec::continuous(1, "DEC", ValFmt::Uni, 0.0, 1.0, 0.559, 1.0 / 128.0, false).ident("DEC"),
    ParamSpec::continuous(2, "SUS", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false).ident("SUS"),
    ParamSpec::continuous(3, "REL", ValFmt::Uni, 0.0, 1.0, 0.559, 1.0 / 128.0, false).ident("REL"),
    ParamSpec::continuous(4, "LEVEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, true).ident("LEVEL"),
    ParamSpec::continuous(5, "VEL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false).ident("VEL"),
    ParamSpec::continuous(6, "H", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false).ident("H"),
    ParamSpec::choice(7, "TYPE", ValFmt::Names(&["A", "B"]), 1.0, 0.0).ident("TYPE"),
    ParamSpec::choice(
        8,
        "SPEED",
        ValFmt::Names(&["FAST", "MED", "SLOW"]),
        2.0,
        1.0,
    )
    .ident("SPEED"),
    ParamSpec::choice(
        9,
        "HOLD",
        ValFmt::Names(&["OFF", "AHDSR", "GATE EXT"]),
        2.0,
        1.0,
    )
    .ident("HOLD"),
    ParamSpec::continuous(10, "TIME", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, true).ident("TIME"),
    ParamSpec::choice(
        11,
        "MODE",
        ValFmt::Names(&["ENV", "LFO", "BURST"]),
        2.0,
        0.0,
    )
    .ident("MODE"),
    // Live: the current MODE's FORM slot (16..=18), which is what is stored.
    ParamSpec::choice(12, "FORM", ValFmt::Names(&["AD", "AHR", "CYCLE"]), 2.0, 0.0)
        .ident("FORM")
        .live(),
    ParamSpec::continuous(13, "RISE", ValFmt::Uni, 0.0, 1.0, 0.206, 1.0 / 128.0, true)
        .ident("RISE"),
    ParamSpec::continuous(14, "FALL", ValFmt::Uni, 0.0, 1.0, 0.640, 1.0 / 128.0, true)
        .ident("FALL"),
    ParamSpec::continuous(15, "SHAPE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true)
        .ident("SHAPE")
        .short("SHAP"),
    ParamSpec::choice(
        16,
        "ENV FORM",
        ValFmt::Names(&["AD", "AHR", "CYCLE"]),
        2.0,
        0.0,
    )
    .ident("ENV_FORM"),
    ParamSpec::choice(
        17,
        "LFO FORM",
        ValFmt::Names(&["FREE", "SYNC", "LFV"]),
        2.0,
        0.0,
    )
    .ident("LFO_FORM"),
    ParamSpec::choice(
        18,
        "BRST FORM",
        ValFmt::Names(&["AD", "AHR", "CYCLE"]),
        2.0,
        0.0,
    )
    .ident("BRST_FORM"),
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
            Self::FORM_ENV => self.func.env_form as u8 as f32,
            Self::FORM_LFO => self.func.lfo_form as u8 as f32,
            Self::FORM_BURST => self.func.burst_form as u8 as f32,
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
            Self::FORM_ENV => self.func.env_form = pick(&EnvForm::ALL, v),
            Self::FORM_LFO => self.func.lfo_form = pick(&LfoForm::ALL, v),
            Self::FORM_BURST => self.func.burst_form = pick(&EnvForm::ALL, v),
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::TYPE => Some(self.env_type.disk_code()),
            Self::SPEED => Some(self.speed.disk_code()),
            Self::HOLD_POS => Some(self.hold_pos.disk_code()),
            Self::MODE => Some(self.func.mode.disk_code()),
            Self::FORM_ENV => Some(self.func.env_form.disk_code()),
            Self::FORM_LFO => Some(self.func.lfo_form.disk_code()),
            Self::FORM_BURST => Some(self.func.burst_form.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::TYPE => Some(self.env_type.disk_ident()),
            Self::SPEED => Some(self.speed.disk_ident()),
            Self::HOLD_POS => Some(self.hold_pos.disk_ident()),
            Self::MODE => Some(self.func.mode.disk_ident()),
            Self::FORM_ENV => Some(self.func.env_form.disk_ident()),
            Self::FORM_LFO => Some(self.func.lfo_form.disk_ident()),
            Self::FORM_BURST => Some(self.func.burst_form.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::TYPE => apply_code(EnvType::from_disk_code(code), |t| self.env_type = t),
            Self::SPEED => apply_code(EnvSpeed::from_disk_code(code), |s| self.speed = s),
            Self::HOLD_POS => apply_code(HoldPos::from_disk_code(code), |h| self.hold_pos = h),
            Self::MODE => apply_code(FuncMode::from_disk_code(code), |m| self.func.mode = m),
            Self::FORM_ENV => apply_code(EnvForm::from_disk_code(code), |f| self.func.env_form = f),
            Self::FORM_LFO => apply_code(LfoForm::from_disk_code(code), |f| self.func.lfo_form = f),
            Self::FORM_BURST => {
                apply_code(EnvForm::from_disk_code(code), |f| self.func.burst_form = f)
            }
            _ => false,
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
    ParamSpec::continuous(0, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true).ident("DRIVE"),
    ParamSpec::continuous(1, "TONE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true).ident("TONE"),
    ParamSpec::continuous(2, "MIX", ValFmt::Bi, 0.0, 1.0, 1.0, 1.0 / 128.0, true).ident("MIX"),
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
    ParamSpec::continuous(0, "FOLD", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true).ident("FOLD"),
    ParamSpec::continuous(1, "SYM", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true).ident("SYM"),
    ParamSpec::continuous(2, "MIX", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true).ident("MIX"),
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

impl DiskCode for EngineType {
    fn disk_code(self) -> u8 {
        match self {
            EngineType::Algo => 0,
            EngineType::Modal => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            EngineType::Algo => "ALGO",
            EngineType::Modal => "MODAL",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(EngineType::Algo),
            1 => Some(EngineType::Modal),
            _ => None,
        }
    }
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
    /// The VCA destination's stored 0 (spec § 4): its value is the sum of its routes.
    pub vca: f32,
    /// AMP's VEL: the VCA's velocity sensitivity, 0..1.
    pub vca_vel: f32,
}

/// INIT's OUT LEVEL: ALGO INIT's C4 at velocity 100 at the factory median,
/// −22 LUFS on P1 (ADR 0063); 45/128, on the knob's grid.
pub const INIT_VOLUME: f32 = 45.0 / 128.0;

impl Default for OutParams {
    fn default() -> Self {
        Self {
            volume: INIT_VOLUME,
            pan: 0.0,
            vca: 0.0,
            vca_vel: 1.0,
        }
    }
}

impl OutParams {
    pub const VOLUME: ParamId = ParamId(0);
    pub const PAN: ParamId = ParamId(1);
    pub const VCA: ParamId = ParamId(2);
    pub const VCA_VEL: ParamId = ParamId(3);
}

/// Volume and VEL are read by `Voice`'s VCA every block, VCA is a hidden
/// destination; pan is not used by `Voice`.
pub static OUT_SPECS: [ParamSpec; 4] = [
    ParamSpec::continuous(
        0,
        "LEVEL",
        ValFmt::Uni,
        0.0,
        1.0,
        INIT_VOLUME,
        1.0 / 128.0,
        true,
    )
    .ident("LEVEL"),
    ParamSpec::continuous(1, "PAN", ValFmt::Pan, -1.0, 1.0, 0.0, 2.0 / 128.0, false).ident("PAN"),
    ParamSpec::continuous(2, "VCA", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true).ident("VCA"),
    ParamSpec::continuous(3, "VEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false).ident("VEL"),
];

impl Block for OutParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &OUT_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::VOLUME => self.volume,
            Self::PAN => self.pan,
            Self::VCA => self.vca,
            Self::VCA_VEL => self.vca_vel,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::VOLUME => self.volume = v,
            Self::PAN => self.pan = v,
            Self::VCA => self.vca = v,
            Self::VCA_VEL => self.vca_vel = v,
            _ => {}
        }
    }
}

/// What a note that steals a sounding voice of its own Part and model
/// does to it (#254, ADR 0065).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Steal {
    /// Cut: the note strikes at its own pitch, as a fresh note.
    #[default]
    Cut,
    /// The ring glides to the new note's pitch over GLIDE TIME, struck anew
    /// on the way, as a Prophet's glide.
    Glide,
}

pub const STEAL_NAMES: [&str; 2] = ["CUT", "GLIDE"];

impl DiskCode for Steal {
    fn disk_code(self) -> u8 {
        match self {
            Steal::Cut => 0,
            Steal::Glide => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Steal::Cut => "CUT",
            Steal::Glide => "GLIDE",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Steal::Cut),
            1 => Some(Steal::Glide),
            _ => None,
        }
    }
}

/// The voice's pitch (ADR 0042): its offset, one block for every engine,
/// so a route to it survives an engine switch; and how a steal moves it
/// (#254).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitchParams {
    /// Semitones, −24..=24; a modulated copy stays fractional.
    pub pitch: f32,
    /// Cents, −100..=100.
    pub fine: f32,
    pub steal: Steal,
    /// GLIDE TIME's slider position, 0..1 on `law::GLIDE_TIME`.
    pub glide_time: f32,
}

/// GLIDE TIME's position at 150 ms.
pub const INIT_GLIDE_TIME: f32 = 0.659_215_8;

impl Default for PitchParams {
    fn default() -> Self {
        Self {
            pitch: 0.0,
            fine: 0.0,
            steal: Steal::Cut,
            glide_time: INIT_GLIDE_TIME,
        }
    }
}

impl PitchParams {
    pub const PITCH: ParamId = ParamId(0);
    pub const FINE: ParamId = ParamId(1);
    pub const STEAL: ParamId = ParamId(2);
    pub const GLIDE_TIME: ParamId = ParamId(3);

    /// The whole offset in semitones.
    pub fn semitones(&self) -> f32 {
        self.pitch + self.fine / 100.0
    }

    /// The frequency ratio, exactly 1 at no offset (no maths: the goldens);
    /// `fast_exp2`, within 0.1 cent, as it runs every block.
    pub fn ratio(&self) -> f32 {
        let st = self.semitones();
        if st == 0.0 {
            1.0
        } else {
            crate::dsp::fast_exp2(st / 12.0)
        }
    }

    /// GLIDE TIME, seconds.
    pub fn glide_secs(&self) -> f32 {
        crate::dsp::modulator::law::GLIDE_TIME.at(self.glide_time)
    }

    /// A steal's glide, as its one-pole's time constant, seconds: GLIDE
    /// TIME is 95 % of the way, three of them. `None` at CUT.
    pub fn steal_glide(&self) -> Option<f32> {
        (self.steal == Steal::Glide).then(|| self.glide_secs() / 3.0)
    }
}

/// PITCH and FINE read by the engine every block; STEAL and GLIDE TIME at
/// a steal.
pub static PITCH_SPECS: [ParamSpec; 4] = [
    ParamSpec::stepped(0, "PITCH", ValFmt::Signed(24), -24.0, 24.0, 0.0, true)
        .ident("PITCH")
        .semitones(24.0),
    ParamSpec::stepped(1, "FINE", ValFmt::Signed(100), -100.0, 100.0, 0.0, true)
        .ident("FINE")
        .cents(100.0),
    ParamSpec::choice(2, "STEAL", ValFmt::Names(&STEAL_NAMES), 1.0, 0.0).ident("STEAL"),
    ParamSpec::continuous(
        3,
        "TIME",
        ValFmt::Law(crate::dsp::modulator::law::Law::GlideTime),
        0.0,
        1.0,
        INIT_GLIDE_TIME,
        1.0 / 128.0,
        false,
    )
    .ident("GLIDE_TIME"),
];

impl Block for PitchParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &PITCH_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::PITCH => self.pitch,
            Self::FINE => self.fine,
            Self::STEAL => self.steal as u8 as f32,
            Self::GLIDE_TIME => self.glide_time,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::PITCH => self.pitch = v,
            Self::FINE => self.fine = v,
            Self::STEAL => self.steal = if v >= 0.5 { Steal::Glide } else { Steal::Cut },
            Self::GLIDE_TIME => self.glide_time = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::STEAL).then(|| self.steal.disk_code())
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        (id == Self::STEAL).then(|| self.steal.disk_ident())
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::STEAL && apply_code(Steal::from_disk_code(code), |s| self.steal = s)
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
    pub pitch: PitchParams,
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
            BlockRef::Pitch => &self.pitch,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::Theme
            | BlockRef::Channels => return None,
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
            BlockRef::Pitch => &mut self.pitch,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp
            | BlockRef::Part
            | BlockRef::Theme
            | BlockRef::Channels => return None,
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
            pitch: PitchParams::default(),
        }
    }
}
