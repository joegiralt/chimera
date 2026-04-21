# DSP Modulation Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the mod matrix into the audio render path so that the Envelope modulator actually affects destination parameters in real time — filter cutoff sweeps, drive amount changes, etc.

**Architecture:** A `ModState` struct shared between UI and audio holds the mod matrix amounts and enabled bits. During Voice::render(), the envelope computes its output once per block. For each active mod route, the envelope output × amount is applied as an offset to the destination param. The block processes with modified params. After processing, params are restored. The modulation is per-block (not per-sample) for CPU efficiency.

**Tech Stack:** Rust, `no_std`, `chimera-core` DSP, `chimera-stm32` DMA ISR

**Spec:** `docs/chimera-modulation-spec.md`

---

## Key Design Decisions

1. **Per-block modulation, not per-sample.** The envelope value is computed once per 64-sample block and held constant for the block. This saves CPU (one envelope tick per block vs 64) and is standard practice — the PreenFM3 does the same.

2. **Modulated param copy.** The ISR makes a shallow copy of the relevant `ParamSnapshot` fields, applies mod offsets, processes the block, then discards the copy. The original `ParamSnapshot` (owned by the UI) is never modified by the audio thread.

3. **Shared mod data via raw pointer.** Same approach as `ParamSnapshot` — the UI owns `MatrixState`, audio gets a read-only raw pointer. The amounts array is small (16×16 = 256 bytes) and reads are atomic at the byte level on Cortex-M7.

4. **Envelope owns its state in Voice, not in MatrixState.** The `Envelope` struct (ADSR state machine) lives on `Voice` as it does now. `MatrixState` only holds the routing (which source goes where with what amount).

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `chimera-core/src/modulation.rs` | **Create** | `ModState` struct (shared between UI and audio), `apply_modulation()` function |
| `chimera-core/src/dsp/voice.rs` | **Modify** | `render()` takes `&ModState`, applies mod offsets before each block process |
| `chimera-core/src/lib.rs` | **Modify** | Add `pub mod modulation;` |
| `chimera-stm32/src/audio.rs` | **Modify** | Share `ModState` pointer with audio ISR, pass to `voice.render()` |
| `chimera-stm32/src/main.rs` | **Modify** | Pass `&ui.matrix_state` to audio init |
| `chimera-core/src/ui/mod.rs` | **Modify** | Expose `matrix_state` for audio thread access |

---

### Task 1: Create ModState — the shared modulation data structure

**Files:**
- Create: `chimera-core/src/modulation.rs`
- Modify: `chimera-core/src/lib.rs`
- Test: `chimera-core/tests/modulation_test.rs`

The `ModState` is a compact, read-only (from audio thread's perspective) struct that contains everything the audio ISR needs to apply modulation. It's a subset of `MatrixState` — just the amounts and enabled bits, no UI cursor/scroll state.

- [ ] **Step 1: Write test**

```rust
// chimera-core/tests/modulation_test.rs
use chimera_core::modulation::ModState;

#[test]
fn mod_state_default_is_empty() {
    let ms = ModState::new();
    assert_eq!(ms.num_sources, 0);
    assert_eq!(ms.num_dests, 0);
}

#[test]
fn mod_state_add_route() {
    let mut ms = ModState::new();
    ms.num_sources = 1; // one envelope
    ms.add_dest(2, 0); // block 2 (filter), param 0 (cutoff)
    assert_eq!(ms.num_dests, 1);
    assert_eq!(ms.dests[0], (2, 0));
    
    ms.amounts[0][0] = 64; // Env 1 → Filter.Cutoff at +64
    assert_eq!(ms.mod_offset(0, 2, 0), 64.0 / 127.0); // ~0.504
}

#[test]
fn mod_offset_zero_when_no_route() {
    let ms = ModState::new();
    assert_eq!(ms.mod_offset(0, 2, 0), 0.0);
}
```

- [ ] **Step 2: Create `modulation.rs`**

```rust
// chimera-core/src/modulation.rs

/// Maximum modulators (envelope, LFOs, etc.)
pub const MAX_MOD_SOURCES: usize = 8;
/// Maximum modulation destinations
pub const MAX_MOD_DESTS: usize = 16;

/// Compact modulation state shared between UI and audio thread.
/// UI writes, audio reads. No heap, no pointers, just plain data.
#[derive(Clone, Debug)]
pub struct ModState {
    /// Number of active modulation sources
    pub num_sources: usize,
    /// Number of active destinations
    pub num_dests: usize,
    /// Destination mapping: (block_idx, param_idx) for each dest slot
    pub dests: [(u8, u8); MAX_MOD_DESTS],
    /// Amounts: [source_idx][dest_idx], -127 to +127
    pub amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
    /// Mod-enabled bitfield (same as MatrixState.mod_enabled)
    pub mod_enabled: u64,
}

impl ModState {
    pub fn new() -> Self {
        Self {
            num_sources: 0,
            num_dests: 0,
            dests: [(0, 0); MAX_MOD_DESTS],
            amounts: [[0; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
            mod_enabled: 0,
        }
    }

    /// Add a destination. Returns the dest index.
    pub fn add_dest(&mut self, block_idx: u8, param_idx: u8) -> usize {
        let idx = self.num_dests;
        if idx < MAX_MOD_DESTS {
            self.dests[idx] = (block_idx, param_idx);
            self.num_dests += 1;
        }
        idx
    }

    /// Get the total modulation offset for a specific block param.
    /// `source_values` contains the current output of each modulator (0.0-1.0 for env, -1.0-1.0 for LFO).
    /// Returns the sum of (source_value * amount/127) for all sources routed to this dest.
    pub fn compute_offset(&self, source_values: &[f32; MAX_MOD_SOURCES], block_idx: u8, param_idx: u8) -> f32 {
        let mut total = 0.0f32;
        for di in 0..self.num_dests {
            let (bi, pi) = self.dests[di];
            if bi == block_idx && pi == param_idx {
                for si in 0..self.num_sources {
                    let amt = self.amounts[si][di] as f32 / 127.0;
                    total += source_values[si] * amt;
                }
                break;
            }
        }
        total
    }

    /// Sync from MatrixState — copy amounts, dests, enabled bits.
    /// Called from UI thread after mod matrix changes.
    pub fn sync_from_matrix(&mut self, matrix: &crate::ui::mod_grid::MatrixState) {
        self.mod_enabled = matrix.mod_enabled;
        self.num_sources = matrix.num_sources;
        self.num_dests = matrix.num_dests;
        for i in 0..matrix.num_dests {
            if let Some(dest) = &matrix.dests[i] {
                self.dests[i] = (dest.block_idx, dest.param_idx);
            }
        }
        // Copy amounts
        for si in 0..MAX_MOD_SOURCES.min(crate::ui::mod_grid::MAX_SOURCES) {
            for di in 0..MAX_MOD_DESTS.min(crate::ui::mod_grid::MAX_DESTS) {
                self.amounts[si][di] = matrix.amounts[si][di];
            }
        }
    }
}
```

- [ ] **Step 3: Add `pub mod modulation;` to `chimera-core/src/lib.rs`**

- [ ] **Step 4: Run tests**

Run: `cargo test -p chimera-core modulation`

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/modulation.rs chimera-core/src/lib.rs chimera-core/tests/modulation_test.rs
git commit -m "feat(core): add ModState — shared modulation data for audio thread"
```

---

### Task 2: Wire ModState into Voice::render()

**Files:**
- Modify: `chimera-core/src/dsp/voice.rs`

The key change: `Voice::render()` accepts `&ModState` and applies modulation offsets to a **copy** of the relevant param fields before processing each block.

- [ ] **Step 1: Add ModState parameter to render()**

```rust
use crate::modulation::ModState;

pub fn render(
    &mut self,
    output: &mut [f32; BLOCK_SIZE],
    params: &ParamSnapshot,
    mod_state: &ModState,
    sample_rate: u32,
) {
```

- [ ] **Step 2: Compute envelope output once per block**

Before the processing chain, tick the envelope and compute modulator output values:

```rust
    // Compute modulator outputs (once per block)
    let mut mod_values = [0.0f32; crate::modulation::MAX_MOD_SOURCES];
    // Source 0 = amp envelope
    if mod_state.num_sources > 0 {
        // Tick envelope for one block (use the value at the start of block)
        mod_values[0] = self.amp_env.process(&params.envelopes[0], sample_rate);
    }
```

- [ ] **Step 3: Create modulated param copies and apply offsets**

```rust
    // Apply modulation offsets to param copies
    let mut mod_filter = params.filter;
    let mut mod_drive = params.drive;
    let mut mod_folder = params.folder;
    let mut mod_pizza = params.pizza;

    // Block 0 = Pizza (params: shape=0, crush=1, level=2)
    mod_pizza.shape += mod_state.compute_offset(&mod_values, 0, 0);
    mod_pizza.crush += mod_state.compute_offset(&mod_values, 0, 1);
    mod_pizza.level += mod_state.compute_offset(&mod_values, 0, 2);
    mod_pizza.shape = mod_pizza.shape.clamp(0.0, 1.0);
    mod_pizza.crush = mod_pizza.crush.clamp(0.0, 1.0);
    mod_pizza.level = mod_pizza.level.clamp(0.0, 1.0);

    // Block 1 = Drive (params: drive=0, tone=1)
    mod_drive.drive.value += mod_state.compute_offset(&mod_values, 1, 0) * (mod_drive.drive.max - mod_drive.drive.min);
    mod_drive.tone.value += mod_state.compute_offset(&mod_values, 1, 1) * (mod_drive.tone.max - mod_drive.tone.min);
    mod_drive.drive.value = mod_drive.drive.value.clamp(mod_drive.drive.min, mod_drive.drive.max);
    mod_drive.tone.value = mod_drive.tone.value.clamp(mod_drive.tone.min, mod_drive.tone.max);

    // Block 2 = Filter (params: cutoff=0, reso=1)
    mod_filter.cutoff.value += mod_state.compute_offset(&mod_values, 2, 0) * (mod_filter.cutoff.max - mod_filter.cutoff.min);
    mod_filter.resonance.value += mod_state.compute_offset(&mod_values, 2, 1) * (mod_filter.resonance.max - mod_filter.resonance.min);
    mod_filter.cutoff.value = mod_filter.cutoff.value.clamp(mod_filter.cutoff.min, mod_filter.cutoff.max);
    mod_filter.resonance.value = mod_filter.resonance.value.clamp(mod_filter.resonance.min, mod_filter.resonance.max);

    // Block 3 = Folder (params: fold=0, sym=1)
    mod_folder.fold.value += mod_state.compute_offset(&mod_values, 3, 0) * (mod_folder.fold.max - mod_folder.fold.min);
    mod_folder.symmetry.value += mod_state.compute_offset(&mod_values, 3, 1) * (mod_folder.symmetry.max - mod_folder.symmetry.min);
    mod_folder.fold.value = mod_folder.fold.value.clamp(mod_folder.fold.min, mod_folder.fold.max);
    mod_folder.symmetry.value = mod_folder.symmetry.value.clamp(mod_folder.symmetry.min, mod_folder.symmetry.max);
```

- [ ] **Step 4: Use modulated copies in the processing chain**

Replace:
```rust
    self.drive.process(output, &params.drive);
    self.filter.process(output, &params.filter, sample_rate);
    self.folder.process(output, &params.folder);
```

With:
```rust
    self.drive.process(output, &mod_drive);
    self.filter.process(output, &mod_filter, sample_rate);
    self.folder.process(output, &mod_folder);
```

And for the Pizza engine, use `&mod_pizza` instead of `&params.pizza`.

- [ ] **Step 5: Update the VCA section**

The VCA now uses the envelope output from `mod_values[0]` instead of calling `self.amp_env.process()` again (we already ticked it above):

```rust
    // VCA — use envelope value computed above
    let volume = params.volume.value;
    match self.active_engine {
        EngineType::Modal => {
            for sample in output.iter_mut() {
                *sample *= volume;
            }
        }
        _ => {
            let env_val = mod_values[0];
            for sample in output.iter_mut() {
                *sample *= env_val * volume;
            }
        }
    }
```

Wait — this only ticks the envelope once per block (64 samples), but the original code ticked it per-sample. For smooth envelope shapes, per-sample is better. Let me keep the per-sample envelope tick for the VCA (amplitude), and use the per-block value for mod destinations:

```rust
    // Compute mod source values (once per block, for modulation offsets)
    let mut mod_values = [0.0f32; crate::modulation::MAX_MOD_SOURCES];
    if mod_state.num_sources > 0 {
        // Sample the envelope level (don't advance — that happens in VCA loop)
        mod_values[0] = self.amp_env.current_level();
    }
```

Actually, simpler: compute the offset using the envelope's current level at the start of the block, apply offsets, then let the VCA section advance the envelope per-sample as before.

Add a `current_level()` method to Envelope:

```rust
pub fn current_level(&self) -> f32 {
    self.level * self.velocity
}
```

- [ ] **Step 6: Build and test**

Run: `cargo test -p chimera-core && cargo build -p chimera-desktop`

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/dsp/voice.rs chimera-core/src/dsp/envelope.rs
git commit -m "feat(core): Voice::render applies mod offsets from ModState"
```

---

### Task 3: Share ModState with audio ISR

**Files:**
- Modify: `chimera-stm32/src/audio.rs`
- Modify: `chimera-stm32/src/main.rs`
- Modify: `chimera-core/src/ui/mod.rs`

- [ ] **Step 1: Add ModState static to audio.rs**

```rust
use chimera_core::modulation::ModState;

/// Modulation state pointer — UI writes, ISR reads.
static mut MOD_STATE: Option<*const ModState> = None;
```

- [ ] **Step 2: Update init_voice to accept ModState pointer**

```rust
pub unsafe fn init_voice(params_ptr: *const ParamSnapshot, mod_ptr: *const ModState) {
    unsafe {
        addr_of_mut!(VOICE).write(Some(Voice::new()));
        addr_of_mut!(PARAMS).write(Some(params_ptr));
        addr_of_mut!(MOD_STATE).write(Some(mod_ptr));
    }
}
```

- [ ] **Step 3: Update render_block to pass ModState to voice.render()**

```rust
    let mod_state_ptr = addr_of_mut!(MOD_STATE);
    let mod_state = match *mod_state_ptr {
        Some(p) => &*p,
        None => {
            // No mod state — use empty default
            static EMPTY_MOD: ModState = ModState::new();
            &EMPTY_MOD
        }
    };

    voice.render(work, params, mod_state, chimera_hal::SAMPLE_RATE);
```

Note: `ModState::new()` needs to be `const fn` for the static default.

- [ ] **Step 4: Add ModState to UiState and sync it**

In `chimera-core/src/ui/mod.rs`, add a `ModState` field to `UiState`:

```rust
pub mod_state: ModState,
```

In the main loop (or in handle_input), sync the ModState from MatrixState whenever the matrix changes:

```rust
// After any matrix change (MIX+Plus/Minus, encoder E on grid)
self.mod_state.sync_from_matrix(&self.matrix_state);
```

- [ ] **Step 5: Update main.rs to pass ModState pointer**

```rust
unsafe { audio::init_voice(&ui.params as *const _, &ui.mod_state as *const _); }
```

- [ ] **Step 6: Build firmware**

Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 7: Commit**

```bash
git add chimera-stm32/src/audio.rs chimera-stm32/src/main.rs chimera-core/src/ui/mod.rs
git commit -m "feat(stm32): share ModState with audio ISR for live modulation"
```

---

### Task 4: Flash and verify on hardware

- [ ] **Step 1: Build and flash**

```bash
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave
```

- [ ] **Step 2: Test modulation**

1. B1 → Pizza page → turn Encoder A (Shape) → MIX+Plus to enable as mod dest
2. Plus to Filter page → turn Encoder A (Cutoff) → MIX+Plus to enable
3. Plus to Mod Matrix grid → navigate to Env 1 × PIZ.SHAP → Encoder E to set +64
4. Navigate to Env 1 × FLT.CUT → Encoder E to set +100
5. Press a note (or it's already playing the A4 test tone)

**Expected:** The filter cutoff should sweep following the envelope shape (attack up, decay to sustain, release down). The Pizza shape should also modulate.

- [ ] **Step 3: Verify no audio glitches**

Navigate between pages while modulation is active. Audio should remain continuous.

- [ ] **Step 4: Commit**

```bash
git add chimera.bin
git commit -m "feat(stm32): DSP modulation working — envelope sweeps filter cutoff"
```
