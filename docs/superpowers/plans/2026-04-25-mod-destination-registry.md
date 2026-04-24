# Mod Destination Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fixed `(block_idx, param_idx)` mod destinations with a dynamic `ParamPath`-based registry that supports multi-instance blocks (FM operators) and persists per-patch.

**Architecture:** `ParamPath` enum uniquely identifies any modulatable param. `ModDestRegistry` (stored in `Patch`) holds the user-primed destinations. `ModState` uses `ParamPath` for audio-thread offset resolution. `MatrixState` reads from the registry instead of a bitfield.

**Tech Stack:** Rust `no_std`, existing `Param`/`ParamSnapshot`/`ModState`/`MatrixState` types.

**Spec:** `docs/superpowers/specs/2026-04-24-mod-destination-registry-design.md`

---

## File Structure

### New files
- `chimera-core/src/mod_path.rs` — `ParamPath` enum, `ModDest`, `ModDestRegistry`, `resolve_param_mut`

### Modified files
- `chimera-core/src/lib.rs` — add `pub mod mod_path;`
- `chimera-core/src/modulation.rs` — `ModState.dests` changes from `(u8, u8)` to `ParamPath`, `compute_offset` takes `ParamPath`
- `chimera-core/src/ui/mod_grid.rs` — `ModDest` uses `ParamPath` instead of `(block_idx, param_idx)`, remove `mod_enabled: u64`, add `rebuild_dests_from_registry`
- `chimera-core/src/ui/mod.rs` — prime/un-prime builds `ParamPath` from page context, mod bar checks registry
- `chimera-core/src/dsp/voice.rs` — `compute_offset` calls use `ParamPath` instead of `(block, param)`
- `chimera-core/src/preset.rs` — `Patch` gains `dest_registry: ModDestRegistry`, FM init patch pre-wires 4 destinations
- `chimera-core/tests/fm_test.rs` — tests for registry + priming

---

## Task 1: ParamPath and ModDestRegistry types

**Files:**
- Create: `chimera-core/src/mod_path.rs`
- Create or modify: `chimera-core/tests/mod_registry_test.rs`
- Modify: `chimera-core/src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
use chimera_core::mod_path::{ParamPath, ModDestEntry, ModDestRegistry};

#[test]
fn registry_starts_empty() {
    let reg = ModDestRegistry::new();
    assert_eq!(reg.count, 0);
}

#[test]
fn registry_add_and_find() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::Block { block: 1, param: 0 };
    reg.add(path, *b"DrvDrv\0\0");
    assert_eq!(reg.count, 1);
    assert!(reg.is_primed(path));
    assert_eq!(reg.find(path), Some(0));
}

#[test]
fn registry_no_duplicates() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::FmOp { op: 0, param: 2 };
    reg.add(path, *b"O1 Lvl\0\0");
    reg.add(path, *b"O1 Lvl\0\0"); // duplicate
    assert_eq!(reg.count, 1);
}

#[test]
fn registry_remove() {
    let mut reg = ModDestRegistry::new();
    let p1 = ParamPath::FmOp { op: 0, param: 2 };
    let p2 = ParamPath::FmOp { op: 1, param: 2 };
    reg.add(p1, *b"O1 Lvl\0\0");
    reg.add(p2, *b"O2 Lvl\0\0");
    assert_eq!(reg.count, 2);
    reg.remove(p1);
    assert_eq!(reg.count, 1);
    assert!(!reg.is_primed(p1));
    assert!(reg.is_primed(p2));
}

#[test]
fn registry_fm_op_paths_are_distinct() {
    let p0 = ParamPath::FmOp { op: 0, param: 2 };
    let p1 = ParamPath::FmOp { op: 1, param: 2 };
    assert_ne!(p0, p1);
    let mut reg = ModDestRegistry::new();
    reg.add(p0, *b"O1 Lvl\0\0");
    assert!(reg.is_primed(p0));
    assert!(!reg.is_primed(p1));
}
```

- [ ] **Step 2: Run tests — verify fail**

- [ ] **Step 3: Implement mod_path.rs**

```rust
pub const MAX_REGISTRY_DESTS: usize = 32;
pub const LABEL_LEN: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamPath {
    Block { block: u8, param: u8 },
    FmOp { op: u8, param: u8 },
    FmEnv { op: u8, param: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct ModDestEntry {
    pub path: ParamPath,
    pub label: [u8; LABEL_LEN],
}

pub struct ModDestRegistry {
    pub entries: [Option<ModDestEntry>; MAX_REGISTRY_DESTS],
    pub count: usize,
}

impl ModDestRegistry {
    pub const fn new() -> Self { ... }
    pub fn add(&mut self, path: ParamPath, label: [u8; LABEL_LEN]) { ... }
    pub fn remove(&mut self, path: ParamPath) { ... }
    pub fn find(&self, path: ParamPath) -> Option<usize> { ... }
    pub fn is_primed(&self, path: ParamPath) -> bool { ... }
    pub fn get(&self, index: usize) -> Option<&ModDestEntry> { ... }
}
```

Add `pub mod mod_path;` to lib.rs.

- [ ] **Step 4: Run tests — verify pass**
- [ ] **Step 5: Commit** `feat(core): ParamPath and ModDestRegistry types`

---

## Task 2: Update ModState to use ParamPath

**Files:**
- Modify: `chimera-core/src/modulation.rs`
- Modify: `chimera-core/tests/mod_registry_test.rs`

- [ ] **Step 1: Write failing tests**

```rust
use chimera_core::modulation::ModState;
use chimera_core::mod_path::ParamPath;

#[test]
fn mod_state_compute_offset_with_param_path() {
    let mut ms = ModState::new();
    ms.num_sources = 1;
    ms.num_dests = 1;
    ms.dests[0] = ParamPath::FmOp { op: 0, param: 2 };
    ms.amounts[0][0] = 127; // full amount
    let sources = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let offset = ms.compute_offset(&sources, ParamPath::FmOp { op: 0, param: 2 });
    assert!((offset - 1.0).abs() < 0.01);
}

#[test]
fn mod_state_no_offset_for_different_path() {
    let mut ms = ModState::new();
    ms.num_sources = 1;
    ms.num_dests = 1;
    ms.dests[0] = ParamPath::FmOp { op: 0, param: 2 };
    ms.amounts[0][0] = 127;
    let sources = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    // Different operator — should return 0
    let offset = ms.compute_offset(&sources, ParamPath::FmOp { op: 1, param: 2 });
    assert_eq!(offset, 0.0);
}
```

- [ ] **Step 2: Change ModState.dests from `(u8, u8)` to `ParamPath`**

```rust
pub struct ModState {
    pub num_sources: usize,
    pub num_dests: usize,
    pub dests: [ParamPath; MAX_MOD_DESTS],
    pub amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
}
```

Update `compute_offset` to take `ParamPath` instead of `(block_idx, param_idx)`:
```rust
pub fn compute_offset(&self, source_values: &[f32; MAX_MOD_SOURCES], path: ParamPath) -> f32
```

Update `sync_from_matrix` to copy `ParamPath` from the new `ModDest` type.

Default `dests` to `[ParamPath::Block { block: 0, param: 0 }; MAX_MOD_DESTS]`.

- [ ] **Step 3: Fix all compilation errors**

Run: `cargo check -p chimera-core`
Fix all call sites of `compute_offset` — they currently pass `(block, param)` and need to pass `ParamPath::Block { block, param }`.

- [ ] **Step 4: Run tests — verify pass**
- [ ] **Step 5: Commit** `refactor(core): ModState uses ParamPath for destinations`

---

## Task 3: Update MatrixState to use registry

**Files:**
- Modify: `chimera-core/src/ui/mod_grid.rs`

- [ ] **Step 1: Replace ModDest and mod_enabled**

Change `ModDest` to use `ParamPath`:
```rust
pub struct ModDest {
    pub path: ParamPath,
    pub label: [u8; 8],
}
```

Remove `mod_enabled: u64` from `MatrixState`.

Replace `rebuild_dests_from_chain` with `rebuild_dests_from_registry`:
```rust
pub fn rebuild_dests_from_registry(&mut self, registry: &ModDestRegistry) {
    self.num_dests = 0;
    for i in 0..registry.count {
        if let Some(entry) = registry.get(i) {
            if self.num_dests < MAX_DESTS {
                self.dests[self.num_dests] = Some(ModDest {
                    path: entry.path,
                    label: entry.label,
                });
                self.num_dests += 1;
            }
        }
    }
}
```

Remove `set_mod_enabled`, `is_mod_enabled`, `rebuild_dests`.

- [ ] **Step 2: Fix all compilation errors**

Run: `cargo check -p chimera-core`
Update `sync_from_matrix` in modulation.rs to copy `ParamPath` from new `ModDest`.
Update all callers of `rebuild_dests_from_chain`, `set_mod_enabled`, etc.

- [ ] **Step 3: Run tests — verify pass**
- [ ] **Step 4: Commit** `refactor(core): MatrixState uses ModDestRegistry, remove mod_enabled`

---

## Task 4: Update Patch to store registry

**Files:**
- Modify: `chimera-core/src/preset.rs`

- [ ] **Step 1: Add ModDestRegistry to Patch**

```rust
pub struct Patch {
    pub name: [u8; NAME_LEN],
    pub chain_type: ChainType,
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,
}
```

Update `Patch::init()` — for FM chain, pre-wire 4 destinations:
```rust
ChainType::Fm => {
    params.engine = EngineType::Fm;
    let mut ms = ModState::new();
    ms.num_sources = 4;
    ms.num_dests = 4;
    ms.dests[0] = ParamPath::FmOp { op: 0, param: 2 }; // Op1 Level
    ms.dests[1] = ParamPath::FmOp { op: 1, param: 2 }; // Op2 Level
    ms.dests[2] = ParamPath::FmOp { op: 2, param: 2 }; // Op3 Level
    ms.dests[3] = ParamPath::FmOp { op: 3, param: 2 }; // Op4 Level
    ms.amounts[0][0] = 127; // E1 → Op1 Level full
    ms.amounts[1][1] = 127;
    ms.amounts[2][2] = 127;
    ms.amounts[3][3] = 127;
    // Pre-populate registry
    let mut reg = ModDestRegistry::new();
    reg.add(ParamPath::FmOp { op: 0, param: 2 }, *b"O1 Lvl\0\0");
    reg.add(ParamPath::FmOp { op: 1, param: 2 }, *b"O2 Lvl\0\0");
    reg.add(ParamPath::FmOp { op: 2, param: 2 }, *b"O3 Lvl\0\0");
    reg.add(ParamPath::FmOp { op: 3, param: 2 }, *b"O4 Lvl\0\0");
    (ms, reg)
}
```

Update `Track::load_from_pool` and `Track::save_to_pool` to include registry.

- [ ] **Step 2: Fix compilation errors + run tests**
- [ ] **Step 3: Commit** `feat(core): Patch stores ModDestRegistry, FM init pre-wired`

---

## Task 5: Update prime/un-prime UI flow

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`

- [ ] **Step 1: Build ParamPath from page context**

Replace the MIX+Plus/Minus handler. Instead of `set_mod_enabled(block_idx, param_idx)`:

```rust
if shift {
    if controls.button_state(ButtonId::Plus) == ButtonState::Pressed {
        let path = self.current_param_path();
        let label = self.current_param_label();
        self.project.tracks[at].patch.dest_registry.add(path, label);
        self.matrix_state.rebuild_dests_from_registry(
            &self.project.tracks[at].patch.dest_registry
        );
        self.project.tracks[at].patch.mod_state.sync_from_matrix(&self.matrix_state);
    }
    if controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
        let path = self.current_param_path();
        self.project.tracks[at].patch.dest_registry.remove(path);
        self.matrix_state.rebuild_dests_from_registry(
            &self.project.tracks[at].patch.dest_registry
        );
        self.project.tracks[at].patch.mod_state.sync_from_matrix(&self.matrix_state);
    }
}
```

Add `current_param_path(&self) -> ParamPath`:
```rust
fn current_param_path(&self) -> ParamPath {
    match self.page {
        PageId::FmOp => ParamPath::FmOp {
            op: fm_selected_op(),
            param: self.last_encoder as u8,
        },
        PageId::FmEnv1 => ParamPath::FmEnv { op: 0, param: self.last_encoder as u8 },
        PageId::FmEnv2 => ParamPath::FmEnv { op: 1, param: self.last_encoder as u8 },
        PageId::FmEnv3 => ParamPath::FmEnv { op: 2, param: self.last_encoder as u8 },
        PageId::FmEnv4 => ParamPath::FmEnv { op: 3, param: self.last_encoder as u8 },
        _ => ParamPath::Block {
            block: self.nav.node as u8,
            param: self.last_encoder as u8,
        },
    }
}
```

Add `current_param_label(&self) -> [u8; 8]` that builds a short label from block short name + param label.

- [ ] **Step 2: Update mod bar indicator**

In the cell grid rendering, a param shows a mod bar if the current `ParamPath` for that cell is primed in the registry. This requires computing the path for each encoder position on the current page.

- [ ] **Step 3: Update patch load to rebuild matrix from registry**

When loading a patch (in the browser Edit handler), after restoring the patch:
```rust
self.matrix_state.rebuild_dests_from_registry(&self.project.tracks[sel_track].patch.dest_registry);
```

- [ ] **Step 4: Fix compilation + run tests**
- [ ] **Step 5: Commit** `feat(core): prime/un-prime uses ParamPath, mod bars context-aware`

---

## Task 6: Update audio thread offset resolution

**Files:**
- Modify: `chimera-core/src/dsp/voice.rs`

- [ ] **Step 1: Change compute_offset calls to use ParamPath**

Replace all:
```rust
mod_state.compute_offset(&mod_values, 1, 0)  // Drive.drive
```
with:
```rust
mod_state.compute_offset(&mod_values, ParamPath::Block { block: 1, param: 0 })
```

For FM engine: add modulation offset application for FM operator params:
```rust
EngineType::Fm => {
    let mut mod_fm = params.fm;
    for op in 0..4u8 {
        let level_offset = mod_state.compute_offset(
            &mod_values, ParamPath::FmOp { op, param: 2 }
        );
        if level_offset != 0.0 {
            mod_fm.operators[op as usize].level.apply_mod_offset(level_offset);
        }
    }
    self.fm.render_params(output, &mod_fm);
}
```

- [ ] **Step 2: Run tests**
- [ ] **Step 3: Build firmware**

Run: `just firmware`

- [ ] **Step 4: Commit** `feat(core): audio thread resolves ParamPath for mod offsets`

---

## Task 7: Integration tests + hardware test

**Files:**
- Modify: `chimera-core/tests/mod_registry_test.rs`

- [ ] **Step 1: Write integration tests**

```rust
#[test]
fn fm_init_patch_has_primed_destinations() {
    let patch = Patch::init(ChainType::Fm);
    assert_eq!(patch.dest_registry.count, 4);
    assert!(patch.dest_registry.is_primed(ParamPath::FmOp { op: 0, param: 2 }));
    assert!(patch.dest_registry.is_primed(ParamPath::FmOp { op: 1, param: 2 }));
}

#[test]
fn fm_mod_state_has_prewired_routes() {
    let patch = Patch::init(ChainType::Fm);
    assert_eq!(patch.mod_state.num_dests, 4);
    assert_eq!(patch.mod_state.dests[0], ParamPath::FmOp { op: 0, param: 2 });
    assert_eq!(patch.mod_state.amounts[0][0], 127);
}

#[test]
fn prime_different_ops_creates_distinct_dests() {
    let mut reg = ModDestRegistry::new();
    reg.add(ParamPath::FmOp { op: 0, param: 2 }, *b"O1 Lvl\0\0");
    reg.add(ParamPath::FmOp { op: 1, param: 2 }, *b"O2 Lvl\0\0");
    assert_eq!(reg.count, 2);
    assert!(reg.is_primed(ParamPath::FmOp { op: 0, param: 2 }));
    assert!(reg.is_primed(ParamPath::FmOp { op: 1, param: 2 }));
    assert!(!reg.is_primed(ParamPath::FmOp { op: 2, param: 2 }));
}
```

- [ ] **Step 2: Run all tests**

Run: `just test`

- [ ] **Step 3: Build + flash firmware**

Run: `just firmware` then flash

- [ ] **Step 4: Hardware test**

- Load FM init patch
- Navigate to mod matrix → verify E1-E4 sources, Op1-Op4 Level destinations visible
- Navigate to operator focus page → select Op1 → touch Level → Mix+Plus → verify mod bar appears
- Select Op2 → verify mod bar disappears (Op2 Level not primed)
- Mix+Plus to prime Op2 Level → mod bar appears
- Navigate to mod matrix → verify 5 destinations now (4 pre-wired + 1 new)

- [ ] **Step 5: Commit** `feat(core): mod destination registry — complete integration`
