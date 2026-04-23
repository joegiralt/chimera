# Preset System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement an Elektron-style sound pool with copy-on-load patch management, per-track parameter isolation, and a patch browser UI.

**Architecture:** A `SoundPool` of 32 `Patch` slots lives in RAM. Six `Track`s each own an independent `Patch` (copied from pool on load). The audio thread reads from track-owned `ParamSnapshot`/`ModState` via existing raw pointer mechanism. Patch browser UI is triggered by double-tap on B1-B6.

**Tech Stack:** Rust `no_std`, chimera-core (portable logic), chimera-stm32 (hardware target), existing `ParamSnapshot`/`ModState`/`ChainNav` types.

**Spec:** `docs/superpowers/specs/2026-04-23-preset-system-design.md`

---

## File Structure

### New files
- `chimera-core/src/preset.rs` — `Patch`, `SoundPool`, `Track`, `Project`, `ChainType` types + init defaults
- `chimera-core/tests/preset_test.rs` — unit tests for preset data model

### Modified files
- `chimera-core/src/lib.rs` — add `pub mod preset;`
- `chimera-core/src/ui/mod.rs` — replace single `params`/`mod_state` with `Project`, wire track selection
- `chimera-core/src/ui/chain.rs` — `ChainId::Part(i)` resolves to track's chain type instead of hardcoded PIZZA_POLY_CHAIN
- `chimera-stm32/src/main.rs` — create `Project`, pass per-track params to audio
- `chimera-stm32/src/audio.rs` — support per-track param pointers (6 voices, 6 param sources)

---

## Task 1: Patch and SoundPool data types

**Files:**
- Create: `chimera-core/src/preset.rs`
- Create: `chimera-core/tests/preset_test.rs`
- Modify: `chimera-core/src/lib.rs`

- [ ] **Step 1: Write failing tests for Patch and SoundPool**

```rust
// chimera-core/tests/preset_test.rs
use chimera_core::preset::{Patch, SoundPool, ChainType};

#[test]
fn patch_init_has_musically_useful_defaults() {
    let p = Patch::init(ChainType::PizzaPoly);
    assert_eq!(p.chain_type, ChainType::PizzaPoly);
    assert!(p.params.volume.value() > 0.0);
    assert!(p.params.filter.cutoff.value() > 1000.0);
    // Name should be "(init)"
    assert!(p.name_str().starts_with("(init)"));
}

#[test]
fn sound_pool_starts_empty() {
    let pool = SoundPool::new();
    assert!(pool.get(0).is_none());
    assert!(pool.get(31).is_none());
}

#[test]
fn sound_pool_store_and_retrieve() {
    let mut pool = SoundPool::new();
    let patch = Patch::init(ChainType::PizzaPoly);
    pool.store(0, patch);
    assert!(pool.get(0).is_some());
    assert_eq!(pool.get(0).unwrap().chain_type, ChainType::PizzaPoly);
}

#[test]
fn sound_pool_slot_count() {
    let pool = SoundPool::new();
    assert_eq!(pool.slot_count(), 32);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `just test`
Expected: FAIL — `preset` module doesn't exist

- [ ] **Step 3: Implement Patch, ChainType, SoundPool**

```rust
// chimera-core/src/preset.rs
use crate::params::ParamSnapshot;
use crate::modulation::ModState;

pub const POOL_SIZE: usize = 32;
pub const NAME_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ChainType {
    #[default]
    PizzaPoly = 0,
    Modal = 1,
    Fm = 2,
}

#[derive(Clone)]
#[repr(C)]  // serialization-ready layout for future SD card persistence
pub struct Patch {
    pub name: [u8; NAME_LEN],
    pub chain_type: ChainType,
    pub params: ParamSnapshot,
    pub mod_state: ModState,
}

impl Patch {
    /// ParamSnapshot::default() already provides musically useful values:
    /// volume=0.8, filter cutoff=20kHz, tuned envelopes, FX bypassed.
    pub fn init(chain_type: ChainType) -> Self {
        let mut name = [0u8; NAME_LEN];
        let tag = b"(init)";
        name[..tag.len()].copy_from_slice(tag);
        Self {
            name,
            chain_type,
            params: ParamSnapshot::default(),
            mod_state: ModState::default(),
        }
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        core::str::from_utf8(&self.name[..end]).unwrap_or("???")
    }
}

pub struct SoundPool {
    slots: [Option<Patch>; POOL_SIZE],
}

impl SoundPool {
    pub fn new() -> Self {
        Self { slots: core::array::from_fn(|_| None) }
    }

    pub fn get(&self, index: usize) -> Option<&Patch> {
        self.slots.get(index)?.as_ref()
    }

    pub fn store(&mut self, index: usize, patch: Patch) {
        if index < POOL_SIZE {
            self.slots[index] = Some(patch);
        }
    }

    pub fn clear(&mut self, index: usize) {
        if index < POOL_SIZE {
            self.slots[index] = None;
        }
    }

    pub fn slot_count(&self) -> usize { POOL_SIZE }
}
```

Add `pub mod preset;` to `chimera-core/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `just test`
Expected: all preset tests PASS

- [ ] **Step 5: Commit**

```
feat(core): add Patch, ChainType, SoundPool data types
```

---

## Task 2: Track and Project types

**Files:**
- Modify: `chimera-core/src/preset.rs`
- Modify: `chimera-core/tests/preset_test.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn track_starts_with_init_patch() {
    let track = Track::new(ChainType::PizzaPoly);
    assert_eq!(track.patch.chain_type, ChainType::PizzaPoly);
    assert!(track.loaded_from.is_none());
}

#[test]
fn track_load_from_pool_copies() {
    let mut pool = SoundPool::new();
    let mut patch = Patch::init(ChainType::PizzaPoly);
    patch.name = *b"Acid Bass\0\0\0\0\0\0\0";
    pool.store(3, patch);

    let mut track = Track::new(ChainType::PizzaPoly);
    track.load_from_pool(&pool, 3);

    assert_eq!(track.patch.name_str(), "Acid Bass");
    assert_eq!(track.loaded_from, Some(3));
}

#[test]
fn track_edit_does_not_modify_pool() {
    let mut pool = SoundPool::new();
    pool.store(0, Patch::init(ChainType::PizzaPoly));

    let mut track = Track::new(ChainType::PizzaPoly);
    track.load_from_pool(&pool, 0);
    track.patch.params.volume = Param::new(0.0, 0.0, 1.0); // mute

    // Pool slot unchanged
    assert!(pool.get(0).unwrap().params.volume.value() > 0.0);
}

#[test]
fn track_save_to_pool_overwrites() {
    let mut pool = SoundPool::new();
    pool.store(5, Patch::init(ChainType::PizzaPoly));

    let mut track = Track::new(ChainType::Modal);
    track.patch.name = *b"My Sound\0\0\0\0\0\0\0\0";
    track.save_to_pool(&mut pool, 5);

    assert_eq!(pool.get(5).unwrap().name_str(), "My Sound");
    assert_eq!(pool.get(5).unwrap().chain_type, ChainType::Modal);
}

#[test]
fn project_has_six_tracks() {
    let project = Project::new();
    assert_eq!(project.tracks.len(), 6);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `just test`
Expected: FAIL — `Track`, `Project` don't exist

- [ ] **Step 3: Implement Track and Project**

```rust
// Add to chimera-core/src/preset.rs

pub struct Track {
    pub patch: Patch,
    pub loaded_from: Option<u8>,
}

impl Track {
    pub fn new(chain_type: ChainType) -> Self {
        Self {
            patch: Patch::init(chain_type),
            loaded_from: None,
        }
    }

    pub fn load_from_pool(&mut self, pool: &SoundPool, slot: usize) {
        if let Some(p) = pool.get(slot) {
            self.patch = p.clone();
            self.loaded_from = Some(slot as u8);
        }
    }

    pub fn save_to_pool(&self, pool: &mut SoundPool, slot: usize) {
        pool.store(slot, self.patch.clone());
    }
}

pub struct MixerState {
    pub levels: [f32; 6],
    pub pans: [f32; 6],
    pub sends: [f32; 6],  // FX send level per track
}

impl Default for MixerState {
    fn default() -> Self {
        Self {
            levels: [0.8; 6],
            pans: [0.0; 6],
            sends: [0.0; 6],
        }
    }
}

pub struct Project {
    pub name: [u8; NAME_LEN],
    pub pool: SoundPool,
    pub tracks: [Track; 6],
    pub mixer: MixerState,
}

impl Project {
    pub fn new() -> Self {
        Self {
            name: *b"New Project\0\0\0\0\0",
            pool: SoundPool::new(),
            tracks: core::array::from_fn(|_| Track::new(ChainType::PizzaPoly)),
            mixer: MixerState::default(),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `just test`
Expected: all preset tests PASS

- [ ] **Step 5: Commit**

```
feat(core): add Track, Project, MixerState types
```

---

## Task 3: Wire UiState to use Project and per-track params

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/src/ui/chain.rs`

This task replaces the single `params`/`mod_state` in `UiState` with a `Project` that holds 6 tracks. The currently-navigated `Part(i)` determines which track's params are active.

- [ ] **Step 1: Read current UiState and understand the param flow**

Read `chimera-core/src/ui/mod.rs` lines 1-120 to understand how `self.params` and `self.mod_state` are currently used throughout the UI.

- [ ] **Step 2: Replace UiState.params/mod_state with Project**

In `chimera-core/src/ui/mod.rs`:
- Replace `pub params: ParamSnapshot` and `pub mod_state: ModState` with `pub project: Project`
- Add `pub active_track: usize` (0-5, derived from `nav.chain_id`)
- Add accessor methods: `fn params(&self) -> &ParamSnapshot` and `fn params_mut(&mut self) -> &mut ParamSnapshot` that return the active track's params
- Similarly for `mod_state()` / `mod_state_mut()`
- Update all internal references from `self.params` to `self.params()` / `self.params_mut()`

- [ ] **Step 3: Update ChainNav to resolve Part(i) chain type from track**

In `chimera-core/src/ui/chain.rs`, update `chain_def_for()` (or equivalent) so `Part(i)` resolves to the track's `chain_type` instead of always returning `PIZZA_POLY_CHAIN`.

- [ ] **Step 4: Fix all compilation errors in chimera-core**

Run: `cargo check -p chimera-core`
Fix any references to `ui.params` / `ui.mod_state` that need updating.

- [ ] **Step 5: Run tests**

Run: `just test`
Expected: all existing tests PASS (behavior unchanged — all tracks default to PizzaPoly init)

- [ ] **Step 6: Commit**

```
refactor(core): UiState uses Project with per-track params
```

---

## Task 4: Wire stm32 main and audio to per-track params

**Files:**
- Modify: `chimera-stm32/src/main.rs`
- Modify: `chimera-stm32/src/audio.rs`

- [ ] **Step 1: Update main.rs to create Project and pass active track params**

Replace `&ui.params` / `&ui.mod_state` with `&ui.project.tracks[0].patch.params` / `&ui.project.tracks[0].patch.mod_state` (track 0 for now — single voice).

- [ ] **Step 2: Build firmware**

Run: `just firmware`
Expected: builds with no errors

- [ ] **Step 3: Flash and verify**

Run: `just flash`
Expected: synth boots and plays exactly as before (same init patch, same sound)

- [ ] **Step 4: Commit**

```
feat(stm32): wire audio to per-track params from Project
```

---

## Task 5: Patch browser UI — double-tap detection

**Files:**
- Modify: `chimera-core/src/ui/mod.rs` (or new `chimera-core/src/ui/browser.rs` if cleaner)
- Modify: `chimera-core/src/ui/mod.rs` (double-tap detection in handle_input)

- [ ] **Step 1: Add double-tap detection for B1-B6 buttons**

Track last-press time per button. If second press within 300ms, emit a `DoubleTap(part_index)` event. This can live in `UiState::handle_input()`.

- [ ] **Step 2: Add browser state to UiState**

```rust
pub enum UiMode {
    Normal,
    PatchBrowser { track: usize, cursor: usize, scroll: usize },
}
```

Double-tap transitions `ui_mode` from `Normal` to `PatchBrowser { track: i, cursor: 0, scroll: 0 }`.

- [ ] **Step 3: Test double-tap detection**

Write a test that simulates two rapid button presses and verifies `ui_mode` transitions to `PatchBrowser`.

- [ ] **Step 4: Commit**

```
feat(core): double-tap B1-B6 opens patch browser
```

---

## Task 6: Patch browser UI — rendering and selection

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/src/ui/mod.rs` — `render()` / `render_dirty()` dispatch to browser renderer when `ui_mode == PatchBrowser`
- Reference: `chimera-core/src/ui/block_def.rs` for existing page rendering patterns (`PageLayout`, `Renderer`)

- [ ] **Step 1: Render patch browser page**

When `ui_mode == PatchBrowser`, `render()` draws a list of the 32 pool slots instead of the normal chain page. Use the existing `Renderer` and `embedded_graphics` drawing primitives:
- Each row: `[slot#] [name] [chain type]`
- Highlight current cursor position
- Encoder scrolls cursor
- Empty slots show "(empty)"

- [ ] **Step 2: Implement selection (encoder press)**

Encoder press in browser mode:
1. Copy selected pool slot into the track (`track.load_from_pool()`)
2. Transition back to `UiMode::Normal`
3. Navigate to the loaded track's chain

- [ ] **Step 3: Implement cancel (back button or double-tap again)**

Pressing back or the same B-button exits browser without loading.

- [ ] **Step 4: Implement init option**

Add a special entry at the end of the list: "(init) [chain type]". Selecting it calls `Track::new()` to reset to defaults.

- [ ] **Step 5: Flash and test on hardware**

Verify: double-tap B1 → browser shows → scroll → select → sound changes → back to chain view.

- [ ] **Step 6: Commit**

```
feat(core): patch browser UI with load and init
```

---

## Task 7: Save track to pool slot

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`

- [ ] **Step 1: Add save-to-pool action in browser**

When in patch browser, a long-press (or dedicated action) on a slot saves the current track's patch into that pool slot via `track.save_to_pool()`.

- [ ] **Step 2: Visual feedback**

Briefly flash the slot name or show a confirmation indicator.

- [ ] **Step 3: Test on hardware**

Verify: edit a sound → open browser → save to slot 5 → slot 5 now shows the edited name/params.

- [ ] **Step 4: Commit**

```
feat(core): save track patch to sound pool slot
```

---

## Task 8: Desktop simulator compatibility

**Files:**
- Modify: `chimera-desktop/src/main.rs`

- [ ] **Step 1: Update desktop sim to use Project**

Mirror the stm32 changes: create `Project` via `UiState`, pass track params.

- [ ] **Step 2: Verify desktop sim runs**

Run: `just desktop`
Expected: simulator opens, synth works as before

- [ ] **Step 3: Commit**

```
feat(desktop): update simulator for Project-based params
```
