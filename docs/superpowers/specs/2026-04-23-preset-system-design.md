# Preset System Design

## Overview

Chimera's preset system follows the Elektron Digitone model: a **Project** holds a **Sound Pool** of 32 patch slots in RAM. Six tracks (B1-B6) each own their own independent parameter state. Loading a patch from the pool **copies** it into the track — edits to a track never modify the pool. The pool is a template library for quick recall.

First iteration: RAM-only (patches lost on power-off). Serialization format is defined for future SD card persistence.

## Terminology

- **Audio block** — a block of audio parameters that forms part of a chain. Sound moves left-to-right through the chain.
- **Modulation block** — lives under the mod matrix. A modulator (LFO, envelope) that can modulate audio block params or other modulator params.
- **Chain** — a series of configured audio blocks and modulation blocks. Defined by a `ChainType` (e.g., PizzaPoly, Modal, FM).
- **Patch** — a chain and its parameter state configured a specific way. A saved sound.
- **Sound Pool** — 32 patch slots held in RAM. A template library for quick recall and future sound locks.
- **Project** — the full synth state: sound pool, mixer state, and per-track parameter state.

## Data Model

### Patch

```rust
struct Patch {
    name: [u8; 16],         // ASCII, null-padded
    chain_type: ChainType,  // which chain template
    params: ParamSnapshot,  // all audio block params
    mod_state: ModState,    // mod matrix routing + amounts
}
```

A `Patch` is self-contained: it knows which chain type it belongs to and holds all parameter + modulation state needed to reproduce the sound.

### SoundPool

```rust
struct SoundPool {
    slots: [Option<Patch>; 32],
}
```

32 slots in RAM. `None` = empty slot. ~500 bytes per patch, ~16KB total. The pool is a library of sound templates — not live state.

### Track

Each track owns its own independent copy of the sound parameters:

```rust
struct Track {
    patch: Patch,              // independent copy, not a reference
    loaded_from: Option<u8>,   // which pool slot this was loaded from (for UI display)
}
```

Loading a patch from the pool copies it into the track. Editing the track modifies only the track's copy. The pool slot is unchanged. The user can save edits back to the pool explicitly.

### Project

```rust
struct Project {
    name: [u8; 16],
    pool: SoundPool,
    tracks: [Track; 6],       // B1-B6, each owns its params
    mixer: MixerState,        // levels, pans, sends
}
```

### ChainType

```rust
enum ChainType {
    PizzaPoly = 0,
    Modal = 1,
    Fm = 2,
    // future chain types
}
```

Maps to the existing chain definitions in `block_registry.rs`.

## Copy-on-Load (Digitone Model)

Tracks own independent copies. The pool is a template library.

```
Pool:   [0: "Acid Bass"] [1: "Pad Wash"] [2: "Pluck"] [3: (init)] ...

Load slot 0 → B1:  B1 gets a COPY of "Acid Bass"
Load slot 0 → B3:  B3 gets a COPY of "Acid Bass"

Edit B1's filter cutoff → only B1 changes. B3 and pool slot 0 are unaffected.
Save B1 → slot 0:  explicitly overwrites pool slot with B1's current state.
```

Benefits:
- Each track is fully independent — no shared mutation
- Audio thread reads track params directly, no indirection through pool
- Pool slots are stable templates, not live-edited state
- Matches existing architecture where each voice has its own `ParamSnapshot`
- Future sound locks (per-step sound changes) use pool indices naturally

## Audio Thread Integration

The audio thread currently reads params via `ParamSnapshot` pointer. With the preset system:

1. Each `Track` owns its `Patch` which contains the `ParamSnapshot`.
2. The voice reads from its track's `ParamSnapshot` — same as today.
3. Loading a patch = memcpy from pool slot into track's patch, then update the voice's param pointer.
4. No lock needed: UI writes to inactive buffer, atomically swaps pointer (existing mechanism).

## UI Flow

### Load patch into track

1. User double-taps B1 (or any B1-B6 button).
2. Patch browser opens, showing the 32 pool slots.
3. Each slot displays: slot number, patch name, chain type.
4. User scrolls with encoder, selects with press.
5. Pool slot is **copied** into B1's track. B1 now plays that sound.

### Edit patch

Normal chain navigation. Editing params modifies the track's own copy. Pool is unaffected.

### Save patch to pool

User action (e.g., long-press in patch browser) copies the track's current state back into a pool slot. This overwrites the slot.

### Init patch

In the patch browser, an "(init)" option copies a fresh default patch (for the selected chain type) into the track. Every chain type has musically useful init defaults (not zeros).

## Serialization Format

Binary, versioned, forward-compatible. For future SD card persistence.

```
Offset  Size  Field
0x00    4     Magic: 0x43484D50 ("CHMP")
0x04    1     Format version (1)
0x05    1     ChainType
0x06    16    Name (ASCII, null-padded)
0x16    N     ParamSnapshot (raw bytes, size depends on version)
0x16+N  M     ModState (raw bytes, size depends on version)
```

Version field allows adding new params in future versions. Reader skips unknown trailing bytes.

For the RAM-only first iteration, serialization is not exercised — patches are constructed in memory directly. The format is defined now so the `Patch` struct layout is designed with serialization in mind.

## Scope

### First iteration (this spec)

- `Patch`, `SoundPool`, `Track`, `Project`, `ChainType` types in `chimera-core`
- Copy-on-load: pool slot → track
- Save-to-pool: track → pool slot
- Patch browser UI (double-tap B1-B6)
- Init patch per chain type (musically useful defaults)
- Wire audio thread to read from track-owned params
- Serialization format defined (structs laid out for `repr(C)` / raw byte access)

### Future iterations

- SD card driver (SPI2) + FAT filesystem
- Save/load patches and projects from SD card
- Sound library on SD (persistent, cross-project, tagged/browsable)
- Sound locks (per-step sound changes from pool — sequencer feature)
- Copy/paste patches between slots
- Factory preset bank
- Project save/load
