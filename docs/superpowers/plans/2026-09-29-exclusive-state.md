# Exclusive State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A voice holds only the engine it plays, Modal holds only the model it plays, and Sympathetic's seven sympathetic strings live in a shared pool of four f32 slots, with no audible change.

**Revised 2026-09-29 (owner).** Task 5 (16-bit strings) and Task 6 are replaced by Tasks 7–10: strings go back to f32 and Sympathetic borrows its seven lines from a pool of four (spec § 4). Tasks 1–4 stand as built.

**Architecture:** One macro in `in_place.rs`, `in_place_enum!`, declares a `#[repr(C, u8)]` enum and generates in-place constructors for each variant. Two slots use it: `EngineSlot` (Algo | Modal), which replaces `Engines` in `Voice`, and `ModelSlot` (Bank | String | Bowed | Sympathetic) inside `ModalEngine`. `Voice` widens its existing 128-sample engine-change fade to any `SlotKind` change and rebuilds the slot in place when the fade ends. The `Instrument` owns a `SymPool` in D2: four `SympatheticSet`s (seven f32 strings each) and a pure allocator, `SymAlloc`, that lends each a non-`Copy` `Lease`. `ModelSlot::Sympathetic` holds the main string and its `Lease`. The functional core is pure and host-tested (`SlotKind::of`, `SlotKind::resting`, `SymAlloc`), and the voice's switch logic and the Instrument's placement are thin shells over it.

**Tech Stack:** Rust 2024, `no_std` `chimera-core` (f32 DSP, `libm`), `thumbv7em-none-eabihf` firmware, `just`.

**Spec:** `docs/superpowers/specs/2026-09-29-exclusive-state-design.md` (owner-approved, binding). Read it with this plan. The § numbers below are the spec's.

**Where it runs:** worktree `/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/wt-excl`, branch `exclusive-state`. Every command runs from that root.

## Global Constraints

- **Unchanged:** `MAX_VOICES` 8, `MAX_STRING_DELAY` 984 (ADR 0040), `VOICE_RAM_BUDGET` 286,720, `Voice::FADE` = 128 samples, all `ModalEngine::COST_*` and `PITCH`, the UI, and the #191 behaviours. `ResonatorMode` keeps its four variants and codes, and `modal/params.rs` is not touched. `SYM_SLOTS` = 4 (spec § 4); the per-Part VOICES control that will show it is #207, not built here.
- **Types decide (ADR 0012):** a slot's variant *is* its engine or model. No field records which one is active. Render dispatches with a `match` on the slot, so each arm sees only its own payload. There is no wildcard arm on either slot.
- **Functional core, imperative shell:** `SlotKind::of`, `SlotKind::resting` and every `SymAlloc` method are pure and have unit tests. `Voice` and `Instrument` only call them.
- **Audio thread:** no heap, no blocking, and no engine-sized value on the stack. Every rebuild writes through a pointer into the slot. `just stack-check` must stay green.
- **`unsafe`:** new `unsafe` goes only into `in_place.rs`, inside `in_place_enum!` or beside `uninit_at`/`by_value` (Task 9's `move_out`). Each caller of an `in_place.rs` `unsafe fn` gets one `// SAFETY:` line. No other new `unsafe`.
- **Sizes (spec § Memory):** `pub const VOICE_CHAIN_BYTES: usize = 2048` in `hw.rs`. Assert that a slot is at most its largest payload plus `align_of` of the slot, and that `size_of::<Voice>() <= VOICE_CHAIN_BYTES + size_of::<EngineSlot>()`. The `[Voice; MAX_VOICES] <= VOICE_RAM_BUDGET` assert stays.
- **The pool (§ 4):** a `Lease` is never `Clone` or `Copy` and has no public constructor; only `SymAlloc::lend` makes one and only `SymAlloc::give_back` takes one. A Sympathetic model can't be built without one. An idle voice never holds one. The pool is built once, in place, and never rebuilt.
- **Goldens (ADR 0011):** Tasks 1–4 keep every golden bit-identical. Task 7 restores the rows Task 5 re-recorded to their pre-Task-5 values, byte for byte (spec § 4.8), and records one new row, `modal_sympathetic`. Tasks 8–10 keep every golden bit-identical, `modal_sympathetic` included. Nothing is re-recorded after Task 7.
- **ADRs:** `docs/adr/0051-*.md` supersedes 0008. `docs/adr/0052-*.md` (16-bit strings) is `Superseded by 0054` since the spec revision; its file stays. `docs/adr/0054-*.md`, written in Task 9, covers the sympathetic slot pool. Each uses `0000-template.md`, carries `Status: Proposed`, and has a row in `docs/adr/README.md`. Never edit an accepted ADR. 0008's file stays as it is, and only its README row's status changes.
- **Green gate per task:** `just check` passes. It runs the core, HAL and desktop tests, all firmware builds, `just clippy` and `just stack-check`. If ALSA's pkg-config is missing, set `PKG_CONFIG_PATH` as the Justfile says.
- **Commits:** terse, no type prefix, and never a Co-Authored-By or other AI attribution line. Stage named paths only. Never stage `docs/chimera-ui-ux-spec.md` or `chimera.bin`.

## Review Focus

*Added during execution (2026-09-29, final review): the owner replaced the Rings rule with no stealing. A Sympathetic note with no free slot plays bare, its main string alone; nothing fades or is stolen for the pool; on a switch the last four played ring and the rest play bare. Focus 1 below, Task 8's `place` steal, `restart` and `Then::To`, and Task 9's steal, eviction and Mono tests are superseded: see spec § 4.1 and § 4.5 and ADR 0054. Their tests became `fifth_note_plays_bare_while_four_ring`, `nothing_is_faded_or_stolen_for_the_pool`, `a_freed_slot_goes_to_the_next_new_note_not_a_ringing_bare_one`, `a_bare_note_never_gains_a_halo` and `switch_gives_the_last_four_played_a_halo`.*

1. **A fifth Sympathetic note.** Four Sympathetic notes hold every slot, and a fifth arrives. The oldest should fade over `FADE` and the new note play on that voice, in that slot, with no other voice able to take the slot in between. Test: `a_fifth_sympathetic_note_steals_the_oldest` (Task 9).
2. **Switching back before the fade ends.** A player turns MODE String → Sympathetic → String within one block. There should be one fade and one rebuild, and the held note should restart on the Sound's kind. Test: `a_switch_back_mid_fade_restarts_on_the_sounds_kind` (Task 4).
3. **A steal across Parts of different kinds.** With the pool full on an Algo Part, a note on a Sympathetic Part steals a voice. The new note should play exactly as on a fresh voice, with at most 2 rebuilds in that block. Test: `a_steal_across_kinds_plays_the_new_kind_clean` (Task 4).
4. **Several switches while a voice is idle.** An idle voice should rebuild once, at its next note-on, not once per edit. This is folded into `idle_voice_switches_in_the_same_block` (Task 4).
5. **A lease that never comes home.** Kills, steals, MODE and ENGINE flips, natural decays and project loads all end Sympathetic notes. Every slot must be free again once the voices are idle. Test: `every_lease_comes_home` (Task 9).
6. **Eight held notes switched to Sympathetic.** The last four played restart, evicting older holders on other Parts if need be; the other four fade and stay silent until played again. Test: `the_last_four_played_restart_through_the_instrument` (Task 9).

## Files

| File | Responsibility | Tasks |
|---|---|---|
| `chimera-core/src/in_place.rs` | `in_place_enum!`: the repr, the private mirror, per-variant `rebuild_*`/`init_*`, layout asserts | 1 |
| `chimera-core/src/dsp/engines.rs` | `EngineSlot`, `SlotKind` (replaces `Engines`) | 2 |
| `chimera-core/src/dsp/voice.rs` | `slot`, `sample_rate`, `rebuilds`; the switch rule | 2, 4 |
| `chimera-core/src/hw.rs` | `VOICE_CHAIN_BYTES` | 2 |
| `chimera-core/src/dsp/modal/mod.rs` | `ModelSlot`, `ModalBank`, `SympatheticVoice`, `SympatheticSet`, `SymPool` | 2, 3, 7, 9 |
| `chimera-core/src/dsp/modal/string.rs` | `KsString` (f32 again) | 7 |
| `chimera-core/src/dsp/modal/q16.rs` | deleted | 7 |
| `chimera-core/src/sym_alloc.rs` (new) | pure core: `SymAlloc`, `Lease`, `SymSlot`, `Place`, `SYM_SLOTS` | 8 |
| `chimera-core/src/voice_alloc.rs` | `VoiceIdx`; `pick` public, `book` | 8, 9 |
| `chimera-core/src/instrument.rs` | `sym: SymPool`; Sympathetic placement; `rest` at step 5 | 9 |
| `chimera-core/tests/exclusive_state_test.rs` (new) | the switch tests of spec § Tests | 4, 9 |
| `chimera-core/tests/sym_pool_test.rs` (new) | the pool's Instrument tests of spec § Tests | 9 |
| `chimera-core/tests/rebuild_stack_test.rs` (new) | `rebuild_fits_a_small_stack`, alone in its binary because a stack overflow aborts the process | 4, 9 |
| `chimera-core/tests/common/{mod,rig}.rs` | `Case::ModalSympathetic`; `Rig` (a boxed `Voice` and `SymPool` with the old call signatures) | 7, 9 |
| `chimera-core/tests/{engines,cost,modal,memory_budget,golden,instrument,codec_compat}_test.rs` | API moves, size prints, goldens restored | 2, 3, 7, 9 |
| `chimera-stm32/src/bench.rs` | REBUILD, SYM NOTE-ON, SWITCH storm, MEMORY screen | 10 |
| `docs/adr/0051-*.md`, `0052-*.md`, `0054-*.md`, `README.md` | the ADRs | 2, 4, 9, 10 |

**Deviations from the spec's Plan order:** there are two, and each is argued in its task.
- The ADRs are written in the tasks that make them real (0051 in Task 2, amended in 4; 0054 in Task 9). Task 10 only adds the chip figures. This follows the owner rule.
- `in_place_enum!` takes one optional generic parameter from Task 1, for Task 5's `ModelSlot<S>`. Task 7 drops the generic store, and the macro's parameter stays (Task 1's, tested by its toy slot; spec § 4.8).

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

---

### Task 5: 16-bit block-float strings — REPLACED (owner, 2026-09-29)

Done as a32923e, 6a3269d, c3194bc and cf3a23f, then replaced by the owner's sympathetic-pool decision (spec § 4). It made `KsString` and `ModalEngine` generic over a `Store`, shipped `Q16` (a per-line block exponent), re-recorded the Modal goldens, and wrote ADR 0052. Sympathetic's high notes missed the −90 dBFS gate (−83.7 dBFS at C6), and the owner chose f32 in a shared pool over a looser bound.

Task 7 undoes it, and keeps three pieces that are bit-identical in f32: the fused injection (`pending`, `tick_coupled`), one-pass `damp`, and the note-on clear (spec § 4.8). ADR 0052 stays on file as `Superseded by 0054`.

---

### Task 6: Chip figures — REPLACED by Task 10

Its bench rows timed the Q16 exponent step and its figures went to ADR 0052. Task 10 is the same task for the pool.

---

### Task 7: Strings back to f32; the 16-bit store goes

**Files:**
- Delete: `chimera-core/src/dsp/modal/q16.rs`
- Modify: `chimera-core/src/dsp/modal/string.rs` (the whole `KsString`), `chimera-core/src/dsp/modal/mod.rs` (drop `S` everywhere, the `F32` alias and the Q16 tests)
- Restore: `chimera-core/tests/golden_test.rs`, `chimera-core/tests/instrument_test.rs`, `chimera-core/tests/codec_compat_test.rs` to 0a7d468
- Modify: `chimera-core/tests/common/mod.rs` (`Case::ModalSympathetic`), then `chimera-core/tests/golden_test.rs` (its one new row)

**Interfaces:**
- Consumes: `ModelSlot`, `ModalBank`, `BowedString`, `SympatheticStrings` as Task 5 left them.
- Produces, all non-generic again:

```rust
pub(super) struct KsString {
    pub(super) buffer: [f32; MAX_STRING_DELAY],
    pub(super) write_pos: usize,
    pub(super) delay_len: usize,
    ens_lfo_phase: u32,
    noise_state: u32,
}
impl KsString {
    pub(super) fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self;   // buffer zeroed in place
    pub(super) fn tick_full(&mut self, p: &KsRenderParams) -> f32;
    pub(super) fn tick_coupled(&mut self, p: &KsRenderParams, input: f32, pending: &mut f32) -> f32; // kept from c3194bc
    pub(super) fn damp(&mut self, passes: u32);    // one load, `passes` multiplies, one store: kept
    pub(super) fn clear(&mut self);                // the whole ring: kept
}
struct BowedString { string: KsString, force: f32 }
struct SympatheticStrings { main: KsString, strings: [KsString; NUM_SYMPATHETIC], ratios: [f32; NUM_SYMPATHETIC], pending: [f32; NUM_SYMPATHETIC] }
enum ModelSlot { Bank(ModalBank), String(KsString), Bowed(BowedString), Sympathetic(SympatheticStrings) }   // via in_place_enum!, no generic
pub struct ModalEngine { /* unchanged fields */ }
```

- `pub use q16::{Exp, Q16, StepBudget, Store}` goes, and so does every `StepBudget` argument.
- `models_are_exclusive` loses its type parameter. Its assert becomes `const _: () = assert!(models_are_exclusive());`.
- `in_place_enum!` keeps its optional generic parameter (Task 1). Its toy test still exercises it.

**Why the generic store goes rather than stays:** one store is left, and the only test that needed two (`strings_i16_match_f32`) goes with Q16. A type parameter on five types for one instantiation is dead flexibility (spec § 4.8).

- [ ] **Step 1: Write the failing test (restore the goldens).** Run `git checkout 0a7d468 -- chimera-core/tests/golden_test.rs chimera-core/tests/instrument_test.rs chimera-core/tests/codec_compat_test.rs`. Then run `git diff 0a7d468 -- chimera-core/tests/golden_test.rs chimera-core/tests/instrument_test.rs chimera-core/tests/codec_compat_test.rs`. Expected: empty. Task 5 changed only golden rows in those files.
- [ ] **Step 2: Run it to verify it fails.** Run `cargo test -p chimera-core --test golden_test --test instrument_test --test codec_compat_test`. Expected: exactly these fail, on the Q16 hashes:
  - `goldens_match` and `goldens_match_through_the_instrument`, on `modal_init`, `modal_lfo_cutoff` and `algo_to_modal_switch`;
  - `instrument_goldens_match` and `the_mix_before_the_limiter_is_mains`, on `two_parts_two_pairs`;
  - `v1_fixtures_render_identically`, on `init_modal.snd`.
- [ ] **Step 3: Implement.**
  - Delete `q16.rs`, and remove its `mod` and `pub use`.
  - Make `KsString` and every type above non-generic, with `buffer: [f32; MAX_STRING_DELAY]` read and written directly.
  - Delete `strings_i16_match_f32`, `strings_i16_with_feedback_stay_bounded`, `pitch_down_after_a_long_tail_reads_no_burst` and `a_short_loop_steps_at_most_once_a_block`. `q16_saturates_and_steps` goes with its file.
  - Keep `note_on_clears_every_line_at_a_fixed_cost`, with its type parameter dropped.
  - Replace every "ADR 0052" comment with the spec's § 4.8 wording, or drop it.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test golden_test --test instrument_test --test codec_compat_test --test modal_test --test modal_integration_test --test in_place_test`. Expected: PASS. The six renders read their pre-Task-5 values:
  - `modal_init` 0x90f1197c153d0b05
  - `modal_lfo_cutoff` 0xe9e4fe3dda9b0262
  - `algo_to_modal_switch` 0x501ad70c947a9c4d
  - `two_parts_two_pairs` 0x851eab45ed8a2f86, and 0x016712a2b7e18d83 before the limiter
  - `init_modal.snd` 0x90f1197c153d0b05

  These were measured on cf3a23f with both defaults set to f32 during the spec revision. If any differs, STOP and report which, with the measured hash. Never re-record here.
- [ ] **Step 5: Record the Sympathetic pin.**
  - Add `Case::ModalSympathetic` to `tests/common/mod.rs`: `ModalInit`'s Sound with `modal.mode = ResonatorMode::Sympathetic`, the same note and harness. `Case::ALL` grows to 24.
  - Run `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test -- goldens_match --nocapture`. Paste only the new `modal_sympathetic` row, with the comment `// Recorded: pins Sympathetic before its set moves to the pool (exclusive-state spec § 4.8).`
  - `git diff chimera-core/tests/golden_test.rs` must show that row alone.
  - Run `cargo test -p chimera-core --test golden_test`. Expected: PASS, `goldens_match_through_the_instrument` included.
- [ ] **Step 6: Check the sizes.** Run `cargo test -p chimera-core --test memory_budget_test -- --nocapture`. Expected (host): `ModalEngine` 31,768, `Voice` 33,608 and `[Voice; 8]` 268,864, under 286,720. Then run `grep -rn "Q16\|StepBudget\|ADR 0052" chimera-core/src`. Expected: no hits.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git rm chimera-core/src/dsp/modal/q16.rs
git add chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/golden_test.rs chimera-core/tests/instrument_test.rs chimera-core/tests/codec_compat_test.rs chimera-core/tests/common/mod.rs
git commit -m "Strings back to f32; the 16-bit store goes"
```

---

### Task 8: `SymAlloc`, the pure pool allocator

**Files:**
- Create: `chimera-core/src/sym_alloc.rs`
- Modify: `chimera-core/src/lib.rs` (`pub mod sym_alloc;`), `chimera-core/src/voice_alloc.rs` (`VoiceIdx`)
- Test: `sym_alloc.rs` (`#[cfg(test)] mod tests` and doc tests), `voice_alloc.rs`

**Interfaces:**
- Produces, in `chimera_core::voice_alloc`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceIdx(u8);                                  // private: always < MAX_VOICES
impl VoiceIdx { pub const ALL: [VoiceIdx; MAX_VOICES]; pub fn index(self) -> usize; }
```

- Produces, in `chimera_core::sym_alloc` (pure: no DSP, no `unsafe`):

```rust
pub const SYM_SLOTS: usize = 4;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymSlot(u8);                                   // private: always < SYM_SLOTS
impl SymSlot { pub fn index(self) -> usize; }
#[must_use = "a dropped Lease leaks its slot; give it back"]
#[derive(Debug)]
pub struct Lease(SymSlot);                                // no Clone, no Copy, no public ctor
impl Lease { pub fn slot(&self) -> SymSlot; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place { On(VoiceIdx), Steal(VoiceIdx), Refused }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart { Claimed { evict: Option<VoiceIdx> }, Silent }
pub struct SymAlloc { slots: [State; SYM_SLOTS] }
impl SymAlloc {
    pub const fn new() -> Self;
    pub fn place(&mut self, pick: Option<VoiceIdx>, age: u32) -> Place;   // age: the note's Allocator age
    pub fn restart(&mut self, voice: VoiceIdx, age: u32) -> Restart;    // called newest first
    pub fn lend(&mut self, voice: VoiceIdx) -> Option<Lease>;
    pub fn give_back(&mut self, lease: Lease);
    pub fn awaits(&self, voice: VoiceIdx) -> bool;
    pub fn forfeit(&mut self, voice: VoiceIdx);
    pub fn holder(&self, slot: SymSlot) -> Option<VoiceIdx>;
    pub fn free(&self) -> usize;
    pub fn lent(&self) -> usize;
}
```

**The algorithm (spec § 4.5, the owner's Rings rule; the signature does not decide it):**

```rust
#[derive(Clone, Copy)] enum Then { Free, To(VoiceIdx) }
#[derive(Clone, Copy)] enum State { Free, Promised { voice: VoiceIdx, age: u32 }, Lent { voice: VoiceIdx, age: u32, then: Then } }
// Ages are the Allocator's note ages: lower is older. No clock of its own.
fn place(&mut self, pick, age) -> Place {
    if let Some(p) = pick && let Some(s) = self.slot_of(p) {           // Promised or Lent to p
        self.retarget(s, age, p); return Place::On(p)                  // retarget: age = age; Lent → then = To(p)
    }
    match (self.first_free(), pick) {
        (Some(s), Some(p)) => { self.slots[s] = State::Promised { voice: p, age }; Place::On(p) }
        (Some(_), None) => Place::Refused,                             // the CPU budget's, not the pool's
        (None, _) => {                                                 // full: the oldest yields, Mono included
            let s = self.oldest().expect("four slots, none free");
            let u = self.voice_at(s); self.retarget(s, age, u); Place::Steal(u)
        }
    }
}
fn restart(&mut self, voice, age) -> Restart {
    if self.slot_of(voice).is_some() { return Restart::Claimed { evict: None } }
    if let Some(s) = self.first_free() { self.slots[s] = State::Promised { voice, age }; return Restart::Claimed { evict: None } }
    match self.oldest().filter(|&s| self.age_at(s) < age) {
        Some(s) => {
            let evicted = self.voice_at(s);
            self.slots[s] = match self.slots[s] {
                State::Promised { .. } => State::Promised { voice, age },
                State::Lent { voice: u, .. } => State::Lent { voice: u, age, then: Then::To(voice) },
                State::Free => unreachable!(),
            };
            Restart::Claimed { evict: Some(evicted) }
        }
        None => Restart::Silent,
    }
}
fn lend(&mut self, voice) -> Option<Lease> {
    if self.awaits(voice) { return None }                             // its slot is still fading out elsewhere
    let s = self.promised_to(voice).or_else(|| self.first_free())?;
    let age = self.age_at(s);                                           // 0 for a free slot taken unpromised
    self.slots[s] = State::Lent { voice, age, then: Then::Free };
    Some(Lease(SymSlot(s as u8)))
}
fn give_back(&mut self, lease: Lease) {
    let s = lease.0.index();
    self.slots[s] = match self.slots[s] {
        State::Lent { age, then: Then::To(w), .. } => State::Promised { voice: w, age },
        State::Lent { .. } => State::Free,
        other => { debug_assert!(false, "a lease for a slot not lent"); other }
    };
}
fn awaits(&self, voice) -> bool { self.slots.iter().any(|s| matches!(s, State::Lent { voice: u, then: Then::To(w), .. } if *w == voice && *u != voice)) }
fn forfeit(&mut self, voice) { if let Some(s) = self.promised_to(voice) { self.slots[s] = State::Free } }
```

`slot_of(v)` is the slot `Promised` or `Lent` to `v`, or `Lent` elsewhere with `then = To(v)`. `oldest()` runs over `Promised` and `Lent` slots by age, and the lower index wins a tie.

- [ ] **Step 1: Write the failing tests** in `sym_alloc.rs`. `v(i)` is `VoiceIdx::ALL[i]`, and the ages are an increasing counter `a` (1, 2, 3, …), as the `Allocator`'s clock gives them.
  - `a_free_slot_is_promised_to_the_pick`: `place(Some(v(0)), 1) == On(v(0))`, then `free() == 3` and `lent() == 0`. Then `lend(v(0))` is `Some`, with `slot().index() == 0` and `lent() == 1`.
  - `a_retrigger_keeps_its_slot`: `v(0)` holds a lease. `place(Some(v(0)), a) == On(v(0))`, `free() == 3`, and `holder(slot 0) == Some(v(0))`.
  - `the_fifth_steals_the_oldest`: `v(0)`–`v(3)` each `place` (ages 1–4) and `lend`. Then:
    - `place(Some(v(4)), 5)` is `Steal(v(0))`, then `Steal(v(1))` for `v(5)` at 6, `Steal(v(2))` for `v(6)` at 7 and `Steal(v(3))` for `v(7)` at 8.
    - A ninth, `place(Some(v(4)), 9)`, is `Steal(v(0))`: age 5 is the oldest again.
    - `free() == 0` and `lent() == 4` throughout.
  - `a_full_pool_never_refuses`: `v(0)`–`v(3)` hold leases. `place(None, 5) == Steal(v(0))`. With a free slot, `place(None, a) == Refused` (the budget's refusal).
  - `a_mono_holder_is_stolen_when_newest_arrives`: `v(0)` holds the oldest slot (age 1), and `v(1)`–`v(3)` hold the rest (ages 2–4). The pool has no notion of Mono, so the newest arrival, `place(Some(v(4)), 5)`, is `Steal(v(0))`. The Instrument half is in Task 9.
  - `a_stolen_slot_comes_back_promised`: `v(0)`–`v(3)` hold leases, and `place(Some(v(4)), 5) == Steal(v(0))`. After `give_back(l0)`: `holder(slot 0) == Some(v(0))`, `free() == 0`, `lend(v(5)).is_none()`, and `lend(v(0))` is slot 0.
  - `last_four_played_restart_on_switch`, in two cases.
    - Empty pool. `restart` for `v(7)` down to `v(0)` (ages 8 down to 1, newest first) gives `Claimed { evict: None }` for `v(7)`–`v(4)` and `Silent` for `v(3)`–`v(0)`.
    - `v(6)` and `v(7)` hold leases at ages 1 and 2 (another Part's older notes). Then `restart` for `v(5)`, `v(4)`, `v(3)`, `v(2)` at ages 10, 9, 8, 7:
      - `Claimed { None }`, `Claimed { None }`, `Claimed { evict: Some(v(6)) }`, `Claimed { evict: Some(v(7)) }`.
      - `awaits(v(3))` and `awaits(v(2))` are true.
      - After `give_back` of `v(6)`'s and `v(7)`'s leases, both are false, and `lend(v(3))` and `lend(v(2))` succeed.
    - A restart never evicts a newer note: after four claims at ages 10–7, `restart(v(1), 3)` is `Silent`.
  - `lend_takes_the_promise_first`: `place(Some(v(2)), 1)` promises slot 0 and `place(Some(v(3)), 2)` slot 1. `lend(v(3))` is slot 1, and `lend(v(2))` is slot 0.
  - `forfeit_frees_only_a_promise`: `v(0)` is promised slot 0; `v(1)` holds slot 1. `forfeit(v(1))` changes nothing (`lent() == 1`, `free() == 2`). Then `forfeit(v(0))` gives `free() == 3`.
  - `sym_alloc_never_gives_a_voice_two_slots`: a local xorshift32 seeded `0x9E37_79B9` drives 100,000 steps over `v(0)`–`v(7)`, with ages from an increasing counter.
    - Each step, uniformly: `place` for a random pick (`None` one time in eight); `restart` for a random voice; `lend` for a random voice; `give_back` of a random held lease; or `forfeit` of a random voice.
    - The test keeps held leases in a `Vec<Lease>`.
    - After each step: each voice is named by at most one slot, as holder or as a `To` target; `lent()` equals the `Vec`'s length; and `free()` plus the promised count plus `lent()` equals 4.
  - Doc tests on `Lease`:
    - `compile_fail,E0599`: `let b = lease.clone();`.
    - `compile_fail,E0382`: `alloc.give_back(lease); let _ = lease.slot();`.
    - `compile_fail,E0423`: `let _ = chimera_core::sym_alloc::Lease(slot);`, a private tuple-struct constructor, as in `storage/frame.rs`.
  - In `voice_alloc.rs`: `voice_idx_all_counts_up`. `VoiceIdx::ALL[i].index() == i` for every `i`.
- [ ] **Step 2: Run the tests to verify they fail.** Run `cargo test -p chimera-core --lib sym_alloc voice_alloc && cargo test -p chimera-core --doc sym_alloc`. Expected: compile errors (`sym_alloc`, `VoiceIdx` not found).
- [ ] **Step 3: Implement the Interfaces and algorithm above.**
- [ ] **Step 4: Run the tests to verify they pass.** Same commands. Expected: PASS.
- [ ] **Step 5: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/sym_alloc.rs chimera-core/src/lib.rs chimera-core/src/voice_alloc.rs
git commit -m "SymAlloc: four sympathetic slots, each lent by one Lease"
```

---

### Task 9: Sympathetic borrows its strings from the pool

**Files:**
- Modify: `chimera-core/src/dsp/modal/mod.rs` (`SympatheticSet`, `SymPool`, `SympatheticVoice`, `Model`; pool arguments), `chimera-core/src/dsp/engines.rs` (`rebuild`'s leases, `Rebuilt`, `SlotKind::resting`), `chimera-core/src/in_place.rs` (`move_out`), `chimera-core/src/dsp/voice.rs` (`id`, pool arguments, `reset`'s choice, `rest`), `chimera-core/src/voice_alloc.rs` (`pick` public, `book`), `chimera-core/src/instrument.rs` (`sym`, placement, step 5)
- Create: `chimera-core/tests/sym_pool_test.rs`, `chimera-core/tests/common/rig.rs`
- Modify: every test that drives a `Voice` or `ModalEngine` directly, moved to `common::Rig` or `SymPool::boxed()`: `chain_spectral`, `click_free`, `desktop_sim`, `engines`, `engine_switch`, `exclusive_state`, `factory_level`, `factory`, `flt_page`, `in_place`, `lfo_slot`, `live_param`, `modal_integration`, `modal`, `modulatable`, `modulation_integration`, `pitch`, `property`, `rebuild_stack`, `reverb`, `routing`, `sanity`, `signal_chain` and `vca` (`*_test.rs`), plus `common/mod.rs`; and `memory_budget_test.rs`
- Create: `docs/adr/0054-sympathetic-strings-from-a-shared-pool.md`. Modify: `docs/adr/README.md`

**Interfaces:**
- Consumes: `SymAlloc`, `Lease`, `Place`, `Restart`, `SYM_SLOTS`, `VoiceIdx` (Task 8); `in_place_enum!` (Task 1); `Voice::rebuild` (Task 4).
- Produces, in `chimera_core::dsp::modal`:

```rust
pub struct SympatheticSet { strings: [KsString; NUM_SYMPATHETIC], ratios: [f32; NUM_SYMPATHETIC], pending: [f32; NUM_SYMPATHETIC] }
pub struct SymPool { alloc: SymAlloc, sets: [SympatheticSet; SYM_SLOTS] }
impl SymPool {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self;   // sets through uninit_at, alloc by value
    pub fn alloc(&self) -> &SymAlloc;
    pub(crate) fn alloc_mut(&mut self) -> &mut SymAlloc;
    fn set(&mut self, lease: &Lease) -> &mut SympatheticSet;
    #[cfg(any(test, feature = "test-support"))] pub fn boxed() -> Box<Self>;  // new_uninit + init_in_place
}
struct SympatheticVoice { main: KsString, lease: Lease }
pub enum Model { Bank, String, Bowed, Sympathetic(Lease) }            // not Copy
enum ModelSlot { Bank(ModalBank), String(KsString), Bowed(BowedString), Sympathetic(SympatheticVoice) }
impl ModalEngine {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, model: Model) -> &mut Self;
    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32, pool: &mut SymPool);
    pub fn note_off(&mut self, pool: &mut SymPool);
    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], params: &ModalParams, sample_rate: u32, pool: &mut SymPool);
    pub(crate) fn lease_mut(&mut self) -> Option<&mut Lease>;         // for EngineSlot::rebuild's move_out only
    #[cfg(any(test, feature = "test-support"))] pub fn new_in(pool: &mut SymPool, mode: ResonatorMode) -> Self; // lends for v0
}
const _: () = assert!(size_of::<SympatheticVoice>() <= max(size_of::<BowedString>(), size_of::<KsString>()));
```

- In `chimera_core::dsp::engines`:

```rust
impl SlotKind { pub fn resting(self) -> Self; }                        // Modal(Sympathetic) → Modal(String); else self
#[must_use] #[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rebuilt { Built, NoSlot }                                     // NoSlot: built `kind.resting()` instead
impl EngineSlot {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, kind: SlotKind) -> &mut Self;   // builds kind.resting(): no lease
    pub fn rebuild(&mut self, kind: SlotKind, pool: &mut SymAlloc, voice: VoiceIdx) -> Rebuilt; // spec § 4.4's table
    // note_on / note_off / render gain `pool: &mut SymPool`
}
```

- In `chimera_core::in_place`: `pub(crate) unsafe fn move_out<T>(r: &mut T) -> T` (spec § 4.4, with its `# Safety`).
- In `Voice`:
  - `id: VoiceIdx`, written once in `init_in_place(slot, sample_rate, id)` and skipped by `write_chain!`. `Voice::new(sample_rate)` uses `VoiceIdx::ALL[0]`.
  - `note_on(…, pool: &mut SymPool) -> bool`, `note_off(&mut self, pool: &mut SymPool)` and `render(…, pool: &mut SymPool)`.
  - `pub fn rest(&mut self, pool: &mut SymPool)`: idle and no note waits. It rebuilds a Sympathetic slot into `resting()`, then calls `forfeit(id)`.
  - `reset(&mut self, params, then_plays: bool)`: `then_plays` rebuilds into `SlotKind::of(params)`, otherwise into `SlotKind::of(params).resting()`. `fade_ended` passes `after != AfterFade::Idle`, and the VCA-lifetime reset passes `false`.
  - `trigger`: a `NoSlot` rebuild leaves the voice idle and returns before the note starts. A kind other than `Modal(Sympathetic)` calls `pool.alloc_mut().forfeit(self.id)`.
  - `pub fn velocity(&self) -> Velocity`: the sounding note's velocity (`last_velocity`), for the `Instrument`'s restarts.
- In `Allocator`: `pub fn pick(&self, part, mode, cost, reserved) -> Option<usize>` (unchanged body), `pub fn next_age(&self) -> u32` (the age `book` will give), and `pub fn book(&mut self, v: usize, part, mode, note, cost) -> Alloc`. `note_on` becomes `pick` then `book`, with the same behaviour. `book` overwrites a Mono slot like any other: the pool's steal gives Mono no protection (spec § 4.5).
- In `Instrument`: a `sym: SymPool` field, built in place in `init_in_place` and listed in `field_list!`, plus `pub fn sym(&self) -> &SymAlloc` and `pub fn slot_kinds(&self) -> [SlotKind; MAX_VOICES]` for tests.
  - A `last_kind: [SlotKind; MAX_PARTS]` field, written in `init_in_place` from the default Sound and listed in `field_list!`.
  - `handle`, for a Part whose `SlotKind::of` is `Modal(Sympathetic)`:
    - `place(pick, alloc.next_age())`, then `book` the voice it names.
    - `Steal(u)` sets `waiting[u] = Some(vel)` and calls `voices[u].kill()`, even for the same Part and kind, Mono included.
    - `Refused` comes only from the CPU budget and counts as today.
    - Every other kind skips `place`.
  - `render`, before the voice loop: for each Part whose `SlotKind::of` became `Modal(Sympathetic)` since `last_kind`, collect its booked, held, active voices (`sounding[v] == p`) into a stack array of at most `MAX_VOICES` `(VoiceIdx, age)`, and sort it newest first. Call `restart` for each:
    - `Claimed { evict }`: `waiting[v] = Some(voices[v].velocity())`, then `voices[v].kill()`. For `evict: Some(u)`: `voices[u].kill()`, and if `waiting[u].take()` held a note, `dropped_unheard()`.
    - `Silent`: `voices[v].kill()`.
    - Then update `last_kind`.
  - Render step 5: a waiting Sympathetic note on a voice for which `sym.alloc().awaits(v)` goes back into `waiting[v]` without `note_on`. The `release_finished` branch calls `voices[v].rest(&mut self.sym)`.

**Keeping sound identical.** `render_sympathetic` runs the same arithmetic on the same state. Only the main string's owner and the set's owner change. `modal_sympathetic` (Task 7) and every other golden must stay bit-identical. A fresh pool's sets are zeroed as a fresh engine's were, and a note-on clears every line it starts (spec § 4.8).

- [ ] **Step 1: Write the failing tests.**
  - **`engines.rs`: `resting_maps_only_sympathetic`.** For all four modes and Algo, `resting()` is identity, except `Modal(Sympathetic)`, which gives `Modal(String)`.
  - **`engines.rs`: `rebuild_moves_leases_as_the_table_says`.** One boxed slot, one `SymAlloc` and voice `v(0)`:
    - Algo → Sympathetic: `Built`, `lent() == 1`.
    - Sympathetic → Sympathetic: `Built`, `lent() == 1`, same `slot()`.
    - Sympathetic → String: `lent() == 0`, `free() == 4`.
    - With all four slots lent to other voices, String → Sympathetic: `NoSlot`, `kind() == Modal(String)`.
  - **`sym_pool_test.rs`.** Use `Instrument` with the `common` helpers. `sym` is `for_engine(Modal)` with `modal.mode = Sympathetic`.
    - `a_fifth_sympathetic_note_steals_the_oldest` (Review Focus 1). Parts 1 and 2 = `sym`, Poly, on channels 0 and 1.
      - Part 1 holds notes 48, 50, 52 and 53, on in blocks 0–3.
      - At block 10, Part 2 plays 55. Just before it, record the voice `u` sounding 48 (`allocator().slots()`) and `r = rebuilds()[u]`.
      - Over the next `FADE / BLOCK_SIZE` blocks, Part 1's bus's largest step is ≤ S + A / `FADE`, with S and A from block 9, as in `switch_never_clicks`.
      - `rebuilds()[u] == r + 2` after the fade-end block, and 55 sounds on `u`.
      - From Part 2's first non-zero block, its bus is bit-identical, for 8 blocks, to 55 alone on a fresh `Instrument`.
      - Every block: the count of `Modal(Sympathetic)` in `slot_kinds()` is at most 4 and equals `sym().lent()`.
    - `other_models_keep_eight_voices`. For each of String, Bowed and the bank: 8 held notes all sound (`part_bus` non-zero with every voice active, via `allocator().slots()`), and `sym().free() == 4`.
    - `four_sympathetic_notes_sound_as_alone`. Parts 1–4 = `sym`, one note each, on four channels. Each `part_bus(p)` is bit-identical to that note alone in a fresh `Instrument`.
    - `a_handed_over_slot_carries_nothing`. Note A (60) plays 20 blocks, goes off, and renders until idle. Then a note-off, and note B (67) on a second Part, so that B lands on another voice and takes slot 0: assert `holder(slot 0)` names B's voice. B's 16 blocks are bit-identical to B in a fresh `Instrument`.
    - `every_lease_comes_home` (Review Focus 5). Parts 1–3; a local xorshift32 seeded `0xC0FF_EE11`, over 2,000 blocks. Each block, with probability ¼ each:
      - a note-on (random Part, note 36–84);
      - a note-off of a random held note;
      - a MODE change on a random Part (random of four);
      - an ENGINE flip on a random Part.

      At every block, the count of `Modal(Sympathetic)` in `slot_kinds()` equals `sym().lent()` and is ≤ 4. Then all notes go off, and it renders until every voice is idle (≤ 2,000 blocks). Then `sym().free() == 4`.
    - `the_last_four_played_restart_through_the_instrument` (Review Focus 6). Part 1 = String, Poly: notes 60–67 go on in that order, one a block, and stay held. At block 10, MODE becomes Sympathetic.
      - After `FADE / BLOCK_SIZE + 1` blocks, the voices of 64–67 are `Modal(Sympathetic)` and active. Those of 60–63 are idle and held (`allocator().slots()[v].held()`), and their `part_bus` contribution is zero.
      - Each restarted voice's `rebuilds` rose by exactly 2 (the rest, then the note).
      - Key-up and re-press 60: it sounds, and the voice of 64, now the oldest, fades over `FADE`.
      - With Part 2 (Sympathetic) holding two older notes before the switch, the two evicted voices are Part 2's. The restarts that claimed their slots start one block after those fades end at the latest, never on a slot still `Lent`.
      - Then all keys go up and it renders until idle: `sym().free() == 4`.
    - `a_resting_voice_gives_its_slot_back`. A `sym` note 60 with `decay` 0.3 renders until the voice goes idle. In that block, `rebuilds` rises by 1, `slot_kinds()[v] == Modal(String)` and `sym().free() == 4`.
    - `a_mono_holder_is_stolen_when_newest_arrives`. Part 1 = `sym`, Mono, holds 48 (the oldest). Parts 2–4 = `sym`, Poly, each hold one note. Part 2 plays another note. Part 1's voice fades over `FADE` and the new note sounds on it. Part 1's next note then takes a voice as a fresh Mono note does, and sounds by stealing the now-oldest slot.
  - **`memory_budget_test.rs`: `sympathetic_pool_fits_d2`.** It prints `SympatheticVoice`, `SympatheticSet`, `SymPool`, `Voice`, `[Voice; 8]`, `Instrument` and `VOICE_RAM_BUDGET − Instrument`. It asserts `size_of::<ModelSlot>()` is at most `BowedString` rounded up to align, plus align (Sympathetic doesn't size the voice), and that `Instrument <= VOICE_RAM_BUDGET`.
  - **`rebuild_stack_test.rs`.** The thread's stack becomes `size_of::<SympatheticSet>()`. It uses a `Rig`, and its switch run includes Sympathetic.
- [ ] **Step 2: Run the tests to verify they fail.** Run `cargo test -p chimera-core --lib engines && cargo test -p chimera-core --test sym_pool_test`. Expected: compile errors (`SymPool`, `resting`, `Rebuilt`, `sym` not found).
- [ ] **Step 3: Implement the Interfaces.**
  - Write `common/rig.rs` first. `pub struct Rig { pub voice: Box<Voice>, pub pool: Box<SymPool> }` offers `new(sr)`, and `note_on`, `note_off` and `render` with the pre-Task-9 signatures, forwarding the pool. It derefs to `Voice` for `kill`, `is_active`, `rebuilds` and `cost`.
  - Move each listed test file from `Voice::new(SR)` to `Rig::new(SR)`, mechanically. Tests on `ModalEngine::new(mode)` use `ModalEngine::new_in(&mut pool, mode)`.
  - `EngineSlot::rebuild` is the one caller of `move_out`, with one `// SAFETY:` line: "the payload is rebuilt below in every arm, and `rebuild_*` aborts rather than unwinds".
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core`, which covers the lib, every test binary and the doc tests. Expected: PASS, with every golden bit-identical, `modal_sympathetic` included. Any golden mismatch means the move changed state: fix it, and never re-record. Also check:
  - `memory_budget_test -- --nocapture` prints (host): `Voice` ≈ 5,840 (at most 5,848 with `id`), `SymPool` ≈ 111,168 and `Instrument` ≈ 160,544, which leaves ≈ 126,176.
  - If any is more than 5 % above, STOP and report.
- [ ] **Step 5: Prove the stack test can fail.** Temporarily make Sympathetic's note-on overwrite its set by value: `*pool.set(&lease) = SympatheticSet::new()`, with a test-support `new` through `by_value`. Run `cargo test -p chimera-core --test rebuild_stack_test`. Expected: the process aborts with "has overflowed its stack". Revert, and confirm `git diff chimera-core/src/dsp/modal/mod.rs` shows only this task's changes. If it doesn't overflow, STOP and report.
- [ ] **Step 6: Write ADR 0054**, `sympathetic-strings-from-a-shared-pool` (Proposed; Deciders: project owner).
  - **Context:** Sympathetic was eight strings to every other model's one. Q16 (ADR 0052) missed −90 dBFS at C6. Rings caps polyphony at 4 and shares 8 strings.
  - **Decision:** spec § 4.2–4.6, stated so it can be checked against the code:
    - the slot is 7 lines, 7 ratios and 7 pending values;
    - the main string stays in the voice;
    - the pool is in D2, inside the `Instrument`;
    - the `Lease`;
    - `place`, `lend`, `give_back` and `forfeit`, and the steal onto the slot's own voice;
    - `resting`;
    - the Rings rule: the oldest yields, Mono included, and a note is never refused by the pool;
    - on a switch, the last four held notes played restart, through `restart` and the `Instrument`'s waiting note.
  - **Alternatives:**
    - Q16 strings (0052): precision.
    - f32 in every voice: 271,520 B, which leaves 15,200 B.
    - The main string in the pool too: +15,840 B for no voice saving.
    - Slot `s` bound to voice `s`: a Sympathetic note would steal an Algo note on voices 0–3 while 4–7 sit free.
    - A registry without a token: a Sympathetic model without a slot becomes representable.
    - Protecting Mono, or refusing a fifth note: the owner chose Rings' rule, that the oldest always yields.
    - The pool in AXI: 111,104 B against about 24.9 KB spare.
  - **Consequences:**
    - § Memory's numbers.
    - At most 4 Sympathetic notes, with a steal that fades.
    - One `rest` rebuild at a natural end.
    - The ADR 0051 bound is unchanged.
    - One `move_out`.
    - Its cap is what #207 shows.
    - A Mono voice can be stolen by the pool, an exception to the `Allocator`'s rule 1.
  - **Sources:** the spec; this plan, Tasks 7–9; `rings/dsp/part.h` and `part.cc` (MIT, Emilie Gillet; the design idea only); ADRs 0051 and 0052.

  In the README, add row 0054, and link 0052's status cell: `Superseded by [0054](0054-sympathetic-strings-from-a-shared-pool.md)`. 0052's own Status line gets the same link. The file is Proposed, never accepted, so it is editable.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0 (stack-check included).
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src chimera-core/tests docs/adr/0052-strings-stored-as-16-bit-block-float.md docs/adr/0054-sympathetic-strings-from-a-shared-pool.md docs/adr/README.md
git status --short   # must list no docs/chimera-ui-ux-spec.md and no chimera.bin
git commit -m "Sympathetic borrows its strings from a pool of four"
```

---

### Task 10: Chip figures

**Files:**
- Modify: `chimera-stm32/src/bench.rs`
- Modify: `docs/adr/0051-…md` and `docs/adr/0054-…md` (figures only, still Proposed)

**Interfaces:**
- Consumes: `EngineSlot::{init_in_place, rebuild}`, `SlotKind` (Task 2), `SymAlloc`, `VoiceIdx` (Task 8), `SymPool`, `Instrument::sym` (Task 9), `Voice`.
- Produces three bench additions:
  - `#[inline(never)] fn time_rebuild() -> u32` gives the cycles per `rebuild(SlotKind::Modal(Sympathetic), …)`. It averages 16 rebuilds that alternate with `rebuild(SlotKind::Algo, …)`, timing only the Sympathetic ones.
    - It runs on a `static mut MaybeUninit<EngineSlot>` and a `static mut SymAlloc`. A Sympathetic rebuild writes only the main string and the lease, so no set is needed.
    - The slot goes in `.ram_d2.voices` with the `Instrument`, as in `audio/engine.rs`: 4,008 B more, inside the ≈ 126 KB headroom.
  - `#[inline(never)] fn time_sym_note_on(inst: &mut Instrument, shared: &AudioShared) -> u32` gives the cycles of `Instrument::handle` for one Sympathetic note-on on an idle voice. That covers `place`, `lend`, the rebuild, and the note-on's clear and excitation. It averages 16 note-on / kill / render-to-idle rounds on the bench's own `Instrument`.
  - A `ROUTING` row `("SWITCH", |p| modal(p, ResonatorMode::String), switch_storm)`. `switch_storm` flips `p.params.modal.mode` between String and Sympathetic every 4 blocks, so it steals and rests too. `ROUTING_ROWS` becomes 29.
  - `fn show_memory(display, rebuild: u32, note_on: u32)` draws one held screen, titled `MEMORY`, shown after the ROUTING pages. Its lines:
    - `VOICE {size_of::<Voice>()}`
    - `SLOT {size_of::<EngineSlot>()}`
    - `MODAL {size_of::<ModalEngine>()}`
    - `SYM POOL {size_of::<SymPool>()}`
    - `INSTR {size_of::<Instrument>()}/{VOICE_RAM_BUDGET}`
    - `REBUILD {rebuild} CYC`
    - `SYM NOTE-ON {note_on} CYC`

- *Added during execution (2026-09-29, after the chip's first bench read SYM NOTE-ON at 40,906 cycles):*
  - **The dirty extent** (61b2cb8). Each `KsString` keeps `dirty`: every sample at or past it reads 0.0, and `set_freq` raises it to each new loop length. `clear` zeros `[0, dirty)` only, bit-identical to the whole ring. A note-on clears each line before retuning it, so `SymPool::note_on_clear` gives the bytes exactly and purely.
  - **The per-block clear budget** (7b40a27). The `Instrument` spends `SYM_CLEAR_BUDGET` bytes a block, one worst-case note-on (`SYM_NOTE_ON_CLEAR_MAX`, eight rings, 31,488 B). A Sympathetic note-on past it, or behind a note already waiting on it, waits on the steal's path, and render step 0 starts it before its idle voice renders. A chord of four slots last played low starts over four blocks, and a typical one at once.
  - **No stealing** (the owner's rule, final review). A note with no free slot plays bare (`Halo::Bare`); nothing is stolen, evicted or silenced for the pool; the budget wait keeps its slot `Promised`. See the note under Review Focus.
  - **Bench.** The per-voice figure counts only the voices that sound (`Instrument::sounding`), so MDL SYM and SWITCH read over four, and MEMORY adds `SYM NOTE-ON LOW`, the worst case (MIDI 0 after MIDI 0). `COST_SYMPATHETIC` is 802, from MDL SYM's slope of 859 less the chain's 57.
- [ ] **Step 1: Implement the bench additions.** Put a `// SAFETY:` comment on each new `static mut` access, as for `SCOPE`/`SHARED`. Nothing large may go on the stack: the slot and the allocator are statics, and the `Instrument` is the bench's own.
- [ ] **Step 2: Run the green gate and record the target's section sizes.** Run `just check && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench && "$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-size" -A target/thumbv7em-none-eabihf/release/chimera-stm32`. Expected: exit 0. Keep the `.ram_d2*` section size for the ADRs.
- [ ] **Step 3: Commit.**

```bash
git add chimera-stm32/src/bench.rs
git commit -m "Bench: rebuild, Sympathetic note-on, switch storm, memory"
```

- [ ] **Step 4: STOP. The owner measures on hardware.** Hand the owner this checklist and wait for the figures:
  1. **Baseline.** In a scratch worktree of `main`, run `just flash-bench`. Let the bench pages run through. On the normal UI, load a Modal Sound and hold an 8-note chord. Flip MODE String ↔ Sympathetic 5 times, then ENGINE Algo ↔ Modal 5 times. Open System › AUDIO and read **STACK** (K).
  2. **Branch.** From this worktree, run `just flash-bench` and read the following:
     - First screen: **MODAL /VOICE**. It must be ≤ 447, today's billed `Voice::cost` for the default Modal Sound.
     - ROUTING pages: **MDL SYM /VOICE** (≤ 1,457 billed), **MDL STR /VOICE** and **SWITCH /VOICE** with its 8 counts.
     - MEMORY screen: **VOICE, SLOT, MODAL, SYM POOL, INSTR, REBUILD, SYM NOTE-ON**.
  3. Repeat the chord and the MODE and ENGINE flips from step 1.
     - On Sympathetic, the 8-note chord should sound 4 notes.
     - Play a fifth, then a sixth note over four held Sympathetic notes. Each steals the oldest with a short fade, and there should be no click.
     - Then read AUDIO **STACK**. It must be ≤ the baseline + 1 K (spec § Tests: at most 1 KB of growth; the page shows whole KB).
  4. Run `just flash` to restore the normal firmware.

  If MODAL or MDL SYM exceeds its billed figure, or STACK grows by more than 1 K, STOP and report to the owner. Do not raise a `COST_*` constant or change the design.
- [ ] **Step 5: Record the figures.** Add a "Measured on the chip (rev V, 480 MHz, 2026-MM-DD)" paragraph, with the owner's readings:
  - to ADR 0051: VOICE, SLOT, INSTR, the `.ram_d2*` size, REBUILD, SWITCH, and STACK before and after;
  - to ADR 0054: SYM POOL, SYM NOTE-ON, MDL SYM and MDL STR.

  Run `just check`. Expected: exit 0.
- [ ] **Step 6: Commit.**

```bash
git add docs/adr/0051-a-voice-holds-one-engine-rebuilt-in-place.md docs/adr/0054-sympathetic-strings-from-a-shared-pool.md
git commit -m "Chip figures for ADRs 0051 and 0054"
```
