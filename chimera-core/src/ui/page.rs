use crate::addr::{BlockRead, BlockRef, Blocks, Op, ParamAddr};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::modulator::EnvSlot;
use crate::params::{DriveParams, EnvParams, FilterParams, FolderParams, OutParams};
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::block_registry::{self as reg, DEMO_BLOCKS};
use crate::ui::nav::Location;

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

/// Pages still driven by `PageId`: SETTINGS leaves and DEMO pages whose
/// slots bind nothing (spec §5). Every other page is a `PageKey::Part`,
/// driven by its `BlockDef` slot bindings (`ui::part_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    /// A leaf page with no bound slots, by `BlockDef::id`: the pages read
    /// no values, so only the def tells them apart.
    System(u16),
    /// A DEMO page, by `BlockDef::id`.
    Demo(u16),
}

/// Page identity for the renderer and dirty-region tracking (spec §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKey {
    /// A slot-bound page by `BlockDef::id` (defs like FILTER are shared
    /// across chains), with the operator selection so a selection change
    /// redraws the page.
    Part { def: u16, op: Op },
    /// Leaf and DEMO pages with no bound slots.
    Legacy(PageId),
}

impl PageKey {
    /// `def` is the page shown at `at`.
    pub fn from_location(at: Location, def: &BlockDef, sel_op: Op) -> Self {
        match PageId::from_location(at, def) {
            Some(page) => PageKey::Legacy(page),
            None => PageKey::Part {
                def: def.id,
                op: sel_op,
            },
        }
    }
}

impl PageId {
    /// The legacy page `def` at `at`; `None` off SETTINGS and on a page
    /// whose slots are bound (THEME, CHANNELS, the glyph pages).
    pub fn from_location(at: Location, def: &BlockDef) -> Option<Self> {
        at.settings()?.at_leaf()?;
        Self::of_leaf(def)
    }

    /// The legacy page leaf page `def` is; `None` when its slots are bound.
    pub fn of_leaf(def: &BlockDef) -> Option<Self> {
        if def
            .params
            .iter()
            .any(|s| !matches!(s.binding, SlotBinding::Legacy { .. } | SlotBinding::Empty))
        {
            return None;
        }
        Some(if DEMO_BLOCKS.iter().any(|b| core::ptr::eq(b.def, def)) {
            PageId::Demo(def.id)
        } else {
            PageId::System(def.id)
        })
    }

    /// The parameter bound to encoder `idx` on this page, if any.
    pub fn binding(&self, idx: usize) -> Option<ParamAddr> {
        let table: &[ParamAddr] = match *self {
            PageId::Demo(id) if id == reg::DEMO_WAVES.id => &DEMO_WAVES,
            PageId::Demo(id) if id == reg::DEMO_SHAPES.id => &DEMO_SHAPES,
            PageId::Demo(id) if id == reg::DEMO_MOTION.id => &DEMO_MOTION,
            PageId::Demo(id) if id == reg::DEMO_FM.id => &DEMO_FM,
            _ => &[],
        };
        table.get(idx).copied()
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &impl BlockRead) -> [f32; 6] {
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
