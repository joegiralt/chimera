use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{ParamId, ParamSpec, find_spec};
use crate::ui::page::{PageLayout, ValFmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizType {
    None,
    FilterResponse,
    Adsr,
    AlgorithmDiagram,
    LpgResponse,
    Logo,
    EffectsFlow,
    MixerLevels,
    CompressorCurve,
    /// TX81Z 5-stage envelope: AR → D1R → D1L → D2R → RR
    FmEnvelope,
    AudioStats,
}

/// What an encoder slot edits (spec §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotBinding {
    Empty,
    /// A fixed param, e.g. Filter cutoff or `FmOp(A)` coarse.
    Param(ParamAddr),
    /// A param of the currently selected FM operator.
    SelectedOp(ParamId),
    /// The FM operator selector itself.
    SelectOp,
    /// Mixer/System/Demo pages, still driven by `PageId`.
    Legacy {
        label: &'static str,
        fmt: ValFmt,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSlot {
    pub binding: SlotBinding,
    /// Display label override; `None` shows the spec's label (plan D6).
    /// Format and step always come from the spec.
    pub label_override: Option<&'static str>,
}

impl ParamSlot {
    pub const EMPTY: ParamSlot = ParamSlot {
        binding: SlotBinding::Empty,
        label_override: None,
    };

    pub const fn param(block: BlockRef, param: ParamId) -> Self {
        Self {
            binding: SlotBinding::Param(ParamAddr::new(block, param)),
            label_override: None,
        }
    }

    pub const fn selected_op(param: ParamId) -> Self {
        Self {
            binding: SlotBinding::SelectedOp(param),
            label_override: None,
        }
    }

    pub const fn select_op() -> Self {
        Self {
            binding: SlotBinding::SelectOp,
            label_override: None,
        }
    }

    pub const fn legacy(label: &'static str, fmt: ValFmt) -> Self {
        Self {
            binding: SlotBinding::Legacy { label, fmt },
            label_override: None,
        }
    }

    pub const fn with_label(self, label: &'static str) -> Self {
        Self {
            label_override: Some(label),
            ..self
        }
    }

    /// The spec this slot edits (bound slots only).
    pub fn spec(&self) -> Option<&'static ParamSpec> {
        match self.binding {
            SlotBinding::Param(a) => a.spec(),
            // All four FM operators share one spec table, so FmOp(Op::A) stands in.
            SlotBinding::SelectedOp(id) => find_spec(BlockRef::FmOp(Op::A).specs(), id),
            SlotBinding::Empty | SlotBinding::SelectOp | SlotBinding::Legacy { .. } => None,
        }
    }

    pub fn label(&self) -> &'static str {
        if let Some(label) = self.label_override {
            return label;
        }
        match self.binding {
            SlotBinding::Empty => "--",
            SlotBinding::SelectOp => "OP",
            SlotBinding::Legacy { label, .. } => label,
            SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                self.spec().map_or("??", |s| s.label)
            }
        }
    }

    pub fn format(&self) -> ValFmt {
        match self.binding {
            SlotBinding::Empty => ValFmt::Uni,
            SlotBinding::SelectOp => ValFmt::OneBased(3),
            SlotBinding::Legacy { fmt, .. } => fmt,
            SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                self.spec().map_or(ValFmt::Uni, |s| s.fmt)
            }
        }
    }
}

/// The address slot `slot` of `def` edits. `SelectedOp` resolves to the
/// operator selected *now*, so a saved route always names a concrete operator.
pub fn slot_addr(def: &BlockDef, slot: usize, sel_op: Op) -> Option<ParamAddr> {
    match def.params.get(slot)?.binding {
        SlotBinding::Param(a) => Some(a),
        SlotBinding::SelectedOp(id) => Some(ParamAddr::new(BlockRef::FmOp(sel_op), id)),
        SlotBinding::Empty | SlotBinding::SelectOp | SlotBinding::Legacy { .. } => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BlockDef {
    /// Unique page identity (`PageKey`); defs like FILTER are shared across chains.
    pub id: u16,
    pub name: &'static str,
    pub short: &'static str,
    pub layout: PageLayout,
    pub viz: VizType,
    pub params: [ParamSlot; 6],
}

#[derive(Clone, Copy, Debug)]
pub struct ChainBlock {
    pub def: &'static BlockDef,
    pub sub_pages: &'static [&'static BlockDef],
}

impl ChainBlock {
    pub fn active_def(&self, sub_page: usize) -> &'static BlockDef {
        if self.sub_pages.is_empty() || sub_page == 0 {
            self.def
        } else {
            self.sub_pages
                .get(sub_page - 1)
                .copied()
                .unwrap_or(self.def)
        }
    }

    pub fn sub_page_count(&self) -> usize {
        if self.sub_pages.is_empty() {
            0
        } else {
            1 + self.sub_pages.len()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ChainDef2 {
    pub name: &'static str,
    pub blocks: &'static [ChainBlock],
    /// Mod matrix source rows, in `Voice`'s source order (spec §4).
    pub mod_sources: &'static [&'static str],
}

impl ChainDef2 {
    pub fn block_at(&self, node: usize) -> Option<&'static ChainBlock> {
        self.blocks.get(node)
    }

    pub fn active_def(&self, node: usize, sub_page: usize) -> Option<&'static BlockDef> {
        self.block_at(node).map(|b| b.active_def(sub_page))
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}
