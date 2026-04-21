# Chimera Modulation Architecture

## Concept

Modulation is a first-class system that connects modulators (LFOs, envelopes, velocity, etc.) to destinations (any block parameter with a modulation input). It works like Eurorack: each modulatable parameter has an input jack and an attenuverter (amount control, positive or negative).

The mod matrix is a global view of all connections. Each block page is a local view showing what's modulating its parameters. Each modulator page shows where it's routed. Modulation is never hidden.

## Architecture

### Modulation Destinations

A block declares which of its parameters are modulatable by marking them in the `ParamSlot` definition. Not every parameter needs modulation — discrete selectors (algorithm, filter mode) typically don't. Continuous parameters (cutoff, drive, fold) do.

```rust
pub struct ParamSlot {
    pub label: &'static str,
    pub format: ValFmt,
    pub icon: CellIcon,
    pub modulatable: bool,  // NEW: can this param receive modulation?
}
```

When `modulatable = true`, the parameter has an implicit modulation amount control. This amount is stored in the mod matrix, not on the block itself. The block's page shows the current modulation amount as a visual indicator (e.g., a ring around the value, a secondary bar, or a small arrow).

A destination is identified by `(block_index, param_index)` — its position in the chain.

### Modulation Sources

Sources are modulators that live in the chain. For MVP:

| Source | Type | Description |
|--------|------|-------------|
| LFO 1 | Continuous | Cyclical modulation, multiple shapes |
| LFO 2 | Continuous | Second LFO |
| Env 1 | Triggered | Amp envelope (already exists in VCA) |
| Env 2 | Triggered | Mod envelope (free-assignable) |
| Velocity | Per-note | MIDI velocity of current note |
| Mod Wheel | CC | MIDI CC1 |
| Note | Per-note | MIDI note number (for keyboard tracking) |

Sources are identified by a `ModSource` enum.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModSource {
    Lfo1,
    Lfo2,
    Env1,
    Env2,
    Velocity,
    ModWheel,
    NoteNum,
}
```

### Mod Matrix Data Structure

The mod matrix stores the amount for every legal (source, destination) pair. "Legal" means the destination exists in the current chain and is marked `modulatable`.

```rust
/// Maximum blocks per chain × max modulatable params per block
const MAX_DESTINATIONS: usize = 32;
/// Maximum modulation sources
const MAX_SOURCES: usize = 8;

/// One modulation routing: source → destination with amount.
#[derive(Clone, Copy, Debug)]
pub struct ModRoute {
    /// Which source modulates this destination
    pub source: ModSource,
    /// Block index in the chain
    pub block_idx: u8,
    /// Parameter index within the block (0-5)
    pub param_idx: u8,
    /// Modulation amount: -1.0 to +1.0 (attenuverter)
    pub amount: f32,
}

/// The mod matrix for one chain. Stores all active routes.
pub struct ModMatrix {
    pub routes: [Option<ModRoute>; 64],  // up to 64 active routes
    pub route_count: usize,
}
```

Alternatively, a flat 2D array indexed by `[source][destination]`:

```rust
/// Dense matrix: amount for every (source, destination) pair.
/// Destination index = flattened (block_idx * 6 + param_idx), filtered to modulatable only.
pub struct ModMatrix {
    /// Amounts indexed by [source_idx][dest_idx]. 0.0 = not connected.
    pub amounts: [[f32; MAX_DESTINATIONS]; MAX_SOURCES],
    /// Number of valid destinations (changes with chain)
    pub dest_count: usize,
    /// Mapping from dest_idx to (block_idx, param_idx)
    pub dest_map: [(u8, u8); MAX_DESTINATIONS],
}
```

The dense approach is simpler for the grid UI and for the DSP apply step.

### How Modulation Is Applied (DSP Side)

During Voice::render(), after the mod matrix is populated with current modulator values, each block's modulatable parameters are offset by the sum of all active modulation routes.

```
For each sample in the block:
    1. Compute all modulator outputs (LFO values, envelope values, velocity, etc.)
    2. For each active mod route:
        effective_offset = source_value * route.amount
        Add effective_offset to the destination parameter's value

    3. Run the block's process() with the modulated parameter values

    4. Restore original parameter values (modulation is per-sample, doesn't persist)
```

In practice, this means creating a "modulated copy" of the relevant parameters before passing them to each block's `process()`.

### Auto-Generation from Chain

The mod matrix page reads the chain to build its axes:

**X-axis (destinations):** Iterate through all blocks in the chain. For each block, iterate through its `BlockDef.params`. If `param.modulatable == true`, add it as a column. The column label is `"BlockShort.ParamLabel"` (e.g., "FLT.CUT", "DRV.AMT", "FLD.FOLD").

**Y-axis (sources):** All available `ModSource` variants. For MVP this is fixed (LFO1, LFO2, Env1, Env2, Vel, MW, Note). In the future, modulators could be chain blocks too, making the Y-axis dynamic.

If the chain changes (block added/removed), the matrix rebuilds. Routes to destinations that no longer exist are silently dropped.

### Visual Feedback on Block Pages

When a block page is displayed, each modulatable parameter shows its modulation state:

```
┌──────┐
│ ~◯~  │  ← icon (normal)
│CUTOFF│
│  72  │
│▓▓▓▓░░│  ← value bar
│ E1+64│  ← modulation indicator: Env1, amount +64
└──────┘
```

The modulation indicator line shows the most significant active modulation route for that parameter. If multiple sources are routed, show the one with the largest |amount|, or cycle through them.

For CellGrid layout, the indicator fits below the value bar. For BigViz layout, it appears next to the parameter value.

### Visual Feedback on Modulator Pages

When an LFO or Envelope page is displayed, the page header or a status line shows where it's routed:

```
┌────────────────────────────────┐
│ LFO 1                          │
│ → FLT.CUT +64  DRV.AMT +20   │  ← routing summary
│                                │
│ A: RATE    B: SHAPE   C: SYNC │
│ ...                            │
└────────────────────────────────┘
```

### Mod Matrix Grid Page

The grid page is the global overview. It shows all sources (rows) × all destinations (columns), with the amount at each intersection.

```
┌─────────────────────────────────────────┐
│ MOD MATRIX                               │
│                                          │
│         PIZ.SHP  DRV.AMT  FLT.CUT  FLD │
│ LFO 1  [     ]  [     ]  [ +64 ]  [   ]│
│ LFO 2  [     ]  [     ]  [     ]  [   ]│
│ Env 1  [     ]  [     ]  [ +32 ]  [   ]│
│ Env 2  [     ]  [ +20 ]  [     ]  [   ]│
│ Vel    [     ]  [     ]  [     ]  [   ]│
│ MW     [     ]  [     ]  [ +48 ]  [   ]│
│                                          │
└─────────────────────────────────────────┘
```

**Navigation within the grid:**
- Encoder A: Select row (source)
- Encoder B: Select column (destination)
- Encoder C: Set amount for selected cell (-127 to +127, displayed as -1.0 to +1.0)
- Minus/Plus: Scroll the grid if it exceeds screen width
- Seq/Edit: Switch to modulator sub-pages (LFO settings, Envelope ADSR, etc.)

**The grid only shows non-zero cells prominently.** Zero-amount cells are dimmed or empty. This keeps the grid readable even when there are many possible connections.

### Modulator Sub-Pages

Below the grid (accessed via Seq/Edit down), each modulator has a settings page:

**LFO Page:**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | RATE | Uni | LFO rate |
| B | SHAPE | Int(5) | Sine/Tri/Saw/Square/Random/S&H |
| C | SYNC | Int(1) | Free / Key sync |
| D | PHASE | Uni | Start phase |
| E | DEPTH | Uni | Global depth multiplier |
| F | DELAY | Uni | Fade-in time |

**Envelope Page:**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | ATK | Uni | Attack |
| B | DEC | Uni | Decay |
| C | SUS | Uni | Sustain |
| D | REL | Uni | Release |
| E | DEPTH | Uni | Envelope depth |
| F | VEL | Uni | Velocity sensitivity |

### What Changes in BlockDef

```rust
pub struct ParamSlot {
    pub label: &'static str,
    pub format: ValFmt,
    pub icon: CellIcon,
    pub modulatable: bool,  // NEW
}
```

Existing blocks updated with `modulatable` flags:

| Block | Modulatable Params |
|-------|-------------------|
| Pizza | shape ✓, crush ✓, level ✓ |
| Drive | drive ✓, tone ✓, mix ✗ |
| Filter | cutoff ✓, resonance ✓, mode ✗, key_track ✗ |
| Wavefolder | fold ✓, symmetry ✓, mix ✗ |
| VCA | level ✓ (via envelope — already handled) |

### What Doesn't Change

- The chain pipe model — audio still flows linearly through blocks
- Block rendering — `process()` takes parameters as before
- The dungeon map — mod matrix is still the last node
- Navigation — same buttons, same behavior

### Implementation Order

1. Add `modulatable: bool` to `ParamSlot`, update all BlockDefs
2. Create `ModSource` enum and `ModMatrix` struct in a new `chimera-core/src/modulation.rs`
3. Create LFO struct (`chimera-core/src/dsp/lfo.rs`) — simple wavetable LFO
4. Build the mod matrix grid page (auto-generated from chain)
5. Wire modulation into Voice::render() — modulated param copies
6. Add visual indicators on block pages
7. Add routing summary on modulator pages
