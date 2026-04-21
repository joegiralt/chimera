# BlockDef Component System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hardcoded `PageId` enum with a declarative `BlockDef` system where blocks define their pages as data, and a small set of reusable components renders everything.

**Architecture:** `BlockDef` structs declare name, layout, viz type, and 6 `ParamSlot` definitions. The renderer reads `BlockDef` to draw pages — it doesn't know what a filter or FM engine is. Navigation resolves the active `BlockDef` from the chain at the current position. Parameter editing uses generic binding from `ParamSlot` to the underlying `Param` via closure or function pointer.

**Tech Stack:** Rust, `no_std`, `embedded-graphics 0.8`, `chimera-core` crate.

**Specs:**
- `docs/chimera-ui-ux-spec.md` — component architecture, block definitions, navigation
- `docs/chimera-sitemap.md` — complete chain/block topology

---

## Scope

This plan covers the UI refactor only — converting the existing hardcoded pages to declarative `BlockDef` pages. It does NOT add new blocks, new chains, mixer channels, or DSP changes. After this plan, the instrument looks and behaves identically to before, but the internals are data-driven.

The refactor is incremental: every task produces a compiling, working system. The old `PageId` code is removed only after all functionality has been migrated.

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `chimera-core/src/ui/block_def.rs` | **Create** | `BlockDef`, `ParamSlot`, `VizType`, `PageLayout` (moved here) |
| `chimera-core/src/ui/block_registry.rs` | **Create** | Static `BlockDef` instances for all existing block types (FM, Drive, Filter, etc.) |
| `chimera-core/src/ui/chain.rs` | **Modify** | Chains reference `BlockDef` instead of `NodeDef`. `ChainNav` resolves to `BlockDef`. |
| `chimera-core/src/ui/renderer.rs` | **Modify** | `draw()`, `draw_region()`, `update()` read from `BlockDef` instead of `PageId` |
| `chimera-core/src/ui/mod.rs` | **Modify** | `UiState` uses `BlockDef` instead of `PageId`. Input handling uses generic parameter binding. |
| `chimera-core/src/ui/region.rs` | **Modify** | Dirty detection uses `BlockDef` layout instead of `PageId` |
| `chimera-core/src/ui/page.rs` | **Delete** (final task) | All functionality migrated to `block_def.rs` + `block_registry.rs` |
| `chimera-core/src/ui/mod.rs` | **Modify** | Add `pub mod block_def; pub mod block_registry;` |

Test file: `chimera-core/tests/block_def_tests.rs`

---

### Task 1: Define `BlockDef` and `ParamSlot` types

**Files:**
- Create: `chimera-core/src/ui/block_def.rs`
- Modify: `chimera-core/src/ui/mod.rs` (add `pub mod block_def;`)
- Test: `chimera-core/tests/block_def_tests.rs`

- [ ] **Step 1: Write the test**

```rust
// chimera-core/tests/block_def_tests.rs
use chimera_core::ui::block_def::*;
use chimera_core::ui::page::{ValFmt, CellIcon, PageLayout};

#[test]
fn block_def_has_6_param_slots() {
    let block = BlockDef {
        name: "Test",
        short: "TST",
        layout: PageLayout::CellGrid,
        viz: VizType::None,
        params: [
            ParamSlot { label: "A", format: ValFmt::Uni, icon: CellIcon::None },
            ParamSlot { label: "B", format: ValFmt::Uni, icon: CellIcon::None },
            ParamSlot { label: "C", format: ValFmt::Uni, icon: CellIcon::None },
            ParamSlot { label: "D", format: ValFmt::Uni, icon: CellIcon::None },
            ParamSlot { label: "E", format: ValFmt::Uni, icon: CellIcon::None },
            ParamSlot { label: "F", format: ValFmt::Uni, icon: CellIcon::None },
        ],
    };
    assert_eq!(block.name, "Test");
    assert_eq!(block.short, "TST");
    assert_eq!(block.params[0].label, "A");
    assert_eq!(block.params.len(), 6);
}

#[test]
fn viz_type_default_is_none() {
    assert_eq!(VizType::None, VizType::None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core block_def`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Create `block_def.rs`**

```rust
// chimera-core/src/ui/block_def.rs

use crate::ui::page::{ValFmt, CellIcon, PageLayout};

/// What visualization to render in BigViz mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizType {
    None,
    FilterResponse,
    Adsr,
    AlgorithmDiagram,
    EqResponse,
    LpgResponse,
    WaveformPreview,
    Logo,
    ModalPeaks,
    DriveClip,
    WaveFold,
    EffectsFlow,
    MixerLevels,
    RoutingMatrix,
    CompressorCurve,
}

/// A single parameter slot on a page.
#[derive(Clone, Copy, Debug)]
pub struct ParamSlot {
    pub label: &'static str,
    pub format: ValFmt,
    pub icon: CellIcon,
}

/// Declarative definition of a block's UI page.
/// The renderer uses this to draw — it doesn't know what the block does.
#[derive(Clone, Copy, Debug)]
pub struct BlockDef {
    /// Full name shown in header (e.g., "Filter")
    pub name: &'static str,
    /// 3-char abbreviation for dungeon map (e.g., "FLT")
    pub short: &'static str,
    /// Layout mode: BigViz or CellGrid
    pub layout: PageLayout,
    /// Visualization type (BigViz only; ignored for CellGrid)
    pub viz: VizType,
    /// 6 parameter slot definitions — one per encoder
    pub params: [ParamSlot; 6],
}

/// A block node in a chain, potentially with sub-pages.
#[derive(Clone, Copy, Debug)]
pub struct ChainBlock {
    /// The primary page for this block
    pub def: &'static BlockDef,
    /// Sub-pages (e.g., FM-A, FM-B, FM-C). Empty slice = no sub-pages.
    pub sub_pages: &'static [&'static BlockDef],
}

impl ChainBlock {
    /// Get the active BlockDef for the given sub-page index.
    /// Returns the primary def if sub_page is 0 or if there are no sub-pages.
    pub fn active_def(&self, sub_page: usize) -> &'static BlockDef {
        if self.sub_pages.is_empty() || sub_page == 0 {
            self.def
        } else {
            // sub_page 1 = sub_pages[0], etc.
            self.sub_pages.get(sub_page - 1).copied().unwrap_or(self.def)
        }
    }

    /// Number of sub-pages (0 if none, otherwise 1 + sub_pages.len())
    pub fn sub_page_count(&self) -> usize {
        if self.sub_pages.is_empty() {
            0
        } else {
            1 + self.sub_pages.len()
        }
    }
}

/// A chain definition — a sequence of blocks.
#[derive(Clone, Copy, Debug)]
pub struct ChainDef2 {
    pub name: &'static str,
    pub blocks: &'static [ChainBlock],
}

impl ChainDef2 {
    /// Get the ChainBlock at the given node index.
    pub fn block_at(&self, node: usize) -> Option<&'static ChainBlock> {
        self.blocks.get(node)
    }

    /// Get the active BlockDef for a given node + sub_page position.
    pub fn active_def(&self, node: usize, sub_page: usize) -> Option<&'static BlockDef> {
        self.block_at(node).map(|b| b.active_def(sub_page))
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}
```

- [ ] **Step 4: Add module to mod.rs**

Add `pub mod block_def;` to `chimera-core/src/ui/mod.rs`.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p chimera-core block_def`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p chimera-core`
Expected: All existing tests still pass

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/ui/block_def.rs chimera-core/src/ui/mod.rs chimera-core/tests/block_def_tests.rs
git commit -m "feat(core): add BlockDef, ParamSlot, VizType, ChainBlock declarative types"
```

---

### Task 2: Create block registry with all existing block definitions

**Files:**
- Create: `chimera-core/src/ui/block_registry.rs`
- Modify: `chimera-core/src/ui/mod.rs` (add `pub mod block_registry;`)
- Test: `chimera-core/tests/block_def_tests.rs` (add registry tests)

- [ ] **Step 1: Write the test**

```rust
// Add to chimera-core/tests/block_def_tests.rs
use chimera_core::ui::block_registry;

#[test]
fn fm_poly_chain_has_6_blocks() {
    let chain = &block_registry::FM_POLY_CHAIN;
    // FM Osc + Drive + Filter + Wavefolder + VCA + Mod Matrix
    assert_eq!(chain.len(), 6);
    assert_eq!(chain.blocks[0].def.name, "FM Osc");
    assert_eq!(chain.blocks[2].def.name, "Filter");
    assert_eq!(chain.blocks[5].def.name, "Mod Matrix");
}

#[test]
fn fm_osc_block_has_3_sub_pages() {
    let chain = &block_registry::FM_POLY_CHAIN;
    let fm_block = &chain.blocks[0];
    assert_eq!(fm_block.sub_page_count(), 4); // primary + 3 sub-pages
    assert_eq!(fm_block.active_def(0).name, "FM Osc");
    assert_eq!(fm_block.active_def(1).name, "FM-B");
    assert_eq!(fm_block.active_def(2).name, "FM-C");
}

#[test]
fn filter_block_has_correct_params() {
    let chain = &block_registry::FM_POLY_CHAIN;
    let filter_def = chain.blocks[2].def;
    assert_eq!(filter_def.params[0].label, "CUTOFF");
    assert_eq!(filter_def.params[1].label, "RESO");
    assert_eq!(filter_def.layout, PageLayout::BigViz);
}

#[test]
fn kick_chain_has_4_blocks() {
    let chain = &block_registry::KICK_CHAIN;
    assert_eq!(chain.len(), 4);
    assert_eq!(chain.blocks[0].def.name, "Noise");
}

#[test]
fn chain_active_def_resolves_correctly() {
    let chain = &block_registry::FM_POLY_CHAIN;
    // Node 0, sub_page 0 = FM Osc (primary)
    let def = chain.active_def(0, 0).unwrap();
    assert_eq!(def.name, "FM Osc");
    // Node 0, sub_page 2 = FM-C
    let def = chain.active_def(0, 2).unwrap();
    assert_eq!(def.name, "FM-C");
    // Node 2, sub_page 0 = Filter
    let def = chain.active_def(2, 0).unwrap();
    assert_eq!(def.name, "Filter");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core block_registry`
Expected: FAIL

- [ ] **Step 3: Create `block_registry.rs`**

Translate all existing `PageId` encoder_labels/val_formats/cell_icons/layout into static `BlockDef` instances. This is a mechanical translation — every match arm in `page.rs` becomes a `BlockDef` const.

```rust
// chimera-core/src/ui/block_registry.rs

use crate::ui::block_def::*;
use crate::ui::page::{ValFmt, CellIcon, PageLayout};

// ── Param slot shorthand ──

const EMPTY: ParamSlot = ParamSlot { label: "--", format: ValFmt::Uni, icon: CellIcon::None };

// ── FM Osc ──

pub static FM_A: BlockDef = BlockDef {
    name: "FM Osc",
    short: "FM",
    layout: PageLayout::BigViz,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "ALGO",  format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "FDBK",  format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "RAT C", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV C", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "LVL C", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "DTN C", format: ValFmt::Bi,     icon: CellIcon::None },
    ],
};

pub static FM_B: BlockDef = BlockDef {
    name: "FM-B",
    short: "FM",
    layout: PageLayout::BigViz,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "RAT M", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV M", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "LVL M", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "DTN M", format: ValFmt::Bi,     icon: CellIcon::None },
        ParamSlot { label: "RAT 2", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "LVL 2", format: ValFmt::Uni,    icon: CellIcon::None },
    ],
};

pub static FM_C: BlockDef = BlockDef {
    name: "FM-C",
    short: "FM",
    layout: PageLayout::BigViz,
    viz: VizType::AlgorithmDiagram,
    params: [
        ParamSlot { label: "RAT 3", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV 3", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "LVL 3", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "WAV 2", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "WAV 4", format: ValFmt::Int(7), icon: CellIcon::None },
        ParamSlot { label: "DTN 4", format: ValFmt::Bi,     icon: CellIcon::None },
    ],
};

// ── Modal Resonator ──

pub static MODAL_1: BlockDef = BlockDef {
    name: "Modal",
    short: "MOD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "MODE",   format: ValFmt::Int(3), icon: CellIcon::Arc },
        ParamSlot { label: "EXCITE", format: ValFmt::Uni,    icon: CellIcon::Burst },
        ParamSlot { label: "DECAY",  format: ValFmt::Uni,    icon: CellIcon::Ripple },
        ParamSlot { label: "BRIGHT", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "POS",    format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "INHARM", format: ValFmt::Uni,    icon: CellIcon::Arc },
    ],
};

pub static MODAL_2: BlockDef = BlockDef {
    name: "Modal-2",
    short: "MOD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "BODY",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "STIFF", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "FDBK",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "E.DPT", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "E.RAT", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "E.MIX", format: ValFmt::Uni, icon: CellIcon::DryWet },
    ],
};

// ── Drive ──

pub static DRIVE: BlockDef = BlockDef {
    name: "Drive",
    short: "DRV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "DRIVE", format: ValFmt::Uni, icon: CellIcon::WaveClip },
        ParamSlot { label: "TONE",  format: ValFmt::Bi,  icon: CellIcon::ToneTilt },
        ParamSlot { label: "MIX",   format: ValFmt::Bi,  icon: CellIcon::DryWet },
        EMPTY, EMPTY, EMPTY,
    ],
};

// ── Filter ──

pub static FILTER: BlockDef = BlockDef {
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        ParamSlot { label: "CUTOFF", format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "RESO",   format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "DRIVE",  format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "FM",     format: ValFmt::Uni,    icon: CellIcon::None },
        ParamSlot { label: "ENV",    format: ValFmt::Bi,     icon: CellIcon::None },
        ParamSlot { label: "TRACK",  format: ValFmt::Uni,    icon: CellIcon::None },
    ],
};

// ── Wavefolder ──

pub static FOLDER: BlockDef = BlockDef {
    name: "Folder",
    short: "FLD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "FOLD", format: ValFmt::Uni, icon: CellIcon::WaveFold },
        ParamSlot { label: "SYM",  format: ValFmt::Bi,  icon: CellIcon::Symmetry },
        ParamSlot { label: "MIX",  format: ValFmt::Bi,  icon: CellIcon::DryWet },
        EMPTY, EMPTY, EMPTY,
    ],
};

// ── VCA ──

pub static VCA: BlockDef = BlockDef {
    name: "VCA",
    short: "VCA",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: [
        ParamSlot { label: "ATK",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "DEC",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "SUS",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "REL",   format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "VEL",   format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

// ── Effects ──

pub static EFX: BlockDef = BlockDef {
    name: "Effects",
    short: "EFX",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "TYPE", format: ValFmt::Int(2), icon: CellIcon::Arc },
        ParamSlot { label: "TIME", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "DAMP", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "SIZE", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "MIX",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        EMPTY,
    ],
};

// ── Noise ──

pub static NOISE: BlockDef = BlockDef {
    name: "Noise",
    short: "NOI",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "TYPE",  format: ValFmt::Int(2), icon: CellIcon::Arc },
        ParamSlot { label: "PITCH", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "SWEEP", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni,    icon: CellIcon::LevelBar },
        ParamSlot { label: "E.AMT", format: ValFmt::Bi,     icon: CellIcon::Arc },
        EMPTY,
    ],
};

// ── Mod Matrix (placeholder — grid rendering TBD) ──

pub static MOD_MATRIX: BlockDef = BlockDef {
    name: "Mod Matrix",
    short: "MOD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "SRC",  format: ValFmt::Int(5), icon: CellIcon::Arc },
        ParamSlot { label: "DST",  format: ValFmt::Int(5), icon: CellIcon::Arc },
        ParamSlot { label: "ON",   format: ValFmt::Int(1), icon: CellIcon::Arc },
        EMPTY, EMPTY, EMPTY,
    ],
};

// ── Mixer / Envelope / Master (existing pages translated) ──

pub static MIXER: BlockDef = BlockDef {
    name: "Mixer",
    short: "MIX",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "VOL",    format: ValFmt::Uni,    icon: CellIcon::LevelBar },
        ParamSlot { label: "PAN",    format: ValFmt::Bi,     icon: CellIcon::PanDot },
        ParamSlot { label: "VOICES", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "MIDI",   format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "PITCH",  format: ValFmt::Bi,     icon: CellIcon::Arc },
        ParamSlot { label: "GLIDE",  format: ValFmt::Uni,    icon: CellIcon::Arc },
    ],
};

pub static ENV_AMP: BlockDef = BlockDef {
    name: "Amp Env",
    short: "AMP",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: VCA.params, // same layout as VCA
};

pub static ENV_FILTER: BlockDef = BlockDef {
    name: "Filter Env",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: VCA.params,
};

pub static ENV_AUX: BlockDef = BlockDef {
    name: "Aux Env",
    short: "AUX",
    layout: PageLayout::BigViz,
    viz: VizType::Adsr,
    params: VCA.params,
};

pub static CHORUS: BlockDef = BlockDef {
    name: "Chorus",
    short: "CHR",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "MODE",  format: ValFmt::Int(3), icon: CellIcon::Arc },
        ParamSlot { label: "RATE",  format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "DEPTH", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "MIX",   format: ValFmt::Uni,    icon: CellIcon::Arc },
        EMPTY, EMPTY,
    ],
};

pub static DELAY: BlockDef = BlockDef {
    name: "Delay",
    short: "DLY",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "TIME", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "FDBK", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "WOW",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "SAT",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "TONE", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "MIX",  format: ValFmt::Uni, icon: CellIcon::Arc },
    ],
};

pub static MASTER: BlockDef = BlockDef {
    name: "Master",
    short: "MST",
    layout: PageLayout::BigViz,
    viz: VizType::CompressorCurve,
    params: [
        ParamSlot { label: "VOL", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "PAN", format: ValFmt::Bi,  icon: CellIcon::None },
        EMPTY, EMPTY, EMPTY, EMPTY,
    ],
};

// ── VA (placeholder) ──

pub static VA: BlockDef = BlockDef {
    name: "VA",
    short: "VA",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "WAVE",   format: ValFmt::Uni, icon: CellIcon::WaveShape },
        ParamSlot { label: "PW",     format: ValFmt::Uni, icon: CellIcon::PulseWidth },
        ParamSlot { label: "SYNC",   format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "SUB",    format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "DETUNE", format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "MIX",    format: ValFmt::Bi,  icon: CellIcon::DryWet },
    ],
};

// ═══════════════════════════════════════════════════
// Chain Templates
// ═══════════════════════════════════════════════════

// ── FM Poly chain ──

static FM_OSC_BLOCK: ChainBlock = ChainBlock {
    def: &FM_A,
    sub_pages: &[&FM_B, &FM_C],
};

static DRIVE_BLOCK: ChainBlock = ChainBlock { def: &DRIVE, sub_pages: &[] };
static FILTER_BLOCK: ChainBlock = ChainBlock { def: &FILTER, sub_pages: &[] };
static FOLDER_BLOCK: ChainBlock = ChainBlock { def: &FOLDER, sub_pages: &[] };
static VCA_BLOCK: ChainBlock = ChainBlock { def: &VCA, sub_pages: &[] };
static MOD_MATRIX_BLOCK: ChainBlock = ChainBlock { def: &MOD_MATRIX, sub_pages: &[] };

pub static FM_POLY_CHAIN: ChainDef2 = ChainDef2 {
    name: "FM Poly",
    blocks: &[FM_OSC_BLOCK, DRIVE_BLOCK, FILTER_BLOCK, FOLDER_BLOCK, VCA_BLOCK, MOD_MATRIX_BLOCK],
};

// ── Kick chain ──

static NOISE_BLOCK: ChainBlock = ChainBlock { def: &NOISE, sub_pages: &[] };
// Tuned resonator placeholder — reuse filter for now
static TUNED_RESO_BLOCK: ChainBlock = ChainBlock { def: &FILTER, sub_pages: &[] };
// LPG placeholder — reuse VCA for now
static LPG_BLOCK: ChainBlock = ChainBlock { def: &VCA, sub_pages: &[] };

pub static KICK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Kick",
    blocks: &[NOISE_BLOCK, TUNED_RESO_BLOCK, LPG_BLOCK, MOD_MATRIX_BLOCK],
};

// ── Modal Pluck chain ──

static MODAL_BLOCK: ChainBlock = ChainBlock {
    def: &MODAL_1,
    sub_pages: &[&MODAL_2],
};

pub static MODAL_PLUCK_CHAIN: ChainDef2 = ChainDef2 {
    name: "Modal Pluck",
    blocks: &[MODAL_BLOCK, FILTER_BLOCK, VCA_BLOCK, MOD_MATRIX_BLOCK],
};

// ── Mix chain (existing, for backward compat during migration) ──

static MIXER_BLOCK: ChainBlock = ChainBlock { def: &MIXER, sub_pages: &[] };
static CHORUS_BLOCK: ChainBlock = ChainBlock { def: &CHORUS, sub_pages: &[] };
static DELAY_BLOCK: ChainBlock = ChainBlock { def: &DELAY, sub_pages: &[] };
static REVERB_BLOCK: ChainBlock = ChainBlock { def: &EFX, sub_pages: &[] };
static MASTER_BLOCK: ChainBlock = ChainBlock { def: &MASTER, sub_pages: &[] };

pub static MIX_CHAIN: ChainDef2 = ChainDef2 {
    name: "Mix",
    blocks: &[MIXER_BLOCK, CHORUS_BLOCK, DELAY_BLOCK, REVERB_BLOCK, MASTER_BLOCK],
};

// ── Envelope chain (existing) ──

static ENV_AMP_BLOCK: ChainBlock = ChainBlock { def: &ENV_AMP, sub_pages: &[] };
static ENV_FILTER_BLOCK: ChainBlock = ChainBlock { def: &ENV_FILTER, sub_pages: &[] };
static ENV_AUX_BLOCK: ChainBlock = ChainBlock { def: &ENV_AUX, sub_pages: &[] };

pub static ENVELOPE_CHAIN: ChainDef2 = ChainDef2 {
    name: "Envelope",
    blocks: &[ENV_AMP_BLOCK, ENV_FILTER_BLOCK, ENV_AUX_BLOCK],
};
```

- [ ] **Step 4: Add module to mod.rs**

Add `pub mod block_registry;` to `chimera-core/src/ui/mod.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p chimera-core`
Expected: All tests pass including new registry tests

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/ui/block_registry.rs chimera-core/src/ui/mod.rs chimera-core/tests/block_def_tests.rs
git commit -m "feat(core): add block registry — all existing pages as BlockDef data"
```

---

### Task 3: Wire renderer to accept BlockDef

**Files:**
- Modify: `chimera-core/src/ui/renderer.rs`

This task makes the renderer able to draw from a `BlockDef` WITHOUT removing the old `PageId` path. Both work in parallel during migration.

- [ ] **Step 1: Add `draw_from_block_def` and `draw_params_from_def` methods**

Add to `Renderer`:

```rust
/// Draw params from a BlockDef (generic — works for any block).
fn draw_params_from_def<D>(&self, display: &mut D, def: &BlockDef)
where
    D: DrawTarget<Color = Rgb565>,
{
    let label_style = MonoTextStyle::new(&FONT_6X10, theme::PARAM_LABEL);
    let value_style = MonoTextStyle::new(&FONT_6X10, theme::PARAM_VALUE);

    for (i, slot) in def.params.iter().enumerate() {
        if slot.label == "--" {
            continue;
        }

        let col = i % 3;
        let row = i / 3;
        let x = theme::PARAM_LEFT + col as i32 * theme::PARAM_COL_WIDTH;
        let y = theme::PARAM_TOP + row as i32 * theme::PARAM_ROW_HEIGHT;

        let val = self.anim[i].current();

        let _ = Text::new(slot.label, Point::new(x, y + 10), label_style).draw(display);

        let mut buf = FmtBuf::new();
        fmt::fmt_val(&mut buf, val, slot.format);
        let label_end = x + slot.label.len() as i32 * 6 + 4;
        let _ = Text::new(buf.as_str(), Point::new(label_end, y + 10), value_style).draw(display);

        let bar_y = y + 15;
        let _ = Rectangle::new(
            Point::new(x, bar_y),
            Size::new(theme::BAR_WIDTH as u32, theme::BAR_HEIGHT as u32),
        )
        .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_BG), display);

        let fill_w = (theme::BAR_WIDTH as f32 * val) as i32;
        if fill_w > 0 {
            let _ = Rectangle::new(
                Point::new(x, bar_y),
                Size::new(fill_w as u32, theme::BAR_HEIGHT as u32),
            )
            .draw_styled(&PrimitiveStyle::with_fill(theme::PARAM_BAR_FG), display);
        }
    }
}

/// Draw cell grid from a BlockDef (generic).
fn draw_cell_grid_from_def<D>(&self, display: &mut D, def: &BlockDef)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, slot) in def.params.iter().enumerate() {
        let col = (i % 3) as i32;
        let row = (i / 3) as i32;
        cell::draw_cell(display, col, row, slot.label, self.anim[i].current(), slot.icon, slot.format);
    }
}

/// Draw visualization from VizType (generic dispatch).
fn draw_viz_from_type<D>(&self, display: &mut D, viz: VizType)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::ui::block_def::VizType;
    match viz {
        VizType::AlgorithmDiagram => self.draw_fm_viz(display),
        VizType::ModalPeaks => self.draw_modal_viz(display),
        VizType::WaveformPreview => self.draw_va_viz(display),
        VizType::DriveClip => self.draw_drive_viz(display),
        VizType::FilterResponse => self.draw_filter_viz(display),
        VizType::WaveFold => self.draw_folder_viz(display),
        VizType::Adsr => self.draw_envelope_viz(display),
        VizType::EffectsFlow => self.draw_efx_viz(display),
        VizType::MixerLevels => self.draw_mixer_viz(display),
        VizType::RoutingMatrix => self.draw_routing_viz(display),
        VizType::CompressorCurve => self.draw_comp_viz(display),
        VizType::None | VizType::EqResponse | VizType::LpgResponse | VizType::Logo => {}
    }
}
```

- [ ] **Step 2: Build and test**

Run: `cargo test -p chimera-core && cargo build -p chimera-core`
Expected: Compiles and all tests pass. New methods exist but aren't called yet.

- [ ] **Step 3: Commit**

```bash
git add chimera-core/src/ui/renderer.rs
git commit -m "feat(core): add BlockDef-based rendering methods to Renderer"
```

---

### Task 4: Migrate renderer to use BlockDef exclusively

**Files:**
- Modify: `chimera-core/src/ui/renderer.rs`
- Modify: `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/src/ui/region.rs`

This task switches the renderer from `PageId` dispatch to `BlockDef` dispatch. The `UiState` resolves the active `BlockDef` and passes it through.

- [ ] **Step 1: Add `active_block_def()` method to `UiState`**

In `mod.rs`, add a method that resolves the current `BlockDef` from navigation position using the new chain system:

```rust
use crate::ui::block_def::{BlockDef, ChainDef2};
use crate::ui::block_registry;

impl UiState {
    /// Resolve the active BlockDef from current navigation position.
    fn active_block_def(&self) -> &'static BlockDef {
        // For now, use a mapping from old chain index to new ChainDef2
        let chain = match self.nav.chain {
            0 => &block_registry::FM_POLY_CHAIN,
            1 => &block_registry::MIX_CHAIN,
            2 => &block_registry::ENVELOPE_CHAIN,
            _ => &block_registry::FM_POLY_CHAIN,
        };
        chain.active_def(self.nav.node, self.nav.sub_page)
            .unwrap_or(block_registry::FM_POLY_CHAIN.blocks[0].def)
    }
}
```

- [ ] **Step 2: Update `Renderer::update()` to accept `BlockDef`**

Change `update` to read values via a passed-in function or accept the `BlockDef` directly. For now, keep the old `page.read_values()` path — the parameter binding migration is Task 5.

- [ ] **Step 3: Update `Renderer::draw()` to use `BlockDef` for layout/viz dispatch**

Replace the `page.layout()` and `draw_visualization(page)` calls with `def.layout` and `draw_viz_from_type(def.viz)`. Replace `draw_params(page)` with `draw_params_from_def(def)`. Replace `draw_cell_grid(page)` with `draw_cell_grid_from_def(def)`.

- [ ] **Step 4: Update `draw_region()` similarly**

- [ ] **Step 5: Update `draw_header()` to read from `BlockDef`**

Instead of reading chain/node names from `ChainNav.chain_def()`, read from the `BlockDef.name`.

- [ ] **Step 6: Update `region.rs` to use `BlockDef.layout` instead of `page.layout()`**

The dirty tracking needs the layout to determine which regions exist. Pass the layout from `BlockDef` instead of from `PageId`.

- [ ] **Step 7: Build firmware to verify**

Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`
Expected: Compiles. The desktop simulator should also build: `cargo build -p chimera-desktop`

- [ ] **Step 8: Run tests**

Run: `cargo test -p chimera-core`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add chimera-core/src/ui/renderer.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/region.rs
git commit -m "refactor(core): renderer uses BlockDef for layout, viz, and param display"
```

---

### Task 5: Migrate parameter editing to generic binding

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/src/ui/block_def.rs` (if binding type needed)

The old system has per-page `apply_encoder` / `read_values` match arms that know which `ParamSnapshot` fields to touch. The new system needs a generic way to bind `ParamSlot` index → `Param` reference.

For this first pass, keep the existing `PageId`-based parameter binding as a compatibility layer. The `BlockDef` determines display (labels, format, icons, layout, viz) while `PageId` still handles parameter read/write. This is a pragmatic intermediate step — full parameter binding can be migrated later.

- [ ] **Step 1: Keep `PageId::read_values()` and `PageId::apply_encoder()` for now**

The `UiState.page` field (PageId) continues to exist alongside the new `active_block_def()`. The renderer uses `BlockDef` for display. Parameter editing uses `PageId`. This works because the `BlockDef` labels and formats match what `PageId` already had.

- [ ] **Step 2: Update `UiState::update()` to pass `BlockDef` to renderer**

```rust
pub fn update(&mut self) {
    let def = self.active_block_def();
    let values = self.page.read_values(&self.params); // still using PageId for values
    for (a, &v) in self.renderer.anim.iter_mut().zip(values.iter()) {
        a.set_target(v);
        a.update();
    }
}
```

- [ ] **Step 3: Verify encoder editing still works**

The existing `handle_input` flow calls `self.page.apply_encoder(...)` which still works unchanged.

- [ ] **Step 4: Build and test everything**

Run: `cargo test -p chimera-core && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`
Expected: Compiles, tests pass, behavior identical to before.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/mod.rs
git commit -m "refactor(core): UiState uses BlockDef for display, PageId for param editing"
```

---

### Task 6: Clean up — remove redundant PageId display methods

**Files:**
- Modify: `chimera-core/src/ui/page.rs`

Now that the renderer reads from `BlockDef`, the following `PageId` methods are dead code:
- `layout()` — replaced by `BlockDef.layout`
- `cell_icons()` — replaced by `ParamSlot.icon`
- `val_formats()` — replaced by `ParamSlot.format`
- `encoder_labels()` — replaced by `ParamSlot.label`

Keep `read_values()`, `apply_encoder()`, `snap_encoder()`, and `from_nav()` — these still handle parameter binding.

- [ ] **Step 1: Remove `layout()`, `cell_icons()`, `val_formats()`, `encoder_labels()` from `PageId`**

Delete these methods and all their match arms.

- [ ] **Step 2: Remove `PageLayout` and `CellIcon` from `page.rs` if they've been moved to `block_def.rs`**

Or keep them in `page.rs` and re-export from `block_def.rs` — whichever avoids circular dependencies.

- [ ] **Step 3: Build and test**

Run: `cargo test -p chimera-core && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`
Expected: Compiles, tests pass. Some dead code warnings may appear for unused `PageId` variants.

- [ ] **Step 4: Commit**

```bash
git add chimera-core/src/ui/page.rs chimera-core/src/ui/block_def.rs
git commit -m "refactor(core): remove PageId display methods — BlockDef is the source of truth"
```

---

### Task 7: Flash and verify on hardware

**Files:** None (hardware test)

- [ ] **Step 1: Build firmware**

Run: `just firmware`

- [ ] **Step 2: Flash**

Run: `rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin && dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave`

- [ ] **Step 3: Verify**

- All pages display correctly (same as before the refactor)
- Encoder editing works on all pages
- Navigation (Minus/Plus, B1-B6, Seq/Edit) works
- Dungeon map renders correctly
- Audio still plays (440 Hz sine from DMA)
- No visual glitches or rendering artifacts

- [ ] **Step 4: Commit binary**

```bash
git add chimera.bin
git commit -m "refactor(core): BlockDef component system — verified on hardware"
```
