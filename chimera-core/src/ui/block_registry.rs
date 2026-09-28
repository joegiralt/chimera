use crate::addr::{BlockRef, Op};
use crate::block::ParamId;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::comp::CompParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::reverb::ReverbParams;
use crate::dsp::tape::TapeParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FolderParams, OutParams};
use crate::part::PartParams;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, FxFlow, FxNode, ParamSlot, VizType};
use crate::ui::page::{PageLayout, ValFmt};
use crate::ui::theme_settings::ThemeSettings;

const EMPTY: ParamSlot = ParamSlot::EMPTY;

// ---------------------------------------------------------------------------
// Modal engine pages
// ---------------------------------------------------------------------------

pub static MODAL_1: BlockDef = BlockDef {
    id: 2,
    name: "Modal",
    short: "MDL",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Modal, ModalParams::MODE),
        ParamSlot::param(BlockRef::Modal, ModalParams::EXCITE),
        ParamSlot::param(BlockRef::Modal, ModalParams::DECAY),
        ParamSlot::param(BlockRef::Modal, ModalParams::BRIGHTNESS),
        ParamSlot::param(BlockRef::Modal, ModalParams::POSITION),
        ParamSlot::param(BlockRef::Modal, ModalParams::INHARM),
    ],
};

pub static MODAL_2: BlockDef = BlockDef {
    id: 3,
    name: "Modal-2",
    short: "MDL2",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_BODY),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_STIFFNESS),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_FEEDBACK),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_ENS_DEPTH),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_ENS_RATE),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_ENS_MIX),
    ],
};

// ---------------------------------------------------------------------------
// Drive / Folder
// ---------------------------------------------------------------------------

pub static DRIVE: BlockDef = BlockDef {
    id: 8,
    name: "Drive",
    short: "DRV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Drive, DriveParams::DRIVE),
        ParamSlot::param(BlockRef::Drive, DriveParams::TONE),
        ParamSlot::param(BlockRef::Drive, DriveParams::MIX),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// FLD / VCA (spec § 5): the fold, then the VCA; last before MOD on every
/// Part chain.
pub static FOLDER: BlockDef = BlockDef {
    id: 9,
    name: "Fold / VCA",
    short: "AMP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Folder, FolderParams::FOLD),
        ParamSlot::param(BlockRef::Folder, FolderParams::SYMMETRY),
        ParamSlot::param(BlockRef::Folder, FolderParams::MIX),
        ParamSlot::param(BlockRef::Out, OutParams::VCA_VEL),
        EMPTY,
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

/// FLT: KIND, then the kind's panel (spec § 6).
pub static FILTER: BlockDef = BlockDef {
    id: 10,
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::KIND),
        ParamSlot::filter_panel(0),
        ParamSlot::filter_panel(1),
        ParamSlot::filter_panel(2),
        ParamSlot::filter_panel(3),
        ParamSlot::filter_panel(4),
    ],
};

/// FLT › MODE: MODE and the kind's extras (spec § UI). `short` is "MDE", not
/// "MODE": on the Algo map the full word overlaps the next node's label
/// (`MOD` is already the MOD node's); this abbreviation is only
/// the map's branch label, not the MODE param's own spec name.
pub static FILTER_MODE: BlockDef = BlockDef {
    id: 59,
    name: "Filter Mode",
    short: "MDE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::MODE),
        ParamSlot::filter_panel(5),
        ParamSlot::filter_panel(6),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

static FILTER_SUB_PAGES: [&BlockDef; 1] = [&FILTER_MODE];

// ---------------------------------------------------------------------------
// Modulators — envelopes, LFOs, etc.
// ---------------------------------------------------------------------------

const fn env_page(id: u16, name: &'static str, short: &'static str, s: EnvSlot) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::BigViz,
        viz: VizType::Adsr,
        params: [
            ParamSlot::env_panel(s, 0),
            ParamSlot::env_panel(s, 1),
            ParamSlot::env_panel(s, 2),
            ParamSlot::env_panel(s, 3),
            ParamSlot::env_panel(s, 4),
            ParamSlot::env_panel(s, 5),
        ],
    }
}

/// E1: first of the MOD node's sub-list (id 11, once the amp envelope's page).
pub static ENVELOPE: BlockDef = env_page(11, "Env 1", "E1", EnvSlot::Env1);
pub static ENV_2: BlockDef = env_page(60, "Env 2", "E2", EnvSlot::Env2);
pub static ENV_3: BlockDef = env_page(61, "Env 3", "E3", EnvSlot::Env3);

const fn lfo_page(id: u16, name: &'static str, short: &'static str, s: LfoSlot) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: VizType::None,
        params: [
            ParamSlot::lfo_panel(s, 0),
            ParamSlot::lfo_panel(s, 1),
            ParamSlot::lfo_panel(s, 2),
            ParamSlot::lfo_panel(s, 3),
            ParamSlot::lfo_panel(s, 4),
            ParamSlot::lfo_panel(s, 5),
        ],
    }
}

/// L1 (id 12, once the one LFO's page).
pub static LFO: BlockDef = lfo_page(12, "LFO 1", "L1", LfoSlot::Lfo1);
pub static LFO_2: BlockDef = lfo_page(64, "LFO 2", "L2", LfoSlot::Lfo2);
pub static LFO_3: BlockDef = lfo_page(65, "LFO 3", "L3", LfoSlot::Lfo3);

/// SPD: type A's SPEED and HOLD POSITION for the three ENV slots (spec § UI).
pub static ENV_SPEED: BlockDef = BlockDef {
    id: 62,
    name: "Env Speed",
    short: "SPD",
    // BigViz, as E1–E3: the three pill columns need the tall plot.
    layout: PageLayout::BigViz,
    viz: VizType::EnvSpeed,
    // A column per slot, under its pills: SPEED on top, HOLD below.
    params: [
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::SPEED).with_label("E1 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env2), EnvParams::SPEED).with_label("E2 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env3), EnvParams::SPEED).with_label("E3 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::HOLD_POS).with_label("E1 HOLD"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env2), EnvParams::HOLD_POS).with_label("E2 HOLD"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env3), EnvParams::HOLD_POS).with_label("E3 HOLD"),
    ],
};

// ---------------------------------------------------------------------------
// FX / Mix chain
// ---------------------------------------------------------------------------

pub static EFX: BlockDef = BlockDef {
    id: 16,
    name: "Reverb",
    short: "REV",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Reverb)),
    params: [
        ParamSlot::param(BlockRef::Reverb, ReverbParams::GRIT),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::TIME),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::DAMPING),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::SIZE),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::MIX),
        EMPTY,
    ],
};

pub static CHORUS: BlockDef = BlockDef {
    id: 18,
    name: "Chorus",
    short: "CHR",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Chorus)),
    params: [
        ParamSlot::param(BlockRef::Chorus, ChorusParams::MODE),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::RATE),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::DEPTH),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::MIX),
        EMPTY,
        EMPTY,
    ],
};

pub static DELAY: BlockDef = BlockDef {
    id: 19,
    name: "Delay",
    short: "DLY",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Delay)),
    params: [
        ParamSlot::param(BlockRef::Delay, DelayParams::TIME_MS),
        ParamSlot::param(BlockRef::Delay, DelayParams::FEEDBACK),
        ParamSlot::param(BlockRef::Delay, DelayParams::TONE),
        ParamSlot::param(BlockRef::Delay, DelayParams::REV_SEND),
        ParamSlot::param(BlockRef::Delay, DelayParams::MIX),
        EMPTY,
    ],
};

/// DLY › CHAR: the tape character, off the delay's main page (FX diet spec
/// § UI; an assumed default, pending the owner's word).
pub static DELAY_CHAR: BlockDef = BlockDef {
    id: 56,
    name: "Delay Char",
    short: "CHAR",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Delay)),
    params: [
        ParamSlot::param(BlockRef::Delay, DelayParams::WOW_FLUTTER),
        ParamSlot::param(BlockRef::Delay, DelayParams::SATURATION),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

static DELAY_SUB_PAGES: [&BlockDef; 1] = [&DELAY_CHAR];

/// Tape on DAC pair 1 (FX diet spec § Tape).
pub static TAPE: BlockDef = BlockDef {
    id: 57,
    name: "Tape",
    short: "TAPE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Tape, TapeParams::DRIVE),
        ParamSlot::param(BlockRef::Tape, TapeParams::TONE),
        ParamSlot::param(BlockRef::Tape, TapeParams::WOW),
        ParamSlot::param(BlockRef::Tape, TapeParams::MIX),
        EMPTY,
        EMPTY,
    ],
};

pub static MASTER: BlockDef = BlockDef {
    id: 20,
    name: "Master",
    short: "MST",
    layout: PageLayout::BigViz,
    viz: VizType::CompressorCurve,
    params: [
        ParamSlot::param(BlockRef::Comp, CompParams::THRESH),
        ParamSlot::param(BlockRef::Comp, CompParams::RATIO),
        ParamSlot::param(BlockRef::Comp, CompParams::ATTACK),
        ParamSlot::param(BlockRef::Comp, CompParams::RELEASE),
        ParamSlot::param(BlockRef::Comp, CompParams::MAKEUP),
        ParamSlot::param(BlockRef::Comp, CompParams::MIX),
    ],
};

/// MST › LEVEL: the legacy VOL and PAN, off the compressor's page (FX diet
/// spec § UI; an assumed default, pending the owner's word).
pub static MASTER_LEVEL: BlockDef = BlockDef {
    id: 58,
    name: "Level",
    short: "LVL",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("VOL", ValFmt::Uni),
        ParamSlot::legacy("PAN", ValFmt::Bi),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

static MASTER_SUB_PAGES: [&BlockDef; 1] = [&MASTER_LEVEL];

// ---------------------------------------------------------------------------
// Mod Matrix (new placeholder)
// ---------------------------------------------------------------------------

pub static MOD_MATRIX: BlockDef = BlockDef {
    id: 22,
    name: "Mod Matrix",
    short: "MTX",
    layout: PageLayout::Matrix,
    viz: VizType::None,
    params: [EMPTY; 6],
};

// ---------------------------------------------------------------------------
// Algo engine: group pages, one parameter across operators 1–6 (spec § UI)
// ---------------------------------------------------------------------------

const fn op_row(id: ParamId) -> [ParamSlot; 6] {
    [
        ParamSlot::param(BlockRef::AlgoOp(Op::A), id).with_label("OP1"),
        ParamSlot::param(BlockRef::AlgoOp(Op::B), id).with_label("OP2"),
        ParamSlot::param(BlockRef::AlgoOp(Op::C), id).with_label("OP3"),
        ParamSlot::param(BlockRef::AlgoOp(Op::D), id).with_label("OP4"),
        ParamSlot::param(BlockRef::AlgoOp(Op::E), id).with_label("OP5"),
        ParamSlot::param(BlockRef::AlgoOp(Op::F), id).with_label("OP6"),
    ]
}

const fn group(id: u16, name: &'static str, short: &'static str, param: ParamId) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: VizType::None,
        params: op_row(param),
    }
}

/// The OSC node's home page; its short name labels the node on the map.
pub static ALGO_WAVE: BlockDef = group(42, "Wave", "OSC", AlgoOpParams::WAVE);
pub static ALGO_LEVEL: BlockDef = group(44, "Level", "LVL", AlgoOpParams::LEVEL);
pub static ALGO_COARSE: BlockDef = group(45, "Coarse", "CRS", AlgoOpParams::COARSE);
pub static ALGO_FINE: BlockDef = group(46, "Fine", "FIN", AlgoOpParams::FINE);
pub static ALGO_DETUNE: BlockDef = group(47, "Detune", "DET", AlgoOpParams::DETUNE);
pub static ALGO_VELOCITY: BlockDef = group(48, "Velocity", "VEL", AlgoOpParams::VELOCITY);
pub static ALGO_AR: BlockDef = group(49, "Env AR", "AR", AlgoOpParams::AR);
pub static ALGO_D1R: BlockDef = group(50, "Env D1R", "D1R", AlgoOpParams::D1R);
pub static ALGO_D1L: BlockDef = group(51, "Env D1L", "D1L", AlgoOpParams::D1L);
pub static ALGO_D2R: BlockDef = group(52, "Env D2R", "D2R", AlgoOpParams::D2R);
pub static ALGO_RR: BlockDef = group(53, "Env RR", "RR", AlgoOpParams::RR);
pub static ALGO_RATE_SCALE: BlockDef = group(54, "Rate Scale", "RS", AlgoOpParams::RATE_SCALE);
pub static ALGO_FEEDBACK: BlockDef = group(55, "Feedback", "FBK", AlgoOpParams::FEEDBACK);

pub static ALGO_ALG: BlockDef = BlockDef {
    id: 43,
    name: "Algorithm",
    short: "ALG",
    layout: PageLayout::CellGrid,
    viz: VizType::AlgoDiagram,
    params: [
        ParamSlot::param(BlockRef::Algo, AlgoParams::ALG_A),
        ParamSlot::param(BlockRef::Algo, AlgoParams::ALG_B),
        ParamSlot::param(BlockRef::Algo, AlgoParams::MORPH),
        ParamSlot::param(BlockRef::Algo, AlgoParams::TRANSPOSE),
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME).with_label("VOL"),
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Chain templates
// ---------------------------------------------------------------------------

/// Mod sources every Part voice produces, in `ModSource` order (spec § 2).
pub static PART_MOD_SOURCES: [&str; crate::modulation::MAX_MOD_SOURCES] = [
    "ENV1", "LFO1", "ENV2", "ENV3", "LFO2", "LFO3", "VELO", "NOTE",
];

/// The MOD node's sub-list after its home MTX (spec § UI).
static MOD_SUB_PAGES: [&BlockDef; 7] =
    [&ENVELOPE, &ENV_2, &ENV_3, &ENV_SPEED, &LFO, &LFO_2, &LFO_3];

static MODAL_SUB_PAGES: [&BlockDef; 1] = [&MODAL_2];

static MODAL_PLUCK_BLOCKS: [ChainBlock; 4] = [
    ChainBlock::with_subs(&MODAL_1, &MODAL_SUB_PAGES),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::page(&FOLDER),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

pub static MODAL_PLUCK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Modal Pluck",
    blocks: &MODAL_PLUCK_BLOCKS,
    mod_sources: &PART_MOD_SOURCES,
};

/// WAVE is the OSC node's home; FINE's DETUNE and the five ENV stages sit
/// right after their group (sub-pages are one level deep).
static ALGO_OSC_SUB_PAGES: [&BlockDef; 12] = [
    &ALGO_COARSE,
    &ALGO_FINE,
    &ALGO_DETUNE,
    &ALGO_LEVEL,
    &ALGO_VELOCITY,
    &ALGO_AR,
    &ALGO_D1R,
    &ALGO_D1L,
    &ALGO_D2R,
    &ALGO_RR,
    &ALGO_RATE_SCALE,
    &ALGO_FEEDBACK,
];

/// ALGO is the engine's home: first on the map, where entering the chain lands.
static ALGO_BLOCKS: [ChainBlock; 6] = [
    ChainBlock::page(&ALGO_ALG),
    ChainBlock::with_subs(&ALGO_WAVE, &ALGO_OSC_SUB_PAGES),
    ChainBlock::page(&DRIVE),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::page(&FOLDER),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

pub static ALGO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Algo",
    blocks: &ALGO_BLOCKS,
    mod_sources: &PART_MOD_SOURCES,
};

// ---------------------------------------------------------------------------
// Mixer channel strip
// ---------------------------------------------------------------------------

/// A Part's MIDI channel, mode, output, level and pan (spec § UI).
pub static PART: BlockDef = BlockDef {
    id: 27,
    name: "Part",
    short: "PRT",
    layout: PageLayout::CellGrid,
    viz: VizType::MixerLevels,
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::CHANNEL),
        ParamSlot::param(BlockRef::Part, PartParams::MODE),
        ParamSlot::param(BlockRef::Part, PartParams::OUTPUT),
        ParamSlot::param(BlockRef::Part, PartParams::LEVEL),
        ParamSlot::param(BlockRef::Part, PartParams::PAN),
        EMPTY,
    ],
};

pub static SENDS: BlockDef = BlockDef {
    id: 30,
    name: "Sends",
    short: "SND",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Sends),
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::SEND_CHORUS),
        ParamSlot::param(BlockRef::Part, PartParams::SEND_DELAY),
        ParamSlot::param(BlockRef::Part, PartParams::SEND_REVERB),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// MIX + B<n>: Part n's mix settings, then the shared FX (spec § UI).
static MIXER_CHANNEL_BLOCKS: [ChainBlock; 7] = [
    ChainBlock::page(&PART),
    ChainBlock::page(&SENDS),
    ChainBlock::page(&CHORUS),
    ChainBlock::with_subs(&DELAY, &DELAY_SUB_PAGES),
    ChainBlock::page(&EFX),
    ChainBlock::page(&TAPE),
    ChainBlock::with_subs(&MASTER, &MASTER_SUB_PAGES),
];

pub static MIXER_CHANNEL_CHAIN: ChainDef2 = ChainDef2 {
    name: "Mixer",
    blocks: &MIXER_CHANNEL_BLOCKS,
    mod_sources: &[],
};

// ---------------------------------------------------------------------------
// System chain
// ---------------------------------------------------------------------------

pub static SYS_MIDI: BlockDef = BlockDef {
    id: 31,
    name: "MIDI Setup",
    short: "MID",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Channels, ParamId(0)),
        ParamSlot::param(BlockRef::Channels, ParamId(1)),
        ParamSlot::param(BlockRef::Channels, ParamId(2)),
        ParamSlot::param(BlockRef::Channels, ParamId(3)),
        ParamSlot::param(BlockRef::Channels, ParamId(4)),
        ParamSlot::param(BlockRef::Channels, ParamId(5)),
    ],
};

pub static SYS_TUNING: BlockDef = BlockDef {
    id: 32,
    name: "Tuning",
    short: "TUN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("TUNE", ValFmt::Bi),
        ParamSlot::legacy("SCALE", ValFmt::Int(2)),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static SYS_THEME: BlockDef = BlockDef {
    id: 33,
    name: "Theme",
    short: "THM",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    // Held by `UiState`, not a Sound (`ui::theme_settings`).
    params: [
        ParamSlot::param(BlockRef::Theme, ThemeSettings::BRIGHT),
        ParamSlot::param(BlockRef::Theme, ThemeSettings::GAMMA),
        ParamSlot::param(BlockRef::Theme, ThemeSettings::ACCENT),
        ParamSlot::param(BlockRef::Theme, ThemeSettings::BLACK),
        EMPTY,
        EMPTY,
    ],
};

pub static SYS_UPDATES: BlockDef = BlockDef {
    id: 34,
    name: "Updates",
    short: "UPD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY],
};

pub static SYS_ABOUT: BlockDef = BlockDef {
    id: 35,
    name: "About",
    short: "ABT",
    layout: PageLayout::BigViz,
    viz: VizType::Logo,
    params: [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY],
};

pub static SYS_AUDIO: BlockDef = BlockDef {
    id: 41,
    name: "Audio",
    short: "AUD",
    layout: PageLayout::CellGrid,
    viz: VizType::AudioStats,
    params: [
        ParamSlot::legacy("LOAD", ValFmt::Int(0)),
        ParamSlot::legacy("PEAK", ValFmt::Int(0)),
        ParamSlot::legacy("OVER", ValFmt::Int(0)),
        ParamSlot::legacy("DROPS", ValFmt::Int(0)),
        ParamSlot::legacy("DESYNC", ValFmt::Int(0)),
        ParamSlot::legacy("STACK", ValFmt::Int(0)),
    ],
};

static SYSTEM_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::page(&SYS_MIDI),
    ChainBlock::page(&SYS_TUNING),
    ChainBlock::page(&SYS_THEME),
    ChainBlock::page(&SYS_UPDATES),
    ChainBlock::with_subs(&SYS_ABOUT, &[&SYS_AUDIO]),
];

pub static SYSTEM_CHAIN: ChainDef2 = ChainDef2 {
    name: "System",
    blocks: &SYSTEM_BLOCKS,
    mod_sources: &[],
};

// ---------------------------------------------------------------------------
// Demo chain (UI component storyboard)
// ---------------------------------------------------------------------------

pub static DEMO_WAVES: BlockDef = BlockDef {
    id: 36,
    name: "Waves",
    short: "WAV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("CLIP", ValFmt::Uni),
        ParamSlot::legacy("WAVE", ValFmt::Uni),
        ParamSlot::legacy("PW", ValFmt::Uni),
        ParamSlot::legacy("FOLD", ValFmt::Uni),
        ParamSlot::legacy("TILT", ValFmt::Bi),
        ParamSlot::legacy("SYM", ValFmt::Bi),
    ],
};

pub static DEMO_SHAPES: BlockDef = BlockDef {
    id: 37,
    name: "Shapes",
    short: "SHP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("ARC", ValFmt::Uni),
        ParamSlot::legacy("LEVEL", ValFmt::Uni),
        ParamSlot::legacy("PAN", ValFmt::Bi),
        ParamSlot::legacy("D/W", ValFmt::Bi),
        ParamSlot::legacy("CUBE", ValFmt::Uni),
        ParamSlot::legacy("STACK", ValFmt::Uni),
    ],
};

pub static DEMO_MOTION: BlockDef = BlockDef {
    id: 38,
    name: "Motion",
    short: "MOT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("RIPPL", ValFmt::Uni),
        ParamSlot::legacy("BURST", ValFmt::Uni),
        ParamSlot::legacy("ORBIT", ValFmt::Uni),
        ParamSlot::legacy("SCATR", ValFmt::Uni),
        ParamSlot::legacy("BOUNC", ValFmt::Bi),
        ParamSlot::legacy("PULSE", ValFmt::Uni),
    ],
};

pub static DEMO_MATRIX: BlockDef = BlockDef {
    id: 39,
    name: "Matrix",
    short: "MTX",
    layout: PageLayout::Matrix,
    viz: VizType::None,
    params: [EMPTY; 6],
};

pub static DEMO_FM: BlockDef = BlockDef {
    id: 40,
    name: "FM Icons",
    short: "FM",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("ALGO", ValFmt::Int(7)),
        ParamSlot::legacy("LOOP", ValFmt::Uni),
        ParamSlot::legacy("SPIRL", ValFmt::Uni),
        ParamSlot::legacy("WAVE", ValFmt::Uni),
        EMPTY,
        EMPTY,
    ],
};

static DEMO_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::page(&DEMO_WAVES),
    ChainBlock::page(&DEMO_SHAPES),
    ChainBlock::page(&DEMO_MOTION),
    ChainBlock::page(&DEMO_FM),
    ChainBlock::page(&DEMO_MATRIX),
];

pub static DEMO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Demo",
    blocks: &DEMO_BLOCKS,
    mod_sources: &[],
};

/// Every chain, for whole-registry checks (unique ids, the focus table).
pub static ALL_CHAINS: [&ChainDef2; 5] = [
    &ALGO_CHAIN,
    &MODAL_PLUCK_CHAIN,
    &MIXER_CHANNEL_CHAIN,
    &SYSTEM_CHAIN,
    &DEMO_CHAIN,
];
