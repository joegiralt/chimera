use crate::addr::{BlockRef, Blocks, Op, ParamAddr};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::modulator::EnvSlot;
use crate::params::{DriveParams, EnvParams, FilterParams, FolderParams, OutParams};
use crate::ui::block_def::SlotBinding;
use crate::ui::chain::ChainNav;

pub use crate::block::ValFmt;

/// Layout mode for a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLayout {
    /// One large visualization + 3x2 parameter grid below.
    BigViz,
    /// Focus band, viz band, and a 3x2 grid of independent parameter cells.
    CellGrid,
    /// Mod matrix grid — source×destination routing table.
    Matrix,
}

/// Pages still driven by `PageId`: System and Demo (spec §5). Part- and
/// Mixer-chain pages are identified by `PageKey::Part` and driven by their
/// `BlockDef` slot bindings (`ui::part_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    /// A System chain page with no bound slots, by `BlockDef::id`: the
    /// pages read no values, so only the def tells them apart.
    System(u16),
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
    /// FILTER are shared across chains), with the operator selection so a
    /// selection change redraws the page.
    Part { def: u16, op: Op },
    /// System/Demo pages.
    Legacy(PageId),
}

impl PageKey {
    pub fn from_nav(nav: &ChainNav, sel_op: Op) -> Self {
        match PageId::from_nav(nav) {
            Some(page) => PageKey::Legacy(page),
            None => PageKey::Part {
                def: nav.active_block_def().id,
                op: sel_op,
            },
        }
    }
}

impl PageId {
    /// The legacy page at the current navigation position; `None` on a
    /// slot-bound page: the Part and Mixer chains, and a System page whose
    /// slots are bound (THEME) (see `PageKey::from_nav`).
    pub fn from_nav(nav: &ChainNav) -> Option<Self> {
        use crate::ui::chain::ChainId;
        Some(match nav.chain_id {
            ChainId::Part(_) | ChainId::Mixer(_) => return None,
            ChainId::System
                if nav
                    .active_block_def()
                    .params
                    .iter()
                    .any(|s| matches!(s.binding, SlotBinding::Param(_))) =>
            {
                return None;
            }
            ChainId::System => PageId::System(nav.active_block_def().id),
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
        match self {
            PageId::DemoWaves => DEMO_WAVES.get(idx).copied(),
            PageId::DemoShapes => DEMO_SHAPES.get(idx).copied(),
            PageId::DemoMotion => DEMO_MOTION.get(idx).copied(),
            PageId::DemoFm => DEMO_FM.get(idx).copied(),
            PageId::DemoMatrix | PageId::System(_) => None,
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

/// Demo pages borrow params from several blocks (spec §5: `envelopes[1]` is
/// addressed as `Env(Env2)`).
const DEMO_WAVES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
    ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
    ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
    ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
    ParamAddr::new(BlockRef::Folder, FolderParams::MIX),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
];
const DEMO_SHAPES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
    ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
    ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
    ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
    ParamAddr::new(BlockRef::Drive, DriveParams::MIX),
];
const DEMO_MOTION: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::ATTACK),
    ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::DECAY),
    ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::SUSTAIN),
    ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::RELEASE),
    ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::ATTACK),
    ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::DECAY),
];
const DEMO_FM: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Algo, AlgoParams::ALG_A),
    ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::AlgoOp(Op::C), AlgoOpParams::FEEDBACK),
];
