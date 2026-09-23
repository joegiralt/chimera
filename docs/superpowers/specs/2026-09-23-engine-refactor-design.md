# Engine Refactor Design

**Date:** 2026-09-23
**Status:** Draft, awaiting review
**Sub-project:** 1 of 5 (engine refactor → composable chains + patch format → on-device chain editor → new engines → web patch builder)

## Intent

Make adding a synthesis engine cheap and type-checked. Today a new engine
touches ~16 places across DSP and UI (`EngineType`, five `match` arms in
`Voice`, `ChainType`, `ChainDef2`, BlockDefs, `PageId` + three per-page
match functions, two `ParamPath` builders, the patch browser). After this
refactor, an engine is:

1. a DSP module implementing `EngineVoice`,
2. a params struct made of `Param`s,
3. its `BlockDef`s (pages) with a `BlockTarget`,
4. one `Engine` variant and one `ChainType` arm.

The compiler flags every other site through exhaustive `match`.

No behavior change for existing sounds (Pizza, FM, Modal). All 280 existing
tests keep passing (updated mechanically where field types change).

## Current state (verified 2026-09-23 at `a3dd4f7`)

- `Voice` owns `pizza: PizzaOsc`, `modal: ModalEngine`, `fm: FmEngine`
  simultaneously and dispatches on `active_engine: EngineType` in `note_on`,
  `note_off`, render, VCA and the active check (`dsp/voice.rs`).
- Engine APIs differ: `PizzaOsc::note_on(freq, sr)`,
  `FmEngine::note_on_params(note, vel_f32, &FmParams, sr_f32)`,
  `ModalEngine::note_on(note, vel_u8, &ModalParams, sr)`.
- `PizzaParams` and `ModalParams` use raw `f32`/`u8`; FM uses `Param`.
- Modulation offsets are hard-coded per engine in `Voice::render` using
  `ParamPath::Block { block, param }`, where `block` is the chain node index.
  The same index means different blocks in different chains, and it cannot
  distinguish sub-pages (Modal's two engine pages).
- UI binding goes through `PageId` (29 variants), `from_part_nav`,
  `read_values`, `apply_encoder` and per-page helper functions (`ui/page.rs`).
- `ChainType` (UI) and `EngineType` (DSP) are kept in sync only by `Patch::init`.
- Patches are not persisted (no SD serialization yet), so `ParamPath` can change
  freely.

## Design

### 1. `EngineVoice` trait and `Engine` enum

```rust
pub trait EngineVoice {
    type Params;
    /// false = engine shapes its own amplitude (Modal); Voice skips the amp envelope.
    const USES_AMP_ENV: bool;
    fn note_on(&mut self, note: MidiNote, vel: Velocity, p: &Self::Params, sr: SampleRate);
    fn note_off(&mut self);
    fn render(&mut self, out: &mut [f32; BLOCK_SIZE], p: &Self::Params);
    fn is_active(&self) -> bool;
}

pub enum Engine {
    Pizza(PizzaOsc),
    Fm(FmEngine),
    Modal(ModalEngine),
    Silent, // VA placeholder until an engine exists
}
```

- `Engine` forwards each method with one `match` that picks the matching
  params field from `ParamSnapshot`.
- `Engine::kind(&self) -> EngineType` and `Engine::new(EngineType) -> Engine`.
- Engines that need the sample rate store it at `note_on` (FM already does).
- `Voice` holds `engine: Engine` instead of three fields. On `note_on`, if
  `params.engine != self.engine.kind()`, `Voice` replaces `self.engine` with
  `Engine::new(params.engine)` before triggering. This replaces the
  auto-retrigger in `render` (commit `451eaee`); the mid-note switch behavior
  is preserved by keeping that check but calling the same replace-then-trigger
  path.
- Memory: `size_of::<Engine>()` is the largest engine, not the sum. A test
  asserts `size_of::<Voice>()` stays under a budget recorded from the first
  measurement plus 10%, so growth is deliberate.
- Constructing a new `Engine` in the audio callback is a stack/in-place write,
  not a heap allocation. `ModalEngine::new()` must not be large enough to
  overflow the audio stack; if measurement shows it is, switch to
  in-place reset (`Engine::reset_to(kind)`) instead of construct-and-move.

### 2. Unit newtypes (only where the trait needs them)

- `MidiNote(u8)`: `TryFrom<u8>`, rejects > 127.
- `Velocity(u8)`: 0..=127, with `as_unit() -> f32`.
- `SampleRate(u32)`.

Nothing else is introduced now. `Hz`, `Unit`, `Cost` and `Budget` wait until a
consumer exists (voice allocator, new engines).

### 3. All engine params become `Param`

- `PizzaParams { shape, crush, level }` → `Param`.
- `ModalParams`: all `f32` fields → `Param`; `mode`, `num_modes`,
  `ks_excitation` → `Param` with integer range, read with `.value as u8`.
- `LfoParams`, `ChorusParams`, `DelayParams`, `ReverbParams` → `Param`, so every
  page reachable in the UI goes through one path and `PageId` can be removed.
- DSP code reads `.value`. Tests that set raw fields switch to `.set(x)`.

### 4. Semantic parameter addresses

Replace chain-index addressing with what the parameter *is*:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockTarget {
    Pizza, Modal1, Modal2,
    FmAlg, FmOp(u8), FmRatio, FmEnv(u8),
    Drive, Filter, Folder,
    AmpEnv, FilterEnv, AuxEnv, Lfo,
    Mixer, Chorus, Delay, Reverb,
    None, // pages with no editable params (Demo, System, MOD grid)
}

pub struct ParamPath { pub target: BlockTarget, pub param: u8 }
```

- One function resolves any address:
  `resolve_param_mut(path, &mut ParamSnapshot) -> Option<&mut Param>`, and a
  matching `resolve_param(path, &ParamSnapshot) -> Option<&Param>`.
  It is a single exhaustive `match` on `target`.
- `ParamPath::FmOp`/`FmEnv` variants fold into `BlockTarget::FmOp(op)` /
  `FmEnv(op)`. The FM operator page's target is built from the selected
  operator at the moment a path is created (as today).
- Because addresses no longer depend on chain position, mod routes survive
  block reordering in sub-project 2.

### 5. Generic modulation in `Voice`

Replace the hard-coded offset block with:

```rust
let mut modded = params.clone(); // stack copy, as the current per-block copies are
for dest in mod_state.active_dests() {
    if let Some(p) = resolve_param_mut(dest, &mut modded) {
        p.apply_mod_offset(mod_state.offset_for(dest, &mod_values));
    }
}
```

- Offsets remain per block (unchanged rate).
- Every `Param` becomes modulatable, including FM envelope params (currently
  silently ignored) and Modal params (currently none).
- `ParamSnapshot` clone cost is checked in `stress_test`. If it is measurably
  worse than today's per-struct copies, clone only the blocks named by active
  destinations.

### 6. BlockDef-driven UI binding (removes `PageId`)

- `BlockDef` gains `target: BlockTarget`. Every page definition in
  `block_registry.rs` sets it.
- `ParamSlot.format` (`ValFmt`) already drives display. It also drives
  encoder steps: `Int(_)` = 1 per tick, `Uni`/`Bi` = range/128 per tick.
- `read_values`, `apply_encoder` and `snap_encoder` become free functions
  over `(&BlockDef, &ParamSnapshot)`, looping slots and calling
  `resolve_param`. Empty slots (`ParamSlot::EMPTY`) are skipped.
- `UiState::current_param_path` and `Renderer::param_path_for_cell` become
  one function: `ParamPath { target: def.target (with selected op), param: idx }`.
- `PageId`, `from_part_nav` and the per-page `apply_*_encoder` helpers are
  deleted. FM algorithm-page side effects (the algorithm encoder also writes
  `volume`) move into `BlockTarget::FmAlg`'s resolve arm or become a normal
  slot pointing at `volume`.
- System and Demo pages use `BlockTarget::None`, so encoders on them no
  longer edit Pizza.

### 7. One source of truth for engine choice

`ChainType::engine(self) -> EngineType` (const fn). `Patch::init` sets
`params.engine` from it; nothing else writes `params.engine`.

### 8. Fixes that fall out

- FM init patch pre-wires E1..E4 → Op levels, but only sources 0 (amp env)
  and 1 (LFO) produce values. The pre-wire changes to the sources that exist
  (amp env → op levels at the current amount), and a test asserts every
  pre-wired source index produces a value. Real per-operator envelope sources
  are out of scope.
- System pages no longer edit Pizza params (§6).

## Testing

TDD: each step starts with a failing test.

- **Engine coverage:** `EngineType::ALL` const. `property_test` and
  `engine_switch_test` iterate it (today they cover Pizza and Modal only). A
  helper `fn init_params(kind: EngineType) -> ParamSnapshot` uses an
  exhaustive `match`, so a new variant fails to compile until tests cover it.
- **Resolver coverage:** for every BlockDef in `block_registry.rs`, every
  non-empty slot resolves to `Some(&Param)`. A new page with a typo'd slot
  fails this test.
- **Modulation:** a route to each `BlockTarget` changes the rendered output
  (FM env, Modal and Pizza included).
- **Behavior preservation:** before refactoring, record the rendered output of
  each engine's init patch (fixed notes, fixed block count) as golden buffers
  in a test. After, output must match bit-for-bit, except where §8 fixes change
  behavior on purpose (FM pre-wire), which is re-recorded deliberately.
- **Memory:** `size_of::<Voice>()` budget test (§1).
- **Existing suite:** all 280 tests pass.

## Out of scope

- STM32 parameter double buffer (audio reads through raw pointers today).
- Voice allocator, polyphony, `Cost`/`Budget`.
- Composable chains and patch serialization (sub-project 2).
- New engines (sub-project 4, see `2026-09-23-engine-pivot-design.md`).
- Per-operator FM envelope mod sources.
