// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{ParamId, ParamSpec, find_spec};
use crate::orbit::RingIdx;
use crate::ui::page::{PageLayout, ValFmt};

/// Whose live SMART EQ cuts a page's band shows (ADR 0296).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartScope {
    /// PRT › EQ: the edited Part's four bands.
    Part,
    /// MST › LVL: every Part's, the edited one lit.
    All,
}

/// MST › LVL's cells, A to F (`block_registry::MASTER_LEVEL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LevelCell {
    Vol,
    Pan,
    Spare,
    SmartEq,
    Amount,
    /// The reverb return's SMART EQ role.
    WashRole,
}

impl LevelCell {
    /// Its slot among the page's six.
    pub const fn slot(self) -> usize {
        self as usize
    }
}

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
    /// What SMART EQ cuts now (ADR 0296), of `SmartScope`.
    SmartEq(SmartScope),
    CompressorCurve,
    AudioStats,
    /// PROJECT › POOL: the pool meter; its cells are `pool_page`'s.
    Pool,
    /// ALG A's diagram moving to ALG B's with MORPH, a loop on each
    /// operator that feeds back: ALG's page.
    AlgoDiagram,
    /// SPD: each slot's SPEED and HOLD POSITION.
    EnvSpeed,
    /// ORB: the rings round their circles inside the progression, swept
    /// by the clock.
    Orbit,
    /// RING · RHYTHM: the ring's pattern unrolled, its step lit.
    OrbitRhythm,
    /// RING · VOICE: each hit's gate as a bar, its force as its height.
    OrbitVoice,
    /// RING · NOTES: the pool as chips, or one a step.
    OrbitNotes,
    /// CHRD: the progression's segments as chips over the loop's bars.
    OrbitChords,
    /// CHRD › CUSTOM: the twelve notes round a circle, the set's lit.
    OrbitCircle,
    /// SCN: the eight slots, the playing one outlined, a queued one
    /// filling as the loop runs out.
    OrbitScenes,
    /// ALGO's AR · D1R · D1L · D2R · RR: the focused operator's envelope.
    OpEnv,
    /// OP1–OP6: the operator's envelope, with its wave inset.
    OpPage(Op),
    /// TAPE's SRC and DECK: the source, the machine's part of it, the heads.
    Strip,
    /// L1–L3: one cycle of the slot's LFO, the newest voice's playhead on
    /// it (ADR 0177).
    Lfo(crate::dsp::modulator::LfoSlot),
    /// FLT › FX: `source › FX › FLT` (or the filter first) and the type's
    /// transfer curve.
    FxCurve,
    /// PIT: the live output, the sounding pitch in its corner (ADR 0201).
    Tuner,
    /// DRUM's KIT: the twelve pads, each its machine's icon and choke
    /// group, the selected one framed, a sounding one lit.
    Pads,
}

impl VizType {
    /// The operator whose own page this is the band of.
    pub const fn op_page(self) -> Option<Op> {
        match self {
            VizType::OpPage(op) => Some(op),
            VizType::None
            | VizType::FilterResponse
            | VizType::Adsr
            | VizType::About
            | VizType::EffectsFlow(_)
            | VizType::MixerLevels
            | VizType::SmartEq(_)
            | VizType::CompressorCurve
            | VizType::AudioStats
            | VizType::Pool
            | VizType::AlgoDiagram
            | VizType::EnvSpeed
            | VizType::Orbit
            | VizType::OrbitRhythm
            | VizType::OrbitVoice
            | VizType::OrbitNotes
            | VizType::OrbitChords
            | VizType::OrbitCircle
            | VizType::OrbitScenes
            | VizType::OpEnv
            | VizType::Strip
            | VizType::Lfo(_)
            | VizType::FxCurve
            | VizType::Tuner
            | VizType::Pads => None,
        }
    }
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
    /// DRUM's PAD: the Part's selected pad.
    SelectPad,
    /// Mixer/System/Demo pages, still driven by `PageId`.
    /// Its label is the slot's override; the format sits out of line, so
    /// the slot (one of 600-odd statics) stays small.
    Legacy(&'static ValFmt),
    /// Knob `k` of the Sound's filter type's panel (`filter_panel::knob`):
    /// 0–2 FLT's MODE or VOICE, FREQ and Q, 3–8 FLT › MOD's cells.
    FilterPanel(u8),
    /// Pot `k` (P1–P3) of FLT › FX under the Sound's FX TYPE
    /// (`fx_panel::cells`).
    FxPanel(u8),
    /// Cell k of ENV slot s's page, per its TYPE, MODE and FORM.
    EnvPanel(crate::dsp::modulator::EnvSlot, u8),
    /// Cell k of LFO slot s's page, per its TYPE and FORM.
    LfoPanel(crate::dsp::modulator::LfoSlot, u8),
    /// Cell k of EXC or MDL2, per MODEL (`modal::page_cells`).
    ModalPanel(crate::dsp::modal::ModalPage, u8),
    /// Cell k of operator `op`'s OP page, per its kind (`op_page::page_cells`).
    OpPanel(Op, u8),
    /// A TAPE cell the machine labels (spec § 3, `tape_engine::machine`).
    Machine(MachineSlot),
    /// TAPE's SOURCE: the Sound's `SourceRef`, not a param.
    Source,
    /// Cell k of REV's knobs 1–4 or of REV › MORE, per the reverb's TYPE
    /// (`reverb_panel::knob`).
    ReverbPanel(crate::ui::reverb_panel::RevPage, u8),
    /// A param of the ORBIT ring the location selects.
    SelectedRing(ParamId),
    /// One of ORBIT's own cells.
    Orbit(crate::ui::orbit::OrbitCell),
    /// Knob `k` of the selected pad's machine (`laws::spec`): its label,
    /// short and readout; empty where the machine has none.
    Knob(crate::dsp::drum::types::Knob),
    /// Knob `k` of the selected pad's filter type's panel
    /// (`filter_panel::knob` on its KIND), the pad's param; empty where
    /// the panel's knob is a route or a jack, which a pad lacks.
    PadFilterPanel(u8),
    /// DRUM's ROUTE: the selected pad's route the route cells edit.
    SelectRoute,
    /// A param of the selected pad's selected route (`BlockRef::PadRoute`).
    SelectedRoute(ParamId),
}

/// Where a TAPE cell sits: cell `k` of DECK after MODEL (the glyph's
/// knobs) or of the model page after TAPE, AGE and WOW, each labelled by
/// the Sound's machine (ADR 0178).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineSlot {
    Deck(u8),
    Page(u8),
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSlot {
    pub binding: SlotBinding,
    /// Display label override; `None` shows the spec's label (plan D6).
    /// Format and step always come from the spec. Behind one more
    /// reference: a thin pointer keeps each of the 600-odd slots 12 bytes.
    pub label_override: Option<&'static &'static str>,
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

    pub const fn selected_ring(param: ParamId) -> Self {
        Self {
            binding: SlotBinding::SelectedRing(param),
            label_override: None,
        }
    }

    pub const fn select_op() -> Self {
        Self {
            binding: SlotBinding::SelectOp,
            label_override: None,
        }
    }

    pub const fn select_pad() -> Self {
        Self {
            binding: SlotBinding::SelectPad,
            label_override: None,
        }
    }

    pub const fn select_route() -> Self {
        Self {
            binding: SlotBinding::SelectRoute,
            label_override: None,
        }
    }

    pub const fn selected_route(param: ParamId) -> Self {
        Self {
            binding: SlotBinding::SelectedRoute(param),
            label_override: None,
        }
    }

    pub const fn pad_filter_panel(k: u8) -> Self {
        Self {
            binding: SlotBinding::PadFilterPanel(k),
            label_override: None,
        }
    }

    pub const fn legacy(label: &'static &'static str, fmt: &'static ValFmt) -> Self {
        Self {
            binding: SlotBinding::Legacy(fmt),
            label_override: Some(label),
        }
    }

    pub const fn filter_panel(k: u8) -> Self {
        Self {
            binding: SlotBinding::FilterPanel(k),
            label_override: None,
        }
    }

    pub const fn fx_panel(k: u8) -> Self {
        Self {
            binding: SlotBinding::FxPanel(k),
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

    pub const fn op_panel(op: Op, k: u8) -> Self {
        Self {
            binding: SlotBinding::OpPanel(op, k),
            label_override: None,
        }
    }

    pub const fn machine(at: MachineSlot) -> Self {
        Self {
            binding: SlotBinding::Machine(at),
            label_override: None,
        }
    }

    pub const fn knob(k: crate::dsp::drum::types::Knob) -> Self {
        Self {
            binding: SlotBinding::Knob(k),
            label_override: None,
        }
    }

    pub const fn source() -> Self {
        Self {
            binding: SlotBinding::Source,
            label_override: None,
        }
    }

    pub const fn reverb_panel(page: crate::ui::reverb_panel::RevPage, k: u8) -> Self {
        Self {
            binding: SlotBinding::ReverbPanel(page, k),
            label_override: None,
        }
    }

    pub const fn orbit(cell: crate::ui::orbit::OrbitCell) -> Self {
        Self {
            binding: SlotBinding::Orbit(cell),
            label_override: None,
        }
    }

    pub const fn with_label(self, label: &'static &'static str) -> Self {
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
            SlotBinding::SelectedRing(id) => {
                find_spec(BlockRef::OrbitRing(RingIdx::ALL[0]).specs(), id)
            }
            SlotBinding::SelectedRoute(id) => {
                find_spec(&crate::dsp::drum::params::PAD_ROUTE_SPECS, id)
            }
            // Out of context: a table operator's cell (views resolve the kind).
            SlotBinding::OpPanel(op, k) => {
                let (id, _) = crate::ui::op_page::table_cell(k)?;
                find_spec(BlockRef::AlgoOp(op).specs(), id)
            }
            SlotBinding::Empty
            | SlotBinding::Orbit(_)
            | SlotBinding::SelectOp
            | SlotBinding::SelectPad
            | SlotBinding::SelectRoute
            | SlotBinding::PadFilterPanel(_)
            | SlotBinding::Legacy(_)
            | SlotBinding::FilterPanel(_)
            | SlotBinding::FxPanel(_)
            | SlotBinding::EnvPanel(..)
            | SlotBinding::LfoPanel(..)
            | SlotBinding::ModalPanel(..)
            | SlotBinding::ReverbPanel(..)
            | SlotBinding::Machine(_)
            | SlotBinding::Knob(_)
            | SlotBinding::Source => None,
        }
    }

    pub fn label(&self) -> &'static str {
        if let Some(label) = self.label_override {
            return label;
        }
        match self.binding {
            // Views resolve a panel knob.
            SlotBinding::Empty
            | SlotBinding::Orbit(_)
            | SlotBinding::FilterPanel(_)
            | SlotBinding::PadFilterPanel(_)
            | SlotBinding::FxPanel(_)
            | SlotBinding::EnvPanel(..)
            | SlotBinding::LfoPanel(..)
            | SlotBinding::ModalPanel(..)
            | SlotBinding::ReverbPanel(..)
            | SlotBinding::Legacy(_)
            | SlotBinding::Machine(_)
            | SlotBinding::Knob(_) => "--",
            SlotBinding::Source => "SOURCE",
            SlotBinding::SelectOp => "OP",
            SlotBinding::SelectPad => "PAD",
            SlotBinding::SelectRoute => "ROUTE",
            SlotBinding::OpPanel(_, k) => crate::ui::op_page::table_cell(k).map_or("--", |c| c.1),
            SlotBinding::Param(_)
            | SlotBinding::SelectedOp(_)
            | SlotBinding::SelectedRing(_)
            | SlotBinding::SelectedRoute(_) => self.spec().map_or("??", |s| s.label),
        }
    }

    pub fn format(&self) -> ValFmt {
        match self.binding {
            SlotBinding::Empty
            | SlotBinding::Orbit(_)
            | SlotBinding::FilterPanel(_)
            | SlotBinding::PadFilterPanel(_)
            | SlotBinding::FxPanel(_)
            | SlotBinding::EnvPanel(..)
            | SlotBinding::LfoPanel(..)
            | SlotBinding::ModalPanel(..)
            | SlotBinding::ReverbPanel(..)
            | SlotBinding::Machine(_)
            | SlotBinding::Knob(_)
            | SlotBinding::Source => ValFmt::Uni,
            SlotBinding::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),
            SlotBinding::SelectRoute => crate::ui::view::ROUTE_FMT,
            SlotBinding::SelectPad => ValFmt::Names(&crate::ui::view::PAD_NUMBERS),
            SlotBinding::Legacy(fmt) => *fmt,
            SlotBinding::Param(_)
            | SlotBinding::SelectedOp(_)
            | SlotBinding::SelectedRing(_)
            | SlotBinding::SelectedRoute(_)
            | SlotBinding::OpPanel(..) => self.spec().map_or(ValFmt::Uni, |s| s.fmt),
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

impl BlockDef {
    /// A DRUM pad's page (KIT, KEYS, MCH, MORE, and the pad's FLT, AMP
    /// and MOD pages): its cells edit the selected pad, MIX + MINUS / PLUS
    /// steps it, and the header names it.
    pub fn edits_pad(&self) -> bool {
        self.params.iter().any(|s| match s.binding {
            SlotBinding::SelectPad
            | SlotBinding::Knob(_)
            | SlotBinding::PadFilterPanel(_)
            | SlotBinding::SelectRoute
            | SlotBinding::SelectedRoute(_) => true,
            SlotBinding::Param(a) => a.block.is_pad(),
            _ => false,
        })
    }
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

    /// The page showing the def `id`, if the chain has it.
    pub fn locate(&self, id: u16) -> Option<PageAt> {
        self.blocks.iter().enumerate().find_map(|(node, b)| {
            let sub = if b.def.id == id {
                0
            } else {
                1 + b.sub_pages.iter().position(|d| d.id == id)?
            };
            self.page(node, sub)
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

/// `at` on `from`, kept on `to` by page id. A page gone falls to its
/// node's first; a node gone, to `to`'s home.
pub fn refocus(from: &ChainDef2, at: PageAt, to: &ChainDef2) -> PageAt {
    from.def_at(at)
        .and_then(|d| to.locate(d.id))
        .or_else(|| to.page(at.node as usize, 0))
        .unwrap_or_else(|| to.home())
}
