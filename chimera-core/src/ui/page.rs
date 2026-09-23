use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::reverb::ReverbParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams, ParamSnapshot};
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

/// Pages still driven by `PageId`: Mixer, System and Demo (spec §5).
/// Part-chain pages are identified by `PageKey::Part` and driven by their
/// `BlockDef` slot bindings (`ui::part_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    Mixer,
    Chorus,
    Delay,
    MixReverb,
    Master,
    /// Standalone envelope pages (not reachable from any chain today).
    EnvAmp,
    EnvFilter,
    EnvAux,
    /// System chain: no editable params yet.
    System,
    DemoWaves,
    DemoShapes,
    DemoMotion,
    DemoFm,
    DemoMatrix,
}

/// Page identity for the renderer and dirty-region tracking (spec §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKey {
    /// A Part-chain page by `BlockDef::id` (defs like FILTER are shared
    /// across chains), with the FM operator selection so a selection change
    /// redraws the page.
    Part { def: u16, op: Op },
    /// Mixer/System/Demo pages.
    Legacy(PageId),
}

impl PageKey {
    pub fn from_nav(nav: &ChainNav, sel_op: Op) -> Self {
        match PageId::from_nav(nav) {
            Some(page) => PageKey::Legacy(page),
            None => PageKey::Part { def: nav.active_block_def().id, op: sel_op },
        }
    }
}

impl PageId {
    /// The legacy page at the current navigation position; `None` on a
    /// Part chain (see `PageKey::from_nav`).
    pub fn from_nav(nav: &ChainNav) -> Option<Self> {
        use crate::ui::chain::ChainId;
        Some(match nav.chain_id {
            ChainId::Part(_) => return None,
            ChainId::Mixer(_) => match nav.node {
                0 => PageId::Mixer,
                1 => PageId::Chorus,
                2 => PageId::Delay,
                3 => PageId::MixReverb,
                _ => PageId::Master,
            },
            ChainId::System => PageId::System,
            ChainId::Demo => match nav.node {
                0 => PageId::DemoWaves,
                1 => PageId::DemoShapes,
                2 => PageId::DemoMotion,
                3 => PageId::DemoFm,
                _ => PageId::DemoMatrix,
            },
        })
    }

    /// The parameter bound to encoder `idx` on this page, if any.
    pub fn binding(&self, idx: usize) -> Option<ParamAddr> {
        use BlockRef as B;
        let at = |block: BlockRef, ids: &[ParamId]| ids.get(idx).map(|&param| ParamAddr::new(block, param));
        match self {
            PageId::EnvAmp => at(B::AmpEnv, &ENV_PAGE),
            PageId::EnvFilter => at(B::FilterEnv, &ENV_PAGE),
            PageId::EnvAux => at(B::AuxEnv, &ENV_PAGE),
            PageId::Mixer | PageId::Master => at(B::Out, &OUT_PAGE),
            PageId::Chorus => at(B::Chorus, &CHORUS_PAGE),
            PageId::Delay => at(B::Delay, &DELAY_PAGE),
            PageId::MixReverb => at(B::Reverb, &REVERB_PAGE),
            PageId::DemoWaves => DEMO_WAVES.get(idx).copied(),
            PageId::DemoShapes => DEMO_SHAPES.get(idx).copied(),
            PageId::DemoMotion => DEMO_MOTION.get(idx).copied(),
            PageId::DemoFm => DEMO_FM.get(idx).copied(),
            PageId::DemoMatrix | PageId::System => None,
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        core::array::from_fn(|i| match (self, i) {
            // Mixer bars for the unbound VOICES and PITCH slots.
            (PageId::Mixer, 2 | 4) => 0.5,
            _ => self.binding(i).map_or(0.0, |a| params.block(a.block).normalized(a.param)),
        })
    }

    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
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
const ENV_PAGE: [ParamId; 6] = [
    EnvParams::ATTACK,
    EnvParams::DECAY,
    EnvParams::SUSTAIN,
    EnvParams::RELEASE,
    EnvParams::LEVEL,
    EnvParams::VEL_SENS,
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
