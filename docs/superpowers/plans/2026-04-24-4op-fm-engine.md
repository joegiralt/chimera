# 4-Operator FM Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a faithful TX81Z-style 4-operator FM synthesis engine with 8 algorithms, 8 waveforms, 5-stage envelopes, and integrate it into Chimera's chain system.

**Architecture:** `FmEngine` contains 4 `FmOperator`s (each with `FmOscillator` + `FmEnvelope`). Algorithm routing is a match on algorithm index, following p81z's `FMArrangement::run()` pattern exactly. Waveforms are computed functions (no lookup tables — saves RAM, CPU is cheap). The engine plugs into `Voice` via `EngineType::Fm`.

**Tech Stack:** Rust `no_std`, `libm` for sin/powf, existing `Param`/`ParamSnapshot`/`Voice` types.

**Spec:** `docs/superpowers/specs/2026-04-24-4op-fm-engine-design.md`
**Primary reference:** `p81z` at `/home/hermes/dev/samples/p81z/sources/`

---

## File Structure

### New files
- `chimera-core/src/dsp/fm_tables.rs` — ratio tables, KVS tables, feedback factors, rate scaling, level conversion
- `chimera-core/src/dsp/fm_waveform.rs` — 8 TX81Z waveform functions
- `chimera-core/src/dsp/envelope_fm.rs` — 5-stage TX81Z envelope state machine
- `chimera-core/src/dsp/engine_fm.rs` — FmOperator, FmOscillator, FmEngine (4 ops + algorithm routing)
- `chimera-core/tests/fm_test.rs` — all FM tests

### Modified files
- `chimera-core/src/dsp/mod.rs` — add `pub mod fm_tables; pub mod fm_waveform; pub mod envelope_fm; pub mod engine_fm;`
- `chimera-core/src/params.rs` — add `FmParams`, `FmOpParams`, add `fm: FmParams` to `ParamSnapshot`
- `chimera-core/src/dsp/voice.rs` — add `FmEngine` field, wire `EngineType::Fm` dispatch
- `chimera-core/src/ui/block_registry.rs` — FM block definitions with 3 sub-pages
- `chimera-core/src/ui/chain.rs` — `ChainType::Fm` resolves to FM chain

---

## Task 1: FM lookup tables and math

**Files:**
- Create: `chimera-core/src/dsp/fm_tables.rs`
- Create: `chimera-core/tests/fm_test.rs`
- Modify: `chimera-core/src/dsp/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
// chimera-core/tests/fm_test.rs
use chimera_core::dsp::fm_tables;

#[test]
fn ratio_table_unity() {
    assert_eq!(fm_tables::compute_ratio(4, 0), 1.0);
}

#[test]
fn ratio_table_half() {
    assert_eq!(fm_tables::compute_ratio(0, 0), 0.5);
}

#[test]
fn ratio_table_fine_interpolates() {
    let r0 = fm_tables::compute_ratio(4, 0);
    let r15 = fm_tables::compute_ratio(4, 15);
    assert!(r15 > r0);
    assert!(r15 <= fm_tables::FREQ_RATIOS_MAX[4]);
}

#[test]
fn ratio_table_sub4_clamps_fine() {
    // coarse < 4: fine clamped to 0-7
    let r7 = fm_tables::compute_ratio(0, 7);
    let r15 = fm_tables::compute_ratio(0, 15); // should clamp to 7
    assert_eq!(r7, r15);
}

#[test]
fn level_to_gain_zero_is_tiny() {
    let g = fm_tables::level_to_gain(0);
    assert!(g > 0.0);
    assert!(g < 0.01);
}

#[test]
fn level_to_gain_99_near_unity() {
    let g = fm_tables::level_to_gain(99);
    assert!(g > 0.5);
    assert!(g <= 2.0);
}

#[test]
fn d1l_zero_returns_zero() {
    assert_eq!(fm_tables::d1l_to_level(0), 0.0);
}

#[test]
fn d1l_15_near_unity() {
    let l = fm_tables::d1l_to_level(15);
    assert!(l > 0.9);
    assert!(l <= 1.0);
}

#[test]
fn feedback_factors_correct() {
    assert_eq!(fm_tables::FEEDBACK[0], 0.0);
    assert_eq!(fm_tables::FEEDBACK[7], 0.26);
}
```

- [ ] **Step 2: Run tests — verify fail**

Run: `cargo test -p chimera-core --test fm_test`

- [ ] **Step 3: Implement fm_tables.rs**

Port from `p81z/sources/TX81Z/TX81Z_common.cpp` and `TX81Z_extra.cpp`:
- `FREQ_RATIOS: [f32; 64]` and `FREQ_RATIOS_MAX: [f32; 64]` tables
- `compute_ratio(coarse: u8, fine: u8) -> f32`
- `db_to_gain(db: f32) -> f32`
- `level_to_gain(level: u8) -> f32` — `dBtoGain(0.74 * (level+1) - 73.26)`
- `d1l_to_level(d1l: u8) -> f32` — `d1l == 0 ? 0.0 : dBtoGain(-3.0 * (15 - d1l))`
- `FEEDBACK: [f32; 8]` — `[0.0, 0.008, 0.015, 0.024, 0.07, 0.12, 0.19, 0.26]`
- `OPERATOR_FACTOR: f32 = 4.0`
- KVS polynomial tables: `KVS_F1..KVS_F5: [f32; 8]`
- `compute_velocity_factor(kvs: u8, velocity: f32) -> f32`
- Rate scaling: `compute_rate_scaling(rs: u8, note: u8) -> f32`
- Envelope rate computation: `attack_rate_factor(rate: u8, rs_offset: f32, sr: f32) -> f32` and `decay_rate_factor(rate: u8, rs_offset: f32, sr: f32) -> f32`

Add `pub mod fm_tables;` to `chimera-core/src/dsp/mod.rs`.

- [ ] **Step 4: Run tests — verify pass**

- [ ] **Step 5: Commit**

```
feat(core): FM lookup tables — ratios, levels, KVS, feedback, rates
```

---

## Task 2: TX81Z waveforms

**Files:**
- Create: `chimera-core/src/dsp/fm_waveform.rs`
- Modify: `chimera-core/tests/fm_test.rs`
- Modify: `chimera-core/src/dsp/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
use chimera_core::dsp::fm_waveform;

#[test]
fn waveform_0_is_sine() {
    let v = fm_waveform::compute(0, 0.25); // sin(2π*0.25) = 1.0
    assert!((v - 1.0).abs() < 0.001);
}

#[test]
fn waveform_0_zero_at_origin() {
    let v = fm_waveform::compute(0, 0.0);
    assert!(v.abs() < 0.001);
}

#[test]
fn waveform_2_half_sine_zero_second_half() {
    let v = fm_waveform::compute(2, 0.75); // second half = 0
    assert!(v.abs() < 0.001);
}

#[test]
fn all_8_waveforms_produce_different_output() {
    let phase = 0.13; // arbitrary non-special phase
    let values: Vec<f32> = (0..8).map(|w| fm_waveform::compute(w, phase)).collect();
    // At least 4 distinct values (some may coincide at certain phases)
    let mut unique = values.clone();
    unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    assert!(unique.len() >= 4);
}

#[test]
fn all_waveforms_bounded() {
    for w in 0..8 {
        for i in 0..1024 {
            let phase = i as f32 / 1024.0;
            let v = fm_waveform::compute(w, phase);
            assert!(v.is_finite(), "w={w} phase={phase}");
            assert!(v >= -2.0 && v <= 2.0, "w={w} phase={phase} v={v}");
        }
    }
}
```

- [ ] **Step 2: Run tests — verify fail**

- [ ] **Step 3: Implement fm_waveform.rs**

Port all 8 waveform cases from `p81z/sources/TX81Z/TX81Z_common.cpp` lines 100–159. Each waveform is a `fn(phase: f32) -> f32` where phase is 0.0–1.0. Use `libm::sinf` for sine computation. Match p81z's quadrant-based logic exactly.

```rust
pub fn compute(waveform: u8, phase: f32) -> f32 {
    match waveform {
        0 => { /* sin(2π*phase) */ }
        1 => { /* rectified-looking with DC offset per half */ }
        // ... all 8 cases from p81z
        _ => libm::sinf(core::f32::consts::TAU * phase),
    }
}
```

Add `pub mod fm_waveform;` to dsp/mod.rs.

- [ ] **Step 4: Run tests — verify pass**

- [ ] **Step 5: Commit**

```
feat(core): TX81Z 8 waveform functions ported from p81z
```

---

## Task 3: 5-stage FM envelope

**Files:**
- Create: `chimera-core/src/dsp/envelope_fm.rs`
- Modify: `chimera-core/tests/fm_test.rs`
- Modify: `chimera-core/src/dsp/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
use chimera_core::dsp::envelope_fm::FmEnvelope;

#[test]
fn envelope_starts_idle() {
    let env = FmEnvelope::new();
    assert!(env.is_idle());
    assert_eq!(env.current_level(), 0.0);
}

#[test]
fn envelope_attack_reaches_peak() {
    let mut env = FmEnvelope::new();
    env.note_on(31, 15, 0, 0, 15, 0, 48000.0, 60); // fast attack
    // Process enough samples
    for _ in 0..48000 {
        env.process();
    }
    // Should have reached peak and decayed to D1L
    assert!(env.current_level() > 0.0);
}

#[test]
fn envelope_gate_off_releases() {
    let mut env = FmEnvelope::new();
    env.note_on(31, 0, 15, 0, 15, 0, 48000.0, 60);
    for _ in 0..1000 { env.process(); }
    env.note_off();
    for _ in 0..48000 { env.process(); }
    assert!(env.current_level() < 0.001);
}

#[test]
fn envelope_d1l_zero_decays_to_silence() {
    let mut env = FmEnvelope::new();
    env.note_on(31, 31, 0, 0, 15, 0, 48000.0, 60); // D1L=0
    for _ in 0..96000 { env.process(); }
    assert!(env.current_level() < 0.001);
}
```

- [ ] **Step 2: Run tests — verify fail**

- [ ] **Step 3: Implement envelope_fm.rs**

Port from `p81z/sources/TX81Z/TX81Z_envelope.cpp`:

```rust
enum EnvState { Idle, Attack, Decay1, Decay2, Release }

pub struct FmEnvelope {
    value: f64,
    state: EnvState,
    attack_factor: f64,
    decay1_factor: f64,
    decay1_target: f64,
    decay2_factor: f64,
    release_factor: f64,
    key_scaling: f32,
}
```

State machine:
- Idle: output 0
- Attack: `value += attack_factor`, if `value >= 1.0` → Decay1
- Decay1: `value *= decay1_factor`, if `value <= decay1_target` → Decay2
- Decay2: `value *= decay2_factor`, if `value < ENVELOPE_LIMIT` → Idle
- Release: `value *= release_factor`, if `value < ENVELOPE_LIMIT` → Idle

`note_on()` configures rates from params, enters Attack.
`note_off()` enters Release.

- [ ] **Step 4: Run tests — verify pass**

- [ ] **Step 5: Commit**

```
feat(core): TX81Z 5-stage FM envelope state machine
```

---

## Task 4: FM operator and oscillator

**Files:**
- Create: `chimera-core/src/dsp/engine_fm.rs`
- Modify: `chimera-core/tests/fm_test.rs`
- Modify: `chimera-core/src/dsp/mod.rs`

- [ ] **Step 1: Write failing tests**

```rust
use chimera_core::dsp::engine_fm::{FmOperator, FmOpSettings};

#[test]
fn operator_produces_sine_at_note_freq() {
    let mut op = FmOperator::new();
    let settings = FmOpSettings::default(); // waveform 0, ratio coarse=4 (1.0), level=99
    op.note_on(69, 1.0, &settings, 48000.0); // A4 = 440Hz
    let mut buf = [0.0f32; 1024];
    let zeros = [0.0f32; 1024];
    op.run(&zeros, &mut buf);
    // Should produce audible output
    let rms: f32 = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms > 0.01);
}

#[test]
fn operator_silent_when_idle() {
    let op = FmOperator::new();
    // Not triggered — should produce silence
    let mut buf = [0.0f32; 64];
    let zeros = [0.0f32; 64];
    // Can't call run without note_on, but envelope is idle → output 0
}

#[test]
fn modulation_input_changes_timbre() {
    let mut op = FmOperator::new();
    let settings = FmOpSettings::default();
    op.note_on(69, 1.0, &settings, 48000.0);
    
    // Render with zero modulation
    let zeros = [0.0f32; 1024];
    let mut clean = [0.0f32; 1024];
    op.run(&zeros, &mut clean);
    
    // Reset and render with modulation
    op.note_on(69, 1.0, &settings, 48000.0);
    let modulator = [0.5f32; 1024]; // constant modulation
    let mut modulated = [0.0f32; 1024];
    op.run(&modulator, &mut modulated);
    
    // Outputs should differ
    let diff: f32 = clean.iter().zip(modulated.iter())
        .map(|(a, b)| (a - b).abs()).sum::<f32>();
    assert!(diff > 1.0);
}
```

- [ ] **Step 2: Run tests — verify fail**

- [ ] **Step 3: Implement FmOscillator and FmOperator**

Port from `p81z/sources/TX81Z/TX81Z_oscillator.cpp` and `FMOperator.cpp`:

```rust
pub struct FmOscillator {
    phase: f32,
    phase_increment: f32,
    prev_output: f32, // for feedback
}

pub struct FmOperator {
    osc: FmOscillator,
    env: FmEnvelope,
}

pub struct FmOpSettings {
    pub waveform: u8,
    pub coarse: u8,
    pub fine: u8,
    pub level: u8,
    pub feedback: u8,
    pub detune: i8,
    pub velocity_sens: u8,
    pub ar: u8, pub d1r: u8, pub d1l: u8, pub d2r: u8, pub rr: u8,
    pub rate_scaling: u8,
}
```

Key rendering loop per sample:
```rust
let phase_mod = self.phase + (OPERATOR_FACTOR * modulator_in)
    + (self.prev_output * FEEDBACK[settings.feedback]);
let wave_val = fm_waveform::compute(settings.waveform, phase_mod.fract());
let output = env_level * wave_val * gain;
self.prev_output = wave_val;
self.phase += self.phase_increment;
self.phase -= self.phase as i32 as f32; // wrap
```

Operator has `run(mod_in, audio_out)` and `run_adding(mod_in, audio_out)` matching p81z.

- [ ] **Step 4: Run tests — verify pass**

- [ ] **Step 5: Commit**

```
feat(core): FM operator with oscillator, envelope, and modulation
```

---

## Task 5: FmEngine with 8 algorithms

**Files:**
- Modify: `chimera-core/src/dsp/engine_fm.rs`
- Modify: `chimera-core/tests/fm_test.rs`

- [ ] **Step 1: Write failing tests**

```rust
use chimera_core::dsp::engine_fm::FmEngine;

#[test]
fn fm_engine_produces_sound() {
    let mut engine = FmEngine::new();
    let params = FmParams::default(); // algorithm 0, all ops active
    engine.note_on(69, 100, &params, 48000);
    let mut buf = [0.0f32; 1024];
    engine.render(&mut buf, &params);
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms > 0.01);
}

#[test]
fn fm_engine_note_off_decays() {
    let mut engine = FmEngine::new();
    let params = FmParams::default();
    engine.note_on(69, 100, &params, 48000);
    let mut buf = [0.0f32; 1024];
    engine.render(&mut buf, &params);
    engine.note_off();
    // Render many blocks until silent
    for _ in 0..1000 {
        engine.render(&mut buf, &params);
    }
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms < 0.001);
}

#[test]
fn all_algorithms_produce_different_output() {
    let mut outputs = Vec::new();
    for alg in 0..8u8 {
        let mut engine = FmEngine::new();
        let mut params = FmParams::default();
        params.algorithm = alg;
        // Set all operators to audible levels with different ratios
        for (i, op) in params.operators.iter_mut().enumerate() {
            op.level = 90;
            op.coarse = (4 + i * 4) as u8; // 1.0, 2.0, 3.0, 4.0
        }
        engine.note_on(69, 100, &params, 48000);
        let mut buf = [0.0f32; 2048];
        engine.render(&mut buf, &params);
        let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 2048.0).sqrt();
        outputs.push(rms);
    }
    // Not all identical
    let first = outputs[0];
    assert!(outputs.iter().any(|&r| (r - first).abs() > 0.001));
}

#[test]
fn fm_output_bounded() {
    for alg in 0..8u8 {
        let mut engine = FmEngine::new();
        let mut params = FmParams::default();
        params.algorithm = alg;
        for op in params.operators.iter_mut() {
            op.level = 99;
            op.feedback = 7; // max feedback
        }
        engine.note_on(69, 127, &params, 48000);
        let mut buf = [0.0f32; 4096];
        engine.render(&mut buf, &params);
        for &s in &buf {
            assert!(s.is_finite(), "alg={alg}");
        }
    }
}
```

- [ ] **Step 2: Run tests — verify fail**

- [ ] **Step 3: Implement FmEngine**

```rust
pub struct FmEngine {
    operators: [FmOperator; 4],
    temp_buf: [f32; BLOCK_SIZE],
}
```

`render()` dispatches to algorithm routing, matching p81z's `FMArrangement::run()` exactly:

```rust
match algorithm {
    0 => { // 4→3→2→[1]
        ops[3].run(&zeros, &mut temp);
        ops[2].run(&temp, &mut temp);
        ops[1].run(&temp, &mut temp);
        ops[0].run(&temp, output);
    }
    1 => { // (3+4)→2→[1]
        ops[2].run(&zeros, &mut temp);
        ops[3].run_adding(&zeros, &mut temp);
        ops[1].run(&temp, &mut temp);
        ops[0].run(&temp, output);
    }
    // ... all 8 from p81z FMArrangement.cpp
}
```

- [ ] **Step 4: Run tests — verify pass**

- [ ] **Step 5: Commit**

```
feat(core): FmEngine with 8 TX81Z algorithms
```

---

## Task 6: FmParams and ParamSnapshot integration

**Files:**
- Modify: `chimera-core/src/params.rs`
- Modify: `chimera-core/tests/fm_test.rs`

- [ ] **Step 1: Add FmParams and FmOpParams to params.rs**

```rust
#[derive(Clone, Copy, Debug)]
pub struct FmOpParams {
    pub waveform: Param,
    pub coarse: Param,
    pub fine: Param,
    pub level: Param,
    pub feedback: Param,
    pub detune: Param,
    pub velocity_sens: Param,
    pub attack_rate: Param,
    pub decay1_rate: Param,
    pub decay1_level: Param,
    pub decay2_rate: Param,
    pub release_rate: Param,
    pub rate_scaling: Param,
}

#[derive(Clone, Copy, Debug)]
pub struct FmParams {
    pub algorithm: Param,
    pub operators: [FmOpParams; 4],
}
```

Add `pub fm: FmParams` to `ParamSnapshot`. Implement `Default` with TX81Z init patch values (algorithm 0, all operators at sine, ratio 1.0, level 0 except op1=99).

- [ ] **Step 2: Update FmEngine to read from FmParams**

Convert `FmEngine::render()` and `note_on()` to read from `FmParams` instead of `FmOpSettings`. Extract operator settings from Param values using `.value` field and cast to integer types.

- [ ] **Step 3: Run all tests**

Run: `just test`
Expected: all existing tests + FM tests pass

- [ ] **Step 4: Commit**

```
feat(core): FmParams in ParamSnapshot, wire to FmEngine
```

---

## Task 7: Wire FM into Voice

**Files:**
- Modify: `chimera-core/src/dsp/voice.rs`
- Modify: `chimera-core/tests/fm_test.rs`

- [ ] **Step 1: Add FmEngine to Voice struct**

Add `fm: FmEngine` field to `Voice`. Wire the three match arms:
- `note_on`: `EngineType::Fm => self.fm.note_on(note, velocity, &params.fm, sample_rate)`
- `note_off`: `EngineType::Fm => self.fm.note_off()`
- `render`: `EngineType::Fm => self.fm.render(output, &params.fm)`

- [ ] **Step 2: Write integration test through Voice**

```rust
#[test]
fn voice_fm_produces_sound() {
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Fm;
    params.fm.operators[0].level = Param::new(0.0, 99.0, 99.0);
    voice.note_on(69, 100, &params, 48000);
    let mut buf = [0.0f32; 1024];
    let mod_state = ModState::default();
    voice.render(&mut buf, &params, &mod_state, 48000);
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms > 0.01);
}

#[test]
fn voice_fm_click_free() {
    // Same pattern as existing click_free_test.rs
    let mut voice = Voice::new();
    let mut params = ParamSnapshot::default();
    params.engine = EngineType::Fm;
    params.fm.operators[0].level = Param::new(0.0, 99.0, 99.0);
    voice.note_on(69, 100, &params, 48000);
    let mod_state = ModState::default();
    let mut prev = 0.0f32;
    for _ in 0..100 {
        let mut buf = [0.0f32; 64];
        voice.render(&mut buf, &params, &mod_state, 48000);
        for &s in &buf {
            assert!((s - prev).abs() < 0.5, "click detected");
            prev = s;
        }
    }
}
```

- [ ] **Step 3: Build firmware**

Run: `just firmware`
Expected: compiles with no errors

- [ ] **Step 4: Run all tests**

Run: `just test`
Expected: all pass

- [ ] **Step 5: Commit**

```
feat(core): wire FM engine into Voice — EngineType::Fm produces sound
```

---

## Task 8: FM chain definition and UI blocks

**Files:**
- Modify: `chimera-core/src/ui/block_registry.rs`
- Modify: `chimera-core/src/ui/chain.rs`
- Modify: `chimera-core/src/ui/page.rs`

- [ ] **Step 1: Create FM block definitions**

In `block_registry.rs`, define:
- `FM_ALG` block: Algorithm page (6 param slots: algorithm, -, output level, -, -, -)
- `FM_OP` block: Operator Focus page (6 param slots: op select, waveform, level, feedback, detune, vel sens)
- `FM_RATIO` block: Ratios page (6 param slots: op1 coarse, op2 coarse, op3 coarse, op4 coarse, fine, -)
- `FM_CHAIN`: chain definition using FM block + Drive + Filter + Folder + Reverb, with FM_OP and FM_RATIO as sub-pages under the FM block

- [ ] **Step 2: Wire ChainType::Fm**

In `chain.rs`, change `ChainType::Fm` from fallback to `&block_registry::FM_CHAIN`.

- [ ] **Step 3: Add FM page IDs**

In `page.rs`, add page IDs for the FM pages so `PageId::from_nav()` resolves correctly and `read_values()` reads the FM params.

- [ ] **Step 4: Build and test**

Run: `just check && just test`
Expected: all pass, FM chain accessible via UI navigation

- [ ] **Step 5: Commit**

```
feat(core): FM chain definition with Algorithm, Operator, Ratio pages
```

---

## Task 9: FM init patch and preset integration

**Files:**
- Modify: `chimera-core/src/preset.rs`

- [ ] **Step 1: Add FM init patch**

In `Patch::init()`, when `chain_type == ChainType::Fm`, set `params.engine = EngineType::Fm` and configure a basic audible FM patch (algorithm 0, op1 as carrier at level 99, op2 as modulator at level 50, ratios 1:1).

- [ ] **Step 2: Test**

```rust
#[test]
fn fm_init_patch_is_audible() {
    let patch = Patch::init(ChainType::Fm);
    assert_eq!(patch.params.engine, EngineType::Fm);
    let mut voice = Voice::new();
    voice.note_on(69, 100, &patch.params, 48000);
    let mut buf = [0.0f32; 1024];
    voice.render(&mut buf, &patch.params, &patch.mod_state, 48000);
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / 1024.0).sqrt();
    assert!(rms > 0.01);
}
```

- [ ] **Step 3: Run tests + build firmware**

Run: `just test && just firmware`

- [ ] **Step 4: Commit**

```
feat(core): FM init patch — audible default for ChainType::Fm
```

---

## Task 10: Flash and hardware test

**Files:**
- No code changes — verification only

- [ ] **Step 1: Flash firmware**

Run: `just flash` (or DFU flash sequence)

- [ ] **Step 2: Test FM on hardware**

- Navigate to a track
- Open patch browser (Edit+B1)
- Load an FM init patch or switch engine to FM
- Verify: audible FM sound, encoders control FM params
- Test all 8 algorithms — each should sound different
- Test waveform changes — timbral variation
- Test ratio changes — harmonic content shifts
- Test feedback — increases brightness/harshness
- Verify no clicks on note on/off

- [ ] **Step 3: Commit any hardware-discovered fixes**
