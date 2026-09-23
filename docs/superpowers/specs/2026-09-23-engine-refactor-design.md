# Engine Refactor Design

**Date:** 2026-09-23
**Status:** Draft v2 (revised after adversarial review), awaiting review
**Sub-project:** 1 of 5 (engine refactor → composable chains + patch format → on-device chain editor → new engines → web patch builder)

## Intent

Make adding a synthesis engine cheap and type-checked, and make each block
the single owner of everything about itself. After this refactor a new engine
is one module containing:

1. its **values** struct (patch data, owned by the UI),
2. its **description**: `const` `ParamSpec`s (in flash, one per block type),
3. its **DSP** struct (audio state, owned by the audio thread),
4. its **pages**: `BlockDef`s whose slots bind to its params,

plus one arm in each exhaustive `match` the compiler points at.

Scope is the **Part chains** (Pizza, FM, Modal and their DRV/FLT/FLD/MOD pages).
Mixer, System and Demo pages keep `PageId` for now; the Mixer chain's
existing page mis-binding (§ Known issues) is fixed with the mixer work.

## Current state (verified at `a3dd4f7`)

- `Voice` owns `pizza`, `modal`, `fm` at once and dispatches on
  `active_engine: EngineType` in `note_on`, `note_off`, render, VCA and the
  activity check (`dsp/voice.rs`).
- Sizes (measured): `ModalEngine` 66,752 B (8 `KsString` × `[f32; 2048]` + 48
  SVFs), `FmEngine` 1,088 B, `PizzaOsc` 16 B, `Voice` 67,912 B,
  `ParamSnapshot` 1,524 B.
- `PizzaParams`/`ModalParams` are raw `f32`/`u8`; FM, filter, drive, folder,
  envelopes use `Param { value, min, max, default }` (16 B each).
- Modulation offsets are hard-coded per engine in `Voice::render` with
  `ParamPath::Block { block, param }`, where `block` is the chain node index
  (means different blocks in different chains; can't address sub-pages).
- Only these are read by the DSP per block: Pizza shape/crush/level, drive,
  filter cutoff/resonance/drive, folder, amp envelope, volume, LFO, and FM op
  level/feedback/waveform (`FmOperator::update_live`). FM ratios, detune and
  envelopes, and Modal's mode/excitation, are read only at `note_on`.
  `envelopes[1..2]`, `filter.fm_amount/env_amount/key_track` are never read.
  Chorus/delay/reverb run outside `Voice`, desktop only.
- Desktop passes `ModState::new()` (empty) to `Voice`, so modulation is
  inaudible in the simulator.
- FM init patch pre-wires sources "E1..E4" → op levels, but only source 0
  (amp env) and 1 (LFO) produce values.
- Patches are not persisted, so addresses can change freely.

## Design

### 1. The `Block` trait: values + description, per block

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ParamId(pub u8);            // stable per block type; never reused

pub enum ParamKind {
    Continuous,                        // modulation: added in normalized space
    Stepped,                           // integer steps; modulation rounds
    Enum { count: u8 },                // discrete choice; never modulatable
}

pub struct ParamSpec {
    pub id: ParamId,
    pub label: &'static str,
    pub min: f32, pub max: f32, pub default: f32,
    pub step: f32,                     // per encoder tick; preserves today's feel
    pub kind: ParamKind,
    pub modulatable: bool,             // true only if Voice reads it per block
}

pub trait Block {
    fn specs(&self) -> &'static [ParamSpec];
    fn get(&self, id: ParamId) -> f32;
    fn set(&mut self, id: ParamId, v: f32); // clamps via its spec
}
```

- Each values struct keeps **real field types** (`f32`, `u8`, and enums such
  as `ResonatorMode`); `get`/`set` convert at the boundary. DSP code reads
  fields directly.
- `Param { value, min, max, default }` is removed. Structs that use it
  (`FmOpParams`, `FmParams`, `FilterParams`, `DriveParams`, `FolderParams`,
  `EnvParams`, volume/pan) become plain fields + `Block` impls.
- `ParamSnapshot` shrinks from 1,524 B to roughly 0.4 KB (measured after).
- A block's specs are a `const` array in its own module. No central table.
- `ValFmt` for display is derived from the spec (`Enum`/`Stepped` → `Int`,
  `min < 0` → `Bi`, else `Uni`), so ranges and display cannot drift apart.
- Initial curves are linear only, so existing sounds and mod depths are
  preserved. Exponential curves are a later, deliberate change.

### 2. Addressing a parameter

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Op { A, B, C, D }             // TryFrom<u8> rejects > 3

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockRef { Pizza, Modal, Fm, FmOp(Op), Drive, Filter, Folder, AmpEnv, Lfo, Out }

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamAddr { pub block: BlockRef, pub param: ParamId }
```

- `ParamSnapshot::block(&self, BlockRef) -> &dyn Block` and `block_mut` are
  one exhaustive `match`. Every UI, modulation and (later) serialization path
  goes through them. `dyn Block` is used on the UI side only; the audio path
  reads fields directly.
- Addresses name *what* a parameter is, not where it sits on a page or in a
  chain. Rearranging cells or reordering blocks does not remap mod routes.
- `Op` makes an out-of-range operator unrepresentable (no panic from bad
  patch or SysEx data later).
- Multiple instances of one block kind (two filters) are sub-project 2: it
  adds an instance id to `BlockRef` with the chain model.
- `ParamAddr` replaces `ParamPath` everywhere (registry, `ModState`, UI).

### 3. Engines: persistent instances, dispatch in one place

Constructing a 66 KB `ModalEngine` in the audio interrupt risks silent stack
overflow (no stack guard; `memory.x` puts the stack in the same 512 KB RAM),
and an enum-of-engines saves only ~1 KB. So engines stay persistent:

```rust
pub struct Engines { pizza: PizzaOsc, fm: FmEngine, modal: ModalEngine }

impl Engines {
    pub fn note_on(&mut self, kind: EngineType, note: MidiNote, vel: Velocity, p: &ParamSnapshot);
    pub fn note_off(&mut self, kind: EngineType);
    pub fn render(&mut self, kind: EngineType, out: &mut [f32; BLOCK_SIZE], p: &ParamSnapshot);
    pub fn uses_amp_env(kind: EngineType) -> bool;               // VCA choice
    pub fn is_active(&self, kind: EngineType, amp_env: &Envelope) -> bool; // voice lifetime
}
```

- Each method is one exhaustive `match kind`. Adding an engine = one field +
  one arm per method; the compiler lists them.
- VCA and activity are separate, explicit rules (today's behavior):

  | Engine | Amp env on VCA | Voice active while |
  |---|---|---|
  | Pizza | yes | `amp_env.is_active()` |
  | FM | yes | `!fm.is_idle()` |
  | Modal | no | `modal.is_active()` |
  | Va | — | never (silent placeholder) |

- Sample rate is passed to `Engines::new(sr)` and stored once, not per call.
- `MidiNote` and `Velocity` newtypes are created by the MIDI parser (the trust
  boundary), so the audio path takes already-valid values.
- Known future constraint: Modal's ~64 KB of string buffers must move to a
  shared pool before polyphony (voice allocator sub-project). Not done here.

### 4. Modulation

```rust
// in Voice::render
let mut m = params.clone();                       // ~0.4 KB stack copy
for d in 0..mod_state.num_dests {
    let off = mod_state.sum_for(d, &mod_values);  // sums sources for dest index d
    if off != 0.0 {
        let blk = m.block_mut(mod_state.dests[d].block);
        apply_offset(blk, mod_state.dests[d].param, off);
    }
}
```

- `apply_offset`: `Continuous` adds `off` in normalized space then
  denormalizes (identical to today's `offset × range` while curves are
  linear); `Stepped` rounds after; `Enum` is never reached because the
  registry refuses non-modulatable params.
- The registry's `add` rejects any address whose spec has
  `modulatable: false`. A test enforces that every `modulatable: true` param
  audibly changes output when modulated, so the flag can't lie.
- The LFO source is computed from `params.lfo` as today (modulating the LFO
  itself is out of scope; `Lfo` params are `modulatable: false`).
- The modulated copy `m` feeds the engine, drive, filter, folder, amp
  envelope and volume.
- **Desktop:** the audio callback receives the active track's real `ModState`
  through the same double buffer as `ParamSnapshot`, so modulation is audible
  in the simulator.
- **FM pre-wire removed.** FM matrix sources become ENV and LFO (the sources
  that exist), like the Pizza chain. Real per-operator envelope sources are
  out of scope.

### 5. UI binding for Part chains

```rust
pub enum SlotBinding {
    Empty,
    Param(ParamAddr),          // fixed param, e.g. Filter cutoff, FmOp(A) coarse
    SelectedOp(ParamId),       // param of the currently selected FM operator
    SelectOp,                  // the FM operator selector itself
}

pub struct ParamSlot { pub binding: SlotBinding, pub icon: CellIcon }
```

- `BlockDef.params: [ParamSlot; 6]`. Label, format and step come from the
  bound spec, so a slot can't disagree with its param.
- `SelectedOp` resolves to `ParamAddr { block: FmOp(sel), .. }` *when the path
  is created* (encoder turn, mod priming). A saved route always names a
  concrete operator.
- FM Ratios page: slots 0–3 = `Param(FmOp(A..D), COARSE)`, slot 4 =
  `SelectedOp(FINE)`.
- FM Algorithm page's level slot binds to `Param(Out, VOLUME)` directly (one
  address per physical param, no aliasing).
- Selected operator moves from the `static FM_SEL_OP` atomic into `UiState`.
- Generic `read_values`, `apply_encoder`, `snap_encoder`, cell labels and mod
  path building all walk `SlotBinding`s. One function builds the `ParamAddr`
  for a slot; `UiState::current_param_path`, `current_param_label` and
  `Renderer::param_path_for_cell` use it.
- `region.rs` dirty tracking for Part pages keys on the `&'static BlockDef`
  plus the selected operator instead of `PageId`.
- `PageId` variants for Part pages (Pizza, EngineModal1/2, Drive, Filter,
  Folder, Vca, Lfo, FmAlg, FmOp, FmRatio, FmEnv1–4) and their helper functions
  are deleted. Mixer/System/Demo keep `PageId` until their own work.

### 6. One source of truth for engine choice

`ChainType::engine(self) -> EngineType` (const fn). `Patch::init` sets
`params.engine` from it. Tests that set `params.engine` directly go through a
`ParamSnapshot::for_engine(EngineType)` helper.

## Intended behavior changes

Everything else must stay identical (verified by golden buffers).

- FM init patch: pre-wire removed; FM matrix sources are ENV and LFO.
- Shift-snap now works on Pizza, Modal, LFO and FM pages (was a no-op there).
- Encoder steps: each spec's `step` is set to today's per-page step; any
  deviation found during the audit is listed in the plan and approved
  explicitly.
- Modal `mode` encoder: today clamps at 2 while `ResonatorMode` has 4 values.
  The spec is `Enum { count: 4 }`; exposing the 4th mode is flagged in the
  plan for a decision.

## Known issues not fixed here

- Mixer chain: `ChainNav` shows `MIXER_CHANNEL_CHAIN` (CHANNEL, MIDI, EQ,
  SENDS) but `PageId::from_nav` binds those nodes to Mixer/Chorus/Delay/Reverb,
  so e.g. the MIDI page edits chorus. Fixed with the mixer work.
- STM32 audio reads params through raw pointers (no double buffer).
- Engine switch mid-note is a hard cut (no crossfade).
- Block-rate modulation of gain params can zipper; ramping is later work.

## Testing

TDD; each landing step starts with a failing test.

- **Goldens (land first):** fixed harness = 48 kHz, note 60 vel 100 on, 200
  blocks, note off, 200 blocks; recorded per engine for: init patch with empty
  `ModState`; init patch + LFO → filter cutoff; Pizza→Modal switch mid-note.
  Output must match bit-for-bit after each step except where an intended
  change re-records deliberately.
- **Specs:** every block's `specs()` has unique `ParamId`s, `min < max`,
  default in range, `step > 0`; `Enum` params are not `modulatable`.
- **Bindings:** for every `BlockDef` reachable via `chain_def_for`, every
  non-`Empty` slot resolves to a spec.
- **Modulatable is true:** for every spec with `modulatable: true`, a route
  from the LFO changes rendered output vs no route.
- **Registry:** adding a non-modulatable address is refused.
- **Engines:** `EngineType::ALL`; `property_test` and `engine_switch_test`
  iterate it; a helper with an exhaustive `match` makes an uncovered variant a
  compile error. Activity/VCA table has one test per row.
- **Existing suite:** all 280 tests pass (updated for new field types and
  addresses).

## Landing order (green at each step)

1. Golden tests.
2. `ParamSpec`/`Block` trait; convert values structs one block at a time
   (Pizza, Modal, Drive, Filter, Folder, Env, LFO, FM, Out), removing `Param`.
3. `Engines` struct with explicit VCA/activity rules; `Voice` uses it.
4. `ParamAddr`/`BlockRef`/`Op` + `ParamSnapshot::block(_mut)`, alongside
   `ParamPath`.
5. `Voice` generic modulation; registry `modulatable` check; desktop ModState.
6. `SlotBinding` UI for Part chains; delete Part-page `PageId` variants;
   delete `ParamPath`.
7. `ChainType::engine`.
8. FM pre-wire removal + FM matrix sources (re-record FM golden).

## Out of scope

- Voice allocator, polyphony, Modal buffer pooling, `Cost`/`Budget`.
- Composable chains, block instance ids, patch serialization (sub-project 2).
- Mixer/System/Demo page binding.
- New engines (sub-project 4, see `2026-09-23-engine-pivot-design.md`).
- Per-operator FM envelope mod sources; modulating FM ratios/envelopes
  (read only at note-on).
- Non-linear parameter curves.
