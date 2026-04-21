# Site Map Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the old hardcoded chain navigation (VOICE_CHAIN/MIX_CHAIN/ENVELOPE_CHAIN indexed by button) with the site map topology: B1-B6 = Part chains, MIX+B1-B6 = mixer channel chains, MENU = system chain, MIX+B6 = demo storyboard.

**Architecture:** `ChainNav` uses a `ChainId` enum (`Part(0-5)`, `Mixer(0-5)`, `System`, `Demo`) instead of a raw chain index. Button handling checks MIX modifier state. Each Part defaults to FM_POLY_CHAIN. Dungeon map reads from `ChainDef2` blocks. Old `ChainDef`/`NodeDef`/`CHAINS` array removed.

**Tech Stack:** Rust, `no_std`, `chimera-core` crate

**Specs:**
- `docs/chimera-ui-ux-spec.md` — navigation state machine, button mapping
- `docs/chimera-sitemap.md` — complete chain topology

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `chimera-core/src/ui/chain.rs` | **Rewrite** | New `ChainId` enum, new `ChainNav` with MIX-aware button handling, resolve to `ChainDef2` |
| `chimera-core/src/ui/block_registry.rs` | **Modify** | Add mixer channel strip chain, system chain, demo chain definitions |
| `chimera-core/src/ui/dungeon_map.rs` | **Rewrite** | Read from `ChainDef2`/`ChainBlock` instead of old `ChainDef`/`NodeDef` |
| `chimera-core/src/ui/mod.rs` | **Modify** | `UiState.active_block_def()` uses new `ChainNav.resolve()`. Remove old chain-index mapping. |
| `chimera-core/src/ui/page.rs` | **Modify** | `PageId::from_nav()` updated for new ChainId structure |
| `chimera-core/src/ui/renderer.rs` | **Modify** | Header rendering uses `ChainId` for context label |
| `chimera-core/tests/block_def_tests.rs` | **Modify** | Add navigation + new chain tests |

---

### Task 1: Add mixer, system, and demo chains to block registry

**Files:**
- Modify: `chimera-core/src/ui/block_registry.rs`
- Modify: `chimera-core/tests/block_def_tests.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn mixer_channel_strip_chain() {
    let chain = &block_registry::MIXER_CHANNEL_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Channel");
    assert_eq!(chain.blocks[1].def.name, "MIDI");
    assert_eq!(chain.blocks[2].def.name, "EQ");
    assert_eq!(chain.blocks[3].def.name, "Sends");
    assert_eq!(chain.len(), 4);
}

#[test]
fn system_chain() {
    let chain = &block_registry::SYSTEM_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "MIDI Setup");
    assert_eq!(chain.len(), 5); // MIDI, Tuning, Theme, Updates, About
}

#[test]
fn demo_chain() {
    let chain = &block_registry::DEMO_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Waves");
    assert_eq!(chain.len(), 3);
}
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test -p chimera-core mixer_channel`
Expected: FAIL

- [ ] **Step 3: Add BlockDef statics for mixer channel blocks**

```rust
// ── Mixer Channel Strip ──

pub static CHANNEL: BlockDef = BlockDef {
    name: "Channel",
    short: "CH",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "VOL",    format: ValFmt::Uni,    icon: CellIcon::LevelBar },
        ParamSlot { label: "PAN",    format: ValFmt::Bi,     icon: CellIcon::PanDot },
        ParamSlot { label: "OUT",    format: ValFmt::Int(2),  icon: CellIcon::Arc },
        ParamSlot { label: "VOICES", format: ValFmt::Int(5),  icon: CellIcon::Arc },
        ParamSlot { label: "MODE",   format: ValFmt::Int(2),  icon: CellIcon::Arc },
        ParamSlot { label: "GLIDE",  format: ValFmt::Uni,    icon: CellIcon::Arc },
    ],
};

pub static MIDI_SETUP: BlockDef = BlockDef {
    name: "MIDI",
    short: "MID",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "CH",    format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "PGM",   format: ValFmt::Int(1),  icon: CellIcon::Arc },
        ParamSlot { label: "CC.RX", format: ValFmt::Int(1),  icon: CellIcon::Arc },
        ParamSlot { label: "BEND",  format: ValFmt::Int(12), icon: CellIcon::Arc },
        ParamSlot { label: "TRNS",  format: ValFmt::Bi,      icon: CellIcon::Arc },
        EMPTY,
    ],
};

pub static EQ: BlockDef = BlockDef {
    name: "EQ",
    short: "EQ",
    layout: PageLayout::BigViz,
    viz: VizType::EqResponse,
    params: [
        ParamSlot { label: "LOW",   format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "L.FRQ", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "MID",   format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "M.FRQ", format: ValFmt::Uni, icon: CellIcon::None },
        ParamSlot { label: "HIGH",  format: ValFmt::Bi,  icon: CellIcon::None },
        ParamSlot { label: "H.FRQ", format: ValFmt::Uni, icon: CellIcon::None },
    ],
};

pub static SENDS: BlockDef = BlockDef {
    name: "Sends",
    short: "SND",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "REV",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "DLY",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "CHR",  format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "S4",   format: ValFmt::Uni, icon: CellIcon::Arc },
        EMPTY, EMPTY,
    ],
};

// Mixer channel strip chain (same for all 6 channels)
static CHANNEL_BLOCK: ChainBlock = ChainBlock { def: &CHANNEL, sub_pages: &[] };
static MIDI_SETUP_BLOCK: ChainBlock = ChainBlock { def: &MIDI_SETUP, sub_pages: &[] };
static EQ_BLOCK: ChainBlock = ChainBlock { def: &EQ, sub_pages: &[] };
static SENDS_BLOCK: ChainBlock = ChainBlock { def: &SENDS, sub_pages: &[] };

pub static MIXER_CHANNEL_CHAIN: ChainDef2 = ChainDef2 {
    name: "Mixer",
    blocks: &[CHANNEL_BLOCK, MIDI_SETUP_BLOCK, EQ_BLOCK, SENDS_BLOCK],
};
```

- [ ] **Step 4: Add system chain blocks**

```rust
// ── System Chain ──

pub static SYS_MIDI: BlockDef = BlockDef {
    name: "MIDI Setup",
    short: "MID",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "P1 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P2 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P3 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P4 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P5 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
        ParamSlot { label: "P6 CH", format: ValFmt::Int(16), icon: CellIcon::Arc },
    ],
};

pub static SYS_TUNING: BlockDef = BlockDef {
    name: "Tuning",
    short: "TUN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "TUNE",  format: ValFmt::Bi,     icon: CellIcon::Arc },
        ParamSlot { label: "SCALE", format: ValFmt::Int(2),  icon: CellIcon::Arc },
        EMPTY, EMPTY, EMPTY, EMPTY,
    ],
};

pub static SYS_THEME: BlockDef = BlockDef {
    name: "Theme",
    short: "THM",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "BRIGHT", format: ValFmt::Uni,    icon: CellIcon::Arc },
        ParamSlot { label: "ACCENT", format: ValFmt::Int(4), icon: CellIcon::Arc },
        EMPTY, EMPTY, EMPTY, EMPTY,
    ],
};

pub static SYS_UPDATES: BlockDef = BlockDef {
    name: "Updates",
    short: "UPD",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [EMPTY; 6],
};

pub static SYS_ABOUT: BlockDef = BlockDef {
    name: "About",
    short: "ABT",
    layout: PageLayout::BigViz,
    viz: VizType::Logo,
    params: [EMPTY; 6],
};

static SYS_MIDI_BLOCK: ChainBlock = ChainBlock { def: &SYS_MIDI, sub_pages: &[] };
static SYS_TUNING_BLOCK: ChainBlock = ChainBlock { def: &SYS_TUNING, sub_pages: &[] };
static SYS_THEME_BLOCK: ChainBlock = ChainBlock { def: &SYS_THEME, sub_pages: &[] };
static SYS_UPDATES_BLOCK: ChainBlock = ChainBlock { def: &SYS_UPDATES, sub_pages: &[] };
static SYS_ABOUT_BLOCK: ChainBlock = ChainBlock { def: &SYS_ABOUT, sub_pages: &[] };

pub static SYSTEM_CHAIN: ChainDef2 = ChainDef2 {
    name: "System",
    blocks: &[SYS_MIDI_BLOCK, SYS_TUNING_BLOCK, SYS_THEME_BLOCK, SYS_UPDATES_BLOCK, SYS_ABOUT_BLOCK],
};
```

- [ ] **Step 5: Add demo chain (storyboard)**

Reuse existing demo BlockDefs from the current code or create new ones for the demo/storyboard pages (Waves, Shapes, Motion). These already exist in the current `DEMO_CHAIN` as `DemoWaves`, `DemoShapes`, `DemoMotion` PageIds.

```rust
// ── Demo / Storyboard Chain ──

pub static DEMO_WAVES: BlockDef = BlockDef {
    name: "Waves",
    short: "WAV",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "CLIP",  format: ValFmt::Uni, icon: CellIcon::WaveClip },
        ParamSlot { label: "WAVE",  format: ValFmt::Uni, icon: CellIcon::WaveShape },
        ParamSlot { label: "PW",    format: ValFmt::Uni, icon: CellIcon::PulseWidth },
        ParamSlot { label: "FOLD",  format: ValFmt::Uni, icon: CellIcon::WaveFold },
        ParamSlot { label: "TILT",  format: ValFmt::Bi,  icon: CellIcon::ToneTilt },
        ParamSlot { label: "SYM",   format: ValFmt::Bi,  icon: CellIcon::Symmetry },
    ],
};

pub static DEMO_SHAPES: BlockDef = BlockDef {
    name: "Shapes",
    short: "SHP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "ARC",   format: ValFmt::Uni, icon: CellIcon::Arc },
        ParamSlot { label: "LEVEL", format: ValFmt::Uni, icon: CellIcon::LevelBar },
        ParamSlot { label: "PAN",   format: ValFmt::Bi,  icon: CellIcon::PanDot },
        ParamSlot { label: "D/W",   format: ValFmt::Bi,  icon: CellIcon::DryWet },
        ParamSlot { label: "CUBE",  format: ValFmt::Uni, icon: CellIcon::Cube },
        ParamSlot { label: "STACK", format: ValFmt::Uni, icon: CellIcon::Stack },
    ],
};

pub static DEMO_MOTION: BlockDef = BlockDef {
    name: "Motion",
    short: "MOT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot { label: "RIPPL", format: ValFmt::Uni, icon: CellIcon::Ripple },
        ParamSlot { label: "BURST", format: ValFmt::Uni, icon: CellIcon::Burst },
        ParamSlot { label: "ORBIT", format: ValFmt::Uni, icon: CellIcon::Orbit },
        ParamSlot { label: "SCATR", format: ValFmt::Uni, icon: CellIcon::Scatter },
        ParamSlot { label: "BOUNC", format: ValFmt::Bi,  icon: CellIcon::Bounce },
        ParamSlot { label: "PULSE", format: ValFmt::Uni, icon: CellIcon::Breathe },
    ],
};

static DEMO_WAVES_BLOCK: ChainBlock = ChainBlock { def: &DEMO_WAVES, sub_pages: &[] };
static DEMO_SHAPES_BLOCK: ChainBlock = ChainBlock { def: &DEMO_SHAPES, sub_pages: &[] };
static DEMO_MOTION_BLOCK: ChainBlock = ChainBlock { def: &DEMO_MOTION, sub_pages: &[] };

pub static DEMO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Demo",
    blocks: &[DEMO_WAVES_BLOCK, DEMO_SHAPES_BLOCK, DEMO_MOTION_BLOCK],
};
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p chimera-core`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/ui/block_registry.rs chimera-core/tests/block_def_tests.rs
git commit -m "feat(core): add mixer channel, system, and demo chain definitions"
```

---

### Task 2: Rewrite ChainNav with ChainId enum

**Files:**
- Modify: `chimera-core/src/ui/chain.rs`
- Modify: `chimera-core/tests/block_def_tests.rs`

- [ ] **Step 1: Write tests**

```rust
use chimera_core::ui::chain::{ChainId, ChainNav};
use chimera_hal::{ButtonId, ButtonState};

#[test]
fn default_nav_is_part_0() {
    let nav = ChainNav::new();
    assert_eq!(nav.chain_id, ChainId::Part(0));
    assert_eq!(nav.node, 0);
    assert_eq!(nav.sub_page, 0);
}

#[test]
fn nav_resolves_part_chain() {
    let nav = ChainNav::new();
    let chain = nav.active_chain();
    assert_eq!(chain.name, "FM Poly");
}

#[test]
fn nav_resolves_block_def() {
    let nav = ChainNav::new();
    let def = nav.active_block_def();
    assert_eq!(def.name, "FM Osc");
}
```

- [ ] **Step 2: Rewrite `chain.rs`**

Replace the entire file. The new `ChainNav`:

```rust
use chimera_hal::{ButtonId, ButtonState, Controls};
use crate::ui::block_def::{BlockDef, ChainDef2};
use crate::ui::block_registry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainId {
    Part(usize),    // 0-5 (B1-B6)
    Mixer(usize),   // 0-5 (MIX + B1-B6)
    System,         // MENU
    Demo,           // MIX + B6
}

#[derive(Clone, Copy, Debug)]
pub struct ChainNav {
    pub chain_id: ChainId,
    pub node: usize,
    pub sub_page: usize,
}

impl Default for ChainNav {
    fn default() -> Self { Self::new() }
}

impl ChainNav {
    pub const fn new() -> Self {
        Self { chain_id: ChainId::Part(0), node: 0, sub_page: 0 }
    }

    /// Get the active ChainDef2 for the current chain_id.
    pub fn active_chain(&self) -> &'static ChainDef2 {
        match self.chain_id {
            ChainId::Part(_) => &block_registry::FM_POLY_CHAIN, // all Parts default to FM Poly for now
            ChainId::Mixer(_) => &block_registry::MIXER_CHANNEL_CHAIN,
            ChainId::System => &block_registry::SYSTEM_CHAIN,
            ChainId::Demo => &block_registry::DEMO_CHAIN,
        }
    }

    /// Get the active BlockDef at the current position.
    pub fn active_block_def(&self) -> &'static BlockDef {
        self.active_chain()
            .active_def(self.node, self.sub_page)
            .unwrap_or(block_registry::FM_POLY_CHAIN.blocks[0].def)
    }

    /// Get the active ChainBlock at the current node.
    pub fn active_chain_block(&self) -> Option<&'static crate::ui::block_def::ChainBlock> {
        self.active_chain().block_at(self.node)
    }

    /// Process control input and update navigation state.
    /// Returns true if position changed.
    pub fn handle_input(&mut self, controls: &impl Controls) -> bool {
        let prev_chain = self.chain_id;
        let prev_node = self.node;
        let prev_sub = self.sub_page;

        let shift = matches!(
            controls.button_state(ButtonId::Mix),
            ButtonState::Pressed | ButtonState::Held
        );

        // B1-B6: Part select (or MIX+B1-B5 = mixer, MIX+B6 = demo)
        let buttons = [ButtonId::B1, ButtonId::B2, ButtonId::B3, ButtonId::B4, ButtonId::B5, ButtonId::B6];
        for (i, &btn) in buttons.iter().enumerate() {
            if controls.button_state(btn) == ButtonState::Pressed {
                let target = if shift {
                    if i == 5 {
                        ChainId::Demo  // MIX + B6 = demo storyboard
                    } else {
                        ChainId::Mixer(i)  // MIX + B1-B5 = mixer channels
                    }
                } else {
                    ChainId::Part(i)  // B1-B6 = Part select
                };

                if self.chain_id == target {
                    // Same chain = snap home
                    self.node = 0;
                    self.sub_page = 0;
                } else {
                    self.chain_id = target;
                    self.node = 0;
                    self.sub_page = 0;
                }
            }
        }

        // MENU: system chain
        if controls.button_state(ButtonId::Menu) == ButtonState::Pressed {
            if self.chain_id == ChainId::System {
                self.node = 0;
                self.sub_page = 0;
            } else {
                self.chain_id = ChainId::System;
                self.node = 0;
                self.sub_page = 0;
            }
        }

        // Minus/Plus: horizontal navigation within chain
        let chain = self.active_chain();
        if controls.button_state(ButtonId::Minus) == ButtonState::Pressed && self.node > 0 {
            self.node -= 1;
            self.sub_page = 0;
        }
        if controls.button_state(ButtonId::Plus) == ButtonState::Pressed && self.node + 1 < chain.len() {
            self.node += 1;
            self.sub_page = 0;
        }

        // Seq/Edit: vertical sub-page navigation
        if controls.button_state(ButtonId::Seq) == ButtonState::Pressed && self.sub_page > 0 {
            self.sub_page -= 1;
        }
        if controls.button_state(ButtonId::Edit) == ButtonState::Pressed {
            if let Some(block) = self.active_chain_block() {
                if block.sub_page_count() > 0 && self.sub_page + 1 < block.sub_page_count() {
                    self.sub_page += 1;
                }
            }
        }

        // Return whether position changed
        self.chain_id != prev_chain || self.node != prev_node || self.sub_page != prev_sub
    }
}
```

Note: The old `ChainDef`, `NodeDef`, `CHAINS`, `VOICE_CHAIN`, `MIX_CHAIN`, `ENVELOPE_CHAIN`, `DEMO_CHAIN` statics are REMOVED. Only keep them temporarily if other code still references them — check first.

- [ ] **Step 3: Run tests, fix compile errors**

The dungeon map, renderer header, and `UiState.active_block_def()` reference the old chain types. These will break. Fix them in subsequent tasks.

For THIS task: get `chain.rs` compiling and its own tests passing. If other files break, add `#[allow(dead_code)]` temporarily on old types or leave the old code alongside the new code.

Actually — the cleanest approach is: keep the old `ChainDef`/`NodeDef` types and `CHAINS` array in chain.rs but mark them `#[allow(dead_code)]`. The new `ChainId`/`ChainNav` exists alongside them. Subsequent tasks migrate consumers and then delete the old types.

- [ ] **Step 4: Build and test**

Run: `cargo test -p chimera-core && cargo build -p chimera-desktop`

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/chain.rs chimera-core/tests/block_def_tests.rs
git commit -m "feat(core): new ChainNav with ChainId — Part/Mixer/System/Demo"
```

---

### Task 3: Wire UiState to new ChainNav

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`

- [ ] **Step 1: Update `active_block_def()` to use `nav.active_block_def()`**

Replace the old chain-index mapping with a single call:

```rust
fn active_block_def(&self) -> &'static BlockDef {
    self.nav.active_block_def()
}
```

- [ ] **Step 2: Update `handle_input()` — remove old PageId engine-type switching**

The old code switches `params.engine` based on `PageId::EngineFmA`, etc. This logic needs to stay but adapt to the new navigation. For now, keep it working by checking the active `BlockDef` name or a new mechanism.

Simplest approach: check if the active block def is one of the FM/Modal/VA defs:

```rust
if nav_changed {
    let def = self.nav.active_block_def();
    // Engine type switching based on active block
    if core::ptr::eq(def, &block_registry::FM_A)
        || core::ptr::eq(def, &block_registry::FM_B)
        || core::ptr::eq(def, &block_registry::FM_C) {
        self.params.engine = EngineType::Fm;
    } else if core::ptr::eq(def, &block_registry::MODAL_1)
        || core::ptr::eq(def, &block_registry::MODAL_2) {
        self.params.engine = EngineType::Modal;
    } else if core::ptr::eq(def, &block_registry::VA) {
        self.params.engine = EngineType::Va;
    }
    // else: keep current engine
}
```

- [ ] **Step 3: Keep `PageId::from_nav()` working for parameter binding**

`PageId::from_nav()` still needs to work for `read_values()` and `apply_encoder()`. It currently reads `nav.chain` (the old usize index). Update it to read `nav.chain_id` and map accordingly:

```rust
pub fn from_nav(nav: &ChainNav) -> Self {
    match nav.chain_id {
        ChainId::Part(_) => { /* same match on nav.node/nav.sub_page as before for chain 0 */ }
        ChainId::Mixer(_) => { /* return Mixer for node 0, etc. */ }
        ChainId::System => { /* placeholder — no param editing yet */ }
        ChainId::Demo => { /* same match as old chain 5 */ }
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cargo test -p chimera-core && cargo build -p chimera-desktop && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/mod.rs chimera-core/src/ui/page.rs
git commit -m "refactor(core): UiState uses new ChainNav with ChainId"
```

---

### Task 4: Rewrite dungeon map for ChainDef2

**Files:**
- Modify: `chimera-core/src/ui/dungeon_map.rs`

- [ ] **Step 1: Update `draw()` to accept ChainDef2 data**

The dungeon map currently calls `nav.chain_def()` (returns old `ChainDef`) and reads `node.short` and `node.sub_pages`. Update to use `nav.active_chain()` (returns `ChainDef2`) and read from `ChainBlock.def.short` and `ChainBlock.sub_pages`.

```rust
pub fn draw<D>(display: &mut D, nav: &ChainNav)
where
    D: DrawTarget<Color = Rgb565>,
{
    let chain = nav.active_chain();
    
    // Separator
    // Chain name label
    // Draw node boxes from chain.blocks[i].def.short
    // Draw connectors between nodes
    // Active node = nav.node
    // Sub-page branches from chain.blocks[nav.node].sub_pages (show def.short for each)
}
```

- [ ] **Step 2: Update `draw_nodes()` to iterate over `chain.blocks`**

Each `ChainBlock` has `def.short` for the 3-char label and `sub_pages` for branch detection.

- [ ] **Step 3: Update `draw_branches()` to use `ChainBlock.sub_pages`**

Sub-pages are now `&[&BlockDef]` instead of `&[&str]`. Use `sub_def.short` or `sub_def.name` as the branch label.

- [ ] **Step 4: Build and test**

Run: `cargo build -p chimera-desktop && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/dungeon_map.rs
git commit -m "refactor(core): dungeon map reads from ChainDef2/ChainBlock"
```

---

### Task 5: Update renderer header for ChainId context

**Files:**
- Modify: `chimera-core/src/ui/renderer.rs`

- [ ] **Step 1: Update `draw_header_with_def()` to show ChainId context**

The header should show:
- Part chains: `"Part N > BlockName"` (e.g., "Part 1 > Filter")
- Mixer chains: `"Mixer CH N > BlockName"` (e.g., "Mixer CH 1 > EQ")
- System: `"System > BlockName"` (e.g., "System > Tuning")
- Demo: `"Demo > BlockName"`

Pass `ChainId` to the header draw function. Use `core::fmt::Write` with `FmtBuf` to build the string.

- [ ] **Step 2: Build and test**

- [ ] **Step 3: Commit**

```bash
git add chimera-core/src/ui/renderer.rs
git commit -m "refactor(core): header shows ChainId context (Part N, Mixer CH N, System, Demo)"
```

---

### Task 6: Remove old ChainDef/NodeDef/CHAINS

**Files:**
- Modify: `chimera-core/src/ui/chain.rs`

- [ ] **Step 1: Remove old types**

Delete: `ChainDef`, `NodeDef`, `VOICE_CHAIN`, `MIX_CHAIN`, `ENVELOPE_CHAIN`, `DEMO_CHAIN`, `CHAINS` array, and the old `chain_def()`/`node_def()` methods from `ChainNav`.

- [ ] **Step 2: Grep for any remaining references**

Search the codebase for `ChainDef ` (with space), `NodeDef`, `VOICE_CHAIN`, `CHAINS`, `chain_def()`, `node_def()`. Remove or update all references.

- [ ] **Step 3: Build everything**

Run: `cargo test -p chimera-core && cargo build -p chimera-desktop && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 4: Commit**

```bash
git add chimera-core/src/ui/chain.rs
git commit -m "refactor(core): remove old ChainDef/NodeDef — ChainDef2 is the only chain system"
```

---

### Task 7: Flash and verify on hardware

**Files:** None

- [ ] **Step 1: Build and flash**

```bash
just firmware
rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave
```

- [ ] **Step 2: Verify Part chain navigation**

- Press B1 → Part 1 chain (FM Poly). Minus/Plus traverses blocks. Seq/Edit for FM sub-pages.
- Press B2 → Part 2 chain (also FM Poly for now — all Parts default to same chain).
- Press same button again → snaps home.

- [ ] **Step 3: Verify mixer channel navigation**

- Hold MIX + press B1 → Mixer CH1 chain (Channel → MIDI → EQ → Sends).
- Minus/Plus traverses mixer blocks.
- Hold MIX + press B2 → Mixer CH2.

- [ ] **Step 4: Verify system chain**

- Press MENU → System chain (MIDI Setup → Tuning → Theme → Updates → About).
- Minus/Plus traverses system blocks.

- [ ] **Step 5: Verify demo storyboard**

- Hold MIX + press B6 → Demo chain (Waves → Shapes → Motion).

- [ ] **Step 6: Verify audio still plays**

440 Hz sine from DMA — unaffected by navigation changes.

- [ ] **Step 7: Commit binary**

```bash
git add chimera.bin
git commit -m "feat(core): site map navigation — verified on hardware"
```
