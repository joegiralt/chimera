# 0007. Each block owns its values and a const description

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
Parameters were stored as `Param { value, min, max, default }` (16 B each)
in some blocks and raw `f32`/`u8` in others. Range metadata was duplicated
in every instance and again in UI formats, and drifted (encoder steps and
display formats disagreed with ranges). RAM is the tight resource
(512 KB; ~150 KB framebuffer, ~66 KB per Modal voice); flash is not
(~900 KB).

## Decision
Each block module owns (a) a values struct with real field types (patch
data, RAM, one per instance), (b) `pub static <BLOCK>_SPECS: [ParamSpec; N]`
describing each parameter (flash, one per block type), (c) its DSP struct
(audio-thread state). A `Block` trait (`specs/get/write`, provided
`set/nudge/snap`) is the one interface for UI, modulation, and later
serialization and the web builder. Values and DSP state stay separate
structs because the UI and the audio interrupt must not share an object.

## Alternatives considered
- **Keep `Param` per instance** (option B) — smaller diff now; metadata in
  every patch and SysEx dump, duplicated again in the web builder.
- **One object per block holding values and DSP state** — shared across
  threads, needs locks the audio thread can't take.

## Consequences
`ParamSnapshot` shrinks ~4× (1.5 KB → ~0.4 KB). The patch format becomes
"just the values". Revisit if resource constraints appear.

## Sources
Spec §1 of `docs/superpowers/specs/2026-09-23-engine-refactor-design.md`;
`chimera-core/src/block.rs`.
