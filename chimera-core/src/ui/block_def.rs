use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{ParamId, ParamSpec, find_spec};
use crate::ui::page::{PageLayout, ValFmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizType {
    None,
    FilterResponse,
    Adsr,
    /// SYSTEM › ABOUT: the name and firmware; its cells are `about_page`'s.
    About,
    /// IN → CHR → DLY → REV → OUT, lighting this page's part of it.
    EffectsFlow(FxFlow),
    MixerLevels,
    CompressorCurve,
    AudioStats,
    /// ALG A's diagram moving to ALG B's with MORPH.
    AlgoDiagram,
    /// SPD: each slot's SPEED and HOLD POSITION.
    EnvSpeed,
    /// ALGO's AR · D1R · D1L · D2R · RR: the focused operator's envelope.
    OpEnv,
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

    pub const fn sub_page_count(&self) -> usize {
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
    /// Where a first visit lands (ADR 0066): only `new` and
    /// `with_home_def` set it, so the build checks it.
    home: PageAt,
}

/// A page on a chain. Only a chain makes one (`ChainDef2::home`, `page`,
/// `step`), so no code can land on a node it assumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageAt {
    node: u8,
    sub: u8,
}

impl PageAt {
    pub const fn node(self) -> u8 {
        self.node
    }

    pub const fn sub(self) -> u8 {
        self.sub
    }

    /// Any page, unchecked: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn of(node: u8, sub: u8) -> Self {
        PageAt { node, sub }
    }
}

/// A key on a chain's pages: PLUS, MINUS, EDIT (sub-page down), SEQ (up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Next,
    Prev,
    Down,
    Up,
}

impl ChainDef2 {
    /// A new chain's home: its first node.
    const FIRST: PageAt = PageAt { node: 0, sub: 0 };

    /// A chain whose home is its first node. An empty chain fails the
    /// build: every chain is a static. Nothing else makes one, so the home
    /// can't skip the checks:
    ///
    /// ```compile_fail,E0451
    /// use chimera_core::ui::block_def::ChainDef2;
    /// use chimera_core::ui::block_registry::ALGO_CHAIN;
    /// static BAD: ChainDef2 = ChainDef2 { blocks: &[], ..ALGO_CHAIN };
    /// ```
    ///
    /// ```compile_fail,E0451
    /// use chimera_core::ui::block_def::PageAt;
    /// let home = PageAt { node: 1, sub: 0 };
    /// ```
    pub const fn new(
        name: &'static str,
        blocks: &'static [ChainBlock],
        mod_sources: &'static [&'static str],
    ) -> Self {
        assert!(!blocks.is_empty(), "a chain has a page");
        assert!(blocks.len() <= u8::MAX as usize);
        ChainDef2 {
            name,
            blocks,
            mod_sources,
            home: Self::FIRST,
        }
    }

    /// Home on `def`'s node instead; a def not on the chain fails the build.
    pub const fn with_home_def(self, def: &BlockDef) -> Self {
        let mut i = 0;
        while i < self.blocks.len() {
            if self.blocks[i].def.id == def.id {
                return ChainDef2 {
                    home: PageAt {
                        node: i as u8,
                        sub: 0,
                    },
                    ..self
                };
            }
            i += 1;
        }
        panic!("home def not on the chain")
    }

    /// Where a first visit lands (ADR 0066).
    pub const fn home(&self) -> PageAt {
        self.home
    }

    /// `node`'s sub-page `sub`, if the chain has it.
    pub const fn page(&self, node: usize, sub: usize) -> Option<PageAt> {
        if node >= self.blocks.len() {
            return None;
        }
        let subs = self.blocks[node].sub_page_count();
        if sub >= if subs == 0 { 1 } else { subs } {
            return None;
        }
        Some(PageAt {
            node: node as u8,
            sub: sub as u8,
        })
    }

    /// The def at `at`, if `at` is on this chain.
    pub fn def_at(&self, at: PageAt) -> Option<&'static BlockDef> {
        self.active_def(at.node as usize, at.sub as usize)
    }

    /// The page `m` moves to from `at`: the next or previous node,
    /// clamped, on its own page; a sub-page down or up.
    pub fn step(&self, at: PageAt, m: Move) -> Option<PageAt> {
        let (node, sub) = (at.node as usize, at.sub as usize);
        match m {
            Move::Next => self.page(node + 1, 0),
            Move::Prev => self.page(node.checked_sub(1)?, 0),
            Move::Down => self.page(node, sub + 1),
            Move::Up => self.page(node, sub.checked_sub(1)?),
        }
    }

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
