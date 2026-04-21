# Issue #002: Rewrite FM engine from TX81Z reference implementations

## Problem

The original FM engine (`fm.rs`, now deleted) had several issues:
- Used `libm::sinf` (~500 cycles per call) instead of wavetable lookup (~10 cycles)
- Default operator levels were wrong (modulators at 0.0 = no audible FM)
- Missing `operatorFactor = 4.0` modulation depth scaling
- Feedback values weren't the hardware-measured 8-level table
- Waveform definitions not verified against hardware

It was replaced with the Pizza oscillator as a working placeholder.

## Reference Implementations

Located in `/home/hermes/dev/samples/`. Priority order:

### 1. p81z (highest priority)
- 1024-entry float sine table with linear interpolation
- 32-bit float phase accumulator (0.0-1.0)
- `operatorFactor = 4.0` — modulation depth scaling
- Feedback table: `{0.0, 0.008, 0.015, 0.024, 0.07, 0.12, 0.19, 0.26}`
- 8 waveforms as piecewise sine variations (correct TX81Z definitions)
- Hardcoded algorithm routing via switch/case
- Key velocity sensitivity: 5th-degree polynomial with 8 sensitivity levels
- Level-to-gain: `0.74 * (level+1) - 73.26` dB

### 2. ymfm (cycle-accurate)
- 256-entry quarter-sine in log-attenuation format (4.8 fixed-point)
- 10.10 fixed-point phase accumulator
- 14-bit signed operator output
- **Two-sample feedback average** — this is the actual hardware behavior
- Algorithm routing via packed 10-bit descriptor table
- All math in log domain (add = multiply in linear)

### 3. cesaref-TX81Z
- Same author as p81z, Cmajor implementation
- PDF presentation: "Emulating the TX81Z" from ADC24
- Identifies what matters and what doesn't for convincing emulation
- Confirms: skip LFO, micro-tuning, effects for v1. Focus on operators, envelope, algorithms.
- Notes: 1-frame modulation delay breaks cyclic dependencies in dataflow

### 4. deicsonze
- 96000-entry table (no interpolation needed)
- **WARNING:** Waveform definitions use `sin*|sin|` which differs from hardware
- Single global feedback coefficient (not the 8-level table)
- Less accurate but simpler implementation

## What the Rewrite Needs

### Must have
1. **1024-entry sine LUT** with linear interpolation (reuse `fast_sin` from `dsp/mod.rs`)
2. **8 correct waveforms** — piecewise sine variations per p81z/cesaref (NOT deicsonze)
3. **operatorFactor = 4.0** modulation depth scaling
4. **8-level feedback table** from p81z: `{0.0, 0.008, 0.015, 0.024, 0.07, 0.12, 0.19, 0.26}`
5. **8 algorithms** — hardcoded operator execution order
6. **4-stage envelope** (attack, decay1, decay2, release) with correct rate curves
7. **DC blocking** on output
8. **No libm calls in render path** — all wavetable or fast approximations

### Nice to have (v1.1)
- Two-sample feedback average (ymfm hardware accuracy)
- Key velocity sensitivity polynomial
- Level-to-gain curve matching hardware
- Rate scaling per note

### Not needed for v1
- LFO (handled by chain modulation system)
- Micro-tuning
- Effects (handled by chain)
- Breath controller

## Architecture

The FM engine should be a Block in the chain architecture:
- `FmEngine` struct with 4 `FmOperator` instances
- Each operator: phase accumulator, wavetable, envelope, output buffer
- Algorithm routing in `render()`: execute operators in dependency order, sum carriers
- The Block's page shows: Algorithm, Feedback, per-operator ratio/level/waveform/detune

## CPU Budget

Target: < 400 cycles/sample for 4 operators at 48kHz.
- 4 × wavetable lookup with interpolation: ~40 cycles
- 4 × envelope: ~40 cycles  
- Algorithm routing: ~20 cycles
- Overhead: ~20 cycles
- **Total: ~120 cycles/sample** — well within budget

With the old `libm::sinf` approach this was ~2000 cycles/sample. The wavetable approach is 15x faster.

## Priority

**High** — FM synthesis is the primary engine for this instrument. The Pizza oscillator is a placeholder.

## Files

- Create: `chimera-core/src/dsp/fm.rs` (new, from scratch)
- Modify: `chimera-core/src/dsp/voice.rs` — re-add FmEngine
- Modify: `chimera-core/src/params.rs` — re-add FmParams  
- Modify: `chimera-core/src/ui/block_registry.rs` — re-add FM BlockDefs + chain
- Reference: `/home/hermes/dev/samples/p81z/`, `/home/hermes/dev/samples/cesaref-TX81Z/`
