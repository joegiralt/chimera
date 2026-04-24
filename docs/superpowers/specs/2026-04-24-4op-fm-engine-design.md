# 4-Operator FM Engine Design

## Overview

Faithful TX81Z-style 4-operator FM synthesis engine for Chimera. Four operators with phase modulation, 8 algorithms, 8 waveforms, 5-stage envelopes, and per-operator feedback. Integrates as a single chain block with sub-pages, feeding into the existing signal chain (Drive → Filter → Folder → Reverb).

Primary reference: `p81z` C++ implementation at `/home/hermes/dev/samples/p81z/`.

## Signal Flow

The 4opFM block is a single audio block in the chain. Sound flows left-to-right as with all Chimera chains:

```
[4opFM] → [Drive] → [Filter] → [Folder] → [Reverb]
```

Inside the FM block, 4 operators are routed according to the selected algorithm. Carriers output to the mix; modulators feed their output as phase modulation into other operators.

## Operators

Each operator is a phase-modulation oscillator with its own envelope:

### Tone Parameters
- **Waveform** — 8 TX81Z waveforms (see Waveforms section)
- **Ratio** — frequency multiplier relative to note (0.5–16.0, coarse steps matching TX81Z)
- **Level** — output level (0–99, TX81Z scale)
- **Feedback** — self-modulation amount (0–7, mapped to TX81Z scaling factors)
- **Detune** — fine tuning (-7..+7)
- **Velocity Sensitivity** — KVS (0–7, polynomial curves matching TX81Z)

### Envelope Parameters (5-stage)
- **AR** — Attack Rate (0–31)
- **D1R** — Decay 1 Rate (0–31)
- **D1L** — Decay 1 Level / sustain breakpoint (0–15)
- **D2R** — Decay 2 Rate (0–31)
- **RR** — Release Rate (1–15)
- **Rate Scaling** — envelope speed scales with pitch (0–3)

## 5-Stage Envelope

Unlike standard ADSR, the TX81Z envelope has two decay segments:

```
Level
  │     AR        D1R       D2R       RR
  │    ╱╲         ╲         ╲
  │   ╱  ╲         ╲         ╲
  │  ╱    ╲         ╲         ╲
  │ ╱      ╲ D1L     ╲         ╲
  │╱        ╲─────────╲         ╲
  │                     ╲────────╲──→ 0
  └──────────────────────────────────→ Time
    Gate On                    Gate Off
```

- AR: attack from 0 to max level
- D1R: first decay from max to D1L
- D1L: sustain breakpoint (not a rate — a level)
- D2R: second decay from D1L toward 0 (slow drift during sustain)
- RR: release from current level to 0 on gate off

Envelope timing uses TX81Z rate tables and rate scaling. Reference: `p81z/sources/TX81Z/TX81Z_envelope.cpp`.

## Algorithms (8)

TX81Z routing patterns. Green = carrier (output to mix). Arrows = phase modulation.

```
ALG 1 (serial):     4→3→2→[1]
ALG 2:              (3+4)→2→[1]
ALG 3:              4→3, 4→2→[1]
ALG 4:              4→3→[1], 4→[2]
ALG 5:              4→[3], 4→[2], 4→[1]
ALG 6:              4→[3], [2], [1]
ALG 7:              [4], 3→[2], [1]
ALG 8 (parallel):   [4], [3], [2], [1]
```

Each algorithm defines which operators are carriers (output to mix) and which are modulators (feed phase modulation to others). Implementation reference: `p81z/sources/FMArrangement.cpp` lines 42–102.

## Waveforms (8)

TX81Z sine-derived waveforms:

| Index | Name | Description |
|-------|------|-------------|
| W1 | Sine | Pure sine |
| W2 | Sine² | abs(sin) shaped |
| W3 | Half-sine | Positive half only |
| W4 | Half-sine² | Positive half, squared |
| W5 | Quarter-sine | First/third quarter |
| W6 | Quarter-sine² | Squared quarter |
| W7 | Clipped sine | +shaped |
| W8 | Clipped sine² | +shaped, squared |

Future: FS1R formant waveforms as W9+.

Reference: `p81z/sources/TX81Z/TX81Z_common.cpp` lines 100–159.

## Feedback

Any operator can have self-feedback (0–7). TX81Z feedback scaling factors from p81z:

```
[0.0, 0.008, 0.015, 0.024, 0.07, 0.12, 0.19, 0.26]
```

Feedback is applied by feeding the operator's previous output sample back as phase modulation input.

## UI Layout

The FM block appears as a single block in the chain with 5 sub-pages:

```
FM Block (top-level)
  ├ Algorithm — alg select (A), output level (C)
  ├ OP1 — wave/ratio/level/fb/detune/vel + shift:envelope
  ├ OP2 — same layout
  ├ OP3 — same layout
  └ OP4 — same layout
```

### Operator Sub-Page Encoder Mapping

Consistent across all 4 operator pages (muscle memory):

| Encoder | Normal | Shift |
|---------|--------|-------|
| A | Waveform | AR |
| B | Ratio | D1R |
| C | Level | D1L |
| D | Feedback | D2R |
| E | Detune | RR |
| F | Vel Sens | Rate Scaling |

## Integration with Chimera

### New Files
- `chimera-core/src/dsp/engine_fm.rs` — FM engine: operators, algorithms, waveforms, rendering
- `chimera-core/src/dsp/envelope_fm.rs` — TX81Z 5-stage envelope

### Modified Files
- `chimera-core/src/params.rs` — add `FmParams` (4 operators + algorithm)
- `chimera-core/src/dsp/voice.rs` — wire `EngineType::Fm` to FM engine
- `chimera-core/src/ui/block_registry.rs` — FM block definitions + sub-pages
- `chimera-core/src/ui/chain.rs` — `ChainType::Fm` resolves to FM chain
- `chimera-core/src/preset.rs` — FM init patch defaults

### ParamSnapshot Extension

```rust
pub struct FmParams {
    pub algorithm: Param,      // 0–7
    pub operators: [FmOpParams; 4],
}

pub struct FmOpParams {
    pub waveform: Param,       // 0–7
    pub ratio: Param,          // 0.5–16.0
    pub level: Param,          // 0–99
    pub feedback: Param,       // 0–7
    pub detune: Param,         // -7..+7
    pub velocity_sens: Param,  // 0–7
    pub attack_rate: Param,    // 0–31
    pub decay1_rate: Param,    // 0–31
    pub decay1_level: Param,   // 0–15
    pub decay2_rate: Param,    // 0–31
    pub release_rate: Param,   // 1–15
    pub rate_scaling: Param,   // 0–3
}
```

Add `pub fm: FmParams` to `ParamSnapshot`.

## CPU Budget

TX81Z FM synthesis is computationally cheap — 4 sine lookups + modulation per sample. The design doc estimates ~200 cycles/sample. With BLOCK_SIZE=64 at 48kHz, this is well within budget even with the post-FM signal chain.

## Scope

### In Scope
- 4 operators with phase modulation
- 8 TX81Z algorithms
- 8 TX81Z waveforms
- 5-stage envelope per operator (AR, D1R, D1L, D2R, RR)
- Feedback per operator (0–7)
- Ratio mode (0.5–16.0)
- Detune, velocity sensitivity, rate scaling
- FM chain definition with block defs + sub-pages
- Integration with existing signal chain
- Desktop simulator support

### Out of Scope (future)
- Fixed frequency mode
- FS1R formant waveforms
- TX81Z preset import/export
- LFO → operator modulation
- Per-operator key scaling
