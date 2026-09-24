use crate::addr::{BlockRef, Op};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::pizza::PizzaParams;
use crate::dsp::reverb::ReverbParams;
use crate::part::PartParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams};
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::page::{PageLayout, ValFmt};

const EMPTY: ParamSlot = ParamSlot::EMPTY;

// ---------------------------------------------------------------------------
// Pizza engine
// ---------------------------------------------------------------------------

pub static PIZZA: BlockDef = BlockDef {
    id: 1,
    name: "Pizza",
    short: "PIZ",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Pizza, PizzaParams::SHAPE),
        ParamSlot::param(BlockRef::Pizza, PizzaParams::CRUSH),
        ParamSlot::param(BlockRef::Pizza, PizzaParams::LEVEL),
        EMPTY, EMPTY, EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Modal engine pages
// ---------------------------------------------------------------------------

pub static MODAL_1: BlockDef = BlockDef {
    id: 2,
    name: "Modal",
    short: "MDL",
    layout: PageLayout::CellGrid,
    viz: VizType::ModalPeaks,
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
    viz: VizType::ModalPeaks,
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
// VA engine
// ---------------------------------------------------------------------------

pub static VA: BlockDef = BlockDef {
    id: 4,
    name: "VA Osc",
    short: "VA",
    layout: PageLayout::CellGrid,
    viz: VizType::WaveformPreview,
    params: [
        ParamSlot::legacy("WAVE", ValFmt::Uni),
        ParamSlot::legacy("PW", ValFmt::Uni),
        ParamSlot::legacy("SYNC", ValFmt::Uni),
        ParamSlot::legacy("SUB", ValFmt::Uni),
        ParamSlot::legacy("DETUNE", ValFmt::Uni),
        ParamSlot::legacy("MIX", ValFmt::Bi),
    ],
};

// ---------------------------------------------------------------------------
// FM engine pages
// ---------------------------------------------------------------------------

pub static FM_ALG: BlockDef = BlockDef {
    id: 5,
    name: "4opFM",
    short: "FM",
    layout: PageLayout::CellGrid,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot::param(BlockRef::Fm, FmParams::ALGORITHM),
        EMPTY,
        // One address per physical param: the voice's output level.
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static FM_OP: BlockDef = BlockDef {
    id: 6,
    name: "Operator",
    short: "OP",
    layout: PageLayout::CellGrid,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot::select_op(),
        ParamSlot::selected_op(FmOpParams::WAVEFORM),
        ParamSlot::selected_op(FmOpParams::LEVEL),
        ParamSlot::selected_op(FmOpParams::FEEDBACK),
        ParamSlot::selected_op(FmOpParams::DETUNE),
        ParamSlot::selected_op(FmOpParams::VELOCITY_SENS),
    ],
};

pub static FM_RATIO: BlockDef = BlockDef {
    id: 7,
    name: "Ratios",
    short: "RAT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::COARSE).with_label("OP1"),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::COARSE).with_label("OP2"),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::COARSE).with_label("OP3"),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::COARSE).with_label("OP4"),
        ParamSlot::selected_op(FmOpParams::FINE),
        EMPTY,
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
    viz: VizType::DriveClip,
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
    viz: VizType::WaveFold,
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

pub static FILTER: BlockDef = BlockDef {
    id: 10,
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::CUTOFF),
        ParamSlot::param(BlockRef::Filter, FilterParams::RESONANCE),
        ParamSlot::param(BlockRef::Filter, FilterParams::DRIVE),
        ParamSlot::param(BlockRef::Filter, FilterParams::FM_AMOUNT),
        ParamSlot::param(BlockRef::Filter, FilterParams::ENV_AMOUNT),
        ParamSlot::param(BlockRef::Filter, FilterParams::KEY_TRACK),
    ],
};

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
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::ATTACK),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::DECAY),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::SUSTAIN),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::RELEASE),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::LEVEL).with_label("DEPTH"),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::VEL_SENS),
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
        ParamSlot::param(BlockRef::Lfo, LfoParams::RATE),
        ParamSlot::param(BlockRef::Lfo, LfoParams::SHAPE),
        ParamSlot::param(BlockRef::Lfo, LfoParams::SYNC),
        ParamSlot::param(BlockRef::Lfo, LfoParams::PHASE),
        ParamSlot::param(BlockRef::Lfo, LfoParams::DEPTH),
        ParamSlot::param(BlockRef::Lfo, LfoParams::OFFSET),
    ],
};

pub static ENV_AMP: BlockDef = BlockDef {
    id: 13,
    name: "Env Amp",
    short: "ENV",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot::legacy("ATK", ValFmt::Uni),
        ParamSlot::legacy("DEC", ValFmt::Uni),
        ParamSlot::legacy("SUS", ValFmt::Uni),
        ParamSlot::legacy("REL", ValFmt::Uni),
        ParamSlot::legacy("LEVEL", ValFmt::Uni),
        ParamSlot::legacy("VEL", ValFmt::Uni),
    ],
};

pub static ENV_FILTER: BlockDef = BlockDef {
    id: 14,
    name: "Env Filter",
    short: "E.F",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot::legacy("ATK", ValFmt::Uni),
        ParamSlot::legacy("DEC", ValFmt::Uni),
        ParamSlot::legacy("SUS", ValFmt::Uni),
        ParamSlot::legacy("REL", ValFmt::Uni),
        ParamSlot::legacy("LEVEL", ValFmt::Uni),
        ParamSlot::legacy("VEL", ValFmt::Uni),
    ],
};

pub static ENV_AUX: BlockDef = BlockDef {
    id: 15,
    name: "Env Aux",
    short: "E.X",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot::legacy("ATK", ValFmt::Uni),
        ParamSlot::legacy("DEC", ValFmt::Uni),
        ParamSlot::legacy("SUS", ValFmt::Uni),
        ParamSlot::legacy("REL", ValFmt::Uni),
        ParamSlot::legacy("LEVEL", ValFmt::Uni),
        ParamSlot::legacy("VEL", ValFmt::Uni),
    ],
};

// ---------------------------------------------------------------------------
// FX / Mix chain
// ---------------------------------------------------------------------------

pub static EFX: BlockDef = BlockDef {
    id: 16,
    name: "Reverb",
    short: "EFX",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow,
    params: [
        ParamSlot::param(BlockRef::Reverb, ReverbParams::REVERB_TYPE),
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
        ParamSlot::param(BlockRef::Delay, DelayParams::WOW_FLUTTER),
        ParamSlot::param(BlockRef::Delay, DelayParams::SATURATION),
        ParamSlot::param(BlockRef::Delay, DelayParams::TONE),
        ParamSlot::param(BlockRef::Delay, DelayParams::MIX),
    ],
};

pub static MASTER: BlockDef = BlockDef {
    id: 20,
    name: "Master",
    short: "MST",
    layout: PageLayout::BigViz,
    viz: VizType::CompressorCurve,
    params: [
        ParamSlot::legacy("VOL", ValFmt::Uni),
        ParamSlot::legacy("PAN", ValFmt::Bi),
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
    id: 21,
    name: "Noise",
    short: "NSE",
    layout: PageLayout::CellGrid,
    viz: VizType::WaveformPreview,
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
    viz: VizType::RoutingMatrix,
    params: [EMPTY; 6],
};

// ---------------------------------------------------------------------------
// TX81Z 5-stage envelopes (one per FM operator)
// ---------------------------------------------------------------------------

pub static FM_ENV1: BlockDef = BlockDef {
    id: 23,
    name: "Op1 Env",
    short: "E1",
    layout: PageLayout::BigViz,
    viz: VizType::FmEnvelope,
    params: [
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::ATTACK_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::DECAY1_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::DECAY1_LEVEL),
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::DECAY2_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::RELEASE_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::RATE_SCALING),
    ],
};

pub static FM_ENV2: BlockDef = BlockDef {
    id: 24,
    name: "Op2 Env",
    short: "E2",
    layout: PageLayout::BigViz,
    viz: VizType::FmEnvelope,
    params: [
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::ATTACK_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::DECAY1_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::DECAY1_LEVEL),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::DECAY2_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::RELEASE_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::RATE_SCALING),
    ],
};

pub static FM_ENV3: BlockDef = BlockDef {
    id: 25,
    name: "Op3 Env",
    short: "E3",
    layout: PageLayout::BigViz,
    viz: VizType::FmEnvelope,
    params: [
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::ATTACK_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::DECAY1_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::DECAY1_LEVEL),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::DECAY2_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::RELEASE_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::RATE_SCALING),
    ],
};

pub static FM_ENV4: BlockDef = BlockDef {
    id: 26,
    name: "Op4 Env",
    short: "E4",
    layout: PageLayout::BigViz,
    viz: VizType::FmEnvelope,
    params: [
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::ATTACK_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::DECAY1_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::DECAY1_LEVEL),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::DECAY2_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::RELEASE_RATE),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::RATE_SCALING),
    ],
};

// ---------------------------------------------------------------------------
// Chain templates
// ---------------------------------------------------------------------------

/// Mod sources every Part voice produces: source 0 = amp envelope, 1 = LFO
/// (`Voice::render`).
pub static PART_MOD_SOURCES: [&str; 2] = ["ENV", "LFO"];

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
    mod_sources: &PART_MOD_SOURCES,
};

static KICK_BLOCKS: [ChainBlock; 3] = [
    ChainBlock { def: &NOISE,      sub_pages: &[] },
    ChainBlock { def: &FILTER,     sub_pages: &[] },
    ChainBlock { def: &MOD_MATRIX, sub_pages: &MOD_MATRIX_SUB_PAGES },
];

pub static KICK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Kick",
    blocks: &KICK_BLOCKS,
    mod_sources: &PART_MOD_SOURCES,
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
    mod_sources: &PART_MOD_SOURCES,
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
    mod_sources: &PART_MOD_SOURCES,
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
    mod_sources: &[],
};

static ENVELOPE_BLOCKS: [ChainBlock; 3] = [
    ChainBlock { def: &ENV_AMP,    sub_pages: &[] },
    ChainBlock { def: &ENV_FILTER, sub_pages: &[] },
    ChainBlock { def: &ENV_AUX,    sub_pages: &[] },
];

pub static ENVELOPE_CHAIN: ChainDef2 = ChainDef2 {
    name: "Envelopes",
    blocks: &ENVELOPE_BLOCKS,
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
    viz: VizType::EqResponse,
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
static MIXER_CHANNEL_BLOCKS: [ChainBlock; 5] = [
    ChainBlock { def: &PART,   sub_pages: &[] },
    ChainBlock { def: &SENDS,  sub_pages: &[] },
    ChainBlock { def: &CHORUS, sub_pages: &[] },
    ChainBlock { def: &DELAY,  sub_pages: &[] },
    ChainBlock { def: &EFX,    sub_pages: &[] },
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
        ParamSlot::legacy("P1 CH", ValFmt::Int(16)),
        ParamSlot::legacy("P2 CH", ValFmt::Int(16)),
        ParamSlot::legacy("P3 CH", ValFmt::Int(16)),
        ParamSlot::legacy("P4 CH", ValFmt::Int(16)),
        ParamSlot::legacy("P5 CH", ValFmt::Int(16)),
        ParamSlot::legacy("P6 CH", ValFmt::Int(16)),
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
    params: [
        ParamSlot::legacy("BRIGHT", ValFmt::Uni),
        ParamSlot::legacy("ACCENT", ValFmt::Int(4)),
        EMPTY,
        EMPTY,
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
    viz: VizType::RoutingMatrix,
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
    ChainBlock { def: &DEMO_WAVES,  sub_pages: &[] },
    ChainBlock { def: &DEMO_SHAPES, sub_pages: &[] },
    ChainBlock { def: &DEMO_MOTION, sub_pages: &[] },
    ChainBlock { def: &DEMO_FM,     sub_pages: &[] },
    ChainBlock { def: &DEMO_MATRIX, sub_pages: &[] },
];

pub static DEMO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Demo",
    blocks: &DEMO_BLOCKS,
    mod_sources: &[],
};
