use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::page::{CellIcon, PageLayout, ValFmt};

const EMPTY: ParamSlot = ParamSlot {
    label: "--",
    format: ValFmt::Uni,
    icon: CellIcon::None,
};

// ---------------------------------------------------------------------------
// FM engine pages
// ---------------------------------------------------------------------------

pub static FM_A: BlockDef = BlockDef {
    name: "FM Osc",
    short: "FM-A",
    layout: PageLayout::BigViz,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "ALGO",  format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "FDBK",  format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "RAT C", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV C", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "LVL C", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "DTN C", format: ValFmt::Bi,     icon: CellIcon::None },
    ],
};

pub static FM_B: BlockDef = BlockDef {
    name: "FM-B",
    short: "FM-B",
    layout: PageLayout::BigViz,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "RAT M", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV M", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "LVL M", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "DTN M", format: ValFmt::Bi,     icon: CellIcon::None },
        ParamSlot { label: "RAT 2", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "LVL 2", format: ValFmt::Uni,    icon: CellIcon::None },
    ],
};

pub static FM_C: BlockDef = BlockDef {
    name: "FM-C",
    short: "FM-C",
    layout: PageLayout::BigViz,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "RAT 3", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV 3", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "LVL 3", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV 2", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV 4", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "DTN 4", format: ValFmt::Uni,    icon: CellIcon::None },
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
// VCA / Envelopes
// ---------------------------------------------------------------------------

pub static VCA: BlockDef = BlockDef {
    name: "VCA",
    short: "VCA",
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
    layout: PageLayout::CellGrid,
    viz: VizType::RoutingMatrix,
    params: [
        ParamSlot { label: "SRC1", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "DST1", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "AMT1", format: ValFmt::Bi,  icon: CellIcon::Arc },
        ParamSlot { label: "SRC2", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "DST2", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "AMT2", format: ValFmt::Bi,  icon: CellIcon::Arc },
    ],
};

// ---------------------------------------------------------------------------
// Chain templates
// ---------------------------------------------------------------------------

static FM_OSC_SUB_PAGES: [&BlockDef; 2] = [&FM_B, &FM_C];

static FM_POLY_BLOCKS: [ChainBlock; 6] = [
    ChainBlock { def: &FM_A,       sub_pages: &FM_OSC_SUB_PAGES },
    ChainBlock { def: &DRIVE,      sub_pages: &[] },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &FOLDER,     sub_pages: &[] },
    ChainBlock { def: &VCA,        sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &[] },
];

pub static FM_POLY_CHAIN: ChainDef2 = ChainDef2 {
    name: "FM Poly",
    blocks: &FM_POLY_BLOCKS,
};

static KICK_BLOCKS: [ChainBlock; 4] = [
    ChainBlock { def: &NOISE,      sub_pages: &[] },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &VCA,        sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &[] },
];

pub static KICK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Kick",
    blocks: &KICK_BLOCKS,
};

static MODAL_SUB_PAGES: [&BlockDef; 1] = [&MODAL_2];

static MODAL_PLUCK_BLOCKS: [ChainBlock; 4] = [
    ChainBlock { def: &MODAL_1,    sub_pages: &MODAL_SUB_PAGES },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &VCA,        sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &[] },
];

pub static MODAL_PLUCK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Modal Pluck",
    blocks: &MODAL_PLUCK_BLOCKS,
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
