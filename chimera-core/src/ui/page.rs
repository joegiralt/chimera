use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::pizza::PizzaParams;
use crate::dsp::reverb::ReverbParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams, ParamSnapshot};
use crate::preset::ChainType;
use crate::ui::chain::ChainNav;

pub use crate::block::ValFmt;

/// Layout mode for a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLayout {
    /// One large visualization + 3x2 parameter grid below.
    BigViz,
    /// 3x2 grid of independent cells, each with its own mini icon.
    CellGrid,
    /// Mod matrix grid — source×destination routing table.
    Matrix,
}

/// What mini icon to draw in a cell (CellGrid mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellIcon {
    /// No icon — just label + value + bar.
    None,
    /// Waveform morphing: sine -> clipped (drive)
    WaveClip,
    /// Tone/EQ tilt indicator
    ToneTilt,
    /// Dry/wet blend arc
    DryWet,
    /// Waveform selector: saw / square / tri
    WaveShape,
    /// Pulse width bar
    PulseWidth,
    /// Knob arc indicator
    Arc,
    /// Vertical level bar
    LevelBar,
    /// Pan dot on L-R line
    PanDot,
    /// Fold: sine getting folded
    WaveFold,
    /// Symmetry bias indicator
    Symmetry,
    /// Concentric ripples expanding from center (reverb, decay)
    Ripple,
    /// Rays bursting from center point (excite, attack)
    Burst,
    /// Dot orbiting center (LFO rate, modulation)
    Orbit,
    /// Dots scattering outward from center (diffusion, spread)
    Scatter,
    /// Breathing circle — pulsing size (depth, amount)
    Breathe,
    /// Horizontal lines stacking up (density, voices)
    Stack,
    /// Bouncing ball: compressed at extremes, round at center (bipolar)
    Bounce,
    /// Isometric 3D cube that fills from bottom to top
    Cube,
    /// FM algorithm topology diagram (8 algorithms, val selects which)
    FmAlgorithm,
    /// Feedback: circular arrow that tightens with value
    FeedbackLoop,
    /// Feedback: spiral expanding outward
    FeedbackSpiral,
    /// Feedback: sine getting progressively distorted
    FeedbackWave,
}

/// Identifies which page is active, derived from chain position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    Pizza,
    EngineModal1,
    EngineModal2,
    Drive,
    Filter,
    Folder,
    Vca,
    Efx,
    Mixer,
    Chorus,
    Delay,
    MixReverb,
    Master,
    EnvAmp,
    EnvFilter,
    EnvAux,
    Lfo,
    FmAlg,
    FmOp,
    FmRatio,
    FmEnv1,
    FmEnv2,
    FmEnv3,
    FmEnv4,
    DemoWaves,
    DemoShapes,
    DemoMotion,
    DemoFm,
    DemoMatrix,
}

impl PageId {
    /// Resolve which page is active from current navigation position.
    pub fn from_nav(nav: &ChainNav) -> Self {
        use crate::ui::chain::ChainId;
        match nav.chain_id {
            ChainId::Part(_) => Self::from_part_nav(nav.chain_type, nav.node, nav.sub_page),
            ChainId::Mixer(_) => match nav.node {
                0 => PageId::Mixer,
                1 => PageId::Chorus,
                2 => PageId::Delay,
                3 => PageId::MixReverb,
                _ => PageId::Master,
            },
            ChainId::System => PageId::Pizza, // no param editing yet
            ChainId::Demo => match nav.node {
                0 => PageId::DemoWaves,
                1 => PageId::DemoShapes,
                2 => PageId::DemoMotion,
                3 => PageId::DemoFm,
                _ => PageId::DemoMatrix,
            },
        }
    }

    /// Resolve Part navigation to a PageId based on chain type.
    fn from_part_nav(chain_type: ChainType, node: usize, sub_page: usize) -> Self {
        match chain_type {
            ChainType::PizzaPoly => match node {
                0 => PageId::Pizza,
                1 => PageId::Drive,
                2 => PageId::Filter,
                3 => PageId::Folder,
                4 => match sub_page {
                    0 => PageId::DemoMatrix, // Mod matrix grid
                    1 => PageId::Vca,        // Envelope (ADSR)
                    _ => PageId::Lfo,        // LFO
                },
                _ => PageId::Efx,
            },
            ChainType::Modal => match node {
                0 => match sub_page {
                    0 => PageId::EngineModal1,
                    _ => PageId::EngineModal2,
                },
                1 => PageId::Filter,
                2 => match sub_page {
                    0 => PageId::DemoMatrix,
                    1 => PageId::Vca,
                    _ => PageId::Lfo,
                },
                _ => PageId::Efx,
            },
            ChainType::Fm => match node {
                0 => match sub_page {
                    0 => PageId::FmAlg,
                    1 => PageId::FmOp,
                    _ => PageId::FmRatio,
                },
                1 => PageId::Drive,
                2 => PageId::Filter,
                3 => PageId::Folder,
                4 => match sub_page {
                    0 => PageId::DemoMatrix,
                    1 => PageId::FmEnv1,
                    2 => PageId::FmEnv2,
                    3 => PageId::FmEnv3,
                    _ => PageId::FmEnv4,
                },
                _ => PageId::Efx,
            },
        }
    }

    /// The parameter bound to encoder `idx` on this page, if any.
    /// FM operator pages resolve to the currently selected operator.
    pub fn binding(&self, idx: usize) -> Option<ParamAddr> {
        use BlockRef as B;
        let at = |block: BlockRef, ids: &[ParamId]| ids.get(idx).map(|&param| ParamAddr::new(block, param));
        match self {
            PageId::Pizza => at(B::Pizza, &PIZZA_PAGE),
            PageId::EngineModal1 => at(B::Modal, &MODAL1_PAGE),
            PageId::EngineModal2 => at(B::Modal, &MODAL2_PAGE),
            PageId::Drive => at(B::Drive, &DRIVE_PAGE),
            PageId::Filter => at(B::Filter, &FILTER_PAGE),
            PageId::Folder => at(B::Folder, &FOLDER_PAGE),
            PageId::EnvAmp | PageId::Vca => at(B::AmpEnv, &ENV_PAGE),
            PageId::EnvFilter => at(B::FilterEnv, &ENV_PAGE),
            PageId::EnvAux => at(B::AuxEnv, &ENV_PAGE),
            PageId::Lfo => at(B::Lfo, &LFO_PAGE),
            PageId::Mixer | PageId::Master => at(B::Out, &OUT_PAGE),
            PageId::Chorus => at(B::Chorus, &CHORUS_PAGE),
            PageId::Delay => at(B::Delay, &DELAY_PAGE),
            PageId::Efx | PageId::MixReverb => at(B::Reverb, &REVERB_PAGE),
            PageId::FmAlg => match idx {
                0 => Some(ParamAddr::new(B::Fm, FmParams::ALGORITHM)),
                2 => Some(ParamAddr::new(B::Out, OutParams::VOLUME)),
                _ => None,
            },
            // Slot 0 selects the operator (see `apply_encoder`).
            PageId::FmOp => {
                let id = *FM_OP_PAGE.get(idx.checked_sub(1)?)?;
                Some(ParamAddr::new(B::FmOp(selected_op()), id))
            }
            PageId::FmRatio => match idx {
                0..=3 => Some(ParamAddr::new(B::FmOp(Op::ALL[idx]), FmOpParams::COARSE)),
                4 => Some(ParamAddr::new(B::FmOp(selected_op()), FmOpParams::FINE)),
                _ => None,
            },
            PageId::FmEnv1 => at(B::FmOp(Op::A), &FM_ENV_PAGE),
            PageId::FmEnv2 => at(B::FmOp(Op::B), &FM_ENV_PAGE),
            PageId::FmEnv3 => at(B::FmOp(Op::C), &FM_ENV_PAGE),
            PageId::FmEnv4 => at(B::FmOp(Op::D), &FM_ENV_PAGE),
            PageId::DemoWaves => DEMO_WAVES.get(idx).copied(),
            PageId::DemoShapes => DEMO_SHAPES.get(idx).copied(),
            PageId::DemoMotion => DEMO_MOTION.get(idx).copied(),
            PageId::DemoFm => DEMO_FM.get(idx).copied(),
            PageId::DemoMatrix => None,
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        core::array::from_fn(|i| match (self, i) {
            (PageId::FmOp, 0) => selected_op().index() as f32 / 3.0,
            // Mixer bars for the unbound VOICES and PITCH slots.
            (PageId::Mixer, 2 | 4) => 0.5,
            _ => self.binding(i).map_or(0.0, |a| params.block(a.block).normalized(a.param)),
        })
    }

    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if *self == PageId::FmOp && idx == 0 {
            set_selected_op(selected_op().nudged(delta));
            return;
        }
        if let Some(a) = self.binding(idx) {
            params.block_mut(a.block).nudge(a.param, delta);
        }
    }

    /// Shift+encoder: snap to the coarse points of the bound param's format.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if let Some(a) = self.binding(idx) {
            params.block_mut(a.block).snap(a.param, delta);
        }
    }
}

/// Encoder slot → param id, for pages bound to a single block.
const PIZZA_PAGE: [ParamId; 3] = [PizzaParams::SHAPE, PizzaParams::CRUSH, PizzaParams::LEVEL];
const MODAL1_PAGE: [ParamId; 6] = [
    ModalParams::MODE,
    ModalParams::EXCITE,
    ModalParams::DECAY,
    ModalParams::BRIGHTNESS,
    ModalParams::POSITION,
    ModalParams::INHARM,
];
const MODAL2_PAGE: [ParamId; 6] = [
    ModalParams::KS_BODY,
    ModalParams::KS_STIFFNESS,
    ModalParams::KS_FEEDBACK,
    ModalParams::KS_ENS_DEPTH,
    ModalParams::KS_ENS_RATE,
    ModalParams::KS_ENS_MIX,
];
const DRIVE_PAGE: [ParamId; 3] = [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX];
const FILTER_PAGE: [ParamId; 6] = [
    FilterParams::CUTOFF,
    FilterParams::RESONANCE,
    FilterParams::DRIVE,
    FilterParams::FM_AMOUNT,
    FilterParams::ENV_AMOUNT,
    FilterParams::KEY_TRACK,
];
const FOLDER_PAGE: [ParamId; 3] = [FolderParams::FOLD, FolderParams::SYMMETRY, FolderParams::MIX];
const ENV_PAGE: [ParamId; 6] = [
    EnvParams::ATTACK,
    EnvParams::DECAY,
    EnvParams::SUSTAIN,
    EnvParams::RELEASE,
    EnvParams::LEVEL,
    EnvParams::VEL_SENS,
];
const LFO_PAGE: [ParamId; 6] = [
    LfoParams::RATE,
    LfoParams::SHAPE,
    LfoParams::SYNC,
    LfoParams::PHASE,
    LfoParams::DEPTH,
    LfoParams::OFFSET,
];
/// FM_OP slots 1..=5 (slot 0 selects the operator).
const FM_OP_PAGE: [ParamId; 5] = [
    FmOpParams::WAVEFORM,
    FmOpParams::LEVEL,
    FmOpParams::FEEDBACK,
    FmOpParams::DETUNE,
    FmOpParams::VELOCITY_SENS,
];
const FM_ENV_PAGE: [ParamId; 6] = [
    FmOpParams::ATTACK_RATE,
    FmOpParams::DECAY1_RATE,
    FmOpParams::DECAY1_LEVEL,
    FmOpParams::DECAY2_RATE,
    FmOpParams::RELEASE_RATE,
    FmOpParams::RATE_SCALING,
];
const OUT_PAGE: [ParamId; 2] = [OutParams::VOLUME, OutParams::PAN];
const CHORUS_PAGE: [ParamId; 4] = [
    ChorusParams::MODE,
    ChorusParams::RATE,
    ChorusParams::DEPTH,
    ChorusParams::MIX,
];
const DELAY_PAGE: [ParamId; 6] = [
    DelayParams::TIME_MS,
    DelayParams::FEEDBACK,
    DelayParams::WOW_FLUTTER,
    DelayParams::SATURATION,
    DelayParams::TONE,
    DelayParams::MIX,
];
const REVERB_PAGE: [ParamId; 5] = [
    ReverbParams::REVERB_TYPE,
    ReverbParams::TIME,
    ReverbParams::DAMPING,
    ReverbParams::SIZE,
    ReverbParams::MIX,
];

/// Demo pages borrow params from several blocks (spec §5: `envelopes[1]` is
/// addressed as `FilterEnv`).
const DEMO_WAVES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
    ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
    ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
    ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
    ParamAddr::new(BlockRef::Filter, FilterParams::ENV_AMOUNT),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
];
const DEMO_SHAPES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
    ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
    ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
    ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
    ParamAddr::new(BlockRef::Filter, FilterParams::FM_AMOUNT),
];
const DEMO_MOTION: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::DECAY),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::SUSTAIN),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::RELEASE),
    ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK),
    ParamAddr::new(BlockRef::FilterEnv, EnvParams::DECAY),
];
const DEMO_FM: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Fm, FmParams::ALGORITHM),
    ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK),
];

// ---------------------------------------------------------------------------
// FM operator selection state (module-level, simple static; Task 20 moves it
// into `UiState`)
// ---------------------------------------------------------------------------

/// Currently selected FM operator (index 0-3).
/// This is UI-only state shared between FM_OP and FM_RATIO pages.
static FM_SEL_OP: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The selected FM operator.
pub fn selected_op() -> Op {
    Op::try_from(FM_SEL_OP.load(core::sync::atomic::Ordering::Relaxed)).unwrap_or(Op::A)
}

fn set_selected_op(op: Op) {
    FM_SEL_OP.store(op.index() as u8, core::sync::atomic::Ordering::Relaxed);
}
