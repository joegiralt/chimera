use crate::addr::{BlockRef, Blocks, Op, ParamAddr};
use crate::block::ParamId;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams};
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

/// Pages still driven by `PageId`: System and Demo (spec §5). Part- and
/// Mixer-chain pages are identified by `PageKey::Part` and driven by their
/// `BlockDef` slot bindings (`ui::part_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
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
    /// A slot-bound page (Part or Mixer chain) by `BlockDef::id` (defs like
    /// FILTER are shared across chains), with the FM operator selection so a
    /// selection change redraws the page.
    Part { def: u16, op: Op },
    /// System/Demo pages.
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
    /// slot-bound Part or Mixer chain (see `PageKey::from_nav`).
    pub fn from_nav(nav: &ChainNav) -> Option<Self> {
        use crate::ui::chain::ChainId;
        Some(match nav.chain_id {
            ChainId::Part(_) | ChainId::Mixer(_) => return None,
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
            PageId::DemoWaves => DEMO_WAVES.get(idx).copied(),
            PageId::DemoShapes => DEMO_SHAPES.get(idx).copied(),
            PageId::DemoMotion => DEMO_MOTION.get(idx).copied(),
            PageId::DemoFm => DEMO_FM.get(idx).copied(),
            PageId::DemoMatrix | PageId::System => None,
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &impl Blocks) -> [f32; 6] {
        core::array::from_fn(|i| {
            self.binding(i)
                .and_then(|a| Some(params.block(a.block)?.normalized(a.param)))
                .unwrap_or(0.0)
        })
    }

    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut impl Blocks) {
        if let Some(a) = self.binding(idx)
            && let Some(b) = params.block_mut(a.block)
        {
            b.nudge(a.param, delta);
        }
    }

    /// Shift+encoder: snap to the coarse points of the bound param's format.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut impl Blocks) {
        if let Some(a) = self.binding(idx)
            && let Some(b) = params.block_mut(a.block)
        {
            b.snap(a.param, delta);
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
