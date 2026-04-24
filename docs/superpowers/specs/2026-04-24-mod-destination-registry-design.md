# Mod Destination Registry Design

## Overview

Replace the fixed `(block_idx, param_idx)` mod destination scheme with a dynamic destination registry. Users prime any parameter for modulation at runtime. Each destination carries a `ParamPath` that uniquely identifies the parameter regardless of UI context (which operator is selected, etc.). The registry is stored per-patch and restored on load.

## Problem

The current system identifies mod destinations as `(block_idx, param_idx)` where `param_idx` = encoder position (0-5). This breaks when a single encoder represents different parameters depending on context — e.g., the FM operator focus page uses encoder C for "Level" but it could be Op1 Level, Op2 Level, Op3 Level, or Op4 Level depending on which operator is selected.

Additionally, `mod_enabled` (a `u64` bitfield) lives on `UiState`, not on the `Patch`. Loading a patch doesn't restore which params were primed for modulation.

## Design

### ParamPath

A compact enum that uniquely identifies any modulatable parameter in the system. Each variant encodes the full context needed to read/write the parameter.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamPath {
    /// Standard chain block param: block node index + encoder index
    Block { block: u8, param: u8 },
    /// FM operator param: operator index (0-3) + param within operator
    FmOp { op: u8, param: u8 },
    /// FM operator envelope param: operator index (0-3) + envelope param
    FmEnv { op: u8, param: u8 },
    // Future:
    // Grain { grain: u8, param: u8 },
    // Partial { partial: u8, param: u8 },
}
```

New variants are added as new block types are created. Existing patches with old variants remain valid — forward compatible.

### ModDest

A primed destination entry in the registry.

```rust
#[derive(Clone, Copy, Debug)]
pub struct ModDest {
    pub path: ParamPath,
    pub label: [u8; 8],  // short display label, e.g., "O1 Lvl\0\0"
}
```

The label is generated at prime-time from the current page context (block name + param name + operator index if applicable).

### Destination Registry

Replaces the `mod_enabled: u64` bitfield. A fixed-size array of primed destinations, stored per-patch.

```rust
pub const MAX_DESTS: usize = 16;

pub struct ModDestRegistry {
    pub dests: [Option<ModDest>; MAX_DESTS],
    pub count: usize,
}
```

Methods:
- `add(path, label)` — add a destination if not already present, return index
- `remove(path)` — remove by path match
- `find(path) -> Option<usize>` — find index by path
- `is_primed(path) -> bool` — check if a path is in the registry

### ModState Changes

The compact audio-thread `ModState` changes:

```rust
pub struct ModState {
    pub num_sources: usize,
    pub num_dests: usize,
    pub dests: [ParamPath; MAX_MOD_DESTS],   // was (u8, u8)
    pub amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
}
```

Destinations are now `ParamPath` instead of `(block_idx, param_idx)`. The audio thread resolves each path to the actual parameter for offset computation.

### Patch Storage

`ModDestRegistry` is stored in the `Patch` struct (replaces the implicit `mod_enabled` state):

```rust
pub struct Patch {
    pub name: [u8; 16],
    pub chain_type: ChainType,
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,  // NEW
}
```

Loading a patch restores the full registry — primed destinations appear in the mod matrix immediately.

### MatrixState Changes

`MatrixState` no longer owns `mod_enabled: u64`. Instead, it reads from the patch's `ModDestRegistry` to populate the destination columns. The `rebuild_dests_from_chain` method is replaced by `rebuild_dests_from_registry`.

### UI Flow

**Priming a parameter:**
1. User is on any page, touches encoder (sets `last_encoder`)
2. Holds Mix + presses Plus
3. System builds a `ParamPath` from the current context:
   - On a regular block page: `ParamPath::Block { block: nav.node, param: encoder_idx }`
   - On FM operator focus page: `ParamPath::FmOp { op: selected_op, param: encoder_idx }`
   - On FM envelope sub-page: `ParamPath::FmEnv { op: env_index, param: encoder_idx }`
4. Generates a display label from the block/param names
5. Adds to the patch's `ModDestRegistry`
6. Rebuilds the mod matrix display

**Un-priming:**
Same flow but Mix + Minus removes the path from the registry.

**Mod bar indicator:**
On the cell grid, a param shows a mod bar if `dest_registry.is_primed(current_param_path)`. The path is computed from the current page context, so Op1 Level shows a bar but Op2 Level doesn't (unless also primed).

### Audio Thread Resolution

`compute_offset()` changes from matching `(block_idx, param_idx)` to matching `ParamPath`. The voice's render loop resolves each destination path to the actual parameter value to apply the offset.

For `ParamPath::Block`, this maps directly to the existing block-based offset application. For `ParamPath::FmOp`, the voice applies the offset to the specific operator's parameter.

### FM Init Patch Pre-wiring

The FM init patch pre-populates:
- 4 sources: E1, E2, E3, E4
- 4 destinations: `FmOp { op: 0, param: 2 }` (Op1 Level), etc.
- 4 routes: E1→Op1Level at amount=127, E2→Op2Level, etc.
- `dest_registry` contains the 4 operator level entries

## Scope

### In Scope
- `ParamPath` enum with Block, FmOp, FmEnv variants
- `ModDest` and `ModDestRegistry` types
- Replace `mod_enabled: u64` with registry
- Update ModState to use ParamPath
- Update prime/un-prime flow to build ParamPath from context
- Update mod bar indicator to check registry
- Update audio thread offset resolution
- FM init patch pre-wiring
- Store/restore registry with patch

### Out of Scope
- New modulator types (S&H, function generators)
- User-designable chains
- Mod amount per-step (sequencer)
