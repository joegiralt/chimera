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

### Modulation Index Scaling

Modulator output is scaled by `operatorFactor = 4.0` before being applied as phase modulation. This constant controls the overall modulation depth and must match the TX81Z behavior. Without it, all modulation depths will be wrong.

Reference: `p81z/sources/TX81Z/TX81Z_common.h` line 26, `TX81Z_oscillator.cpp` line 65.

## Operators

Each operator is a phase-modulation oscillator with its own envelope.

### Tone Parameters
- **Waveform** — 8 TX81Z waveforms (see Waveforms section)
- **Coarse** — frequency ratio coarse select (0–63, indexes into 64-entry ratio lookup table)
- **Fine** — frequency ratio fine adjust (0–15; clamped to 0–7 when coarse < 4)
- **Level** — output level (0–99), converted via `dBtoGain(0.74 * (level + 1) - 73.26)`. This is NOT linear.
- **Feedback** — self-modulation amount (0–7, mapped to TX81Z scaling factors)
- **Detune** — fine tuning (-7..+7)
- **Velocity Sensitivity** — KVS (0–7), applied via 5th-degree polynomial with 5 coefficient lookup tables

### Envelope Parameters (5-stage)
- **AR** — Attack Rate (0–31)
- **D1R** — Decay 1 Rate (0–31)
- **D1L** — Decay 1 Level / sustain breakpoint (0–15), converted via `dBtoGain(-3.0 * (15 - level))`, 0 returns 0.0
- **D2R** — Decay 2 Rate (0–31)
- **RR** — Release Rate (1–15)
- **Rate Scaling** — envelope speed scales with pitch (0–3)

### Frequency Ratio System

The TX81Z does NOT use a continuous ratio range. It uses a 64-entry lookup table of discrete, unevenly-spaced ratios plus a fine interpolation:

```rust
const FREQ_RATIOS: [f32; 64] = [
    0.50, 0.71, 0.78, 0.87, 1.00, 1.41, 1.57, 1.73,
    2.00, 2.82, 3.00, 3.14, 3.46, 4.00, 4.24, 4.71,
    5.00, 5.19, 5.65, 6.00, 6.28, 6.92, 7.00, 7.07,
    7.85, 8.00, 8.48, 8.65, 9.00, 9.42, 9.89, 10.00,
    10.38, 10.99, 11.00, 11.30, 12.00, 12.11, 12.56, 12.72,
    13.00, 13.84, 14.00, 14.10, 14.13, 15.00, 15.55, 15.57,
    15.70, 16.96, 17.27, 17.30, 18.37, 18.84, 19.03, 19.78,
    20.41, 20.76, 21.20, 21.98, 22.49, 23.55, 24.22, 25.95,
];

const FREQ_RATIOS_MAX: [f32; 64] = [
    0.93, 1.32, 1.37, 1.62, 1.93, 2.73, 3.04, 3.35,
    2.93, 4.14, 3.93, 4.61, 5.08, 4.93, 5.55, 6.18,
    5.93, 6.81, 6.96, 6.93, 7.75, 8.54, 7.93, 8.37,
    9.32, 8.93, 9.78, 10.27, 9.93, 10.89, 11.19, 10.93,
    12.00, 12.46, 11.93, 12.60, 12.93, 13.73, 14.03, 14.01,
    13.93, 15.46, 14.93, 15.42, 15.60, 15.93, 16.83, 17.19,
    17.17, 18.24, 18.74, 18.92, 19.65, 20.31, 20.65, 21.06,
    21.88, 22.38, 22.47, 23.45, 24.11, 25.02, 25.84, 27.57,
];

fn compute_ratio(coarse: u8, fine: u8) -> f32 {
    let fine = if coarse < 4 { fine.min(7) } else { fine };
    let min = FREQ_RATIOS[coarse as usize];
    let max = FREQ_RATIOS_MAX[coarse as usize];
    let steps = if coarse < 4 { 7.0 } else { 15.0 };
    (min + (max - min) / steps * fine as f32).min(max)
}
```

Reference: `p81z/sources/TX81Z/TX81Z_extra.cpp` lines 4–14.

### Output Level Conversion

Operator level (0–99) maps to gain via a dB curve, NOT linearly:

```rust
fn level_to_gain(level: u8) -> f32 {
    db_to_gain(0.74 * (level as f32 + 1.0) - 73.26)
}

fn db_to_gain(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}
```

Reference: `p81z/sources/TX81Z/TX81Z_common.cpp` line 32.

### Velocity Sensitivity

KVS (0–7) uses a 5th-degree polynomial with 5 coefficient lookup tables (factors 1–5). Each KVS setting selects different polynomial coefficients that shape the velocity→level curve.

Reference: `p81z/sources/TX81Z/TX81Z_common.cpp` — `computeVelocityFactor()` function and KVS factor tables.

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

- **AR**: attack from 0 to max level
- **D1R**: first decay from max to D1L
- **D1L**: sustain breakpoint — a level, not a rate. Converted: `dBtoGain(-3.0 * (15 - d1l))`, d1l=0 returns 0.0
- **D2R**: second decay from D1L toward 0 (slow drift during sustain)
- **RR**: release from current level to 0 on gate off

States: `Idle → Attack → Decay1 → Decay2 → Release → Idle`. Gate on triggers Attack. Gate off triggers Release from any state.

Envelope timing uses TX81Z rate tables and rate scaling. Reference: `p81z/sources/TX81Z/TX81Z_envelope.cpp`.

## Algorithms (8)

Traced from `p81z/sources/FMArrangement.cpp` lines 42–102. Notation: `→` = phase modulation input, `[N]` = carrier (output to mix), `+` = summed into same modulation bus.

```
ALG 1:  4→3→2→[1]                    (full serial)
ALG 2:  (3+4)→2→[1]                  (op3 & op4 sum into op2's mod input)
ALG 3:  3→2, (2+4)→[1]              (op3 mods op2, op2+op4 sum into op1's mod)
ALG 4:  4→3, (3+4→2)→[1]            (op4 mods op3, op3 output + op2 w/op4 mod sum into op1)
ALG 5:  2→[1], 4→[3]                 (two pairs: op2→op1, op4→op3, both carriers)
ALG 6:  4→[1], 4→[2], 4→[3]         (op4 mods all three carriers)
ALG 7:  [1], [2], 4→[3]              (op1+op2 free, op4 mods op3)
ALG 8:  [1], [2], [3], [4]           (full parallel, all carriers)
```

## Waveforms (8)

All 8 waveforms are variations on sine. Implementations must match the p81z definitions exactly — do NOT use vague descriptions. Use the code at `p81z/sources/TX81Z/TX81Z_common.cpp` lines 100–159 as the canonical reference.

| Index | p81z case | Description |
|-------|-----------|-------------|
| W1 | case 0 | `sin(phase)` — pure sine |
| W2 | case 1 | Rectified-looking wave with DC offset per half-cycle |
| W3 | case 2 | `sin(phase)` in first half, 0 in second half |
| W4 | case 3 | `abs(sin(phase))` — full-wave rectified sine |
| W5 | case 4 | `sin(2*phase)` in first quarter, 0 in second, repeat |
| W6 | case 5 | `abs(sin(2*phase))` in first half, 0 in second |
| W7 | case 6 | `sin(3*phase)` in first third, 0 for rest |
| W8 | case 7 | `abs(sin(4*phase))` in first quarter, 0 for rest |

**Do not invent waveform names.** Port the exact math from p81z case-by-case.

Future: FS1R formant waveforms as W9+.

## Feedback

Any operator can have self-feedback (0–7). TX81Z feedback scaling factors from p81z:

```
[0.0, 0.008, 0.015, 0.024, 0.07, 0.12, 0.19, 0.26]
```

Feedback is applied by feeding the operator's previous output sample back as phase modulation input, scaled by the factor above and then by `operatorFactor (4.0)`.

## UI Layout

The FM block is a single chain block with 3 sub-pages. Per-operator envelope editing (AR, D1R, D1L, D2R, RR, rate scaling) lives in the mod matrix sub-pages, following the same pattern as existing envelope/LFO editing.

```
FM Block
  ├ Algorithm — algorithm select, diagram on screen
  ├ Operator Focus — select OP, edit its tone params
  └ Ratios — all 4 operator ratios for harmonic relationships
```

### Sub-Page 1: Algorithm

| Encoder | Parameter |
|---------|-----------|
| A | Algorithm (0–7) |
| B | — |
| C | Output Level |
| D | — |
| E | — |
| F | — |

Screen shows the algorithm routing diagram (e.g., `4→3→2→[1]`) so the user always knows the FM topology.

### Sub-Page 2: Operator Focus

Encoder A selects which operator (1–4). Encoders B–F edit the selected operator. The screen highlights the selected operator in the algorithm diagram.

| Encoder | Parameter |
|---------|-----------|
| A | Operator Select (1–4) |
| B | Waveform (0–7) |
| C | Level (0–99) |
| D | Feedback (0–7) |
| E | Detune (-7..+7) |
| F | Vel Sens (0–7) |

One page instead of four — same muscle memory, the selected operator is highlighted on screen.

### Sub-Page 3: Ratios

All 4 operator ratios on one page for harmonic relationship editing. Seeing all ratios together makes it easy to tune intervals.

| Encoder | Parameter |
|---------|-----------|
| A | OP1 Coarse (0–63) |
| B | OP2 Coarse (0–63) |
| C | OP3 Coarse (0–63) |
| D | OP4 Coarse (0–63) |
| E | Fine (selected op) |
| F | — |

### Envelope Editing

The 5-stage TX81Z envelope (AR, D1R, D1L, D2R, RR, rate scaling) per operator is accessed through the mod matrix sub-pages, consistent with how Chimera handles envelopes and LFOs for all engines. The mod matrix sees the 4 operator envelopes as modulation sources.

## Integration with Chimera

### New Files
- `chimera-core/src/dsp/engine_fm.rs` — FM engine: operators, algorithms, waveforms, rendering
- `chimera-core/src/dsp/envelope_fm.rs` — TX81Z 5-stage envelope
- `chimera-core/src/dsp/fm_tables.rs` — ratio tables, KVS polynomial tables, rate tables, waveform functions

### Modified Files
- `chimera-core/src/params.rs` — add `FmParams` (4 operators + algorithm)
- `chimera-core/src/dsp/voice.rs` — wire `EngineType::Fm` to FM engine
- `chimera-core/src/ui/block_registry.rs` — FM block definitions + sub-pages
- `chimera-core/src/ui/chain.rs` — `ChainType::Fm` resolves to FM chain
- `chimera-core/src/preset.rs` — FM init patch defaults

### ParamSnapshot Extension

```rust
pub struct FmParams {
    pub algorithm: Param,           // 0–7
    pub operators: [FmOpParams; 4],
}

pub struct FmOpParams {
    pub waveform: Param,            // 0–7
    pub coarse: Param,              // 0–63 (indexes ratio table)
    pub fine: Param,                // 0–15 (0–7 when coarse < 4)
    pub level: Param,               // 0–99
    pub feedback: Param,            // 0–7
    pub detune: Param,              // -7..+7
    pub velocity_sens: Param,       // 0–7
    pub attack_rate: Param,         // 0–31
    pub decay1_rate: Param,         // 0–31
    pub decay1_level: Param,        // 0–15
    pub decay2_rate: Param,         // 0–31
    pub release_rate: Param,        // 1–15
    pub rate_scaling: Param,        // 0–3
}
```

Add `pub fm: FmParams` to `ParamSnapshot`.

## CPU Budget

TX81Z FM synthesis is computationally cheap — 4 waveform lookups + modulation per sample. The design doc estimates ~200 cycles/sample. With BLOCK_SIZE=64 at 48kHz, this is well within budget even with the post-FM signal chain.

## Scope

### In Scope
- 4 operators with phase modulation
- 8 TX81Z algorithms (traced from p81z)
- 8 TX81Z waveforms (ported from p81z case-by-case)
- 5-stage envelope per operator (AR, D1R, D1L, D2R, RR)
- Feedback per operator (0–7) with TX81Z scaling factors
- Ratio mode with 64-entry coarse table + fine interpolation
- Level-to-gain dB conversion (not linear)
- Modulation index scaling (operatorFactor = 4.0)
- Velocity sensitivity with KVS polynomial tables
- Rate scaling
- Detune
- FM chain definition with block defs + sub-pages
- Integration with existing signal chain
- Desktop simulator support

### Out of Scope (future)
- Fixed frequency mode
- FS1R formant waveforms
- TX81Z preset import/export
- LFO → operator modulation
- Per-operator key scaling
