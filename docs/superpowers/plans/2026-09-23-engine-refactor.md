# Engine Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each synthesis block the single owner of its values, description (`ParamSpec`s), DSP and pages, address parameters semantically (`ParamAddr`), and dispatch engines in one place — without changing a single output sample except where the spec lists an intended change.

**Architecture:** A `Block` trait (`specs`/`get`/`write` + provided `set`/`nudge`/`snap`/`normalized`) is implemented by every values struct, with `static` spec tables in each block's own module. `ParamSnapshot::block(_mut)(BlockRef)` is the one exhaustive dispatch used by UI and modulation; `Voice` owns a persistent `Engines` struct and applies modulation generically through `apply_offset`. Part-chain pages bind slots to `ParamAddr`s (`SlotBinding`) and are identified by `PageKey`; golden hashes of the rendered audio lock the refactor bit-for-bit.

**Tech Stack:** Rust 2024 edition, `no_std` `chimera-core` (+ `libm`), `chimera-hal`, integration tests in `chimera-core/tests/*.rs` (`cargo test -p chimera-core`), rustc 1.100 nightly on this machine.

**Spec:** `docs/superpowers/specs/2026-09-23-engine-refactor-design.md` (read it before starting any task; this plan argues from it and follows its 8-step "Landing order").

## Global Constraints

- Every task ends with `cargo test -p chimera-core` fully green (the whole workspace test fails on this machine only because `chimera-desktop` needs ALSA headers).
- Goldens (`chimera-core/tests/golden_test.rs`) must match bit-for-bit after every task. The only deliberate re-record is Task 22 (FM pre-wire removal). Never "fix" a golden mismatch by re-recording.
- No `unsafe` without a `// SAFETY:` comment (CLAUDE.md).
- No heap allocation in the audio callback; the audio thread never blocks, never allocates (CLAUDE.md). `Voice::render`, `Engines`, `apply_offset`, `ModState::sum_for` use only stack/`static` data.
- No libc (CLAUDE.md). `chimera-core` stays `#![no_std]`; rounding uses `libm::roundf`.
- All parameter changes lerped in UI — never snap (CLAUDE.md): displayed values keep going through `AnimatedValue::set_target`.
- "A block's specs are a `const` array in its own module. No central table." — implemented as `pub static <BLOCK>_SPECS: [ParamSpec; N]` next to the values struct.
- "`set` clamps to `min..=max`; for `Stepped`/`Enum` it rounds to nearest (UI input only; the modulation path does not call `set`)."
- "`apply_offset` for `Continuous` and `Stepped` is exactly today's formula, `(v + off * (max - min)).clamp(min, max)`."
- "`Enum` … never modulatable." "`modulatable: true` = read by `Voice` per block."
- "`ParamSpec.default` is only the value a UI reset returns to. Initial values still come from each values struct's `Default` and from `Patch::init`." In this plan every spec's `default` equals its values struct's `Default` (tested).
- "Initial curves are linear only."
- "Patches are not persisted, so addresses can change freely." `ParamId`s are assigned here; once assigned they are never reused.
- Match surrounding code style (named consts, `///` docs, no new dependencies). Type-driven: make invalid states unrepresentable where cheap (`Op`, `MidiNote`, `Velocity`, private `ModState`/registry fields, private `ParamSnapshot::engine`).
- Environment: `cargo check -p chimera-desktop` fails here on `alsa-sys` — it needs `sudo apt install libasound2-dev`. The firmware check `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf` needs `rustup target add thumbv7em-none-eabihf` (not installed here). Tasks that edit those crates list the command; if the environment cannot run it, report "not compiled: <reason>" in the task report — never skip the edit.
- Commits: conventional style (`feat(core): …`, `refactor(core): …`, `test(core): …`, `fix(core): …`), each message ending with a blank line and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Stage only the files the task names.

## Review Focus

1. **Priming a mod destination on a Mixer/System/Demo page** (MIX + Plus): today it registers `Block{node,i}`, which `Voice` reads as Pizza/Drive/Filter/Folder. Expected: nothing is registered. Test: Task 19 (`priming_on_legacy_page_registers_nothing`).
2. **More than 16 primed destinations, or a matrix with more than 8 sources:** today `sync_from_matrix` copies `num_sources` up to 16 and `compute_offset` indexes `amounts[si]` past `MAX_MOD_SOURCES` (8) → panic in the audio ISR. Expected: `ModState` truncates to 16 dests / 8 sources, amounts stay aligned with their dests, no panic. Test: Task 16 (`mod_state_truncates_without_misaligning`, `mod_state_clamps_sources`).
3. **Shift-snap on `Stepped`/`Enum` params** (Modal MODE, FM coarse/level/RR, LFO SHAPE): snap now works on these pages (spec intended change). Expected: the value lands on an integer inside the range, never fractional. Tests: Task 3 (`modal_mode_snap_lands_on_integer`), Task 9 (`fm_snap_lands_on_integers`).
4. **MIDI boundary input:** note 127 accepted, 128 rejected; velocity 0 is a note-off (not a silent note-on); velocity 127 accepted. Test: Task 12 (`midi_types_test.rs`).
5. **Changing the selected FM operator after priming a `SelectedOp` slot:** the saved route must keep naming the operator that was selected when it was primed (spec: "A saved route always names a concrete operator"). Test: Task 20 (`selected_op_route_is_concrete`).

---

## Decisions and spec ambiguities resolved in this plan (review these first)

| # | Decision | Why |
|---|---|---|
| D1 | `Block` has a required `write(&mut self, id, v)` (raw store, no clamp/round) and **provided** `set`, `spec`, `normalized`, `nudge`, `snap`. | Spec §1 lists only `specs/get/set`, but `set` must round `Stepped` values while `apply_offset` must not (FM level stays fractional, DSP truncates). Something must write raw. |
| D2 | `ParamKind::Enum` is a unit variant; the choice count is `max + 1` with `min == 0`. Constructor `ParamSpec::choice(..)` forces `modulatable: false`. | Spec shows both "0..=max (min is 0)" and "`Enum { count: 4 }`". One representation. |
| D3 | **Needs your decision:** Modal MODE spec is `0..=3` (`Int(3)`), so the encoder now reaches mode 3 (Sympathetic); today it clamps at 2. Alternative: `max 2` + `ValFmt::Int(2)`. | Spec flags this "for a decision". With `max 3`, `Int(3)` also displays the true mode (today mode 2 displays "3"). |
| D4 | **Needs your decision:** FM `RR` spec range is `0..=15` (today's encoder clamps at 1). `ValFmt::Int(15)` requires `max - min == 15`. The FM envelope accepts 0 (`1 + rr*2`). Alternative: keep min 1 and change fmt to `Int(14)` (displays rr−1). | The spec's `Int(n)` test flags the mismatch "for fixing in the plan". |
| D5 | MODAL_2 BODY fmt `Int(3)` → `Uni` (it is a 0..1 float). | Spec names this mismatch. |
| D6 | `ParamSlot` keeps an optional `label_override: Option<&'static str>` (None = spec label). Used for FM Ratio "OP1".."OP4" (all four are `COARSE`) and ENVELOPE's "DEPTH". | Spec §5 says labels come from the spec; four identical "CRSE" labels on one page would be a regression. Format and step still always come from the spec. |
| D7 | `ParamAddr::modulatable()` = `spec.modulatable && block.voice_reads()`. `FilterEnv`/`AuxEnv` share `EnvParams`' specs (A/D/S/R `modulatable: true`) but `Voice` never reads them, so the address is not modulatable. | Specs are per block *type*; modulatability is per *address*. Keeps the "modulatable is true" test honest. |
| D8 | Bridge signature is `legacy_to_addr(chain: ChainType, path: ParamPath) -> Option<ParamAddr>` (no `sel_op`). | `ParamPath::FmOp` already carries the operator; `Block{..}` paths map to the node's main page. `sel_op` would be unused. |
| D9 | In step 5 the registry is still keyed by `ParamPath`; `ModDestRegistry::add` takes the `ChainType` so it can run the `modulatable` check through the bridge. Step 6 re-keys it by `ParamAddr`. | Spec step 5 requires the registry check while the UI still builds `ParamPath`. |
| D10 | Golden init cases use an empty `ModState` (spec), so FM pre-wire removal cannot change them. An extra case `fm_init_patch_mod` (FM init params + FM init `ModState`) is the golden that step 8 re-records; after Task 22 it must equal `fm_init`. | Spec says step 8 re-records "the FM golden"; only a case that uses the init `ModState` can change. |
| D11 | **Sanity gate result (measured while writing this plan at `c1dbd23` + Task 1's compile fix):** Pizza and FM pass. Modal fails: fundamental reads **+12.14 semitones** (waveform repeats every 91 samples at note 60) and the tail 200 blocks after note-off still peaks at **5.5e-3** (> 1e-4). Task 1 files `docs/issues/003-modal-sanity-gate.md`, marks those two Modal sanity tests `#[ignore]`, and lists the Modal goldens as known-broken. | Spec: "A failing engine gets an issue … its golden is still recorded and marked". |
| D12 | Task 1 first fixes a **compile error**: rustc 1.100 (edition 2024) denies `static_mut_refs` in `scope.rs`, so `cargo test -p chimera-core` does not build at `c1dbd23`. The fix takes references through `addr_of(_mut)!`; the data race the spec lists under "Known issues" stays. | The suite must run before goldens can be recorded. |
| D13 | `MidiNote`/`Velocity` live in `chimera-hal` (where `MidiMessage` is; hal cannot depend on core) and are re-exported as `chimera_core::{MidiNote, Velocity}`. | Spec: "created by the MIDI parser (the trust boundary, `chimera-hal` `MidiMessage` → core)". |
| D14 | `Voice::new(sample_rate)`; `Voice::note_on(note, vel, params)` and `Voice::render(out, params, mod_state)` lose their sample-rate argument. | Spec: sample rate "stored once, not per call". Keeping per-call rates on `Voice` while `Engines` stores one would allow disagreement. |
| D15 | `ParamSnapshot::engine` becomes a private field with `engine()` getter; set only via `ParamSnapshot::for_engine` / `Patch::init`. | Spec §6 "one source of truth"; makes a stray `params.engine = …` a compile error. |
| D16 | Only UI-bound params get specs. `ModalParams::{note, num_modes, ks_excitation, ks_color, bow_velocity, bow_force}` and `FilterParams::mode` have none (fields unchanged). | YAGNI; sub-project 2 (serialization) adds them. |
| D17 | `ModalParams::mode` becomes `ResonatorMode` (spec names it). LFO `shape`/`sync`, filter `mode`, chorus `mode`, reverb `reverb_type` stay `u8`. | Spec: "keeps real field types (`f32`, `u8`, and enums such as `ResonatorMode`)"; minimal churn elsewhere. |
| D18 | Field types for FM: `Stepped` **and** modulatable ⇒ `f32` (`level`, `feedback`: the modulated copy holds fractional values, DSP truncates `as u8`); non-modulatable Stepped/Enum ⇒ integer fields (`u8`/`i8`). | Bit-identical modulation; type-driven elsewhere. |
| D19 | Display-only side effects (no audio change): LFO RATE bar is `(rate−0.01)/19.99` (was `rate/20`); Delay TIME bar is `(t−10)/990` (was `t/1000`); Modal MODE bar is `mode/3` (was `mode/2`, which displayed mode 2 as "3"); matrix source rows read "ENV"/"LFO" (were "Envelope"/"LFO"; FM: "Op1 Env".."Op4 Env"); priming labels for FM envelope/ratio params become "O1 AR"/"O1 CRSE" (those params are not modulatable, so they are refused anyway). | Consequence of "label, format and step come from the spec". |
| D20 | `Renderer::update` (unused anywhere) is deleted in Task 20; `PageId::Efx` (only an unreachable Part-chain fallback) is deleted with the Part variants. `PageId::EnvAmp/EnvFilter/EnvAux` (unreachable, not listed by the spec) stay as legacy pages. | Spec lists which `PageId`s to delete; these are dead code that would otherwise need Part bindings. |
| D21 | The MIDI parser moves from `chimera-stm32/src/midi.rs` (compiled but never called) to `chimera-hal::midi`, unchanged except that it builds `MidiNote`/`Velocity`. | The spec makes the parser the trust boundary; in the firmware crate it cannot be tested on the host. |
| D22 | `mod_path.rs` keeps its name after `ParamPath` is deleted (it then holds only the registry). | Avoids churning every import; renaming is cosmetic and can ride with sub-project 2. |

### Encoder step audit (spec: "any deviation found during the audit is listed in the plan and approved explicitly")

Every spec `step` equals today's per-page step; `Param` pages used `(max−min)/128`.

| Page / params | Today | Spec `step` |
|---|---|---|
| Pizza SHAPE/CRUSH/LEVEL | `1/128` | `1.0 / 128.0` |
| Modal EXCITE…INHARM, BODY…E.MIX | `1/128` | `1.0 / 128.0` |
| Modal MODE | int ±1, clamp 0..2 | `1.0`, range 0..3 (**D3**) |
| Drive DRIVE/TONE/MIX, Folder FOLD/SYM/MIX, Filter RESO/DRIVE/FM/TRACK | `(1−0)/128` | `1.0 / 128.0` |
| Filter CUTOFF | `(20000−20)/128` | `(20000.0 - 20.0) / 128.0` |
| Filter ENV | `(1−(−1))/128` | `2.0 / 128.0` |
| Env ATK/DEC/REL | `(10−0.001)/128` | `(10.0 - 0.001) / 128.0` |
| Env SUS/LEVEL/VEL | `1/128` | `1.0 / 128.0` |
| LFO RATE | `0.15` | `0.15` |
| LFO SHAPE/SYNC | int ±1 | `1.0` |
| LFO PHASE/DEPTH | `1/128` | `1.0 / 128.0` |
| LFO OFST | `step * 2.0` with `step = 1/128` | `2.0 / 128.0` (exactly equal: ×2 is exact in IEEE-754) |
| FM ALG, WAVE, CRSE, FINE, LEVEL, FDBK, DETUN, V.SNS, AR, D1R, D1L, D2R, RR, RS | int ±1 | `1.0` (RR range **D4**) |
| Out volume (Mixer/Master/FM ALG), pan | `(1−0)/128`, `(1−(−1))/128` | `1.0 / 128.0`, `2.0 / 128.0` |
| Chorus MODE / RATE, DEPTH, MIX | int ±1 / `1/128` | `1.0` / `1.0 / 128.0` |
| Delay TIME / others | `8.0` / `1/128` | `8.0` / `1.0 / 128.0` |
| Reverb TYPE / others | int ±1 / `1/128` | `1.0` / `1.0 / 128.0` |

---

## File structure

New files:
- `chimera-core/src/block.rs` — `ValFmt` (moved from `ui/page.rs`, re-exported there), `ParamId`, `ParamKind`, `ParamSpec`, `Block` trait, `find_spec`, `apply_offset`. Generic; knows no concrete block.
- `chimera-core/src/addr.rs` — `Op`, `BlockRef`, `ParamAddr` (semantic addresses; `BlockRef::specs` is the only place mapping a block kind to its spec table).
- `chimera-core/src/dsp/engines.rs` — `Engines` (persistent Pizza/FM/Modal instances, one exhaustive `match` per method).
- `chimera-core/src/ui/part_page.rs` — read/encoder/snap for Part-chain pages, driven by `SlotBinding`.
- Tests: `chimera-core/tests/common/mod.rs` (render harness), `sanity_test.rs`, `golden_test.rs`, `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`, `midi_types_test.rs`, `engines_test.rs`, `addr_test.rs`, `modulatable_test.rs`, `binding_test.rs`, `ui_routing_test.rs`, `part_page_test.rs`, `engine_source_test.rs`.
- `docs/issues/003-modal-sanity-gate.md`.

Modified (each block's values + specs + `Block` impl live together): `params.rs` (Drive/Filter/Folder/Env/FmOp/Fm/Out + `ParamSnapshot`), `dsp/pizza.rs`, `dsp/modal.rs`, `dsp/lfo.rs`, `dsp/chorus.rs`, `dsp/delay.rs`, `dsp/reverb.rs`, DSP readers (`drive.rs`, `filter.rs`, `wavefolder.rs`, `envelope.rs`, `engine_fm.rs`), `dsp/voice.rs`, `modulation.rs`, `mod_path.rs` (bridge, then deleted), `preset.rs`, `ui/{page,mod,renderer,region,block_def,block_registry,mod_grid}.rs`, `scope.rs`, `chimera-hal/src/lib.rs`, `chimera-desktop/src/{main,audio}.rs`, `chimera-stm32/src/{main,audio,midi}.rs`, and existing tests (mechanical updates, rules given per task).

## Task list (spine = spec landing order)

| Spec step | Tasks |
|---|---|
| 1 Golden tests | 1 |
| 2 `ParamSpec`/`Block`, one block at a time; `Param` deleted | 2 Pizza (+ trait), 3 Modal, 4 Drive, 5 Filter, 6 Folder, 7 Env, 8 LFO, 9 FM, 10 Out, 11 FX + delete `Param` |
| 3 `Engines`, `MidiNote`/`Velocity` | 12 MIDI types, 13 `Engines` + `Voice` |
| 4 `ParamAddr`/`BlockRef`/`Op` + `block(_mut)` | 14 addresses, 15 `PageId::binding` |
| 5 `ModState` addresses, generic modulation, registry check, desktop `AudioShared`, bridge | 16 core, 17 desktop |
| 6 `SlotBinding` + `PageKey`; delete Part `PageId`s, `ParamPath`, bridge | 18 bindings, 19 address-based mod UI, 20 `PageKey` + Part pages |
| 7 `ChainType::engine` | 21 |
| 8 FM pre-wire removal + `mod_sources` | 22 |

---

### Task 1: Sanity gate and golden refactor lock

**Files:**
- Modify: `chimera-core/src/scope.rs:36-54` (compile fix only)
- Create: `chimera-core/tests/common/mod.rs`
- Create: `chimera-core/tests/sanity_test.rs`
- Create: `chimera-core/tests/golden_test.rs`
- Create: `docs/issues/003-modal-sanity-gate.md`

**Interfaces:**
- Consumes: today's API — `Voice::new()`, `Voice::note_on(u8, u8, &ParamSnapshot, u32)`, `Voice::render(&mut [f32; 64], &ParamSnapshot, &ModState, u32)`, `Voice::note_off()`, `ModState` public fields, `ParamPath`, `Patch::init(ChainType)`.
- Produces (test-only, `tests/common/mod.rs`; later tasks edit only the bodies of `init_params`, `lfo_route`, `setup` and `render_case`, never the constants or case list):
  - `pub enum Case { PizzaInit, PizzaLfoCutoff, FmInit, FmLfoCutoff, FmLfoOpALevel, FmInitPatchMod, ModalInit, ModalLfoCutoff, VaInit, PizzaToModalSwitch }`, `Case::ALL: [Case; 10]`, `Case::name(self) -> &'static str`
  - `pub fn init_params(engine: EngineType) -> ParamSnapshot`
  - `pub fn setup(case: Case) -> (ParamSnapshot, ModState)`
  - `pub fn render_case(case: Case) -> Vec<f32>` (exactly `TOTAL_SAMPLES` samples)
  - `pub fn fnv1a(samples: &[f32]) -> u64`, `pub fn spots(samples: &[f32]) -> [u32; 8]`
  - consts `SR = 48_000`, `NOTE: u8 = 60`, `VEL: u8 = 100`, `ON_BLOCKS = 200`, `OFF_BLOCKS = 200`, `TOTAL_SAMPLES`, `MOD_LFO_RATE = 5.0`, `MOD_AMOUNT: i8 = 64`, `SPOT_IDX: [usize; 8]`
  - `golden_test.rs`: `const GOLDENS: &[(&str, u64, [u32; 8])]`, `const KNOWN_BROKEN: &[(&str, &str)]`; record path `GOLDEN_RECORD=1`.

- [ ] **Step 1: Confirm the suite does not build**

Run: `cargo test -p chimera-core 2>&1 | grep -A3 '^error'`
Expected: `error: creating a mutable reference to mutable static` at `chimera-core/src/scope.rs:42` and `error: creating a shared reference to mutable static` at `scope.rs:52`.

- [ ] **Step 2: Fix the two references in `scope.rs`**

Replace, inside `write_samples`, the line
```rust
            FRONT.copy_from_slice(&BACK[trigger..trigger + SCOPE_LEN]);
```
with
```rust
            // SAFETY: FRONT/BACK are only written from the audio thread (single
            // writer). References are created through raw pointers, never to the
            // `static mut` directly. The UI-side race is a known issue (spec
            // § Known issues) and does not affect audio output.
            let front = &mut *core::ptr::addr_of_mut!(FRONT);
            let back = &*core::ptr::addr_of!(BACK);
            front.copy_from_slice(&back[trigger..trigger + SCOPE_LEN]);
```
and, inside `read_samples`, replace `out.copy_from_slice(&FRONT);` with
```rust
        // SAFETY: shared reference created through a raw pointer; a torn read
        // only affects the oscilloscope display.
        out.copy_from_slice(&*core::ptr::addr_of!(FRONT));
```

- [ ] **Step 3: Run the existing suite**

Run: `cargo test -p chimera-core 2>&1 | grep '^test result' | awk '{p+=$4; f+=$6} END {print p, "passed", f, "failed"}'`
Expected: `280 passed 0 failed`.

- [ ] **Step 4: Write the shared render harness**

Create `chimera-core/tests/common/mod.rs`:
```rust
//! Fixed render harness shared by the refactor lock (`golden_test.rs`) and the
//! sanity gate (`sanity_test.rs`).
//! Spec: docs/superpowers/specs/2026-09-23-engine-refactor-design.md § Testing.
//!
//! Harness: fresh `Voice` per case (RNG seeds are per instance), 48 kHz,
//! note 60 vel 100 on, ON_BLOCKS blocks, note off, OFF_BLOCKS blocks.
#![allow(dead_code)]

use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::ParamPath;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::{ChainType, Patch};
use chimera_hal::BLOCK_SIZE;

pub const SR: u32 = 48_000;
pub const NOTE: u8 = 60;
pub const VEL: u8 = 100;
pub const ON_BLOCKS: usize = 200;
pub const OFF_BLOCKS: usize = 200;
pub const TOTAL_SAMPLES: usize = (ON_BLOCKS + OFF_BLOCKS) * BLOCK_SIZE;
/// LFO rate for every modulated case. At the 1 Hz default the LFO stays
/// positive for the first 0.5 s, so a route to a param already at its max
/// (cutoff 20 kHz, FM op A level 99) would clamp and never be exercised.
pub const MOD_LFO_RATE: f32 = 5.0;
/// Matrix amount (−127..=127) for every modulated case.
pub const MOD_AMOUNT: i8 = 64;

/// Every golden case. `name()` is the key in `golden_test.rs::GOLDENS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    PizzaInit,
    PizzaLfoCutoff,
    FmInit,
    FmLfoCutoff,
    FmLfoOpALevel,
    /// FM init params with the FM init patch's own `ModState` (the pre-wire).
    /// The only case Task 22 (pre-wire removal) re-records.
    FmInitPatchMod,
    ModalInit,
    ModalLfoCutoff,
    VaInit,
    /// Pizza init; engine switched to Modal at block ON_BLOCKS / 2 (mid-note).
    PizzaToModalSwitch,
}

impl Case {
    pub const ALL: [Case; 10] = [
        Case::PizzaInit,
        Case::PizzaLfoCutoff,
        Case::FmInit,
        Case::FmLfoCutoff,
        Case::FmLfoOpALevel,
        Case::FmInitPatchMod,
        Case::ModalInit,
        Case::ModalLfoCutoff,
        Case::VaInit,
        Case::PizzaToModalSwitch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Case::PizzaInit => "pizza_init",
            Case::PizzaLfoCutoff => "pizza_lfo_cutoff",
            Case::FmInit => "fm_init",
            Case::FmLfoCutoff => "fm_lfo_cutoff",
            Case::FmLfoOpALevel => "fm_lfo_op_a_level",
            Case::FmInitPatchMod => "fm_init_patch_mod",
            Case::ModalInit => "modal_init",
            Case::ModalLfoCutoff => "modal_lfo_cutoff",
            Case::VaInit => "va_init",
            Case::PizzaToModalSwitch => "pizza_to_modal_switch",
        }
    }
}

/// Init params per engine: the chain's `Patch::init` params for the three
/// real engines; defaults with `engine = Va` for Va (it has no chain).
pub fn init_params(engine: EngineType) -> ParamSnapshot {
    match engine {
        EngineType::Pizza => Patch::init(ChainType::PizzaPoly).params,
        EngineType::Fm => Patch::init(ChainType::Fm).params,
        EngineType::Modal => Patch::init(ChainType::Modal).params,
        EngineType::Va => {
            let mut p = ParamSnapshot::default();
            p.engine = EngineType::Va;
            p
        }
    }
}

/// Filter cutoff at today's DSP path. `Voice::render` maps `Block{2,0}` to
/// filter cutoff on every chain — including Modal, whose UI would emit
/// `Block{1,0}` (spec § Intended behavior changes).
pub const CUTOFF: ParamPath = ParamPath::Block { block: 2, param: 0 };
/// FM operator A level at today's DSP path.
pub const OP_A_LEVEL: ParamPath = ParamPath::FmOp { op: 0, param: 2 };

/// One LFO (source 1) route at MOD_AMOUNT to `dest`; env is source 0 so
/// `num_sources >= 2` and the LFO runs.
pub fn lfo_route(dest: ParamPath) -> ModState {
    let mut ms = ModState::new();
    ms.num_sources = 2;
    ms.num_dests = 1;
    ms.dests[0] = dest;
    ms.amounts[1][0] = MOD_AMOUNT;
    ms
}

/// Params + ModState for a case (the switch's second half is in `render_case`).
pub fn setup(case: Case) -> (ParamSnapshot, ModState) {
    let with_lfo = |engine: EngineType, dest: ParamPath| {
        let mut p = init_params(engine);
        p.lfo.rate = MOD_LFO_RATE;
        (p, lfo_route(dest))
    };
    match case {
        Case::PizzaInit => (init_params(EngineType::Pizza), ModState::new()),
        Case::PizzaLfoCutoff => with_lfo(EngineType::Pizza, CUTOFF),
        Case::FmInit => (init_params(EngineType::Fm), ModState::new()),
        Case::FmLfoCutoff => with_lfo(EngineType::Fm, CUTOFF),
        Case::FmLfoOpALevel => with_lfo(EngineType::Fm, OP_A_LEVEL),
        Case::FmInitPatchMod => {
            let patch = Patch::init(ChainType::Fm);
            (patch.params, patch.mod_state)
        }
        Case::ModalInit => (init_params(EngineType::Modal), ModState::new()),
        Case::ModalLfoCutoff => with_lfo(EngineType::Modal, CUTOFF),
        Case::VaInit => (init_params(EngineType::Va), ModState::new()),
        Case::PizzaToModalSwitch => (init_params(EngineType::Pizza), ModState::new()),
    }
}

/// Params the switch case renders with from block ON_BLOCKS / 2 on.
fn switched_params(params: &ParamSnapshot) -> ParamSnapshot {
    let mut p = params.clone();
    p.engine = EngineType::Modal;
    p
}

/// Render the fixed harness for one case. Returns TOTAL_SAMPLES samples.
pub fn render_case(case: Case) -> Vec<f32> {
    let (params, mod_state) = setup(case);
    let switched = switched_params(&params);
    let mut voice = Voice::new();
    voice.note_on(NOTE, VEL, &params, SR);
    let mut out = Vec::with_capacity(TOTAL_SAMPLES);
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..ON_BLOCKS + OFF_BLOCKS {
        if b == ON_BLOCKS {
            voice.note_off();
        }
        let p = if case == Case::PizzaToModalSwitch && b >= ON_BLOCKS / 2 {
            &switched
        } else {
            &params
        };
        voice.render(&mut block, p, &mod_state, SR);
        out.extend_from_slice(&block);
    }
    out
}

/// FNV-1a 64 over the little-endian bytes of each sample's `f32::to_bits`.
pub fn fnv1a(samples: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Spot-check indices: start, attack, sustain, around note-off, release, end.
pub const SPOT_IDX: [usize; 8] = [0, 1_000, 5_000, 10_000, 12_799, 12_800, 19_200, 25_599];

pub fn spots(samples: &[f32]) -> [u32; 8] {
    SPOT_IDX.map(|i| samples[i].to_bits())
}
```

- [ ] **Step 5: Write the sanity gate**

Create `chimera-core/tests/sanity_test.rs`:
```rust
//! Sanity gate (spec § Testing), run before goldens are recorded.
//! Per engine init patch: finite, within ±1.0, not silent, silent after
//! note-off; pitched engines' fundamental within one semitone of the note.
//! A failing engine gets an issue in docs/issues/ and its failing test is
//! marked `#[ignore = "known broken: …"]`. It is not fixed in this refactor.

mod common;

use chimera_hal::BLOCK_SIZE;
use common::*;

/// −60 dBFS.
const AUDIBLE: f32 = 1e-3;
/// −80 dBFS.
const SILENT: f32 = 1e-4;
/// Trailing blocks of the render that must be silent.
const TAIL_BLOCKS: usize = 10;

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Perceived fundamental via normalized autocorrelation: the shortest lag in
/// 40..=1200 Hz whose correlation is within 90 % of the best lag, refined to
/// its local maximum. A waveform that repeats every half period of the note
/// reads as an octave up — which is what a listener hears.
fn fundamental_hz(s: &[f32]) -> f32 {
    let (min_lag, max_lag) = (SR as usize / 1200, SR as usize / 40);
    let n = s.len() - max_lag;
    let acf = |lag: usize| -> f32 {
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (a, b) = (s[i] as f64, s[i + lag] as f64);
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        if xx == 0.0 || yy == 0.0 { 0.0 } else { (xy / (xx * yy).sqrt()) as f32 }
    };
    let r: Vec<f32> = (0..=max_lag).map(|l| if l < min_lag { 0.0 } else { acf(l) }).collect();
    let best = r[min_lag..].iter().copied().fold(f32::MIN, f32::max);
    let mut lag = (min_lag..=max_lag).find(|&l| r[l] >= 0.9 * best).unwrap();
    while lag < max_lag && r[lag + 1] > r[lag] {
        lag += 1;
    }
    SR as f32 / lag as f32
}

fn assert_finite_bounded_audible(case: Case) {
    let out = render_case(case);
    assert!(out.iter().all(|x| x.is_finite()), "{}: non-finite sample", case.name());
    let pk = peak(&out);
    assert!(pk <= 1.0, "{}: peak {pk} exceeds ±1.0", case.name());
    let on = peak(&out[..ON_BLOCKS * BLOCK_SIZE]);
    assert!(on > AUDIBLE, "{}: silent during note-on (peak {on})", case.name());
}

fn assert_silent_after_note_off(case: Case) {
    let out = render_case(case);
    let tail = peak(&out[TOTAL_SAMPLES - TAIL_BLOCKS * BLOCK_SIZE..]);
    assert!(tail < SILENT, "{}: tail peak {tail} after note-off", case.name());
}

fn assert_pitched(case: Case) {
    let out = render_case(case);
    let f = fundamental_hz(&out[50 * BLOCK_SIZE..150 * BLOCK_SIZE]);
    let target = chimera_core::dsp::note_to_freq(NOTE);
    let semis = 12.0 * (f / target).log2();
    assert!(
        semis.abs() < 1.0,
        "{}: fundamental {f} Hz is {semis:+.2} semitones from note {NOTE} ({target} Hz)",
        case.name()
    );
}

#[test]
fn pizza_is_finite_bounded_audible() { assert_finite_bounded_audible(Case::PizzaInit); }
#[test]
fn pizza_is_silent_after_note_off() { assert_silent_after_note_off(Case::PizzaInit); }
#[test]
fn pizza_is_pitched() { assert_pitched(Case::PizzaInit); }

#[test]
fn fm_is_finite_bounded_audible() { assert_finite_bounded_audible(Case::FmInit); }
#[test]
fn fm_is_silent_after_note_off() { assert_silent_after_note_off(Case::FmInit); }
#[test]
fn fm_is_pitched() { assert_pitched(Case::FmInit); }

#[test]
fn modal_is_finite_bounded_audible() { assert_finite_bounded_audible(Case::ModalInit); }
#[test]
fn modal_is_silent_after_note_off() { assert_silent_after_note_off(Case::ModalInit); }
#[test]
fn modal_is_pitched() { assert_pitched(Case::ModalInit); }

/// Va is a placeholder engine: it must render exact silence, never garbage.
#[test]
fn va_is_silent_placeholder() {
    let out = render_case(Case::VaInit);
    assert!(out.iter().all(|&x| x == 0.0), "va_init must be exact silence");
}
```

- [ ] **Step 6: Run the sanity gate**

Run: `cargo test -p chimera-core --test sanity_test 2>&1 | grep -E '^test |panicked|semitones|tail peak'`
Expected: all pass except
- `modal_is_pitched` FAILED: `modal_init: fundamental 527.47 Hz is +12.14 semitones from note 60 (261.63 Hz)`
- `modal_is_silent_after_note_off` FAILED: `modal_init: tail peak 0.0053… after note-off`

(If other tests fail, stop: add each failure to the issue in Step 7 with its measured numbers and ignore that test the same way.)

- [ ] **Step 7: File the issue and mark the known-broken tests**

Create `docs/issues/003-modal-sanity-gate.md`:
```markdown
# Issue #003: Modal engine fails the refactor sanity gate

Found by `chimera-core/tests/sanity_test.rs` (engine refactor, sub-project 1),
Modal init patch (`Patch::init(ChainType::Modal)`: mode String/KS+, pluck
position 0.0), note 60 vel 100, 48 kHz, 200 blocks on, 200 blocks off.

## Findings (measured at c1dbd23 + scope compile fix)

1. **Plays an octave high.** Normalized autocorrelation over blocks 50..150
   is 0.998 at a lag of 91 samples (527.5 Hz) and 0.999 at 183 (262 Hz): the
   waveform repeats every half period, so the fundamental is (almost) absent
   and note 60 is heard as note 72 (+12.14 semitones).
2. **Rings after note-off.** Peak per block after note-off: block 200 0.038,
   250 0.0083, 300 0.0071, 350 0.0062, 399 0.0053 — it does not reach
   −80 dBFS (1e-4) within 200 blocks (0.27 s).

## Status

Not fixed in the engine refactor. The goldens `modal_init`,
`modal_lfo_cutoff` and `pizza_to_modal_switch` lock this output bit-for-bit
(`golden_test.rs::KNOWN_BROKEN`). `modal_is_pitched` and
`modal_is_silent_after_note_off` are `#[ignore]`d until this is fixed; the
fix must re-record those goldens deliberately.
```

In `sanity_test.rs`, add the attribute line directly above `fn modal_is_silent_after_note_off` and above `fn modal_is_pitched` (below their `#[test]`):
```rust
#[ignore = "known broken: docs/issues/003-modal-sanity-gate.md"]
```

- [ ] **Step 8: Run the sanity gate again**

Run: `cargo test -p chimera-core --test sanity_test 2>&1 | grep '^test result'`
Expected: `test result: ok. 8 passed; 0 failed; 2 ignored`

- [ ] **Step 9: Write the golden test with an empty table**

Create `chimera-core/tests/golden_test.rs`:
```rust
//! Refactor lock (spec 2026-09-23 § Testing).
//!
//! Goldens freeze today's output, good or bad — they are not a quality claim.
//! They must match bit-for-bit after every refactor step. Re-record ONLY for
//! a change the spec lists as intended, in the task that makes it:
//!
//!     GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test -- --nocapture
//!
//! and paste the printed rows over `GOLDENS`.

mod common;

use common::*;

/// (case name, FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX).
const GOLDENS: &[(&str, u64, [u32; 8])] = &[];

/// Goldens that lock output which failed the sanity gate.
const KNOWN_BROKEN: &[(&str, &str)] = &[
    ("modal_init", "docs/issues/003-modal-sanity-gate.md"),
    ("modal_lfo_cutoff", "docs/issues/003-modal-sanity-gate.md"),
    ("pizza_to_modal_switch", "docs/issues/003-modal-sanity-gate.md"),
];

#[test]
fn goldens_match() {
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let mut failures = Vec::new();
    for case in Case::ALL {
        let out = render_case(case);
        let (hash, sp) = (fnv1a(&out), spots(&out));
        if record {
            println!("    (\"{}\", 0x{hash:016x}, {sp:?}),", case.name());
            continue;
        }
        match GOLDENS.iter().find(|g| g.0 == case.name()) {
            None => failures.push(format!(
                "{}: no golden recorded (run with GOLDEN_RECORD=1)",
                case.name()
            )),
            Some(&(_, want_hash, want_spots)) => {
                if hash != want_hash || sp != want_spots {
                    failures.push(format!(
                        "{}: hash 0x{hash:016x} (want 0x{want_hash:016x}), spots {sp:?} (want {want_spots:?})",
                        case.name()
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "golden mismatch:\n{}", failures.join("\n"));
}

#[test]
fn known_broken_goldens_have_issues() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (case, issue) in KNOWN_BROKEN {
        assert!(Case::ALL.iter().any(|c| c.name() == *case), "unknown case {case}");
        assert!(root.join(issue).exists(), "{case}: missing {issue}");
    }
}

#[test]
fn harness_is_deterministic() {
    for case in [Case::PizzaInit, Case::FmInit, Case::ModalInit] {
        assert_eq!(fnv1a(&render_case(case)), fnv1a(&render_case(case)), "{}", case.name());
    }
}

/// A route that silently does nothing would make its golden a copy of the
/// unmodulated one and lock nothing.
#[test]
fn modulated_cases_differ_from_unmodulated() {
    for (modulated, plain) in [
        (Case::PizzaLfoCutoff, Case::PizzaInit),
        (Case::FmLfoCutoff, Case::FmInit),
        (Case::FmLfoOpALevel, Case::FmInit),
        (Case::ModalLfoCutoff, Case::ModalInit),
    ] {
        assert_ne!(
            fnv1a(&render_case(modulated)),
            fnv1a(&render_case(plain)),
            "{} renders the same as {}",
            modulated.name(),
            plain.name()
        );
    }
}
```

- [ ] **Step 10: Run it to see it fail**

Run: `cargo test -p chimera-core --test golden_test 2>&1 | grep -E 'no golden recorded|^test result'`
Expected: `goldens_match` FAILED with ten `…: no golden recorded (run with GOLDEN_RECORD=1)` lines; the other three tests pass.

- [ ] **Step 11: Record and paste the goldens**

Run: `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test goldens_match -- --nocapture 2>&1 | grep '^    ("'`

Paste the ten printed rows between the brackets of `const GOLDENS: &[(&str, u64, [u32; 8])] = &[ … ];`. The values measured while writing this plan (x86_64, rustc 1.100 nightly) were:
```rust
    ("pizza_init", 0xc533d18a26331e4c, [3111930299, 1053289385, 999362770, 1051778009, 3163126675, 3167254354, 1028737615, 0]),
    ("pizza_lfo_cutoff", 0x54c74c6f8ae46bd4, [3111930299, 1053289385, 995579318, 1051778010, 3163126675, 3167254354, 1028758405, 0]),
    ("fm_init", 0x9bfe44d54ef0385b, [898059883, 1045152839, 1054792150, 3163439516, 3202136915, 3201882817, 0, 0]),
    ("fm_lfo_cutoff", 0x34b678f3574b1538, [898059883, 1045152839, 1054642467, 3163439516, 3202136915, 3201882817, 0, 0]),
    ("fm_lfo_op_a_level", 0x2016ba5789cfbf28, [898059883, 1045152839, 1049001337, 3163439517, 3202136915, 3201882817, 0, 0]),
    ("fm_init_patch_mod", 0xadc0aa292dba2808, [898059883, 1045018283, 1048839351, 1049419518, 3201107490, 3194638958, 0, 0]),
    ("modal_init", 0x90f1197c153d0b05, [3146805428, 3183273506, 3195882399, 1063217482, 3191764060, 3172592491, 993906163, 1000698095]),
    ("modal_lfo_cutoff", 0x40afa2290a49dae2, [3146805428, 3183273506, 3196374285, 1063217482, 3191764060, 3172592491, 993770242, 999762977]),
    ("va_init", 0x5125674880996325, [0, 0, 0, 0, 0, 0, 0, 0]),
    ("pizza_to_modal_switch", 0x2b4f3ce20159fef6, [3111930299, 1053289385, 999362770, 1051584936, 3196878310, 3185521127, 3117844736, 984628055]),
```
If your recorded rows differ from these, use yours (they are the lock for your toolchain) and note the difference in the task report.

- [ ] **Step 12: Run the golden test and the full suite**

Run: `cargo test -p chimera-core --test golden_test 2>&1 | grep '^test result'`
Expected: `test result: ok. 4 passed; 0 failed`
Run: `cargo test -p chimera-core 2>&1 | grep '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print p, "passed", f, "failed", i, "ignored"}'`
Expected: `292 passed 0 failed 2 ignored`

- [ ] **Step 13: Commit**

```bash
git add chimera-core/src/scope.rs chimera-core/tests/common/mod.rs chimera-core/tests/sanity_test.rs chimera-core/tests/golden_test.rs docs/issues/003-modal-sanity-gate.md
git commit -m "test(core): sanity gate and golden refactor lock for all engines

Fixes the static_mut_refs compile error in scope.rs so the suite builds on
rustc 1.100. Modal fails the sanity gate (octave high, rings after
note-off): issue 003; its goldens lock the current output.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 2: `Block` trait and `ParamSpec`; convert Pizza

**Files:**
- Create: `chimera-core/src/block.rs`
- Modify: `chimera-core/src/lib.rs` (add `pub mod block;`)
- Modify: `chimera-core/src/ui/page.rs:1-39` (move `ValFmt` out, re-export), `PageId::read_values`/`apply_encoder`/`snap_encoder`, delete `apply_pizza_encoder`
- Modify: `chimera-core/src/dsp/pizza.rs` (ids, `PIZZA_SPECS`, `Block` impl)
- Modify: `chimera-core/src/dsp/voice.rs:129-138` (Pizza offsets through `apply_offset`)
- Create: `chimera-core/tests/block_test.rs`, `chimera-core/tests/block_spec_test.rs`, `chimera-core/tests/page_block_test.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces (used by every later task):
  - `chimera_core::block::ValFmt` (same enum/impl as today; `chimera_core::ui::page::ValFmt` keeps working via `pub use`)
  - `pub struct ParamId(pub u8)` — `Clone, Copy, Debug, PartialEq, Eq`, `#[repr(transparent)]`
  - `pub enum ParamKind { Continuous, Stepped, Enum }`
  - `pub struct ParamSpec { pub id: ParamId, pub label: &'static str, pub fmt: ValFmt, pub min: f32, pub max: f32, pub default: f32, pub step: f32, pub kind: ParamKind, pub modulatable: bool }`
  - `ParamSpec::continuous(id: u8, label, fmt, min, max, default, step, modulatable) -> Self` (const), `ParamSpec::stepped(id: u8, label, fmt, min, max, default, modulatable) -> Self` (const, step 1.0), `ParamSpec::choice(id: u8, label, fmt, max, default) -> Self` (const, `Enum`, min 0, step 1.0, not modulatable), `ParamSpec::quantize(&self, v: f32) -> f32`, `ParamSpec::normalize(&self, v: f32) -> f32`
  - `pub fn find_spec(specs: &'static [ParamSpec], id: ParamId) -> Option<&'static ParamSpec>`
  - `pub trait Block { fn specs(&self) -> &'static [ParamSpec]; fn get(&self, id: ParamId) -> f32; fn write(&mut self, id: ParamId, v: f32); /* provided: */ fn spec(&self, id) -> Option<&'static ParamSpec>; fn set(&mut self, id, v: f32); fn normalized(&self, id) -> f32; fn nudge(&mut self, id, delta: i8); fn snap(&mut self, id, delta: i8); }`
  - `pub fn apply_offset(blk: &mut dyn Block, id: ParamId, off: f32)`
  - `PizzaParams::{SHAPE, CRUSH, LEVEL}: ParamId` (0, 1, 2); `pub static PIZZA_SPECS: [ParamSpec; 3]`
  - In `ui/page.rs` (private, extended by Tasks 3–11): `fn resolve_mut<'a>(&self, idx: usize, params: &'a mut ParamSnapshot) -> Option<(&'a mut dyn Block, ParamId)>`, `fn bind<'a>(b: &'a mut dyn Block, id: ParamId) -> Option<(&'a mut dyn Block, ParamId)>`, `fn read_block<const N: usize>(b: &dyn Block, ids: [ParamId; N]) -> [f32; 6]`, `const PIZZA_PAGE: [ParamId; 3]`
  - Test helpers in `tests/block_test.rs`: `fn conforms(name: &str, b: impl Block)` (defaults match spec, round-trip, clamping); `tests/block_spec_test.rs`: `fn all_specs() -> Vec<(&'static str, &'static [ParamSpec])>`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/block_test.rs`:
```rust
//! `Block` trait semantics (spec §1) and per-block conformance.

use chimera_core::block::{apply_offset, Block, ParamId, ParamKind, ParamSpec, ValFmt};
use chimera_core::dsp::pizza::PizzaParams;

/// A block with one param of each kind.
#[derive(Default)]
struct Probe {
    c: f32,
    s: f32,
    e: u8,
}

const C: ParamId = ParamId(0);
const S: ParamId = ParamId(1);
const E: ParamId = ParamId(2);

static PROBE_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "C", ValFmt::Bi, -1.0, 1.0, 0.0, 0.25, true),
    ParamSpec::stepped(1, "S", ValFmt::Int(10), 0.0, 10.0, 5.0, true),
    ParamSpec::choice(2, "E", ValFmt::Int(3), 3.0, 0.0),
];

impl Block for Probe {
    fn specs(&self) -> &'static [ParamSpec] {
        &PROBE_SPECS
    }
    fn get(&self, id: ParamId) -> f32 {
        match id {
            C => self.c,
            S => self.s,
            E => self.e as f32,
            _ => 0.0,
        }
    }
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            C => self.c = v,
            S => self.s = v,
            E => self.e = v as u8,
            _ => {}
        }
    }
}

#[test]
fn choice_is_enum_and_never_modulatable() {
    assert_eq!(PROBE_SPECS[2].kind, ParamKind::Enum);
    assert_eq!(PROBE_SPECS[2].min, 0.0);
    assert!(!PROBE_SPECS[2].modulatable);
}

#[test]
fn set_clamps_to_range() {
    let mut p = Probe::default();
    p.set(C, 5.0);
    assert_eq!(p.c, 1.0);
    p.set(C, -5.0);
    assert_eq!(p.c, -1.0);
}

#[test]
fn set_keeps_continuous_fraction() {
    let mut p = Probe::default();
    p.set(C, 0.3);
    assert_eq!(p.c, 0.3);
}

#[test]
fn set_rounds_stepped_and_enum() {
    let mut p = Probe::default();
    p.set(S, 4.6);
    assert_eq!(p.s, 5.0);
    p.set(S, 4.4);
    assert_eq!(p.s, 4.0);
    p.set(E, 2.5);
    assert_eq!(p.e, 3);
    p.set(E, 7.0);
    assert_eq!(p.e, 3);
}

#[test]
fn nudge_moves_by_spec_step_and_clamps() {
    let mut p = Probe { c: 0.0, s: 5.0, e: 0 };
    p.nudge(C, 2);
    assert_eq!(p.c, 0.5);
    p.nudge(S, -1);
    assert_eq!(p.s, 4.0);
    p.nudge(E, 5);
    assert_eq!(p.e, 3);
}

#[test]
fn snap_follows_format_points() {
    // Bi points (normalized): 0, 20/127, 64/127, 107/127, 1. c = 0.0 is n = 0.5.
    let mut p = Probe::default();
    p.snap(C, 1);
    assert_eq!(p.c, -1.0 + (107.0 / 127.0) * 2.0);
    // Int points: 0, 1 → jumps to max / min.
    p.s = 5.0;
    p.snap(S, 1);
    assert_eq!(p.s, 10.0);
    p.snap(S, -1);
    assert_eq!(p.s, 0.0);
}

#[test]
fn normalized_uses_spec_range() {
    let p = Probe::default();
    assert_eq!(p.normalized(C), 0.5);
}

#[test]
fn unknown_id_is_inert() {
    let mut p = Probe::default();
    let x = ParamId(9);
    assert!(p.spec(x).is_none());
    assert_eq!(p.get(x), 0.0);
    p.set(x, 1.0);
    p.nudge(x, 1);
    p.snap(x, 1);
    apply_offset(&mut p, x, 1.0);
    assert_eq!((p.c, p.s, p.e), (0.0, 0.0, 0));
}

#[test]
fn apply_offset_is_the_old_formula_and_never_rounds() {
    let mut p = Probe { c: 0.0, s: 5.0, e: 0 };
    apply_offset(&mut p, S, 0.03);
    assert_eq!(p.s, (5.0f32 + 0.03f32 * (10.0 - 0.0)).clamp(0.0, 10.0));
    assert_ne!(p.s, 5.0); // fractional: Stepped modulation is not rounded
    apply_offset(&mut p, C, -0.1);
    assert_eq!(p.c, (0.0f32 + -0.1f32 * (1.0 - -1.0)).clamp(-1.0, 1.0));
    apply_offset(&mut p, C, 2.0);
    assert_eq!(p.c, 1.0);
}

// ── Per-block conformance ────────────────────────────────────────────

/// Every spec's `default` equals the values struct's `Default` (Global Constraints).
fn assert_defaults(name: &str, b: &dyn Block) {
    for s in b.specs() {
        assert_eq!(b.get(s.id), s.default, "{name}.{}: Default vs spec default", s.label);
    }
}

/// `get` returns what `set` stored at both ends; out-of-range input clamps.
fn assert_roundtrip(name: &str, b: &mut dyn Block) {
    for s in b.specs() {
        for (input, want) in [(s.max, s.max), (s.min, s.min), (s.max + 1000.0, s.max), (s.min - 1000.0, s.min)] {
            b.set(s.id, input);
            assert_eq!(b.get(s.id), want, "{name}.{}: set({input})", s.label);
        }
    }
}

fn conforms(name: &str, mut b: impl Block) {
    assert_defaults(name, &b);
    assert_roundtrip(name, &mut b);
}

#[test]
fn pizza_conforms() {
    conforms("pizza", PizzaParams::default());
}
```

Create `chimera-core/tests/block_spec_test.rs`:
```rust
//! Every spec table is well-formed (spec § Testing "Specs").

use chimera_core::block::{ParamKind, ParamSpec, ValFmt};

/// Every block's spec table. Tasks 3–11 add one row each; Task 14 replaces
/// the list with `BlockRef::ALL`.
fn all_specs() -> Vec<(&'static str, &'static [ParamSpec])> {
    vec![("pizza", &chimera_core::dsp::pizza::PIZZA_SPECS[..])]
}

fn check(name: &str, specs: &[ParamSpec]) {
    for (i, s) in specs.iter().enumerate() {
        let what = format!("{name}.{}", s.label);
        assert!(specs[..i].iter().all(|o| o.id != s.id), "{what}: duplicate id {:?}", s.id);
        assert!(s.min < s.max, "{what}: min {} >= max {}", s.min, s.max);
        assert!(s.default >= s.min && s.default <= s.max, "{what}: default {} out of range", s.default);
        assert!(s.step > 0.0, "{what}: step {} <= 0", s.step);
        if s.kind == ParamKind::Enum {
            assert!(!s.modulatable, "{what}: Enum params are never modulatable");
            assert_eq!(s.min, 0.0, "{what}: Enum min must be 0");
        }
        if let ValFmt::Int(n) = s.fmt {
            assert!(s.kind != ParamKind::Continuous, "{what}: Int({n}) on a Continuous param");
            assert_eq!(s.max - s.min, n as f32, "{what}: Int({n}) but range is {}..={}", s.min, s.max);
        }
    }
}

#[test]
fn every_spec_table_is_well_formed() {
    for (name, specs) in all_specs() {
        check(name, specs);
    }
}
```

Create `chimera-core/tests/page_block_test.rs`:
```rust
//! Page encoders, snap and display after moving onto `Block` specs.
//! Parity tests pin today's step sizes (plan § Encoder step audit).

use chimera_core::params::ParamSnapshot;
use chimera_core::ui::page::{PageId, ValFmt};

#[test]
fn pizza_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Pizza.apply_encoder(0, 3, &mut p);
    assert_eq!(p.pizza.shape, 0.5 + 3.0 * (1.0 / 128.0));
    PageId::Pizza.apply_encoder(2, 127, &mut p);
    assert_eq!(p.pizza.level, 1.0);
}

/// Spec § Intended behavior changes: shift-snap now works on Pizza.
#[test]
fn pizza_snap_now_works() {
    let mut p = ParamSnapshot::default();
    PageId::Pizza.snap_encoder(1, 1, ValFmt::Uni, &mut p);
    assert_eq!(p.pizza.crush, 100.0 / 127.0);
}

#[test]
fn pizza_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Pizza.read_values(&p), [0.5, 0.0, 0.8, 0.0, 0.0, 0.0]);
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error|FAILED|panicked' | head`
Expected: `error[E0432]: unresolved import `chimera_core::block`` (block_test, block_spec_test) and `pizza_snap_now_works` FAILED (crush stays 0.0).

- [ ] **Step 3: Create `chimera-core/src/block.rs`**

```rust
//! Parameter description and access, implemented once per block type.
//!
//! Spec: docs/superpowers/specs/2026-09-23-engine-refactor-design.md §1.
//! Each values struct keeps real field types and implements `Block`; its
//! `ParamSpec` table is a `static` in the block's own module.

/// Display format and shift-snap behavior of a parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValFmt {
    /// Unipolar: 0 to 127. Snaps: 0, 100, 127.
    Uni,
    /// Bipolar: -64 to +63. Snaps: -64, -44, 0, +43, +63.
    Bi,
    /// Discrete integer 0..N. N is stored in the variant.
    /// Display shows the integer directly. Snaps at each integer.
    Int(u8),
}

impl ValFmt {
    /// Coarse snap points in normalized 0..1 space.
    pub fn snap_points(self) -> &'static [f32] {
        match self {
            ValFmt::Uni => &[0.0, 100.0 / 127.0, 1.0],
            ValFmt::Bi => &[0.0, 20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0],
            // Discrete: shift-encoder jumps to 0 or max
            ValFmt::Int(_) => &[0.0, 1.0],
        }
    }

    pub fn is_bipolar(self) -> bool {
        matches!(self, ValFmt::Bi)
    }

    /// Max integer value (only meaningful for Int variant).
    pub fn max_int(self) -> u8 {
        match self {
            ValFmt::Int(n) => n,
            _ => 127,
        }
    }
}

/// Identifies a parameter within its block type. Stable; never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ParamId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// Any value in `min..=max`.
    Continuous,
    /// Integer-valued. UI input rounds; a modulated copy stays fractional and
    /// the DSP truncates as it does today (FM level `as u8`).
    Stepped,
    /// Discrete choice `0..=max` (`min` is 0). Never modulatable.
    Enum,
}

/// Description of one parameter. Lives in flash (`static` tables).
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub id: ParamId,
    pub label: &'static str,
    /// Display format, set explicitly to today's per-slot format.
    pub fmt: ValFmt,
    pub min: f32,
    pub max: f32,
    /// UI reset value only. Initial values come from the values struct's
    /// `Default` and from `Patch::init`.
    pub default: f32,
    /// Value change per encoder tick (today's per-page step).
    pub step: f32,
    pub kind: ParamKind,
    /// True only if `Voice` reads the param per block (see `ParamAddr::modulatable`).
    pub modulatable: bool,
}

impl ParamSpec {
    #[allow(clippy::too_many_arguments)]
    pub const fn continuous(
        id: u8,
        label: &'static str,
        fmt: ValFmt,
        min: f32,
        max: f32,
        default: f32,
        step: f32,
        modulatable: bool,
    ) -> Self {
        Self { id: ParamId(id), label, fmt, min, max, default, step, kind: ParamKind::Continuous, modulatable }
    }

    /// Integer-valued, one unit per encoder tick.
    pub const fn stepped(
        id: u8,
        label: &'static str,
        fmt: ValFmt,
        min: f32,
        max: f32,
        default: f32,
        modulatable: bool,
    ) -> Self {
        Self { id: ParamId(id), label, fmt, min, max, default, step: 1.0, kind: ParamKind::Stepped, modulatable }
    }

    /// Discrete choice `0..=max`, one choice per tick. Never modulatable.
    pub const fn choice(id: u8, label: &'static str, fmt: ValFmt, max: f32, default: f32) -> Self {
        Self {
            id: ParamId(id),
            label,
            fmt,
            min: 0.0,
            max,
            default,
            step: 1.0,
            kind: ParamKind::Enum,
            modulatable: false,
        }
    }

    /// `v` clamped to the range; Stepped/Enum rounded to nearest (UI input).
    pub fn quantize(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        match self.kind {
            ParamKind::Continuous => v,
            ParamKind::Stepped | ParamKind::Enum => libm::roundf(v),
        }
    }

    /// `v` mapped to 0..1 over the range.
    pub fn normalize(&self, v: f32) -> f32 {
        (v - self.min) / (self.max - self.min)
    }
}

/// Look up a spec by id in a block's table.
pub fn find_spec(specs: &'static [ParamSpec], id: ParamId) -> Option<&'static ParamSpec> {
    specs.iter().find(|s| s.id == id)
}

/// Values + description of one block. `get`/`write` convert between the
/// struct's real field types and `f32` at this boundary; DSP code reads the
/// fields directly. Unknown ids read 0.0 and ignore writes.
pub trait Block {
    fn specs(&self) -> &'static [ParamSpec];
    fn get(&self, id: ParamId) -> f32;
    /// Store `v` with no clamping or rounding (integer fields truncate with
    /// `as`). Only `set` and `apply_offset` call this.
    fn write(&mut self, id: ParamId, v: f32);

    fn spec(&self, id: ParamId) -> Option<&'static ParamSpec> {
        find_spec(self.specs(), id)
    }

    /// UI input: clamps to `min..=max`; Stepped/Enum round to nearest.
    fn set(&mut self, id: ParamId, v: f32) {
        if let Some(s) = self.spec(id) {
            self.write(id, s.quantize(v));
        }
    }

    /// 0..1 display value.
    fn normalized(&self, id: ParamId) -> f32 {
        self.spec(id).map_or(0.0, |s| s.normalize(self.get(id)))
    }

    /// One encoder turn: `delta` ticks of the spec's step.
    fn nudge(&mut self, id: ParamId, delta: i8) {
        if let Some(s) = self.spec(id) {
            self.set(id, self.get(id) + delta as f32 * s.step);
        }
    }

    /// Shift+encoder: jump to the next snap point of the spec's format.
    fn snap(&mut self, id: ParamId, delta: i8) {
        let Some(s) = self.spec(id) else { return };
        let n = s.normalize(self.get(id));
        let points = s.fmt.snap_points();
        let target = if delta > 0 {
            points.iter().copied().find(|&sp| sp > n + 0.005).unwrap_or(1.0)
        } else {
            points.iter().rev().copied().find(|&sp| sp < n - 0.005).unwrap_or(0.0)
        };
        self.set(id, s.min + target * (s.max - s.min));
    }
}

/// Apply a modulation offset (spec §4): `(v + off * (max - min)).clamp(min, max)`,
/// written raw. Bit-identical to the former `Param::apply_mod_offset`; never
/// rounds (a Stepped value stays fractional). Audio-thread safe: no allocation.
pub fn apply_offset(blk: &mut dyn Block, id: ParamId, off: f32) {
    if let Some(s) = blk.spec(id) {
        let v = (blk.get(id) + off * (s.max - s.min)).clamp(s.min, s.max);
        blk.write(id, v);
    }
}
```

- [ ] **Step 4: Register the module and move `ValFmt`**

In `chimera-core/src/lib.rs`, add `pub mod block;` as the first `pub mod` line.

In `chimera-core/src/ui/page.rs`, replace lines 1-39 (the three `use` lines and the whole `ValFmt` enum + `impl ValFmt`) with:
```rust
use crate::block::{Block, ParamId};
use crate::dsp::pizza::PizzaParams;
use crate::params::ParamSnapshot;
use crate::preset::ChainType;
use crate::ui::chain::ChainNav;

pub use crate::block::ValFmt;
```

- [ ] **Step 5: Give Pizza its spec and `Block` impl**

In `chimera-core/src/dsp/pizza.rs`, add after `use chimera_hal::BLOCK_SIZE;`:
```rust
use crate::block::{Block, ParamId, ParamSpec, ValFmt};
```
and after `impl Default for PizzaParams { … }`:
```rust
impl PizzaParams {
    pub const SHAPE: ParamId = ParamId(0);
    pub const CRUSH: ParamId = ParamId(1);
    pub const LEVEL: ParamId = ParamId(2);
}

/// All three are read by `Voice` every block, so all are modulatable.
pub static PIZZA_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "SHAPE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "CRUSH", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "LEVEL", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, true),
];

impl Block for PizzaParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &PIZZA_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::SHAPE => self.shape,
            Self::CRUSH => self.crush,
            Self::LEVEL => self.level,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::SHAPE => self.shape = v,
            Self::CRUSH => self.crush = v,
            Self::LEVEL => self.level = v,
            _ => {}
        }
    }
}
```

- [ ] **Step 6: Route the Pizza page through the spec**

In `chimera-core/src/ui/page.rs`:

1. In `read_values`, replace the `PageId::Pizza => [ … ],` arm with
```rust
            PageId::Pizza => read_block(&params.pizza, PIZZA_PAGE),
```
2. In `apply_encoder`, delete the `PageId::Pizza => { apply_pizza_encoder(idx, delta, &mut params.pizza); return; }` arm and insert as the first statement of the function body:
```rust
        if let Some((blk, id)) = self.resolve_mut(idx, params) {
            blk.nudge(id, delta);
            return;
        }
```
3. Replace the body of `snap_encoder` with:
```rust
        if let Some((blk, id)) = self.resolve_mut(idx, params) {
            blk.snap(id, delta);
            return;
        }
        if let Some(param) = self.resolve_param_mut(idx, params) {
            param.snap_to(delta, fmt.snap_points());
        }
```
4. Add this method inside `impl PageId`, directly after `snap_encoder`:
```rust
    /// Block + param bound to encoder `idx`, for pages whose block implements
    /// `Block`. Tasks 3–11 add one arm per converted block.
    fn resolve_mut<'a>(
        &self,
        idx: usize,
        params: &'a mut ParamSnapshot,
    ) -> Option<(&'a mut dyn Block, ParamId)> {
        let p = params;
        match self {
            PageId::Pizza => bind(&mut p.pizza, *PIZZA_PAGE.get(idx)?),
            _ => None,
        }
    }
```
5. Delete `fn apply_pizza_encoder`. Add these items right after the closing `}` of `impl PageId`:
```rust
/// Encoder slot → param id, per page. Shared by `read_values` and `resolve_mut`.
const PIZZA_PAGE: [ParamId; 3] = [PizzaParams::SHAPE, PizzaParams::CRUSH, PizzaParams::LEVEL];

/// Coercion point so every `resolve_mut` arm has the same type.
fn bind<'a>(b: &'a mut dyn Block, id: ParamId) -> Option<(&'a mut dyn Block, ParamId)> {
    Some((b, id))
}

/// Normalized values of `ids` on `b`, padded with 0.0 to six slots.
fn read_block<const N: usize>(b: &dyn Block, ids: [ParamId; N]) -> [f32; 6] {
    let mut out = [0.0f32; 6];
    for (o, id) in out.iter_mut().zip(ids) {
        *o = b.normalized(id);
    }
    out
}
```

- [ ] **Step 7: Pizza modulation through `apply_offset` (bit-identical: `off * (1.0 - 0.0) == off`)**

In `chimera-core/src/dsp/voice.rs`, change `use crate::dsp::pizza::PizzaOsc;` to `use crate::dsp::pizza::{PizzaOsc, PizzaParams};`, add `use crate::block::apply_offset;`, and replace the three `mod_pizza.… = (… + …).clamp(0.0, 1.0);` lines with:
```rust
        apply_offset(&mut mod_pizza, PizzaParams::SHAPE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 0, param: 0 }));
        apply_offset(&mut mod_pizza, PizzaParams::CRUSH, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 0, param: 1 }));
        apply_offset(&mut mod_pizza, PizzaParams::LEVEL, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 0, param: 2 }));
```

- [ ] **Step 8: Run the new tests, the goldens, and the suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: four `ok` lines (block_test 10 passed, block_spec_test 1, page_block_test 3, golden_test 4).
Run: `cargo test -p chimera-core 2>&1 | grep -E '^test result|FAILED' | awk '/FAILED/ {print} {f+=$6} END {print f, "failed"}'`
Expected: `0 failed`.

- [ ] **Step 9: Commit**

```bash
git add chimera-core/src/block.rs chimera-core/src/lib.rs chimera-core/src/ui/page.rs chimera-core/src/dsp/pizza.rs chimera-core/src/dsp/voice.rs chimera-core/tests/block_test.rs chimera-core/tests/block_spec_test.rs chimera-core/tests/page_block_test.rs
git commit -m "feat(core): Block trait and ParamSpec; Pizza owns its spec

Pizza page encoders, snap and display now go through the spec; shift-snap
works on the Pizza page. Pizza modulation uses apply_offset (bit-identical).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 3: Convert Modal (`mode` becomes `ResonatorMode`)

**Files:**
- Modify: `chimera-core/src/dsp/modal.rs` (`ModalParams.mode` type, ids, `MODAL_SPECS`, `Block` impl, `note_on`)
- Modify: `chimera-core/src/ui/page.rs` (EngineModal1/2 via `resolve_mut`; delete `apply_modal1_encoder`, `apply_modal2_encoder`)
- Modify: `chimera-core/src/ui/block_registry.rs:52` (MODAL_2 BODY format, plan D5)
- Modify (mechanical): `chimera-core/tests/{click_free_test,desktop_sim_test,live_param_test,modal_integration_test,property_test,stress_test,modal_test}.rs`
- Test: `chimera-core/tests/block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Task 2's `Block`, `ParamSpec::{choice, continuous}`, `bind`, `read_block`, `resolve_mut`.
- Produces: `ModalParams.mode: ResonatorMode`; `ModalParams::{MODE, EXCITE, DECAY, BRIGHTNESS, POSITION, INHARM, KS_BODY, KS_STIFFNESS, KS_FEEDBACK, KS_ENS_DEPTH, KS_ENS_RATE, KS_ENS_MIX}: ParamId` (0..=11); `pub static MODAL_SPECS: [ParamSpec; 12]`; page consts `MODAL1_PAGE`, `MODAL2_PAGE: [ParamId; 6]`.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/block_test.rs`:
```rust
#[test]
fn modal_conforms() {
    conforms("modal", chimera_core::dsp::modal::ModalParams::default());
}
```
In `chimera-core/tests/block_spec_test.rs`, make `all_specs` return:
```rust
    vec![
        ("pizza", &chimera_core::dsp::pizza::PIZZA_SPECS[..]),
        ("modal", &chimera_core::dsp::modal::MODAL_SPECS[..]),
    ]
```
Append to `chimera-core/tests/page_block_test.rs`:
```rust
// ── Modal (Task 3) ───────────────────────────────────────────────────

use chimera_core::dsp::modal::ResonatorMode;

#[test]
fn modal_float_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::EngineModal1.apply_encoder(1, 2, &mut p);
    assert_eq!(p.modal.excite, 0.8 + 2.0 * (1.0 / 128.0));
    PageId::EngineModal2.apply_encoder(5, -1, &mut p);
    assert_eq!(p.modal.ks_ens_mix, 0.0);
}

/// Plan D3: the MODE encoder now reaches Sympathetic (was clamped at Bowed).
#[test]
fn modal_mode_reaches_sympathetic() {
    let mut p = ParamSnapshot::default();
    p.modal.mode = ResonatorMode::Bowed;
    PageId::EngineModal1.apply_encoder(0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    PageId::EngineModal1.apply_encoder(0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
}

/// Review Focus 3: snap on an Enum lands on a valid choice.
#[test]
fn modal_mode_snap_lands_on_integer() {
    let mut p = ParamSnapshot::default();
    PageId::EngineModal1.snap_encoder(0, 1, ValFmt::Int(3), &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    PageId::EngineModal1.snap_encoder(0, -1, ValFmt::Int(3), &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::String);
}

/// Plan D19: MODE displays mode/3 (mode 2 used to display as "3").
#[test]
fn modal_mode_display_is_true_value() {
    let mut p = ParamSnapshot::default();
    p.modal.mode = ResonatorMode::Bowed;
    assert_eq!(PageId::EngineModal1.read_values(&p)[0], 2.0 / 3.0);
}

/// Plan D5: BODY is a 0..1 float, displayed Uni.
#[test]
fn modal2_body_is_uni() {
    assert_eq!(chimera_core::ui::block_registry::MODAL_2.params[0].format, ValFmt::Uni);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value MODAL_SPECS`, `the trait bound ModalParams: Block is not satisfied`, `mismatched types` (`expected u8, found ResonatorMode`).

- [ ] **Step 3: Convert `ModalParams`**

In `chimera-core/src/dsp/modal.rs`:
1. After `use chimera_hal::BLOCK_SIZE;` add `use crate::block::{Block, ParamId, ParamSpec, ValFmt};`.
2. Field: `pub mode: u8, // 0=String(KS+), 1=Modal, 2=Bowed` → `pub mode: ResonatorMode,`.
3. `Default`: `mode: 0, // String (KS+) by default` → `mode: ResonatorMode::String,`.
4. `ModalEngine::note_on`: `self.active_mode = ResonatorMode::from_u8(params.mode);` → `self.active_mode = params.mode;`.
5. Insert before the `// ── Modal Engine ──…` banner:
```rust
impl ModalParams {
    pub const MODE: ParamId = ParamId(0);
    pub const EXCITE: ParamId = ParamId(1);
    pub const DECAY: ParamId = ParamId(2);
    pub const BRIGHTNESS: ParamId = ParamId(3);
    pub const POSITION: ParamId = ParamId(4);
    pub const INHARM: ParamId = ParamId(5);
    pub const KS_BODY: ParamId = ParamId(6);
    pub const KS_STIFFNESS: ParamId = ParamId(7);
    pub const KS_FEEDBACK: ParamId = ParamId(8);
    pub const KS_ENS_DEPTH: ParamId = ParamId(9);
    pub const KS_ENS_RATE: ParamId = ParamId(10);
    pub const KS_ENS_MIX: ParamId = ParamId(11);
}

/// Modal params are read at note-on (or by the engine from the unmodulated
/// snapshot), never from `Voice`'s modulated copy: none are modulatable.
/// Only UI-bound params have specs (plan D16). MODE max 3 is plan D3.
pub static MODAL_SPECS: [ParamSpec; 12] = [
    ParamSpec::choice(0, "MODE", ValFmt::Int(3), 3.0, 0.0),
    ParamSpec::continuous(1, "EXCITE", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DECAY", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "BRIGHT", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "POS", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "INHARM", ValFmt::Uni, 0.0, 1.0, 0.25, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "BODY", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(7, "STIFF", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(8, "FDBK", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false),
    ParamSpec::continuous(9, "E.DPT", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(10, "E.RAT", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(11, "E.MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for ModalParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &MODAL_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as u8 as f32,
            Self::EXCITE => self.excite,
            Self::DECAY => self.decay,
            Self::BRIGHTNESS => self.brightness,
            Self::POSITION => self.position,
            Self::INHARM => self.inharm,
            Self::KS_BODY => self.ks_body,
            Self::KS_STIFFNESS => self.ks_stiffness,
            Self::KS_FEEDBACK => self.ks_feedback,
            Self::KS_ENS_DEPTH => self.ks_ens_depth,
            Self::KS_ENS_RATE => self.ks_ens_rate,
            Self::KS_ENS_MIX => self.ks_ens_mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODE => self.mode = ResonatorMode::from_u8(v as u8),
            Self::EXCITE => self.excite = v,
            Self::DECAY => self.decay = v,
            Self::BRIGHTNESS => self.brightness = v,
            Self::POSITION => self.position = v,
            Self::INHARM => self.inharm = v,
            Self::KS_BODY => self.ks_body = v,
            Self::KS_STIFFNESS => self.ks_stiffness = v,
            Self::KS_FEEDBACK => self.ks_feedback = v,
            Self::KS_ENS_DEPTH => self.ks_ens_depth = v,
            Self::KS_ENS_RATE => self.ks_ens_rate = v,
            Self::KS_ENS_MIX => self.ks_ens_mix = v,
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Route the Modal pages through the spec**

In `chimera-core/src/ui/page.rs`:
1. Add `use crate::dsp::modal::ModalParams;` above the `PizzaParams` import.
2. In `read_values`, replace the `PageId::EngineModal1 => [ … ],` and `PageId::EngineModal2 => [ … ],` arms with:
```rust
            PageId::EngineModal1 => read_block(&params.modal, MODAL1_PAGE),
            PageId::EngineModal2 => read_block(&params.modal, MODAL2_PAGE),
```
3. In `apply_encoder`, delete the `PageId::EngineModal1 => { … return; }` and `PageId::EngineModal2 => { … return; }` arms.
4. In `resolve_mut`, add below the Pizza arm:
```rust
            PageId::EngineModal1 => bind(&mut p.modal, *MODAL1_PAGE.get(idx)?),
            PageId::EngineModal2 => bind(&mut p.modal, *MODAL2_PAGE.get(idx)?),
```
5. Add below `const PIZZA_PAGE …`:
```rust
const MODAL1_PAGE: [ParamId; 6] = [
    ModalParams::MODE,
    ModalParams::EXCITE,
    ModalParams::DECAY,
    ModalParams::BRIGHTNESS,
    ModalParams::POSITION,
    ModalParams::INHARM,
];
const MODAL2_PAGE: [ParamId; 6] = [
    ModalParams::KS_BODY,
    ModalParams::KS_STIFFNESS,
    ModalParams::KS_FEEDBACK,
    ModalParams::KS_ENS_DEPTH,
    ModalParams::KS_ENS_RATE,
    ModalParams::KS_ENS_MIX,
];
```
6. Delete `fn apply_modal1_encoder` and `fn apply_modal2_encoder`.

In `chimera-core/src/ui/block_registry.rs`, MODAL_2's first slot: `format: ValFmt::Int(3)` → `format: ValFmt::Uni` (plan D5).

- [ ] **Step 5: Update test sites (mechanical)**

Rule: an integer assigned to `modal.mode` becomes the `ResonatorMode` variant with that discriminant (0 String, 1 Modal, 2 Bowed, 3 Sympathetic).
```bash
cd chimera-core/tests
grep -l 'modal\.mode = [0-3];' *.rs | xargs sed -i 's/modal\.mode = 0;/modal.mode = ResonatorMode::String;/; s/modal\.mode = 1;/modal.mode = ResonatorMode::Modal;/; s/modal\.mode = 2;/modal.mode = ResonatorMode::Bowed;/; s/modal\.mode = 3;/modal.mode = ResonatorMode::Sympathetic;/'
sed -i 's/p\.modal\.mode = rng\.u8(2);/p.modal.mode = ResonatorMode::from_u8(rng.u8(2));/; s/if params\.modal\.mode == 2 {/if params.modal.mode == ResonatorMode::Bowed {/; s/modal_mode={} note={}/modal_mode={:?} note={}/; s/engine={:?} mode={}",/engine={:?} mode={:?}",/' property_test.rs
sed -i 's/^use chimera_core::dsp::modal::{ModalEngine, ModalParams};/use chimera_core::dsp::modal::{ModalEngine, ModalParams, ResonatorMode};/; s/    p\.mode = 1; \/\/ Modal resonator/    p.mode = ResonatorMode::Modal; \/\/ Modal resonator/' modal_test.rs
# files that now name ResonatorMode but do not import it yet
grep -l 'ResonatorMode::' *.rs | xargs grep -L 'use chimera_core::dsp::modal::.*ResonatorMode' | xargs sed -i '0,/^use chimera_core::/s//use chimera_core::dsp::modal::ResonatorMode;\nuse chimera_core::/'
cd ../..
```
Then `cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4` must print nothing.

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `11 passed`, `1 passed`, `8 passed`, `4 passed`, all `ok`.
Run: `cargo test -p chimera-core 2>&1 | grep -E '^test result' | awk '{f+=$6} END {print f, "failed"}'`
Expected: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/dsp/modal.rs chimera-core/src/ui/page.rs chimera-core/src/ui/block_registry.rs chimera-core/tests/
git commit -m "refactor(core): Modal owns its spec; mode is a ResonatorMode

MODE spans all four resonator modes (was clamped at Bowed); BODY displays
as a 0..1 value. Shift-snap now works on the Modal pages.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 4: Convert Drive (`Param` → `f32`)

**Files:**
- Modify: `chimera-core/src/params.rs` (imports; `DriveParams` + ids + `DRIVE_SPECS` + `Block`)
- Modify: `chimera-core/src/dsp/drive.rs:22-30` (read fields)
- Modify: `chimera-core/src/dsp/voice.rs:141-142` (drive offsets via `apply_offset`)
- Modify: `chimera-core/src/ui/page.rs` (Drive page, DemoWaves slots 0–1)
- Modify (mechanical): `chimera-core/tests/{chain_spectral_test,signal_chain_test,property_test,stress_test,live_param_test,desktop_sim_test,click_free_test,reverb_test}.rs` (whichever the command below touches)
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Task 2 (`Block`, `ParamSpec::continuous`, `apply_offset`, `bind`, `read_block`).
- Produces: `DriveParams { pub drive: f32, pub tone: f32, pub mix: f32 }`, `DriveParams::{DRIVE, TONE, MIX}: ParamId` (0, 1, 2), `pub static DRIVE_SPECS: [ParamSpec; 3]` (all modulatable), page const `DRIVE_PAGE: [ParamId; 3]`. `params.rs` now imports `crate::block::{Block, ParamId, ParamSpec, ValFmt}` (Tasks 5–10 reuse it).

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn drive_conforms() {
    conforms("drive", chimera_core::params::DriveParams::default());
}
```
Add to the `all_specs` vec in `block_spec_test.rs` (after the modal row):
```rust
        ("drive", &chimera_core::params::DRIVE_SPECS[..]),
```
Append to `page_block_test.rs`:
```rust
// ── Drive (Task 4) ───────────────────────────────────────────────────

#[test]
fn drive_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Drive.apply_encoder(0, 5, &mut p);
    assert_eq!(p.drive.drive, 5.0 * ((1.0 - 0.0) / 128.0));
    PageId::DemoWaves.apply_encoder(1, -2, &mut p);
    assert_eq!(p.drive.tone, 0.5 - 2.0 * (1.0 / 128.0));
}

#[test]
fn drive_snap_uses_spec_format() {
    let mut p = ParamSnapshot::default();
    // TONE is Bi: from the centre, the next point up is +43 (107/127).
    PageId::Drive.snap_encoder(1, 1, ValFmt::Bi, &mut p);
    assert_eq!(p.drive.tone, 107.0 / 127.0);
}

#[test]
fn drive_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Drive.read_values(&p), [0.0, 0.5, 1.0, 0.0, 0.0, 0.0]);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value DRIVE_SPECS`, `DriveParams: Block is not satisfied`, `mismatched types` (`expected f32, found Param` / `binary operation`).

- [ ] **Step 3: Convert `DriveParams`**

In `chimera-core/src/params.rs`, add as the first line:
```rust
use crate::block::{Block, ParamId, ParamSpec, ValFmt};

```
Replace the whole `/// Parameters for pre-filter drive stage` struct and its `Default` impl with:
```rust
/// Parameters for pre-filter drive stage
#[derive(Clone, Copy, Debug)]
pub struct DriveParams {
    pub drive: f32,
    pub tone: f32,
    pub mix: f32,
}

impl Default for DriveParams {
    fn default() -> Self {
        Self {
            drive: 0.0,
            tone: 0.5,
            mix: 1.0,
        }
    }
}

impl DriveParams {
    pub const DRIVE: ParamId = ParamId(0);
    pub const TONE: ParamId = ParamId(1);
    pub const MIX: ParamId = ParamId(2);
}

/// All three are read by `Voice` every block from the modulated copy.
pub static DRIVE_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "TONE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "MIX", ValFmt::Bi, 0.0, 1.0, 1.0, 1.0 / 128.0, true),
];

impl Block for DriveParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &DRIVE_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::DRIVE => self.drive,
            Self::TONE => self.tone,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::DRIVE => self.drive = v,
            Self::TONE => self.tone = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
```
In `chimera-core/src/dsp/drive.rs`, `params.drive.value` → `params.drive`, `params.tone.value` → `params.tone`, `params.mix.value` → `params.mix`.

In `chimera-core/src/dsp/voice.rs`, import `DriveParams` (`use crate::params::{DriveParams, EngineType, ParamSnapshot};`) and replace the two `mod_drive.….apply_mod_offset(…)` lines with:
```rust
        apply_offset(&mut mod_drive, DriveParams::DRIVE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 1, param: 0 }));
        apply_offset(&mut mod_drive, DriveParams::TONE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 1, param: 1 }));
```

- [ ] **Step 4: Route the Drive page and DemoWaves slots 0–1 through the spec**

In `chimera-core/src/ui/page.rs`:
1. `use crate::params::ParamSnapshot;` → `use crate::params::{DriveParams, ParamSnapshot};`
2. `read_values`: the `PageId::Drive => [ … ],` arm becomes `PageId::Drive => read_block(&params.drive, DRIVE_PAGE),`; in the `PageId::DemoWaves` array replace its first two entries with `params.drive.normalized(DriveParams::DRIVE),` and `params.drive.normalized(DriveParams::TONE),`.
3. `resolve_mut`: add
```rust
            PageId::Drive => bind(&mut p.drive, *DRIVE_PAGE.get(idx)?),
            PageId::DemoWaves => match idx {
                0 => bind(&mut p.drive, DriveParams::DRIVE),
                1 => bind(&mut p.drive, DriveParams::TONE),
                _ => None,
            },
```
4. `resolve_param_mut`: delete the whole `PageId::Drive => match idx { … },` arm and the `0 => …drive.drive…` and `1 => …drive.tone…` lines of the `PageId::DemoWaves` arm.
5. Add above `const MODAL1_PAGE`:
```rust
const DRIVE_PAGE: [ParamId; 3] = [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX];
```

- [ ] **Step 5: Update test sites (mechanical)**

Rules: `X.drive.<field>.set(v)` → `X.drive.<field> = v`; `X.drive.drive.value` → `X.drive.drive`; in functions whose `params` is a bare `DriveParams`, `params.<field>.set(v)` → `params.<field> = v` (those lines are listed by number; the numbers are valid at this point of the plan).
```bash
cd chimera-core/tests
sed -i 's/\.drive\.\(drive\|tone\|mix\)\.set(\(.*\))\([;,]\)/.drive.\1 = \2\3/; s/\.drive\.drive\.value\b/.drive.drive/g' *.rs
sed -i '26,27s/params\.\(drive\|mix\)\.set(\(.*\));/params.\1 = \2;/; 40,41s/params\.\(drive\|mix\)\.set(\(.*\));/params.\1 = \2;/' signal_chain_test.rs
sed -i '61,62s/params\.\(drive\|mix\)\.set(\(.*\));/params.\1 = \2;/; 103,104s/params\.\(drive\|mix\)\.set(\(.*\));/params.\1 = \2;/; 135,137s/params\.\(drive\|tone\|mix\)\.set(\(.*\));/params.\1 = \2;/' chain_spectral_test.rs
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `12 passed`, `1 passed`, `11 passed`, `4 passed`, all `ok`.
Run: `cargo test -p chimera-core 2>&1 | grep -E '^test result' | awk '{f+=$6} END {print f, "failed"}'`
Expected: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/dsp/drive.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/page.rs chimera-core/tests/
git commit -m "refactor(core): Drive owns its spec; plain f32 fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 5: Convert Filter

**Files:**
- Modify: `chimera-core/src/params.rs` (`FilterParams` + ids + `FILTER_SPECS` + `Block`; `ParamSnapshot::default` cutoff)
- Modify: `chimera-core/src/dsp/filter.rs:28-30` (read fields)
- Modify: `chimera-core/src/dsp/voice.rs:145-146` (cutoff/resonance offsets via `apply_offset`)
- Modify: `chimera-core/src/ui/page.rs` (Filter page; DemoWaves slot 4; DemoShapes slots 1, 3, 4, 5)
- Modify (mechanical): tests touched by the commands below (`chain_spectral_test`, `signal_chain_test`, `property_test`, `desktop_sim_test`, `live_param_test`, `modal_integration_test`, `modulation_integration_test`, `stress_test`, `preset_test`)
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Tasks 2 and 4 (`params.rs` block imports).
- Produces: `FilterParams { pub cutoff, resonance, drive, fm_amount, env_amount, key_track: f32, pub mode: u8 }`; `FilterParams::{CUTOFF, RESONANCE, DRIVE, FM_AMOUNT, ENV_AMOUNT, KEY_TRACK}: ParamId` (0..=5); `pub static FILTER_SPECS: [ParamSpec; 6]` (CUTOFF/RESONANCE/DRIVE modulatable); page const `FILTER_PAGE: [ParamId; 6]`.

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn filter_conforms() {
    conforms("filter", chimera_core::params::FilterParams::default());
}

/// Spec §1: defaults differ per instance — the snapshot's filter is fully open.
#[test]
fn snapshot_filter_starts_open() {
    assert_eq!(chimera_core::params::FilterParams::default().cutoff, 1000.0);
    assert_eq!(chimera_core::params::ParamSnapshot::default().filter.cutoff, 20000.0);
}
```
Add to `all_specs` in `block_spec_test.rs`: `("filter", &chimera_core::params::FILTER_SPECS[..]),`
Append to `page_block_test.rs`:
```rust
// ── Filter (Task 5) ──────────────────────────────────────────────────

#[test]
fn filter_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Filter.apply_encoder(0, -1, &mut p);
    assert_eq!(p.filter.cutoff, 20000.0 - (20000.0 - 20.0) / 128.0);
    PageId::Filter.apply_encoder(4, 1, &mut p);
    assert_eq!(p.filter.env_amount, (1.0 - -1.0) / 128.0);
    PageId::DemoShapes.apply_encoder(4, 3, &mut p);
    assert_eq!(p.filter.resonance, 3.0 / 128.0);
}

#[test]
fn filter_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Filter.read_values(&p), [1.0, 0.0, 0.0, 0.0, 0.5, 0.0]);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value FILTER_SPECS`, `FilterParams: Block is not satisfied`, `mismatched types`.

- [ ] **Step 3: Convert `FilterParams`**

In `chimera-core/src/params.rs`, replace the `FilterParams` struct and its `Default` impl with:
```rust
/// Parameters for one voice's filter
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    pub fm_amount: f32,
    pub env_amount: f32,
    pub key_track: f32,
    pub mode: u8,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            drive: 0.0,
            fm_amount: 0.0,
            env_amount: 0.0,
            key_track: 0.0,
            mode: 2, // LP4
        }
    }
}

impl FilterParams {
    pub const CUTOFF: ParamId = ParamId(0);
    pub const RESONANCE: ParamId = ParamId(1);
    pub const DRIVE: ParamId = ParamId(2);
    pub const FM_AMOUNT: ParamId = ParamId(3);
    pub const ENV_AMOUNT: ParamId = ParamId(4);
    pub const KEY_TRACK: ParamId = ParamId(5);
}

/// Cutoff, resonance and drive are read by `Voice` every block. FM amount,
/// env amount and key track are never read (spec § Current state).
/// `mode` has no spec (not on any page; plan D16).
pub static FILTER_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "CUTOFF", ValFmt::Uni, 20.0, 20000.0, 1000.0, (20000.0 - 20.0) / 128.0, true),
    ParamSpec::continuous(1, "RESO", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(3, "FM", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "ENV", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
    ParamSpec::continuous(5, "TRACK", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for FilterParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FILTER_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::CUTOFF => self.cutoff,
            Self::RESONANCE => self.resonance,
            Self::DRIVE => self.drive,
            Self::FM_AMOUNT => self.fm_amount,
            Self::ENV_AMOUNT => self.env_amount,
            Self::KEY_TRACK => self.key_track,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CUTOFF => self.cutoff = v,
            Self::RESONANCE => self.resonance = v,
            Self::DRIVE => self.drive = v,
            Self::FM_AMOUNT => self.fm_amount = v,
            Self::ENV_AMOUNT => self.env_amount = v,
            Self::KEY_TRACK => self.key_track = v,
            _ => {}
        }
    }
}
```
In `ParamSnapshot::default`, `f.cutoff = Param::new(20.0, 20000.0, 20000.0); // fully open` → `f.cutoff = 20000.0; // fully open`.

In `chimera-core/src/dsp/filter.rs`, `params.cutoff.value` / `params.resonance.value` / `params.drive.value` → `params.cutoff` / `params.resonance` / `params.drive`.

In `chimera-core/src/dsp/voice.rs`, import `FilterParams` and replace the two `mod_filter.….apply_mod_offset(…)` lines with:
```rust
        apply_offset(&mut mod_filter, FilterParams::CUTOFF, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 2, param: 0 }));
        apply_offset(&mut mod_filter, FilterParams::RESONANCE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 2, param: 1 }));
```

- [ ] **Step 4: Route the Filter page and Demo slots through the spec**

In `chimera-core/src/ui/page.rs`:
1. Import `FilterParams` (`use crate::params::{DriveParams, FilterParams, ParamSnapshot};`).
2. `read_values`: `PageId::Filter => read_block(&params.filter, FILTER_PAGE),`; in `DemoWaves` replace `params.filter.env_amount.normalized(),` with `params.filter.normalized(FilterParams::ENV_AMOUNT),`; in `DemoShapes` replace the four `params.filter.X.normalized()` entries with `params.filter.normalized(FilterParams::CUTOFF)`, `…::DRIVE`, `…::RESONANCE`, `…::FM_AMOUNT` (same positions: 1, 3, 4, 5).
3. `resolve_mut`: add `4 => bind(&mut p.filter, FilterParams::ENV_AMOUNT),` to the `DemoWaves` match, and add:
```rust
            PageId::Filter => bind(&mut p.filter, *FILTER_PAGE.get(idx)?),
            PageId::DemoShapes => match idx {
                1 => bind(&mut p.filter, FilterParams::CUTOFF),
                3 => bind(&mut p.filter, FilterParams::DRIVE),
                4 => bind(&mut p.filter, FilterParams::RESONANCE),
                5 => bind(&mut p.filter, FilterParams::FM_AMOUNT),
                _ => None,
            },
```
4. `resolve_param_mut`: delete the `PageId::Filter => match idx { … },` arm, the `4 => Some(&mut params.filter.env_amount),` line (DemoWaves), and the `1 =>`, `3 =>`, `4 =>`, `5 =>` filter lines of `DemoShapes` (it keeps `0 => volume`, `2 => pan`).
5. Add above `const MODAL1_PAGE`:
```rust
const FILTER_PAGE: [ParamId; 6] = [
    FilterParams::CUTOFF,
    FilterParams::RESONANCE,
    FilterParams::DRIVE,
    FilterParams::FM_AMOUNT,
    FilterParams::ENV_AMOUNT,
    FilterParams::KEY_TRACK,
];
```

- [ ] **Step 5: Update test sites (mechanical)**

Two multi-line calls in `property_test.rs` first (the sed below would otherwise half-rewrite them):
```bash
cd chimera-core/tests
python3 - <<'EOF'
p = open('property_test.rs').read()
p = p.replace("""            0 => params_b
                .filter
                .cutoff
                .set(params_a.filter.cutoff.value * 0.1 + 100.0),""",
"""            0 => params_b.filter.cutoff = params_a.filter.cutoff * 0.1 + 100.0,""")
p = p.replace("""        params
            .filter
            .resonance
            .set(params.filter.resonance.value * 0.5); // prevent self-oscillation""",
"""        params.filter.resonance *= 0.5; // prevent self-oscillation""")
open('property_test.rs', 'w').write(p)
EOF
sed -i 's/\.filter\.\(cutoff\|resonance\|drive\|fm_amount\|env_amount\|key_track\)\.set(\(.*\))\([;,]\)/.filter.\1 = \2\3/; s/\.filter\.\(cutoff\|resonance\|drive\)\.value()/.filter.\1/g; s/\.filter\.\(cutoff\|resonance\|drive\)\.value\b/.filter.\1/g' *.rs
sed -i '56s/params\.cutoff\.set(\(.*\));/params.cutoff = \1;/; 81s/params\.cutoff\.set(\(.*\));/params.cutoff = \1;/; 106,107s/params\.\(cutoff\|resonance\)\.set(\([^)]*\));/params.\1 = \2;/' signal_chain_test.rs
sed -i '159s/params\.cutoff\.set(\(.*\));/params.cutoff = \1;/; 197s/params\.cutoff\.set(\(.*\));/params.cutoff = \1;/; 234,235s/params\.\(cutoff\|resonance\)\.set(\(.*\));/params.\1 = \2;/; 269,270s/params\.\(cutoff\|resonance\)\.set(\(.*\));/params.\1 = \2;/; 305s/params\.cutoff\.set(\(.*\));/params.cutoff = \1;/' chain_spectral_test.rs
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `14 passed`, `1 passed`, `13 passed`, `4 passed`, all `ok`.
Run: `cargo test -p chimera-core 2>&1 | grep -E '^test result' | awk '{f+=$6} END {print f, "failed"}'` → `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/dsp/filter.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/page.rs chimera-core/tests/
git commit -m "refactor(core): Filter owns its spec; plain f32 fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 6: Convert Folder

**Files:**
- Modify: `chimera-core/src/params.rs` (`FolderParams` + ids + `FOLDER_SPECS` + `Block`)
- Modify: `chimera-core/src/dsp/wavefolder.rs:22-29` (read fields)
- Modify: `chimera-core/src/dsp/voice.rs:149-150` (fold/symmetry offsets via `apply_offset`)
- Modify: `chimera-core/src/ui/page.rs` (Folder page; DemoWaves slots 2–3)
- Modify (mechanical): tests touched by the commands below
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Tasks 2, 4.
- Produces: `FolderParams { pub fold: f32, pub symmetry: f32, pub mix: f32 }`; `FolderParams::{FOLD, SYMMETRY, MIX}: ParamId` (0, 1, 2); `pub static FOLDER_SPECS: [ParamSpec; 3]` (all modulatable); page const `FOLDER_PAGE: [ParamId; 3]`.

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn folder_conforms() {
    conforms("folder", chimera_core::params::FolderParams::default());
}
```
Add to `all_specs`: `("folder", &chimera_core::params::FOLDER_SPECS[..]),`
Append to `page_block_test.rs`:
```rust
// ── Folder (Task 6) ──────────────────────────────────────────────────

#[test]
fn folder_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Folder.apply_encoder(0, 4, &mut p);
    assert_eq!(p.folder.fold, 4.0 / 128.0);
    PageId::DemoWaves.apply_encoder(3, -1, &mut p);
    assert_eq!(p.folder.symmetry, 0.5 - 1.0 / 128.0);
    assert_eq!(PageId::Folder.read_values(&p)[..3], [4.0 / 128.0, 0.5 - 1.0 / 128.0, 0.5]);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value FOLDER_SPECS`, `FolderParams: Block is not satisfied`, `mismatched types`.

- [ ] **Step 3: Convert `FolderParams`**

In `chimera-core/src/params.rs`, replace the `FolderParams` struct and its `Default` impl with:
```rust
/// Parameters for post-filter wavefolder
#[derive(Clone, Copy, Debug)]
pub struct FolderParams {
    pub fold: f32,
    pub symmetry: f32,
    pub mix: f32,
}

impl Default for FolderParams {
    fn default() -> Self {
        Self {
            fold: 0.0,
            symmetry: 0.5,
            mix: 0.5,
        }
    }
}

impl FolderParams {
    pub const FOLD: ParamId = ParamId(0);
    pub const SYMMETRY: ParamId = ParamId(1);
    pub const MIX: ParamId = ParamId(2);
}

/// All three are read by `Voice` every block from the modulated copy.
pub static FOLDER_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "FOLD", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "SYM", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "MIX", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, true),
];

impl Block for FolderParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FOLDER_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::FOLD => self.fold,
            Self::SYMMETRY => self.symmetry,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::FOLD => self.fold = v,
            Self::SYMMETRY => self.symmetry = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
```
In `chimera-core/src/dsp/wavefolder.rs`, drop `.value` from `params.fold.value`, `params.symmetry.value`, `params.mix.value`.

In `chimera-core/src/dsp/voice.rs`, import `FolderParams` and replace the two `mod_folder.….apply_mod_offset(…)` lines with:
```rust
        apply_offset(&mut mod_folder, FolderParams::FOLD, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 3, param: 0 }));
        apply_offset(&mut mod_folder, FolderParams::SYMMETRY, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 3, param: 1 }));
```

- [ ] **Step 4: Route the Folder page and DemoWaves slots 2–3 through the spec**

In `chimera-core/src/ui/page.rs`:
1. Import `FolderParams`.
2. `read_values`: `PageId::Folder => read_block(&params.folder, FOLDER_PAGE),`; in `DemoWaves`, entries 2 and 3 become `params.folder.normalized(FolderParams::FOLD),` and `params.folder.normalized(FolderParams::SYMMETRY),`.
3. `resolve_mut`: in the `DemoWaves` match add `2 => bind(&mut p.folder, FolderParams::FOLD),` and `3 => bind(&mut p.folder, FolderParams::SYMMETRY),`; add the arm `PageId::Folder => bind(&mut p.folder, *FOLDER_PAGE.get(idx)?),`.
4. `resolve_param_mut`: delete the `PageId::Folder => match idx { … },` arm and DemoWaves' `2 =>`/`3 =>` folder lines (it keeps only `5 => pan`).
5. Add above `const MODAL1_PAGE`:
```rust
const FOLDER_PAGE: [ParamId; 3] = [FolderParams::FOLD, FolderParams::SYMMETRY, FolderParams::MIX];
```

- [ ] **Step 5: Update test sites (mechanical)**

```bash
cd chimera-core/tests
sed -i 's/\.folder\.\(fold\|symmetry\|mix\)\.set(\(.*\))\([;,]\)/.folder.\1 = \2\3/; s/\.folder\.\(fold\|symmetry\|mix\)\.value\b/.folder.\1/g' *.rs
sed -i '142,143s/params\.\(fold\|mix\)\.set(\(.*\));/params.\1 = \2;/; 160,161s/params\.\(fold\|mix\)\.set(\(.*\));/params.\1 = \2;/' signal_chain_test.rs
sed -i '354,355s/params\.\(fold\|mix\)\.set(\(.*\));/params.\1 = \2;/; 375,376s/params\.\(fold\|mix\)\.set(\(.*\));/params.\1 = \2;/' chain_spectral_test.rs
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `15 passed`, `1 passed`, `14 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/dsp/wavefolder.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/page.rs chimera-core/tests/
git commit -m "refactor(core): Folder owns its spec; plain f32 fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 7: Convert envelopes

**Files:**
- Modify: `chimera-core/src/params.rs` (`EnvParams` + ids + `ENV_SPECS` + `Block`)
- Modify: `chimera-core/src/dsp/envelope.rs:52-75` (read fields)
- Modify: `chimera-core/src/ui/page.rs` (Vca/EnvAmp/EnvFilter/EnvAux, DemoMotion; delete `read_env_values`, `resolve_env_param`)
- Modify: `chimera-core/src/ui/mod.rs:410` (display-side env source)
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Tasks 2, 4.
- Produces: `EnvParams { pub attack, decay, sustain, release, level, vel_sens: f32 }`; `EnvParams::{ATTACK, DECAY, SUSTAIN, RELEASE, LEVEL, VEL_SENS}: ParamId` (0..=5); `pub static ENV_SPECS: [ParamSpec; 6]` (A/D/S/R modulatable); page const `ENV_PAGE: [ParamId; 6]` (kept until Task 20 for the legacy envelope pages).

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn env_conforms() {
    conforms("env", chimera_core::params::EnvParams::default());
}
```
Add to `all_specs`: `("env", &chimera_core::params::ENV_SPECS[..]),`
Append to `page_block_test.rs`:
```rust
// ── Envelopes (Task 7) ───────────────────────────────────────────────

#[test]
fn env_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Vca.apply_encoder(0, 1, &mut p);
    assert_eq!(p.envelopes[0].attack, 0.01 + (10.0 - 0.001) / 128.0);
    PageId::Vca.apply_encoder(2, -1, &mut p);
    assert_eq!(p.envelopes[0].sustain, 0.7 - 1.0 / 128.0);
    PageId::DemoMotion.apply_encoder(4, 1, &mut p);
    assert_eq!(p.envelopes[1].attack, 0.01 + (10.0 - 0.001) / 128.0);
    PageId::EnvAux.apply_encoder(3, -128, &mut p);
    assert_eq!(p.envelopes[2].release, 0.001);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value ENV_SPECS`, `EnvParams: Block is not satisfied`, `mismatched types`.

- [ ] **Step 3: Convert `EnvParams`**

In `chimera-core/src/params.rs`, replace the `EnvParams` struct and its `Default` impl with:
```rust
/// Parameters for one envelope
#[derive(Clone, Copy, Debug)]
pub struct EnvParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub level: f32,
    pub vel_sens: f32,
}

impl Default for EnvParams {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.3,
            sustain: 0.7,
            release: 0.3,
            level: 1.0,
            vel_sens: 0.5,
        }
    }
}

impl EnvParams {
    pub const ATTACK: ParamId = ParamId(0);
    pub const DECAY: ParamId = ParamId(1);
    pub const SUSTAIN: ParamId = ParamId(2);
    pub const RELEASE: ParamId = ParamId(3);
    pub const LEVEL: ParamId = ParamId(4);
    pub const VEL_SENS: ParamId = ParamId(5);
}

/// Shared by all three envelopes. A/D/S/R are read by `Voice` every block
/// for the amp envelope (`envelopes[0]`); level and vel_sens are never read.
/// `envelopes[1..2]` are never read at all — `ParamAddr::modulatable`
/// excludes them (plan D7).
pub static ENV_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "ATK", ValFmt::Uni, 0.001, 10.0, 0.01, (10.0 - 0.001) / 128.0, true),
    ParamSpec::continuous(1, "DEC", ValFmt::Uni, 0.001, 10.0, 0.3, (10.0 - 0.001) / 128.0, true),
    ParamSpec::continuous(2, "SUS", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, true),
    ParamSpec::continuous(3, "REL", ValFmt::Uni, 0.001, 10.0, 0.3, (10.0 - 0.001) / 128.0, true),
    ParamSpec::continuous(4, "LEVEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "VEL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
];

impl Block for EnvParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ENV_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ATTACK => self.attack,
            Self::DECAY => self.decay,
            Self::SUSTAIN => self.sustain,
            Self::RELEASE => self.release,
            Self::LEVEL => self.level,
            Self::VEL_SENS => self.vel_sens,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::ATTACK => self.attack = v,
            Self::DECAY => self.decay = v,
            Self::SUSTAIN => self.sustain = v,
            Self::RELEASE => self.release = v,
            Self::LEVEL => self.level = v,
            Self::VEL_SENS => self.vel_sens = v,
            _ => {}
        }
    }
}
```
In `chimera-core/src/dsp/envelope.rs`, drop `.value` from `params.attack.value`, `params.decay.value`, `params.sustain.value`, `params.release.value`.

- [ ] **Step 4: Route the envelope pages and DemoMotion through the spec**

In `chimera-core/src/ui/page.rs`:
1. Import `EnvParams`.
2. `read_values`: the three envelope arms become
```rust
            PageId::EnvAmp | PageId::Vca => read_block(&params.envelopes[0], ENV_PAGE),
            PageId::EnvFilter => read_block(&params.envelopes[1], ENV_PAGE),
            PageId::EnvAux => read_block(&params.envelopes[2], ENV_PAGE),
```
and `DemoMotion` becomes
```rust
            PageId::DemoMotion => [
                params.envelopes[0].normalized(EnvParams::ATTACK),
                params.envelopes[0].normalized(EnvParams::DECAY),
                params.envelopes[0].normalized(EnvParams::SUSTAIN),
                params.envelopes[0].normalized(EnvParams::RELEASE),
                params.envelopes[1].normalized(EnvParams::ATTACK),
                params.envelopes[1].normalized(EnvParams::DECAY),
            ],
```
3. `resolve_mut`: add
```rust
            PageId::EnvAmp | PageId::Vca => bind(&mut p.envelopes[0], *ENV_PAGE.get(idx)?),
            PageId::EnvFilter => bind(&mut p.envelopes[1], *ENV_PAGE.get(idx)?),
            PageId::EnvAux => bind(&mut p.envelopes[2], *ENV_PAGE.get(idx)?),
            PageId::DemoMotion => match idx {
                0..=3 => bind(&mut p.envelopes[0], ENV_PAGE[idx]),
                4 => bind(&mut p.envelopes[1], EnvParams::ATTACK),
                5 => bind(&mut p.envelopes[1], EnvParams::DECAY),
                _ => None,
            },
```
4. `resolve_param_mut`: delete the three `resolve_env_param(…)` arms and the whole `PageId::DemoMotion => match idx { … },` arm. Delete `fn read_env_values` and `fn resolve_env_param`.
5. Add above `const MODAL1_PAGE`:
```rust
const ENV_PAGE: [ParamId; 6] = [
    EnvParams::ATTACK,
    EnvParams::DECAY,
    EnvParams::SUSTAIN,
    EnvParams::RELEASE,
    EnvParams::LEVEL,
    EnvParams::VEL_SENS,
];
```
In `chimera-core/src/ui/mod.rs`: `use crate::params::ParamSnapshot;` → `use crate::block::Block;` + `use crate::params::{EnvParams, ParamSnapshot};`, and `patch.params.envelopes[0].sustain.normalized()` → `patch.params.envelopes[0].normalized(EnvParams::SUSTAIN)`.

- [ ] **Step 5: Test sites**

No test writes envelope `Param`s. Run `cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4` — must print nothing.

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `16 passed`, `1 passed`, `15 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/dsp/envelope.rs chimera-core/src/ui/page.rs chimera-core/src/ui/mod.rs chimera-core/tests/
git commit -m "refactor(core): envelopes own their spec; plain f32 fields

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 8: LFO gets a spec

**Files:**
- Modify: `chimera-core/src/dsp/lfo.rs` (ids, `LFO_SPECS`, `Block` impl; fields unchanged)
- Modify: `chimera-core/src/ui/page.rs` (Lfo page via `resolve_mut`; delete `apply_lfo_encoder`)
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces: `LfoParams::{RATE, SHAPE, SYNC, PHASE, DEPTH, OFFSET}: ParamId` (0..=5); `pub static LFO_SPECS: [ParamSpec; 6]` (none modulatable); page const `LFO_PAGE: [ParamId; 6]`.

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn lfo_conforms() {
    conforms("lfo", chimera_core::dsp::lfo::LfoParams::default());
}
```
Add to `all_specs`: `("lfo", &chimera_core::dsp::lfo::LFO_SPECS[..]),`
Append to `page_block_test.rs`:
```rust
// ── LFO (Task 8) ─────────────────────────────────────────────────────

#[test]
fn lfo_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Lfo.apply_encoder(0, 2, &mut p);
    assert_eq!(p.lfo.rate, 1.0 + 2.0 * 0.15);
    PageId::Lfo.apply_encoder(1, 9, &mut p);
    assert_eq!(p.lfo.shape, 4);
    PageId::Lfo.apply_encoder(2, 1, &mut p);
    assert_eq!(p.lfo.sync, 1);
    PageId::Lfo.apply_encoder(5, 3, &mut p);
    assert_eq!(p.lfo.offset, 3.0 * (1.0 / 128.0) * 2.0);
}

/// Spec § Intended behavior changes: shift-snap now works on the LFO page.
#[test]
fn lfo_snap_now_works() {
    let mut p = ParamSnapshot::default();
    PageId::Lfo.snap_encoder(1, 1, ValFmt::Int(4), &mut p);
    assert_eq!(p.lfo.shape, 4);
}

/// Plan D19: RATE displays over its real range 0.01..20.
#[test]
fn lfo_rate_display_uses_range() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Lfo.read_values(&p)[0], (1.0 - 0.01) / (20.0 - 0.01));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[|FAILED' | sort | uniq -c`
Expected: `cannot find value LFO_SPECS`, `LfoParams: Block is not satisfied` (compile errors; once those are fixed `lfo_snap_now_works` and `lfo_rate_display_uses_range` would fail on the old page code).

- [ ] **Step 3: Give `LfoParams` its spec**

In `chimera-core/src/dsp/lfo.rs`, add `use crate::block::{Block, ParamId, ParamSpec, ValFmt};` above `use crate::dsp::fast_sin;`, and insert before `/// LFO state.`:
```rust
impl LfoParams {
    pub const RATE: ParamId = ParamId(0);
    pub const SHAPE: ParamId = ParamId(1);
    pub const SYNC: ParamId = ParamId(2);
    pub const PHASE: ParamId = ParamId(3);
    pub const DEPTH: ParamId = ParamId(4);
    pub const OFFSET: ParamId = ParamId(5);
}

/// The LFO source is computed from the unmodulated `params.lfo`; modulating
/// the LFO itself is out of scope, so nothing here is modulatable.
pub static LFO_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "RATE", ValFmt::Uni, 0.01, 20.0, 1.0, 0.15, false),
    ParamSpec::choice(1, "SHAPE", ValFmt::Int(4), 4.0, 0.0),
    ParamSpec::choice(2, "SYNC", ValFmt::Int(1), 1.0, 0.0),
    ParamSpec::continuous(3, "PHASE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "DEPTH", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "OFST", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
];

impl Block for LfoParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &LFO_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::RATE => self.rate,
            Self::SHAPE => self.shape as f32,
            Self::SYNC => self.sync as f32,
            Self::PHASE => self.phase_offset,
            Self::DEPTH => self.depth,
            Self::OFFSET => self.offset,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::RATE => self.rate = v,
            Self::SHAPE => self.shape = v as u8,
            Self::SYNC => self.sync = v as u8,
            Self::PHASE => self.phase_offset = v,
            Self::DEPTH => self.depth = v,
            Self::OFFSET => self.offset = v,
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Route the LFO page through the spec**

In `chimera-core/src/ui/page.rs`:
1. Add `use crate::dsp::lfo::LfoParams;`.
2. `read_values`: `PageId::Lfo => read_block(&params.lfo, LFO_PAGE),` (plan D19: RATE bar is now `(rate−0.01)/19.99`).
3. `apply_encoder`: delete the `PageId::Lfo => { apply_lfo_encoder(…); return; }` arm. `resolve_param_mut`: delete `PageId::Lfo => None, // handled by apply_encoder special case`.
4. `resolve_mut`: add `PageId::Lfo => bind(&mut p.lfo, *LFO_PAGE.get(idx)?),`.
5. Delete `fn apply_lfo_encoder`. Add above `const MODAL1_PAGE`:
```rust
const LFO_PAGE: [ParamId; 6] = [
    LfoParams::RATE,
    LfoParams::SHAPE,
    LfoParams::SYNC,
    LfoParams::PHASE,
    LfoParams::DEPTH,
    LfoParams::OFFSET,
];
```

- [ ] **Step 5: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `17 passed`, `1 passed`, `18 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/dsp/lfo.rs chimera-core/src/ui/page.rs chimera-core/tests/
git commit -m "refactor(core): LFO owns its spec; LFO page snaps

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 9: Convert FM (`FmOpParams`, `FmParams`)

**Files:**
- Modify: `chimera-core/src/params.rs` (`FmOpParams`, `FmParams` + ids + `FM_OP_SPECS`, `FM_SPECS` + `Block`)
- Modify: `chimera-core/src/dsp/engine_fm.rs:34-50, 353, 361` (`FmOpSettings::from_params`, `algorithm`)
- Modify: `chimera-core/src/dsp/voice.rs:164` (op level offset via `apply_offset`)
- Modify: `chimera-core/src/ui/page.rs` (FmAlg/FmOp/FmRatio/FmEnv1–4/DemoFm; delete the five FM helper fns)
- Modify (mechanical): `chimera-core/tests/fm_test.rs:340-341`
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Tasks 2, 4.
- Produces:
  - `FmOpParams { pub waveform: u8, coarse: u8, fine: u8, level: f32, feedback: f32, detune: i8, velocity_sens: u8, attack_rate: u8, decay1_rate: u8, decay1_level: u8, decay2_rate: u8, release_rate: u8, rate_scaling: u8 }` (plan D18)
  - `FmOpParams::{WAVEFORM, COARSE, FINE, LEVEL, FEEDBACK, DETUNE, VELOCITY_SENS, ATTACK_RATE, DECAY1_RATE, DECAY1_LEVEL, DECAY2_RATE, RELEASE_RATE, RATE_SCALING}: ParamId` (0..=12); `pub static FM_OP_SPECS: [ParamSpec; 13]` (LEVEL, FEEDBACK modulatable; RR range 0..=15, plan D4)
  - `FmParams { pub algorithm: u8, pub operators: [FmOpParams; 4] }`; `FmParams::ALGORITHM: ParamId` (0); `pub static FM_SPECS: [ParamSpec; 1]`
  - page consts `FM_OP_PAGE: [ParamId; 5]` (FM_OP slots 1..=5), `FM_ENV_PAGE: [ParamId; 6]`

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn fm_conforms() {
    conforms("fm", chimera_core::params::FmParams::default());
    conforms("fm_op", chimera_core::params::FmOpParams::default());
}

/// Plan D18: a modulated (fractional) level truncates exactly like the old
/// `Param.value as u8` — never rounds.
#[test]
fn fm_settings_truncate_fractional_level() {
    use chimera_core::dsp::engine_fm::FmOpSettings;
    use chimera_core::params::FmOpParams;
    let op = FmOpParams { level: 50.7, feedback: 6.9, ..FmOpParams::default() };
    let s = FmOpSettings::from_params(&op);
    assert_eq!((s.level, s.feedback), (50, 6));
}
```
Add to `all_specs`:
```rust
        ("fm", &chimera_core::params::FM_SPECS[..]),
        ("fm_op", &chimera_core::params::FM_OP_SPECS[..]),
```
Append to `page_block_test.rs`:
```rust
// ── FM (Task 9) ──────────────────────────────────────────────────────

/// The selected operator is a process-wide static until Task 20, so every
/// assertion that depends on it lives in this one test.
#[test]
fn fm_pages_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::FmOp.apply_encoder(0, 1, &mut p); // select op B
    PageId::FmOp.apply_encoder(2, 5, &mut p);
    assert_eq!(p.fm.operators[1].level, 5.0);
    PageId::FmOp.apply_encoder(4, -9, &mut p);
    assert_eq!(p.fm.operators[1].detune, -7);
    PageId::FmRatio.apply_encoder(4, 1, &mut p); // FINE of the selected op
    assert_eq!(p.fm.operators[1].fine, 1);
    assert_eq!(PageId::FmOp.read_values(&p)[0], 1.0 / 3.0);
    // Review Focus 3: snapping a Stepped level lands on an integer.
    PageId::FmOp.snap_encoder(2, 1, ValFmt::Uni, &mut p);
    assert_eq!(p.fm.operators[1].level, 78.0); // 99 * 100/127 = 77.95 → 78
    PageId::FmOp.apply_encoder(0, -1, &mut p); // back to op A
}

#[test]
fn fm_fixed_op_pages_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::FmAlg.apply_encoder(0, 9, &mut p);
    assert_eq!(p.fm.algorithm, 7);
    PageId::FmRatio.apply_encoder(2, 1, &mut p);
    assert_eq!(p.fm.operators[2].coarse, 5);
    PageId::FmEnv3.apply_encoder(2, -1, &mut p);
    assert_eq!(p.fm.operators[2].decay1_level, 14);
    PageId::DemoFm.apply_encoder(3, 2, &mut p);
    assert_eq!(p.fm.operators[2].feedback, 2.0);
}

/// Review Focus 3: snap on Stepped params lands on integers in range.
#[test]
fn fm_snap_lands_on_integers() {
    let mut p = ParamSnapshot::default();
    PageId::FmRatio.snap_encoder(0, 1, ValFmt::Int(63), &mut p);
    assert_eq!(p.fm.operators[0].coarse, 63);
    PageId::FmEnv1.snap_encoder(0, -1, ValFmt::Int(31), &mut p);
    assert_eq!(p.fm.operators[0].attack_rate, 0);
}

/// Plan D4: RR spans 0..=15 (today's encoder stopped at 1).
#[test]
fn fm_rr_reaches_zero() {
    let mut p = ParamSnapshot::default();
    PageId::FmEnv2.apply_encoder(4, -20, &mut p);
    assert_eq!(p.fm.operators[1].release_rate, 0);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value FM_SPECS` / `FM_OP_SPECS`, `FmParams: Block` / `FmOpParams: Block is not satisfied`, `mismatched types`.

- [ ] **Step 3: Convert the FM values structs**

In `chimera-core/src/params.rs`, replace everything from `/// Parameters for one FM operator.` up to (not including) `/// Which synthesis engine is active.` with:
```rust
/// Parameters for one FM operator. Stepped params that are modulatable
/// (`level`, `feedback`) are `f32` so a modulated copy can hold fractional
/// values (the DSP truncates `as u8`); the others keep integer types (plan D18).
#[derive(Clone, Copy, Debug)]
pub struct FmOpParams {
    pub waveform: u8,      // 0–7
    pub coarse: u8,        // 0–63
    pub fine: u8,          // 0–15
    pub level: f32,        // 0–99 (integer steps)
    pub feedback: f32,     // 0–7 (integer steps)
    pub detune: i8,        // -7–7
    pub velocity_sens: u8, // 0–7
    pub attack_rate: u8,   // 0–31
    pub decay1_rate: u8,   // 0–31
    pub decay1_level: u8,  // 0–15
    pub decay2_rate: u8,   // 0–31
    pub release_rate: u8,  // 0–15 (plan D4)
    pub rate_scaling: u8,  // 0–3
}

impl Default for FmOpParams {
    fn default() -> Self {
        Self {
            waveform: 0,
            coarse: 4,
            fine: 0,
            level: 0.0,
            feedback: 0.0,
            detune: 0,
            velocity_sens: 0,
            attack_rate: 31,
            decay1_rate: 0,
            decay1_level: 15,
            decay2_rate: 0,
            release_rate: 15,
            rate_scaling: 0,
        }
    }
}

impl FmOpParams {
    pub const WAVEFORM: ParamId = ParamId(0);
    pub const COARSE: ParamId = ParamId(1);
    pub const FINE: ParamId = ParamId(2);
    pub const LEVEL: ParamId = ParamId(3);
    pub const FEEDBACK: ParamId = ParamId(4);
    pub const DETUNE: ParamId = ParamId(5);
    pub const VELOCITY_SENS: ParamId = ParamId(6);
    pub const ATTACK_RATE: ParamId = ParamId(7);
    pub const DECAY1_RATE: ParamId = ParamId(8);
    pub const DECAY1_LEVEL: ParamId = ParamId(9);
    pub const DECAY2_RATE: ParamId = ParamId(10);
    pub const RELEASE_RATE: ParamId = ParamId(11);
    pub const RATE_SCALING: ParamId = ParamId(12);
}

/// Level and feedback are read every block (`FmOperator::update_live`);
/// waveform is too, but it is a choice. Ratios, detune and the envelope are
/// read only at note-on, so they are not modulatable.
pub static FM_OP_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(0, "WAVE", ValFmt::Int(7), 7.0, 0.0),
    ParamSpec::stepped(1, "CRSE", ValFmt::Int(63), 0.0, 63.0, 4.0, false),
    ParamSpec::stepped(2, "FINE", ValFmt::Int(15), 0.0, 15.0, 0.0, false),
    ParamSpec::stepped(3, "LEVEL", ValFmt::Uni, 0.0, 99.0, 0.0, true),
    ParamSpec::stepped(4, "FDBK", ValFmt::Int(7), 0.0, 7.0, 0.0, true),
    ParamSpec::stepped(5, "DETUN", ValFmt::Bi, -7.0, 7.0, 0.0, false),
    ParamSpec::stepped(6, "V.SNS", ValFmt::Int(7), 0.0, 7.0, 0.0, false),
    ParamSpec::stepped(7, "AR", ValFmt::Int(31), 0.0, 31.0, 31.0, false),
    ParamSpec::stepped(8, "D1R", ValFmt::Int(31), 0.0, 31.0, 0.0, false),
    ParamSpec::stepped(9, "D1L", ValFmt::Int(15), 0.0, 15.0, 15.0, false),
    ParamSpec::stepped(10, "D2R", ValFmt::Int(31), 0.0, 31.0, 0.0, false),
    ParamSpec::stepped(11, "RR", ValFmt::Int(15), 0.0, 15.0, 15.0, false),
    ParamSpec::stepped(12, "RS", ValFmt::Int(3), 0.0, 3.0, 0.0, false),
];

impl Block for FmOpParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FM_OP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVEFORM => self.waveform as f32,
            Self::COARSE => self.coarse as f32,
            Self::FINE => self.fine as f32,
            Self::LEVEL => self.level,
            Self::FEEDBACK => self.feedback,
            Self::DETUNE => self.detune as f32,
            Self::VELOCITY_SENS => self.velocity_sens as f32,
            Self::ATTACK_RATE => self.attack_rate as f32,
            Self::DECAY1_RATE => self.decay1_rate as f32,
            Self::DECAY1_LEVEL => self.decay1_level as f32,
            Self::DECAY2_RATE => self.decay2_rate as f32,
            Self::RELEASE_RATE => self.release_rate as f32,
            Self::RATE_SCALING => self.rate_scaling as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::WAVEFORM => self.waveform = v as u8,
            Self::COARSE => self.coarse = v as u8,
            Self::FINE => self.fine = v as u8,
            Self::LEVEL => self.level = v,
            Self::FEEDBACK => self.feedback = v,
            Self::DETUNE => self.detune = v as i8,
            Self::VELOCITY_SENS => self.velocity_sens = v as u8,
            Self::ATTACK_RATE => self.attack_rate = v as u8,
            Self::DECAY1_RATE => self.decay1_rate = v as u8,
            Self::DECAY1_LEVEL => self.decay1_level = v as u8,
            Self::DECAY2_RATE => self.decay2_rate = v as u8,
            Self::RELEASE_RATE => self.release_rate = v as u8,
            Self::RATE_SCALING => self.rate_scaling = v as u8,
            _ => {}
        }
    }
}

/// Parameters for the 4-operator FM engine.
#[derive(Clone, Copy, Debug)]
pub struct FmParams {
    pub algorithm: u8, // 0–7
    pub operators: [FmOpParams; 4],
}

impl Default for FmParams {
    fn default() -> Self {
        let mut op0 = FmOpParams::default();
        op0.level = 99.0;
        Self {
            algorithm: 0,
            operators: [
                op0,
                FmOpParams::default(),
                FmOpParams::default(),
                FmOpParams::default(),
            ],
        }
    }
}

impl FmParams {
    pub const ALGORITHM: ParamId = ParamId(0);
}

/// Engine-level FM params. Operators are separate blocks (`FmOpParams`).
pub static FM_SPECS: [ParamSpec; 1] = [ParamSpec::choice(0, "ALG", ValFmt::Int(7), 7.0, 0.0)];

impl Block for FmParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FM_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ALGORITHM => self.algorithm as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        if id == Self::ALGORITHM {
            self.algorithm = v as u8;
        }
    }
}
```
In `chimera-core/src/dsp/engine_fm.rs`, replace the body of `FmOpSettings::from_params` with:
```rust
        Self {
            waveform: p.waveform,
            coarse: p.coarse,
            fine: p.fine,
            level: p.level as u8,
            feedback: p.feedback as u8,
            detune: p.detune,
            velocity_sens: p.velocity_sens,
            ar: p.attack_rate,
            d1r: p.decay1_rate,
            d1l: p.decay1_level,
            d2r: p.decay2_rate,
            rr: p.release_rate,
            rate_scaling: p.rate_scaling,
        }
```
and both `let alg = params.algorithm.value as u8;` → `let alg = params.algorithm;`.

In `chimera-core/src/dsp/voice.rs`, import `FmOpParams` and replace `mod_fm.operators[op as usize].level.apply_mod_offset(offset);` with `apply_offset(&mut mod_fm.operators[op as usize], FmOpParams::LEVEL, offset);`.

- [ ] **Step 4: Route the FM pages through the spec**

In `chimera-core/src/ui/page.rs`:
1. Import `FmOpParams, FmParams` from `crate::params`.
2. `read_values`: replace the `DemoFm`, `FmAlg`, `FmOp`, `FmRatio`, `FmEnv1`..`FmEnv4` arms with:
```rust
            PageId::DemoFm => [
                params.fm.normalized(FmParams::ALGORITHM),
                params.fm.operators[0].normalized(FmOpParams::FEEDBACK),
                params.fm.operators[1].normalized(FmOpParams::FEEDBACK),
                params.fm.operators[2].normalized(FmOpParams::FEEDBACK),
                0.0,
                0.0,
            ],
            PageId::FmAlg => [
                params.fm.normalized(FmParams::ALGORITHM),
                0.0,
                params.volume.normalized(),
                0.0,
                0.0,
                0.0,
            ],
            PageId::FmOp => {
                let sel = fm_selected_op();
                let mut v = read_block(&params.fm.operators[sel], FM_OP_PAGE);
                v.rotate_right(1); // slot 0 is the operator selector
                v[0] = sel as f32 / 3.0;
                v
            }
            PageId::FmRatio => [
                params.fm.operators[0].normalized(FmOpParams::COARSE),
                params.fm.operators[1].normalized(FmOpParams::COARSE),
                params.fm.operators[2].normalized(FmOpParams::COARSE),
                params.fm.operators[3].normalized(FmOpParams::COARSE),
                params.fm.operators[fm_selected_op()].normalized(FmOpParams::FINE),
                0.0,
            ],
            PageId::FmEnv1 => read_block(&params.fm.operators[0], FM_ENV_PAGE),
            PageId::FmEnv2 => read_block(&params.fm.operators[1], FM_ENV_PAGE),
            PageId::FmEnv3 => read_block(&params.fm.operators[2], FM_ENV_PAGE),
            PageId::FmEnv4 => read_block(&params.fm.operators[3], FM_ENV_PAGE),
```
3. `apply_encoder`: delete the `FmAlg`, `FmOp`, `FmRatio`, `FmEnv1`..`FmEnv4` special arms, and insert as the very first statement (before the `resolve_mut` check):
```rust
        if *self == PageId::FmOp && idx == 0 {
            // Operator select: 0-3
            let cur = fm_selected_op() as i16;
            fm_set_selected_op((cur + delta as i16).clamp(0, 3) as u8);
            return;
        }
```
(`i16` avoids the old `i8` overflow on large deltas.)
4. `resolve_mut`: add
```rust
            PageId::FmAlg => match idx {
                0 => bind(&mut p.fm, FmParams::ALGORITHM),
                _ => None,
            },
            // Slot 0 selects the operator (handled in `apply_encoder`).
            PageId::FmOp => bind(&mut p.fm.operators[fm_selected_op()], *FM_OP_PAGE.get(idx.checked_sub(1)?)?),
            PageId::FmRatio => match idx {
                0..=3 => bind(&mut p.fm.operators[idx], FmOpParams::COARSE),
                4 => bind(&mut p.fm.operators[fm_selected_op()], FmOpParams::FINE),
                _ => None,
            },
            PageId::FmEnv1 => bind(&mut p.fm.operators[0], *FM_ENV_PAGE.get(idx)?),
            PageId::FmEnv2 => bind(&mut p.fm.operators[1], *FM_ENV_PAGE.get(idx)?),
            PageId::FmEnv3 => bind(&mut p.fm.operators[2], *FM_ENV_PAGE.get(idx)?),
            PageId::FmEnv4 => bind(&mut p.fm.operators[3], *FM_ENV_PAGE.get(idx)?),
            PageId::DemoFm => match idx {
                0 => bind(&mut p.fm, FmParams::ALGORITHM),
                1..=3 => bind(&mut p.fm.operators[idx - 1], FmOpParams::FEEDBACK),
                _ => None,
            },
```
5. `resolve_param_mut`: replace the `PageId::DemoFm => match idx { … },` arm with the FM ALG level slot (volume stays a `Param` until Task 10):
```rust
            PageId::FmAlg => match idx {
                2 => Some(&mut params.volume),
                _ => None,
            },
```
6. Delete `fn apply_fm_alg_encoder`, `fn apply_fm_op_encoder`, `fn read_fm_env_values`, `fn apply_fm_env_encoder`, `fn apply_fm_ratio_encoder`. Add above `const MODAL1_PAGE`:
```rust
/// FM_OP slots 1..=5 (slot 0 selects the operator).
const FM_OP_PAGE: [ParamId; 5] = [
    FmOpParams::WAVEFORM,
    FmOpParams::LEVEL,
    FmOpParams::FEEDBACK,
    FmOpParams::DETUNE,
    FmOpParams::VELOCITY_SENS,
];
const FM_ENV_PAGE: [ParamId; 6] = [
    FmOpParams::ATTACK_RATE,
    FmOpParams::DECAY1_RATE,
    FmOpParams::DECAY1_LEVEL,
    FmOpParams::DECAY2_RATE,
    FmOpParams::RELEASE_RATE,
    FmOpParams::RATE_SCALING,
];
```

- [ ] **Step 5: Update test sites (mechanical)**

```bash
sed -i 's/op\.\(level\|feedback\)\.value = /op.\1 = /' chimera-core/tests/fm_test.rs
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `19 passed`, `1 passed`, `22 passed`, `4 passed`, all `ok` (golden `fm_lfo_op_a_level` proves the level offset is unchanged). Full suite: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/dsp/engine_fm.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/page.rs chimera-core/tests/
git commit -m "refactor(core): FM engine and operators own their specs

Level/feedback stay f32 (modulated copies are fractional, DSP truncates);
other operator params use integer fields. FM pages snap; RR reaches 0.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 10: `OutParams { volume, pan }`

**Files:**
- Modify: `chimera-core/src/params.rs` (`OutParams` + ids + `OUT_SPECS` + `Block`; `ParamSnapshot.volume/pan` → `out`)
- Modify: `chimera-core/src/dsp/voice.rs:192` (`params.out.volume`)
- Modify: `chimera-core/src/ui/page.rs` (Mixer, Master, FmAlg slot 2, DemoWaves slot 5, DemoShapes slots 0 and 2)
- Modify (mechanical): `chimera-core/tests/{live_param_test,preset_test,property_test}.rs`
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Tasks 2, 4.
- Produces: `pub struct OutParams { pub volume: f32, pub pan: f32 }` (Default 0.8 / 0.0); `OutParams::{VOLUME, PAN}: ParamId` (0, 1); `pub static OUT_SPECS: [ParamSpec; 2]` (VOLUME modulatable, label "LEVEL"); `ParamSnapshot.out: OutParams` (replaces `volume`, `pan`); page const `OUT_PAGE: [ParamId; 2]`. After this task `resolve_param_mut` has no live arms.

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn out_conforms() {
    conforms("out", chimera_core::params::OutParams::default());
}
```
Add to `all_specs`: `("out", &chimera_core::params::OUT_SPECS[..]),`
Append to `page_block_test.rs`:
```rust
// ── Out (Task 10) ────────────────────────────────────────────────────

#[test]
fn out_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Mixer.apply_encoder(0, -8, &mut p);
    assert_eq!(p.out.volume, 0.8 - 8.0 / 128.0);
    PageId::Master.apply_encoder(1, 1, &mut p);
    assert_eq!(p.out.pan, 2.0 / 128.0);
    PageId::FmAlg.apply_encoder(2, 1, &mut p);
    assert_eq!(p.out.volume, 0.8 - 8.0 / 128.0 + 1.0 / 128.0);
}

/// The mixer page keeps its placeholder bars for unbound slots.
#[test]
fn mixer_read_values_keep_placeholders() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Mixer.read_values(&p), [0.8, 0.5, 0.5, 0.0, 0.5, 0.0]);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find type OutParams`, `cannot find value OUT_SPECS`, `no field out on type ParamSnapshot`.

- [ ] **Step 3: Add `OutParams` and move the snapshot fields**

In `chimera-core/src/params.rs`, insert directly above `#[derive(Clone, Debug)] pub struct ParamSnapshot`:
```rust
/// Voice output stage: level into the mixer and pan.
#[derive(Clone, Copy, Debug)]
pub struct OutParams {
    pub volume: f32,
    pub pan: f32,
}

impl Default for OutParams {
    fn default() -> Self {
        Self { volume: 0.8, pan: 0.0 }
    }
}

impl OutParams {
    pub const VOLUME: ParamId = ParamId(0);
    pub const PAN: ParamId = ParamId(1);
}

/// Volume is read by `Voice`'s VCA every block (newly modulatable); pan is
/// not used by `Voice`.
pub static OUT_SPECS: [ParamSpec; 2] = [
    ParamSpec::continuous(0, "LEVEL", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, true),
    ParamSpec::continuous(1, "PAN", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
];

impl Block for OutParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &OUT_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::VOLUME => self.volume,
            Self::PAN => self.pan,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::VOLUME => self.volume = v,
            Self::PAN => self.pan = v,
            _ => {}
        }
    }
}
```
In `ParamSnapshot`, replace the fields `pub volume: Param,` and `pub pan: Param,` with `pub out: OutParams,`, and in its `Default` replace `volume: Param::new(0.0, 1.0, 0.8),` / `pan: Param::new(-1.0, 1.0, 0.0),` with `out: OutParams::default(),`.

In `chimera-core/src/dsp/voice.rs`: `let volume = params.volume.value;` → `let volume = params.out.volume;`.

- [ ] **Step 4: Route every volume/pan slot through `OutParams`**

In `chimera-core/src/ui/page.rs`:
1. Import `OutParams`.
2. `read_values`: `Mixer` becomes `[params.out.normalized(OutParams::VOLUME), params.out.normalized(OutParams::PAN), 0.5, 0.0, 0.5, 0.0 /* placeholders */]`; `Master => read_block(&params.out, OUT_PAGE),`; every remaining `params.volume.normalized()` → `params.out.normalized(OutParams::VOLUME)` and `params.pan.normalized()` → `params.out.normalized(OutParams::PAN)` (in `DemoWaves`, `DemoShapes`, `FmAlg`).
3. `resolve_mut`: add `5 => bind(&mut p.out, OutParams::PAN),` to `DemoWaves`; `0 => bind(&mut p.out, OutParams::VOLUME),` and `2 => bind(&mut p.out, OutParams::PAN),` to `DemoShapes`; `2 => bind(&mut p.out, OutParams::VOLUME),` to `FmAlg`; and the arm `PageId::Mixer | PageId::Master => bind(&mut p.out, *OUT_PAGE.get(idx)?),`.
4. `resolve_param_mut`: delete the `Mixer`, `DemoWaves`, `DemoShapes` and `FmAlg` arms (only `DemoMatrix => None, _ => None` remain).
5. Add above `const MODAL1_PAGE`: `const OUT_PAGE: [ParamId; 2] = [OutParams::VOLUME, OutParams::PAN];`

- [ ] **Step 5: Update test sites (mechanical)**

```bash
cd chimera-core/tests
sed -i 's/\.volume\.set(\(.*\))\([;,]\)/.out.volume = \1\2/; s/\.volume\.value()/.out.volume/g; s/\.volume\.value\b/.out.volume/g' *.rs
sed -i 's/params\.volume = Param::new(0\.0, 1\.0, 0\.0);/params.out.volume = 0.0;/; /^use chimera_core::params::Param;$/d' preset_test.rs
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `20 passed`, `1 passed`, `24 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/page.rs chimera-core/tests/
git commit -m "refactor(core): OutParams owns voice volume and pan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 11: FX get specs; delete `Param`

**Files:**
- Modify: `chimera-core/src/dsp/chorus.rs`, `dsp/delay.rs`, `dsp/reverb.rs` (ids, `*_SPECS`, `Block` impls; fields unchanged)
- Modify: `chimera-core/src/ui/page.rs` (Chorus/Delay/Efx/MixReverb via `resolve_mut`; `apply_encoder`/`snap_encoder` final form; delete `resolve_param_mut`, `nudge_float`, `nudge_u8`, `apply_chorus_encoder`, `apply_delay_encoder`, `apply_reverb_encoder`)
- Modify: `chimera-core/src/params.rs` (delete `Param`)
- Modify: `chimera-core/src/ui/mod.rs:347-349` (`snap_encoder` call)
- Delete: `chimera-core/tests/param_test.rs` (its snap walk is ported to `block_test.rs`)
- Modify (mechanical): `chimera-core/tests/page_block_test.rs` (`snap_encoder` loses its `ValFmt` argument)
- Test: `block_test.rs`, `block_spec_test.rs`, `page_block_test.rs`

**Interfaces:**
- Consumes: Tasks 2–10.
- Produces: `ChorusParams::{MODE, RATE, DEPTH, MIX}` + `pub static CHORUS_SPECS: [ParamSpec; 4]`; `DelayParams::{TIME_MS, FEEDBACK, WOW_FLUTTER, SATURATION, TONE, MIX}` + `pub static DELAY_SPECS: [ParamSpec; 6]`; `ReverbParams::{REVERB_TYPE, TIME, DAMPING, SIZE, MIX}` + `pub static REVERB_SPECS: [ParamSpec; 5]` (none modulatable). **Signature change:** `PageId::snap_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot)` (format now comes from the spec). `crate::params::Param` no longer exists. `ParamSnapshot` ≤ 512 B (376 B measured).

- [ ] **Step 1: Write the failing tests**

Append to `block_test.rs`:
```rust
#[test]
fn fx_conform() {
    conforms("chorus", chimera_core::dsp::chorus::ChorusParams::default());
    conforms("delay", chimera_core::dsp::delay::DelayParams::default());
    conforms("reverb", chimera_core::dsp::reverb::ReverbParams::default());
}

/// Ported from the deleted `param_test.rs`: the bipolar snap walk
/// −64 → −44 → 0 → +43 → +63 and back.
#[test]
fn snap_walks_bipolar_points_both_ways() {
    let mut p = Probe { c: -1.0, s: 0.0, e: 0 };
    let up = [20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0];
    for want in up {
        p.snap(C, 1);
        assert!((p.normalized(C) - want).abs() < 0.01, "up: {} vs {want}", p.normalized(C));
    }
    for want in [107.0 / 127.0, 64.0 / 127.0, 20.0 / 127.0, 0.0] {
        p.snap(C, -1);
        assert!((p.normalized(C) - want).abs() < 0.01, "down: {} vs {want}", p.normalized(C));
    }
}

/// Spec §1: `ParamSnapshot` shrinks from 1,524 B to roughly 0.4 KB.
#[test]
fn snapshot_is_small() {
    let size = core::mem::size_of::<chimera_core::params::ParamSnapshot>();
    eprintln!("ParamSnapshot = {size} B");
    assert!(size <= 512, "ParamSnapshot is {size} B");
}
```
Add to `all_specs`:
```rust
        ("chorus", &chimera_core::dsp::chorus::CHORUS_SPECS[..]),
        ("delay", &chimera_core::dsp::delay::DELAY_SPECS[..]),
        ("reverb", &chimera_core::dsp::reverb::REVERB_SPECS[..]),
```
Append to `page_block_test.rs`:
```rust
// ── FX (Task 11) ─────────────────────────────────────────────────────

#[test]
fn fx_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Delay.apply_encoder(0, 2, &mut p);
    assert_eq!(p.delay.time_ms, 375.0 + 2.0 * 8.0);
    PageId::Chorus.apply_encoder(0, 5, &mut p);
    assert_eq!(p.chorus.mode, 3);
    PageId::MixReverb.apply_encoder(0, 5, &mut p);
    assert_eq!(p.reverb.reverb_type, 2);
    PageId::Efx.apply_encoder(4, 1, &mut p);
    assert_eq!(p.reverb.mix, 1.0 / 128.0);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test 2>&1 | grep -E '^error\[' | sort | uniq -c`
Expected: `cannot find value CHORUS_SPECS` (and DELAY/REVERB), `ChorusParams: Block is not satisfied` (and Delay/Reverb).

- [ ] **Step 3: Give the FX blocks their specs**

In each of `dsp/chorus.rs`, `dsp/delay.rs`, `dsp/reverb.rs`, add `use crate::block::{Block, ParamId, ParamSpec, ValFmt};` below `use chimera_hal::BLOCK_SIZE;`, and insert after the params struct's `impl Default … { … }` block:

`dsp/chorus.rs`:
```rust
impl ChorusParams {
    pub const MODE: ParamId = ParamId(0);
    pub const RATE: ParamId = ParamId(1);
    pub const DEPTH: ParamId = ParamId(2);
    pub const MIX: ParamId = ParamId(3);
}

/// Chorus runs outside `Voice` (desktop only): nothing is modulatable.
pub static CHORUS_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(0, "MODE", ValFmt::Int(3), 3.0, 0.0),
    ParamSpec::continuous(1, "RATE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DEPTH", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for ChorusParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &CHORUS_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as f32,
            Self::RATE => self.rate,
            Self::DEPTH => self.depth,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODE => self.mode = v as u8,
            Self::RATE => self.rate = v,
            Self::DEPTH => self.depth = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
```
`dsp/delay.rs`:
```rust
impl DelayParams {
    pub const TIME_MS: ParamId = ParamId(0);
    pub const FEEDBACK: ParamId = ParamId(1);
    pub const WOW_FLUTTER: ParamId = ParamId(2);
    pub const SATURATION: ParamId = ParamId(3);
    pub const TONE: ParamId = ParamId(4);
    pub const MIX: ParamId = ParamId(5);
}

/// Delay runs outside `Voice` (desktop only): nothing is modulatable.
pub static DELAY_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "TIME", ValFmt::Uni, 10.0, 1000.0, 375.0, 8.0, false),
    ParamSpec::continuous(1, "FDBK", ValFmt::Uni, 0.0, 1.0, 0.4, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "WOW", ValFmt::Uni, 0.0, 1.0, 0.15, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "SAT", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "TONE", ValFmt::Uni, 0.0, 1.0, 0.6, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for DelayParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &DELAY_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::TIME_MS => self.time_ms,
            Self::FEEDBACK => self.feedback,
            Self::WOW_FLUTTER => self.wow_flutter,
            Self::SATURATION => self.saturation,
            Self::TONE => self.tone,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::TIME_MS => self.time_ms = v,
            Self::FEEDBACK => self.feedback = v,
            Self::WOW_FLUTTER => self.wow_flutter = v,
            Self::SATURATION => self.saturation = v,
            Self::TONE => self.tone = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
```
`dsp/reverb.rs`:
```rust
impl ReverbParams {
    pub const REVERB_TYPE: ParamId = ParamId(0);
    pub const TIME: ParamId = ParamId(1);
    pub const DAMPING: ParamId = ParamId(2);
    pub const SIZE: ParamId = ParamId(3);
    pub const MIX: ParamId = ParamId(4);
}

/// Reverb runs outside `Voice` (desktop only): nothing is modulatable.
pub static REVERB_SPECS: [ParamSpec; 5] = [
    ParamSpec::choice(0, "TYPE", ValFmt::Int(2), 2.0, 0.0),
    ParamSpec::continuous(1, "TIME", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DAMP", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "SIZE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "MIX", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
];

impl Block for ReverbParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &REVERB_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::REVERB_TYPE => self.reverb_type as f32,
            Self::TIME => self.time,
            Self::DAMPING => self.damping,
            Self::SIZE => self.size,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::REVERB_TYPE => self.reverb_type = v as u8,
            Self::TIME => self.time = v,
            Self::DAMPING => self.damping = v,
            Self::SIZE => self.size = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Finish `page.rs` and delete `Param`**

In `chimera-core/src/ui/page.rs`:
1. Add imports `use crate::dsp::chorus::ChorusParams;`, `use crate::dsp::delay::DelayParams;`, `use crate::dsp::reverb::ReverbParams;`.
2. `read_values`: replace the `Chorus`, `Delay`, `Efx | MixReverb` arms with
```rust
            PageId::Chorus => read_block(&params.chorus, CHORUS_PAGE),
            PageId::Delay => read_block(&params.delay, DELAY_PAGE),
            PageId::Efx | PageId::MixReverb => read_block(&params.reverb, REVERB_PAGE),
```
(plan D19: the Delay TIME bar is now `(t−10)/990`).
3. Replace `apply_encoder` and `snap_encoder` (everything from `/// Apply an encoder delta.` to the end of `snap_encoder`) with:
```rust
    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if *self == PageId::FmOp && idx == 0 {
            // Operator select: 0-3
            let cur = fm_selected_op() as i16;
            fm_set_selected_op((cur + delta as i16).clamp(0, 3) as u8);
            return;
        }
        if let Some((blk, id)) = self.resolve_mut(idx, params) {
            blk.nudge(id, delta);
        }
    }

    /// Shift+encoder: snap to the coarse points of the bound param's format.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if let Some((blk, id)) = self.resolve_mut(idx, params) {
            blk.snap(id, delta);
        }
    }
```
4. `resolve_mut`: add
```rust
            PageId::Chorus => bind(&mut p.chorus, *CHORUS_PAGE.get(idx)?),
            PageId::Delay => bind(&mut p.delay, *DELAY_PAGE.get(idx)?),
            PageId::Efx | PageId::MixReverb => bind(&mut p.reverb, *REVERB_PAGE.get(idx)?),
```
5. Delete `fn resolve_param_mut` (and its doc comment), `fn apply_chorus_encoder`, `fn apply_delay_encoder`, `fn apply_reverb_encoder`, `fn nudge_float`, `fn nudge_u8`. Add above `const MODAL1_PAGE`:
```rust
const CHORUS_PAGE: [ParamId; 4] = [
    ChorusParams::MODE,
    ChorusParams::RATE,
    ChorusParams::DEPTH,
    ChorusParams::MIX,
];
const DELAY_PAGE: [ParamId; 6] = [
    DelayParams::TIME_MS,
    DelayParams::FEEDBACK,
    DelayParams::WOW_FLUTTER,
    DelayParams::SATURATION,
    DelayParams::TONE,
    DelayParams::MIX,
];
const REVERB_PAGE: [ParamId; 5] = [
    ReverbParams::REVERB_TYPE,
    ReverbParams::TIME,
    ReverbParams::DAMPING,
    ReverbParams::SIZE,
    ReverbParams::MIX,
];
```
In `chimera-core/src/params.rs`, delete `pub struct Param` and its `impl Param` (from `/// A single parameter value with range clamping` to the line before `/// Parameters for one voice's filter`).

In `chimera-core/src/ui/mod.rs`, replace
```rust
                        let fmt = def.params[i].format;
                        page.snap_encoder(i, delta, fmt, &mut self.project.tracks[at].patch.params);
```
with
```rust
                        page.snap_encoder(i, delta, &mut self.project.tracks[at].patch.params);
```

- [ ] **Step 5: Update tests (mechanical)**

```bash
git rm chimera-core/tests/param_test.rs
sed -i 's/snap_encoder(\([^,]*\), \([^,]*\), ValFmt::[A-Za-z]*\(([0-9]*)\)\?, /snap_encoder(\1, \2, /' chimera-core/tests/*.rs
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
grep -rnw 'Param' chimera-core/src | grep -v '//'   # must print nothing
```

- [ ] **Step 6: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test block_test --test block_spec_test --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `23 passed`, `1 passed`, `25 passed`, `4 passed`, all `ok`.
Run: `cargo test -p chimera-core 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6} END {print p, "passed", f, "failed"}'`
Expected: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/dsp/chorus.rs chimera-core/src/dsp/delay.rs chimera-core/src/dsp/reverb.rs chimera-core/src/ui/page.rs chimera-core/src/params.rs chimera-core/src/ui/mod.rs chimera-core/tests/
git commit -m "refactor(core): FX own their specs; delete Param

Every page encoder and shift-snap now goes through a ParamSpec. ParamSnapshot
is 376 B (was 1,524 B).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 12: `MidiNote` / `Velocity` at the MIDI trust boundary

**Files:**
- Modify: `chimera-hal/src/lib.rs` (types; `MidiMessage` uses them; `pub mod midi;`)
- Create: `chimera-hal/src/midi.rs` (the parser, moved from `chimera-stm32/src/midi.rs` — plan D21)
- Delete: `chimera-stm32/src/midi.rs`; Modify: `chimera-stm32/src/main.rs:8` (drop `mod midi;`)
- Modify: `chimera-core/src/lib.rs` (re-export)
- Create: `chimera-core/tests/midi_types_test.rs`

**Interfaces:**
- Consumes: nothing.
- Produces (used by Task 13 and the binaries):
  - `chimera_hal::MidiNote` — `pub const A4`, `pub const fn new(n: u8) -> Option<MidiNote>` (0..=127), `pub const fn get(self) -> u8`
  - `chimera_hal::Velocity` — `pub const MAX`, `pub const DEFAULT` (100), `pub const fn new(v: u8) -> Option<Velocity>` (1..=127), `pub const fn get(self) -> u8`, `pub fn unit(self) -> f32` (= `v as f32 / 127.0`)
  - `MidiMessage::NoteOn { channel: u8, note: MidiNote, velocity: Velocity }`, `MidiMessage::NoteOff { channel: u8, note: MidiNote, velocity: u8 }`; `MidiMessage` derives `PartialEq, Eq`
  - `chimera_hal::midi::MidiParser { new, feed(&mut self, u8) -> Option<MidiMessage> }` (+ `Default`)
  - `chimera_core::{MidiNote, Velocity}` (re-export)

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/midi_types_test.rs`:
```rust
//! MIDI trust boundary (spec §3, Review Focus 4): only valid notes and
//! note-on velocities are representable, and the parser builds them.

use chimera_core::{MidiNote, Velocity};
use chimera_hal::midi::MidiParser;
use chimera_hal::MidiMessage;

#[test]
fn midi_note_accepts_0_to_127_only() {
    assert_eq!(MidiNote::new(0).map(MidiNote::get), Some(0));
    assert_eq!(MidiNote::new(127).map(MidiNote::get), Some(127));
    assert_eq!(MidiNote::new(128), None);
    assert_eq!(MidiNote::new(255), None);
}

#[test]
fn velocity_accepts_1_to_127_only() {
    assert_eq!(Velocity::new(0), None);
    assert_eq!(Velocity::new(1).map(Velocity::get), Some(1));
    assert_eq!(Velocity::new(127), Some(Velocity::MAX));
    assert_eq!(Velocity::new(128), None);
}

#[test]
fn velocity_unit_is_the_old_formula() {
    for v in 1..=127u8 {
        assert_eq!(Velocity::new(v).unwrap().unit(), v as f32 / 127.0);
    }
}

fn feed(p: &mut MidiParser, bytes: &[u8]) -> Vec<MidiMessage> {
    bytes.iter().filter_map(|&b| p.feed(b)).collect()
}

#[test]
fn parser_builds_note_on() {
    let msgs = feed(&mut MidiParser::new(), &[0x90, 60, 127]);
    assert_eq!(
        msgs,
        [MidiMessage::NoteOn { channel: 0, note: MidiNote::new(60).unwrap(), velocity: Velocity::MAX }]
    );
}

/// Velocity 0 is a note-off, never a zero-velocity note-on.
#[test]
fn parser_velocity_zero_is_note_off() {
    let msgs = feed(&mut MidiParser::new(), &[0x91, 127, 0]);
    assert_eq!(msgs, [MidiMessage::NoteOff { channel: 1, note: MidiNote::new(127).unwrap(), velocity: 0 }]);
}

#[test]
fn parser_running_status_note_on_then_off() {
    let msgs = feed(&mut MidiParser::new(), &[0x90, 60, 100, 60, 0]);
    let n60 = MidiNote::new(60).unwrap();
    assert_eq!(
        msgs,
        [
            MidiMessage::NoteOn { channel: 0, note: n60, velocity: Velocity::DEFAULT },
            MidiMessage::NoteOff { channel: 0, note: n60, velocity: 0 },
        ]
    );
}

#[test]
fn parser_note_off_keeps_release_velocity() {
    let msgs = feed(&mut MidiParser::new(), &[0x82, 64, 64]);
    assert_eq!(msgs, [MidiMessage::NoteOff { channel: 2, note: MidiNote::new(64).unwrap(), velocity: 64 }]);
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test midi_types_test 2>&1 | grep -E '^error\[' | head -3`
Expected: `unresolved imports chimera_core::MidiNote, chimera_core::Velocity` and `could not find midi in chimera_hal`.

- [ ] **Step 3: Add the types to `chimera-hal`**

In `chimera-hal/src/lib.rs`, add `pub mod midi;` after the `use` lines, and replace the `MidiMessage` enum (and its derive line) with:
```rust
/// MIDI note number, 0..=127. Built at the MIDI trust boundary (the parser,
/// the desktop keyboard), so the audio path only ever sees valid notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MidiNote(u8);

impl MidiNote {
    /// A4 (440 Hz).
    pub const A4: MidiNote = MidiNote(69);

    pub const fn new(n: u8) -> Option<Self> {
        if n <= 127 { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Note-on velocity, 1..=127. Zero means note-off and is not representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(u8);

impl Velocity {
    pub const MAX: Velocity = Velocity(127);
    /// Velocity of the desktop keyboard and the firmware test note.
    pub const DEFAULT: Velocity = Velocity(100);

    pub const fn new(v: u8) -> Option<Self> {
        if v >= 1 && v <= 127 { Some(Self(v)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Velocity as 0..1, exactly `v as f32 / 127.0` (what the engines used).
    pub fn unit(self) -> f32 {
        self.0 as f32 / 127.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: MidiNote, velocity: Velocity },
    /// Release velocity may be 0.
    NoteOff { channel: u8, note: MidiNote, velocity: u8 },
    ControlChange { channel: u8, cc: u8, value: u8 },
    PitchBend { channel: u8, value: i16 },
}
```

- [ ] **Step 4: Move the parser to `chimera-hal/src/midi.rs`**

```bash
git mv chimera-stm32/src/midi.rs chimera-hal/src/midi.rs
sed -i '/^mod midi;$/d' chimera-stm32/src/main.rs
```
In `chimera-hal/src/midi.rs`:
1. Replace the module doc and `use` (first 6 lines) with:
```rust
//! MIDI byte-stream parser — the trust boundary where raw bytes become
//! `MidiNote`/`Velocity`. Hardware-independent (moved from chimera-stm32 so
//! it is host-testable); the firmware feeds it bytes from USART1 @ 31250.

use crate::{MidiMessage, MidiNote, Velocity};
```
2. Add above `pub struct MidiParser`:
```rust
impl Default for MidiParser {
    fn default() -> Self {
        Self::new()
    }
}

```
3. In `make_message`, replace the `0x90 => { … }` and `0x80 => …` arms with:
```rust
            0x90 => {
                let note = MidiNote::new(self.data[0])?;
                match Velocity::new(self.data[1]) {
                    Some(velocity) => Some(MidiMessage::NoteOn { channel, note, velocity }),
                    // Note-on with velocity 0 is a note-off (MIDI 1.0 spec).
                    None => Some(MidiMessage::NoteOff { channel, note, velocity: 0 }),
                }
            }
            0x80 => Some(MidiMessage::NoteOff {
                channel,
                note: MidiNote::new(self.data[0])?,
                velocity: self.data[1],
            }),
```
In `chimera-core/src/lib.rs`, add after `#![no_std]`:
```rust

pub use chimera_hal::{MidiNote, Velocity};
```

- [ ] **Step 5: Run the tests, goldens and suite; check the firmware crate**

Run: `cargo test -p chimera-core --test midi_types_test --test golden_test 2>&1 | grep '^test result'`
Expected: `7 passed`, `4 passed`. Full suite: `0 failed`.
Run: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf 2>&1 | tail -2` — expected to compile (only `mod midi;` was removed). If the target is not installed (`rustup target add thumbv7em-none-eabihf`), report "firmware not compiled: target missing".

- [ ] **Step 6: Commit**

```bash
git add chimera-hal/src/lib.rs chimera-hal/src/midi.rs chimera-stm32/src/main.rs chimera-core/src/lib.rs chimera-core/tests/midi_types_test.rs
git commit -m "feat(hal): MidiNote and Velocity built at the MIDI parser

The parser moves to chimera-hal (it was unused in the firmware) so the trust
boundary is host-tested: velocity 0 is a note-off, >127 is unrepresentable.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 13: `Engines` with explicit VCA/activity rules; `Voice` uses it

**Files:**
- Create: `chimera-core/src/dsp/engines.rs`
- Modify: `chimera-core/src/dsp/mod.rs` (`pub mod engines;`)
- Modify: `chimera-core/src/params.rs` (`EngineType::ALL`)
- Replace: `chimera-core/src/dsp/voice.rs` (full new content below)
- Modify: `chimera-core/tests/common/mod.rs` (`expects_sound`; harness picks up the new `Voice` API via the mechanical step)
- Create: `chimera-core/tests/engines_test.rs`
- Modify: `chimera-core/tests/engine_switch_test.rs`, `chimera-core/tests/property_test.rs` (iterate `EngineType::ALL`)
- Modify (mechanical): every test that calls `Voice::new`, `voice*.note_on`, `voice*.render`
- Modify: `chimera-desktop/src/{main,audio}.rs`, `chimera-stm32/src/{main,audio}.rs`

**Interfaces:**
- Consumes: Task 12 (`MidiNote`, `Velocity`, `Velocity::unit`), Tasks 2–11 (`apply_offset`, block ids).
- Produces:
  - `pub struct Engines` — `pub fn new(sample_rate: u32) -> Self`, `pub fn sample_rate(&self) -> u32`, `pub fn note_on(&mut self, kind: EngineType, note: MidiNote, vel: Velocity, p: &ParamSnapshot)`, `pub fn note_off(&mut self, kind: EngineType)`, `pub fn render(&mut self, kind: EngineType, out: &mut [f32; BLOCK_SIZE], p: &ParamSnapshot)`, `pub fn uses_amp_env(kind: EngineType) -> bool`, `pub fn is_active(&self, kind: EngineType, amp_env: &Envelope) -> bool`
  - **Signature changes (plan D14):** `Voice::new(sample_rate: u32) -> Voice`, `Voice::sample_rate(&self) -> u32`, `Voice::note_on(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot)`, `Voice::render(&mut self, output: &mut [f32; BLOCK_SIZE], params: &ParamSnapshot, mod_state: &ModState)`. `Voice`'s `pub pizza/modal/fm` fields are gone (nothing used them).
  - `EngineType::ALL: [EngineType; 4]`
  - `tests/common/mod.rs`: `pub fn expects_sound(e: EngineType) -> bool` (exhaustive match)

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/common/mod.rs`:
```rust
/// Whether an engine produces sound from its default params. Exhaustive on
/// purpose: adding an `EngineType` variant fails to compile here until its
/// expectation is written (spec § Testing "Engines").
pub fn expects_sound(e: EngineType) -> bool {
    match e {
        EngineType::Pizza | EngineType::Fm | EngineType::Modal => true,
        EngineType::Va => false,
    }
}
```

Create `chimera-core/tests/engines_test.rs`:
```rust
//! `Engines` dispatch and the VCA/activity table (spec §3), one test per row:
//!
//! | Engine | Amp env on VCA | Voice active while     |
//! |--------|----------------|------------------------|
//! | Pizza  | yes            | `amp_env.is_active()`  |
//! | FM     | yes            | `!fm.is_idle()`        |
//! | Modal  | no             | `modal.is_active()`    |
//! | Va     | yes (silent)   | never                  |

mod common;

use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::envelope::Envelope;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{expects_sound, SR};

fn params(kind: EngineType) -> ParamSnapshot {
    let mut p = ParamSnapshot::default();
    p.engine = kind;
    p
}

fn render(e: &mut Engines, kind: EngineType, p: &ParamSnapshot, blocks: usize) -> f32 {
    let mut out = [0.0f32; BLOCK_SIZE];
    let mut peak = 0.0f32;
    for _ in 0..blocks {
        e.render(kind, &mut out, p);
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

#[test]
fn all_lists_every_engine_once_in_order() {
    assert_eq!(EngineType::ALL.len(), EngineType::Va as usize + 1);
    for (i, &e) in EngineType::ALL.iter().enumerate() {
        assert_eq!(e as usize, i);
        let _ = expects_sound(e); // exhaustive match: new variants fail to compile
    }
}

#[test]
fn every_engine_renders_per_its_expectation() {
    for kind in EngineType::ALL {
        let mut e = Engines::new(SR);
        e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
        let peak = render(&mut e, kind, &params(kind), 8);
        assert!(peak.is_finite());
        assert_eq!(peak > 1e-3, expects_sound(kind), "{kind:?}: peak {peak}");
    }
}

#[test]
fn pizza_row_amp_env_on_vca_and_lives_with_it() {
    let kind = EngineType::Pizza;
    assert!(Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let mut env = Envelope::new();
    assert!(!e.is_active(kind, &env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    env.note_on(Velocity::DEFAULT.unit());
    assert!(e.is_active(kind, &env));
    // The oscillator keeps running after note-off; only the envelope ends the voice.
    e.note_off(kind);
    render(&mut e, kind, &params(kind), 10);
    assert!(e.is_active(kind, &env));
    assert!(!e.is_active(kind, &Envelope::new()));
}

#[test]
fn fm_row_amp_env_on_vca_and_lives_until_operators_idle() {
    let kind = EngineType::Fm;
    assert!(Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let idle_env = Envelope::new(); // FM activity ignores the amp envelope
    assert!(!e.is_active(kind, &idle_env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    render(&mut e, kind, &params(kind), 1);
    assert!(e.is_active(kind, &idle_env));
    e.note_off(kind);
    render(&mut e, kind, &params(kind), 400);
    assert!(!e.is_active(kind, &idle_env));
}

#[test]
fn modal_row_no_amp_env_and_lives_until_modes_decay() {
    let kind = EngineType::Modal;
    assert!(!Engines::uses_amp_env(kind));
    let mut p = params(kind);
    p.modal.mode = ResonatorMode::Modal;
    p.modal.decay = 0.0;
    let mut e = Engines::new(SR);
    let mut running_env = Envelope::new();
    running_env.note_on(1.0); // Modal activity ignores the amp envelope
    assert!(!e.is_active(kind, &running_env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &p);
    assert!(e.is_active(kind, &running_env));
    e.note_off(kind);
    render(&mut e, kind, &p, 400);
    assert!(!e.is_active(kind, &running_env));
}

#[test]
fn va_row_amp_env_on_vca_never_active_silent() {
    let kind = EngineType::Va;
    assert!(Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let mut env = Envelope::new();
    env.note_on(1.0);
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    assert!(!e.is_active(kind, &env));
    assert_eq!(render(&mut e, kind, &params(kind), 4), 0.0);
}
```

Append to `chimera-core/tests/engine_switch_test.rs` (it gets its `MidiNote`/`Velocity` import in Step 5):
```rust
/// Spec § Testing "Engines": every engine pair switches mid-note (hard cut,
/// retrigger) without panicking or producing non-finite output.
#[test]
fn every_engine_pair_switches_mid_note() {
    for from in EngineType::ALL {
        for to in EngineType::ALL {
            let mut a = ParamSnapshot::default();
            a.engine = from;
            let mut b = ParamSnapshot::default();
            b.engine = to;
            let mut voice = Voice::new(SR);
            voice.note_on(MidiNote::A4, Velocity::DEFAULT, &a);
            let mut block = [0.0f32; 64];
            for i in 0..16 {
                voice.render(&mut block, if i < 8 { &a } else { &b }, &ModState::new());
                assert!(block.iter().all(|x| x.is_finite()), "{from:?} -> {to:?}");
            }
        }
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test engines_test 2>&1 | grep -E '^error\[' | head -3`
Expected: `could not find engines in dsp` / `no associated item named ALL found for enum EngineType`.

- [ ] **Step 3: Create `chimera-core/src/dsp/engines.rs`**

```rust
//! Persistent engine instances with dispatch in one place (spec §3).
//!
//! Engines are never constructed in the audio interrupt: `ModalEngine` is
//! ~66 KB and the stack has no guard. Adding an engine = one field here plus
//! one arm in each exhaustive `match` below; the compiler lists them.

use chimera_hal::BLOCK_SIZE;

use crate::dsp::engine_fm::FmEngine;
use crate::dsp::envelope::Envelope;
use crate::dsp::modal::ModalEngine;
use crate::dsp::pizza::PizzaOsc;
use crate::params::{EngineType, ParamSnapshot};
use crate::{MidiNote, Velocity};

pub struct Engines {
    pizza: PizzaOsc,
    fm: FmEngine,
    modal: ModalEngine,
    sample_rate: u32,
}

impl Engines {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            pizza: PizzaOsc::new(),
            fm: FmEngine::new(),
            modal: ModalEngine::new(),
            sample_rate,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn note_on(&mut self, kind: EngineType, note: MidiNote, vel: Velocity, p: &ParamSnapshot) {
        match kind {
            EngineType::Pizza => {
                self.pizza.note_on(crate::dsp::note_to_freq(note.get()), self.sample_rate)
            }
            EngineType::Fm => {
                self.fm.note_on_params(note.get(), vel.unit(), &p.fm, self.sample_rate as f32)
            }
            EngineType::Modal => self.modal.note_on(note.get(), vel.get(), &p.modal, self.sample_rate),
            EngineType::Va => {} // placeholder: silent
        }
    }

    pub fn note_off(&mut self, kind: EngineType) {
        match kind {
            EngineType::Pizza => self.pizza.note_off(),
            EngineType::Fm => self.fm.note_off(),
            EngineType::Modal => self.modal.note_off(),
            EngineType::Va => {}
        }
    }

    /// Render one block of raw engine output from (possibly modulated) params.
    pub fn render(&mut self, kind: EngineType, out: &mut [f32; BLOCK_SIZE], p: &ParamSnapshot) {
        match kind {
            EngineType::Pizza => self.pizza.render(out, &p.pizza, self.sample_rate),
            EngineType::Fm => self.fm.render_params(out, &p.fm),
            EngineType::Modal => self.modal.render(out, &p.modal, self.sample_rate),
            EngineType::Va => out.fill(0.0),
        }
    }

    /// VCA choice: does the amp envelope shape this engine's output?
    /// Modal's modes decay naturally, so it only gets the volume.
    pub fn uses_amp_env(kind: EngineType) -> bool {
        match kind {
            EngineType::Pizza | EngineType::Fm | EngineType::Va => true,
            EngineType::Modal => false,
        }
    }

    /// Voice lifetime: is this engine still sounding?
    pub fn is_active(&self, kind: EngineType, amp_env: &Envelope) -> bool {
        match kind {
            EngineType::Pizza => amp_env.is_active(),
            EngineType::Fm => !self.fm.is_idle(),
            EngineType::Modal => self.modal.is_active(),
            EngineType::Va => false,
        }
    }
}
```

Register it in `chimera-core/src/dsp/mod.rs` (after `pub mod engine_fm;`): `pub mod engines;`

In `chimera-core/src/params.rs`, directly after the `EngineType` enum:
```rust
impl EngineType {
    /// Every engine. Tests iterate this; see `engines_test.rs` for the
    /// exhaustive-match guard that makes a new variant a compile error there.
    pub const ALL: [EngineType; 4] = [EngineType::Pizza, EngineType::Fm, EngineType::Modal, EngineType::Va];
}
```

- [ ] **Step 4: Replace `chimera-core/src/dsp/voice.rs`**

The render path is reordered but arithmetically identical: one stack copy `m` carries every modulated block (Pizza/Drive/Filter/Folder/FM op levels, the same hard-coded paths and formula as before); Modal, the amp envelope and volume read unmodulated values exactly as before because nothing routes to them yet. The goldens prove it.
```rust
use chimera_hal::BLOCK_SIZE;

use crate::block::apply_offset;
use crate::dsp::drive::Drive;
use crate::dsp::engines::Engines;
use crate::dsp::envelope::Envelope;
use crate::dsp::filter::SvfFilter;
use crate::dsp::lfo::Lfo;
use crate::dsp::pizza::PizzaParams;
use crate::dsp::wavefolder::Wavefolder;
use crate::mod_path::ParamPath;
use crate::modulation::{ModState, MAX_MOD_SOURCES};
use crate::params::{DriveParams, EngineType, FilterParams, FmOpParams, FolderParams, ParamSnapshot};
use crate::{MidiNote, Velocity};

/// Complete voice signal chain:
/// [Engine] → [Drive] → [Filter] → [Wavefolder] → [VCA]
/// Modulators: Envelope + LFO
pub struct Voice {
    engines: Engines,
    drive: Drive,
    filter: SvfFilter,
    folder: Wavefolder,
    amp_env: Envelope,
    pub lfo: Lfo,
    active_engine: EngineType,
    active: bool,
    last_note: MidiNote,
    last_velocity: Velocity,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new(chimera_hal::SAMPLE_RATE)
    }
}

impl Voice {
    /// The sample rate is stored once (spec §3), not passed per call.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            engines: Engines::new(sample_rate),
            drive: Drive::new(),
            filter: SvfFilter::new(),
            folder: Wavefolder::new(),
            amp_env: Envelope::new(),
            lfo: Lfo::new(),
            active_engine: EngineType::Pizza,
            active: false,
            last_note: MidiNote::A4,
            last_velocity: Velocity::DEFAULT,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.engines.sample_rate()
    }

    pub fn note_on(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot) {
        self.active_engine = params.engine;
        self.last_note = note;
        self.last_velocity = velocity;
        self.engines.note_on(self.active_engine, note, velocity, params);
        self.amp_env.note_on(velocity.unit());
        self.active = true;
    }

    pub fn note_off(&mut self) {
        self.engines.note_off(self.active_engine);
        self.amp_env.note_off();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(&mut self, output: &mut [f32; BLOCK_SIZE], params: &ParamSnapshot, mod_state: &ModState) {
        let sample_rate = self.sample_rate();

        // Auto-retrigger if engine type changed (e.g., user loaded FM patch)
        if self.active && params.engine != self.active_engine {
            self.note_on(self.last_note, self.last_velocity, params);
        }

        if !self.active {
            output.fill(0.0);
            return;
        }

        // Compute modulator source values
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        // Source 0 = Envelope
        if mod_state.num_sources > 0 {
            mod_values[0] = self.amp_env.current_level();
        }
        // Source 1 = LFO
        if mod_state.num_sources > 1 {
            mod_values[1] = self.lfo.process(&params.lfo, sample_rate);
        }

        // Modulated copy (stack only). Offsets still use the chain-index paths
        // `Voice` hard-coded before the refactor; Task 16 makes this generic.
        let mut m = params.clone();
        let off = |block: u8, param: u8| mod_state.compute_offset(&mod_values, ParamPath::Block { block, param });
        apply_offset(&mut m.pizza, PizzaParams::SHAPE, off(0, 0));
        apply_offset(&mut m.pizza, PizzaParams::CRUSH, off(0, 1));
        apply_offset(&mut m.pizza, PizzaParams::LEVEL, off(0, 2));
        apply_offset(&mut m.drive, DriveParams::DRIVE, off(1, 0));
        apply_offset(&mut m.drive, DriveParams::TONE, off(1, 1));
        apply_offset(&mut m.filter, FilterParams::CUTOFF, off(2, 0));
        apply_offset(&mut m.filter, FilterParams::RESONANCE, off(2, 1));
        apply_offset(&mut m.folder, FolderParams::FOLD, off(3, 0));
        apply_offset(&mut m.folder, FolderParams::SYMMETRY, off(3, 1));
        for (op, p) in m.fm.operators.iter_mut().enumerate() {
            let offset = mod_state.compute_offset(&mod_values, ParamPath::FmOp { op: op as u8, param: 2 });
            if offset != 0.0 {
                apply_offset(p, FmOpParams::LEVEL, offset);
            }
        }

        // 1. Engine → raw oscillator output
        self.engines.render(self.active_engine, output, &m);

        // 2. Drive
        self.drive.process(output, &m.drive);

        // 3. Filter
        self.filter.process(output, &m.filter, sample_rate);

        // 4. Wavefolder
        self.folder.process(output, &m.folder);

        // 5. VCA
        let volume = m.out.volume;
        if Engines::uses_amp_env(self.active_engine) {
            for sample in output.iter_mut() {
                let env = self.amp_env.process(&m.envelopes[0], sample_rate);
                *sample *= env * volume;
            }
        } else {
            for sample in output.iter_mut() {
                *sample *= volume;
            }
        }

        // 6. Scope — capture end-of-chain for oscilloscope display
        crate::scope::write_samples(output);

        // Check if done
        self.active = self.engines.is_active(self.active_engine, &self.amp_env);
    }
}
```

- [ ] **Step 5: Migrate test call sites (mechanical)**

Rules: `Voice::new()` → `Voice::new(chimera_hal::SAMPLE_RATE)`; `voiceX.note_on(n, v, p, SR)` → `voiceX.note_on(MidiNote::new(n).unwrap(), Velocity::new(v).unwrap(), p)`; `voiceX.render(buf, p, ms, SR)` → `voiceX.render(buf, p, ms)` (the rate argument is `SR`, `48000` or `SAMPLE_RATE` in today's tests). Commands (bash- and zsh-safe):
```bash
cd chimera-core/tests
grep -l 'voice[A-Za-z0-9_]*\.note_on(\|Voice::new()' *.rs common/mod.rs | xargs perl -pi -e 's/Voice::new\(\)/Voice::new(chimera_hal::SAMPLE_RATE)/g; s/(voice\w*)\.note_on\(([^,]+), ([^,]+), (.+), (?:SR|48000|SAMPLE_RATE)\)/$1.note_on(MidiNote::new($2).unwrap(), Velocity::new($3).unwrap(), $4)/; s/(voice\w*\.render\(.+), (?:SR|48000|SAMPLE_RATE)\)/$1)/'
grep -l 'MidiNote::new(' *.rs common/mod.rs | xargs grep -L '^use chimera_core::{MidiNote, Velocity};' | xargs perl -0pi -e 's/^(use chimera_core::)/use chimera_core::{MidiNote, Velocity};\n$1/m'
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```
(`const SR` / `SAMPLE_RATE` imports that become unused only warn.)

Make `property_test.rs` iterate every engine. Add `mod common;` as the first item after its `//!` doc comment and `use common::expects_sound;` with the other `use`s; then:
1. In `random_params`, replace the `// Engine type` block (`let engine = rng.u8(1); …` through the closing `};`) with
```rust
    // Engine type: every engine (spec § Testing "Engines")
    p.engine = EngineType::ALL[rng.u8(EngineType::ALL.len() as u8 - 1) as usize];
```
2. In `prop_note_on_produces_sound`, right after `let note = rng.note();` add
```rust
        if !expects_sound(params.engine) {
            continue;
        }
```
3. In `prop_param_change_changes_output`, right after `let note = 60;` add
```rust
        if !expects_sound(params_a.engine) {
            continue;
        }
```

- [ ] **Step 6: Update the binaries**

`chimera-stm32/src/audio.rs`: `use chimera_hal::BLOCK_SIZE;` → `use chimera_hal::{BLOCK_SIZE, MidiNote, Velocity};`; `voice.render(work, params, mod_state, chimera_hal::SAMPLE_RATE);` → `voice.render(work, params, mod_state);`; `Voice::new()` → `Voice::new(chimera_hal::SAMPLE_RATE)`; `pub fn trigger_note(note: u8, velocity: u8)` → `pub fn trigger_note(note: MidiNote, velocity: Velocity)` and its call `voice.note_on(note, velocity, &*p, chimera_hal::SAMPLE_RATE);` → `voice.note_on(note, velocity, &*p);`.
`chimera-stm32/src/main.rs`: `audio::trigger_note(69, 100);  // A4, velocity 100` → `audio::trigger_note(chimera_hal::MidiNote::A4, chimera_hal::Velocity::DEFAULT);`

`chimera-desktop/src/audio.rs`: add `use chimera_core::{MidiNote, Velocity};`; `Box::new(Voice::new())` → `Box::new(Voice::new(sample_rate))`; `voice.render(&mut block, params, &empty_mod, sample_rate);` → `voice.render(&mut block, params, &empty_mod);`; replace the note-on branch of the callback with
```rust
                    if cmd & NOTE_ON_FLAG != 0 {
                        let vel = shared_clone.velocity.load(Ordering::Relaxed);
                        // Both were stored from a MidiNote/Velocity, so these always succeed.
                        if let (Some(note), Some(vel)) = (MidiNote::new(cmd & 0x7F), Velocity::new(vel)) {
                            voice.note_on(note, vel, params);
                        }
                    } else if cmd > 0 {
```
and `DesktopAudio::note_on` becomes
```rust
    pub fn note_on(&self, note: MidiNote, velocity: Velocity) {
        self.shared.velocity.store(velocity.get(), Ordering::Relaxed);
        self.shared
            .note_cmd
            .store(NOTE_ON_FLAG | note.get(), Ordering::Relaxed);
    }
```
`chimera-desktop/src/main.rs`: `use chimera_hal::ChimeraDisplay;` → `use chimera_hal::{ChimeraDisplay, MidiNote, Velocity};`; `let mut current_note: Option<u8> = None;` → `Option<MidiNote>`; the piano line becomes
```rust
        let note = piano_note(&keys)
            .and_then(|n| MidiNote::new((n as i8 + octave * 12).clamp(0, 127) as u8));
```
and `audio.note_on(n, 100);` → `audio.note_on(n, Velocity::DEFAULT);`.

- [ ] **Step 7: Run everything**

Run: `cargo test -p chimera-core --test engines_test --test engine_switch_test --test property_test --test golden_test 2>&1 | grep '^test result'`
Expected: `6 passed`, `5 passed`, `16 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.
Run: `cargo check -p chimera-desktop 2>&1 | tail -3` — on this machine it stops in `alsa-sys` (needs `sudo apt install libasound2-dev`); report "desktop not compiled: ALSA headers missing" if so.
Run: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf 2>&1 | tail -3` — report "not compiled: target missing" if `rustup target add thumbv7em-none-eabihf` has not been run.

- [ ] **Step 8: Commit**

```bash
git add chimera-core/src/dsp/engines.rs chimera-core/src/dsp/mod.rs chimera-core/src/dsp/voice.rs chimera-core/src/params.rs chimera-core/tests/ chimera-desktop/src/main.rs chimera-desktop/src/audio.rs chimera-stm32/src/main.rs chimera-stm32/src/audio.rs
git commit -m "refactor(core): Engines owns persistent engines; Voice dispatches once

VCA and voice-lifetime rules are explicit per engine. Voice takes the sample
rate at construction and MidiNote/Velocity at note-on. Property and
engine-switch tests cover every EngineType.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 14: `ParamAddr`, `BlockRef`, `Op`; `ParamSnapshot::block(_mut)`

**Files:**
- Create: `chimera-core/src/addr.rs`
- Modify: `chimera-core/src/lib.rs` (`pub mod addr;`)
- Modify: `chimera-core/src/params.rs` (import `BlockRef`; `impl ParamSnapshot { block, block_mut }`)
- Create: `chimera-core/tests/addr_test.rs`
- Modify: `chimera-core/tests/block_spec_test.rs` (iterate `BlockRef::ALL`)

**Interfaces:**
- Consumes: every `*_SPECS` table and `Block` impl from Tasks 2–11.
- Produces (alongside `ParamPath`, which stays until Task 19):
  - `pub enum Op { A, B, C, D }` — `Op::ALL: [Op; 4]`, `const fn index(self) -> usize`, `fn nudged(self, delta: i8) -> Op`, `impl TryFrom<u8> for Op { type Error = OpOutOfRange }`; `pub struct OpOutOfRange(pub u8)`
  - `pub enum BlockRef { Pizza, Modal, Fm, FmOp(Op), Drive, Filter, Folder, AmpEnv, FilterEnv, AuxEnv, Lfo, Out, Chorus, Delay, Reverb }` — `BlockRef::ALL: [BlockRef; 18]`, `fn specs(self) -> &'static [ParamSpec]`, `const fn voice_reads(self) -> bool`
  - `pub struct ParamAddr { pub block: BlockRef, pub param: ParamId }` — `const fn new(block, param)`, `fn spec(self) -> Option<&'static ParamSpec>`, `fn modulatable(self) -> bool` (plan D7)
  - `ParamSnapshot::block(&self, b: BlockRef) -> &dyn Block`, `ParamSnapshot::block_mut(&mut self, b: BlockRef) -> &mut dyn Block`
  - All four derive `Clone, Copy, Debug, PartialEq, Eq`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/addr_test.rs`:
```rust
//! Semantic addresses (spec §2) and `ParamSnapshot::block(_mut)`.

use chimera_core::addr::{BlockRef, Op, OpOutOfRange, ParamAddr};
use chimera_core::block::Block;
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FolderParams, OutParams, ParamSnapshot};

#[test]
fn op_rejects_out_of_range() {
    for (i, op) in Op::ALL.iter().enumerate() {
        assert_eq!(Op::try_from(i as u8), Ok(*op));
        assert_eq!(op.index(), i);
    }
    assert_eq!(Op::try_from(4), Err(OpOutOfRange(4)));
    assert_eq!(Op::try_from(255), Err(OpOutOfRange(255)));
}

#[test]
fn op_nudge_clamps() {
    assert_eq!(Op::A.nudged(-1), Op::A);
    assert_eq!(Op::A.nudged(2), Op::C);
    assert_eq!(Op::C.nudged(127), Op::D);
    assert_eq!(Op::D.nudged(-128), Op::A);
}

#[test]
fn block_ref_all_has_no_duplicates() {
    for (i, b) in BlockRef::ALL.iter().enumerate() {
        assert!(!BlockRef::ALL[..i].contains(b), "{b:?} listed twice");
    }
}

/// `block()` hands out the instance whose spec table `BlockRef::specs` names.
#[test]
fn block_and_specs_agree() {
    let p = ParamSnapshot::default();
    for b in BlockRef::ALL {
        assert!(core::ptr::eq(p.block(b).specs(), b.specs()), "{b:?}");
    }
}

#[test]
fn block_mut_reaches_the_named_instance() {
    let mut p = ParamSnapshot::default();
    p.block_mut(BlockRef::FmOp(Op::C)).set(FmOpParams::LEVEL, 42.0);
    assert_eq!(p.fm.operators[2].level, 42.0);
    p.block_mut(BlockRef::FilterEnv).set(EnvParams::ATTACK, 2.0);
    assert_eq!(p.envelopes[1].attack, 2.0);
    assert_eq!(p.envelopes[0].attack, 0.01);
    p.block_mut(BlockRef::Out).set(OutParams::VOLUME, 0.25);
    assert_eq!(p.out.volume, 0.25);
    assert_eq!(p.block(BlockRef::Out).get(OutParams::VOLUME), 0.25);
}

/// Spec §4: exactly these are modulatable (read by `Voice` per block).
#[test]
fn modulatable_addresses_are_exactly_the_spec_list() {
    let mut want = vec![
        ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE),
        ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH),
        ParamAddr::new(BlockRef::Pizza, PizzaParams::LEVEL),
        ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
        ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
        ParamAddr::new(BlockRef::Drive, DriveParams::MIX),
        ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
        ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
        ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
        ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
        ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
        ParamAddr::new(BlockRef::Folder, FolderParams::MIX),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::DECAY),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::SUSTAIN),
        ParamAddr::new(BlockRef::AmpEnv, EnvParams::RELEASE),
        ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
    ];
    for op in Op::ALL {
        want.push(ParamAddr::new(BlockRef::FmOp(op), FmOpParams::LEVEL));
        want.push(ParamAddr::new(BlockRef::FmOp(op), FmOpParams::FEEDBACK));
    }
    let got: Vec<ParamAddr> = BlockRef::ALL
        .iter()
        .flat_map(|&b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|a| a.modulatable())
        .collect();
    assert_eq!(got.len(), want.len());
    for a in &want {
        assert!(got.contains(a), "{a:?} should be modulatable");
    }
}

#[test]
fn unknown_param_has_no_spec_and_is_not_modulatable() {
    let a = ParamAddr::new(BlockRef::Pizza, chimera_core::block::ParamId(99));
    assert!(a.spec().is_none());
    assert!(!a.modulatable());
}
```

In `chimera-core/tests/block_spec_test.rs`, add `use chimera_core::addr::BlockRef;`, replace `fn all_specs` (and its doc comment) with
```rust
/// Every block's spec table, via the one exhaustive `BlockRef` list.
fn all_specs() -> Vec<(String, &'static [ParamSpec])> {
    BlockRef::ALL.iter().map(|b| (format!("{b:?}"), b.specs())).collect()
}
```
and in `every_spec_table_is_well_formed` change `check(name, specs);` to `check(&name, specs);`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test addr_test --test block_spec_test 2>&1 | grep -E '^error\[' | head -3`
Expected: `unresolved import chimera_core::addr`.

- [ ] **Step 3: Create `chimera-core/src/addr.rs`**

```rust
//! Semantic parameter addresses (spec §2): an address names *what* a
//! parameter is, not where it sits on a page or in a chain, so rearranging
//! cells or reordering blocks never remaps a mod route.

use crate::block::{find_spec, ParamId, ParamSpec};

/// An FM operator. `TryFrom<u8>` rejects values above 3, so an out-of-range
/// operator (bad patch or SysEx data) is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    A,
    B,
    C,
    D,
}

/// Rejected operator index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpOutOfRange(pub u8);

impl Op {
    pub const ALL: [Op; 4] = [Op::A, Op::B, Op::C, Op::D];

    /// Index into `FmParams::operators`.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Step through the operators by encoder ticks, clamped at A and D.
    pub fn nudged(self, delta: i8) -> Op {
        Op::ALL[(self.index() as i16 + delta as i16).clamp(0, 3) as usize]
    }
}

impl TryFrom<u8> for Op {
    type Error = OpOutOfRange;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Op::ALL.get(v as usize).copied().ok_or(OpOutOfRange(v))
    }
}

/// One block instance of a Part voice. Multiple instances of one kind (two
/// filters) are sub-project 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockRef {
    Pizza,
    Modal,
    Fm,
    FmOp(Op),
    Drive,
    Filter,
    Folder,
    /// `envelopes[0]`
    AmpEnv,
    /// `envelopes[1]`
    FilterEnv,
    /// `envelopes[2]`
    AuxEnv,
    Lfo,
    /// `OutParams { volume, pan }`
    Out,
    /// Chorus, delay and reverb run outside `Voice` (desktop only).
    Chorus,
    Delay,
    Reverb,
}

impl BlockRef {
    pub const ALL: [BlockRef; 18] = [
        BlockRef::Pizza,
        BlockRef::Modal,
        BlockRef::Fm,
        BlockRef::FmOp(Op::A),
        BlockRef::FmOp(Op::B),
        BlockRef::FmOp(Op::C),
        BlockRef::FmOp(Op::D),
        BlockRef::Drive,
        BlockRef::Filter,
        BlockRef::Folder,
        BlockRef::AmpEnv,
        BlockRef::FilterEnv,
        BlockRef::AuxEnv,
        BlockRef::Lfo,
        BlockRef::Out,
        BlockRef::Chorus,
        BlockRef::Delay,
        BlockRef::Reverb,
    ];

    /// The block type's spec table (static; no instance needed).
    pub fn specs(self) -> &'static [ParamSpec] {
        match self {
            BlockRef::Pizza => &crate::dsp::pizza::PIZZA_SPECS,
            BlockRef::Modal => &crate::dsp::modal::MODAL_SPECS,
            BlockRef::Fm => &crate::params::FM_SPECS,
            BlockRef::FmOp(_) => &crate::params::FM_OP_SPECS,
            BlockRef::Drive => &crate::params::DRIVE_SPECS,
            BlockRef::Filter => &crate::params::FILTER_SPECS,
            BlockRef::Folder => &crate::params::FOLDER_SPECS,
            BlockRef::AmpEnv | BlockRef::FilterEnv | BlockRef::AuxEnv => &crate::params::ENV_SPECS,
            BlockRef::Lfo => &crate::dsp::lfo::LFO_SPECS,
            BlockRef::Out => &crate::params::OUT_SPECS,
            BlockRef::Chorus => &crate::dsp::chorus::CHORUS_SPECS,
            BlockRef::Delay => &crate::dsp::delay::DELAY_SPECS,
            BlockRef::Reverb => &crate::dsp::reverb::REVERB_SPECS,
        }
    }

    /// Whether `Voice::render` reads this block from its modulated copy.
    /// Filter/aux envelopes are never read; the LFO is read unmodulated; FX
    /// run outside `Voice`.
    pub const fn voice_reads(self) -> bool {
        match self {
            BlockRef::Pizza
            | BlockRef::Modal
            | BlockRef::Fm
            | BlockRef::FmOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::AmpEnv
            | BlockRef::Out => true,
            BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb => false,
        }
    }
}

/// A parameter of a block instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamAddr {
    pub block: BlockRef,
    pub param: ParamId,
}

impl ParamAddr {
    pub const fn new(block: BlockRef, param: ParamId) -> Self {
        Self { block, param }
    }

    pub fn spec(self) -> Option<&'static ParamSpec> {
        find_spec(self.block.specs(), self.param)
    }

    /// Modulation may target this address: the spec says the value is read
    /// per block *and* `Voice` reads this block instance (plan D7).
    pub fn modulatable(self) -> bool {
        self.block.voice_reads() && self.spec().is_some_and(|s| s.modulatable)
    }
}
```

In `chimera-core/src/lib.rs` add `pub mod addr;` above `pub mod block;`.

- [ ] **Step 4: Add the dispatch to `ParamSnapshot`**

In `chimera-core/src/params.rs`, add `use crate::addr::BlockRef;` at the top and insert above `impl Default for ParamSnapshot`:
```rust
impl ParamSnapshot {
    /// The one exhaustive dispatch from a block address to its values
    /// (spec §2). UI and modulation go through this; DSP reads fields.
    pub fn block(&self, b: BlockRef) -> &dyn Block {
        match b {
            BlockRef::Pizza => &self.pizza,
            BlockRef::Modal => &self.modal,
            BlockRef::Fm => &self.fm,
            BlockRef::FmOp(op) => &self.fm.operators[op.index()],
            BlockRef::Drive => &self.drive,
            BlockRef::Filter => &self.filter,
            BlockRef::Folder => &self.folder,
            BlockRef::AmpEnv => &self.envelopes[0],
            BlockRef::FilterEnv => &self.envelopes[1],
            BlockRef::AuxEnv => &self.envelopes[2],
            BlockRef::Lfo => &self.lfo,
            BlockRef::Out => &self.out,
            BlockRef::Chorus => &self.chorus,
            BlockRef::Delay => &self.delay,
            BlockRef::Reverb => &self.reverb,
        }
    }

    pub fn block_mut(&mut self, b: BlockRef) -> &mut dyn Block {
        match b {
            BlockRef::Pizza => &mut self.pizza,
            BlockRef::Modal => &mut self.modal,
            BlockRef::Fm => &mut self.fm,
            BlockRef::FmOp(op) => &mut self.fm.operators[op.index()],
            BlockRef::Drive => &mut self.drive,
            BlockRef::Filter => &mut self.filter,
            BlockRef::Folder => &mut self.folder,
            BlockRef::AmpEnv => &mut self.envelopes[0],
            BlockRef::FilterEnv => &mut self.envelopes[1],
            BlockRef::AuxEnv => &mut self.envelopes[2],
            BlockRef::Lfo => &mut self.lfo,
            BlockRef::Out => &mut self.out,
            BlockRef::Chorus => &mut self.chorus,
            BlockRef::Delay => &mut self.delay,
            BlockRef::Reverb => &mut self.reverb,
        }
    }
}
```

- [ ] **Step 5: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test addr_test --test block_spec_test --test golden_test 2>&1 | grep '^test result'`
Expected: `7 passed`, `1 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/addr.rs chimera-core/src/lib.rs chimera-core/src/params.rs chimera-core/tests/addr_test.rs chimera-core/tests/block_spec_test.rs
git commit -m "feat(core): semantic ParamAddr/BlockRef/Op and ParamSnapshot::block

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 15: One page table — `PageId::binding(idx) -> Option<ParamAddr>`

Collapses the two per-page tables (`read_values` arms and `resolve_mut` arms) into one table of semantic addresses. Encoders, snap and display become three generic lines each. Behavior is unchanged (all `page_block_test` parity tests keep passing).

**Files:**
- Modify: `chimera-core/src/ui/page.rs` (imports; replace everything from `/// Read 6 normalized` to end of file)
- Modify: `chimera-core/src/ui/mod.rs:119,142`, `chimera-core/src/ui/renderer.rs:75` (`fm_selected_op()` → `selected_op()`)
- Test: `chimera-core/tests/page_block_test.rs`

**Interfaces:**
- Consumes: Task 14 (`ParamAddr`, `BlockRef`, `Op`, `ParamSnapshot::block(_mut)`).
- Produces: `PageId::binding(&self, idx: usize) -> Option<ParamAddr>`; `page::selected_op() -> Op` (replaces `fm_selected_op() -> usize`; the static stays until Task 20). Deleted: `resolve_mut`, `bind`, `read_block`, `fm_selected_op`, `fm_set_selected_op`.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/page_block_test.rs`:
```rust
// ── Bindings (Task 15) ───────────────────────────────────────────────

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::params::{EnvParams, FmOpParams, FmParams, OutParams};

#[test]
fn page_bindings_name_semantic_addresses() {
    assert_eq!(
        PageId::FmRatio.binding(3),
        Some(ParamAddr::new(BlockRef::FmOp(Op::D), FmOpParams::COARSE))
    );
    assert_eq!(PageId::FmAlg.binding(0), Some(ParamAddr::new(BlockRef::Fm, FmParams::ALGORITHM)));
    assert_eq!(PageId::FmAlg.binding(2), Some(ParamAddr::new(BlockRef::Out, OutParams::VOLUME)));
    assert_eq!(PageId::FmAlg.binding(1), None);
    assert_eq!(PageId::FmOp.binding(0), None); // operator selector
    // Spec §5: Demo pages address envelopes[1] via FilterEnv.
    assert_eq!(
        PageId::DemoMotion.binding(4),
        Some(ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK))
    );
    assert_eq!(PageId::DemoMatrix.binding(0), None);
    assert_eq!(PageId::Pizza.binding(3), None);
}

/// Every bound slot of every page resolves to a spec.
#[test]
fn every_page_binding_has_a_spec() {
    let pages = [
        PageId::Pizza, PageId::EngineModal1, PageId::EngineModal2, PageId::Drive, PageId::Filter,
        PageId::Folder, PageId::Vca, PageId::Efx, PageId::Mixer, PageId::Chorus, PageId::Delay,
        PageId::MixReverb, PageId::Master, PageId::EnvAmp, PageId::EnvFilter, PageId::EnvAux,
        PageId::Lfo, PageId::FmAlg, PageId::FmOp, PageId::FmRatio, PageId::FmEnv1, PageId::FmEnv2,
        PageId::FmEnv3, PageId::FmEnv4, PageId::DemoWaves, PageId::DemoShapes, PageId::DemoMotion,
        PageId::DemoFm, PageId::DemoMatrix,
    ];
    for page in pages {
        for i in 0..6 {
            if let Some(a) = page.binding(i) {
                assert!(a.spec().is_some(), "{page:?} slot {i}: {a:?} has no spec");
            }
        }
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test page_block_test 2>&1 | grep -E '^error\[' | head -3`
Expected: `no method named binding found for enum PageId`.

- [ ] **Step 3: Replace the page tables**

In `chimera-core/src/ui/page.rs`, change `use crate::block::{Block, ParamId};` to
```rust
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
```
and replace everything from the line `    /// Read 6 normalized (0..1) encoder values from params for this page.` to the end of the file with:
```rust
    /// The parameter bound to encoder `idx` on this page, if any.
    /// FM operator pages resolve to the currently selected operator.
    pub fn binding(&self, idx: usize) -> Option<ParamAddr> {
        use BlockRef as B;
        let at = |block: BlockRef, ids: &[ParamId]| ids.get(idx).map(|&param| ParamAddr::new(block, param));
        match self {
            PageId::Pizza => at(B::Pizza, &PIZZA_PAGE),
            PageId::EngineModal1 => at(B::Modal, &MODAL1_PAGE),
            PageId::EngineModal2 => at(B::Modal, &MODAL2_PAGE),
            PageId::Drive => at(B::Drive, &DRIVE_PAGE),
            PageId::Filter => at(B::Filter, &FILTER_PAGE),
            PageId::Folder => at(B::Folder, &FOLDER_PAGE),
            PageId::EnvAmp | PageId::Vca => at(B::AmpEnv, &ENV_PAGE),
            PageId::EnvFilter => at(B::FilterEnv, &ENV_PAGE),
            PageId::EnvAux => at(B::AuxEnv, &ENV_PAGE),
            PageId::Lfo => at(B::Lfo, &LFO_PAGE),
            PageId::Mixer | PageId::Master => at(B::Out, &OUT_PAGE),
            PageId::Chorus => at(B::Chorus, &CHORUS_PAGE),
            PageId::Delay => at(B::Delay, &DELAY_PAGE),
            PageId::Efx | PageId::MixReverb => at(B::Reverb, &REVERB_PAGE),
            PageId::FmAlg => match idx {
                0 => Some(ParamAddr::new(B::Fm, FmParams::ALGORITHM)),
                2 => Some(ParamAddr::new(B::Out, OutParams::VOLUME)),
                _ => None,
            },
            // Slot 0 selects the operator (see `apply_encoder`).
            PageId::FmOp => {
                let id = *FM_OP_PAGE.get(idx.checked_sub(1)?)?;
                Some(ParamAddr::new(B::FmOp(selected_op()), id))
            }
            PageId::FmRatio => match idx {
                0..=3 => Some(ParamAddr::new(B::FmOp(Op::ALL[idx]), FmOpParams::COARSE)),
                4 => Some(ParamAddr::new(B::FmOp(selected_op()), FmOpParams::FINE)),
                _ => None,
            },
            PageId::FmEnv1 => at(B::FmOp(Op::A), &FM_ENV_PAGE),
            PageId::FmEnv2 => at(B::FmOp(Op::B), &FM_ENV_PAGE),
            PageId::FmEnv3 => at(B::FmOp(Op::C), &FM_ENV_PAGE),
            PageId::FmEnv4 => at(B::FmOp(Op::D), &FM_ENV_PAGE),
            PageId::DemoWaves => DEMO_WAVES.get(idx).copied(),
            PageId::DemoShapes => DEMO_SHAPES.get(idx).copied(),
            PageId::DemoMotion => DEMO_MOTION.get(idx).copied(),
            PageId::DemoFm => DEMO_FM.get(idx).copied(),
            PageId::DemoMatrix => None,
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        core::array::from_fn(|i| match (self, i) {
            (PageId::FmOp, 0) => selected_op().index() as f32 / 3.0,
            // Mixer bars for the unbound VOICES and PITCH slots.
            (PageId::Mixer, 2 | 4) => 0.5,
            _ => self.binding(i).map_or(0.0, |a| params.block(a.block).normalized(a.param)),
        })
    }

    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if *self == PageId::FmOp && idx == 0 {
            set_selected_op(selected_op().nudged(delta));
            return;
        }
        if let Some(a) = self.binding(idx) {
            params.block_mut(a.block).nudge(a.param, delta);
        }
    }

    /// Shift+encoder: snap to the coarse points of the bound param's format.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if let Some(a) = self.binding(idx) {
            params.block_mut(a.block).snap(a.param, delta);
        }
    }
}

/// Encoder slot → param id, for pages bound to a single block.
const PIZZA_PAGE: [ParamId; 3] = [PizzaParams::SHAPE, PizzaParams::CRUSH, PizzaParams::LEVEL];
const MODAL1_PAGE: [ParamId; 6] = [
    ModalParams::MODE,
    ModalParams::EXCITE,
    ModalParams::DECAY,
    ModalParams::BRIGHTNESS,
    ModalParams::POSITION,
    ModalParams::INHARM,
];
const MODAL2_PAGE: [ParamId; 6] = [
    ModalParams::KS_BODY,
    ModalParams::KS_STIFFNESS,
    ModalParams::KS_FEEDBACK,
    ModalParams::KS_ENS_DEPTH,
    ModalParams::KS_ENS_RATE,
    ModalParams::KS_ENS_MIX,
];
const DRIVE_PAGE: [ParamId; 3] = [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX];
const FILTER_PAGE: [ParamId; 6] = [
    FilterParams::CUTOFF,
    FilterParams::RESONANCE,
    FilterParams::DRIVE,
    FilterParams::FM_AMOUNT,
    FilterParams::ENV_AMOUNT,
    FilterParams::KEY_TRACK,
];
const FOLDER_PAGE: [ParamId; 3] = [FolderParams::FOLD, FolderParams::SYMMETRY, FolderParams::MIX];
const ENV_PAGE: [ParamId; 6] = [
    EnvParams::ATTACK,
    EnvParams::DECAY,
    EnvParams::SUSTAIN,
    EnvParams::RELEASE,
    EnvParams::LEVEL,
    EnvParams::VEL_SENS,
];
const LFO_PAGE: [ParamId; 6] = [
    LfoParams::RATE,
    LfoParams::SHAPE,
    LfoParams::SYNC,
    LfoParams::PHASE,
    LfoParams::DEPTH,
    LfoParams::OFFSET,
];
/// FM_OP slots 1..=5 (slot 0 selects the operator).
const FM_OP_PAGE: [ParamId; 5] = [
    FmOpParams::WAVEFORM,
    FmOpParams::LEVEL,
    FmOpParams::FEEDBACK,
    FmOpParams::DETUNE,
    FmOpParams::VELOCITY_SENS,
];
const FM_ENV_PAGE: [ParamId; 6] = [
    FmOpParams::ATTACK_RATE,
    FmOpParams::DECAY1_RATE,
    FmOpParams::DECAY1_LEVEL,
    FmOpParams::DECAY2_RATE,
    FmOpParams::RELEASE_RATE,
    FmOpParams::RATE_SCALING,
];
const OUT_PAGE: [ParamId; 2] = [OutParams::VOLUME, OutParams::PAN];
const CHORUS_PAGE: [ParamId; 4] = [
    ChorusParams::MODE,
    ChorusParams::RATE,
    ChorusParams::DEPTH,
    ChorusParams::MIX,
];
const DELAY_PAGE: [ParamId; 6] = [
    DelayParams::TIME_MS,
    DelayParams::FEEDBACK,
    DelayParams::WOW_FLUTTER,
    DelayParams::SATURATION,
    DelayParams::TONE,
    DelayParams::MIX,
];
const REVERB_PAGE: [ParamId; 5] = [
    ReverbParams::REVERB_TYPE,
    ReverbParams::TIME,
    ReverbParams::DAMPING,
    ReverbParams::SIZE,
    ReverbParams::MIX,
];

/// Demo pages borrow params from several blocks (spec §5: `envelopes[1]` is
/// addressed as `FilterEnv`).
const DEMO_WAVES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
    ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
    ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
    ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
    ParamAddr::new(BlockRef::Filter, FilterParams::ENV_AMOUNT),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
];
const DEMO_SHAPES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
    ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
    ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
    ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
    ParamAddr::new(BlockRef::Filter, FilterParams::FM_AMOUNT),
];
const DEMO_MOTION: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::DECAY),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::SUSTAIN),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::RELEASE),
    ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK),
    ParamAddr::new(BlockRef::FilterEnv, EnvParams::DECAY),
];
const DEMO_FM: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Fm, FmParams::ALGORITHM),
    ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK),
];

// ---------------------------------------------------------------------------
// FM operator selection state (module-level, simple static; Task 20 moves it
// into `UiState`)
// ---------------------------------------------------------------------------

/// Currently selected FM operator (index 0-3).
/// This is UI-only state shared between FM_OP and FM_RATIO pages.
static FM_SEL_OP: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The selected FM operator.
pub fn selected_op() -> Op {
    Op::try_from(FM_SEL_OP.load(core::sync::atomic::Ordering::Relaxed)).unwrap_or(Op::A)
}

fn set_selected_op(op: Op) {
    FM_SEL_OP.store(op.index() as u8, core::sync::atomic::Ordering::Relaxed);
}
```

In `chimera-core/src/ui/mod.rs` (two places) and `chimera-core/src/ui/renderer.rs` (one place): `page::fm_selected_op() as u8` → `page::selected_op().index() as u8` (renderer: `crate::ui::page::fm_selected_op() as u8` → `crate::ui::page::selected_op().index() as u8`).

- [ ] **Step 4: Run the tests, goldens and suite**

Run: `cargo test -p chimera-core --test page_block_test --test golden_test 2>&1 | grep '^test result'`
Expected: `27 passed`, `4 passed`. Full suite: `0 failed`. `grep -n 'fn resolve_mut\|fn read_block\|fm_selected_op' -r chimera-core/src` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/page.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/renderer.rs chimera-core/tests/page_block_test.rs
git commit -m "refactor(ui): pages bind slots to ParamAddr through one table

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 16: `ModState` stores `ParamAddr`; generic modulation in `Voice`; registry refuses non-modulatable

**Files:**
- Replace: `chimera-core/src/modulation.rs` (full content below)
- Modify: `chimera-core/src/mod_path.rs` (private registry fields, `len`/`is_empty`, `RegistryError`, checked `add`, bridge `legacy_to_addr`)
- Modify: `chimera-core/src/ui/mod_grid.rs:104` (`registry.count` → `registry.len()`)
- Modify: `chimera-core/src/preset.rs` (`ChainType::ALL`; FM pre-wire through the registry)
- Modify: `chimera-core/src/dsp/voice.rs` (generic modulation loop)
- Modify: `chimera-core/src/ui/mod.rs` (`sync_mod_state`, checked priming, display offsets by `PageId::binding`)
- Replace: `chimera-core/tests/modulation_test.rs`, `chimera-core/tests/mod_registry_test.rs`
- Modify: `chimera-core/tests/modulation_integration_test.rs`, `chimera-core/tests/common/mod.rs`
- Create: `chimera-core/tests/modulatable_test.rs`

**Interfaces:**
- Consumes: Task 14 (`ParamAddr::modulatable`, `ParamSnapshot::block_mut`), Task 15 (`PageId::binding`), Task 2 (`apply_offset`).
- Produces:
  - `ModState` fields private; `ModState::new() -> Self` (const), `num_sources(&self) -> usize`, `num_dests(&self) -> usize`, `dest(&self, d: usize) -> ParamAddr`, `amount(&self, source: usize, dest: usize) -> i8`, `sum_for(&self, d: usize, source_values: &[f32; MAX_MOD_SOURCES]) -> f32`, `offset_for(&self, addr: ParamAddr, source_values: &[f32; MAX_MOD_SOURCES]) -> f32`, `from_registry(registry: &ModDestRegistry, chain: ChainType, num_sources: usize) -> Self`, `set_amount(&mut self, source: usize, dest: usize, amount: i8)`, `sync_from_matrix(&mut self, matrix: &MatrixState, chain: ChainType)`. `compute_offset` is deleted.
  - `ModDestRegistry` fields private; `len(&self) -> usize`, `is_empty(&self) -> bool`, `add(&mut self, chain: ChainType, path: ParamPath, label: [u8; LABEL_LEN]) -> Result<(), RegistryError>`; `pub enum RegistryError { NotModulatable, Full }`
  - Bridge (deleted in Task 19): `pub fn legacy_to_addr(chain: ChainType, path: ParamPath) -> Option<ParamAddr>` (plan D8)
  - `ChainType::ALL: [ChainType; 3]`
  - `UiState::sync_mod_state(&mut self, track: usize)` (private)
  - Harness: `common::lfo_route((ChainType, ParamPath)) -> ModState`; consts `PIZZA_CUTOFF`, `FM_CUTOFF`, `MODAL_CUTOFF`, `OP_A_LEVEL: (ChainType, ParamPath)`

- [ ] **Step 1: Write the failing tests**

Replace `chimera-core/tests/modulation_test.rs` with:
```rust
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::mod_path::{ModDestRegistry, ParamPath};
use chimera_core::modulation::{ModState, MAX_MOD_DESTS, MAX_MOD_SOURCES};
use chimera_core::params::{DriveParams, FilterParams, FolderParams};
use chimera_core::preset::ChainType;
use chimera_core::ui::mod_grid::MatrixState;

const PIZZA: ChainType = ChainType::PizzaPoly;

/// A ModState with one dest (`path` on the Pizza chain) and `n` sources.
fn one_dest(path: ParamPath, n: usize) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(PIZZA, path, *b"TEST\0\0\0\0").unwrap();
    ModState::from_registry(&reg, PIZZA, n)
}

#[test]
fn mod_state_default_is_empty() {
    let ms = ModState::new();
    assert_eq!(ms.num_sources(), 0);
    assert_eq!(ms.num_dests(), 0);
    for si in 0..MAX_MOD_SOURCES {
        for di in 0..MAX_MOD_DESTS {
            assert_eq!(ms.amount(si, di), 0);
        }
    }
}

#[test]
fn mod_state_offset_no_routes() {
    let ms = ModState::new();
    let sources = [0.0f32; MAX_MOD_SOURCES];
    let addr = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);
    assert_eq!(ms.offset_for(addr, &sources), 0.0);
}

#[test]
fn mod_state_offset_single_route() {
    let mut ms = one_dest(ParamPath::Block { block: 2, param: 0 }, 1); // filter cutoff
    ms.set_amount(0, 0, 64);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF));

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.offset_for(ms.dest(0), &sources);
    let expected = 1.0 * (64.0 / 127.0);
    assert!((offset - expected).abs() < 1e-5, "expected {expected}, got {offset}");
    assert_eq!(ms.sum_for(0, &sources), offset);
}

#[test]
fn mod_state_offset_multiple_sources() {
    let mut ms = one_dest(ParamPath::Block { block: 1, param: 0 }, 2); // drive amount
    ms.set_amount(0, 0, 50);
    ms.set_amount(1, 0, 100);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE));

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 0.5;
    sources[1] = -0.8;
    let offset = ms.sum_for(0, &sources);
    let expected = 0.5 * (50.0 / 127.0) + (-0.8) * (100.0 / 127.0);
    assert!((offset - expected).abs() < 1e-5, "expected {expected}, got {offset}");
}

#[test]
fn mod_state_offset_negative_amount() {
    let mut ms = one_dest(ParamPath::Block { block: 3, param: 1 }, 1); // folder symmetry
    ms.set_amount(0, 0, -80);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY));

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.sum_for(0, &sources);
    assert!((offset - (-80.0 / 127.0)).abs() < 1e-5);
    assert!(offset < 0.0);
}

#[test]
fn mod_state_set_amount_ignores_out_of_range() {
    let mut ms = one_dest(ParamPath::Block { block: 0, param: 0 }, 2);
    ms.set_amount(5, 0, 99); // no such source
    ms.set_amount(0, 3, 99); // no such dest
    assert_eq!(ms.amount(5, 0), 0);
    assert_eq!(ms.amount(0, 3), 0);
}

#[test]
fn mod_state_sync_from_matrix() {
    let mut matrix = MatrixState::new();
    let mut registry = ModDestRegistry::new();
    registry.add(PIZZA, ParamPath::Block { block: 0, param: 1 }, *b"A  p\0\0\0\0").unwrap();
    registry.add(PIZZA, ParamPath::Block { block: 2, param: 0 }, *b"B  q\0\0\0\0").unwrap();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 2;
    matrix.amounts[0][0] = 42;
    matrix.amounts[1][1] = -99;

    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix, PIZZA);

    assert_eq!(ms.num_sources(), 2);
    assert_eq!(ms.num_dests(), 2);
    assert_eq!(ms.amount(0, 0), 42);
    assert_eq!(ms.amount(1, 1), -99);
    assert_eq!(ms.dest(0), ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH));
    assert_eq!(ms.dest(1), ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF));
}

/// Review Focus 2: more destinations than fit keep amounts aligned.
#[test]
fn mod_state_truncates_without_misaligning() {
    let mut registry = ModDestRegistry::new();
    // 16 modulatable Block paths on the Pizza chain + 4 FM op levels = 20.
    for (block, params) in [(0u8, 0..3u8), (1, 0..3), (2, 0..3), (3, 0..3), (4, 0..4)] {
        for param in params {
            registry.add(PIZZA, ParamPath::Block { block, param }, *b"X\0\0\0\0\0\0\0").unwrap();
        }
    }
    for op in 0..4u8 {
        registry.add(PIZZA, ParamPath::FmOp { op, param: 2 }, *b"X\0\0\0\0\0\0\0").unwrap();
    }
    assert_eq!(registry.len(), 20);
    assert_eq!(ModState::from_registry(&registry, PIZZA, 2).num_dests(), MAX_MOD_DESTS);

    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry); // matrix keeps 16 too
    matrix.num_sources = 2;
    matrix.amounts[1][15] = 77;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix, PIZZA);
    assert_eq!(ms.num_dests(), MAX_MOD_DESTS);
    assert_eq!(ms.amount(1, 15), 77);
}

/// Review Focus 2: a matrix with more than MAX_MOD_SOURCES rows is clamped
/// (the old code indexed `amounts[si]` past 8 in the audio thread).
#[test]
fn mod_state_clamps_sources() {
    let mut registry = ModDestRegistry::new();
    registry.add(PIZZA, ParamPath::Block { block: 2, param: 0 }, *b"X\0\0\0\0\0\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 16;
    matrix.amounts[12][0] = 50;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix, PIZZA);
    assert_eq!(ms.num_sources(), MAX_MOD_SOURCES);
    let values = [1.0f32; MAX_MOD_SOURCES];
    assert_eq!(ms.sum_for(0, &values), 0.0); // row 12 was dropped, nothing panicked
}
```

Replace `chimera-core/tests/mod_registry_test.rs` with (the old `registry_max_capacity` test cannot exist any more: only 25 addresses are modulatable, fewer than the 32 slots):
```rust
use chimera_core::addr::ParamAddr;
use chimera_core::mod_path::{legacy_to_addr, ModDestRegistry, ParamPath, RegistryError};
use chimera_core::preset::ChainType;

const PIZZA: ChainType = ChainType::PizzaPoly;

#[test]
fn registry_starts_empty() {
    let reg = ModDestRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
}

#[test]
fn registry_add_and_find() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::Block { block: 1, param: 0 };
    assert_eq!(reg.add(PIZZA, path, *b"DrvDrv\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
    assert!(reg.is_primed(path));
    assert_eq!(reg.find(path), Some(0));
}

#[test]
fn registry_no_duplicates() {
    let mut reg = ModDestRegistry::new();
    let path = ParamPath::FmOp { op: 0, param: 2 };
    assert_eq!(reg.add(ChainType::Fm, path, *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.add(ChainType::Fm, path, *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_remove() {
    let mut reg = ModDestRegistry::new();
    let p1 = ParamPath::FmOp { op: 0, param: 2 };
    let p2 = ParamPath::FmOp { op: 1, param: 2 };
    reg.add(ChainType::Fm, p1, *b"O1 Lvl\0\0").unwrap();
    reg.add(ChainType::Fm, p2, *b"O2 Lvl\0\0").unwrap();
    assert_eq!(reg.len(), 2);
    reg.remove(p1);
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_primed(p1));
    assert!(reg.is_primed(p2));
}

#[test]
fn registry_fm_op_paths_are_distinct() {
    let p0 = ParamPath::FmOp { op: 0, param: 2 };
    let p1 = ParamPath::FmOp { op: 1, param: 2 };
    assert_ne!(p0, p1);
    let mut reg = ModDestRegistry::new();
    reg.add(ChainType::Fm, p0, *b"O1 Lvl\0\0").unwrap();
    assert!(reg.is_primed(p0));
    assert!(!reg.is_primed(p1));
}

/// Spec § Testing "Registry": adding a non-modulatable address is refused.
#[test]
fn registry_refuses_non_modulatable() {
    let mut reg = ModDestRegistry::new();
    let refused = [
        (ChainType::Modal, ParamPath::Block { block: 0, param: 1 }), // Modal EXCITE (note-on only)
        (ChainType::Fm, ParamPath::Block { block: 0, param: 0 }),    // FM ALG (Enum)
        (ChainType::Fm, ParamPath::FmOp { op: 0, param: 1 }),        // op waveform (Enum)
        (ChainType::Fm, ParamPath::FmEnv { op: 0, param: 0 }),       // op AR (note-on only)
        (PIZZA, ParamPath::Block { block: 2, param: 3 }),            // filter FM amount (never read)
        (PIZZA, ParamPath::Block { block: 9, param: 0 }),            // no such node
        (PIZZA, ParamPath::FmOp { op: 7, param: 2 }),                // no such operator
    ];
    for (chain, path) in refused {
        assert_eq!(reg.add(chain, path, *b"X\0\0\0\0\0\0\0"), Err(RegistryError::NotModulatable), "{path:?}");
    }
    assert!(reg.is_empty());
}

/// Every path the UI can emit is accepted exactly when its address is modulatable.
#[test]
fn registry_accepts_exactly_the_modulatable_paths() {
    for chain in ChainType::ALL {
        let mut reg = ModDestRegistry::new();
        let mut accepted = 0;
        let blocks = (0..6u8).flat_map(|block| (0..6u8).map(move |param| ParamPath::Block { block, param }));
        let ops = (0..4u8).flat_map(|op| (0..6u8).map(move |param| ParamPath::FmOp { op, param }));
        for path in blocks.chain(ops) {
            let ok = legacy_to_addr(chain, path).is_some_and(ParamAddr::modulatable);
            assert_eq!(reg.add(chain, path, *b"X\0\0\0\0\0\0\0").is_ok(), ok, "{chain:?} {path:?}");
            accepted += ok as usize;
        }
        assert_eq!(reg.len(), accepted);
    }
}
```

Create `chimera-core/tests/modulatable_test.rs`:
```rust
//! Spec § Testing "Modulatable is true": for every address whose spec says
//! `modulatable: true`, an LFO route changes the rendered output. Keeps the
//! flag from lying.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::{legacy_to_addr, ModDestRegistry, ParamPath};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::ChainType;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// A base patch in which `block` is audible (spec: engine params use their
/// own engine; drive/folder > 0; amp env and FM use Pizza or FM).
fn recipe(block: BlockRef) -> ParamSnapshot {
    let mut p = ParamSnapshot::default();
    p.lfo.rate = 5.0; // swings both ways within the render
    match block {
        BlockRef::Fm | BlockRef::FmOp(_) => {
            p.engine = EngineType::Fm;
            p.fm.algorithm = 7; // every operator is a carrier
            for op in p.fm.operators.iter_mut() {
                op.level = 99.0;
            }
        }
        BlockRef::Modal => p.engine = EngineType::Modal,
        BlockRef::Drive => p.drive.drive = 0.5,
        BlockRef::Filter => p.filter.cutoff = 2000.0,
        BlockRef::Folder => p.folder.fold = 0.5,
        _ => {} // Pizza engine defaults: Pizza, AmpEnv, Out
    }
    p
}

/// A UI path that reaches `addr` (every modulatable address must have one).
fn ui_path_for(addr: ParamAddr) -> (ChainType, ParamPath) {
    for chain in ChainType::ALL {
        let blocks = (0..6u8).flat_map(|block| (0..6u8).map(move |param| ParamPath::Block { block, param }));
        let ops = (0..4u8).flat_map(|op| (0..6u8).map(move |param| ParamPath::FmOp { op, param }));
        if let Some(path) = blocks.chain(ops).find(|&p| legacy_to_addr(chain, p) == Some(addr)) {
            return (chain, path);
        }
    }
    panic!("no UI path reaches {addr:?}");
}

fn lfo_route(addr: ParamAddr) -> ModState {
    let (chain, path) = ui_path_for(addr);
    let mut reg = ModDestRegistry::new();
    reg.add(chain, path, *b"TEST\0\0\0\0").expect("modulatable");
    let mut ms = ModState::from_registry(&reg, chain, 2);
    ms.set_amount(1, 0, 127);
    ms
}

fn render(params: &ParamSnapshot, mod_state: &ModState) -> Vec<f32> {
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, params);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..200 {
        if b == 100 {
            voice.note_off();
        }
        voice.render(&mut block, params, mod_state);
        out.extend_from_slice(&block);
    }
    out
}

#[test]
fn every_modulatable_param_audibly_changes_output() {
    let mut checked = 0;
    for block in BlockRef::ALL {
        for spec in block.specs() {
            let addr = ParamAddr::new(block, spec.id);
            if !addr.modulatable() {
                continue;
            }
            let base = recipe(block);
            let dry = render(&base, &ModState::new());
            let wet = render(&base, &lfo_route(addr));
            let diff = dry.iter().zip(&wet).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(diff > 1e-4, "{block:?}.{}: LFO route changes nothing (max diff {diff})", spec.label);
            checked += 1;
        }
    }
    assert_eq!(checked, 25);
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test modulation_test --test mod_registry_test --test modulatable_test 2>&1 | grep -E '^error\[' | sort | uniq -c | head`
Expected: `no function or associated item named from_registry`, `unresolved import chimera_core::mod_path::legacy_to_addr`, `RegistryError`, `no method named len`, `this method takes 2 arguments but 3 arguments were supplied`.

- [ ] **Step 3: Replace `chimera-core/src/modulation.rs`**

```rust
//! Compact modulation state shared between UI and audio thread.
//!
//! Built only from the destination registry (`from_registry`) or the UI's
//! matrix (`sync_from_matrix`), both of which admit only modulatable
//! addresses (spec §4). The audio ISR reads routes + source values to compute
//! per-destination offsets.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::pizza::PizzaParams;
use crate::mod_path::{legacy_to_addr, ModDestRegistry};
use crate::preset::ChainType;
use crate::ui::mod_grid::MatrixState;

pub const MAX_MOD_SOURCES: usize = 8;
pub const MAX_MOD_DESTS: usize = 16;

/// Fills unused dest slots; never read (only `d < num_dests` is).
const UNUSED: ParamAddr = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);

/// Compact modulation state shared between UI and audio thread.
#[derive(Clone, Debug)]
pub struct ModState {
    num_sources: usize,
    num_dests: usize,
    dests: [ParamAddr; MAX_MOD_DESTS],
    /// amounts[source][dest], -127 to +127
    amounts: [[i8; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
}

impl ModState {
    /// No sources, no destinations.
    pub const fn new() -> Self {
        Self {
            num_sources: 0,
            num_dests: 0,
            dests: [UNUSED; MAX_MOD_DESTS],
            amounts: [[0; MAX_MOD_DESTS]; MAX_MOD_SOURCES],
        }
    }

    pub fn num_sources(&self) -> usize {
        self.num_sources
    }

    pub fn num_dests(&self) -> usize {
        self.num_dests
    }

    /// Destination `d` (`d < num_dests()`).
    pub fn dest(&self, d: usize) -> ParamAddr {
        self.dests[d]
    }

    /// Amount from `source` to destination `dest`; 0 when out of range.
    pub fn amount(&self, source: usize, dest: usize) -> i8 {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest]
        } else {
            0
        }
    }

    /// Sum of `source_value * amount / 127` over all sources for dest `d`.
    /// Same order and arithmetic as the old `compute_offset`.
    pub fn sum_for(&self, d: usize, source_values: &[f32; MAX_MOD_SOURCES]) -> f32 {
        let mut total = 0.0f32;
        for si in 0..self.num_sources {
            let amt = self.amounts[si][d];
            if amt != 0 {
                total += source_values[si] * (amt as f32 / 127.0);
            }
        }
        total
    }

    /// Offset for `addr`, or 0.0 if it is not a destination (UI display).
    pub fn offset_for(&self, addr: ParamAddr, source_values: &[f32; MAX_MOD_SOURCES]) -> f32 {
        (0..self.num_dests)
            .find(|&d| self.dests[d] == addr)
            .map_or(0.0, |d| self.sum_for(d, source_values))
    }

    /// Destinations from the registry (in order, at most `MAX_MOD_DESTS`),
    /// all amounts zero. `num_sources` is clamped to `MAX_MOD_SOURCES`.
    pub fn from_registry(registry: &ModDestRegistry, chain: ChainType, num_sources: usize) -> Self {
        let mut ms = Self::new();
        ms.num_sources = num_sources.min(MAX_MOD_SOURCES);
        for i in 0..registry.len() {
            let Some(entry) = registry.get(i) else { continue };
            ms.push_dest(legacy_to_addr(chain, entry.path));
        }
        ms
    }

    /// Set one amount. Ignored when `source` or `dest` is out of range.
    pub fn set_amount(&mut self, source: usize, dest: usize, amount: i8) {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest] = amount;
        }
    }

    /// Copy routing from the UI's matrix. Keeps only modulatable
    /// destinations (at most `MAX_MOD_DESTS`, amounts moved with their dest)
    /// and at most `MAX_MOD_SOURCES` sources.
    pub fn sync_from_matrix(&mut self, matrix: &MatrixState, chain: ChainType) {
        *self = Self::new();
        self.num_sources = matrix.num_sources.min(MAX_MOD_SOURCES);
        for di in 0..matrix.num_dests {
            let addr = matrix.dests[di].as_ref().and_then(|d| legacy_to_addr(chain, d.path));
            if self.push_dest(addr) {
                let d = self.num_dests - 1;
                for si in 0..self.num_sources {
                    self.amounts[si][d] = matrix.amounts[si][di];
                }
            }
        }
    }

    /// Append `addr` if it is modulatable and there is room.
    fn push_dest(&mut self, addr: Option<ParamAddr>) -> bool {
        match addr {
            Some(a) if a.modulatable() && self.num_dests < MAX_MOD_DESTS => {
                self.dests[self.num_dests] = a;
                self.num_dests += 1;
                true
            }
            _ => false,
        }
    }
}

impl Default for ModState {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Registry check and bridge in `chimera-core/src/mod_path.rs`**

1. At the top add
```rust
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::preset::ChainType;
use crate::ui::page::PageId;
```
2. Above `#[derive(Clone)] pub struct ModDestRegistry` add
```rust
/// Why `ModDestRegistry::add` refused a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The address's spec is not modulatable (or the path means nothing).
    NotModulatable,
    Full,
}
```
and make its fields private (`entries: …`, `count: usize` without `pub`).
3. Replace `pub fn add(&mut self, path: ParamPath, label: [u8; LABEL_LEN]) { … }` with
```rust
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Prime `path` (as the UI on `chain` means it) as a mod destination.
    /// Refuses non-modulatable addresses (spec §4). Priming an already
    /// primed path is a no-op success.
    pub fn add(&mut self, chain: ChainType, path: ParamPath, label: [u8; LABEL_LEN]) -> Result<(), RegistryError> {
        if !legacy_to_addr(chain, path).is_some_and(ParamAddr::modulatable) {
            return Err(RegistryError::NotModulatable);
        }
        if self.is_primed(path) {
            return Ok(());
        }
        if self.count >= MAX_REGISTRY_DESTS {
            return Err(RegistryError::Full);
        }
        self.entries[self.count] = Some(ModDestEntry { path, label });
        self.count += 1;
        Ok(())
    }
```
4. Append the bridge at the end of the file:
```rust
/// Temporary bridge (deleted in Task 19): the semantic address a UI
/// `ParamPath` means on `chain`. `Block { block: node, param: slot }` names
/// the node's main page, except each chain's MOD node, whose slots mean the
/// envelope sub-page (plan D8).
pub fn legacy_to_addr(chain: ChainType, path: ParamPath) -> Option<ParamAddr> {
    match path {
        ParamPath::FmOp { op, param } => {
            let op = Op::try_from(op).ok()?;
            let a = PageId::FmOp.binding(param as usize)?;
            Some(ParamAddr::new(BlockRef::FmOp(op), a.param))
        }
        ParamPath::FmEnv { op, param } => {
            let op = Op::try_from(op).ok()?;
            let a = PageId::FmEnv1.binding(param as usize)?;
            Some(ParamAddr::new(BlockRef::FmOp(op), a.param))
        }
        ParamPath::Block { block, param } => {
            let page = match (chain, block) {
                (ChainType::PizzaPoly, 0) => PageId::Pizza,
                (ChainType::Modal, 0) => PageId::EngineModal1,
                (ChainType::Fm, 0) => PageId::FmAlg,
                (ChainType::PizzaPoly | ChainType::Fm, 1) => PageId::Drive,
                (ChainType::PizzaPoly | ChainType::Fm, 2) | (ChainType::Modal, 1) => PageId::Filter,
                (ChainType::PizzaPoly | ChainType::Fm, 3) => PageId::Folder,
                (ChainType::PizzaPoly, 4) | (ChainType::Modal, 2) => PageId::Vca,
                _ => return None,
            };
            page.binding(param as usize)
        }
    }
}
```
In `chimera-core/src/ui/mod_grid.rs`, `for i in 0..registry.count {` → `for i in 0..registry.len() {`.

- [ ] **Step 5: FM pre-wire through the registry; `ChainType::ALL`**

In `chimera-core/src/preset.rs`, add as the first item of `impl ChainType`:
```rust
    pub const ALL: [ChainType; 3] = [ChainType::PizzaPoly, ChainType::Modal, ChainType::Fm];

```
In `Patch::init`'s `ChainType::Fm` arm, replace everything from `// Pre-wire: 4 envelope sources → 4 FM operator levels` through the four `reg.add(…)` lines (keep `(ms, reg)`) with:
```rust
                // Pre-wire: 4 envelope sources → 4 FM operator levels
                let mut reg = ModDestRegistry::new();
                for (op, label) in [(0u8, *b"O1 Lvl\0\0"), (1, *b"O2 Lvl\0\0"), (2, *b"O3 Lvl\0\0"), (3, *b"O4 Lvl\0\0")] {
                    let _ = reg.add(ChainType::Fm, ParamPath::FmOp { op, param: 2 }, label);
                }
                let mut ms = ModState::from_registry(&reg, ChainType::Fm, 4);
                for i in 0..4 {
                    ms.set_amount(i, i, 127); // E(i+1) → Op(i+1) Level full
                }
```

- [ ] **Step 6: Generic modulation in `Voice::render`**

In `chimera-core/src/dsp/voice.rs`:
1. `mod_state.num_sources > 0` / `> 1` → `mod_state.num_sources() > 0` / `> 1`.
2. Replace the whole block from `// Modulated copy (stack only). Offsets still use the chain-index paths` down to (not including) `// 1. Engine → raw oscillator output` with:
```rust
        // Modulated copy (stack only): every routed destination gets its
        // offset through its block's spec (spec §4).
        let mut m = params.clone();
        for d in 0..mod_state.num_dests() {
            let off = mod_state.sum_for(d, &mod_values);
            if off != 0.0 {
                let a = mod_state.dest(d);
                apply_offset(m.block_mut(a.block), a.param, off);
            }
        }

```
3. Remove the now-unused imports `crate::dsp::pizza::PizzaParams`, `crate::mod_path::ParamPath`, and `DriveParams, FilterParams, FmOpParams, FolderParams` (keep `use crate::params::{EngineType, ParamSnapshot};`).

Bit-identity: for each dest the offset is the same sum in the same order, applied by the same formula; destinations `Voice` used to ignore are only ever the ones spec § Intended behavior changes lists. The goldens check this.

- [ ] **Step 7: `UiState` on the new `ModState`**

In `chimera-core/src/ui/mod.rs`:
1. Add inside `impl UiState` (before `current_param_path`):
```rust
    /// Rebuild a track's audio-side `ModState` from the matrix.
    fn sync_mod_state(&mut self, track: usize) {
        let patch = &mut self.project.tracks[track].patch;
        patch.mod_state.sync_from_matrix(&self.matrix_state, patch.chain_type);
    }
```
2. Replace the three `self.project.tracks[…].patch.mod_state.sync_from_matrix(&self.matrix_state);` calls with `self.sync_mod_state(self.active_track);` (matrix encoder) / `self.sync_mod_state(at);` (Plus and Minus).
3. Priming: `self.project.tracks[at].patch.dest_registry.add(path, label);` →
```rust
                    // Refused when the param is not modulatable (spec §4).
                    let chain = self.project.tracks[at].patch.chain_type;
                    let _ = self.project.tracks[at].patch.dest_registry.add(chain, path, label);
```
4. In `update`: `num_dests` / `num_sources` field reads → `num_dests()` / `num_sources()`; delete `let block_idx = self.nav.node as u8;`; the offset line becomes
```rust
                let offset = self.page.binding(i).map_or(0.0, |a| patch.mod_state.offset_for(a, &mod_sources));
```

- [ ] **Step 8: Update the golden harness and the integration test (mechanical)**

In `chimera-core/tests/common/mod.rs`: `use chimera_core::mod_path::ParamPath;` → `use chimera_core::mod_path::{ModDestRegistry, ParamPath};`; replace everything from `/// Filter cutoff at today's DSP path.` up to (not including) `/// Params + ModState for a case` with:
```rust
/// Filter cutoff on each chain's UI path. Before Task 16, `Voice` mapped
/// `Block{2,0}` to cutoff on every chain; the Modal chain's filter page is
/// node 1, so the same destination is `Block{1,0}` there.
pub const PIZZA_CUTOFF: (ChainType, ParamPath) = (ChainType::PizzaPoly, ParamPath::Block { block: 2, param: 0 });
pub const FM_CUTOFF: (ChainType, ParamPath) = (ChainType::Fm, ParamPath::Block { block: 2, param: 0 });
pub const MODAL_CUTOFF: (ChainType, ParamPath) = (ChainType::Modal, ParamPath::Block { block: 1, param: 0 });
/// FM operator A level.
pub const OP_A_LEVEL: (ChainType, ParamPath) = (ChainType::Fm, ParamPath::FmOp { op: 0, param: 2 });

/// One LFO (source 1) route at MOD_AMOUNT to `dest`; env is source 0 so
/// `num_sources >= 2` and the LFO runs.
pub fn lfo_route((chain, dest): (ChainType, ParamPath)) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(chain, dest, *b"GOLDEN\0\0").expect("golden destination must be modulatable");
    let mut ms = ModState::from_registry(&reg, chain, 2);
    ms.set_amount(1, 0, MOD_AMOUNT);
    ms
}
```
and in `setup`: the closure parameter `dest: ParamPath` → `dest: (ChainType, ParamPath)`; `with_lfo(EngineType::Pizza, CUTOFF)` → `PIZZA_CUTOFF`, `with_lfo(EngineType::Fm, CUTOFF)` → `FM_CUTOFF`, `with_lfo(EngineType::Modal, CUTOFF)` → `MODAL_CUTOFF` (the Modal golden now reaches the filter through the Modal chain's own filter page; output is identical).

In `chimera-core/tests/modulation_integration_test.rs`:
```bash
cd chimera-core/tests
python3 - <<'PY'
p = open('modulation_integration_test.rs').read()
start = p.index('    // Modulated: LFO (source 1) -> filter cutoff (block 2, param 0)')
end = p.index('    voice_dry.note_on(')
p = p[:start] + """    // Modulated: LFO (source 1) -> filter cutoff (Pizza chain node 2, slot 0)
    let mut registry = chimera_core::mod_path::ModDestRegistry::new();
    registry
        .add(ChainType::PizzaPoly, chimera_core::mod_path::ParamPath::Block { block: 2, param: 0 }, *b"FLTCUT\\0\\0")
        .unwrap();
    let mut mod_state = ModState::from_registry(&registry, ChainType::PizzaPoly, 2); // env, LFO
    mod_state.set_amount(1, 0, 100); // LFO -> cutoff at high amount

""" + p[end:]
p = p.replace('use chimera_core::params::ParamSnapshot;', 'use chimera_core::params::ParamSnapshot;\nuse chimera_core::preset::ChainType;', 1)
p = p.replace('registry.add(ParamPath::', 'registry.add(ChainType::PizzaPoly, ParamPath::')
for label in ['*b"B1 Prm0\\0"', '*b"TSTaPrm\\0"', '*b"PIZShape"', '*b"FLT Freq"']:
    p = p.replace(label + ');', label + ').unwrap();')
open('modulation_integration_test.rs', 'w').write(p)
PY
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 9: Run everything**

Run: `cargo test -p chimera-core --test modulation_test --test mod_registry_test --test modulatable_test --test modulation_integration_test --test golden_test 2>&1 | grep '^test result'`
Expected: `9 passed`, `7 passed`, `1 passed`, `6 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.
Run: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf 2>&1 | tail -2` (its `static DEFAULT_MOD_STATE: ModState = ModState::new();` still works: `new` is `const`). Report if the target is missing.

- [ ] **Step 10: Commit**

```bash
git add chimera-core/src/modulation.rs chimera-core/src/mod_path.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/preset.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/mod.rs chimera-core/tests/
git commit -m "feat(core): generic modulation over ParamAddr; registry refuses non-modulatable

ModState is private and built only from the registry or the matrix. Voice
applies every route through the block's spec, so Modal-chain routes now
modulate the Modal filter, and amp env, drive/folder mix, FM feedback and
volume become modulatable. Sources/dests are clamped (no out-of-bounds in
the ISR). The UI still speaks ParamPath through a temporary bridge.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 17: Desktop double buffer carries `AudioShared { params, mod_state }`

Makes modulation audible in the simulator (spec §4 "Desktop"; today it renders with `ModState::new()`). `chimera-desktop` has no test target and cannot be compiled on this machine (ALSA headers), so this task has no failing unit test; `modulatable_test.rs` (Task 16) already proves the core path the callback now uses. Verification is a compile check plus a manual listen.

**Files:**
- Replace: `chimera-desktop/src/audio.rs` (full content below)
- Modify: `chimera-desktop/src/main.rs:54-55`

**Interfaces:**
- Consumes: Task 16 (`ModState: Clone + Default`), Task 13 (`Voice::render(out, params, mod_state)`, `MidiNote`, `Velocity`).
- Produces: `DesktopAudio::update(&mut self, params: &ParamSnapshot, mod_state: &ModState)` (replaces `update_params`); private `struct AudioShared { params: ParamSnapshot, mod_state: ModState }` swapped through one `AtomicPtr`.

- [ ] **Step 1: Replace `chimera-desktop/src/audio.rs`**

```rust
use chimera_core::dsp::chorus::JunoChorus;
use chimera_core::dsp::delay::TapeDelay;
use chimera_core::dsp::reverb::Reverb;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_core::{MidiNote, Velocity};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

const NOTE_NONE: u8 = 0;
const NOTE_ON_FLAG: u8 = 0x80;

/// Everything the audio callback reads from the UI, swapped as one unit so
/// params and modulation routes always match (spec §4, "Desktop").
#[derive(Clone, Default)]
struct AudioShared {
    params: ParamSnapshot,
    mod_state: ModState,
}

struct SharedState {
    current: AtomicPtr<AudioShared>,
    note_cmd: AtomicU8,
    velocity: AtomicU8,
}

pub struct DesktopAudio {
    _stream: Stream,
    shared: Arc<SharedState>,
    bufs: Box<[AudioShared; 2]>,
    active_buf: usize,
}

impl DesktopAudio {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = device.default_output_config().expect("no output config");
        let sample_rate = config.sample_rate().0;

        let mut bufs = Box::new([AudioShared::default(), AudioShared::default()]);
        let initial_ptr = &mut bufs[0] as *mut AudioShared;

        let shared = Arc::new(SharedState {
            current: AtomicPtr::new(initial_ptr),
            note_cmd: AtomicU8::new(NOTE_NONE),
            velocity: AtomicU8::new(Velocity::DEFAULT.get()),
        });
        let shared_clone = Arc::clone(&shared);

        let mut voice = Box::new(Voice::new(sample_rate));
        let mut chorus = Box::new(JunoChorus::new());
        let mut delay = Box::new(TapeDelay::new());
        let mut reverb = Box::new(Reverb::new());
        let mut block = [0.0f32; chimera_hal::BLOCK_SIZE];
        let mut block_pos: usize = chimera_hal::BLOCK_SIZE;

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let current = shared_clone.current.load(Ordering::Acquire);
                    // SAFETY: pointer always valid — points into `bufs` owned by
                    // DesktopAudio. UI writes the inactive buffer, swaps atomically.
                    let AudioShared { params, mod_state } = unsafe { &*current };

                    let cmd = shared_clone.note_cmd.swap(NOTE_NONE, Ordering::Relaxed);
                    if cmd & NOTE_ON_FLAG != 0 {
                        let vel = shared_clone.velocity.load(Ordering::Relaxed);
                        // Both were stored from a MidiNote/Velocity, so these always succeed.
                        if let (Some(note), Some(vel)) = (MidiNote::new(cmd & 0x7F), Velocity::new(vel)) {
                            voice.note_on(note, vel, params);
                        }
                    } else if cmd > 0 {
                        voice.note_off();
                    }

                    for sample in data.iter_mut() {
                        if block_pos >= chimera_hal::BLOCK_SIZE {
                            voice.render(&mut block, params, mod_state);
                            // Effects chain: chorus → delay → reverb (Digitone II style)
                            chorus.process(&mut block, &params.chorus, sample_rate);
                            delay.process(&mut block, &params.delay, sample_rate);
                            reverb.process(&mut block, &params.reverb);
                            block_pos = 0;
                        }
                        *sample = libm::tanhf(block[block_pos] * 0.7);
                        block_pos += 1;
                    }
                },
                |err| eprintln!("audio error: {}", err),
                None,
            )
            .expect("failed to build audio stream");

        stream.play().expect("failed to play stream");

        Self {
            _stream: stream,
            shared,
            bufs,
            active_buf: 0,
        }
    }

    /// Push params and modulation routes to the audio thread (lock-free swap).
    pub fn update(&mut self, params: &ParamSnapshot, mod_state: &ModState) {
        let inactive = 1 - self.active_buf;
        let buf = &mut self.bufs[inactive];
        buf.params = params.clone();
        buf.mod_state = mod_state.clone();
        let ptr = buf as *mut AudioShared;
        self.shared.current.store(ptr, Ordering::Release);
        self.active_buf = inactive;
    }

    pub fn note_on(&self, note: MidiNote, velocity: Velocity) {
        self.shared.velocity.store(velocity.get(), Ordering::Relaxed);
        self.shared
            .note_cmd
            .store(NOTE_ON_FLAG | note.get(), Ordering::Relaxed);
    }

    pub fn note_off(&self) {
        self.shared.note_cmd.store(1, Ordering::Relaxed);
    }
}
```

- [ ] **Step 2: Push both halves from the UI loop**

In `chimera-desktop/src/main.rs` replace
```rust
        // Push full param snapshot to audio thread (track 0)
        audio.update_params(&ui.project.tracks[0].patch.params);
```
with
```rust
        // Push params + modulation routes to the audio thread (track 0)
        let patch = &ui.project.tracks[0].patch;
        audio.update(&patch.params, &patch.mod_state);
```

- [ ] **Step 3: Verify**

Run: `cargo check -p chimera-desktop 2>&1 | tail -3`
Expected: `Finished`. On this machine it stops in `alsa-sys` until `sudo apt install libasound2-dev` is run — report "desktop not compiled: ALSA headers missing" and have the reviewer read the diff.
Run: `cargo test -p chimera-core 2>&1 | grep -E '^test result' | awk '{f+=$6} END {print f, "failed"}'` → `0 failed` (core untouched).
Manual (when a desktop build is possible): `just desktop`, go to the Filter page, MIX+Plus on CUTOFF, open the MOD page, set LFO→cutoff to +100 with encoder E, hold a key: the filter sweeps.

- [ ] **Step 4: Commit**

```bash
git add chimera-desktop/src/audio.rs chimera-desktop/src/main.rs
git commit -m "feat(desktop): audio thread renders with the track's ModState

Params and mod routes are swapped together as one AudioShared buffer, so
modulation is audible in the simulator.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 18: `SlotBinding` + `BlockDef.id`; Part pages bind to addresses

Data-only step: every `BlockDef` slot gets a `SlotBinding`, and every def an `id`. Part-chain pages bind to `ParamAddr`s; Mixer/System/Demo slots become `Legacy`. The renderer asks the slot for its label and format (which for bound slots come from the spec). Encoders still go through `PageId` (Task 20 switches them).

**Files:**
- Modify: `chimera-core/src/ui/block_def.rs` (`SlotBinding`, new `ParamSlot`, `slot_addr`, `BlockDef.id`)
- Modify: `chimera-core/src/ui/block_registry.rs` (mechanical + Part defs rebound)
- Modify: `chimera-core/src/ui/renderer.rs` (`slot.label()` / `slot.format()`), `chimera-core/src/ui/mod.rs:137` (`.label()`)
- Modify (mechanical): `chimera-core/tests/{ui_test,block_def_tests,page_block_test,modulation_integration_test}.rs`
- Create: `chimera-core/tests/binding_test.rs`

**Interfaces:**
- Consumes: Task 14 (`ParamAddr`, `BlockRef`, `Op`, `BlockRef::specs`), `find_spec` (Task 2), all block id consts.
- Produces:
  - `pub enum SlotBinding { Empty, Param(ParamAddr), SelectedOp(ParamId), SelectOp, Legacy { label: &'static str, fmt: ValFmt } }`
  - `pub struct ParamSlot { pub binding: SlotBinding, pub icon: CellIcon, pub label_override: Option<&'static str> }` with `ParamSlot::EMPTY`, `const fn param(block: BlockRef, param: ParamId, icon: CellIcon)`, `const fn selected_op(param: ParamId, icon)`, `const fn select_op(icon)`, `const fn legacy(label, fmt, icon)`, `const fn with_label(self, label)`, `fn spec(&self) -> Option<&'static ParamSpec>`, `fn label(&self) -> &'static str`, `fn format(&self) -> ValFmt`
  - `pub fn slot_addr(def: &BlockDef, slot: usize, sel_op: Op) -> Option<ParamAddr>`
  - `BlockDef { pub id: u16, … }` — ids assigned 1..=40 in file order (PIZZA = 1 … DEMO_FM = 40)

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/binding_test.rs`:
```rust
//! Slot bindings (spec §5, § Testing "Bindings").

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::params::FmOpParams;
use chimera_core::preset::ChainType;
use chimera_core::ui::block_def::{slot_addr, BlockDef, SlotBinding};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::page::ValFmt;

/// Every page reachable from a Part chain (main pages and sub-pages).
fn part_defs() -> Vec<&'static BlockDef> {
    let mut defs = Vec::new();
    for ct in ChainType::ALL {
        for block in chain_def_for(ct).blocks {
            defs.push(block.def);
            defs.extend(block.sub_pages.iter().copied());
        }
    }
    defs
}

#[test]
fn every_part_slot_resolves_to_a_spec() {
    for def in part_defs() {
        for (i, slot) in def.params.iter().enumerate() {
            match slot.binding {
                SlotBinding::Empty | SlotBinding::SelectOp => {}
                SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                    assert!(slot.spec().is_some(), "{} slot {i}: no spec", def.name)
                }
                SlotBinding::Legacy { .. } => panic!("{} slot {i}: Part pages must not be Legacy", def.name),
            }
        }
    }
}

#[test]
fn block_def_ids_are_unique() {
    let all: [&BlockDef; 40] = [
        &reg::PIZZA, &reg::MODAL_1, &reg::MODAL_2, &reg::VA, &reg::FM_ALG, &reg::FM_OP,
        &reg::FM_RATIO, &reg::DRIVE, &reg::FOLDER, &reg::FILTER, &reg::ENVELOPE, &reg::LFO,
        &reg::ENV_AMP, &reg::ENV_FILTER, &reg::ENV_AUX, &reg::EFX, &reg::MIXER, &reg::CHORUS,
        &reg::DELAY, &reg::MASTER, &reg::NOISE, &reg::MOD_MATRIX, &reg::FM_ENV1, &reg::FM_ENV2,
        &reg::FM_ENV3, &reg::FM_ENV4, &reg::CHANNEL, &reg::MIDI_CFG, &reg::EQ, &reg::SENDS,
        &reg::SYS_MIDI, &reg::SYS_TUNING, &reg::SYS_THEME, &reg::SYS_UPDATES, &reg::SYS_ABOUT,
        &reg::DEMO_WAVES, &reg::DEMO_SHAPES, &reg::DEMO_MOTION, &reg::DEMO_MATRIX, &reg::DEMO_FM,
    ];
    for (i, d) in all.iter().enumerate() {
        assert!(all[..i].iter().all(|o| o.id != d.id), "{} reuses id {}", d.name, d.id);
    }
}

/// Labels and formats of Part pages are exactly what they displayed before
/// bindings (spec labels + the two plan-D6 overrides; plan D5 BODY fix).
#[test]
fn part_pages_display_like_before() {
    use ValFmt::{Bi, Int, Uni};
    let want: [(&BlockDef, [(&str, ValFmt); 6]); 15] = [
        (&reg::PIZZA, [("SHAPE", Uni), ("CRUSH", Uni), ("LEVEL", Uni), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::MODAL_1, [("MODE", Int(3)), ("EXCITE", Uni), ("DECAY", Uni), ("BRIGHT", Uni), ("POS", Uni), ("INHARM", Uni)]),
        (&reg::MODAL_2, [("BODY", Uni), ("STIFF", Uni), ("FDBK", Uni), ("E.DPT", Uni), ("E.RAT", Uni), ("E.MIX", Uni)]),
        (&reg::FM_ALG, [("ALG", Int(7)), ("--", Uni), ("LEVEL", Uni), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::FM_OP, [("OP", Int(3)), ("WAVE", Int(7)), ("LEVEL", Uni), ("FDBK", Int(7)), ("DETUN", Bi), ("V.SNS", Int(7))]),
        (&reg::FM_RATIO, [("OP1", Int(63)), ("OP2", Int(63)), ("OP3", Int(63)), ("OP4", Int(63)), ("FINE", Int(15)), ("--", Uni)]),
        (&reg::DRIVE, [("DRIVE", Uni), ("TONE", Bi), ("MIX", Bi), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::FOLDER, [("FOLD", Uni), ("SYM", Bi), ("MIX", Bi), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::FILTER, [("CUTOFF", Uni), ("RESO", Uni), ("DRIVE", Uni), ("FM", Uni), ("ENV", Bi), ("TRACK", Uni)]),
        (&reg::ENVELOPE, [("ATK", Uni), ("DEC", Uni), ("SUS", Uni), ("REL", Uni), ("DEPTH", Uni), ("VEL", Uni)]),
        (&reg::LFO, [("RATE", Uni), ("SHAPE", Int(4)), ("SYNC", Int(1)), ("PHASE", Uni), ("DEPTH", Uni), ("OFST", Bi)]),
        (&reg::FM_ENV1, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
        (&reg::FM_ENV2, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
        (&reg::FM_ENV3, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
        (&reg::FM_ENV4, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
    ];
    for (def, slots) in want {
        for (i, (label, fmt)) in slots.iter().enumerate() {
            assert_eq!(def.params[i].label(), *label, "{} slot {i}", def.name);
            assert_eq!(def.params[i].format(), *fmt, "{} slot {i}", def.name);
        }
    }
}

/// Spec §5: `SelectedOp` resolves to the operator selected when the address
/// is built; fixed bindings ignore the selection.
#[test]
fn slot_addr_resolves_selected_op_at_build_time() {
    assert_eq!(
        slot_addr(&reg::FM_OP, 2, Op::C),
        Some(ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::LEVEL))
    );
    assert_eq!(
        slot_addr(&reg::FM_RATIO, 1, Op::D),
        Some(ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::COARSE))
    );
    assert_eq!(
        slot_addr(&reg::FM_RATIO, 4, Op::D),
        Some(ParamAddr::new(BlockRef::FmOp(Op::D), FmOpParams::FINE))
    );
    assert_eq!(slot_addr(&reg::FM_OP, 0, Op::A), None); // the selector
    assert_eq!(slot_addr(&reg::PIZZA, 5, Op::A), None); // empty
    assert_eq!(slot_addr(&reg::MIXER, 0, Op::A), None); // legacy
    assert_eq!(slot_addr(&reg::PIZZA, 9, Op::A), None); // out of range
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test binding_test 2>&1 | grep -E '^error\[' | head -3`
Expected: `unresolved imports chimera_core::ui::block_def::slot_addr, …::SlotBinding`.

- [ ] **Step 3: New slot types in `chimera-core/src/ui/block_def.rs`**

Change the import line to
```rust
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{find_spec, ParamId, ParamSpec};
use crate::ui::page::{CellIcon, PageLayout, ValFmt};
```
replace the old `ParamSlot` struct (with its derive) with:
```rust
/// What an encoder slot edits (spec §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotBinding {
    Empty,
    /// A fixed param, e.g. Filter cutoff or `FmOp(A)` coarse.
    Param(ParamAddr),
    /// A param of the currently selected FM operator.
    SelectedOp(ParamId),
    /// The FM operator selector itself.
    SelectOp,
    /// Mixer/System/Demo pages, still driven by `PageId`.
    Legacy { label: &'static str, fmt: ValFmt },
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSlot {
    pub binding: SlotBinding,
    pub icon: CellIcon,
    /// Display label override; `None` shows the spec's label (plan D6).
    /// Format and step always come from the spec.
    pub label_override: Option<&'static str>,
}

impl ParamSlot {
    pub const EMPTY: ParamSlot = ParamSlot { binding: SlotBinding::Empty, icon: CellIcon::None, label_override: None };

    pub const fn param(block: BlockRef, param: ParamId, icon: CellIcon) -> Self {
        Self { binding: SlotBinding::Param(ParamAddr::new(block, param)), icon, label_override: None }
    }

    pub const fn selected_op(param: ParamId, icon: CellIcon) -> Self {
        Self { binding: SlotBinding::SelectedOp(param), icon, label_override: None }
    }

    pub const fn select_op(icon: CellIcon) -> Self {
        Self { binding: SlotBinding::SelectOp, icon, label_override: None }
    }

    pub const fn legacy(label: &'static str, fmt: ValFmt, icon: CellIcon) -> Self {
        Self { binding: SlotBinding::Legacy { label, fmt }, icon, label_override: None }
    }

    pub const fn with_label(self, label: &'static str) -> Self {
        Self { label_override: Some(label), ..self }
    }

    /// The spec this slot edits (bound slots only).
    pub fn spec(&self) -> Option<&'static ParamSpec> {
        match self.binding {
            SlotBinding::Param(a) => a.spec(),
            SlotBinding::SelectedOp(id) => find_spec(BlockRef::FmOp(Op::A).specs(), id),
            SlotBinding::Empty | SlotBinding::SelectOp | SlotBinding::Legacy { .. } => None,
        }
    }

    pub fn label(&self) -> &'static str {
        if let Some(label) = self.label_override {
            return label;
        }
        match self.binding {
            SlotBinding::Empty => "--",
            SlotBinding::SelectOp => "OP",
            SlotBinding::Legacy { label, .. } => label,
            SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => self.spec().map_or("??", |s| s.label),
        }
    }

    pub fn format(&self) -> ValFmt {
        match self.binding {
            SlotBinding::Empty => ValFmt::Uni,
            SlotBinding::SelectOp => ValFmt::Int(3),
            SlotBinding::Legacy { fmt, .. } => fmt,
            SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => self.spec().map_or(ValFmt::Uni, |s| s.fmt),
        }
    }
}

/// The address slot `slot` of `def` edits. `SelectedOp` resolves to the
/// operator selected *now*, so a saved route always names a concrete operator.
pub fn slot_addr(def: &BlockDef, slot: usize, sel_op: Op) -> Option<ParamAddr> {
    match def.params.get(slot)?.binding {
        SlotBinding::Param(a) => Some(a),
        SlotBinding::SelectedOp(id) => Some(ParamAddr::new(BlockRef::FmOp(sel_op), id)),
        SlotBinding::Empty | SlotBinding::SelectOp | SlotBinding::Legacy { .. } => None,
    }
}
```
and add as the first field of `BlockDef`:
```rust
    /// Unique page identity (`PageKey`); defs like FILTER are shared across chains.
    pub id: u16,
```

- [ ] **Step 4: Convert `block_registry.rs`**

Mechanical part (EMPTY, every `ParamSlot { label, format, icon }` literal → `ParamSlot::legacy(…)`, ids in file order):
```bash
cd chimera-core/src/ui
perl -0pi -e 's/const EMPTY: ParamSlot = ParamSlot \{\n    label: "--",\n    format: ValFmt::Uni,\n    icon: CellIcon::None,\n\};/const EMPTY: ParamSlot = ParamSlot::EMPTY;/' block_registry.rs
perl -pi -e 's/ParamSlot \{ label: ("[^"]*"),\s*format: ([^,]+?),\s*icon: ([A-Za-z:]+)\s*\}/ParamSlot::legacy($1, $2, $3)/g' block_registry.rs
perl -0pi -e 'my $n = 0; s/(pub static \w+: BlockDef = BlockDef \{\n)/$1 . "    id: " . ++$n . ",\n"/ge' block_registry.rs
grep -c 'ParamSlot::legacy' block_registry.rs   # 182
cd ../../..
```
Then rebind the fifteen Part-chain pages (replaces only their `params: [ … ]` bodies and adds the imports). Save as `/tmp/rebind.py` and run `python3 /tmp/rebind.py chimera-core/src/ui/block_registry.rs`:
```python
import sys

PART = {
'PIZZA': '''        ParamSlot::param(BlockRef::Pizza, PizzaParams::SHAPE, CellIcon::WaveShape),
        ParamSlot::param(BlockRef::Pizza, PizzaParams::CRUSH, CellIcon::WaveClip),
        ParamSlot::param(BlockRef::Pizza, PizzaParams::LEVEL, CellIcon::LevelBar),
        EMPTY, EMPTY, EMPTY,''',
'MODAL_1': '''        ParamSlot::param(BlockRef::Modal, ModalParams::MODE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::EXCITE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::DECAY, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::BRIGHTNESS, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::POSITION, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::INHARM, CellIcon::Arc),''',
'MODAL_2': '''        ParamSlot::param(BlockRef::Modal, ModalParams::KS_BODY, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_STIFFNESS, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_FEEDBACK, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_ENS_DEPTH, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_ENS_RATE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Modal, ModalParams::KS_ENS_MIX, CellIcon::Arc),''',
'FM_ALG': '''        ParamSlot::param(BlockRef::Fm, FmParams::ALGORITHM, CellIcon::FmAlgorithm),
        EMPTY,
        // One address per physical param: the voice's output level.
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME, CellIcon::LevelBar),
        EMPTY,
        EMPTY,
        EMPTY,''',
'FM_OP': '''        ParamSlot::select_op(CellIcon::Arc),
        ParamSlot::selected_op(FmOpParams::WAVEFORM, CellIcon::WaveShape),
        ParamSlot::selected_op(FmOpParams::LEVEL, CellIcon::LevelBar),
        ParamSlot::selected_op(FmOpParams::FEEDBACK, CellIcon::Arc),
        ParamSlot::selected_op(FmOpParams::DETUNE, CellIcon::Arc),
        ParamSlot::selected_op(FmOpParams::VELOCITY_SENS, CellIcon::Arc),''',
'FM_RATIO': '''        ParamSlot::param(BlockRef::FmOp(Op::A), FmOpParams::COARSE, CellIcon::Arc).with_label("OP1"),
        ParamSlot::param(BlockRef::FmOp(Op::B), FmOpParams::COARSE, CellIcon::Arc).with_label("OP2"),
        ParamSlot::param(BlockRef::FmOp(Op::C), FmOpParams::COARSE, CellIcon::Arc).with_label("OP3"),
        ParamSlot::param(BlockRef::FmOp(Op::D), FmOpParams::COARSE, CellIcon::Arc).with_label("OP4"),
        ParamSlot::selected_op(FmOpParams::FINE, CellIcon::Arc),
        EMPTY,''',
'DRIVE': '''        ParamSlot::param(BlockRef::Drive, DriveParams::DRIVE, CellIcon::WaveClip),
        ParamSlot::param(BlockRef::Drive, DriveParams::TONE, CellIcon::ToneTilt),
        ParamSlot::param(BlockRef::Drive, DriveParams::MIX, CellIcon::DryWet),
        EMPTY,
        EMPTY,
        EMPTY,''',
'FOLDER': '''        ParamSlot::param(BlockRef::Folder, FolderParams::FOLD, CellIcon::WaveFold),
        ParamSlot::param(BlockRef::Folder, FolderParams::SYMMETRY, CellIcon::Symmetry),
        ParamSlot::param(BlockRef::Folder, FolderParams::MIX, CellIcon::DryWet),
        EMPTY,
        EMPTY,
        EMPTY,''',
'FILTER': '''        ParamSlot::param(BlockRef::Filter, FilterParams::CUTOFF, CellIcon::None),
        ParamSlot::param(BlockRef::Filter, FilterParams::RESONANCE, CellIcon::None),
        ParamSlot::param(BlockRef::Filter, FilterParams::DRIVE, CellIcon::None),
        ParamSlot::param(BlockRef::Filter, FilterParams::FM_AMOUNT, CellIcon::None),
        ParamSlot::param(BlockRef::Filter, FilterParams::ENV_AMOUNT, CellIcon::None),
        ParamSlot::param(BlockRef::Filter, FilterParams::KEY_TRACK, CellIcon::None),''',
'ENVELOPE': '''        ParamSlot::param(BlockRef::AmpEnv, EnvParams::ATTACK, CellIcon::None),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::DECAY, CellIcon::None),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::SUSTAIN, CellIcon::None),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::RELEASE, CellIcon::None),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::LEVEL, CellIcon::None).with_label("DEPTH"),
        ParamSlot::param(BlockRef::AmpEnv, EnvParams::VEL_SENS, CellIcon::None),''',
'LFO': '''        ParamSlot::param(BlockRef::Lfo, LfoParams::RATE, CellIcon::Orbit),
        ParamSlot::param(BlockRef::Lfo, LfoParams::SHAPE, CellIcon::WaveShape),
        ParamSlot::param(BlockRef::Lfo, LfoParams::SYNC, CellIcon::Arc),
        ParamSlot::param(BlockRef::Lfo, LfoParams::PHASE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Lfo, LfoParams::DEPTH, CellIcon::Breathe),
        ParamSlot::param(BlockRef::Lfo, LfoParams::OFFSET, CellIcon::Arc),''',
}
for n, op in [('FM_ENV1','A'),('FM_ENV2','B'),('FM_ENV3','C'),('FM_ENV4','D')]:
    PART[n] = '\n'.join('        ParamSlot::param(BlockRef::FmOp(Op::%s), FmOpParams::%s, CellIcon::None),' % (op, p) for p in ['ATTACK_RATE','DECAY1_RATE','DECAY1_LEVEL','DECAY2_RATE','RELEASE_RATE','RATE_SCALING'])
IMPORTS = '''use crate::addr::{BlockRef, Op};
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::pizza::PizzaParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams};
'''
def apply(src):
    for name, body in PART.items():
        i = src.index('pub static %s: BlockDef = BlockDef {' % name)
        j = src.index('    params: [', i) + len('    params: [\n')
        k = src.index('\n    ],\n};', j)
        src = src[:j] + body + src[k:]
    return IMPORTS + src

path = sys.argv[1]
src = open(path).read()
open(path, 'w').write(apply(src))
```

- [ ] **Step 5: Ask slots for label/format**

`chimera-core/src/ui/renderer.rs`: `let label = slot.label;` → `slot.label();`, `fmt::fmt_val(&mut buf, val, slot.format);` → `slot.format()`, and in `draw_cell_grid_from_def` the arguments `slot.label,` / `slot.format,` → `slot.label(),` / `slot.format(),`.
`chimera-core/src/ui/mod.rs`: `def.params[self.last_encoder].label;` → `def.params[self.last_encoder].label();`.

Tests (mechanical):
```bash
cd chimera-core/tests
sed -i 's/\.params\[\([0-9]\)\]\.format\b/.params[\1].format()/g; s/\.params\[\([0-9]\)\]\.label\b/.params[\1].label()/g' ui_test.rs block_def_tests.rs page_block_test.rs
perl -pi -e 's/chimera_core::ui::block_def::ParamSlot \{ label: ("[^"]*"), format: ([^,]+), icon: ([A-Za-z:]+) \}/chimera_core::ui::block_def::ParamSlot::legacy($1, $2, $3)/' modulation_integration_test.rs
perl -0pi -e 's/(static ENV_DEF: BlockDef = BlockDef \{\n)/$1        id: 900,\n/; s/(static LFO_DEF: BlockDef = BlockDef \{\n)/$1        id: 901,\n/' modulation_integration_test.rs
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```

- [ ] **Step 6: Run everything**

Run: `cargo test -p chimera-core --test binding_test --test ui_test --test block_def_tests --test golden_test 2>&1 | grep '^test result'`
Expected: all `ok` (`binding_test` 4 passed). Full suite: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/ui/block_def.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/mod.rs chimera-core/tests/
git commit -m "feat(ui): slots bind to ParamAddr; label and format come from the spec

Every BlockDef gets a unique id. Part-chain pages bind to semantic addresses
(FM operator pages via SelectedOp/SelectOp); Mixer/System/Demo stay Legacy.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 19: The UI builds `ParamAddr`s; registry and matrix hold addresses; delete `ParamPath` and the bridge

**Files:**
- Replace: `chimera-core/src/mod_path.rs` (registry only; full content below)
- Modify: `chimera-core/src/modulation.rs` (`from_registry`/`sync_from_matrix` lose `chain`)
- Modify: `chimera-core/src/ui/mod_grid.rs` (`ModDest.addr`, `mod_info_for`)
- Modify: `chimera-core/src/preset.rs` (pre-wire by address)
- Modify: `chimera-core/src/ui/mod.rs` (`current_param_addr`, `mod_label`, priming, display offsets via `slot_addr`)
- Modify: `chimera-core/src/ui/renderer.rs` (mod bars via `slot_addr`; drop `block_idx` params)
- Replace: `chimera-core/tests/modulation_test.rs`, `chimera-core/tests/mod_registry_test.rs`
- Modify: `chimera-core/tests/{common/mod.rs,modulation_integration_test.rs,modulatable_test.rs}`
- Create: `chimera-core/tests/ui_routing_test.rs`

**Interfaces:**
- Consumes: Task 18 (`slot_addr`, `ParamSlot::label()`), Task 16 (`ModState` API), Task 15 (`page::selected_op()`).
- Produces:
  - `ModDestEntry { pub addr: ParamAddr, pub label: [u8; LABEL_LEN] }`; `ModDestRegistry::{add(&mut self, addr: ParamAddr, label) -> Result<(), RegistryError>, remove(addr), find(addr) -> Option<usize>, is_primed(addr) -> bool, len, is_empty, get}` (+ `Default`)
  - `ModState::from_registry(registry: &ModDestRegistry, num_sources: usize) -> Self`, `ModState::sync_from_matrix(&mut self, matrix: &MatrixState)`
  - `mod_grid::ModDest { pub addr: ParamAddr, pub label }`, `MatrixState::mod_info_for(&self, addr: ParamAddr) -> Option<f32>` (replaces `mod_info_for_param`/`mod_info_for_path`)
  - `UiState::current_param_addr(&self) -> Option<ParamAddr>`, `UiState::mod_label(&self, addr: ParamAddr) -> [u8; LABEL_LEN]` (private)
  - `Renderer::draw_params_from_def(&self, display, def: &BlockDef, matrix_state: &MatrixState)` and `draw_cell_grid_from_def(…)` (no `block_idx`)
  - Deleted: `ParamPath`, `legacy_to_addr`, `UiState::current_param_path/current_param_label`, `Renderer::param_path_for_cell`
  - Harness: `common::lfo_route(dest: ParamAddr) -> ModState`; consts `CUTOFF`, `OP_A_LEVEL: ParamAddr`

- [ ] **Step 1: Write the failing tests**

Replace `chimera-core/tests/modulation_test.rs` with:
```rust
use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{ModState, MAX_MOD_DESTS, MAX_MOD_SOURCES};
use chimera_core::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FolderParams};
use chimera_core::ui::mod_grid::MatrixState;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

/// A ModState with one dest and `n` sources.
fn one_dest(addr: ParamAddr, n: usize) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(addr, *b"TEST\0\0\0\0").unwrap();
    ModState::from_registry(&reg, n)
}

/// Every modulatable address (25).
fn all_modulatable() -> Vec<ParamAddr> {
    BlockRef::ALL
        .iter()
        .flat_map(|&b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|a| a.modulatable())
        .collect()
}

#[test]
fn mod_state_default_is_empty() {
    let ms = ModState::new();
    assert_eq!(ms.num_sources(), 0);
    assert_eq!(ms.num_dests(), 0);
    for si in 0..MAX_MOD_SOURCES {
        for di in 0..MAX_MOD_DESTS {
            assert_eq!(ms.amount(si, di), 0);
        }
    }
}

#[test]
fn mod_state_offset_no_routes() {
    let ms = ModState::new();
    let sources = [0.0f32; MAX_MOD_SOURCES];
    assert_eq!(ms.offset_for(ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE), &sources), 0.0);
}

#[test]
fn mod_state_offset_single_route() {
    let mut ms = one_dest(CUTOFF, 1);
    ms.set_amount(0, 0, 64);
    assert_eq!(ms.dest(0), CUTOFF);

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.offset_for(CUTOFF, &sources);
    let expected = 1.0 * (64.0 / 127.0);
    assert!((offset - expected).abs() < 1e-5, "expected {expected}, got {offset}");
    assert_eq!(ms.sum_for(0, &sources), offset);
}

#[test]
fn mod_state_offset_multiple_sources() {
    let mut ms = one_dest(ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE), 2);
    ms.set_amount(0, 0, 50);
    ms.set_amount(1, 0, 100);

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 0.5;
    sources[1] = -0.8;
    let offset = ms.sum_for(0, &sources);
    let expected = 0.5 * (50.0 / 127.0) + (-0.8) * (100.0 / 127.0);
    assert!((offset - expected).abs() < 1e-5, "expected {expected}, got {offset}");
}

#[test]
fn mod_state_offset_negative_amount() {
    let mut ms = one_dest(ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY), 1);
    ms.set_amount(0, 0, -80);

    let mut sources = [0.0f32; MAX_MOD_SOURCES];
    sources[0] = 1.0;
    let offset = ms.sum_for(0, &sources);
    assert!((offset - (-80.0 / 127.0)).abs() < 1e-5);
    assert!(offset < 0.0);
}

#[test]
fn mod_state_set_amount_ignores_out_of_range() {
    let mut ms = one_dest(ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE), 2);
    ms.set_amount(5, 0, 99); // no such source
    ms.set_amount(0, 3, 99); // no such dest
    assert_eq!(ms.amount(5, 0), 0);
    assert_eq!(ms.amount(0, 3), 0);
}

#[test]
fn mod_state_sync_from_matrix() {
    let crush = ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH);
    let mut registry = ModDestRegistry::new();
    registry.add(crush, *b"A  p\0\0\0\0").unwrap();
    registry.add(CUTOFF, *b"B  q\0\0\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 2;
    matrix.amounts[0][0] = 42;
    matrix.amounts[1][1] = -99;

    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);

    assert_eq!(ms.num_sources(), 2);
    assert_eq!(ms.num_dests(), 2);
    assert_eq!(ms.amount(0, 0), 42);
    assert_eq!(ms.amount(1, 1), -99);
    assert_eq!(ms.dest(0), crush);
    assert_eq!(ms.dest(1), CUTOFF);
}

/// Review Focus 2: more destinations than fit keep amounts aligned.
#[test]
fn mod_state_truncates_without_misaligning() {
    let mut registry = ModDestRegistry::new();
    for a in all_modulatable() {
        registry.add(a, *b"X\0\0\0\0\0\0\0").unwrap();
    }
    assert_eq!(registry.len(), 25);
    assert_eq!(ModState::from_registry(&registry, 2).num_dests(), MAX_MOD_DESTS);

    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry); // matrix keeps 16 too
    matrix.num_sources = 2;
    matrix.amounts[1][15] = 77;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);
    assert_eq!(ms.num_dests(), MAX_MOD_DESTS);
    assert_eq!(ms.amount(1, 15), 77);
    assert_eq!(Some(ms.dest(15)), matrix.dests[15].map(|d| d.addr));
}

/// Review Focus 2: a matrix with more than MAX_MOD_SOURCES rows is clamped
/// (the old code indexed `amounts[si]` past 8 in the audio thread).
#[test]
fn mod_state_clamps_sources() {
    let mut registry = ModDestRegistry::new();
    registry.add(CUTOFF, *b"X\0\0\0\0\0\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    matrix.num_sources = 16;
    matrix.amounts[12][0] = 50;
    let mut ms = ModState::new();
    ms.sync_from_matrix(&matrix);
    assert_eq!(ms.num_sources(), MAX_MOD_SOURCES);
    let values = [1.0f32; MAX_MOD_SOURCES];
    assert_eq!(ms.sum_for(0, &values), 0.0); // row 12 was dropped, nothing panicked
}

/// The matrix holds addresses, so a route to an FM operator stays on that
/// operator (and the amp envelope is a destination like any other).
#[test]
fn matrix_dests_are_semantic() {
    let op_c = ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK);
    let atk = ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK);
    let mut registry = ModDestRegistry::new();
    registry.add(op_c, *b"O3 FDBK\0").unwrap();
    registry.add(atk, *b"ENVATK\0\0").unwrap();
    let mut matrix = MatrixState::new();
    matrix.rebuild_dests_from_registry(&registry);
    assert_eq!(matrix.mod_info_for(op_c), Some(0.0));
    assert_eq!(matrix.mod_info_for(atk), Some(0.0));
    assert_eq!(matrix.mod_info_for(ParamAddr::new(BlockRef::FmOp(Op::D), FmOpParams::FEEDBACK)), None);
}
```

Replace `chimera-core/tests/mod_registry_test.rs` with:
```rust
use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::mod_path::{ModDestRegistry, RegistryError};
use chimera_core::params::{DriveParams, FilterParams, FmOpParams, FmParams};
use chimera_core::dsp::modal::ModalParams;

fn op_level(op: Op) -> ParamAddr {
    ParamAddr::new(BlockRef::FmOp(op), FmOpParams::LEVEL)
}

#[test]
fn registry_starts_empty() {
    let reg = ModDestRegistry::new();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());
}

#[test]
fn registry_add_and_find() {
    let mut reg = ModDestRegistry::new();
    let addr = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    assert_eq!(reg.add(addr, *b"DrvDrv\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
    assert!(reg.is_primed(addr));
    assert_eq!(reg.find(addr), Some(0));
}

#[test]
fn registry_no_duplicates() {
    let mut reg = ModDestRegistry::new();
    assert_eq!(reg.add(op_level(Op::A), *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.add(op_level(Op::A), *b"O1 Lvl\0\0"), Ok(()));
    assert_eq!(reg.len(), 1);
}

#[test]
fn registry_remove() {
    let mut reg = ModDestRegistry::new();
    reg.add(op_level(Op::A), *b"O1 Lvl\0\0").unwrap();
    reg.add(op_level(Op::B), *b"O2 Lvl\0\0").unwrap();
    assert_eq!(reg.len(), 2);
    reg.remove(op_level(Op::A));
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_primed(op_level(Op::A)));
    assert!(reg.is_primed(op_level(Op::B)));
}

#[test]
fn registry_fm_ops_are_distinct() {
    assert_ne!(op_level(Op::A), op_level(Op::B));
    let mut reg = ModDestRegistry::new();
    reg.add(op_level(Op::A), *b"O1 Lvl\0\0").unwrap();
    assert!(reg.is_primed(op_level(Op::A)));
    assert!(!reg.is_primed(op_level(Op::B)));
}

/// Spec § Testing "Registry": adding a non-modulatable address is refused.
#[test]
fn registry_refuses_non_modulatable() {
    let mut reg = ModDestRegistry::new();
    let refused = [
        ParamAddr::new(BlockRef::Modal, ModalParams::EXCITE),         // note-on only
        ParamAddr::new(BlockRef::Fm, FmParams::ALGORITHM),            // Enum
        ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::WAVEFORM),  // Enum
        ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::ATTACK_RATE), // note-on only
        ParamAddr::new(BlockRef::Filter, FilterParams::FM_AMOUNT),    // never read
        ParamAddr::new(BlockRef::FilterEnv, chimera_core::params::EnvParams::ATTACK), // never read (plan D7)
        ParamAddr::new(BlockRef::Pizza, ParamId(99)),                 // no such param
    ];
    for addr in refused {
        assert_eq!(reg.add(addr, *b"X\0\0\0\0\0\0\0"), Err(RegistryError::NotModulatable), "{addr:?}");
    }
    assert!(reg.is_empty());
}

/// Every address is accepted exactly when it is modulatable.
#[test]
fn registry_accepts_exactly_the_modulatable_addresses() {
    let mut reg = ModDestRegistry::new();
    let mut accepted = 0;
    for b in BlockRef::ALL {
        for s in b.specs() {
            let addr = ParamAddr::new(b, s.id);
            assert_eq!(reg.add(addr, *b"X\0\0\0\0\0\0\0").is_ok(), addr.modulatable(), "{addr:?}");
            accepted += addr.modulatable() as usize;
        }
    }
    assert_eq!(accepted, 25);
    assert_eq!(reg.len(), 25);
}
```

Create `chimera-core/tests/ui_routing_test.rs`:
```rust
//! Priming mod destinations from pages (spec §5, Review Focus 1 and 5).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::pizza::PizzaParams;
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

/// Mock controls for driving `UiState::handle_input`.
struct MockControls {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl MockControls {
    fn new() -> Self {
        Self { buttons: Vec::new(), encoders: Vec::new() }
    }
    fn button(mut self, id: ButtonId, state: ButtonState) -> Self {
        self.buttons.push((id, state));
        self
    }
    fn encoder(mut self, id: EncoderId, delta: i8) -> Self {
        self.encoders.push((id, delta));
        self
    }
}

impl Controls for MockControls {
    fn button_state(&self, id: ButtonId) -> ButtonState {
        self.buttons.iter().find(|b| b.0 == id).map_or(ButtonState::Up, |b| b.1)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
}

fn press(ui: &mut UiState, id: ButtonId) {
    ui.handle_input(&MockControls::new().button(id, ButtonState::Pressed));
}

/// Touch encoder A (focus slot 0), then MIX + Plus.
fn prime_slot_0(ui: &mut UiState) {
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1));
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
}

fn primed(ui: &UiState) -> Vec<ParamAddr> {
    let reg = &ui.project.tracks[0].patch.dest_registry;
    (0..reg.len()).filter_map(|i| reg.get(i)).map(|e| e.addr).collect()
}

#[test]
fn priming_on_a_part_page_registers_its_address() {
    let mut ui = UiState::new(); // Part 1, Pizza page
    prime_slot_0(&mut ui);
    assert_eq!(primed(&ui), [ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE)]);
    let reg = &ui.project.tracks[0].patch.dest_registry;
    assert_eq!(reg.get(0).unwrap().label_str(), "PIZSHAPE");
    assert_eq!(ui.mod_state().num_dests(), 1);
}

/// Review Focus 1: Mixer/System/Demo slots are Legacy — priming there must
/// not register anything (it used to register `Block{node,i}`, which the
/// voice read as a Pizza/Drive/Filter/Folder param).
#[test]
fn priming_on_legacy_page_registers_nothing() {
    let mut ui = UiState::new();
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    ); // Mixer chain
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
    press(&mut ui, ButtonId::Menu); // System chain
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
}

/// Spec §4: the registry refuses non-modulatable params (LFO RATE).
#[test]
fn priming_a_non_modulatable_param_is_refused() {
    let mut ui = UiState::new();
    for _ in 0..4 {
        press(&mut ui, ButtonId::Plus); // → MOD node
    }
    press(&mut ui, ButtonId::Edit); // Envelope sub-page
    press(&mut ui, ButtonId::Edit); // LFO sub-page
    prime_slot_0(&mut ui);
    assert!(primed(&ui).is_empty());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test modulation_test --test mod_registry_test --test ui_routing_test 2>&1 | grep -E '^error\[' | sort | uniq -c | head`
Expected: `no field addr on type &ModDestEntry`, `this method takes 3 arguments but 2 arguments were supplied` (`add`), `no method named mod_info_for`. (With the old code, `priming_on_legacy_page_registers_nothing` would fail: the Mixer page primes `Block{0,0}`, which the bridge reads as Pizza SHAPE.)

- [ ] **Step 3: Registry by address**

Replace `chimera-core/src/mod_path.rs` with:
```rust
//! The mod destination registry: parameters a patch has primed for
//! modulation, by semantic address (spec §2, §4).

use crate::addr::ParamAddr;

pub const MAX_REGISTRY_DESTS: usize = 32;
pub const LABEL_LEN: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct ModDestEntry {
    pub addr: ParamAddr,
    pub label: [u8; LABEL_LEN],
}

impl ModDestEntry {
    pub fn label_str(&self) -> &str {
        let end = self.label.iter().position(|&b| b == 0).unwrap_or(LABEL_LEN);
        core::str::from_utf8(&self.label[..end]).unwrap_or("???")
    }
}

/// Why `ModDestRegistry::add` refused a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// The address's spec is not modulatable.
    NotModulatable,
    Full,
}

#[derive(Clone)]
pub struct ModDestRegistry {
    entries: [Option<ModDestEntry>; MAX_REGISTRY_DESTS],
    count: usize,
}

impl ModDestRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_REGISTRY_DESTS],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Prime `addr` as a mod destination. Refuses non-modulatable addresses
    /// (spec §4). Priming an already primed address is a no-op success.
    pub fn add(&mut self, addr: ParamAddr, label: [u8; LABEL_LEN]) -> Result<(), RegistryError> {
        if !addr.modulatable() {
            return Err(RegistryError::NotModulatable);
        }
        if self.is_primed(addr) {
            return Ok(());
        }
        if self.count >= MAX_REGISTRY_DESTS {
            return Err(RegistryError::Full);
        }
        self.entries[self.count] = Some(ModDestEntry { addr, label });
        self.count += 1;
        Ok(())
    }

    pub fn remove(&mut self, addr: ParamAddr) {
        if let Some(i) = self.find(addr) {
            // Shift remaining entries down
            for j in i..self.count - 1 {
                self.entries[j] = self.entries[j + 1];
            }
            self.entries[self.count - 1] = None;
            self.count -= 1;
        }
    }

    pub fn find(&self, addr: ParamAddr) -> Option<usize> {
        (0..self.count).find(|&i| self.entries[i].is_some_and(|e| e.addr == addr))
    }

    pub fn is_primed(&self, addr: ParamAddr) -> bool {
        self.find(addr).is_some()
    }

    pub fn get(&self, index: usize) -> Option<&ModDestEntry> {
        if index < self.count {
            self.entries[index].as_ref()
        } else {
            None
        }
    }
}

impl Default for ModDestRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

`chimera-core/src/modulation.rs`:
1. Imports: `use crate::mod_path::{legacy_to_addr, ModDestRegistry};` and `use crate::preset::ChainType;` → `use crate::mod_path::ModDestRegistry;`
2. `pub fn from_registry(registry: &ModDestRegistry, chain: ChainType, num_sources: usize) -> Self` → `pub fn from_registry(registry: &ModDestRegistry, num_sources: usize) -> Self`, and inside it `ms.push_dest(legacy_to_addr(chain, entry.path));` → `ms.push_dest(Some(entry.addr));`
3. `pub fn sync_from_matrix(&mut self, matrix: &MatrixState, chain: ChainType)` → `pub fn sync_from_matrix(&mut self, matrix: &MatrixState)`, and `let addr = matrix.dests[di].as_ref().and_then(|d| legacy_to_addr(chain, d.path));` → `let addr = matrix.dests[di].map(|d| d.addr);`

`chimera-core/src/ui/mod_grid.rs`:
1. `use crate::mod_path::{ParamPath, LABEL_LEN};` → `use crate::addr::ParamAddr;` + `use crate::mod_path::LABEL_LEN;`
2. `ModDest`: doc → `/// A destination in the mod matrix — a primed param.`; field `pub path: ParamPath,` → `pub addr: ParamAddr,`; in `rebuild_dests_from_registry`, `path: entry.path,` → `addr: entry.addr,`
3. Delete `mod_info_for_param` (with its doc comment) and rename/retype `mod_info_for_path`:
```rust
    /// Whether `addr` is a mod destination, and its summed amount (−1..1).
    /// `None` = not primed; `Some(0.0)` = primed with no amounts set.
    pub fn mod_info_for(&self, addr: ParamAddr) -> Option<f32> {
```
with `if dest.path == path {` → `if dest.addr == addr {` in its body.

`chimera-core/src/preset.rs`: `use crate::mod_path::{ModDestRegistry, ParamPath};` → `use crate::addr::{BlockRef, Op, ParamAddr};` + `use crate::mod_path::ModDestRegistry;`; the pre-wire loop becomes
```rust
                for (op, label) in [(Op::A, *b"O1 Lvl\0\0"), (Op::B, *b"O2 Lvl\0\0"), (Op::C, *b"O3 Lvl\0\0"), (Op::D, *b"O4 Lvl\0\0")] {
                    let _ = reg.add(ParamAddr::new(BlockRef::FmOp(op), crate::params::FmOpParams::LEVEL), label);
                }
                let mut ms = ModState::from_registry(&reg, 4);
```

- [ ] **Step 4: The UI builds addresses with `slot_addr`**

`chimera-core/src/ui/mod.rs`:
1. Imports: `use crate::mod_path::ParamPath;` → `use crate::addr::{BlockRef, ParamAddr};` + `use crate::mod_path::LABEL_LEN;`; add `use block_def::slot_addr;` next to `use chain::{…};`.
2. Replace `fn current_param_path` and `fn current_param_label` (with their doc comments) by:
```rust
    /// The address the focused encoder edits, if its slot is bound. Mixer,
    /// System and Demo slots are `Legacy`, so priming there does nothing.
    fn current_param_addr(&self) -> Option<ParamAddr> {
        slot_addr(self.nav.active_block_def(), self.last_encoder, page::selected_op())
    }

    /// 8-byte matrix column label for a primed destination: `O<n> ` + spec
    /// label for FM operator params, else the page's short name (≤ 3 chars)
    /// + the slot label.
    fn mod_label(&self, addr: ParamAddr) -> [u8; LABEL_LEN] {
        let def = self.nav.active_block_def();
        let op_prefix;
        let (prefix, name): (&[u8], &str) = match addr.block {
            BlockRef::FmOp(op) => {
                op_prefix = [b'O', b'1' + op.index() as u8, b' '];
                (&op_prefix, addr.spec().map_or("", |s| s.label))
            }
            _ => {
                let short = def.short.as_bytes();
                (&short[..short.len().min(3)], def.params[self.last_encoder].label())
            }
        };
        let mut label = [0u8; LABEL_LEN];
        label[..prefix.len()].copy_from_slice(prefix);
        let rest = name.as_bytes();
        let rlen = rest.len().min(LABEL_LEN - prefix.len());
        label[prefix.len()..prefix.len() + rlen].copy_from_slice(&rest[..rlen]);
        label
    }
```
3. Replace the two `if controls.button_state(ButtonId::Plus) …` / `ButtonId::Minus …` blocks inside `if shift { … }` with:
```rust
                if controls.button_state(ButtonId::Plus) == ButtonState::Pressed {
                    // Unbound slots prime nothing; non-modulatable params are refused.
                    if let Some(addr) = self.current_param_addr() {
                        let label = self.mod_label(addr);
                        let _ = self.project.tracks[at].patch.dest_registry.add(addr, label);
                        self.matrix_state.rebuild_dests_from_registry(
                            &self.project.tracks[at].patch.dest_registry
                        );
                        self.sync_mod_state(at);
                    }
                }
                if controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
                    if let Some(addr) = self.current_param_addr() {
                        self.project.tracks[at].patch.dest_registry.remove(addr);
                        self.matrix_state.rebuild_dests_from_registry(
                            &self.project.tracks[at].patch.dest_registry
                        );
                        self.sync_mod_state(at);
                    }
                }
```
4. `sync_mod_state` body: `patch.mod_state.sync_from_matrix(&self.matrix_state, patch.chain_type);` → `patch.mod_state.sync_from_matrix(&self.matrix_state);`
5. In `update`, replace
```rust
            // Apply offsets to the 6 display values
            for i in 0..6 {
                let offset = self.page.binding(i).map_or(0.0, |a| patch.mod_state.offset_for(a, &mod_sources));
```
with
```rust
            // Apply offsets to the 6 display values
            let def = self.nav.active_block_def();
            let sel_op = page::selected_op();
            for i in 0..6 {
                let offset = slot_addr(def, i, sel_op).map_or(0.0, |a| patch.mod_state.offset_for(a, &mod_sources));
```

`chimera-core/src/ui/renderer.rs`:
1. `use crate::ui::block_def::{BlockDef, VizType};` → `use crate::ui::block_def::{slot_addr, BlockDef, VizType};`
2. Replace `fn param_path_for_cell` (and its doc comment) with
```rust
    /// Mod-bar amount for slot `i` of `def`, if that param is a destination.
    fn cell_mod_info(def: &BlockDef, i: usize, matrix_state: &MatrixState) -> Option<f32> {
        slot_addr(def, i, crate::ui::page::selected_op()).and_then(|a| matrix_state.mod_info_for(a))
    }
```
3. In `draw_params_from_def` and `draw_cell_grid_from_def`: delete the `block_idx: usize,` parameter and replace both
```rust
            let mod_path = self.param_path_for_cell(block_idx, i);
            let mod_info = matrix_state.mod_info_for_path(mod_path);
```
with `let mod_info = Self::cell_mod_info(def, i, matrix_state);`. The four call sites `(display, def, nav.node, matrix_state)` become `(display, def, matrix_state)`.

- [ ] **Step 5: Tests follow the new API (mechanical)**

`chimera-core/tests/common/mod.rs`: `use chimera_core::mod_path::{ModDestRegistry, ParamPath};` → `use chimera_core::addr::{BlockRef, Op, ParamAddr};` + `use chimera_core::mod_path::ModDestRegistry;`; add `FilterParams, FmOpParams` to the `chimera_core::params` import; replace the block from `/// Filter cutoff on each chain's UI path.` up to `/// Params + ModState for a case` with:
```rust
/// Filter cutoff — the same semantic address on every chain.
pub const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
/// FM operator A level.
pub const OP_A_LEVEL: ParamAddr = ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::LEVEL);

/// One LFO (source 1) route at MOD_AMOUNT to `dest`; env is source 0 so
/// `num_sources >= 2` and the LFO runs.
pub fn lfo_route(dest: ParamAddr) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(dest, *b"GOLDEN\0\0").expect("golden destination must be modulatable");
    let mut ms = ModState::from_registry(&reg, 2);
    ms.set_amount(1, 0, MOD_AMOUNT);
    ms
}
```
and in `setup`: `|engine: EngineType, dest: (ChainType, ParamPath)|` → `|engine: EngineType, dest: ParamAddr|`; `PIZZA_CUTOFF`, `FM_CUTOFF`, `MODAL_CUTOFF` → `CUTOFF`.

`chimera-core/tests/modulatable_test.rs`: delete `fn ui_path_for` (with its doc comment), replace `fn lfo_route` with
```rust
fn lfo_route(addr: ParamAddr) -> ModState {
    let mut reg = ModDestRegistry::new();
    reg.add(addr, *b"TEST\0\0\0\0").expect("modulatable");
    let mut ms = ModState::from_registry(&reg, 2);
    ms.set_amount(1, 0, 127);
    ms
}
```
and change the imports `use chimera_core::mod_path::{legacy_to_addr, ModDestRegistry, ParamPath};` → `use chimera_core::mod_path::ModDestRegistry;`, deleting `use chimera_core::preset::ChainType;`.

`chimera-core/tests/modulation_integration_test.rs` — run from `chimera-core/tests`:
```bash
cd chimera-core/tests
python3 - <<'PY'
p = open('modulation_integration_test.rs').read()
start = p.index('    // Modulated: LFO (source 1) -> filter cutoff (Pizza chain node 2, slot 0)')
end = p.index('    voice_dry.note_on(')
p = p[:start] + """    // Modulated: LFO (source 1) -> filter cutoff
    let mut registry = chimera_core::mod_path::ModDestRegistry::new();
    registry.add(CUTOFF, *b"FLTCUT\\0\\0").unwrap();
    let mut mod_state = ModState::from_registry(&registry, 2); // env, LFO
    mod_state.set_amount(1, 0, 100); // LFO -> cutoff at high amount

""" + p[end:]
p = p.replace('use chimera_core::preset::ChainType;\n', 'use chimera_core::addr::{BlockRef, ParamAddr};\nuse chimera_core::dsp::pizza::PizzaParams;\nuse chimera_core::params::{DriveParams, FilterParams};\n')
p = p.replace('use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};\n', """use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const DRIVE: ParamAddr = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
const CRUSH: ParamAddr = ParamAddr::new(BlockRef::Pizza, PizzaParams::CRUSH);
const SHAPE: ParamAddr = ParamAddr::new(BlockRef::Pizza, PizzaParams::SHAPE);
""")
p = p.replace('use chimera_core::mod_path::{ModDestRegistry, ParamPath};', 'use chimera_core::mod_path::ModDestRegistry;')
for old, new in [
    ('matrix.mod_info_for_param(1, 0)', 'matrix.mod_info_for(DRIVE)'),
    ('matrix.mod_info_for_param(0, 1)', 'matrix.mod_info_for(CRUSH)'),
    ('ChainType::PizzaPoly, ParamPath::Block { block: 1, param: 0 }, *b"B1 Prm0', 'DRIVE, *b"B1 Prm0'),
    ('ChainType::PizzaPoly, ParamPath::Block { block: 0, param: 1 }, *b"TSTaPrm', 'CRUSH, *b"TSTaPrm'),
    ('ChainType::PizzaPoly, ParamPath::Block { block: 0, param: 0 }, *b"PIZShape', 'SHAPE, *b"PIZShape'),
    ('ChainType::PizzaPoly, ParamPath::Block { block: 1, param: 0 }, *b"FLT Freq', 'DRIVE, *b"FLT Freq'),
    ('assert_eq!(dest0.path, ParamPath::Block { block: 0, param: 0 });', 'assert_eq!(dest0.addr, SHAPE);'),
    ('assert_eq!(dest1.path, ParamPath::Block { block: 1, param: 0 });', 'assert_eq!(dest1.addr, DRIVE);'),
]:
    assert old in p, old
    p = p.replace(old, new)
open('modulation_integration_test.rs', 'w').write(p)
PY
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
grep -rn 'ParamPath\|legacy_to_addr' chimera-core/src chimera-core/tests   # must print nothing
```

- [ ] **Step 6: Run everything**

Run: `cargo test -p chimera-core --test modulation_test --test mod_registry_test --test ui_routing_test --test modulatable_test --test modulation_integration_test --test golden_test 2>&1 | grep '^test result'`
Expected: `10 passed`, `7 passed`, `3 passed`, `1 passed`, `6 passed`, `4 passed`, all `ok`. Full suite: `0 failed`.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/mod_path.rs chimera-core/src/modulation.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/preset.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/renderer.rs chimera-core/tests/
git commit -m "refactor(ui): mod routes are ParamAddrs end to end; delete ParamPath

The UI builds addresses from slot bindings, so priming on Mixer/System/Demo
pages registers nothing and FM operator routes name a concrete operator.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 20: `PageKey` and generic Part pages; selected operator moves into `UiState`; delete Part `PageId`s

**Files:**
- Modify: `chimera-core/src/ui/page.rs` (imports; replace from `/// Identifies which page is active` to end of file)
- Create: `chimera-core/src/ui/part_page.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod part_page;`, `page: PageKey`, `sel_op`, encoder dispatch, `page_values`)
- Modify: `chimera-core/src/ui/renderer.rs` (drop `current_page`/`update`; `snap_to_current(values)`; `sel_op` argument)
- Modify: `chimera-core/src/ui/region.rs` (`PageKey` in snapshots)
- Create: `chimera-core/tests/part_page_test.rs`; Replace: `chimera-core/tests/page_block_test.rs` (legacy pages only)
- Modify: `chimera-core/tests/{ui_test,region_tests,ui_routing_test}.rs`

**Interfaces:**
- Consumes: Task 18 (`SlotBinding`, `slot_addr`, `BlockDef.id`), Task 19 (`current_param_addr`), Task 14 (`Op::nudged`).
- Produces:
  - `pub enum PageKey { Part { def: u16, op: Op }, Legacy(PageId) }` (`Clone, Copy, Debug, PartialEq, Eq`), `PageKey::from_nav(nav: &ChainNav, sel_op: Op) -> PageKey`
  - `PageId` = `Mixer, Chorus, Delay, MixReverb, Master, EnvAmp, EnvFilter, EnvAux, System, DemoWaves, DemoShapes, DemoMotion, DemoFm, DemoMatrix`; `PageId::from_nav(nav) -> Option<PageId>` (`None` on Part chains)
  - `ui::part_page::{read_values(def: &BlockDef, params: &ParamSnapshot, sel_op: Op) -> [f32; 6], apply_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut ParamSnapshot, sel_op: &mut Op), snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut ParamSnapshot, sel_op: Op)}`
  - `UiState::page(&self) -> PageKey`, `UiState::selected_op(&self) -> Op`
  - `Renderer::snap_to_current(&mut self, values: [f32; 6])`; `draw_with_def(…, matrix_state, sel_op: Op)`, `draw_region_with_def(…, matrix_state, sel_op: Op)`, `draw_params_from_def(display, def, sel_op, matrix_state)`, `draw_cell_grid_from_def(display, def, sel_op, matrix_state)`
  - `RegionData::{viz, params, cells}` take `PageKey`
  - Deleted: `page::selected_op`, `FM_SEL_OP`, `Renderer::update`, `Renderer::current_page`, `PageId::{Pizza, EngineModal1, EngineModal2, Drive, Filter, Folder, Vca, Efx, Lfo, FmAlg, FmOp, FmRatio, FmEnv1..4}`, `PageId::from_part_nav`

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/part_page_test.rs` (the Part-page parity tests move here from `page_block_test.rs`):
```rust
//! Part-chain pages driven by slot bindings (spec §5): encoders, shift-snap
//! and display go through the bound param's spec. Parity tests pin today's
//! step sizes (plan § Encoder step audit).

use chimera_core::addr::Op;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::part_page;

/// One encoder turn with operator A selected.
fn turn(def: &BlockDef, slot: usize, delta: i8, p: &mut ParamSnapshot) {
    let mut op = Op::A;
    part_page::apply_encoder(def, slot, delta, p, &mut op);
}

fn snap(def: &BlockDef, slot: usize, delta: i8, p: &mut ParamSnapshot) {
    part_page::snap_encoder(def, slot, delta, p, Op::A);
}

fn read(def: &BlockDef, p: &ParamSnapshot) -> [f32; 6] {
    part_page::read_values(def, p, Op::A)
}

#[test]
fn pizza_page() {
    let mut p = ParamSnapshot::default();
    assert_eq!(read(&reg::PIZZA, &p), [0.5, 0.0, 0.8, 0.0, 0.0, 0.0]);
    turn(&reg::PIZZA, 0, 3, &mut p);
    assert_eq!(p.pizza.shape, 0.5 + 3.0 * (1.0 / 128.0));
    turn(&reg::PIZZA, 2, 127, &mut p);
    assert_eq!(p.pizza.level, 1.0);
    snap(&reg::PIZZA, 1, 1, &mut p); // shift-snap works on Pizza (spec)
    assert_eq!(p.pizza.crush, 100.0 / 127.0);
    turn(&reg::PIZZA, 4, 1, &mut p); // empty slot: nothing happens
}

#[test]
fn modal_pages() {
    let mut p = ParamSnapshot::default();
    turn(&reg::MODAL_1, 1, 2, &mut p);
    assert_eq!(p.modal.excite, 0.8 + 2.0 * (1.0 / 128.0));
    turn(&reg::MODAL_2, 5, -1, &mut p);
    assert_eq!(p.modal.ks_ens_mix, 0.0);
    // Plan D3: MODE reaches Sympathetic; Review Focus 3: snap lands on a choice.
    p.modal.mode = ResonatorMode::Bowed;
    assert_eq!(read(&reg::MODAL_1, &p)[0], 2.0 / 3.0);
    turn(&reg::MODAL_1, 0, 1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::Sympathetic);
    snap(&reg::MODAL_1, 0, -1, &mut p);
    assert_eq!(p.modal.mode, ResonatorMode::String);
}

#[test]
fn drive_filter_folder_pages() {
    let mut p = ParamSnapshot::default();
    assert_eq!(read(&reg::DRIVE, &p), [0.0, 0.5, 1.0, 0.0, 0.0, 0.0]);
    turn(&reg::DRIVE, 0, 5, &mut p);
    assert_eq!(p.drive.drive, 5.0 * ((1.0 - 0.0) / 128.0));
    snap(&reg::DRIVE, 1, 1, &mut p);
    assert_eq!(p.drive.tone, 107.0 / 127.0);

    assert_eq!(read(&reg::FILTER, &p), [1.0, 0.0, 0.0, 0.0, 0.5, 0.0]);
    turn(&reg::FILTER, 0, -1, &mut p);
    assert_eq!(p.filter.cutoff, 20000.0 - (20000.0 - 20.0) / 128.0);
    turn(&reg::FILTER, 4, 1, &mut p);
    assert_eq!(p.filter.env_amount, (1.0 - -1.0) / 128.0);

    turn(&reg::FOLDER, 0, 4, &mut p);
    assert_eq!(p.folder.fold, 4.0 / 128.0);
}

#[test]
fn envelope_and_lfo_pages() {
    let mut p = ParamSnapshot::default();
    turn(&reg::ENVELOPE, 0, 1, &mut p);
    assert_eq!(p.envelopes[0].attack, 0.01 + (10.0 - 0.001) / 128.0);
    turn(&reg::ENVELOPE, 2, -1, &mut p);
    assert_eq!(p.envelopes[0].sustain, 0.7 - 1.0 / 128.0);

    assert_eq!(read(&reg::LFO, &p)[0], (1.0 - 0.01) / (20.0 - 0.01));
    turn(&reg::LFO, 0, 2, &mut p);
    assert_eq!(p.lfo.rate, 1.0 + 2.0 * 0.15);
    turn(&reg::LFO, 1, 9, &mut p);
    assert_eq!(p.lfo.shape, 4);
    turn(&reg::LFO, 5, 3, &mut p);
    assert_eq!(p.lfo.offset, 3.0 * (1.0 / 128.0) * 2.0);
    snap(&reg::LFO, 1, -1, &mut p); // shift-snap works on LFO (spec)
    assert_eq!(p.lfo.shape, 0);
}

#[test]
fn fm_operator_page_follows_the_selection() {
    let mut p = ParamSnapshot::default();
    let mut op = Op::A;
    part_page::apply_encoder(&reg::FM_OP, 0, 1, &mut p, &mut op); // selector
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&reg::FM_OP, 0, 9, &mut p, &mut op);
    assert_eq!(op, Op::D);
    part_page::apply_encoder(&reg::FM_OP, 0, -2, &mut p, &mut op);
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&reg::FM_OP, 2, 5, &mut p, &mut op);
    assert_eq!(p.fm.operators[1].level, 5.0);
    part_page::apply_encoder(&reg::FM_OP, 4, -9, &mut p, &mut op);
    assert_eq!(p.fm.operators[1].detune, -7);
    part_page::apply_encoder(&reg::FM_RATIO, 4, 1, &mut p, &mut op); // FINE of B
    assert_eq!(p.fm.operators[1].fine, 1);
    assert_eq!(part_page::read_values(&reg::FM_OP, &p, op)[0], 1.0 / 3.0);
    // Review Focus 3: snapping a Stepped level lands on an integer.
    part_page::snap_encoder(&reg::FM_OP, 2, 1, &mut p, op);
    assert_eq!(p.fm.operators[1].level, 78.0); // 99 * 100/127 = 77.95 → 78
    part_page::snap_encoder(&reg::FM_OP, 0, 1, &mut p, op); // selector: no snap
    assert_eq!(op, Op::B);
}

#[test]
fn fm_fixed_pages() {
    let mut p = ParamSnapshot::default();
    turn(&reg::FM_ALG, 0, 9, &mut p);
    assert_eq!(p.fm.algorithm, 7);
    turn(&reg::FM_ALG, 2, 1, &mut p); // LEVEL = voice output volume
    assert_eq!(p.out.volume, 0.8 + 1.0 / 128.0);
    turn(&reg::FM_RATIO, 2, 1, &mut p);
    assert_eq!(p.fm.operators[2].coarse, 5);
    snap(&reg::FM_RATIO, 0, 1, &mut p);
    assert_eq!(p.fm.operators[0].coarse, 63);
    turn(&reg::FM_ENV3, 2, -1, &mut p);
    assert_eq!(p.fm.operators[2].decay1_level, 14);
    turn(&reg::FM_ENV2, 4, -20, &mut p); // plan D4: RR reaches 0
    assert_eq!(p.fm.operators[1].release_rate, 0);
    snap(&reg::FM_ENV1, 0, -1, &mut p);
    assert_eq!(p.fm.operators[0].attack_rate, 0);
}
```

Replace `chimera-core/tests/page_block_test.rs` with (legacy pages only):
```rust
//! Legacy (`PageId`) pages — Mixer, System, Demo — after moving onto
//! `Block` specs and `ParamAddr` bindings. Parity tests pin today's steps.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::params::{EnvParams, FilterParams, FmOpParams, OutParams, ParamSnapshot};
use chimera_core::ui::page::PageId;

#[test]
fn demo_pages_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::DemoWaves.apply_encoder(1, -2, &mut p);
    assert_eq!(p.drive.tone, 0.5 - 2.0 * (1.0 / 128.0));
    PageId::DemoWaves.apply_encoder(3, -1, &mut p);
    assert_eq!(p.folder.symmetry, 0.5 - 1.0 / 128.0);
    PageId::DemoShapes.apply_encoder(4, 3, &mut p);
    assert_eq!(p.filter.resonance, 3.0 / 128.0);
    PageId::DemoMotion.apply_encoder(4, 1, &mut p);
    assert_eq!(p.envelopes[1].attack, 0.01 + (10.0 - 0.001) / 128.0);
    PageId::DemoFm.apply_encoder(3, 2, &mut p);
    assert_eq!(p.fm.operators[2].feedback, 2.0);
    PageId::EnvAux.apply_encoder(3, -128, &mut p);
    assert_eq!(p.envelopes[2].release, 0.001);
}

#[test]
fn out_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Mixer.apply_encoder(0, -8, &mut p);
    assert_eq!(p.out.volume, 0.8 - 8.0 / 128.0);
    PageId::Master.apply_encoder(1, 1, &mut p);
    assert_eq!(p.out.pan, 2.0 / 128.0);
}

/// The mixer page keeps its placeholder bars for unbound slots.
#[test]
fn mixer_read_values_keep_placeholders() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Mixer.read_values(&p), [0.8, 0.5, 0.5, 0.0, 0.5, 0.0]);
}

#[test]
fn fx_encoders_step_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Delay.apply_encoder(0, 2, &mut p);
    assert_eq!(p.delay.time_ms, 375.0 + 2.0 * 8.0);
    PageId::Chorus.apply_encoder(0, 5, &mut p);
    assert_eq!(p.chorus.mode, 3);
    PageId::MixReverb.apply_encoder(0, 5, &mut p);
    assert_eq!(p.reverb.reverb_type, 2);
    PageId::MixReverb.apply_encoder(4, -1, &mut p);
    assert_eq!(p.reverb.mix, 0.0);
    PageId::Delay.snap_encoder(5, 1, &mut p);
    assert_eq!(p.delay.mix, 100.0 / 127.0);
}

#[test]
fn legacy_bindings_name_semantic_addresses() {
    // Spec §5: Demo pages address envelopes[1] via FilterEnv.
    assert_eq!(
        PageId::DemoMotion.binding(4),
        Some(ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK))
    );
    assert_eq!(
        PageId::DemoFm.binding(1),
        Some(ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::FEEDBACK))
    );
    assert_eq!(PageId::DemoShapes.binding(1), Some(ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)));
    assert_eq!(PageId::Master.binding(0), Some(ParamAddr::new(BlockRef::Out, OutParams::VOLUME)));
    assert_eq!(PageId::Mixer.binding(2), None);
    assert_eq!(PageId::DemoMatrix.binding(0), None);
    // Spec §5: System has its own page with no editable params.
    for i in 0..6 {
        assert_eq!(PageId::System.binding(i), None);
    }
}

/// Every bound slot of every legacy page resolves to a spec.
#[test]
fn every_legacy_binding_has_a_spec() {
    let pages = [
        PageId::Mixer, PageId::Chorus, PageId::Delay, PageId::MixReverb, PageId::Master,
        PageId::EnvAmp, PageId::EnvFilter, PageId::EnvAux, PageId::DemoWaves, PageId::DemoShapes,
        PageId::DemoMotion, PageId::DemoFm, PageId::DemoMatrix, PageId::System,
    ];
    for page in pages {
        for i in 0..6 {
            if let Some(a) = page.binding(i) {
                assert!(a.spec().is_some(), "{page:?} slot {i}: {a:?} has no spec");
            }
        }
    }
}
```

In `chimera-core/tests/ui_test.rs`, change the import `use chimera_core::ui::page::{PageId, ValFmt};` to
```rust
use chimera_core::addr::Op;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::page::{PageId, PageKey, ValFmt};
```
and replace the three tests `test_chain_nav_starts_at_part0_engine`, `test_page_from_nav_part_chain`, `test_page_from_nav_demo_chain` with:
```rust
fn part(def: &BlockDef) -> PageKey {
    PageKey::Part { def: def.id, op: Op::A }
}

#[test]
fn test_chain_nav_starts_at_part0_engine() {
    let nav = ChainNav::new();
    assert_eq!(nav.chain_id, ChainId::Part(0));
    assert_eq!(nav.node, 0);
    assert_eq!(nav.sub_page, 0);
    assert_eq!(PageKey::from_nav(&nav, Op::A), part(&reg::PIZZA));
}

#[test]
fn test_page_from_nav_part_chain() {
    let mut nav = ChainNav::new();
    for (node, def) in [(0, &reg::PIZZA), (1, &reg::DRIVE), (2, &reg::FILTER), (3, &reg::FOLDER), (4, &reg::MOD_MATRIX)] {
        nav.node = node;
        assert_eq!(PageKey::from_nav(&nav, Op::A), part(def), "node {node}");
    }
    nav.sub_page = 1;
    assert_eq!(PageKey::from_nav(&nav, Op::A), part(&reg::ENVELOPE)); // Envelope at sub_page 1
    nav.sub_page = 2;
    assert_eq!(PageKey::from_nav(&nav, Op::A), part(&reg::LFO)); // LFO at sub_page 2
    // The operator selection is part of a Part page's identity.
    assert_ne!(PageKey::from_nav(&nav, Op::B), PageKey::from_nav(&nav, Op::A));
}

#[test]
fn test_page_from_nav_demo_chain() {
    let mut nav = ChainNav::new();
    nav.chain_id = ChainId::Demo;
    nav.node = 0;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::DemoWaves));
    nav.node = 1;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::DemoShapes));
    nav.node = 2;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::DemoMotion));
}

/// Spec §5: System gets its own page (it used to alias the Pizza page).
#[test]
fn test_system_chain_has_its_own_page() {
    let mut nav = ChainNav::new();
    nav.chain_id = ChainId::System;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::System));
}
```

Append to `chimera-core/tests/ui_routing_test.rs`:
```rust
/// Review Focus 5 / spec §5: a route primed on a `SelectedOp` slot names the
/// operator selected at that moment; changing the selection later does not
/// retarget it.
#[test]
fn selected_op_route_is_concrete() {
    use chimera_core::addr::Op;
    use chimera_core::params::FmOpParams;
    use chimera_core::preset::POOL_SIZE;

    let mut ui = UiState::new();
    // Load "(init) FM" into track 1 via the patch browser.
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 2) as i8));
    press(&mut ui, ButtonId::Edit);
    press(&mut ui, ButtonId::Edit); // FM node → Operator sub-page
    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1)); // select op B
    assert_eq!(ui.selected_op(), Op::B);
    ui.handle_input(&MockControls::new().encoder(EncoderId::D, 1)); // FDBK slot
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
    let fdbk_b = ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::FEEDBACK);
    assert!(primed(&ui).contains(&fdbk_b));

    ui.handle_input(&MockControls::new().encoder(EncoderId::A, 1)); // select op C
    assert_eq!(ui.selected_op(), Op::C);
    assert!(primed(&ui).contains(&fdbk_b));
    assert!(!primed(&ui).contains(&ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK)));
    assert_eq!(ui.project.tracks[0].patch.params.fm.operators[1].feedback, 1.0);
}
```

`chimera-core/tests/region_tests.rs` (mechanical):
```bash
sed -i 's/PageId::Filter/PageKey::Legacy(PageId::Mixer)/g; s/^use chimera_core::ui::page::{PageId, PageLayout};/use chimera_core::ui::page::{PageId, PageKey, PageLayout};/' chimera-core/tests/region_tests.rs
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test part_page_test --test ui_test --test ui_routing_test 2>&1 | grep -E '^error\[' | sort | uniq -c | head`
Expected: `could not find part_page in ui`, `cannot find type PageKey`, `no method named selected_op found for struct UiState`.

- [ ] **Step 3: Slot-driven Part pages**

Create `chimera-core/src/ui/part_page.rs`:
```rust
//! Encoders and display for Part-chain pages, driven by the `BlockDef`'s
//! slot bindings (spec §5): label, format, step and range all come from the
//! bound param's spec.

use crate::addr::Op;
use crate::params::ParamSnapshot;
use crate::ui::block_def::{slot_addr, BlockDef, SlotBinding};

/// Normalized (0..1) display values of the six slots.
pub fn read_values(def: &BlockDef, params: &ParamSnapshot, sel_op: Op) -> [f32; 6] {
    core::array::from_fn(|i| match def.params[i].binding {
        SlotBinding::SelectOp => sel_op.index() as f32 / 3.0,
        _ => slot_addr(def, i, sel_op).map_or(0.0, |a| params.block(a.block).normalized(a.param)),
    })
}

/// One encoder turn on `slot`: steps the bound param, or the operator
/// selection for the `SelectOp` slot.
pub fn apply_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut ParamSnapshot, sel_op: &mut Op) {
    if def.params.get(slot).is_some_and(|s| s.binding == SlotBinding::SelectOp) {
        *sel_op = sel_op.nudged(delta);
    } else if let Some(a) = slot_addr(def, slot, *sel_op) {
        params.block_mut(a.block).nudge(a.param, delta);
    }
}

/// Shift+encoder on `slot`: snap the bound param (the selector does not snap).
pub fn snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut ParamSnapshot, sel_op: Op) {
    if let Some(a) = slot_addr(def, slot, sel_op) {
        params.block_mut(a.block).snap(a.param, delta);
    }
}
```
Register it in `chimera-core/src/ui/mod.rs`: `pub mod part_page;` after `pub mod page;`.

- [ ] **Step 4: Shrink `PageId`, add `PageKey`**

In `chimera-core/src/ui/page.rs`, replace the import block (everything above `pub use crate::block::ValFmt;`) with
```rust
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::reverb::ReverbParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams, ParamSnapshot};
use crate::ui::chain::ChainNav;

```
keep `pub use crate::block::ValFmt;`, `PageLayout` and `CellIcon` unchanged, and replace everything from `/// Identifies which page is active, derived from chain position.` to the end of the file with:
```rust
/// Pages still driven by `PageId`: Mixer, System and Demo (spec §5).
/// Part-chain pages are identified by `PageKey::Part` and driven by their
/// `BlockDef` slot bindings (`ui::part_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    Mixer,
    Chorus,
    Delay,
    MixReverb,
    Master,
    /// Standalone envelope pages (not reachable from any chain today).
    EnvAmp,
    EnvFilter,
    EnvAux,
    /// System chain: no editable params yet.
    System,
    DemoWaves,
    DemoShapes,
    DemoMotion,
    DemoFm,
    DemoMatrix,
}

/// Page identity for the renderer and dirty-region tracking (spec §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKey {
    /// A Part-chain page by `BlockDef::id` (defs like FILTER are shared
    /// across chains), with the FM operator selection so a selection change
    /// redraws the page.
    Part { def: u16, op: Op },
    /// Mixer/System/Demo pages.
    Legacy(PageId),
}

impl PageKey {
    pub fn from_nav(nav: &ChainNav, sel_op: Op) -> Self {
        match PageId::from_nav(nav) {
            Some(page) => PageKey::Legacy(page),
            None => PageKey::Part { def: nav.active_block_def().id, op: sel_op },
        }
    }
}

impl PageId {
    /// The legacy page at the current navigation position; `None` on a
    /// Part chain (see `PageKey::from_nav`).
    pub fn from_nav(nav: &ChainNav) -> Option<Self> {
        use crate::ui::chain::ChainId;
        Some(match nav.chain_id {
            ChainId::Part(_) => return None,
            ChainId::Mixer(_) => match nav.node {
                0 => PageId::Mixer,
                1 => PageId::Chorus,
                2 => PageId::Delay,
                3 => PageId::MixReverb,
                _ => PageId::Master,
            },
            ChainId::System => PageId::System,
            ChainId::Demo => match nav.node {
                0 => PageId::DemoWaves,
                1 => PageId::DemoShapes,
                2 => PageId::DemoMotion,
                3 => PageId::DemoFm,
                _ => PageId::DemoMatrix,
            },
        })
    }

    /// The parameter bound to encoder `idx` on this page, if any.
    pub fn binding(&self, idx: usize) -> Option<ParamAddr> {
        use BlockRef as B;
        let at = |block: BlockRef, ids: &[ParamId]| ids.get(idx).map(|&param| ParamAddr::new(block, param));
        match self {
            PageId::EnvAmp => at(B::AmpEnv, &ENV_PAGE),
            PageId::EnvFilter => at(B::FilterEnv, &ENV_PAGE),
            PageId::EnvAux => at(B::AuxEnv, &ENV_PAGE),
            PageId::Mixer | PageId::Master => at(B::Out, &OUT_PAGE),
            PageId::Chorus => at(B::Chorus, &CHORUS_PAGE),
            PageId::Delay => at(B::Delay, &DELAY_PAGE),
            PageId::MixReverb => at(B::Reverb, &REVERB_PAGE),
            PageId::DemoWaves => DEMO_WAVES.get(idx).copied(),
            PageId::DemoShapes => DEMO_SHAPES.get(idx).copied(),
            PageId::DemoMotion => DEMO_MOTION.get(idx).copied(),
            PageId::DemoFm => DEMO_FM.get(idx).copied(),
            PageId::DemoMatrix | PageId::System => None,
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        core::array::from_fn(|i| match (self, i) {
            // Mixer bars for the unbound VOICES and PITCH slots.
            (PageId::Mixer, 2 | 4) => 0.5,
            _ => self.binding(i).map_or(0.0, |a| params.block(a.block).normalized(a.param)),
        })
    }

    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if let Some(a) = self.binding(idx) {
            params.block_mut(a.block).nudge(a.param, delta);
        }
    }

    /// Shift+encoder: snap to the coarse points of the bound param's format.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if let Some(a) = self.binding(idx) {
            params.block_mut(a.block).snap(a.param, delta);
        }
    }
}

/// Encoder slot → param id, for pages bound to a single block.
const ENV_PAGE: [ParamId; 6] = [
    EnvParams::ATTACK,
    EnvParams::DECAY,
    EnvParams::SUSTAIN,
    EnvParams::RELEASE,
    EnvParams::LEVEL,
    EnvParams::VEL_SENS,
];
const OUT_PAGE: [ParamId; 2] = [OutParams::VOLUME, OutParams::PAN];
const CHORUS_PAGE: [ParamId; 4] = [
    ChorusParams::MODE,
    ChorusParams::RATE,
    ChorusParams::DEPTH,
    ChorusParams::MIX,
];
const DELAY_PAGE: [ParamId; 6] = [
    DelayParams::TIME_MS,
    DelayParams::FEEDBACK,
    DelayParams::WOW_FLUTTER,
    DelayParams::SATURATION,
    DelayParams::TONE,
    DelayParams::MIX,
];
const REVERB_PAGE: [ParamId; 5] = [
    ReverbParams::REVERB_TYPE,
    ReverbParams::TIME,
    ReverbParams::DAMPING,
    ReverbParams::SIZE,
    ReverbParams::MIX,
];

/// Demo pages borrow params from several blocks (spec §5: `envelopes[1]` is
/// addressed as `FilterEnv`).
const DEMO_WAVES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
    ParamAddr::new(BlockRef::Drive, DriveParams::TONE),
    ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
    ParamAddr::new(BlockRef::Folder, FolderParams::SYMMETRY),
    ParamAddr::new(BlockRef::Filter, FilterParams::ENV_AMOUNT),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
];
const DEMO_SHAPES: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
    ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF),
    ParamAddr::new(BlockRef::Out, OutParams::PAN),
    ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
    ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
    ParamAddr::new(BlockRef::Filter, FilterParams::FM_AMOUNT),
];
const DEMO_MOTION: [ParamAddr; 6] = [
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::ATTACK),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::DECAY),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::SUSTAIN),
    ParamAddr::new(BlockRef::AmpEnv, EnvParams::RELEASE),
    ParamAddr::new(BlockRef::FilterEnv, EnvParams::ATTACK),
    ParamAddr::new(BlockRef::FilterEnv, EnvParams::DECAY),
];
const DEMO_FM: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Fm, FmParams::ALGORITHM),
    ParamAddr::new(BlockRef::FmOp(Op::A), FmOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::FEEDBACK),
];
```

- [ ] **Step 5: `UiState` owns the operator selection and dispatches by `PageKey`**

In `chimera-core/src/ui/mod.rs`:
1. Imports: `use crate::addr::{BlockRef, ParamAddr};` → `use crate::addr::{BlockRef, Op, ParamAddr};`; `use page::{PageId, PageLayout};` → `use block_def::BlockDef;` + `use page::{PageKey, PageLayout};`
2. Struct: `page: PageId,` → 
```rust
    page: PageKey,
    /// Selected FM operator — one global selection, as before (spec §5).
    sel_op: Op,
```
3. `new()`: 
```rust
        let page = PageKey::from_nav(&nav, Op::A);
        let mut renderer = Renderer::new();
        renderer.snap_to_current(page_values(page, nav.active_block_def(), &project.tracks[0].patch.params, Op::A));
```
and add `sel_op: Op::A,` after `page,` in the struct literal.
4. Replace `pub fn page(&self) -> PageId { … }` with:
```rust
    /// Current page identity.
    pub fn page(&self) -> PageKey {
        self.page
    }

    /// The selected FM operator.
    pub fn selected_op(&self) -> Op {
        self.sel_op
    }

    /// Recompute the page identity and jump the display to its values.
    fn enter_page(&mut self) {
        self.page = PageKey::from_nav(&self.nav, self.sel_op);
        let values = page_values(self.page, self.nav.active_block_def(), self.params(), self.sel_op);
        self.renderer.snap_to_current(values);
    }
```
5. `current_param_addr`: `page::selected_op()` → `self.sel_op`.
6. Patch-browser load: delete `self.page = PageId::from_nav(&self.nav);` and `self.renderer.current_page = self.page;`, and replace `self.renderer.snap_to_current(self.page, &self.project.tracks[sel_track].patch.params);` with `self.enter_page();`. Navigation change: replace the two lines `self.page = PageId::from_nav(&self.nav);` / `self.renderer.snap_to_current(…)` with `self.enter_page();`.
7. Replace the non-matrix encoder loop (from `let page = self.page;` through the end of its `for` loop) with:
```rust
            let at = self.active_track;
            for (i, &enc) in encoder_ids.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta != 0 {
                    self.last_encoder = i;
                    self.renderer.focused = i;
                    let params = &mut self.project.tracks[at].patch.params;
                    match (self.page, shift) {
                        (PageKey::Part { .. }, true) => part_page::snap_encoder(def, i, delta, params, self.sel_op),
                        (PageKey::Part { .. }, false) => part_page::apply_encoder(def, i, delta, params, &mut self.sel_op),
                        (PageKey::Legacy(p), true) => p.snap_encoder(i, delta, params),
                        (PageKey::Legacy(p), false) => p.apply_encoder(i, delta, params),
                    }
                }
            }
            // The operator selection is part of the page identity.
            self.page = PageKey::from_nav(&self.nav, self.sel_op);
```
8. `update`: replace `let mut values = self.page.read_values(&patch.params);` with
```rust
        let def = self.nav.active_block_def();
        let mut values = page_values(self.page, def, &patch.params, self.sel_op);
```
and in the display-offset loop delete `let def = self.nav.active_block_def();` / `let sel_op = page::selected_op();` and use `slot_addr(def, i, self.sel_op)`.
9. `render`: pass `self.sel_op` as the last argument of `draw_with_def`. `render_dirty`: add `let sel_op = self.sel_op;` before `for r in self.region_set.active_regions_mut()` and pass `sel_op` as the last argument of `draw_region_with_def`.
10. Add above `fn nav_tag`:
```rust
/// Display values for `page`: Part pages through slot bindings, legacy pages
/// through `PageId`.
fn page_values(page: PageKey, def: &BlockDef, params: &ParamSnapshot, sel_op: Op) -> [f32; 6] {
    match page {
        PageKey::Part { .. } => part_page::read_values(def, params, sel_op),
        PageKey::Legacy(p) => p.read_values(params),
    }
}
```

- [ ] **Step 6: Renderer and regions take `PageKey`/`sel_op`**

`chimera-core/src/ui/renderer.rs`:
1. Delete `use crate::params::ParamSnapshot;`; `use crate::ui::page::{PageId, PageLayout};` → `use crate::addr::Op;` + `use crate::ui::page::PageLayout;`
2. Delete the `current_page` field (with its doc comment) and its initializer; delete `pub fn update`; replace `snap_to_current` with
```rust
    /// Jump the animated values (page change: nothing to lerp from).
    pub fn snap_to_current(&mut self, values: [f32; 6]) {
        for (a, &v) in self.anim.iter_mut().zip(values.iter()) {
            a.snap(v);
        }
    }
```
3. `cell_mod_info(def, i, matrix_state)` → `cell_mod_info(def: &BlockDef, i: usize, sel_op: Op, matrix_state: &MatrixState)` using `slot_addr(def, i, sel_op)`; its two callers pass `sel_op`.
4. Add a `sel_op: Op,` parameter after `def: &BlockDef,` in `draw_params_from_def` and `draw_cell_grid_from_def`; add `sel_op: Op` as the last parameter of `draw_with_def` and `draw_region_with_def`; the four inner calls become `(display, def, sel_op, matrix_state)`.

`chimera-core/src/ui/region.rs`: `use crate::ui::page::{PageId, PageLayout};` → `use crate::ui::page::{PageId, PageKey, PageLayout};`; every `page: PageId` (the three `RegionData` variants and the `viz`/`params`/`cells` constructors) → `page: PageKey`; add below `const SENTINEL`:
```rust
/// Page used in sentinel snapshots (the SENTINEL values make them unequal).
const SENTINEL_PAGE: PageKey = PageKey::Legacy(PageId::System);
```
and replace the three `page: PageId::Filter` in the sentinel constructors with `page: SENTINEL_PAGE`.

- [ ] **Step 7: Run everything**

Run: `cargo test -p chimera-core --test part_page_test --test page_block_test --test ui_test --test ui_routing_test --test region_tests --test golden_test 2>&1 | grep '^test result'`
Expected: `6`, `6`, `23`, `4`, `19`, `4` passed, all `ok`. Full suite: `0 failed`. `grep -rn 'FM_SEL_OP\|selected_op()\|current_page' chimera-core/src | grep -v 'fn selected_op'` prints nothing.

- [ ] **Step 8: Commit**

```bash
git add chimera-core/src/ui/ chimera-core/tests/
git commit -m "refactor(ui): Part pages are driven by slot bindings and identified by PageKey

The selected FM operator lives in UiState and is passed to the renderer.
Part-page PageId variants are deleted; System gets its own page.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 21: `ChainType::engine` — one source of truth; `ParamSnapshot::engine` becomes private

**Files:**
- Modify: `chimera-core/src/params.rs` (private `engine`, `for_engine`, `engine()`)
- Modify: `chimera-core/src/preset.rs` (`ChainType::engine`; `Patch::init` uses it)
- Modify: `chimera-core/src/dsp/voice.rs:58,79` (`params.engine()`)
- Create: `chimera-core/tests/engine_source_test.rs`
- Modify (mechanical): every test that assigns or reads `.engine` (89 assignment sites at `c1dbd23`, plus the ones Tasks 13/14/16 added): `click_free_test`, `desktop_sim_test`, `engine_switch_test`, `engines_test`, `fm_test`, `live_param_test`, `modal_integration_test`, `modulatable_test`, `property_test`, `reverb_test`, `stress_test`, `common/mod.rs`

**Interfaces:**
- Consumes: Task 13 (`EngineType::ALL`), Task 16 (`ChainType::ALL`).
- Produces: `ChainType::engine(self) -> EngineType` (`const fn`); `ParamSnapshot::for_engine(engine: EngineType) -> ParamSnapshot`; `ParamSnapshot::engine(&self) -> EngineType`; the `engine` field is private (a stray `params.engine = …` no longer compiles).

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/engine_source_test.rs`:
```rust
//! One source of truth for engine choice (spec §6).

use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::{ChainType, Patch};

/// `ChainType::engine` is usable in const context.
const FM_ENGINE: EngineType = ChainType::Fm.engine();

#[test]
fn chain_type_names_its_engine() {
    assert_eq!(ChainType::PizzaPoly.engine(), EngineType::Pizza);
    assert_eq!(ChainType::Modal.engine(), EngineType::Modal);
    assert_eq!(FM_ENGINE, EngineType::Fm);
}

#[test]
fn patch_init_takes_its_engine_from_the_chain() {
    for ct in ChainType::ALL {
        assert_eq!(Patch::init(ct).params.engine(), ct.engine(), "{ct:?}");
    }
}

#[test]
fn for_engine_is_the_defaults_with_that_engine() {
    for e in EngineType::ALL {
        let p = ParamSnapshot::for_engine(e);
        assert_eq!(p.engine(), e);
        assert_eq!(p.filter.cutoff, ParamSnapshot::default().filter.cutoff);
        assert_eq!(p.out.volume, ParamSnapshot::default().out.volume);
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test engine_source_test 2>&1 | grep -E '^error\[' | head -3`
Expected: `no method named engine found for enum ChainType`, `no function or associated item named for_engine`.

- [ ] **Step 3: Implement**

`chimera-core/src/params.rs`: in `ParamSnapshot`, `pub engine: EngineType,` →
```rust
    /// Private: set only through `for_engine` (and so `Patch::init`, from
    /// `ChainType::engine`) — one source of truth for engine choice (spec §6).
    engine: EngineType,
```
and add at the top of `impl ParamSnapshot`:
```rust
    /// Default params for `engine`.
    pub fn for_engine(engine: EngineType) -> Self {
        Self { engine, ..Self::default() }
    }

    pub fn engine(&self) -> EngineType {
        self.engine
    }

```
`chimera-core/src/preset.rs`: `use crate::params::ParamSnapshot;` → `use crate::params::{EngineType, ParamSnapshot};`; add to `impl ChainType` after `label`:
```rust

    /// The engine this chain plays (spec §6).
    pub const fn engine(self) -> EngineType {
        match self {
            ChainType::PizzaPoly => EngineType::Pizza,
            ChainType::Modal => EngineType::Modal,
            ChainType::Fm => EngineType::Fm,
        }
    }
```
and in `Patch::init` replace `let mut params = ParamSnapshot::default();` and the first two match arms with
```rust
        let params = ParamSnapshot::for_engine(chain_type.engine());
        let (mod_state, dest_registry) = match chain_type {
            ChainType::PizzaPoly | ChainType::Modal => (ModState::default(), ModDestRegistry::new()),
            ChainType::Fm => {
```
deleting `params.engine = crate::params::EngineType::Fm;` from the FM arm.
`chimera-core/src/dsp/voice.rs`: both `params.engine` → `params.engine()`.

- [ ] **Step 4: Migrate the test sites (mechanical, in this order)**

Rules: (1) `let mut x = ParamSnapshot::default();` immediately followed by `x.engine = E;` → `let mut x = ParamSnapshot::for_engine(E);`; (2) in closures `|p| { p.engine = E; … }` (always the first statement — verified) → `*p = ParamSnapshot::for_engine(E);`; (3) owned `params…` reassigned later → `params = ParamSnapshot::for_engine(E);`; (4) `ui.params_mut().engine = E;` (always first in its closure) → `*ui.params_mut() = ParamSnapshot::for_engine(E);`; (5) reads `.engine` → `.engine()`; three sites are rewritten by hand (Python below).
```bash
cd chimera-core/tests
grep -l '\.engine = ' *.rs common/mod.rs | xargs perl -0pi -e 's/let mut (\w+) = ParamSnapshot::default\(\);\n\s*\1\.engine = ([^;]+);/let mut $1 = ParamSnapshot::for_engine($2);/g'
python3 - <<'PY'
p = open('property_test.rs').read()
old = """    let mut p = ParamSnapshot::default();

    // Engine type: every engine (spec § Testing "Engines")
    p.engine = EngineType::ALL[rng.u8(EngineType::ALL.len() as u8 - 1) as usize];
"""
assert old in p
p = p.replace(old, """    // Engine type: every engine (spec § Testing "Engines")
    let mut p = ParamSnapshot::for_engine(EngineType::ALL[rng.u8(EngineType::ALL.len() as u8 - 1) as usize]);
""")
open('property_test.rs', 'w').write(p)

m = open('modulatable_test.rs').read()
start = m.index('fn recipe(block: BlockRef) -> ParamSnapshot {')
end = m.index('fn lfo_route(')
m = m[:start] + """fn recipe(block: BlockRef) -> ParamSnapshot {
    let engine = match block {
        BlockRef::Fm | BlockRef::FmOp(_) => EngineType::Fm,
        BlockRef::Modal => EngineType::Modal,
        _ => EngineType::Pizza, // Pizza, AmpEnv, Out, Drive, Filter, Folder
    };
    let mut p = ParamSnapshot::for_engine(engine);
    p.lfo.rate = 5.0; // swings both ways within the render
    match block {
        BlockRef::Fm | BlockRef::FmOp(_) => {
            p.fm.algorithm = 7; // every operator is a carrier
            for op in p.fm.operators.iter_mut() {
                op.level = 99.0;
            }
        }
        BlockRef::Drive => p.drive.drive = 0.5,
        BlockRef::Filter => p.filter.cutoff = 2000.0,
        BlockRef::Folder => p.folder.fold = 0.5,
        _ => {}
    }
    p
}

""" + m[end:]
open('modulatable_test.rs', 'w').write(m)

c = open('common/mod.rs').read()
old_fn = c[c.index('/// Params the switch case renders with from block ON_BLOCKS / 2 on.'):c.index('/// Render the fixed harness for one case.')]
c = c.replace(old_fn, '')
c = c.replace('    let switched = switched_params(&params);', """    // Pizza→Modal: from block ON_BLOCKS / 2 the same (default) params with
    // the engine switched — i.e. the Modal init params.
    let switched = init_params(EngineType::Modal);""")
open('common/mod.rs', 'w').write(c)
PY
perl -pi -e 's/^(\s*)p\.engine = (EngineType::\w+);/$1*p = ParamSnapshot::for_engine($2);/' click_free_test.rs live_param_test.rs stress_test.rs property_test.rs
perl -pi -e 's/^(\s*)(params\w*)\.engine = (.+);/$1$2 = ParamSnapshot::for_engine($3);/' engine_switch_test.rs
perl -pi -e 's/^(\s*)ui\.params_mut\(\)\.engine = (EngineType::\w+);/$1*ui.params_mut() = ParamSnapshot::for_engine($2);/' desktop_sim_test.rs
sed -i 's/^use chimera_core::params::EngineType;/use chimera_core::params::{EngineType, ParamSnapshot};/' desktop_sim_test.rs
perl -pi -e 's/\.engine\b(?!\s*\()/.engine()/g' modal_integration_test.rs fm_test.rs property_test.rs
grep -n '\.engine = ' *.rs common/mod.rs   # must print nothing
cd ../..
cargo test -p chimera-core --no-run 2>&1 | grep -E '^error' -A4   # must print nothing
```
(`unused_mut` warnings on `let mut x = ParamSnapshot::for_engine(…)` lines that are no longer mutated are expected; leave them or drop the `mut` by hand — do not run `cargo fix` over unrelated warnings.)

- [ ] **Step 5: Run everything**

Run: `cargo test -p chimera-core --test engine_source_test --test golden_test 2>&1 | grep '^test result'`
Expected: `3 passed`, `4 passed` (the Pizza→Modal golden proves `init_params(Modal)` equals the old "clone with engine switched"). Full suite: `0 failed`.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/preset.rs chimera-core/src/dsp/voice.rs chimera-core/tests/
git commit -m "refactor(core): ChainType::engine is the one source of engine choice

ParamSnapshot::engine is private; tests build snapshots with for_engine.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 22: Remove the FM pre-wire; chains declare `mod_sources`; re-record the FM golden

**Files:**
- Modify: `chimera-core/src/preset.rs` (`Patch::init` without pre-wire; drop now-unused imports)
- Modify: `chimera-core/src/ui/block_def.rs` (`ChainDef2.mod_sources`)
- Modify: `chimera-core/src/ui/block_registry.rs` (`PART_MOD_SOURCES`; every `ChainDef2` gets `mod_sources`)
- Modify: `chimera-core/src/ui/mod_grid.rs` (`rebuild_sources(&[&'static str])`)
- Modify: `chimera-core/src/ui/mod.rs` (two `rebuild_sources` call sites)
- Modify: `chimera-core/tests/golden_test.rs` (re-record `fm_init_patch_mod` — the only deliberate golden change in this plan)
- Modify: `chimera-core/tests/{binding_test,ui_routing_test,modulation_integration_test}.rs`

**Interfaces:**
- Consumes: Task 21 (`ParamSnapshot::for_engine`, `ChainType::engine`), Task 16 (`ChainType::ALL`).
- Produces: `ChainDef2 { …, pub mod_sources: &'static [&'static str] }`; `pub static PART_MOD_SOURCES: [&str; 2] = ["ENV", "LFO"]`; `MatrixState::rebuild_sources(&mut self, names: &[&'static str])`; `Patch::init` returns an empty `ModState`/registry for every chain.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/binding_test.rs`:
```rust
/// Spec §4: matrix source rows are what `Voice` produces — ENV and LFO on
/// every Part chain (FM no longer lists four envelope rows).
#[test]
fn part_chains_offer_env_and_lfo_sources() {
    for ct in ChainType::ALL {
        assert_eq!(chain_def_for(ct).mod_sources, ["ENV", "LFO"], "{ct:?}");
        let patch = chimera_core::preset::Patch::init(ct);
        assert!(patch.dest_registry.is_empty(), "{ct:?}: no pre-wired destinations");
        assert_eq!(patch.mod_state.num_dests(), 0, "{ct:?}");
    }
    // The FM_ENV pages stay as MOD sub-pages; they are just not source rows.
    assert_eq!(chain_def_for(ChainType::Fm).blocks[4].sub_pages.len(), 4);
}
```
Append to `chimera-core/tests/ui_routing_test.rs`:
```rust
/// Spec §4: after loading the FM init patch the matrix rows are ENV and LFO
/// (they used to be "Op1 Env".."Op4 Env", of which only two produced values).
#[test]
fn fm_matrix_rows_are_env_and_lfo() {
    use chimera_core::preset::POOL_SIZE;

    let mut ui = UiState::new();
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Edit, ButtonState::Held)
            .button(ButtonId::B1, ButtonState::Pressed),
    );
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 2) as i8));
    press(&mut ui, ButtonId::Edit); // load "(init) FM"
    let rows: Vec<&str> = (0..ui.matrix_state.num_sources)
        .map(|i| ui.matrix_state.sources[i].unwrap().name)
        .collect();
    assert_eq!(rows, ["ENV", "LFO"]);
    assert_eq!(ui.matrix_state.num_dests, 0);
}
```
Append to `chimera-core/tests/golden_test.rs`:
```rust
/// Spec step 8: without the pre-wire, the FM init patch renders exactly like
/// FM init params with no modulation.
#[test]
fn fm_init_patch_has_no_prewire() {
    assert_eq!(fnv1a(&render_case(Case::FmInitPatchMod)), fnv1a(&render_case(Case::FmInit)));
}
```
In `chimera-core/tests/modulation_integration_test.rs`, replace the body of `matrix_state_rebuild_sources` (the two static `BlockDef`s and the `sub_pages` slice go away) with:
```rust
    let mut matrix = MatrixState::new();
    matrix.rebuild_sources(&["Env", "LFO"]);

    assert_eq!(matrix.num_sources, 2);
    assert_eq!(matrix.sources[0].as_ref().unwrap().name, "Env");
    assert_eq!(matrix.sources[1].as_ref().unwrap().name, "LFO");
```
and delete the now-unused `use chimera_core::ui::block_def::BlockDef;` / `use chimera_core::ui::page::{CellIcon, PageLayout, ValFmt};` lines inside that test.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p chimera-core --test binding_test --test golden_test --test modulation_integration_test --no-fail-fast 2>&1 | grep -E '^error\[|FAILED' | head`
Expected: `no field mod_sources on type &ChainDef2`, `mismatched types` for `rebuild_sources(&["Env", "LFO"])`, and `fm_init_patch_has_no_prewire` FAILED (hashes differ while the pre-wire exists).

- [ ] **Step 3: Implement**

`chimera-core/src/ui/block_def.rs`, in `ChainDef2` after `blocks`:
```rust
    /// Mod matrix source rows, in `Voice`'s source order (spec §4).
    pub mod_sources: &'static [&'static str],
```
`chimera-core/src/ui/block_registry.rs`: below the `// Chain templates` banner add
```rust

/// Mod sources every Part voice produces: source 0 = amp envelope, 1 = LFO
/// (`Voice::render`).
pub static PART_MOD_SOURCES: [&str; 2] = ["ENV", "LFO"];
```
and add `mod_sources: &PART_MOD_SOURCES,` as the last field of `PIZZA_POLY_CHAIN`, `KICK_CHAIN`, `MODAL_PLUCK_CHAIN`, `FM_CHAIN`, and `mod_sources: &[],` to `MIX_CHAIN`, `ENVELOPE_CHAIN`, `MIXER_CHANNEL_CHAIN`, `SYSTEM_CHAIN`, `DEMO_CHAIN`.

`chimera-core/src/ui/mod_grid.rs`: `ModSource` doc → `/// A source row in the mod matrix.`; replace `rebuild_sources` with
```rust
    /// Rebuild the source rows from the chain's `mod_sources`.
    /// Call this when the chain changes or at init.
    pub fn rebuild_sources(&mut self, names: &[&'static str]) {
        self.num_sources = 0;
        for &name in names {
            if self.num_sources < MAX_SOURCES {
                self.sources[self.num_sources] = Some(ModSource { name });
                self.num_sources += 1;
            }
        }
    }
```
`chimera-core/src/ui/mod.rs`: in `new()` replace the four lines starting `// Build source list from the chain's mod block sub-pages` with
```rust
        // Source rows = what the chain's voice produces (ENV, LFO)
        matrix_state.rebuild_sources(nav.active_chain().mod_sources);
```
and in the patch-browser load replace the `if let Some(last_block) = chain.blocks.last() { … }` block (and its `let chain = …;`) with `self.matrix_state.rebuild_sources(self.nav.active_chain().mod_sources);`.

`chimera-core/src/preset.rs`: replace the body of `Patch::init` after the `name` lines with
```rust
        Self {
            name,
            chain_type,
            params: ParamSnapshot::for_engine(chain_type.engine()),
            // No pre-wired routes: the matrix starts empty on every chain
            // (spec §4 "FM pre-wire removed").
            mod_state: ModState::new(),
            dest_registry: ModDestRegistry::new(),
        }
```
and delete `use crate::addr::{BlockRef, Op, ParamAddr};`.

- [ ] **Step 4: Re-record the one golden that must change**

Run: `cargo test -p chimera-core --test golden_test 2>&1 | grep -A2 'golden mismatch'`
Expected: exactly one line, `fm_init_patch_mod: hash 0x9bfe44d54ef0385b (want 0xadc0aa292dba2808) …` — the new hash equals `fm_init`'s. Any other mismatching case is a bug: stop and investigate. Then replace the `fm_init_patch_mod` row in `GOLDENS` with
```rust
    // Re-recorded in Task 22 (spec step 8): the FM pre-wire is gone, so the
    // FM init patch's own ModState is empty and this equals `fm_init`.
    ("fm_init_patch_mod", 0x9bfe44d54ef0385b, [898059883, 1045152839, 1054792150, 3163439516, 3202136915, 3201882817, 0, 0]),
```
(if your Task 1 recording differed, copy your own `fm_init` row's values).

- [ ] **Step 5: Run everything**

Run: `cargo test -p chimera-core --no-fail-fast 2>&1 | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {print p, "passed", f, "failed", i, "ignored"}'`
Expected: `0 failed`, `2 ignored` (the two Modal sanity tests, issue 003).
Run the binaries' checks once more: `cargo check -p chimera-desktop` and `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf` (report environment blockers as in Task 13).

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/preset.rs chimera-core/src/ui/block_def.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/mod.rs chimera-core/tests/
git commit -m "feat(core): drop the FM init pre-wire; chains declare their mod sources

Matrix rows are ENV and LFO on every Part chain, matching what Voice
produces. The fm_init_patch_mod golden is re-recorded deliberately (now
equal to fm_init).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Final verification (after Task 22)

- [ ] `cargo test -p chimera-core` → 0 failed, 2 ignored (issue 003).
- [ ] `grep -rn 'ParamPath\|legacy_to_addr\|FM_SEL_OP\|resolve_param_mut' chimera-core/src` → nothing.
- [ ] `grep -rnw 'Param' chimera-core/src | grep -v '//'` → nothing.
- [ ] `cargo clippy -p chimera-core --tests 2>&1 | grep -c '^warning'` is not higher than before Task 1 (pre-existing warnings are out of scope).
- [ ] Desktop / firmware compile checks run, or their environment blockers reported (ALSA headers; `thumbv7em-none-eabihf` target).
- [ ] Manual (desktop, when buildable): Pizza, Modal and FM chains play; FM Operator page selector moves the LEVEL/FDBK bars to the chosen operator; priming CUTOFF and setting LFO → cutoff audibly sweeps; Mixer page MIX+Plus adds no matrix column.
