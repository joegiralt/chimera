use crate::addr::{BlockRef, Op};
use crate::block::ParamId;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::comp::CompParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::drum::params::{
    KitParams, PadAmpParams, PadEnvParams, PadLfoParams, PadParams, PadRouteParams,
};
use crate::dsp::drum::types::Knob;
use crate::dsp::modal::{ModalPage, ModalParams};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::reverb::ReverbParams;
#[cfg(feature = "master-tape")]
use crate::dsp::tape::TapeParams;
use crate::dsp::tape_engine::TapeEngineParams;
use crate::dsp::va::VaParams;
use crate::dsp::voice_fx::params::VoiceFxParams;
use crate::modulation::{MAX_MOD_SOURCES, ModSource};
use crate::params::{EnvParams, FilterParams, OutParams, PitchParams};
use crate::part::PartParams;
use crate::ui::block_def::{
    BlockDef, ChainBlock, ChainDef2, FxFlow, FxNode, MachineSlot, ParamSlot, VizType,
};
use crate::ui::midi_settings::MidiSettings;
use crate::ui::page::{PageLayout, ValFmt};
use crate::ui::reverb_panel::RevPage;
use crate::ui::settings::leaves::{
    ABOUT_LEAF, AUDIO_LOAD_LEAF, CHANNELS_LEAF, DEMO_LEAVES, ECHO_LEAF, FOLLOW_LEAF,
    LIVE_SNAP_LEAF, MAP_LEAF, MPE_LEAF, OUTPUTS_LEAF, POOL_LEAF, PUSH_LEAF, RANDOM_LEAF,
    SAVER_LEAF, SYNC_LEAF, TEXT_LEAF, THEME_LEAF, TUNING_LEAF,
};
use crate::ui::theme_settings::ThemeSettings;

const EMPTY: ParamSlot = ParamSlot::EMPTY;

// ---------------------------------------------------------------------------
// Modal engine pages
// ---------------------------------------------------------------------------

/// EXC: the model's exciter; its cells follow MODEL.
pub static MODAL_EXC: BlockDef = BlockDef {
    id: 67,
    name: "Exciter",
    short: "EXC",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::modal_panel(ModalPage::Exciter, 0),
        ParamSlot::modal_panel(ModalPage::Exciter, 1),
        ParamSlot::modal_panel(ModalPage::Exciter, 2),
        ParamSlot::modal_panel(ModalPage::Exciter, 3),
        ParamSlot::modal_panel(ModalPage::Exciter, 4),
        ParamSlot::modal_panel(ModalPage::Exciter, 5),
    ],
};

/// Home: MODEL and the four macros.
pub static MODAL_1: BlockDef = BlockDef {
    id: 2,
    name: "Modal",
    short: "RES",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Modal, ModalParams::MODE),
        ParamSlot::param(BlockRef::Modal, ModalParams::STRUCTURE),
        ParamSlot::param(BlockRef::Modal, ModalParams::BRIGHT),
        ParamSlot::param(BlockRef::Modal, ModalParams::DAMP),
        ParamSlot::param(BlockRef::Modal, ModalParams::POS),
        EMPTY,
    ],
};

/// The model page: its cells follow MODEL.
pub static MODAL_2: BlockDef = BlockDef {
    id: 3,
    name: "Modal-2",
    short: "MDL2",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::modal_panel(ModalPage::Model, 0),
        ParamSlot::modal_panel(ModalPage::Model, 1),
        ParamSlot::modal_panel(ModalPage::Model, 2),
        ParamSlot::modal_panel(ModalPage::Model, 3),
        ParamSlot::modal_panel(ModalPage::Model, 4),
        ParamSlot::modal_panel(ModalPage::Model, 5),
    ],
};

// ---------------------------------------------------------------------------
// TAPE engine pages (spec § 3): SRC · DECK, the model page under DECK
// ---------------------------------------------------------------------------

/// SRC: the source and the window it plays. Ids 93–95: POOL took 92.
pub static TAPE_SRC: BlockDef = BlockDef {
    id: 93,
    name: "Source",
    short: "SRC",
    layout: PageLayout::CellGrid,
    viz: VizType::Strip,
    params: [
        ParamSlot::source(),
        ParamSlot::param(BlockRef::TapeEngine, TapeEngineParams::START),
        ParamSlot::param(BlockRef::TapeEngine, TapeEngineParams::END),
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME).with_label(&"VOL"),
        EMPTY,
        EMPTY,
    ],
};

/// DECK, the home: MODEL, then every knob that moves the machine's glyph
/// as the machine labels them (ADR 0178).
pub static TAPE_DECK: BlockDef = BlockDef {
    id: 94,
    name: "Deck",
    short: "DECK",
    layout: PageLayout::CellGrid,
    viz: VizType::Strip,
    params: [
        ParamSlot::param(BlockRef::TapeEngine, TapeEngineParams::MODEL),
        ParamSlot::machine(MachineSlot::Deck(0)),
        ParamSlot::machine(MachineSlot::Deck(1)),
        ParamSlot::machine(MachineSlot::Deck(2)),
        ParamSlot::machine(MachineSlot::Deck(3)),
        ParamSlot::machine(MachineSlot::Deck(4)),
    ],
};

/// The model page: named by the machine, TAPE, AGE and WOW in the same
/// cells on every machine, then the machine's other cells (ADR 0178).
pub static TAPE_MODEL: BlockDef = BlockDef {
    id: 95,
    name: "Machine",
    short: "MCH",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::TapeEngine, TapeEngineParams::TAPE),
        ParamSlot::param(BlockRef::TapeEngine, TapeEngineParams::AGE),
        ParamSlot::param(BlockRef::TapeEngine, TapeEngineParams::WOW),
        ParamSlot::machine(MachineSlot::Page(0)),
        ParamSlot::machine(MachineSlot::Page(1)),
        ParamSlot::machine(MachineSlot::Page(2)),
    ],
};

// ---------------------------------------------------------------------------
// VA engine pages (spec § Pages): VA, then OSC 1, OSC 2 and NOISE beneath it
// ---------------------------------------------------------------------------

/// VA, the home. Ids 87-89: free, under the focus table's top. AMOUNT
/// reads BLEND, STAB STABILITY on the page (ADR 0129); their idents stay.
pub static VA_HOME: BlockDef = BlockDef {
    id: 87,
    name: "VA",
    short: "VA",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Va, VaParams::WAVE1),
        ParamSlot::param(BlockRef::Va, VaParams::WAVE2),
        ParamSlot::param(BlockRef::Va, VaParams::MODE),
        ParamSlot::param(BlockRef::Va, VaParams::AMOUNT).with_label(&"BLEND"),
        ParamSlot::param(BlockRef::Va, VaParams::SUB),
        ParamSlot::param(BlockRef::Va, VaParams::STAB).with_label(&"STABILITY"),
    ],
};

pub static VA_OSC1: BlockDef = BlockDef {
    id: 88,
    name: "Osc 1",
    short: "OSC1",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Va, VaParams::CRSE1).with_label(&"CRSE"),
        ParamSlot::param(BlockRef::Va, VaParams::FINE1).with_label(&"FINE"),
        ParamSlot::param(BlockRef::Va, VaParams::SHAPE1).with_label(&"SHAPE"),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static VA_OSC2: BlockDef = BlockDef {
    id: 89,
    name: "Osc 2",
    short: "OSC2",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Va, VaParams::CRSE2).with_label(&"CRSE"),
        ParamSlot::param(BlockRef::Va, VaParams::FINE2).with_label(&"FINE"),
        ParamSlot::param(BlockRef::Va, VaParams::SHAPE2).with_label(&"SHAPE"),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// NOISE (ADR 0197): the voice's noise source, mixed beside SUB after
/// MODE. Id 110: the next past SETTINGS' 109.
pub static VA_NOISE: BlockDef = BlockDef {
    id: 110,
    name: "Noise",
    short: "NOISE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Va, VaParams::NOISE_KIND),
        ParamSlot::param(BlockRef::Va, VaParams::NOISE).with_label(&"LEVEL"),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Voice pitch (ADR 0042): a sub-page of every engine's node
// ---------------------------------------------------------------------------

/// PIT: the voice's PITCH and FINE, a steal's STEAL and GLIDE TIME
/// (#254), and PITCH's SNAP (TAPE Task 12b); F is free. The scope reads
/// the sounding pitch in its corner (ADR 0201).
pub static PITCH: BlockDef = BlockDef {
    id: 66,
    name: "Pitch",
    short: "PIT",
    layout: PageLayout::CellGrid,
    viz: VizType::Tuner,
    params: [
        ParamSlot::param(BlockRef::Pitch, PitchParams::PITCH),
        ParamSlot::param(BlockRef::Pitch, PitchParams::FINE),
        ParamSlot::param(BlockRef::Pitch, PitchParams::STEAL),
        ParamSlot::param(BlockRef::Pitch, PitchParams::GLIDE_TIME),
        ParamSlot::param(BlockRef::Pitch, PitchParams::SNAP),
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// Amp
// ---------------------------------------------------------------------------

/// AMP: the VCA's VEL and its low-pass gate, LPG · DECAY · STRIKE (ADR
/// 0195), last before MOD on every Part chain; E and F are free. The sends
/// are AMP › SENDS, the mixer's own SENDS page.
pub static AMP: BlockDef = BlockDef {
    id: 9,
    name: "Amp",
    short: "AMP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Out, OutParams::VCA_VEL),
        ParamSlot::param(BlockRef::Out, OutParams::LPG),
        ParamSlot::param(BlockRef::Out, OutParams::DECAY),
        ParamSlot::param(BlockRef::Out, OutParams::STRIKE),
        EMPTY,
        EMPTY,
    ],
};

/// AMP › SENDS (ADR 0195): CHORUS · DELAY · REVERB, the page the mixer
/// opens on, so its cells keep one focus.
static AMP_SUB_PAGES: [&BlockDef; 1] = [&SENDS];

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

/// FLT's front (ADR 0167): KIND · FX · Q or RES · MODE or VOICE · FX PLACE ·
/// FREQ or CUTOFF, the kind's words.
pub static FILTER: BlockDef = BlockDef {
    id: 10,
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::KIND),
        ParamSlot::param(BlockRef::VoiceFx, VoiceFxParams::TYPE),
        ParamSlot::filter_panel(2),
        ParamSlot::filter_panel(0),
        ParamSlot::param(BlockRef::VoiceFx, VoiceFxParams::PLACE).with_label(&"FX PLACE"),
        ParamSlot::filter_panel(1),
    ],
};

/// FLT › MOD: the type's cells (filter-types spec § 2, § 3, ADR 0190): ENV AMT ·
/// KEYBOARD · FREQ MOD · FEEDBACK · RES MOD; F is SEM's
/// OUTPUT (ADR 0192), empty on the others. `short` is
/// "FMD" (filter mod), not "MOD": on the Algo map "MOD" would read as the
/// MOD node's own, and "FMOD" runs into the next node.
pub static FILTER_MOD: BlockDef = BlockDef {
    id: 59,
    name: "Filter Mod",
    short: "FMD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::filter_panel(3),
        ParamSlot::filter_panel(4),
        ParamSlot::filter_panel(5),
        ParamSlot::filter_panel(6),
        ParamSlot::filter_panel(7),
        ParamSlot::filter_panel(8),
    ],
};

/// FLT › JACK (ADR 0190): the jacks, patching, not sound; FM3 IN in C and
/// QM IN in E, under FM3's and QM's encoders on FLT › MOD.
pub static FILTER_JACKS: BlockDef = BlockDef {
    id: 98,
    name: "Filter Jacks",
    short: "JACK",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        EMPTY,
        EMPTY,
        ParamSlot::filter_panel(10),
        EMPTY,
        ParamSlot::filter_panel(11),
        EMPTY,
    ],
};

/// FLT › FX (ADR 0155): INPUT (the filter's LEVEL), the FX TYPE's three
/// pots, MIX, and the chip's cell, STABILITY on the 2040 and LADDER (ADR
/// 0168). In DRV's freed id, so page memory doesn't grow.
pub static FILTER_FX: BlockDef = BlockDef {
    id: 8,
    name: "FX",
    short: "FX",
    layout: PageLayout::CellGrid,
    viz: VizType::FxCurve,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::LEVEL).with_label(&"INPUT"),
        ParamSlot::fx_panel(0),
        ParamSlot::fx_panel(1),
        ParamSlot::fx_panel(2),
        ParamSlot::param(BlockRef::VoiceFx, VoiceFxParams::MIX),
        ParamSlot::filter_panel(9),
    ],
};

static FILTER_SUB_PAGES: [&BlockDef; 3] = [&FILTER_MOD, &FILTER_FX, &FILTER_JACKS];

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
        viz: VizType::Lfo(s),
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
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::SPEED).with_label(&"E1 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env2), EnvParams::SPEED).with_label(&"E2 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env3), EnvParams::SPEED).with_label(&"E3 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::HOLD_POS).with_label(&"E1 HOLD"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env2), EnvParams::HOLD_POS).with_label(&"E2 HOLD"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env3), EnvParams::HOLD_POS).with_label(&"E3 HOLD"),
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
    // TYPE · the TYPE's four knobs · MIX (reverb TYPEs spec § Pages):
    // the level always in the same place.
    params: [
        ParamSlot::param(BlockRef::Reverb, ReverbParams::TYPE),
        ParamSlot::reverb_panel(RevPage::Main, 0),
        ParamSlot::reverb_panel(RevPage::Main, 1),
        ParamSlot::reverb_panel(RevPage::Main, 2),
        ParamSlot::reverb_panel(RevPage::Main, 3),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::MIX),
    ],
};

/// REV › MORE: PRE-DLY · DAMP, SHAPE or CURVE · SMEAR · RATE · SYNC, by
/// the TYPE's panel; QUAD shows RATE only.
pub static EFX_MORE: BlockDef = BlockDef {
    id: 96,
    name: "Reverb More",
    short: "MORE",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Reverb)),
    params: [
        ParamSlot::reverb_panel(RevPage::More, 0),
        ParamSlot::reverb_panel(RevPage::More, 1),
        ParamSlot::reverb_panel(RevPage::More, 2),
        ParamSlot::reverb_panel(RevPage::More, 3),
        ParamSlot::reverb_panel(RevPage::More, 4),
        ParamSlot::reverb_panel(RevPage::More, 5),
    ],
};

/// REV › MORE 2: HALL's DENSITY (ADR 0095 § HALL, as built); empty
/// under the other TYPEs.
pub static EFX_MORE2: BlockDef = BlockDef {
    id: 97,
    name: "Reverb More 2",
    short: "MOR2",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Reverb)),
    params: [
        ParamSlot::reverb_panel(RevPage::More2, 0),
        ParamSlot::reverb_panel(RevPage::More2, 1),
        ParamSlot::reverb_panel(RevPage::More2, 2),
        ParamSlot::reverb_panel(RevPage::More2, 3),
        ParamSlot::reverb_panel(RevPage::More2, 4),
        ParamSlot::reverb_panel(RevPage::More2, 5),
    ],
};

static EFX_SUB_PAGES: [&BlockDef; 2] = [&EFX_MORE, &EFX_MORE2];

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
    // TYPE · the shared five (ADR 0202): TAPE's own in DLY › CHAR,
    // GRAIN's in DLY › GRAIN.
    params: [
        ParamSlot::param(BlockRef::Delay, DelayParams::TYPE),
        ParamSlot::param(BlockRef::Delay, DelayParams::TIME_MS),
        ParamSlot::param(BlockRef::Delay, DelayParams::FEEDBACK),
        ParamSlot::param(BlockRef::Delay, DelayParams::TONE),
        ParamSlot::param(BlockRef::Delay, DelayParams::REV_SEND),
        ParamSlot::param(BlockRef::Delay, DelayParams::MIX),
    ],
};

/// DLY › CHAR: TAPE's character, off the delay's main page (FX diet spec
/// § UI).
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

/// DLY › GRAIN: GRAIN's four (ADR 0202). A table, no code, so bank 1
/// stays frozen (ADR 0198): the page is there under TAPE too, its knobs
/// held for GRAIN.
pub static DELAY_GRAIN: BlockDef = BlockDef {
    id: 111,
    name: "Delay Grain",
    short: "GRAIN",
    layout: PageLayout::CellGrid,
    viz: VizType::EffectsFlow(FxFlow::Effect(FxNode::Delay)),
    params: [
        ParamSlot::param(BlockRef::Delay, DelayParams::SIZE),
        ParamSlot::param(BlockRef::Delay, DelayParams::DENSITY),
        ParamSlot::param(BlockRef::Delay, DelayParams::PITCH),
        ParamSlot::param(BlockRef::Delay, DelayParams::TEXTURE),
        EMPTY,
        EMPTY,
    ],
};

static DELAY_SUB_PAGES: [&BlockDef; 2] = [&DELAY_CHAR, &DELAY_GRAIN];

/// Tape on DAC pair 1 (FX diet spec § Tape); only with `master-tape`
/// (ADR 0055).
#[cfg(feature = "master-tape")]
pub static TAPE: BlockDef = BlockDef {
    id: 57,
    name: "Reel",
    short: "REEL",
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
        ParamSlot::param(BlockRef::Comp, CompParams::VOL),
        ParamSlot::param(BlockRef::Comp, CompParams::PAN),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// MST › SMART EQ (ADR 0296): the switch and AMOUNT for every Part's SMART
/// EQ cuts; the band shows each Part's four live cuts and its role.
pub static MASTER_SMART: BlockDef = BlockDef {
    id: 120,
    name: "Smart EQ",
    short: "SMART",
    layout: PageLayout::CellGrid,
    viz: VizType::SmartEq,
    params: [
        ParamSlot::param(BlockRef::Comp, CompParams::SMART_EQ),
        ParamSlot::param(BlockRef::Comp, CompParams::AMOUNT),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

static MASTER_SUB_PAGES: [&BlockDef; 2] = [&MASTER_LEVEL, &MASTER_SMART];

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
        ParamSlot::param(BlockRef::AlgoOp(Op::A), id).with_label(&"OP1"),
        ParamSlot::param(BlockRef::AlgoOp(Op::B), id).with_label(&"OP2"),
        ParamSlot::param(BlockRef::AlgoOp(Op::C), id).with_label(&"OP3"),
        ParamSlot::param(BlockRef::AlgoOp(Op::D), id).with_label(&"OP4"),
        ParamSlot::param(BlockRef::AlgoOp(Op::E), id).with_label(&"OP5"),
        ParamSlot::param(BlockRef::AlgoOp(Op::F), id).with_label(&"OP6"),
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

/// An envelope stage's group: the band draws the focused operator's envelope.
const fn env_group(id: u16, name: &'static str, short: &'static str, param: ParamId) -> BlockDef {
    BlockDef {
        viz: VizType::OpEnv,
        ..group(id, name, short, param)
    }
}

/// The OSC node's home page; its short name labels the node on the map.
pub static ALGO_WAVE: BlockDef = group(42, "Wave", "OSC", AlgoOpParams::WAVE);
pub static ALGO_LEVEL: BlockDef = group(44, "Level", "LVL", AlgoOpParams::LEVEL);
pub static ALGO_COARSE: BlockDef = group(45, "Coarse", "CRS", AlgoOpParams::COARSE);
pub static ALGO_FINE: BlockDef = group(46, "Fine", "FIN", AlgoOpParams::FINE);
pub static ALGO_DETUNE: BlockDef = group(47, "Detune", "DET", AlgoOpParams::DETUNE);
pub static ALGO_VELOCITY: BlockDef = group(48, "Velocity", "VEL", AlgoOpParams::VELOCITY);
pub static ALGO_AR: BlockDef = env_group(49, "Env AR", "AR", AlgoOpParams::AR);
pub static ALGO_D1R: BlockDef = env_group(50, "Env D1R", "D1R", AlgoOpParams::D1R);
pub static ALGO_D1L: BlockDef = env_group(51, "Env D1L", "D1L", AlgoOpParams::D1L);
pub static ALGO_D2R: BlockDef = env_group(52, "Env D2R", "D2R", AlgoOpParams::D2R);
pub static ALGO_RR: BlockDef = env_group(53, "Env RR", "RR", AlgoOpParams::RR);
pub static ALGO_RATE_SCALE: BlockDef = group(54, "Rate Scale", "RS", AlgoOpParams::RATE_SCALE);
pub static ALGO_FEEDBACK: BlockDef = group(55, "Feedback", "FBK", AlgoOpParams::FEEDBACK);

/// One FRMT per operator, on OSC while any operator is a formant.
pub static ALGO_FORMANT: BlockDef = group(106, "Formant", "FRM", AlgoOpParams::FRMT);

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
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME).with_label(&"VOL"),
        ParamSlot::param(BlockRef::Algo, AlgoParams::VOWEL),
    ],
};

// ---------------------------------------------------------------------------
// Chain templates
// ---------------------------------------------------------------------------

/// Mod sources every Part voice produces, in `ModSource` order (spec § 2).
pub static PART_MOD_SOURCES: [&str; MAX_MOD_SOURCES] = {
    let mut tags = [""; MAX_MOD_SOURCES];
    let mut i = 0;
    while i < MAX_MOD_SOURCES {
        tags[i] = ModSource::ALL[i].tag();
        i += 1;
    }
    tags
};

/// The MOD node's sub-list after its home MTX (spec § UI).
static MOD_SUB_PAGES: [&BlockDef; 7] =
    [&ENVELOPE, &ENV_2, &ENV_3, &ENV_SPEED, &LFO, &LFO_2, &LFO_3];

static MODAL_SUB_PAGES: [&BlockDef; 2] = [&MODAL_2, &PITCH];

static MODAL_PLUCK_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::page(&MODAL_EXC),
    ChainBlock::with_subs(&MODAL_1, &MODAL_SUB_PAGES),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::with_subs(&AMP, &AMP_SUB_PAGES),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

/// Modal's home is RES (owner, 2026-10-01: ADR 0066).
pub static MODAL_PLUCK_CHAIN: ChainDef2 =
    ChainDef2::new("Modal Pluck", &MODAL_PLUCK_BLOCKS, &PART_MOD_SOURCES).with_home_def(&MODAL_1);

pub use crate::ui::op_page::ALGO_OP_PAGES;

/// WAVE is the OSC node's home; FINE's DETUNE and the five ENV stages sit
/// right after their group (sub-pages are one level deep), then OP1–OP6.
static ALGO_OSC_SUB_PAGES: [&BlockDef; 18] = [
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
    &ALGO_OP_PAGES[0],
    &ALGO_OP_PAGES[1],
    &ALGO_OP_PAGES[2],
    &ALGO_OP_PAGES[3],
    &ALGO_OP_PAGES[4],
    &ALGO_OP_PAGES[5],
];

/// `ALGO_OSC_SUB_PAGES` with the FORMANT page first.
static ALGO_FORMANT_OSC_SUB_PAGES: [&BlockDef; 19] = {
    let mut a = [&ALGO_FORMANT; 19];
    let mut i = 0;
    while i < 18 {
        a[i + 1] = ALGO_OSC_SUB_PAGES[i];
        i += 1;
    }
    a
};

/// ALGO is the engine's home: first on the map, where entering the chain lands.
static ALGO_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::with_subs(&ALGO_ALG, &[&PITCH]),
    ChainBlock::with_subs(&ALGO_WAVE, &ALGO_OSC_SUB_PAGES),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::with_subs(&AMP, &AMP_SUB_PAGES),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

pub static ALGO_CHAIN: ChainDef2 = ChainDef2::new("Algo", &ALGO_BLOCKS, &PART_MOD_SOURCES);

static ALGO_FORMANT_BLOCKS: [ChainBlock; 5] = {
    let mut b = [ALGO_BLOCKS[0]; 5];
    let mut i = 1;
    while i < 5 {
        b[i] = ALGO_BLOCKS[i];
        i += 1;
    }
    b[1] = ChainBlock::with_subs(&ALGO_WAVE, &ALGO_FORMANT_OSC_SUB_PAGES);
    b
};

/// ALGO's chain while an operator is a formant or a noise band.
pub static ALGO_FORMANT_CHAIN: ChainDef2 =
    ChainDef2::new("Algo", &ALGO_FORMANT_BLOCKS, &PART_MOD_SOURCES);

static TAPE_SUB_PAGES: [&BlockDef; 2] = [&TAPE_MODEL, &PITCH];

/// SRC · DECK · FLT · AMP · MOD: ALGO's shape with MODAL's first
/// node; the model page and PIT under DECK.
static TAPE_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::page(&TAPE_SRC),
    ChainBlock::with_subs(&TAPE_DECK, &TAPE_SUB_PAGES),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::with_subs(&AMP, &AMP_SUB_PAGES),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

/// TAPE's home is DECK (ADR 0066, MODAL's rule).
pub static TAPE_CHAIN: ChainDef2 =
    ChainDef2::new("Tape", &TAPE_BLOCKS, &PART_MOD_SOURCES).with_home_def(&TAPE_DECK);

static VA_SUB_PAGES: [&BlockDef; 4] = [&VA_OSC1, &VA_OSC2, &VA_NOISE, &PITCH];

/// VA · FLT · AMP · MOD: the voice chain behind VA's node.
static VA_BLOCKS: [ChainBlock; 4] = [
    ChainBlock::with_subs(&VA_HOME, &VA_SUB_PAGES),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::with_subs(&AMP, &AMP_SUB_PAGES),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

pub static VA_CHAIN: ChainDef2 =
    ChainDef2::new("VA", &VA_BLOCKS, &PART_MOD_SOURCES).with_home_def(&VA_HOME);

// ---------------------------------------------------------------------------
// DRUM pages (ADR 0265): every one edits the Part's selected pad. Ids 4-7,
// 13-15, 17, 21 and 23: free, under the focus table's top.
// ---------------------------------------------------------------------------

/// KIT, the home: which pad, its machine, level and pan, and the kit's
/// TONAL (ADR 0270); the band the twelve pads.
pub static DRUM_KIT: BlockDef = BlockDef {
    id: 4,
    name: "Kit",
    short: "KIT",
    layout: PageLayout::CellGrid,
    viz: VizType::Pads,
    params: [
        ParamSlot::select_pad(),
        ParamSlot::param(BlockRef::Pad, PadParams::MACHINE),
        ParamSlot::param(BlockRef::Pad, PadParams::LEVEL),
        ParamSlot::param(BlockRef::Pad, PadParams::PAN),
        ParamSlot::param(BlockRef::Kit, KitParams::TONAL),
        EMPTY,
    ],
};

/// KEYS: how the pad's hits meet: its choke group and retrigger.
pub static DRUM_KEYS: BlockDef = BlockDef {
    id: 5,
    name: "Keys",
    short: "KEYS",
    layout: PageLayout::CellGrid,
    viz: VizType::Pads,
    params: [
        ParamSlot::select_pad(),
        ParamSlot::param(BlockRef::Pad, PadParams::CHOKE),
        ParamSlot::param(BlockRef::Pad, PadParams::RETRIG),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// MCH: the pad's machine's knobs A-F, each its machine's; the header
/// names the machine.
pub static DRUM_MCH: BlockDef = BlockDef {
    id: 6,
    name: "Machine",
    short: "MCH",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::knob(Knob::A),
        ParamSlot::knob(Knob::B),
        ParamSlot::knob(Knob::C),
        ParamSlot::knob(Knob::D),
        ParamSlot::knob(Knob::E),
        ParamSlot::knob(Knob::F),
    ],
};

/// MORE: MACHINE again, and the machine's knobs G and H.
pub static DRUM_MORE: BlockDef = BlockDef {
    id: 7,
    name: "More",
    short: "MORE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Pad, PadParams::MACHINE),
        ParamSlot::knob(Knob::G),
        ParamSlot::knob(Knob::H),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// DRUM's FLT (ADR 0269): FLT's front on the pad's filter, the Part's FX
/// TYPE and FX PLACE between; the curve the pad's.
pub static DRUM_FLT: BlockDef = BlockDef {
    id: 13,
    params: [
        ParamSlot::param(BlockRef::PadFilter, FilterParams::KIND),
        ParamSlot::param(BlockRef::VoiceFx, VoiceFxParams::TYPE),
        ParamSlot::pad_filter_panel(2),
        ParamSlot::pad_filter_panel(0),
        ParamSlot::param(BlockRef::VoiceFx, VoiceFxParams::PLACE).with_label(&"FX PLACE"),
        ParamSlot::pad_filter_panel(1),
    ],
    ..FILTER
};

/// An AHD page on `b`'s envelope: ATTACK · HOLD · DECAY · SPEED, the
/// curve the pad's; E and F `e`, `f`.
const fn pad_env_page(
    id: u16,
    name: &'static str,
    short: &'static str,
    b: BlockRef,
    e: ParamSlot,
    f: ParamSlot,
) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::BigViz,
        viz: VizType::Adsr,
        params: [
            ParamSlot::param(b, PadEnvParams::ATTACK),
            ParamSlot::param(b, PadEnvParams::HOLD),
            ParamSlot::param(b, PadEnvParams::DECAY),
            ParamSlot::param(b, PadEnvParams::SPEED),
            e,
            f,
        ],
    }
}

/// DRUM's AMP: the pad's AHD, its VEL and its MODE (ONE-SHOT or GATED).
pub static DRUM_AMP: BlockDef = pad_env_page(
    14,
    "Amp",
    "AMP",
    BlockRef::PadAmp,
    ParamSlot::param(BlockRef::PadAmp, PadAmpParams::VEL),
    ParamSlot::param(BlockRef::PadAmp, PadAmpParams::MODE),
);

/// AMP › GATE: the pad's low-pass gate, in AMP's places.
pub static DRUM_GATE: BlockDef = BlockDef {
    id: 15,
    name: "Gate",
    short: "GATE",
    params: [
        EMPTY,
        ParamSlot::param(BlockRef::PadAmp, PadAmpParams::LPG),
        ParamSlot::param(BlockRef::PadAmp, PadAmpParams::GATE_DECAY),
        ParamSlot::param(BlockRef::PadAmp, PadAmpParams::STRIKE),
        EMPTY,
        EMPTY,
    ],
    ..AMP
};

/// MOD › PENV: the pad's mod envelope.
pub static DRUM_PENV: BlockDef =
    pad_env_page(17, "Pad Env", "PENV", BlockRef::PadEnv, EMPTY, EMPTY);

/// MOD › PLFO: the pad's LFO.
pub static DRUM_PLFO: BlockDef = BlockDef {
    id: 21,
    name: "Pad LFO",
    short: "PLFO",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::PadLfo, PadLfoParams::WAVE),
        ParamSlot::param(BlockRef::PadLfo, PadLfoParams::SPEED),
        ParamSlot::param(BlockRef::PadLfo, PadLfoParams::DEPTH),
        ParamSlot::param(BlockRef::PadLfo, PadLfoParams::PHASE),
        ParamSlot::param(BlockRef::PadLfo, PadLfoParams::MODE),
        EMPTY,
    ],
};

/// MOD › ROUTE: the pad's four routes, one at a time.
pub static DRUM_ROUTE: BlockDef = BlockDef {
    id: 23,
    name: "Route",
    short: "ROUTE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::select_route(),
        ParamSlot::selected_route(PadRouteParams::SOURCE),
        ParamSlot::selected_route(PadRouteParams::DEST),
        ParamSlot::selected_route(PadRouteParams::AMOUNT),
        EMPTY,
        EMPTY,
    ],
};

/// DRUM's MOD sub-list: the pad's PENV, PLFO and ROUTE, then the Part's.
static DRUM_MOD_SUB_PAGES: [&BlockDef; 10] = {
    let mut a = [&DRUM_PENV; 10];
    a[1] = &DRUM_PLFO;
    a[2] = &DRUM_ROUTE;
    let mut i = 0;
    while i < MOD_SUB_PAGES.len() {
        a[i + 3] = MOD_SUB_PAGES[i];
        i += 1;
    }
    a
};

/// KIT · MCH · FLT · AMP · MOD: the pad's pages; KEYS under KIT, MORE
/// under MCH; FLT's subs and AMP's SENDS the Part's.
static DRUM_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::with_subs(&DRUM_KIT, &[&DRUM_KEYS]),
    ChainBlock::with_subs(&DRUM_MCH, &[&DRUM_MORE]),
    ChainBlock::with_subs(&DRUM_FLT, &FILTER_SUB_PAGES),
    ChainBlock::with_subs(&DRUM_AMP, &[&DRUM_GATE, &SENDS]),
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &DRUM_MOD_SUB_PAGES,
        map: Some("MOD"),
    },
];

pub static DRUM_CHAIN: ChainDef2 = ChainDef2::new("Drum", &DRUM_BLOCKS, &PART_MOD_SOURCES);

// ---------------------------------------------------------------------------
// Mixer channel strip
// ---------------------------------------------------------------------------

/// A Part's MIDI channel, mode, output, level, pan and pitch-bend range (spec § UI).
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
        ParamSlot::param(BlockRef::Part, PartParams::BEND),
    ],
};

/// PRT's sub-page: a Part's share of the voice pool beside MONO/POLY
/// (ADR 0281). MODE is PRT's too.
pub static PART_VOICES: BlockDef = BlockDef {
    id: 117,
    name: "Voices",
    short: "VOI",
    layout: PageLayout::CellGrid,
    viz: VizType::MixerLevels,
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::MODE),
        ParamSlot::param(BlockRef::Part, PartParams::MAX_VOICES),
        ParamSlot::param(BlockRef::Part, PartParams::RESERVE),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// PRT's second sub-page: the Part's channel EQ (ADR 0284), TILT and LO
/// CUT, before LEVEL, PAN and the sends, and its SMART EQ ROLE; the band
/// shows SMART EQ's four live cuts on the Part (ADR 0296).
pub static PART_EQ: BlockDef = BlockDef {
    id: 118,
    name: "EQ",
    short: "EQ",
    layout: PageLayout::CellGrid,
    viz: VizType::SmartEq,
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::TILT),
        ParamSlot::param(BlockRef::Part, PartParams::LO_CUT),
        ParamSlot::param(BlockRef::Part, PartParams::ROLE),
        EMPTY,
        EMPTY,
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
        ParamSlot::param(BlockRef::Out, OutParams::SEND1),
        ParamSlot::param(BlockRef::Out, OutParams::SEND2),
        ParamSlot::param(BlockRef::Out, OutParams::SEND3),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// MIX + B<n>: Part n's mix settings, then the shared FX (spec § UI).
/// TAPE sits before MASTER only with `master-tape` (ADR 0055).
static MIXER_CHANNEL_BLOCKS: &[ChainBlock] = &[
    ChainBlock::with_subs(&PART, &[&PART_VOICES, &PART_EQ]),
    ChainBlock::page(&SENDS),
    ChainBlock::page(&CHORUS),
    ChainBlock::with_subs(&DELAY, &DELAY_SUB_PAGES),
    ChainBlock::with_subs(&EFX, &EFX_SUB_PAGES),
    // Seven nodes sit 32 px apart: REEL beside a pill doesn't fit, so the
    // map says MTP, ADR 0170's short for the master tape (#646).
    #[cfg(feature = "master-tape")]
    ChainBlock {
        def: &TAPE,
        sub_pages: &[],
        map: Some("MTP"),
    },
    ChainBlock::with_subs(&MASTER, &MASTER_SUB_PAGES),
];

/// Where the mixer opens: SENDS, not PART, whose C is OUT (ADR 0057).
pub const MIXER_HOME: usize = 1;
/// PART's node: remembered only from mixer to mixer (ADR 0057).
pub const MIXER_PART: usize = 0;

/// The mixer's home is SENDS (ADR 0057).
pub static MIXER_CHANNEL_CHAIN: ChainDef2 =
    ChainDef2::new("Mixer", MIXER_CHANNEL_BLOCKS, &[]).with_home_def(&SENDS);
const _: () = assert!(MIXER_CHANNEL_BLOCKS[MIXER_HOME].def.id == SENDS.id);
const _: () = assert!(MIXER_CHANNEL_BLOCKS[MIXER_PART].def.id == PART.id);

// ---------------------------------------------------------------------------
// SETTINGS leaf pages (`ui::settings::leaves` chains them)
// ---------------------------------------------------------------------------

pub static SYS_TUNING: BlockDef = BlockDef {
    id: 32,
    name: "Tuning",
    short: "TUN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::TUNE),
        // The active Part's, as LIVE SNAP's cells are (ADR 0240).
        ParamSlot::param(BlockRef::Part, PartParams::TUNING),
        ParamSlot::param(BlockRef::Part, PartParams::MAPPING),
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
        ParamSlot::param(BlockRef::Theme, ThemeSettings::HINTS),
        ParamSlot::param(BlockRef::Theme, ThemeSettings::INVERT),
    ],
};

/// SETTINGS › PERSONALIZE › SCREENSAVER: the idle time, one cell. Held in
/// the THEME block (`ui::theme_settings`), stored in SYSTEM with it.
pub static SYS_SAVER: BlockDef = BlockDef {
    id: 99,
    name: "Screensaver",
    short: "SAV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Theme, ThemeSettings::SAVER),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// SETTINGS › PERSONALIZE › CHAIN MAP: PAGES, how the map shows a node's
/// sub-pages (ADR 0145). Held in the THEME block, stored in SYSTEM with it.
pub static SYS_MAP: BlockDef = BlockDef {
    id: 108,
    name: "Chain Map",
    short: "MAP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Theme, ThemeSettings::PAGES),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// SETTINGS › PERSONALIZE › TEXT SIZE: SIZE, how large the reading text
/// draws (ADR 0154). Held in the THEME block, stored in SYSTEM with it.
pub static SYS_TEXT: BlockDef = BlockDef {
    id: 109,
    name: "Text Size",
    short: "TXT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Theme, ThemeSettings::TEXT),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// SETTINGS › PERSONALIZE › PAD FOLLOW: whether a live hit moves KIT's pad
/// (ADR 0268). Held in the THEME block, stored in SYSTEM with it.
pub static SYS_FOLLOW: BlockDef = BlockDef {
    id: 116,
    name: "Pad Follow",
    short: "FOL",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Theme, ThemeSettings::FOLLOW),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// SYSTEM › PUSH ENCODERS: the control board's encoders push (ADR 0140).
pub static SYS_PUSH: BlockDef = BlockDef {
    id: 107,
    name: "Push Encoders",
    short: "PSH",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::PUSH),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// MIDI CONFIG › ECHO: DIN OUT's master switch (spec § 8.4), and PARAM
/// OUT, each panel edit's NRPN (ADR 0258).
pub static SYS_ECHO: BlockDef = BlockDef {
    // Never used before, and below the top: the focus table keeps its
    // size (`MAX_PAGES`).
    id: 63,
    name: "Echo",
    short: "ECH",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::ECHO),
        ParamSlot::param(BlockRef::Midi, MidiSettings::PARAM_OUT),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// MIDI CONFIG › MPE: the lower and upper zones' member channels and
/// their bend (ADR 0252).
pub static SYS_MPE: BlockDef = BlockDef {
    // Never used before: the new top, one more focus slot (`MAX_PAGES`).
    id: 113,
    name: "MPE",
    short: "MPE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::LOWER),
        ParamSlot::param(BlockRef::Midi, MidiSettings::LOWER_BEND),
        ParamSlot::param(BlockRef::Midi, MidiSettings::UPPER),
        ParamSlot::param(BlockRef::Midi, MidiSettings::UPPER_BEND),
        EMPTY,
        EMPTY,
    ],
};

/// MIDI CONFIG › SYNC: who drives ORBIT's transport (ADR 0080), and
/// PROGRAM IN, whether a program change loads a Sound (ADR 0224).
pub static SYS_SYNC: BlockDef = BlockDef {
    id: 86,
    name: "Sync",
    short: "SYN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    // Held by `UiState` and stored in SYSTEM (`ui::midi_settings`).
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::SYNC),
        // #515: whether a program change loads a Sound (ADR 0224).
        ParamSlot::param(BlockRef::Midi, MidiSettings::PROGRAM_IN),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

pub static SYS_ABOUT: BlockDef = BlockDef {
    id: 35, // 34, SYSTEM's old UPDATES page, is retired: never reuse it.
    name: "About",
    short: "ABT",
    layout: PageLayout::BigViz,
    viz: VizType::About,
    // Read-only: `about_page` fills them.
    params: [
        ParamSlot::legacy(&"VERSION", &ValFmt::Int(0)),
        ParamSlot::legacy(&"BUILD", &ValFmt::Int(0)),
        ParamSlot::legacy(&"CHIP", &ValFmt::Int(0)),
        ParamSlot::legacy(&"CLOCK", &ValFmt::Int(0)),
        ParamSlot::legacy(&"RESET", &ValFmt::Int(0)),
        ParamSlot::legacy(&"CARD", &ValFmt::Int(0)),
    ],
};

pub static SYS_AUDIO: BlockDef = BlockDef {
    id: 41,
    name: "Audio",
    short: "AUD",
    layout: PageLayout::CellGrid,
    viz: VizType::AudioStats,
    params: [
        ParamSlot::legacy(&"LOAD", &ValFmt::Int(0)),
        ParamSlot::legacy(&"PEAK", &ValFmt::Int(0)),
        ParamSlot::legacy(&"OVER", &ValFmt::Int(0)),
        ParamSlot::legacy(&"DROPS", &ValFmt::Int(0)),
        ParamSlot::legacy(&"DESYNC", &ValFmt::Int(0)),
        ParamSlot::legacy(&"STACK", &ValFmt::Int(0)),
    ],
};

/// PROJECT › POOL: the pool's breakdown (ADR 0087).
pub static SYS_POOL: BlockDef = BlockDef {
    id: 92,
    name: "Pool",
    short: "POL",
    layout: PageLayout::CellGrid,
    viz: VizType::Pool,
    // Read-only: `pool_page` fills them.
    params: [
        ParamSlot::legacy(&"SOUNDS", &ValFmt::Int(0)),
        ParamSlot::legacy(&"SOURCES", &ValFmt::Int(0)),
        ParamSlot::legacy(&"FREE", &ValFmt::Int(0)),
        ParamSlot::legacy(&"PENDING", &ValFmt::Int(0)),
        EMPTY,
        EMPTY,
    ],
};

// ---------------------------------------------------------------------------
// DEMO pages (UI component storyboard)
// ---------------------------------------------------------------------------

pub static DEMO_WAVES: BlockDef = BlockDef {
    id: 36,
    name: "Waves",
    short: "WAV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy(&"CLIP", &ValFmt::Uni),
        ParamSlot::legacy(&"WAVE", &ValFmt::Uni),
        ParamSlot::legacy(&"PW", &ValFmt::Uni),
        ParamSlot::legacy(&"FOLD", &ValFmt::Uni),
        ParamSlot::legacy(&"TILT", &ValFmt::Bi),
        ParamSlot::legacy(&"SYM", &ValFmt::Bi),
    ],
};

pub static DEMO_SHAPES: BlockDef = BlockDef {
    id: 37,
    name: "Shapes",
    short: "SHP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy(&"ARC", &ValFmt::Uni),
        ParamSlot::legacy(&"LEVEL", &ValFmt::Uni),
        ParamSlot::legacy(&"PAN", &ValFmt::Bi),
        ParamSlot::legacy(&"D/W", &ValFmt::Bi),
        ParamSlot::legacy(&"CUBE", &ValFmt::Uni),
        ParamSlot::legacy(&"STACK", &ValFmt::Uni),
    ],
};

pub static DEMO_MOTION: BlockDef = BlockDef {
    id: 38,
    name: "Motion",
    short: "MOT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::legacy(&"RIPPL", &ValFmt::Uni),
        ParamSlot::legacy(&"BURST", &ValFmt::Uni),
        ParamSlot::legacy(&"ORBIT", &ValFmt::Uni),
        ParamSlot::legacy(&"SCATR", &ValFmt::Uni),
        ParamSlot::legacy(&"BOUNC", &ValFmt::Bi),
        ParamSlot::legacy(&"PULSE", &ValFmt::Uni),
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
        ParamSlot::legacy(&"ALGO", &ValFmt::Int(7)),
        ParamSlot::legacy(&"LOOP", &ValFmt::Uni),
        ParamSlot::legacy(&"SPIRL", &ValFmt::Uni),
        ParamSlot::legacy(&"WAVE", &ValFmt::Uni),
        EMPTY,
        EMPTY,
    ],
};

// One page per focus glyph: its slots bind the params that drive it.

/// GLYPH: ARC, on two real ARC params. a: unipolar (RESO), b: bipolar
/// from 12:00 (PAN).
pub static DEMO_GLYPH_ARC: BlockDef = BlockDef {
    id: 70,
    name: "Glyph: Arc",
    short: "ARC",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::RESONANCE).with_label(&"UNI"),
        ParamSlot::param(BlockRef::Out, OutParams::PAN).with_label(&"BI"),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// Glyph pages take ids 70 and up (68–69 are nav-core's CHANNELS and
/// OUTPUTS); raise `focus::MAX_PAGES` when one needs it.
/// GLYPH: NONE. a steps MODEL's words, which get the whole band.
pub static DEMO_GLYPH_NONE: BlockDef = BlockDef {
    id: 71,
    name: "Glyph: None",
    short: "NONE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Modal, ModalParams::MODE),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// GLYPH: SWITCH. a flips THEME's HINTS, OFF / ON (LFO SYNC, once its
/// param, draws the LFO's own glyph: ADR 0177).
pub static DEMO_GLYPH_SWITCH: BlockDef = BlockDef {
    id: 72,
    name: "Glyph: Switch",
    short: "SW",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Theme, ThemeSettings::HINTS),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// GLYPH: LEVEL. a: a continuous level (VOLUME, 8 ticks); b: a stepped
/// one (operator A's FDBK, 0..7, a tick per step).
pub static DEMO_GLYPH_LEVEL: BlockDef = BlockDef {
    id: 73,
    name: "Glyph: Level",
    short: "LVL",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME),
        ParamSlot::param(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// GLYPH: CROSSFADER. a: ALG A to ALG B (MORPH); b: dry to wet (the FX
/// slot's MIX). The cap follows the set value, never modulation.
pub static DEMO_GLYPH_XF: BlockDef = BlockDef {
    id: 74,
    name: "Glyph: Crossfader",
    short: "XF",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Algo, AlgoParams::MORPH),
        ParamSlot::param(BlockRef::VoiceFx, VoiceFxParams::MIX),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// GLYPH: BRAID, the chorus braid (the chorus specs carry it): a MODE,
/// b RATE, c DEPTH, d MIX, each emphasised when focused.
pub static DEMO_GLYPH_BRAID: BlockDef = BlockDef {
    id: 75,
    name: "Glyph: Braid",
    short: "BRD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Chorus, ChorusParams::MODE),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::RATE),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::DEPTH),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::MIX),
        EMPTY,
        EMPTY,
    ],
};

/// GLYPH: RINGS, the delay rings (the delay specs carry them): a TIME,
/// b FDBK, c TONE, d MIX, e MECHANICS, f SAT (REV, read too, has no slot
/// here).
pub static DEMO_GLYPH_RINGS: BlockDef = BlockDef {
    id: 76,
    name: "Glyph: Rings",
    short: "RNG",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Delay, DelayParams::TIME_MS),
        ParamSlot::param(BlockRef::Delay, DelayParams::FEEDBACK),
        ParamSlot::param(BlockRef::Delay, DelayParams::TONE),
        ParamSlot::param(BlockRef::Delay, DelayParams::MIX),
        ParamSlot::param(BlockRef::Delay, DelayParams::WOW_FLUTTER),
        ParamSlot::param(BlockRef::Delay, DelayParams::SATURATION),
    ],
};

/// GLYPH: CUBE, the reverb cube (the reverb specs carry it): a SIZE,
/// b TIME, c DAMP, d MIX, e SMEAR, f RATE, each emphasised when focused.
pub static DEMO_GLYPH_CUBE: BlockDef = BlockDef {
    id: 77,
    name: "Glyph: Cube",
    short: "CUB",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Reverb, ReverbParams::SIZE),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::TIME),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::DAMPING),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::MIX),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::SMEAR),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::RATE),
    ],
};

/// GLYPH: STAFF. a: TRANSPOSE, a note on the staff by semitones.
pub static DEMO_GLYPH_STAFF: BlockDef = BlockDef {
    id: 90,
    name: "Glyph: Staff",
    short: "STF",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Algo, AlgoParams::TRANSPOSE),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// GLYPH: WAVE (SCP: WAV is the WAVES demo). a: operator A's WAVE, a scope
/// tracing its table.
pub static DEMO_GLYPH_WAVE: BlockDef = BlockDef {
    id: 91,
    name: "Glyph: Wave",
    short: "SCP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::AlgoOp(Op::A), AlgoOpParams::WAVE),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

/// The DEMO pages, one SETTINGS leaf each (`leaves::DEMO_LEAVES`).
pub static DEMO_BLOCKS: [ChainBlock; 15] = [
    ChainBlock::page(&DEMO_WAVES),
    ChainBlock::page(&DEMO_SHAPES),
    ChainBlock::page(&DEMO_MOTION),
    ChainBlock::page(&DEMO_FM),
    ChainBlock::page(&DEMO_MATRIX),
    ChainBlock::page(&DEMO_GLYPH_ARC),
    ChainBlock::page(&DEMO_GLYPH_NONE),
    ChainBlock::page(&DEMO_GLYPH_SWITCH),
    ChainBlock::page(&DEMO_GLYPH_LEVEL),
    ChainBlock::page(&DEMO_GLYPH_XF),
    ChainBlock::page(&DEMO_GLYPH_STAFF),
    ChainBlock::page(&DEMO_GLYPH_WAVE),
    ChainBlock::page(&DEMO_GLYPH_BRAID),
    ChainBlock::page(&DEMO_GLYPH_RINGS),
    ChainBlock::page(&DEMO_GLYPH_CUBE),
];

/// The chains besides DEMO's leaves.
const FIXED: [&ChainDef2; 26 + cfg!(feature = "usb-audio") as usize] = [
    &ALGO_CHAIN,
    &ALGO_FORMANT_CHAIN,
    &MODAL_PLUCK_CHAIN,
    &TAPE_CHAIN,
    &VA_CHAIN,
    &DRUM_CHAIN,
    &MIXER_CHANNEL_CHAIN,
    CHANNELS_LEAF.chain(),
    super::settings::leaves::CONTROL_LEAF.chain(),
    SYNC_LEAF.chain(),
    ECHO_LEAF.chain(),
    MPE_LEAF.chain(),
    OUTPUTS_LEAF.chain(),
    #[cfg(feature = "usb-audio")]
    super::settings::leaves::USB_AUDIO_LEAF.chain(),
    TUNING_LEAF.chain(),
    THEME_LEAF.chain(),
    SAVER_LEAF.chain(),
    MAP_LEAF.chain(),
    TEXT_LEAF.chain(),
    FOLLOW_LEAF.chain(),
    PUSH_LEAF.chain(),
    ABOUT_LEAF.chain(),
    AUDIO_LOAD_LEAF.chain(),
    POOL_LEAF.chain(),
    LIVE_SNAP_LEAF.chain(),
    RANDOM_LEAF.chain(),
    &crate::ui::orbit::ORBIT_CHAIN,
];

/// Every chain, for whole-registry checks (unique ids, the focus table):
/// `FIXED`, then DEMO's leaves.
pub static ALL_CHAINS: [&ChainDef2; FIXED.len() + DEMO_LEAVES.len()] = {
    let mut a = [FIXED[0]; FIXED.len() + DEMO_LEAVES.len()];
    let mut i = 0;
    while i < a.len() {
        a[i] = if i < FIXED.len() {
            FIXED[i]
        } else {
            DEMO_LEAVES[i - FIXED.len()].chain()
        };
        i += 1;
    }
    a
};
