# Exclusive State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A voice holds only the engine it plays, Modal holds only the model it plays, and string delay lines are stored in 16 bits, with no audible change.

**Architecture:** One macro in `in_place.rs`, `in_place_enum!`, declares a `#[repr(C, u8)]` enum and generates in-place constructors for each variant. Two slots use it: `EngineSlot` (Algo | Modal), which replaces `Engines` in `Voice`, and `ModelSlot` (Bank | String | Bowed | Sympathetic) inside `ModalEngine`. `Voice` widens its existing 128-sample engine-change fade to any `SlotKind` change and rebuilds the slot in place when the fade ends. Strings store `i16` with a per-string block exponent. The functional core is pure and host-tested (`SlotKind::of`, `q16::{store, load, next_exp}`), and the voice's switch logic is a thin shell over it.

**Tech Stack:** Rust 2024, `no_std` `chimera-core` (f32 DSP, `libm`), `thumbv7em-none-eabihf` firmware, `just`.

**Spec:** `docs/superpowers/specs/2026-09-29-exclusive-state-design.md` (owner-approved, binding). Read it with this plan. The § numbers below are the spec's.

**Where it runs:** worktree `/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/wt-excl`, branch `exclusive-state`. Every command runs from that root.

## Global Constraints

- **Unchanged:** `MAX_VOICES` 8, `MAX_STRING_DELAY` 984 (ADR 0040), `VOICE_RAM_BUDGET` 286,720, `Voice::FADE` = 128 samples, all `ModalEngine::COST_*` and `PITCH`, the UI, and the #191 behaviours. `ResonatorMode` keeps its four variants and codes, and `modal/params.rs` is not touched.
- **Types decide (ADR 0012):** a slot's variant *is* its engine or model. No field records which one is active. Render dispatches with a `match` on the slot, so each arm sees only its own payload. There is no wildcard arm on either slot.
- **Functional core, imperative shell:** `SlotKind::of`, `q16::store`, `q16::load` and `q16::next_exp` are pure and have unit tests. `Voice` only calls them.
- **Audio thread:** no heap, no blocking, and no engine-sized value on the stack. Every rebuild writes through a pointer into the slot. `just stack-check` must stay green.
- **`unsafe`:** new `unsafe` goes only into `in_place.rs`, inside `in_place_enum!` and following the pattern of `uninit_at`/`by_value`. Each caller of a generated `unsafe fn` gets one `// SAFETY:` line. No other new `unsafe`.
- **Sizes (spec § Memory):** `pub const VOICE_CHAIN_BYTES: usize = 2048` in `hw.rs`. Assert that a slot is at most its largest payload plus `align_of` of the slot, and that `size_of::<Voice>() <= VOICE_CHAIN_BYTES + size_of::<EngineSlot>()`. The `[Voice; MAX_VOICES] <= VOICE_RAM_BUDGET` assert stays.
- **Block exponent (§ 4):** `e` starts every note at 14 and stays in `14..=24`. It steps up when the peak is below ¼ full scale (|q| < 8192) and down when the peak is above ½ (|q| > 16384). At most one string per voice steps per block.
- **Goldens (ADR 0011):** Tasks 1–4 and Task 5's first commit keep every golden bit-identical. Task 5's second commit re-records exactly `modal_init`, `modal_lfo_cutoff` and `algo_to_modal_switch` (no factory Sound plays Modal), each with the comment `// Re-recorded: Modal strings stored as 16-bit block float (exclusive-state spec § 4).`
- **ADRs:** `docs/adr/0051-*.md` supersedes 0008 and `docs/adr/0052-*.md` covers the 16-bit block-float strings. Both use `0000-template.md`, carry `Status: Proposed`, and get a row in `docs/adr/README.md`. Never edit an accepted ADR. 0008's file stays as it is, and only its README row's status changes.
- **Green gate per task:** `just check` passes. It runs the core, HAL and desktop tests, all firmware builds, `just clippy` and `just stack-check`. If ALSA's pkg-config is missing, set `PKG_CONFIG_PATH` as the Justfile says.
- **Commits:** terse, no type prefix, and never a Co-Authored-By or other AI attribution line. Stage named paths only. Never stage `docs/chimera-ui-ux-spec.md` or `chimera.bin`.

## Review Focus

1. **Pitch bent down after a long tail.** A string note decays until its exponent has climbed, then a new note restarts `e` at 14. If a PITCH route later lengthens the loop, it reads samples written under the old exponent. The player should hear no burst: the Q16 engine's peak must stay within 0.01 of the f32 engine's. Test: `pitch_down_after_a_long_tail_reads_no_burst` (Task 5).
2. **Switching back before the fade ends.** A player turns MODE String → Sympathetic → String within one block. There should be one fade and one rebuild, and the held note should restart on the Sound's kind. Test: `a_switch_back_mid_fade_restarts_on_the_sounds_kind` (Task 4).
3. **A steal across Parts of different kinds.** With the pool full on an Algo Part, a note on a Sympathetic Part steals a voice. The new note should play exactly as on a fresh voice, with at most 2 rebuilds in that block. Test: `a_steal_across_kinds_plays_the_new_kind_clean` (Task 4).
4. **Several switches while a voice is idle.** An idle voice should rebuild once, at its next note-on, not once per edit. This is folded into `idle_voice_switches_in_the_same_block` (Task 4).
5. **High notes wrap several times a block.** A C7 string's write position wraps several times per block. Spikes should stay bounded: at most one exponent step per voice per block, across all eight strings. Test: `a_short_loop_steps_at_most_once_a_block` (Task 5).

## Files

| File | Responsibility | Tasks |
|---|---|---|
| `chimera-core/src/in_place.rs` | `in_place_enum!`: the repr, the private mirror, per-variant `rebuild_*`/`init_*`, layout asserts | 1 |
| `chimera-core/src/dsp/engines.rs` | `EngineSlot`, `SlotKind` (replaces `Engines`) | 2 |
| `chimera-core/src/dsp/voice.rs` | `slot`, `sample_rate`, `rebuilds`; the switch rule | 2, 4 |
| `chimera-core/src/hw.rs` | `VOICE_CHAIN_BYTES` | 2 |
| `chimera-core/src/dsp/modal/mod.rs` | `ModelSlot`, `ModalBank`, `SympatheticStrings`; `ModalEngine<S>` | 2, 3, 5 |
| `chimera-core/src/dsp/modal/string.rs` | `KsString<S: Store>` | 5 |
| `chimera-core/src/dsp/modal/q16.rs` (new) | pure core `Exp`, `store`, `load`, `next_exp`; `Store`, `Q16`, `StepBudget` | 5 |
| `chimera-core/tests/exclusive_state_test.rs` (new) | the switch tests of spec § Tests | 4 |
| `chimera-core/tests/rebuild_stack_test.rs` (new) | `rebuild_fits_a_small_stack`, alone in its binary because a stack overflow aborts the process | 4 |
| `chimera-core/tests/{engines,cost,modal,memory_budget,golden}_test.rs` | API moves, size prints, the re-record | 2, 3, 5 |
| `chimera-stm32/src/bench.rs` | REBUILD, EXP STEP, SWITCH storm, MEMORY screen | 6 |
| `docs/adr/0051-*.md`, `0052-*.md`, `README.md` | the two ADRs | 2, 4, 5, 6 |

**Deviations from the spec's Plan order:** there are three, and each is argued in its task.
- The ADRs are written in the tasks that make them real (0051 in Task 2, amended in 4; 0052 in Task 5). Task 6 only adds the chip figures. This follows the owner rule.
- `in_place_enum!` takes one optional generic parameter from Task 1, because Task 5 needs `ModelSlot<S>`.
- `strings_i16_match_f32` lives in `modal/mod.rs`, not `string.rs`, because Bowed and Sympathetic render there. It compares `ModalEngine<[f32; 984]>` with `ModalEngine<Q16>`, so `ModalEngine` is generic over the store and defaults to `Q16`.

---

### Task 1: `in_place_enum!`

**Files:**
- Modify: `chimera-core/src/in_place.rs`
- Test: `chimera-core/src/in_place.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces a macro, exported with `pub(crate) use in_place_enum;` like `field_list`:

```rust
in_place_enum! {
    $(#[$meta])* $vis enum $Name $(<$G: $Bound>)? {
        $( $Variant($Payload) => $rebuild_fn, $init_fn; )+
    }
}
```

  It expands to the following:
  - `#[repr(C, u8)] $vis enum $Name { $Variant($Payload), … }`.
  - A private mirror inside a `const _: () = { … };` block, together with the impl, so nothing outside can name it. The mirror is `#[repr(C)] struct { tag: Tag, payload: Payload }`, where `#[repr(u8)] enum Tag` has one variant per `$Variant` in order and `#[repr(C)] union Payload` has one `ManuallyDrop<$Payload>` field per variant. This is the layout the Reference defines for `repr(C, u8)`.
  - `pub(crate) unsafe fn $rebuild_fn(&mut self, init: impl FnOnce(&mut MaybeUninit<$Payload>) -> &mut $Payload)`. It writes the tag first, then runs `init` on the payload's address through `uninit_at`. It is `pub(crate)` whatever `$vis` is, so no unsafe constructor leaves the crate.
  - `pub(crate) unsafe fn $init_fn(slot: &mut MaybeUninit<Self>, init: impl FnOnce(&mut MaybeUninit<$Payload>) -> &mut $Payload) -> &mut Self`.
  - Both functions carry `by_value`'s `# Safety` text: `init` must write every field of the payload it is given.
  - An associated `const LAYOUT: ()` that asserts three things: no payload `needs_drop`; the mirror's size and align equal the enum's; and `size_of::<Self>() <= max(size_of::<$Payload>()…) + align_of::<Self>()`. Every generated fn evaluates it with `let () = Self::LAYOUT;`, so a generic slot is checked when it is monomorphised.
  - `#[cfg(test)] pub(crate) fn mirror_parts(&self) -> (u8, *const u8)`. It returns the tag byte and the payload address, both read through the mirror.

- [ ] **Step 1: Write the failing test** `slot_layout_matches_repr` in `in_place.rs`. Build a toy slot: `in_place_enum! { enum Toy<T: Copy> { Small(u8) => rebuild_small, init_small; Wide(Wide<T>) => rebuild_wide, init_wide; } }` with `struct Wide<T>([T; 3], u16)` and `T = u64`. Build it in a `Box::<Toy<u64>>::new_uninit()` filled with `0xA5`, using `init_small`. Then assert:

```rust
assert_eq!(slot.mirror_parts().0, 0);
match &*slot { Toy::Small(p) => assert_eq!(p as *const u8, slot.mirror_parts().1), _ => panic!() }
// SAFETY: the closure writes every field.
unsafe { slot.rebuild_wide(|w| w.write(Wide([7; 3], 9))) };
assert_eq!(slot.mirror_parts().0, 1);
match &*slot { Toy::Wide(w) => { assert_eq!((w.0, w.1), ([7; 3], 9)); assert_eq!(w as *const _ as *const u8, slot.mirror_parts().1) } _ => panic!() }
```

- [ ] **Step 2: Run it to verify it fails.** Run `cargo test -p chimera-core --lib in_place`. Expected: it fails to compile with "cannot find macro `in_place_enum`".
- [ ] **Step 3: Implement `in_place_enum!` as in Interfaces.** Put `#[allow(non_snake_case)]` on the union, whose fields are named after the variants.
- [ ] **Step 4: Run the test to verify it passes.** Run `cargo test -p chimera-core --lib in_place`. Expected: PASS.
- [ ] **Step 5: Run the green gate.** Run `just check`. Expected: exit 0. The macro may be unused outside tests at this point: put `#[allow(unused_macros)]` on it, and Task 2 removes the allow.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/in_place.rs
git commit -m "in_place_enum!: a repr(C, u8) enum rebuilt where it lives"
```

---

### Task 2: `EngineSlot` replaces `Engines`

**Files:**
- Modify: `chimera-core/src/dsp/engines.rs` (the whole file), `chimera-core/src/dsp/voice.rs:10,26-63,100-112,138-145,147-158,174-227,246-275,277-322,461-462,478-489,504-505`, `chimera-core/src/dsp/modal/mod.rs:131-164` (constructors), `chimera-core/src/hw.rs`
- Test: `chimera-core/tests/engines_test.rs`, `cost_test.rs` (`Engines::cost` → `EngineSlot::cost`), `modal_test.rs` (`ModalEngine::new()` → `ModalEngine::new(params.mode)`), `memory_budget_test.rs`
- Create: `docs/adr/0051-a-voice-holds-one-engine-rebuilt-in-place.md`. Modify: `docs/adr/README.md`

**Interfaces:**
- Consumes: `in_place_enum!` (Task 1).
- Produces, in `chimera_core::dsp::engines`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind { Algo, Modal(ResonatorMode) }
impl SlotKind {
    pub fn of(p: &ParamSnapshot) -> Self;          // p.engine(), then p.modal.mode
    pub fn engine(self) -> EngineType;
}
in_place_enum! { pub enum EngineSlot { Algo(AlgoEngine) => rebuild_algo, init_algo; Modal(ModalEngine) => rebuild_modal, init_modal; } }
impl EngineSlot {
    pub fn new(kind: SlotKind) -> Self;             // tests only; by_value
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, kind: SlotKind) -> &mut Self;
    pub fn kind(&self) -> SlotKind;                 // Modal(m.mode())
    pub fn rebuild(&mut self, kind: SlotKind);      // in place; one `// SAFETY:` line per arm
    pub fn note_on(&mut self, note: MidiNote, vel: Velocity, p: &ParamSnapshot, sample_rate: u32);
    pub fn note_off(&mut self);
    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], p: &ParamSnapshot, live: &AlgoLive, sample_rate: u32);
    pub fn is_active(&self) -> bool;
    pub fn modal_playing(&self) -> Option<ResonatorMode>;
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost;   // body unchanged from Engines::cost
}
```

- `ModalEngine::new(mode: ResonatorMode) -> Self`, `ModalEngine::init_in_place(slot, mode: ResonatorMode) -> &mut Self` (writes `active_mode = mode`), and `ModalEngine::mode(&self) -> ResonatorMode`. `Default` is dropped.
- `Voice` fields: `slot: EngineSlot` and `sample_rate: u32` replace `engines`, and `active_engine` goes. The public API is unchanged, including `sample_rate()`.
- `hw::VOICE_CHAIN_BYTES: usize = 2048`.

**Keeping sound identical.** Today every engine other than `active_engine` is fresh, because `trigger` and `reset` rebuild the one being left. So rebuilding the slot whenever the engine differs plays exactly the same state. `reset(&mut self, params: &ParamSnapshot)` does `self.slot.rebuild(SlotKind::of(params))`, then runs `init_chain`. Its two callers, `fade_ended` and the VCA-lifetime end of `render`, both have `params`. This task compares only `.engine()`. Task 4 widens the comparison to the whole `SlotKind`.

- [ ] **Step 1: Write the failing tests** in `engines.rs`'s `#[cfg(test)] mod tests`:
  - `slot_kind_reads_engine_then_mode`: `SlotKind::of(&for_engine(Algo))` with any `modal.mode` is `SlotKind::Algo`. For each of the four modes, `SlotKind::of(&for_engine(Modal))` is `Modal(mode)`.
  - `engine_slot_layout_matches_repr`: build the slot boxed and poisoned (`0xA5`). For each of `[Algo, Modal(String), Modal(Modal), Modal(Bowed), Modal(Sympathetic)]`, call `rebuild(k)` and assert `kind() == k`. Assert `mirror_parts().0` is 0 for Algo and 1 for Modal, and that the payload reached through `match &*slot` has address `mirror_parts().1`.

  In `memory_budget_test.rs`, add `voice_is_its_chain_plus_one_slot`. It prints `AlgoEngine`, `ModalEngine`, `EngineSlot`, the chain (`size_of::<Voice>() - size_of::<EngineSlot>()`), `Voice` and `[Voice; 8]`. It asserts `chain <= hw::VOICE_CHAIN_BYTES` and `size_of::<EngineSlot>() <= size_of::<AlgoEngine>().max(size_of::<ModalEngine>()) + align_of::<EngineSlot>()`.
- [ ] **Step 2: Run the tests to verify they fail.** Run `cargo test -p chimera-core --lib engines && cargo test -p chimera-core --test memory_budget_test`. Expected: compile errors (`SlotKind`, `EngineSlot`, `VOICE_CHAIN_BYTES` not found).
- [ ] **Step 3: Implement.**
  - `engines.rs` implements the Interfaces above, and its module doc says the slot holds one engine, rebuilt in place (ADR 0051).
  - `voice.rs`:
    - `write_chain!`'s exhaustive pattern skips `slot: _, sample_rate: _`.
    - `init_in_place` writes `slot` with `EngineSlot::init_in_place(…, SlotKind::Algo)` and writes `sample_rate` itself.
    - `note_on`'s wait condition and `render`'s fade check become `params.engine() != self.slot.kind().engine()`.
    - `trigger` rebuilds into `SlotKind::of(params)` when the engines differ.
    - `fade_ended` drops its `active_engine` write.
    - The no-route VCA arm keeps its exhaustive `match &self.slot { EngineSlot::Algo(_) | EngineSlot::Modal(_) => … }` and its #148 comment.
    - `held_model_extra` matches `self.slot.modal_playing()`.
    - Add `const _: () = assert!(size_of::<Voice>() <= VOICE_CHAIN_BYTES + size_of::<EngineSlot>());`.
  - Update the four test files listed under Files.
  - Remove Task 1's `#[allow(unused_macros)]`.
- [ ] **Step 4: Run the tests to verify they pass, goldens included.** Run `cargo test -p chimera-core --lib engines && cargo test -p chimera-core --test memory_budget_test --test golden_test --test in_place_test --test click_free_test --test engines_test -- --nocapture`. Expected: PASS, with `goldens_match` and `goldens_match_through_the_instrument` bit-identical. Any golden mismatch means the refactor changed state: fix it, and never re-record.
- [ ] **Step 5: Write ADR 0051** with Status Proposed, "Supersedes 0008", and Deciders project owner.
  - **Context:** 0008's 66 KB by-value risk; `in_place.rs` now builds in place; a `Voice` is 35,248 B and Modal 2 needs room.
  - **Decision:** `EngineSlot` is a `repr(C, u8)` slot generated by `in_place_enum!`. It is rebuilt in place when a fade ends and when an idle voice triggers a different kind. The variant is the engine.
  - **Alternatives:** keep `Engines` (the sum grows with every Modal 2 exciter); a by-value enum (stack); a shared buffer pool (aliasing, and every voice needs its own state).
  - **Consequences:** memory is the largest engine, not the sum; at most 2 rebuilds per voice per block; the new `unsafe` lives in `in_place.rs` only.
  - **Sources:** the spec and this plan.

  In the README, add row 0051 and change 0008's status cell to `Superseded by [0051](0051-a-voice-holds-one-engine-rebuilt-in-place.md)`. Do not touch 0008's file.
- [ ] **Step 6: Run the green gate.** Run `just check`. Expected: exit 0 (stack-check included).
- [ ] **Step 7: Commit.**

```bash
git add chimera-core/src/in_place.rs chimera-core/src/dsp/engines.rs chimera-core/src/dsp/voice.rs chimera-core/src/dsp/modal/mod.rs chimera-core/src/hw.rs chimera-core/tests/engines_test.rs chimera-core/tests/cost_test.rs chimera-core/tests/modal_test.rs chimera-core/tests/memory_budget_test.rs docs/adr/0051-a-voice-holds-one-engine-rebuilt-in-place.md docs/adr/README.md
git commit -m "A voice holds one engine: EngineSlot replaces Engines"
```

---

### Task 3: `ModelSlot` in `ModalEngine`

**Files:**
- Modify: `chimera-core/src/dsp/modal/mod.rs:51-569`
- Test: `chimera-core/src/dsp/modal/mod.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `in_place_enum!` (Task 1), `ModalEngine::{new, init_in_place, mode}(…mode)` (Task 2).
- Produces the following (all private to `modal`, except where marked):

```rust
struct ModalBank { filters: [Svf; MAX_MODES], cos_osc: CosineOsc, resolution: usize }
struct SympatheticStrings { main: KsString, strings: [KsString; NUM_SYMPATHETIC], ratios: [f32; NUM_SYMPATHETIC] }
in_place_enum! { enum ModelSlot {
    Bank(ModalBank) => rebuild_bank, init_bank;
    String(KsString) => rebuild_string, init_string;
    Bowed(KsString) => rebuild_bowed, init_bowed;
    Sympathetic(SympatheticStrings) => rebuild_sympathetic, init_sympathetic;
} }
impl ModelSlot {
    fn init_in_place(slot: &mut MaybeUninit<Self>, mode: ResonatorMode) -> &mut Self;
    fn rebuild(&mut self, mode: ResonatorMode);   // one `// SAFETY:` line per arm
    fn mode(&self) -> ResonatorMode;
}
```

- `ModalEngine` fields become `model: ModelSlot, frequency, pitch, tuned, released, exciter_remaining, exciter_amp, noise_state, exciter_lp, active, silence_counter`. `active_mode`, `filters`, `cos_osc`, `resolution`, `string`, `sym_strings` and `sym_ratios` go, and `field_list!` is updated.
- The public `mode()` and `playing()` read the variant.
- `render_modal(bank: &mut ModalBank, …)`, `render_string(s: &mut KsString, …)`, `render_bowed(s: &mut KsString, …)` and `render_sympathetic(m: &mut SympatheticStrings, …)` each take only their own payload. Shared fields are passed as arguments. `compute_filters`, `retune`, `note_on` and `note_off` each become one `match &mut self.model`.
- `note_on` first calls `self.model.rebuild(params.mode)` if `params.mode != self.mode()`. This keeps `ModalEngine`'s own contract ("note_on plays `params.mode`") for direct users such as `modal_test.rs`. The note-on bodies are unchanged.

- [ ] **Step 1: Write the failing tests** in `modal/mod.rs`:
  - `model_slot_layout_matches_repr`: for each of the four modes, a boxed, poisoned `ModelSlot` is `rebuild`-ed into that mode. Assert `mode()` round-trips, `mirror_parts().0 == mode index in declaration order (Bank 0, String 1, Bowed 2, Sympathetic 3)`, and the payload address equals `mirror_parts().1`.
  - `a_mode_change_at_note_on_plays_like_a_fresh_engine`: engine A plays String for 40 blocks, then `note_on` with Sympathetic. Engine B is `ModalEngine::new(Sympathetic)` with the same note-on. The next 20 blocks must be bit-identical.
- [ ] **Step 2: Run the tests to verify they fail.** Run `cargo test -p chimera-core --lib modal`. Expected: compile error (`ModelSlot` not found).
- [ ] **Step 3: Implement the Interfaces above.** `ModalBank::init_in_place` and `SympatheticStrings::init_in_place` follow `KsString::init_in_place`'s pattern. Sympathetic's `ratios` start at `[1.0; 7]`.
- [ ] **Step 4: Run the tests to verify they pass, with goldens and instrument tests.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test golden_test --test modal_test --test modal_integration_test --test in_place_test --test memory_budget_test -- --nocapture`. Expected: PASS and goldens bit-identical. `ModalEngine` shrinks by about the bank's 984 B (host), because the bank now overlaps Sympathetic.
- [ ] **Step 5: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/modal/mod.rs
git commit -m "Modal holds one model: ModelSlot"
```

---

### Task 4: Switching on `SlotKind`, the rebuild count, and the switch tests

**Files:**
- Modify: `chimera-core/src/dsp/voice.rs` (`note_on`, `trigger`, `reset`, `render`'s fade check, `init_in_place`, `write_chain!`)
- Create: `chimera-core/tests/exclusive_state_test.rs`, `chimera-core/tests/rebuild_stack_test.rs`
- Modify: `docs/adr/0051-a-voice-holds-one-engine-rebuilt-in-place.md` (still Proposed)

**Interfaces:**
- Consumes: `SlotKind::of`, `EngineSlot::{kind, rebuild}` (Task 2), `ModelSlot` (Task 3).
- Produces `Voice::rebuilds(&self) -> u16`, a wrapping count of slot rebuilds. It is written once in `init_in_place` and skipped by `write_chain!`, so `reset` never clears it.
- `fn rebuild(&mut self, kind: SlotKind)` is private to `Voice`. It calls `slot.rebuild(kind)` and adds 1 to `rebuilds`, wrapping. It is the only path to `slot.rebuild`.

The rule (§ 3) replaces each `.engine()` comparison from Task 2 with a whole-kind comparison, `SlotKind::of(params) != self.slot.kind()`. It applies in `note_on`'s wait condition, in `render`'s fade check (sounding voice) and in `trigger` (idle voice: `self.rebuild(kind)`, then play in the same block). `reset` always calls `self.rebuild(SlotKind::of(params))`.

- [ ] **Step 1: Write the failing tests** in `exclusive_state_test.rs`. Use `SR`, `tri()` and `rms` from `common`, and a local `max_step(x: &[f32]) -> f32` as in `click_free_test.rs`. A "fresh voice" is `Voice::new(SR)` given the same note and velocity on the new Sound. `sym`/`string` are `for_engine(Modal)` with `modal.mode` set.
  - `switch_never_clicks`: run two cases, `tri()` → `for_engine(Modal)` and `string` → `sym`. Hold note 60 for 8 blocks, and let `S` and `A` be `max_step` and the peak of block 8. Then render with the new Sound for `FADE / BLOCK_SIZE` blocks. Assert `max_step(fade, with the last pre-switch sample prepended) <= S + A / Voice::FADE as f32`. The next 8 blocks must be bit-identical to a fresh voice's first 8.
  - `idle_voice_switches_in_the_same_block`: play a `tri()` note, `note_off`, and render until `!is_active()`. Record `r = rebuilds()`. Then render 3 blocks each on `for_engine(Modal)`, `tri()` and `sym`, which is three kind changes while idle. Assert `rebuilds() == r`. Then `note_on` with `sym`. Its first block must be bit-identical to a fresh `sym` voice's, and `rebuilds() == r + 1`.
  - `other_parts_are_untouched_by_a_switch`: two `Instrument`s on the same events. Part 1 (channel 0) plays `tri()` note 60, and Part 2 (channel 1) plays `string` note 64. All sends are 0.5 with reverb mix 0.5. At block 20 both notes go off, and at block 21 Part 1's Sound becomes `for_engine(Modal)` in run B only. Over blocks 0..60, `part_bus(1)` must be bit-identical between runs A and B. In run B, the DAC output of block 22 has a sample `!= 0.0` (the reverb tail rings on).
  - `model_switch_rebuilds_once`: a held `string` note renders 8 blocks. Record `r`. The Sound becomes `sym` for `FADE / BLOCK_SIZE + 8` blocks. Assert `rebuilds() == r + 1`, and the last 8 blocks must be bit-identical to a fresh `sym` voice's first 8.
  - `knob_moves_never_rebuild`: a held `sym` note, 60 blocks, run twice. In the moved run, before each block, every `BlockRef::ALL` block that `p.block_mut(b)` returns gets every spec `nudge`d by `+1` on even blocks and `-1` on odd ones, except `(BlockRef::Modal, ModalParams::MODE)`. Assert `rebuilds()` is unchanged, `is_active()` holds throughout, and the moved output differs from the unmoved one.
  - `a_switch_back_mid_fade_restarts_on_the_sounds_kind` (Review Focus 2): a held `string` note renders 8 blocks, then 1 block on `sym`, then `string` again. Assert `rebuilds() == r + 1`, and the blocks after the fade must be bit-identical to a fresh `string` voice's.
  - `a_steal_across_kinds_plays_the_new_kind_clean` (Review Focus 3) has two halves.
    - Instrument half: Part 1 = `tri()` holds 8 notes, which fills the pool. Part 2 = `sym` then plays note 50. Render until Part 2's bus is non-zero. For 8 blocks, that bus must be bit-identical to a lone fresh `sym` voice's output from its first sounding block.
    - Voice half: `Instrument` exposes no `Voice`, so this half pins the rebuild bound. Replay the steal on a `Voice`: `kill` a sounding `tri()` voice, render it with `tri()` until `!is_active()`, then `note_on` with `sym` before the next render. `rebuilds()` must have risen by exactly 2 since the `kill` (the fade end's rebuild, then the note's). The next 8 blocks must be bit-identical to a fresh `sym` voice's.

  In `rebuild_stack_test.rs`, write `rebuild_fits_a_small_stack`. It spawns `std::thread::Builder::new().stack_size(size_of::<ModalEngine>())`. On that thread it builds a `Voice` with `Box::<Voice>::new_uninit()` and `Voice::init_in_place`, and plays `tri()` → `string` → `sym` → `tri()` switches, each held for 4 blocks. `join()` returns `Ok`.
- [ ] **Step 2: Run the tests to verify they fail.** Run `cargo test -p chimera-core --test exclusive_state_test`. Expected: compile error (`rebuilds` not found). After a stub `rebuilds()` that returns 0, `model_switch_rebuilds_once`, `switch_never_clicks` (the MODE case) and `a_switch_back_mid_fade…` fail.
- [ ] **Step 3: Implement the rule above.**
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test exclusive_state_test --test rebuild_stack_test --test golden_test --test click_free_test --test instrument_test --test engine_switch_test`. Expected: PASS and goldens bit-identical. An existing test may fail only if it asserts that a MODE edit leaves a ringing note on its old model. If so, update it to the § 3 rule and name it in the commit body. Any other failure means stop and investigate.
- [ ] **Step 5: Prove the stack test can fail.** Temporarily replace the body of `EngineSlot::rebuild` with `*self = match kind { SlotKind::Algo => EngineSlot::Algo(AlgoEngine::new()), SlotKind::Modal(m) => EngineSlot::Modal(ModalEngine::new(m)) };`. Run `cargo test -p chimera-core --test rebuild_stack_test`. Expected: the process aborts with "has overflowed its stack". Revert, and confirm `git diff chimera-core/src/dsp/engines.rs` is empty. If it does not overflow (opt-level 1 may build in place), STOP and report: the test cannot fail, and the spec's gate needs the owner.
- [ ] **Step 6: Amend ADR 0051** (still Proposed) with the switch rule. A change to the Sound's `SlotKind` fades that Part's sounding voices over `Voice::FADE`, and each is rebuilt once silent. A held key restarts on the new kind. Idle voices switch at their next note. Knob moves never rebuild. A MODE edit now fades ringing notes where it used to leave them on the old model.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/voice.rs chimera-core/tests/exclusive_state_test.rs chimera-core/tests/rebuild_stack_test.rs docs/adr/0051-a-voice-holds-one-engine-rebuilt-in-place.md
git commit -m "A MODE change fades and rebuilds, as an engine change does"
```

---

### Task 5: 16-bit block-float strings

**Files:**
- Create: `chimera-core/src/dsp/modal/q16.rs`
- Modify: `chimera-core/src/dsp/modal/string.rs` (the whole `KsString`), `chimera-core/src/dsp/modal/mod.rs` (generic over `S`, every buffer access)
- Test: `q16.rs` and `modal/mod.rs` test modules, `chimera-core/tests/golden_test.rs`
- Create: `docs/adr/0052-strings-stored-as-16-bit-block-float.md`. Modify: `docs/adr/README.md`

**Interfaces:**
- Consumes: `ModelSlot`, `ModalBank`, `SympatheticStrings` (Task 3); `in_place_enum!`'s generic form (Task 1).
- Produces the following in `q16.rs`. `Exp`, `Q16`, `StepBudget` and `Store` are `pub` and re-exported from `chimera_core::dsp::modal`, because they bound public generics and the bench uses them in Task 6. `store`, `load` and `next_exp` are `pub` within the private `q16` module.

```rust
pub struct Exp(u8);                       // private field: always 14..=24
impl Exp { pub const START: Exp; pub const MAX: Exp; pub fn get(self) -> u8; }
pub fn store(x: f32, e: Exp) -> i16;
pub fn load(q: i16, e: Exp) -> f32;
pub fn next_exp(peak: u16, e: Exp) -> Exp;
pub struct StepBudget(bool);
impl StepBudget { pub const fn one() -> Self; pub fn take(&mut self) -> bool; }
pub trait Store {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self where Self: Sized; // zeros, e = START
    fn load(&self, i: usize) -> f32;
    fn store(&mut self, i: usize, x: f32);
    fn wrapped(&mut self, budget: &mut StepBudget);  // the write position wrapped
    fn restart(&mut self);                           // a note starts: e back to START, values kept
    fn clear(&mut self);                             // zeros, e = START
}
impl Store for [f32; MAX_STRING_DELAY]   // today's arithmetic; wrapped/restart are no-ops
pub struct Q16 { q: [i16; MAX_STRING_DELAY], e: Exp, peak: u16 }
impl Q16 { pub fn exp(&self) -> Exp; }
```

- `KsString<S: Store = Q16>` has a `line: S` field in place of `buffer`. `tick_full(&mut self, p, budget: &mut StepBudget) -> f32`, and `damp(&mut self, passes: u32)` loads each of `[..delay_len]` once, multiplies it by `0.2` `passes` times in f32, and stores it once. That is bit-identical to today's repeated passes under f32. `SympatheticStrings<S>`, `ModelSlot<S: Store>` and `ModalEngine<S: Store = Q16>` are generic too. `EngineSlot` keeps plain `ModalEngine`.

**The algorithm (§ 4; the signature does not decide it):**

```rust
// q16.rs — full scale at e is 2^(15−e); ¼ FS is |q| 8192, ½ FS is |q| 16384.
fn pow2(e: i32) -> f32 { f32::from_bits(((127 + e) as u32) << 23) }   // exact
pub fn store(x: f32, e: Exp) -> i16 { libm::roundf(x * pow2(e.0 as i32)) as i16 } // `as`: saturates, NaN → 0
pub fn load(q: i16, e: Exp) -> f32 { f32::from(q) * pow2(-(e.0 as i32)) }
pub fn next_exp(peak: u16, e: Exp) -> Exp {
    if peak < 8192 && e.0 < 24 { Exp(e.0 + 1) }
    else if peak > 16384 && e.0 > 14 { Exp(e.0 - 1) }
    else { e }
}
impl Store for Q16 {
    fn store(&mut self, i, x) { let v = store(x, self.e); self.q[i] = v; self.peak = self.peak.max(v.unsigned_abs()); }
    fn wrapped(&mut self, budget) {
        let next = next_exp(self.peak, self.e);
        if next != self.e && budget.take() {
            if next.0 > self.e.0 { for v in &mut self.q { *v = v.saturating_mul(2) } }        // exact below ½ FS
            else { for v in &mut self.q { *v = ((i32::from(*v) + 1) >> 1) as i16 } }         // round half up
            self.e = next;
        }
        self.peak = 0;                         // a denied step waits for the next wrap
    }
    fn restart(&mut self) {                    // stale samples keep their level (Review Focus 1)
        let k = u32::from(self.e.0 - 14);
        if k > 0 { for v in &mut self.q { *v = ((i32::from(*v) + (1 << (k - 1))) >> k) as i16 } }
        self.e = Exp::START; self.peak = 0;
    }
}
```

Steps run over the whole 984-sample ring, so samples past `delay_len` stay at the line's exponent if a retune lengthens the loop.

Where the calls go:
- `tick_full` calls `wrapped` when the new write position `<=` the old one.
- `render_bowed` calls `wrapped` when `write_pos` returns to 0.
- `trigger` calls `restart` first.
- Bowed's note-on calls `clear`.
- Sympathetic's note-on calls `restart` on each of the seven strings, then stores `0.0` over `[..delay_len]`.
- The sympathetic injection becomes `store(wp, load(wp) + sym_input)`.
- `note_off` becomes `damp(3)` for String, `damp(5)` for Bowed and `damp(1)` for Sympathetic.
- `ModalEngine::render` makes one `StepBudget::one()` per block and passes it to every string of the voice.

- [ ] **Step 1: Write the failing tests.**
  - In `q16.rs`, write `q16_saturates_and_steps`. At `Exp::START`, `store` of `3.0`, `f32::INFINITY`, `-3.0`, `f32::NEG_INFINITY` and `f32::NAN` gives `32767, 32767, -32768, -32768, 0`. `load(store(0.5, START), START) == 0.5`. `next_exp(8191, Exp(14)) == Exp(15)`, `next_exp(8192, Exp(14)) == Exp(14)`, `next_exp(16385, Exp(20)) == Exp(19)` and `next_exp(16384, Exp(20)) == Exp(20)`. Over every `peak` in `0..=32768` and every `e` in `14..=24`, `next_exp` stays in `14..=24`. A `Q16` holding `q = 4000` at `e = 14` whose `wrapped` takes a step reads back `load == 4000.0 * 2^-14` exactly at `e = 15`. With the budget already taken, `e` is unchanged.
  - In `modal/mod.rs`, write `strings_i16_match_f32`. Use `fn play<S: Store>(p: &ModalParams, note: u8, release_block: Option<usize>, blocks: usize) -> (Vec<f32>, Option<usize>)`, which returns the output and the first block with `!is_active()`. Velocity is 100 and the default params apply except as listed.
    - With `ks_feedback = 0.0`, each of String, Bowed and Sympathetic plays notes 31 (G1), 69 (A4) and 84 (C6) at `decay` 0.0 and 0.3. Add String note 45 (A2) with `ks_ens_mix = 0.5, ks_ens_depth = 0.5`. Bowed releases at 0.5 s, and every run lasts 10 s.
    - Over the first second, `20·log10(rms(q − f)) <= -90.0`.
    - Over 480-sample windows, `|20·log10(peak_q / peak_f)| <= 0.1` wherever `peak_f > 0.001`.
    - The quiet blocks differ by no more than 15 (20 ms), or both are `None`.
    - With `ks_feedback` 0.2 and 1.0, String A4 and Sympathetic A3 (57) run for 1 s. Every sample is finite and `|y| <= 1.5`, and the window peaks are within 2 dB.
  - In `modal/mod.rs`, write `pitch_down_after_a_long_tail_reads_no_burst` (Review Focus 1). Each engine plays String G1 at `decay` 0 for 3 s, then `note_on` A4, renders 1 block, calls `set_pitch(0.25)` and renders 8 blocks. The peaks of `Q16` and `[f32; 984]` over those 8 blocks differ by at most 0.01.
  - In `modal/mod.rs`, write `a_short_loop_steps_at_most_once_a_block` (Review Focus 5). A `Q16` Sympathetic engine plays C7 (96) for 2 s, rendered block by block. The number of strings whose `line.exp()` changed during a block is at most 1 every block, and it is at least 1 in some block.
- [ ] **Step 2: Run the tests to verify they fail.** Run `cargo test -p chimera-core --lib modal`. Expected: compile errors (`q16` and `Store` not found).
- [ ] **Step 3: Implement `q16.rs`, the generic `KsString<S>` and the generic `ModalEngine<S>`, with both defaults set to `[f32; MAX_STRING_DELAY]` for now** (`KsString<S: Store = [f32; MAX_STRING_DELAY]>`, and the same for `ModalEngine`). This first commit proves that the generic refactor alone changes nothing. In `modal/mod.rs`, add `mod q16; pub use q16::{Exp, Q16, StepBudget, Store};`. The pure functions stay reachable as `q16::…` inside `modal` only.
- [ ] **Step 4: Run the tests and goldens.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test golden_test --test modal_test --test modal_integration_test --test in_place_test`. Expected: PASS, and every golden is bit-identical. If a gate figure misses, STOP and report the measured numbers: do not loosen a bound, and do not change the algorithm silently.
- [ ] **Step 5: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/modal/q16.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs
git commit -m "Modal generic over its string store; Q16 gated against f32"
```

- [ ] **Step 7: Switch the default to `Q16`.** Change both defaults, `ModalEngine<S: Store = Q16>` and `KsString<S: Store = Q16>`. Run `cargo test -p chimera-core --test golden_test -- goldens_match`. Expected: exactly `modal_init`, `modal_lfo_cutoff` and `algo_to_modal_switch` mismatch. If any other case mismatches, STOP.
- [ ] **Step 8: Re-record the three.** Run `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test -- goldens_match --nocapture` and paste only those three rows. Each row's comment becomes `// Re-recorded: Modal strings stored as 16-bit block float (exclusive-state spec § 4).` Every other row stays byte-identical, which `git diff` must show. Then run `cargo test -p chimera-core --test golden_test`. Expected: PASS, `goldens_match_through_the_instrument` included.
  - **If `algo-init-morph` is already on `main` when this branch rebases:** re-record `algo_to_modal_switch` once more after the rebase, and give its comment both reasons. Whichever branch lands second does this (spec § Interactions).
- [ ] **Step 9: Check the sizes.** Run `cargo test -p chimera-core --test memory_budget_test -- --nocapture`. Expected (host): `ModalEngine` ≈ 16,080, `Voice` ≈ 17,928 and `[Voice; 8]` ≈ 143,424. If any is more than 5 % above, STOP and report.
- [ ] **Step 10: Write ADR 0052** (Proposed).
  - **Context:** the spec's prototype numbers. Round-to-nearest Q1.14 never goes quiet, and truncation halves the tails.
  - **Decision:** the block exponent above, with the gate (RMS error re full scale over 1 s with FDBK 0, plus envelope and bounds checks with feedback).
  - **Alternatives:** f32 strings (~13 KB freed); plain Q1.14; truncation.
  - **Consequences:** about 15,680 B freed per voice; a step costs ≤ 984 halfword shifts and happens at most once per voice per block.
  - **Sources:** the spec § 4 and this plan.

  Add its README row.
- [ ] **Step 11: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 12: Commit.**

```bash
git add chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/golden_test.rs docs/adr/0052-strings-stored-as-16-bit-block-float.md docs/adr/README.md
git commit -m "Strings stored as 16-bit block float; Modal goldens re-recorded"
```

---

### Task 6: Chip figures

**Files:**
- Modify: `chimera-stm32/src/bench.rs`
- Modify: `docs/adr/0051-…md`, `docs/adr/0052-…md` (figures only, still Proposed)

**Interfaces:**
- Consumes: `EngineSlot::{init_in_place, rebuild}`, `SlotKind` (Task 2), `Q16`, `Store`, `StepBudget` (Task 5), `Voice`.
- Produces three bench additions:
  - `#[inline(never)] fn time_rebuild() -> u32` gives the cycles per `rebuild(SlotKind::Modal(Sympathetic))`, averaged over 16 rebuilds that alternate with `rebuild(SlotKind::Algo)`. Only the Sympathetic ones are timed. It runs on a `static mut MaybeUninit<EngineSlot>` in the voices' D2 section (`.ram_d2.voices`, as in `audio/engine.rs`).
  - `#[inline(never)] fn time_exp_step() -> u32` gives the cycles of one `Q16::wrapped` that steps. It runs on a static `Q16` built with `Store::init_in_place`, with every index filled by `Store::store(line, i, 0.01)`, so the peak is below ¼ full scale and the step goes up.
  - A `ROUTING` row `("SWITCH", |p| modal(p, ResonatorMode::String), switch_storm)`. `switch_storm` flips `p.params.modal.mode` between String and Sympathetic every 4 blocks. `ROUTING_ROWS` becomes 29.
  - `fn show_memory(display, rebuild: u32, step: u32)` draws one more held screen, titled `MEMORY`, with these lines:
    - `VOICE {size_of::<Voice>()}`
    - `SLOT {size_of::<EngineSlot>()}`
    - `MODAL {size_of::<ModalEngine>()}`
    - `POOL {size_of::<[Voice; MAX_VOICES]>()}/{VOICE_RAM_BUDGET}`
    - `REBUILD {rebuild} CYC`
    - `EXP STEP {step} CYC`

    It is shown after the ROUTING pages.

- [ ] **Step 1: Implement the bench additions.** Put a `// SAFETY:` comment on each new `static mut` access, as for `SCOPE`/`SHARED`. Nothing large may go on the stack: the slot and the line are statics.
- [ ] **Step 2: Run the green gate and record the target's section sizes.** Run `just check && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench && "$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-size" -A target/thumbv7em-none-eabihf/release/chimera-stm32`. Expected: exit 0. Keep the `.ram_d2*` section size for the ADR.
- [ ] **Step 3: Commit.**

```bash
git add chimera-stm32/src/bench.rs
git commit -m "Bench: rebuild, exponent step, switch storm, memory"
```

- [ ] **Step 4: STOP. The owner measures on hardware.** Hand the owner this checklist and wait for the figures:
  1. **Baseline.** In a scratch worktree of `main` (937b89f), run `just flash-bench`. Let the bench pages run through. On the normal UI, load a Modal Sound and hold an 8-note chord. Flip MODE String ↔ Sympathetic 5 times, then ENGINE Algo ↔ Modal 5 times. Open System › AUDIO and read **STACK** (K).
  2. **Branch.** From this worktree, run `just flash-bench` and read the following:
     - First screen: **MODAL /VOICE**. It must be ≤ 447, today's billed `Voice::cost` for the default Modal Sound.
     - ROUTING pages: **MDL SYM /VOICE** (≤ 1,457 billed), **MDL STR /VOICE** and **SWITCH /VOICE** with its 8 counts.
     - MEMORY screen: **VOICE, SLOT, MODAL, POOL, REBUILD, EXP STEP**.
  3. Repeat the chord and MODE/ENGINE flips from step 1, then read AUDIO **STACK**. It must be ≤ the baseline + 1 K (§ Tests: at most 1 KB of growth; the page shows whole KB).
  4. Run `just flash` to restore the normal firmware.

  If MODAL or MDL SYM exceeds its billed figure, or STACK grows by more than 1 K, STOP and report to the owner. Do not raise a `COST_*` constant or change the design.
- [ ] **Step 5: Record the figures.** Add a "Measured on the chip (rev V, 480 MHz, 2026-MM-DD)" paragraph to ADR 0051 (VOICE, SLOT, POOL, `.ram_d2*` size, REBUILD, SWITCH, and STACK before/after) and to ADR 0052 (MODAL, MDL STR, MDL SYM, EXP STEP) with the owner's readings. Run `just check`. Expected: exit 0.
- [ ] **Step 6: Commit.**

```bash
git add docs/adr/0051-a-voice-holds-one-engine-rebuilt-in-place.md docs/adr/0052-strings-stored-as-16-bit-block-float.md
git commit -m "Chip figures for ADRs 0051 and 0052"
```
