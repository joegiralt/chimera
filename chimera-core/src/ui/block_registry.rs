use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::page::{CellIcon, PageLayout, ValFmt};

const EMPTY: ParamSlot = ParamSlot {
    label: "--",
    format: ValFmt::Uni,
    icon: CellIcon::None,
};

// ---------------------------------------------------------------------------
// Pizza engine
// ---------------------------------------------------------------------------

pub static PIZZA: BlockDef = BlockDef {
    name: "Pizza",
    short: "PIZ",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "SHAPE", format: ValFmt::Uni, icon: CellIcon::WaveShape },
        ParamSlot { label: "CRUSH", format: ValFmt::Uni, icon: CellIcon::WaveClip },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::LevelBar },
        EMPTY, EMPTY, EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Modal engine pages
// ---------------------------------------------------------------------------

pub static MODAL_1: BlockDef = BlockDef {
    name: "Modal",
    short: "MDL",
    layout: PageLayout::CellGrid,
    viz: VizType::ModalPeaks,
    params: [
        ParamSlot { label: "MODE",   format: ValFmt::Int(3), icon: CellIcon::Arc },
        ParamSlot { label: "EXCITE", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "DECAY",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "BRIGHT", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "POS",    format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "INHARM", format: ValFmt::Uni,    icon: CellIcon::Arc },
    ],
};

pub static MODAL_2: BlockDef = BlockDef {
    name: "Modal-2",
    short: "MDL2",
    layout: PageLayout::CellGrid,
    viz: VizType::ModalPeaks,
    params: [
        ParamSlot { label: "BODY",  format: ValFmt::Int(3), icon: CellIcon::Arc },
        ParamSlot { label: "STIFF", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "FDBK",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "E.DPT", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "E.RAT", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "E.MIX", format: ValFmt::Uni,    icon: CellIcon::Arc },
    ],
};

// ---------------------------------------------------------------------------
// VA engine
// ---------------------------------------------------------------------------

pub static VA: BlockDef = BlockDef {
    name: "VA Osc",
    short: "VA",
    layout: PageLayout::CellGrid,
    viz: VizType::WaveformPreview,
    params: [
        ParamSlot { label: "WAVE",   format: ValFmt::Uni, icon: CellIcon::WaveShape  },
        ParamSlot { label: "PW",     format: ValFmt::Uni, icon: CellIcon::PulseWidth },
        ParamSlot { label: "SYNC",   format: ValFmt::Uni, icon: CellIcon::Arc        },
        ParamSlot { label: "SUB",    format: ValFmt::Uni, icon: CellIcon::Arc        },
        ParamSlot { label: "DETUNE", format: ValFmt::Uni, icon: CellIcon::Arc        },
        ParamSlot { label: "MIX",    format: ValFmt::Bi,  icon: CellIcon::DryWet     },
    ],
};

// ---------------------------------------------------------------------------
// FM engine pages
// ---------------------------------------------------------------------------

pub static FM_ALG: BlockDef = BlockDef {
    name: "4opFM",
    short: "FM",
    layout: PageLayout::CellGrid,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "ALG",   format: ValFmt::Int(7), icon: CellIcon::Arc },
        EMPTY,
        ParamSlot { label: "LEVEL", format: ValFmt::Uni,    icon: CellIcon::LevelBar },
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static FM_OP: BlockDef = BlockDef {
    name: "Operator",
    short: "OP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "OP",     format: ValFmt::Int(3), icon: CellIcon::Arc },
        ParamSlot { label: "WAVE",   format: ValFmt::Int(7), icon: CellIcon::WaveShape },
        ParamSlot { label: "LEVEL",  format: ValFmt::Uni,    icon: CellIcon::LevelBar },
        ParamSlot { label: "FDBK",   format: ValFmt::Int(7), icon: CellIcon::Arc },
        ParamSlot { label: "DETUN",  format: ValFmt::Bi,     icon: CellIcon::Arc },
        ParamSlot { label: "V.SNS",  format: ValFmt::Int(7), icon: CellIcon::Arc },
    ],
};

pub static FM_RATIO: BlockDef = BlockDef {
    name: "Ratios",
    short: "RAT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "OP1",  format: ValFmt::Int(63), icon: CellIcon::Arc },
        ParamSlot { label: "OP2",  format: ValFmt::Int(63), icon: CellIcon::Arc },
        ParamSlot { label: "OP3",  format: ValFmt::Int(63), icon: CellIcon::Arc },
        ParamSlot { label: "OP4",  format: ValFmt::Int(63), icon: CellIcon::Arc },
        ParamSlot { label: "FINE", format: ValFmt::Int(15), icon: CellIcon::Arc },
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Drive / Folder
// ---------------------------------------------------------------------------

pub static DRIVE: BlockDef = BlockDef {
    name: "Drive",
    short: "DRV",
    layout: PageLayout::CellGrid,
    viz: VizType::DriveClip,
    params: [
        ParamSlot { label: "DRIVE", format: ValFmt::Uni, icon: CellIcon::WaveClip },
        ParamSlot { label: "TONE",  format: ValFmt::Bi,  icon: CellIcon::ToneTilt },
        ParamSlot { label: "MIX",   format: ValFmt::Bi,  icon: CellIcon::DryWet   },
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static FOLDER: BlockDef = BlockDef {
    name: "Folder",
    short: "FLD",
    layout: PageLayout::CellGrid,
    viz: VizType::WaveFold,
    params: [
        ParamSlot { label: "FOLD", format: ValFmt::Uni, icon: CellIcon::WaveFold },
        ParamSlot { label: "SYM",  format: ValFmt::Bi,  icon: CellIcon::Symmetry },
        ParamSlot { label: "MIX",  format: ValFmt::Bi,  icon: CellIcon::DryWet   },
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

pub static FILTER: BlockDef = BlockDef {
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        ParamSlot { label: "CUTOFF", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "RESO",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DRIVE",  format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "FM",     format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "ENV",    format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "TRACK",  format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

// ---------------------------------------------------------------------------
// Modulators — envelopes, LFOs, etc.
// ---------------------------------------------------------------------------

/// ADSR Envelope modulator — the template for all envelope modulators.
/// Not an audio block — it's a modulation source that appears in the mod matrix Y-axis.
pub static ENVELOPE: BlockDef = BlockDef {
    name: "Envelope",
    short: "ENV",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "ATK",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DEC",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "SUS",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "REL",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DEPTH", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "VEL",   format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

/// LFO modulator — cyclical modulation source.
pub static LFO: BlockDef = BlockDef {
    name: "LFO",
    short: "LFO",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "RATE",  format: ValFmt::Uni,    icon: CellIcon::Orbit },
        ParamSlot { label: "SHAPE", format: ValFmt::Int(4), icon: CellIcon::WaveShape },
        ParamSlot { label: "SYNC",  format: ValFmt::Int(1), icon: CellIcon::Arc },
        ParamSlot { label: "PHASE", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "DEPTH", format: ValFmt::Uni,    icon: CellIcon::Breathe },
        ParamSlot { label: "OFST",  format: ValFmt::Bi,     icon: CellIcon::Arc },
    ],
};

pub static ENV_AMP: BlockDef = BlockDef {
    name: "Env Amp",
    short: "ENV",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "ATK",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DEC",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "SUS",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "REL",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "VEL",   format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

pub static ENV_FILTER: BlockDef = BlockDef {
    name: "Env Filter",
    short: "E.F",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "ATK",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DEC",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "SUS",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "REL",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "VEL",   format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

pub static ENV_AUX: BlockDef = BlockDef {
    name: "Env Aux",
    short: "E.X",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "ATK",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DEC",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "SUS",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "REL",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "VEL",   format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

// ---------------------------------------------------------------------------
// FX / Mix chain
// ---------------------------------------------------------------------------

pub static EFX: BlockDef = BlockDef {
    name: "Reverb",
    short: "EFX",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow,
    params: [
        ParamSlot { label: "TYPE", format: ValFmt::Int(2), icon: CellIcon::Arc },
        ParamSlot { label: "TIME", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "DAMP", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "SIZE", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "MIX",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        EMPTY,
    ],
};

pub static MIXER: BlockDef = BlockDef {
    name: "Mixer",
    short: "MIX",
    layout: PageLayout::CellGrid,
    viz: VizType::MixerLevels,
    params: [
        ParamSlot { label: "VOL",    format: ValFmt::Uni, icon: CellIcon::LevelBar },
        ParamSlot { label: "PAN",    format: ValFmt::Bi,  icon: CellIcon::PanDot   },
        ParamSlot { label: "VOICES", format: ValFmt::Uni, icon: CellIcon::Arc      },
        ParamSlot { label: "MIDI",   format: ValFmt::Uni, icon: CellIcon::Arc      },
        ParamSlot { label: "PITCH",  format: ValFmt::Bi,  icon: CellIcon::Arc      },
        ParamSlot { label: "GLIDE",  format: ValFmt::Uni, icon: CellIcon::Arc      },
    ],
};

pub static CHORUS: BlockDef = BlockDef {
    name: "Chorus",
    short: "CHR",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow,
    params: [
        ParamSlot { label: "MODE",  format: ValFmt::Int(3), icon: CellIcon::Arc },
        ParamSlot { label: "RATE",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "DEPTH", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "MIX",   format: ValFmt::Uni,    icon: CellIcon::Arc },
        EMPTY,
        EMPTY,
    ],
};

pub static DELAY: BlockDef = BlockDef {
    name: "Delay",
    short: "DLY",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow,
    params: [
        ParamSlot { label: "TIME", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "FDBK", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "WOW",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "SAT",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "TONE", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "MIX",  format: ValFmt::Uni, icon: CellIcon::Arc },
    ],
};

pub static MASTER: BlockDef = BlockDef {
    name: "Master",
    short: "MST",
    layout: PageLayout::BigViz,
    viz: VizType::CompressorCurve,
    params: [
        ParamSlot { label: "VOL", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "PAN", format: ValFmt::Bi,  icon: CellIcon::None },
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Noise (new — not in current PageId)
// ---------------------------------------------------------------------------

pub static NOISE: BlockDef = BlockDef {
    name: "Noise",
    short: "NSE",
    layout: PageLayout::CellGrid,
    viz: VizType::WaveformPreview,
    params: [
        ParamSlot { label: "COLOR", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "PITCH", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "DECAY", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "CLICK", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "TONE",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::Arc },
    ],
};

// ---------------------------------------------------------------------------
// Mod Matrix (new placeholder)
// ---------------------------------------------------------------------------

pub static MOD_MATRIX: BlockDef = BlockDef {
    name: "Mod Matrix",
    short: "MOD",
    layout: PageLayout::Matrix,
    viz: VizType::RoutingMatrix,
    params: [EMPTY; 6],
};

// ---------------------------------------------------------------------------
// TX81Z 5-stage envelopes (one per FM operator)
// ---------------------------------------------------------------------------

pub static FM_ENV1: BlockDef = BlockDef {
    name: "Op1 Env",
    short: "E1",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "AR",  format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1L", format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "D2R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "RR",  format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "RS",  format: ValFmt::Int(3),  icon: CellIcon::None },
    ],
};

pub static FM_ENV2: BlockDef = BlockDef {
    name: "Op2 Env",
    short: "E2",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "AR",  format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1L", format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "D2R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "RR",  format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "RS",  format: ValFmt::Int(3),  icon: CellIcon::None },
    ],
};

pub static FM_ENV3: BlockDef = BlockDef {
    name: "Op3 Env",
    short: "E3",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "AR",  format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1L", format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "D2R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "RR",  format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "RS",  format: ValFmt::Int(3),  icon: CellIcon::None },
    ],
};

pub static FM_ENV4: BlockDef = BlockDef {
    name: "Op4 Env",
    short: "E4",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "AR",  format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "D1L", format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "D2R", format: ValFmt::Int(31), icon: CellIcon::None },
        ParamSlot { label: "RR",  format: ValFmt::Int(15), icon: CellIcon::None },
        ParamSlot { label: "RS",  format: ValFmt::Int(3),  icon: CellIcon::None },
    ],
};

// ---------------------------------------------------------------------------
// Chain templates
// ---------------------------------------------------------------------------

static PIZZA_BLOCK: ChainBlock = ChainBlock { def: &PIZZA, sub_pages: &[] };

static MOD_MATRIX_SUB_PAGES: [&BlockDef; 2] = [&ENVELOPE, &LFO];

static PIZZA_POLY_BLOCKS: [ChainBlock; 5] = [
    PIZZA_BLOCK,
    ChainBlock { def: &DRIVE,      sub_pages: &[] },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &FOLDER,     sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &MOD_MATRIX_SUB_PAGES },
];

pub static PIZZA_POLY_CHAIN: ChainDef2 = ChainDef2 {
    name: "Pizza",
    blocks: &PIZZA_POLY_BLOCKS,
};

static KICK_BLOCKS: [ChainBlock; 3] = [
    ChainBlock { def: &NOISE,      sub_pages: &[] },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &MOD_MATRIX_SUB_PAGES },
];

pub static KICK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Kick",
    blocks: &KICK_BLOCKS,
};

static MODAL_SUB_PAGES: [&BlockDef; 1] = [&MODAL_2];

static MODAL_PLUCK_BLOCKS: [ChainBlock; 3] = [
    ChainBlock { def: &MODAL_1,    sub_pages: &MODAL_SUB_PAGES },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &MOD_MATRIX_SUB_PAGES },
];

pub static MODAL_PLUCK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Modal Pluck",
    blocks: &MODAL_PLUCK_BLOCKS,
};

static FM_SUB_PAGES: [&BlockDef; 2] = [&FM_OP, &FM_RATIO];

static FM_MOD_MATRIX_SUB_PAGES: [&BlockDef; 4] = [&FM_ENV1, &FM_ENV2, &FM_ENV3, &FM_ENV4];

static FM_BLOCKS: [ChainBlock; 5] = [
    ChainBlock { def: &FM_ALG,     sub_pages: &FM_SUB_PAGES },
    ChainBlock { def: &DRIVE,      sub_pages: &[] },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &FOLDER,     sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &FM_MOD_MATRIX_SUB_PAGES },
];

pub static FM_CHAIN: ChainDef2 = ChainDef2 {
    name: "FM",
    blocks: &FM_BLOCKS,
};

static MIX_BLOCKS: [ChainBlock; 5] = [
    ChainBlock { def: &MIXER,  sub_pages: &[] },
    ChainBlock { def: &CHORUS, sub_pages: &[] },
    ChainBlock { def: &DELAY,  sub_pages: &[] },
    ChainBlock { def: &EFX,    sub_pages: &[] },
    ChainBlock { def: &MASTER, sub_pages: &[] },
];

pub static MIX_CHAIN: ChainDef2 = ChainDef2 {
    name: "Mix",
    blocks: &MIX_BLOCKS,
};

static ENVELOPE_BLOCKS: [ChainBlock; 3] = [
    ChainBlock { def: &ENV_AMP,    sub_pages: &[] },
    ChainBlock { def: &ENV_FILTER, sub_pages: &[] },
    ChainBlock { def: &ENV_AUX,    sub_pages: &[] },
];

pub static ENVELOPE_CHAIN: ChainDef2 = ChainDef2 {
    name: "Envelopes",
    blocks: &ENVELOPE_BLOCKS,
};

// ---------------------------------------------------------------------------
// Mixer channel strip
// ---------------------------------------------------------------------------

pub static CHANNEL: BlockDef = BlockDef {
    name: "Channel",
    short: "CH",
    layout: PageLayout::CellGrid,
    viz: VizType::MixerLevels,
    params: [
        ParamSlot { label: "VOL",    format: ValFmt::Uni,     icon: CellIcon::LevelBar },
        ParamSlot { label: "PAN",    format: ValFmt::Bi,      icon: CellIcon::PanDot   },
        ParamSlot { label: "OUT",    format: ValFmt::Int(2),  icon: CellIcon::Arc      },
        ParamSlot { label: "VOICES", format: ValFmt::Int(5),  icon: CellIcon::Arc      },
        ParamSlot { label: "MODE",   format: ValFmt::Int(2),  icon: CellIcon::Arc      },
        ParamSlot { label: "GLIDE",  format: ValFmt::Uni,     icon: CellIcon::Arc      },
    ],
};

pub static MIDI_CFG: BlockDef = BlockDef {
    name: "MIDI",
    short: "MID",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "CH",     format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "PGM",    format: ValFmt::Int(1),  icon: CellIcon::Arc },
        ParamSlot { label: "CC.RX",  format: ValFmt::Int(1),  icon: CellIcon::Arc },
        ParamSlot { label: "BEND",   format: ValFmt::Int(12), icon: CellIcon::Arc },
        ParamSlot { label: "TRNS",   format: ValFmt::Bi,      icon: CellIcon::Arc },
        EMPTY,
    ],
};

pub static EQ: BlockDef = BlockDef {
    name: "EQ",
    short: "EQ",
    layout: PageLayout::BigViz,
    viz: VizType::EqResponse,
    params: [
        ParamSlot { label: "LOW",   format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "L.FRQ", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "MID",   format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "M.FRQ", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "HIGH",  format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "H.FRQ", format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

pub static SENDS: BlockDef = BlockDef {
    name: "Sends",
    short: "SND",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "REV", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "DLY", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "CHR", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "S4",  format: ValFmt::Uni, icon: CellIcon::Arc },
        EMPTY,
        EMPTY,
    ],
};

static MIXER_CHANNEL_BLOCKS: [ChainBlock; 4] = [
    ChainBlock { def: &CHANNEL,  sub_pages: &[] },
    ChainBlock { def: &MIDI_CFG, sub_pages: &[] },
    ChainBlock { def: &EQ,       sub_pages: &[] },
    ChainBlock { def: &SENDS,    sub_pages: &[] },
];

pub static MIXER_CHANNEL_CHAIN: ChainDef2 = ChainDef2 {
    name: "Mixer",
    blocks: &MIXER_CHANNEL_BLOCKS,
};

// ---------------------------------------------------------------------------
// System chain
// ---------------------------------------------------------------------------

pub static SYS_MIDI: BlockDef = BlockDef {
    name: "MIDI Setup",
    short: "MID",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "P1 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P2 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P3 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P4 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P5 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P6 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
    ],
};

pub static SYS_TUNING: BlockDef = BlockDef {
    name: "Tuning",
    short: "TUN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "TUNE",  format: ValFmt::Bi,     icon: CellIcon::Arc },
        ParamSlot { label: "SCALE", format: ValFmt::Int(2), icon: CellIcon::Arc },
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static SYS_THEME: BlockDef = BlockDef {
    name: "Theme",
    short: "THM",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "BRIGHT",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "ACCENT",  format: ValFmt::Int(4), icon: CellIcon::Arc },
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static SYS_UPDATES: BlockDef = BlockDef {
    name: "Updates",
    short: "UPD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY],
};

pub static SYS_ABOUT: BlockDef = BlockDef {
    name: "About",
    short: "ABT",
    layout: PageLayout::BigViz,
    viz: VizType::Logo,
    params: [EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY],
};

static SYSTEM_BLOCKS: [ChainBlock; 5] = [
    ChainBlock { def: &SYS_MIDI,    sub_pages: &[] },
    ChainBlock { def: &SYS_TUNING,  sub_pages: &[] },
    ChainBlock { def: &SYS_THEME,   sub_pages: &[] },
    ChainBlock { def: &SYS_UPDATES, sub_pages: &[] },
    ChainBlock { def: &SYS_ABOUT,   sub_pages: &[] },
];

pub static SYSTEM_CHAIN: ChainDef2 = ChainDef2 {
    name: "System",
    blocks: &SYSTEM_BLOCKS,
};

// ---------------------------------------------------------------------------
// Demo chain (UI component storyboard)
// ---------------------------------------------------------------------------

pub static DEMO_WAVES: BlockDef = BlockDef {
    name: "Waves",
    short: "WAV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "CLIP", format: ValFmt::Uni, icon: CellIcon::WaveClip   },
        ParamSlot { label: "WAVE", format: ValFmt::Uni, icon: CellIcon::WaveShape  },
        ParamSlot { label: "PW",   format: ValFmt::Uni, icon: CellIcon::PulseWidth },
        ParamSlot { label: "FOLD", format: ValFmt::Uni, icon: CellIcon::WaveFold   },
        ParamSlot { label: "TILT", format: ValFmt::Bi,  icon: CellIcon::ToneTilt   },
        ParamSlot { label: "SYM",  format: ValFmt::Bi,  icon: CellIcon::Symmetry   },
    ],
};

pub static DEMO_SHAPES: BlockDef = BlockDef {
    name: "Shapes",
    short: "SHP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "ARC",   format: ValFmt::Uni, icon: CellIcon::Arc      },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::LevelBar },
        ParamSlot { label: "PAN",   format: ValFmt::Bi,  icon: CellIcon::PanDot   },
        ParamSlot { label: "D/W",   format: ValFmt::Bi,  icon: CellIcon::DryWet   },
        ParamSlot { label: "CUBE",  format: ValFmt::Uni, icon: CellIcon::Cube     },
        ParamSlot { label: "STACK", format: ValFmt::Uni, icon: CellIcon::Stack    },
    ],
};

pub static DEMO_MOTION: BlockDef = BlockDef {
    name: "Motion",
    short: "MOT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "RIPPL", format: ValFmt::Uni, icon: CellIcon::Ripple  },
        ParamSlot { label: "BURST", format: ValFmt::Uni, icon: CellIcon::Burst   },
        ParamSlot { label: "ORBIT", format: ValFmt::Uni, icon: CellIcon::Orbit   },
        ParamSlot { label: "SCATR", format: ValFmt::Uni, icon: CellIcon::Scatter },
        ParamSlot { label: "BOUNC", format: ValFmt::Bi,  icon: CellIcon::Bounce  },
        ParamSlot { label: "PULSE", format: ValFmt::Uni, icon: CellIcon::Breathe },
    ],
};

pub static DEMO_MATRIX: BlockDef = BlockDef {
    name: "Matrix",
    short: "MTX",
    layout: PageLayout::Matrix,
    viz: VizType::RoutingMatrix,
    params: [EMPTY; 6],
};

static DEMO_BLOCKS: [ChainBlock; 4] = [
    ChainBlock { def: &DEMO_WAVES,  sub_pages: &[] },
    ChainBlock { def: &DEMO_SHAPES, sub_pages: &[] },
    ChainBlock { def: &DEMO_MOTION, sub_pages: &[] },
    ChainBlock { def: &DEMO_MATRIX, sub_pages: &[] },
];

pub static DEMO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Demo",
    blocks: &DEMO_BLOCKS,
};
