# Preset System Design

## Overview

Chimera's preset system follows the Elektron model: a **Project** holds a **Sound Pool** of 32 patch slots in RAM. Six tracks (B1-B6) reference slots in the pool. Multiple tracks can share the same slot — editing the slot affects all tracks that reference it.

First iteration: RAM-only (patches lost on power-off). Serialization format is defined for future SD card persistence.

## Terminology

- **Audio block** — a block of audio parameters that forms part of a chain. Sound moves left-to-right through the chain.
- **Modulation block** — lives under the mod matrix. A modulator (LFO, envelope) that can modulate audio block params or other modulator params.
- **Chain** — a series of configured audio blocks and modulation blocks. Defined by a `ChainType` (e.g., PizzaPoly, Modal, FM).
- **Patch** — a chain and its parameter state configured a specific way. A saved sound.
- **Sound Pool** — 32 patch slots held in RAM. The hot-swappable patch bank.
- **Project** — the full synth state: sound pool, mixer state, and which tracks reference which slots.

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

32 slots in RAM. `None` = empty slot. On boot, slots default to init patches (chain-specific defaults). ~500 bytes per patch, ~16KB total.

### Project

```rust
struct Project {
    name: [u8; 16],
    pool: SoundPool,
    tracks: [TrackAssignment; 6],  // B1-B6
    mixer: MixerState,             // levels, pans, sends
}

struct TrackAssignment {
    slot: Option<u8>,  // index into pool, None = silent
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

## Shared References

Tracks reference pool slots by index. Multiple tracks can point to the same slot.

```
Pool:   [0: "Acid Bass"] [1: "Pad Wash"] [2: "Pluck"] [3: (init)] ...

B1 → slot 0  ●
B2 → slot 1  ●
B3 → slot 0  ●  ← same as B1, edits affect both
B4 → slot 2  ●
B5 → None    (silent)
B6 → None    (silent)
```

When the user edits params on B1, they're editing slot 0. B3 shares slot 0, so B3's sound changes too. This matches the Digitone sound pool model.

## Audio Thread Integration

The audio thread currently reads params via `ParamSnapshot` pointer. With the preset system:

1. `Project` owns the `SoundPool` which owns the `Patch` structs.
2. Each track's voice reads from its assigned patch's `ParamSnapshot`.
3. When a track's slot assignment changes, the voice's param pointer is updated.
4. Slot swaps are a pointer change — zero-copy, safe for the audio thread.

## UI Flow

### Load patch into track

1. User double-taps B1 (or any B1-B6 button).
2. Patch browser opens, showing the 32 pool slots.
3. Each slot displays: slot number, patch name, chain type.
4. User scrolls with encoder, selects with press.
5. B1 now references the selected slot.

### Edit patch

Normal chain navigation. Editing params modifies the referenced pool slot directly. All tracks sharing that slot hear changes immediately.

### Init patch

In the patch browser, an "(init)" option writes a fresh default patch (for the selected chain type) into the current slot.

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

- `Patch`, `SoundPool`, `Project`, `TrackAssignment` types in `chimera-core`
- Track-to-slot assignment and hot-swap
- Patch browser UI (double-tap B1-B6)
- Init patch per chain type
- Wire audio thread to read from pool slots
- Serialization format defined (structs laid out for `repr(C)` / raw byte access)

### Future iterations

- SD card driver (SPI2) + FAT filesystem
- Save/load patches and projects from SD card
- Copy/paste patches between slots
- Factory preset bank
- Project save/load
