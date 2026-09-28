use crate::addr::{BlockRef, Op};
use crate::block::ParamId;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::comp::CompParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::reverb::ReverbParams;
use crate::dsp::tape::TapeParams;
use crate::modulation::ModSource;
use crate::params::{DriveParams, EnvParams, FilterParams, FolderParams, OutParams};
use crate::part::PartParams;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
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

pub static FOLDER: BlockDef = BlockDef {
    id: 9,
    name: "Folder",
    short: "FLD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Folder, FolderParams::FOLD),
        ParamSlot::param(BlockRef::Folder, FolderParams::SYMMETRY),
        ParamSlot::param(BlockRef::Folder, FolderParams::MIX),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

/// SVF: — · CUTOFF · RES / MODE · ENV · KEY (KIND and the route knobs come
/// later in the filter-routing plan; spec § 6).
pub static FILTER: BlockDef = BlockDef {
    id: 10,
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        EMPTY,
        ParamSlot::param(BlockRef::Filter, FilterParams::CUTOFF),
        ParamSlot::param(BlockRef::Filter, FilterParams::RESONANCE),
        ParamSlot::param(BlockRef::Filter, FilterParams::MODE),
        ParamSlot::route(ModSource::Env1, "ENV"),
        ParamSlot::route(ModSource::Note, "KEY"),
    ],
};

/// FLT › MODE: MODE and the SVF's extras (spec § UI). `short` is "MDE", not
/// "MODE": on the Algo map that reaches FOLDER, the full word overlaps FLD's
/// label by 2px (`MOD` is already MOD_MATRIX's); this abbreviation is only
/// the map's branch label, not the MODE param's own spec name.
pub static FILTER_MODE: BlockDef = BlockDef {
    id: 59,
    name: "Filter Mode",
    short: "MDE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::MODE),
        ParamSlot::param(BlockRef::Filter, FilterParams::DRIVE),
        ParamSlot::route(ModSource::Lfo1, "LFO"),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

static FILTER_SUB_PAGES: [&BlockDef; 1] = [&FILTER_MODE];

// ---------------------------------------------------------------------------
// Modulators — envelopes, LFOs, etc.
// ---------------------------------------------------------------------------

/// ADSR Envelope modulator — the template for all envelope modulators.
/// Not an audio block — it's a modulation source that appears in the mod matrix Y-axis.
pub static ENVELOPE: BlockDef = BlockDef {
    id: 11,
    name: "Envelope",
    short: "ENV",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::ATTACK),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::DECAY),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::SUSTAIN),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::RELEASE),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::HOLD),
        EMPTY,
    ],
};

/// LFO modulator — cyclical modulation source.
pub static LFO: BlockDef = BlockDef {
    id: 12,
    name: "LFO",
    short: "LFO",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::RATE),
        ParamSlot::param(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::SHAPE),
        ParamSlot::param(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::SYNC),
        ParamSlot::param(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::PHASE),
        ParamSlot::param(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH),
        EMPTY,
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
    viz: VizType::EffectsFlow,
    params: [
        ParamSlot::param(BlockRef::Reverb, ReverbParams::GRIT),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::TIME),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::DAMPING),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::SIZE),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::MIX),
        EMPTY,
    ],
};

pub static MIXER: BlockDef = BlockDef {
    id: 17,
    name: "Mixer",
    short: "MIX",
    layout: PageLayout::CellGrid,
    viz: VizType::MixerLevels,
    params: [
        ParamSlot::legacy("VOL", ValFmt::Uni),
        ParamSlot::legacy("PAN", ValFmt::Bi),
        ParamSlot::legacy("VOICES", ValFmt::Uni),
        ParamSlot::legacy("MIDI", ValFmt::Uni),
        ParamSlot::legacy("PITCH", ValFmt::Bi),
        ParamSlot::legacy("GLIDE", ValFmt::Uni),
    ],
};

pub static CHORUS: BlockDef = BlockDef {
    id: 18,
    name: "Chorus",
    short: "CHR",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow,
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
    viz: VizType::EffectsFlow,
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
    viz: VizType::EffectsFlow,
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
// Noise (new — not in current PageId)
// ---------------------------------------------------------------------------

pub static NOISE: BlockDef = BlockDef {
    id: 21,
    name: "Noise",
    short: "NSE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("COLOR", ValFmt::Uni),
        ParamSlot::legacy("PITCH", ValFmt::Uni),
        ParamSlot::legacy("DECAY", ValFmt::Uni),
        ParamSlot::legacy("CLICK", ValFmt::Uni),
        ParamSlot::legacy("TONE", ValFmt::Uni),
        ParamSlot::legacy("LEVEL", ValFmt::Uni),
    ],
};

// ---------------------------------------------------------------------------
// Mod Matrix (new placeholder)
// ---------------------------------------------------------------------------

pub static MOD_MATRIX: BlockDef = BlockDef {
    id: 22,
    name: "Mod Matrix",
    short: "MOD",
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
pub static PART_MOD_SOURCES: [&str; crate::modulation::MAX_MOD_SOURCES] =
    ["E1", "LF1", "E2", "E3", "LF2", "LF3", "VEL", "NTE"];

static MOD_MATRIX_SUB_PAGES: [&BlockDef; 2] = [&ENVELOPE, &LFO];

static KICK_BLOCKS: [ChainBlock; 3] = [
    ChainBlock {
        def: &NOISE,
        sub_pages: &[],
    },
    ChainBlock {
        def: &FILTER,
        sub_pages: &FILTER_SUB_PAGES,
    },
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_MATRIX_SUB_PAGES,
    },
];

pub static KICK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Kick",
    blocks: &KICK_BLOCKS,
    mod_sources: &PART_MOD_SOURCES,
};

static MODAL_SUB_PAGES: [&BlockDef; 1] = [&MODAL_2];

static MODAL_PLUCK_BLOCKS: [ChainBlock; 3] = [
    ChainBlock {
        def: &MODAL_1,
        sub_pages: &MODAL_SUB_PAGES,
    },
    ChainBlock {
        def: &FILTER,
        sub_pages: &FILTER_SUB_PAGES,
    },
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_MATRIX_SUB_PAGES,
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
    ChainBlock {
        def: &ALGO_ALG,
        sub_pages: &[],
    },
    ChainBlock {
        def: &ALGO_WAVE,
        sub_pages: &ALGO_OSC_SUB_PAGES,
    },
    ChainBlock {
        def: &DRIVE,
        sub_pages: &[],
    },
    ChainBlock {
        def: &FILTER,
        sub_pages: &FILTER_SUB_PAGES,
    },
    ChainBlock {
        def: &FOLDER,
        sub_pages: &[],
    },
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_MATRIX_SUB_PAGES,
    },
];

pub static ALGO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Algo",
    blocks: &ALGO_BLOCKS,
    mod_sources: &PART_MOD_SOURCES,
};

static MIX_BLOCKS: [ChainBlock; 6] = [
    ChainBlock {
        def: &MIXER,
        sub_pages: &[],
    },
    ChainBlock {
        def: &CHORUS,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DELAY,
        sub_pages: &DELAY_SUB_PAGES,
    },
    ChainBlock {
        def: &EFX,
        sub_pages: &[],
    },
    ChainBlock {
        def: &TAPE,
        sub_pages: &[],
    },
    ChainBlock {
        def: &MASTER,
        sub_pages: &MASTER_SUB_PAGES,
    },
];

pub static MIX_CHAIN: ChainDef2 = ChainDef2 {
    name: "Mix",
    blocks: &MIX_BLOCKS,
    mod_sources: &[],
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

pub static MIDI_CFG: BlockDef = BlockDef {
    id: 28,
    name: "MIDI",
    short: "MID",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("CH", ValFmt::Int(16)),
        ParamSlot::legacy("PGM", ValFmt::Int(1)),
        ParamSlot::legacy("CC.RX", ValFmt::Int(1)),
        ParamSlot::legacy("BEND", ValFmt::Int(12)),
        ParamSlot::legacy("TRNS", ValFmt::Bi),
        EMPTY,
    ],
};

pub static EQ: BlockDef = BlockDef {
    id: 29,
    name: "EQ",
    short: "EQ",
    layout: PageLayout::BigViz,
    viz: VizType::None,
    params: [
        ParamSlot::legacy("LOW", ValFmt::Bi),
        ParamSlot::legacy("L.FRQ", ValFmt::Uni),
        ParamSlot::legacy("MID", ValFmt::Bi),
        ParamSlot::legacy("M.FRQ", ValFmt::Uni),
        ParamSlot::legacy("HIGH", ValFmt::Bi),
        ParamSlot::legacy("H.FRQ", ValFmt::Uni),
    ],
};

pub static SENDS: BlockDef = BlockDef {
    id: 30,
    name: "Sends",
    short: "SND",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow,
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
    ChainBlock {
        def: &PART,
        sub_pages: &[],
    },
    ChainBlock {
        def: &SENDS,
        sub_pages: &[],
    },
    ChainBlock {
        def: &CHORUS,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DELAY,
        sub_pages: &DELAY_SUB_PAGES,
    },
    ChainBlock {
        def: &EFX,
        sub_pages: &[],
    },
    ChainBlock {
        def: &TAPE,
        sub_pages: &[],
    },
    ChainBlock {
        def: &MASTER,
        sub_pages: &MASTER_SUB_PAGES,
    },
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
        // 1-based (CH 1..16), matching the Mixer PART page's CH formatter.
        ParamSlot::legacy("P1 CH", ValFmt::OneBased(15)),
        ParamSlot::legacy("P2 CH", ValFmt::OneBased(15)),
        ParamSlot::legacy("P3 CH", ValFmt::OneBased(15)),
        ParamSlot::legacy("P4 CH", ValFmt::OneBased(15)),
        ParamSlot::legacy("P5 CH", ValFmt::OneBased(15)),
        ParamSlot::legacy("P6 CH", ValFmt::OneBased(15)),
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
    ChainBlock {
        def: &SYS_MIDI,
        sub_pages: &[],
    },
    ChainBlock {
        def: &SYS_TUNING,
        sub_pages: &[],
    },
    ChainBlock {
        def: &SYS_THEME,
        sub_pages: &[],
    },
    ChainBlock {
        def: &SYS_UPDATES,
        sub_pages: &[],
    },
    ChainBlock {
        def: &SYS_ABOUT,
        sub_pages: &[&SYS_AUDIO],
    },
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
    ChainBlock {
        def: &DEMO_WAVES,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DEMO_SHAPES,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DEMO_MOTION,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DEMO_FM,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DEMO_MATRIX,
        sub_pages: &[],
    },
];

pub static DEMO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Demo",
    blocks: &DEMO_BLOCKS,
    mod_sources: &[],
};
