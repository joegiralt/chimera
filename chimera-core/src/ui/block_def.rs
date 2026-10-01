use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{ParamId, ParamSpec, find_spec};
use crate::ui::nav::PageAt;
use crate::ui::page::{PageLayout, ValFmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizType {
    None,
    FilterResponse,
    Adsr,
    Logo,
    /// IN → CHR → DLY → REV → OUT, lighting this page's part of it.
    EffectsFlow(FxFlow),
    MixerLevels,
    CompressorCurve,
    AudioStats,
    /// ALG A's diagram moving to ALG B's with MORPH.
    AlgoDiagram,
    /// SPD: each slot's SPEED and HOLD POSITION.
    EnvSpeed,
}

/// A node of the FX flow; the value is its index in `viz::effects_flow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxNode {
    Chorus = 0,
    Delay = 1,
    Reverb = 2,
}

/// What an FX page's flow lights: its effect, or on SENDS the focused
/// send, with every send's level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxFlow {
    Effect(FxNode),
    Sends,
}

/// What an encoder slot edits (spec §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotBinding {
    Empty,
    /// A fixed param, e.g. Filter cutoff or `AlgoOp(A)` level.
    Param(ParamAddr),
    /// A param of the currently selected operator.
    SelectedOp(ParamId),
    /// The operator selector itself.
    SelectOp,
    /// Mixer/System/Demo pages, still driven by `PageId`.
    Legacy {
        label: &'static str,
        fmt: ValFmt,
    },
    /// Knob `k` of the Sound's filter KIND's panel (spec § 6): 0–4 FLT's
    /// knobs 2–6, 5–6 FLT › MODE's extras.
    FilterPanel(u8),
    /// Cell k of ENV slot s's page, per its TYPE, MODE and FORM.
    EnvPanel(crate::dsp::modulator::EnvSlot, u8),
    /// Cell k of LFO slot s's page, per its TYPE and FORM.
    LfoPanel(crate::dsp::modulator::LfoSlot, u8),
    /// Cell k of EXC or MDL2, per MODEL (`modal::page_cells`).
    ModalPanel(crate::dsp::modal::ModalPage, u8),
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

    pub const fn filter_panel(k: u8) -> Self {
        Self {
            binding: SlotBinding::FilterPanel(k),
            label_override: None,
        }
    }

    pub const fn env_panel(s: crate::dsp::modulator::EnvSlot, k: u8) -> Self {
        Self {
            binding: SlotBinding::EnvPanel(s, k),
            label_override: None,
        }
    }

    pub const fn lfo_panel(s: crate::dsp::modulator::LfoSlot, k: u8) -> Self {
        Self {
            binding: SlotBinding::LfoPanel(s, k),
            label_override: None,
        }
    }

    pub const fn modal_panel(page: crate::dsp::modal::ModalPage, k: u8) -> Self {
        Self {
            binding: SlotBinding::ModalPanel(page, k),
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
            // All six operators share one spec table, so AlgoOp(Op::A) stands in.
            SlotBinding::SelectedOp(id) => find_spec(BlockRef::AlgoOp(Op::A).specs(), id),
            SlotBinding::Empty
            | SlotBinding::SelectOp
            | SlotBinding::Legacy { .. }
            | SlotBinding::FilterPanel(_)
            | SlotBinding::EnvPanel(..)
            | SlotBinding::LfoPanel(..)
            | SlotBinding::ModalPanel(..) => None,
        }
    }

    pub fn label(&self) -> &'static str {
        if let Some(label) = self.label_override {
            return label;
        }
        match self.binding {
            // Views resolve a panel knob.
            SlotBinding::Empty
            | SlotBinding::FilterPanel(_)
            | SlotBinding::EnvPanel(..)
            | SlotBinding::LfoPanel(..)
            | SlotBinding::ModalPanel(..) => "--",
            SlotBinding::SelectOp => "OP",
            SlotBinding::Legacy { label, .. } => label,
            SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                self.spec().map_or("??", |s| s.label)
            }
        }
    }

    pub fn format(&self) -> ValFmt {
        match self.binding {
            SlotBinding::Empty
            | SlotBinding::FilterPanel(_)
            | SlotBinding::EnvPanel(..)
            | SlotBinding::LfoPanel(..)
            | SlotBinding::ModalPanel(..) => ValFmt::Uni,
            SlotBinding::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),
            SlotBinding::Legacy { fmt, .. } => fmt,
            SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                self.spec().map_or(ValFmt::Uni, |s| s.fmt)
            }
        }
    }
}

/// The address slot `slot` of `def` edits under `ctx`: operator slots name
/// the selected operator, panel knobs the Sound's kind's parameter.
pub fn slot_addr(def: &BlockDef, slot: usize, ctx: &crate::ui::view::SlotCtx) -> Option<ParamAddr> {
    crate::ui::view::view(def, slot, ctx).addr()
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
    /// The map's label for this node when it isn't the home page's short
    /// (the MOD node's home is MTX).
    pub map: Option<&'static str>,
}

impl ChainBlock {
    /// A page with no sub-pages.
    pub const fn page(def: &'static BlockDef) -> Self {
        Self {
            def,
            sub_pages: &[],
            map: None,
        }
    }

    pub const fn with_subs(
        def: &'static BlockDef,
        sub_pages: &'static [&'static BlockDef],
    ) -> Self {
        Self {
            def,
            sub_pages,
            map: None,
        }
    }

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
    /// Where a first visit lands (ADR 0066); `ui::nav` builds and reads
    /// it (`ChainDef2::new`, `home`).
    pub(crate) home: PageAt,
}

impl ChainDef2 {
    /// The engine's node: the one PIT hangs under (ADR 0042); the first
    /// on a chain without it.
    pub fn engine_node(&self) -> usize {
        let pitch = crate::ui::block_registry::PITCH.id;
        self.blocks
            .iter()
            .position(|b| b.sub_pages.iter().any(|d| d.id == pitch))
            .unwrap_or(0)
    }

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
