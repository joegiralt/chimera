# Formant Operators Implementation Plan

**Goal:** Any of Algo's six operators can be a voiced formant (FRMT) or a noise band (NOIS), moved together by a VOWEL macro, billed so that every patch you can build keeps ADR 0026's four voices; #37's operator pages land first.

**Architecture:**
- **Functional core:** new pure modules in `dsp/algo/`:
  - `kinds.rs` holds `OpKind`, `Kinds` and the derived budget;
  - `formant.rs` holds the window, the SKIRT constants, the width floor, the level and peak gains and the edge law;
  - `vowel.rs` holds the vowel table, `fc_hz` and slot resolution;
  - `noise.rs` holds the band, gain and seed laws.

  Tests call each law directly.
- **Imperative shell:** `AlgoEngine` computes one kind block per operator per block from those laws. `Kernel` runs a `KindState` per operator and dispatches once per block: `run_one` (table, unchanged), `run_formant::<N, S>` or `run_noise::<N>`.
- **Storage and UI:** `AlgoParams` owns the kinds, and every write goes through its budget check. The UI's chain is picked per Sound (`PartChain`), and page focus is kept by `BlockDef.id`.

**Tech Stack:** Rust 2024, `no_std` `chimera-core` (f32 DSP, `libm`), `thumbv7em-none-eabihf` firmware, `just`. No new dependency.

**Spec:** `docs/superpowers/specs/2026-09-30-formant-operators-design.md` (owner-approved, e67b2a1). Phase 0 also implements `docs/superpowers/specs/2026-09-27-algo-pages-design.md` § Operator pages. The adversarial review, `formant-adversarial-review.md`, and its probes in `fprobe/` are in the session scratchpad. The § numbers below are the formant spec's.

**Where it runs:**
- **Phase 0:** branch `algo-op-pages`, off `main` once `modal2-resonators`, `projects-core` and `orbit` have merged.
- **Phases 1–9:** branch `formant-ops`, rebased onto `main` after Phase 0 merges. The worktree is `…/scratchpad/wt-formant`. Every command runs from the worktree root.

## Global Constraints

- **The table path doesn't move.** An operator whose kind is `Table` runs today's code. Every existing Algo golden, factory render hash and `codec_compat` hash stays bit-identical, and nothing is re-recorded except the ALGO home screen golden (Task 15).
- **No GPL.** No FSVR code, tables or constants (ADR 0032). The only sources are FSVR's prose docs, the expired patent US5610354 and Peterson & Barney (1952). `THIRD_PARTY.md` doesn't change.
- **Frozen codes (ADR 0045).** The spec fixes these:
  - WAVE 16 `FRMT` and 17 `NOIS`;
  - operator ids 13 `FRMT`, 14 `BW`, 15 `SKIRT`, 16 `KEY`, 17 `RES` and 18 `VSLOT`;
  - VSLOT codes 0–4: `AUTO`, `F1`, `F2`, `F3` and `OFF`;
  - Algo id 4, `VOWEL`.

  `chimera-core/tests/fixtures/disk_codes_v1.txt` is append-only. INIT's new values are part of the decode base, so they are frozen too.
- **Values the spec pins:**
  - FRMT is `100·2^(v/24)` Hz (0–168, default 72) and BW `20·2^(v/24)` Hz (0–216, default 72).
  - SKIRT runs 0–3 (default 1), with `q = 2^(SKIRT+1)`. KEY and RES run 0–100 (default 0), VSLOT 0–4 (default AUTO), and VOWEL 0–100 (default 50, `x = v/25`).
  - INIT's FRMT is 69, 83, 111, 123, 123 and 123.
  - The reference `R` is −12 dB (0.25119) of a full-scale sine's RMS, with `g_n = 0.251`.
  - KEY_REF is MIDI 60, and FC's floor is 20 Hz.
- **Budget:** `70·n_FRMT + 10·n_NOIS ≤ FORMANT_BUDGET` (225), derived from the cost constants and never typed in. The costs are `COST_FRMT_ONE` 90 (SKIRT 0), `COST_FRMT` 130 (SKIRT 1–3) and `COST_NOIS` 70.
- **Audio thread:**
  - no heap, no blocking, no libc, and no new per-sample table read: the new curves are read per block;
  - no new `unsafe` for the formant state;
  - every per-block value that moves is ramped across the block, and every phase is continuous. Parameter changes never snap.
- **Functional core, imperative shell.** Every law in the spec's § Principles is a `pub fn` in its module with unit tests. The engine and kernel only call them.
- **Type-driven.** `OpKind`, `Kinds`, `Skirt`, `VowelSlot` and `Formant` are enums or newtypes. A kind is read only through `OpKind`, and no raw u8 WAVE code is compared.
- **No MIDI CC source** (owner, #247): VOWEL, FRMT and BW are reached through the per-Part MIDI map, which this plan doesn't build.
- **Hardware:** nothing is flashed before the one shared ship flash (Task 19).
- **Green per task:** `just check` passes before every commit. It runs the core with and without `master-tape`, the desktop, every firmware build, clippy and `just stack-check`.
- **Commits:**
  - terse and plain, with no type prefix of any kind (owner's rule, which overrides any harness reminder);
  - comments only where genuinely needed;
  - stage named paths only, and never `docs/chimera-ui-ux-spec.md` or `chimera.bin`;
  - bugs and follow-ups go in GitHub issues (joegiralt/chimera), referenced by URL.
- **ADRs:**
  - `0000-template.md`, `Status: Proposed`, and a row in `docs/adr/README.md`;
  - never edit an accepted ADR;
  - numbers: modal2 holds 0056–0058, main 0059 and ORBIT 0060–0061, so **Phase 0's ADR is 0062 and the formant ADR is 0063**. Recheck `docs/adr/` on `main` when each is written, and take the next free number if either is gone.

## Review Focus

These are the inputs the spec implies but none of its tests exercise, most likely to bite first. Each has its test in the named task.

1. **A kind change under a held note**, including a fast WAVE spin across table, FRMT and NOIS. Expect a duck and swap with no click, and the new kind starting closed (§ 8 Swap). Tests: Task 10, `kind_swaps_under_a_held_note_do_not_click` (NOIS); Task 11, the same test extended to FRMT.
2. **MORPH carrying a FRMT operator between modulator and carrier.** Its carrier weight and `g_c/g_m` ramp together, so the level glides and never steps. Task 12: `morph_across_a_formant_carrier_is_smooth`.
3. **Extreme pitch and ratio.** Examples: note 127, COARSE 63, TRNSP +24 with FRMT 0; f0 above FC; B_min past the Nyquist room. Expect a finite, bounded, possibly silent output, never NaN. Task 11: `formant_extremes_stay_finite`.
4. **Retrigger or steal on a sounding formant voice.** `note_on` keeps a sounding operator's state, so no click follows, and the harmonics stay exact. Task 11: `retrigger_on_a_sounding_formant_voice_does_not_click`.
5. **A Sound load, undo or engine switch while an operator page or the FRMT page is open.** The focus stays by identity, and the operator page re-resolves its cells for the new kind. An engine switch to Modal closes the operator page. Task 16: `a_sound_load_on_an_operator_page_keeps_it`.

## Rulings (where the spec or the brief left a choice)

1. **Phase 0 absorbs the spec's step 1**, as the brief asks: the dynamic sub-row mechanism and focus by `BlockDef.id`. The FRMT page itself comes in Task 15.
2. **Phase 0's hold is ADR 0044's.** It uses ORBIT's `ui::hold::HoldGates` and `HOLD_MS` = 500 ms, not the 2026-09-27 spec's own 1 s `press_kind`, because one mechanism serves every key.
   - A hold fires once, and its release does nothing.
   - Hold B*n* opens operator *n*'s page only from the edited Part's sound pages, outside ORBIT, on an Algo Part. Anywhere else it does nothing new, and in ORBIT it keeps `orbit_out`.
   - "EXIT" is the MENU tap. A tap of the same B*n* also returns.
   - ADR 0062 records this and amends 0044's key table (a new row).
3. **Phase 0 leaves some of #37 out:** the envelope playhead dot and its `OpLevels` telemetry stay in #37, with gang edit, LEVEL SCALE and the living diagram. The formant viz shows FC and BW_eff computed from the stored parameters at C4, with no audio telemetry.
4. **One UAT and one flash.** Phase 0 merges after its desktop QA gate. Its hardware UAT items join the single end list at the shared ship flash, per the owner's hardware-at-ship rule; the spec had given Phase 0 its own UAT.
5. **`(AlgoOp(n), WAVE)` is served through `AlgoParams`** by changing `Blocks` to return guards:
   - `block` returns `Option<BlockView<'_>>`, and `block_mut` returns `Option<BlockViewMut<'_>>`. Both deref to `dyn Block`.
   - For `AlgoOp(n)` the guard is `OpView` or `OpViewMut`, which reads and writes WAVE through `Kinds`.
   - `AlgoOpParams` has no wave byte, and `AlgoParams::kinds` is private. So no write path can skip the check, and `ops` stays `pub` for every other field.
6. **Naming in the kernel.** The spec's `OpState` enum is named `KindState`, because `kernel.rs` already has a private `OpState`.
   - Each operator's noise generator lives in `Kernel::rng: [u32; OPS]`, outside the enum, so its seed survives kind swaps.
   - `AlgoEngine::seed(VoiceIdx)` seeds it, and is called by `Voice::init_in_place` and by `EngineSlot::rebuild` when it rebuilds Algo.
7. **Cost terms first.** The cost terms and billing by kind land in Phase 1, because the budget derives from them. Task 17 recounts them from the release disassembly.
8. **The demo Sounds** exist from Task 6 as `factory::demo_sound(Demo)`, so that every later gate renders them. Task 18 adds them to the bank at slots 8–10 and tunes each VOL to within ±1 dB of ADR 0058's `REFERENCE_LUFS`. If VOL can't reach it with the peak ≤ 1.0, stop and ask the owner.
9. **v1 fixtures.** `v1_fixtures_equal_factory` and `factory_level_test` stay pinned to the eight existing fixtures and levels. New Sounds are covered by round-trip tests, not new v1 fixtures.
10. **The ρ_q and p_q curves** are flash literals. An `#[ignore]`d generator test prints them, and `laws_are_pinned` recomputes them.
11. **Dimming** covers every parameter a kind ignores:
    - the six formant parameters on a table operator;
    - SKIRT and KEY on NOIS;
    - RES on FRMT;
    - COARSE, FINE, DETUNE and FDBK on NOIS.
12. **The sweep's formant fixture** plays its single note at C3 (48). The chord is unchanged.
13. **SKIRT is latched only when the operator is idle** at note-on, so a retrigger never changes an open grain's exponent. Slot, KEY and RES latch at every note-on.

## Rebase points

These change on other branches. Re-read them before the task that touches them.

- **Algo code.** `modal2-resonators` changes `engine.rs`: bffbb67 "An Algo voice is held by the carriers heard at its MORPH". It also changes `active` and the carrier test in `render`. Formant carriers follow whatever rule is there.
- **`ui/chain.rs`:**
  - Modal 2 Task 15: `ChainNav::go`, `next_on_part_button`, `sound_left` and `mix_page` (ADR 0057).
  - ORBIT: `ChainId::Orbit`, `handle_input(c, &Presses)`, `open_part`, `go_to`, and `Place`.
  - Task 1 changes the stored page positions (`SoundPage`, and `Place` for Part chains) to page ids.
- **ADR 0058:** `REFERENCE_LUFS` = −15.0 in `param_sweep_test.rs`, and `EngineSlot::out_gain`, which is 1 for Algo. Formant operators don't change it.
- **`tests/common/sweep.rs`** (param-sweep, on modal2): `Patch`, `play`'s hard-coded note 60, `registry`, `level`, `lufs`, `Tape::click`.
- **`disk_codes_v1.txt`:** ORBIT and projects-core append too. Keep every append in merge order, with ours last.
- **`BlockDef.id`:** the new pages take the next free ids after ORBIT's and projects-core's (68+ on modal2). The existing id-uniqueness test decides.

## QA gate (every phase ends with it)

Run each item that exists by that phase, and note any that doesn't apply yet.

- **G1, tests:** `just check`.
- **G2, the sweep:** `cargo test --release -p chimera-core --test param_sweep_test -- --include-ignored`. Every new registry parameter is picked up automatically (`the_sweep_covers_the_registry`). From Task 10 on it is also swept on `Patch::Formant`. No new defect is allowed.
- **G3, levels:** `cargo test --release -p chimera-core --test param_sweep_test modal_models_match_the_loudness_reference levels_are_safe`. ALGO INIT must stay at −15.0 ± 0.1 LUFS. From Task 10 on, also run `formant_fixture_level`, which prints `Patch::Formant`'s LUFS against the reference and asserts its peak ≤ 1.0.
- **G4, the floor:** `cargo test -p chimera-core --test cost_test formant_budget_keeps_four_voices`, and again with `--features master-tape`. The test itself checks both bus costs.
- **G5, demo renders:**
  - Run `cargo test --release -p chimera-core --test formant_demo_test`. It checks clicks, peak, level and aliasing.
  - Then run `cargo test --release -p chimera-core --test formant_demo_test -- --ignored render_formant_demo_clips`, and listen to `target/demo/*.wav` on the desktop.
  - BREATH applies from Task 10, and CHOIR and TALK BASS from Task 11.
- **G6, goldens:** `cargo test -p chimera-core --test golden_test --test screen_golden_test --test codec_compat_test` shows no diff (Task 15's one re-record excepted).

A gate that finds a defect gets it fixed before the next phase starts, or filed as a GitHub issue and named in the PR if the owner defers it.

## File structure

| File | Responsibility |
|---|---|
| `chimera-core/src/ui/chain.rs` | `PartChain`, `ChainNav::set_part_chain`, the operator-page overlay `op_page: Option<Op>`, and B*n* holds (Tasks 1–2, 15) |
| `chimera-core/src/ui/block_def.rs` | `ChainDef2::locate`, the pure `refocus`, and `SlotBinding::OpCell` (Tasks 1, 16) |
| `chimera-core/src/ui/block_registry.rs` | `ALGO_OP_PAGE`, `ALGO_FORMANT`, `ALGO_FORMANT_CHAIN`, and VOWEL on `ALGO_ALG` (Tasks 3, 15) |
| `chimera-core/src/ui/op_page.rs` (new) | `op_page_cells(OpKind)`, and the op-page viz's pure curves (Tasks 3, 16) |
| `chimera-core/src/ui/view.rs` | `SlotCtx.op_kind`, the new `dimmed` arms (Tasks 15–16) |
| `chimera-core/src/addr.rs` | `BlockView`, `BlockViewMut`, the new `Blocks` signatures (Task 5) |
| `chimera-core/src/dsp/algo/kinds.rs` (new) | `OpKind`, the code, name and ident tables, `Kinds`, `OverBudget`, `spend`, `fits`, `step_kind`, `FORMANT_BUDGET` (Tasks 4–5) |
| `chimera-core/src/dsp/algo/params.rs` | the new fields, specs and INIT, `OpView`, `OpViewMut`, `try_set_kind` (Tasks 5–6) |
| `chimera-core/src/dsp/algo/formant.rs` (new) | `Skirt`, `window`, the constants and curves, the width floor, the gains, the edge law, `formant_law` (Tasks 7–8) |
| `chimera-core/src/dsp/algo/vowel.rs` (new) | `VowelSlot`, `Formant`, `VOWEL_HZ`, `VOWEL_OCT`, `vowel_delta`, `fc_hz`, `resolve_slots` (Task 9) |
| `chimera-core/src/dsp/algo/noise.rs` (new) | `noise_coef`, `res_mix`, `noise_edge`, `noise_seed`, `NOISE_GAIN`, `NOISE_CLAMP` (Task 8) |
| `chimera-core/src/dsp/algo/kernel.rs` | `KindBlock`, `KindState`, `run_formant`, `run_noise`, and the `step` arms (Tasks 10–12) |
| `chimera-core/src/dsp/algo/engine.rs` | kinds, the latches, the per-block kind blocks, `cost` by kind, the `COST_*` terms, `AlgoLive` (Tasks 4, 10–14) |
| `chimera-core/src/factory.rs` | `Demo`, `demo_sound`, and bank slots 8–10 (Tasks 6, 18) |
| `chimera-core/tests/common/formant.rs` (new) | `render_algo`, `grain_fc`, `spectrum`, `off_grid_db`, `synth_level` (the probes' exact one-period synthesis) (Task 7 on) |
| `chimera-stm32/src/bench.rs` | the bench rows FRMT0, FRMT3, FRMT PM, NOIS, FRMT MAX and CHOIR (Task 17) |

---

# Phase 0 — Operator pages and page identity (branch `algo-op-pages`)

### Task 1: Page focus by identity; the Part's chain is picked per Sound

**Files:**
- Modify:
  - `chimera-core/src/ui/block_def.rs`;
  - `chimera-core/src/ui/chain.rs`;
  - `chimera-core/src/ui/mod.rs` (the per-tick resolve, and `rebuild_sources`);
  - `chimera-core/src/ui/browser.rs`;
  - `chimera-core/src/ui/orbit/nav.rs` (`Place` for a Part chain).
- Create: `chimera-core/tests/page_identity_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  impl ChainDef2 { pub fn locate(&self, id: u16) -> Option<(usize, usize)>; }   // (node, sub_page)
  pub fn refocus(from: &ChainDef2, at: (usize, usize), to: &ChainDef2) -> (usize, usize);
  pub enum PartChain { Algo, Modal }   // Task 15 adds AlgoFormant
  impl PartChain {
      pub fn of(sound: &Sound) -> PartChain;
      pub fn def(self) -> &'static ChainDef2;
      pub fn engine(self) -> EngineType;
  }
  impl ChainNav { pub fn set_part_chain(&mut self, pc: PartChain); }   // replaces set_engine
  ```
- `refocus` keeps the page with the same id. If that page is gone it falls back to the node's home `(node, 0)`, and if the node is gone to `(0, 0)`.
- `set_part_chain` goes home on an engine change, as `set_engine` does today. Otherwise it applies `refocus`.
- `SoundPage` stores `(part, chain: PartChain, page: u16)` instead of node and sub_page. ORBIT's `Place` gains `page: u16` for `ChainId::Part`, and restores through `locate`.
- `UiState` calls `nav.set_part_chain(PartChain::of(&edited_sound))` once per UI tick, after input and before draw. That covers load, undo and every edit.
- `chain_def_for(engine)` is removed. Its callers use `PartChain::of(sound).def()`.

- [ ] **Step 1: Write the failing tests** in `page_identity_test.rs`. Two test chains are built from `block_registry`'s `group` defs:
  - OLD is home H with subs [COARSE, FINE, DETUNE];
  - NEW is home H with subs [LEVEL, COARSE, FINE, DETUNE];
  - SHORT has one node.

```rust
#[test]
fn refocus_keeps_the_page_by_identity() {
    assert_eq!(refocus(&OLD, (1, 3), &NEW), (1, 4)); // DETUNE stays DETUNE
    assert_eq!(refocus(&NEW, (1, 1), &OLD), (1, 0)); // LEVEL left: its node's home
    assert_eq!(refocus(&OLD, (1, 3), &SHORT), (0, 0)); // node gone: chain home
    assert_eq!(OLD.locate(ALGO_DETUNE.id), Some((1, 3)));
}
#[test]
fn an_engine_change_still_goes_home() { /* Algo at (1,3); set_part_chain(Modal) → (0,0) */ }
#[test]
fn the_mixer_round_trip_returns_by_identity() { /* ADR 0057's B<n> round trip lands on the same BlockDef.id */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test page_identity_test`. Expected: FAIL, because `refocus`, `locate` and `PartChain` aren't defined.
- [ ] **Step 3: Implement** the interfaces above. `refocus` is `from.active_def(at)` → `to.locate(id)`, with the fallbacks.
- [ ] **Step 4: Run the new tests, then the existing UI tests.** Run `cargo test -p chimera-core --test page_identity_test --test ui_test --test all_pages_walk_test --test screen_golden_test`, then `just check`. Expected: PASS, with every screen golden unchanged. The modal2 and ORBIT navigation tests pass as they are.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests/page_identity_test.rs
git commit -m "Page focus is kept by page id; the Part's chain is picked per Sound"
```

### Task 2: Hold B*n* opens operator *n*'s page (ADR 0062)

**Files:**
- Modify:
  - `chimera-core/src/ui/chain.rs`;
  - `chimera-core/src/ui/mod.rs` (the `SlotCtx.sel_op` override and the redraw key);
  - `docs/adr/README.md`.
- Create:
  - `chimera-core/tests/op_page_test.rs`;
  - `docs/adr/0062-hold-bn-opens-operator-pages.md`.

**Interfaces:**
- Consumes:
  - ORBIT's `ui::hold::{Presses, Press, HOLD_MS}`;
  - `ChainNav::handle_input(&mut self, c: &impl Controls, p: &Presses) -> bool`.
- Produces:
  - `ChainNav.op_page: Option<Op>`, an overlay: node and sub_page don't move under it;
  - `ChainNav::open_op(&mut self, op: Op)` and `ChainNav::close_op(&mut self)`;
  - `active_block_def()` returns `&ALGO_OP_PAGE` while `op_page` is `Some` (Task 3 defines the page; this task uses a placeholder with `EMPTY` cells, replaced in Task 3);
  - UiState builds `SlotCtx` with `sel_op = nav.op_page.unwrap_or(ui.sel_op)`;
  - `op_page` is part of the redraw key (`mod.rs`'s region encoding).
- **Keys on an operator page:**
  - PLUS goes to `op.nudged(1)` and MINUS to `op.nudged(-1)`, each wrapping 6 → 1 and 1 → 6;
  - the MENU tap and a tap of the same B*n* call `close_op`;
  - a tap of another B*n* calls `close_op`, then does its ADR 0044 meaning;
  - EDIT and SEQ do nothing.
- **When `close_op` fires on its own:** on an engine change to Modal, and on leaving the Part chain.

- [ ] **Step 1: Write the failing tests** in `op_page_test.rs`. Drive `UiState` through the harness that ORBIT's `hold_gate_*` tests use, with elapsed ms.

```rust
#[test] fn holding_b3_on_algo_pages_opens_operator_3() { /* hold B3 ≥ HOLD_MS on Part 1's pages → nav.op_page == Some(Op::C); node, sub_page and the edited Part unchanged; release → nothing */ }
#[test] fn a_short_b_press_keeps_its_meaning_on_release() { /* 499 ms: next_on_part_button's meaning, on release; op_page None */ }
#[test] fn a_hold_on_a_modal_part_does_nothing() {}
#[test] fn a_hold_in_orbit_keeps_orbit_out() {}
#[test] fn plus_and_minus_wrap_the_operators() { /* F +1 → A; A −1 → F */ }
#[test] fn menu_or_the_same_b_returns_to_the_page_it_came_from() {}
#[test] fn an_engine_change_to_modal_closes_the_operator_page() {}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test op_page_test`. Expected: FAIL, because `op_page` isn't defined.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Write ADR 0062**, "Hold B*n* opens operator *n*'s page". It records:
  - the gesture, and its scope (Algo Part, sound pages, outside ORBIT);
  - the shared `HOLD_MS`, which supersedes the 2026-09-27 spec's 1 s;
  - MENU tap as EXIT;
  - the overlay model;
  - that it amends ADR 0044's key table with one row.

  Add its README row.
- [ ] **Step 5: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test op_page_test`, then `just check`. Expected: PASS.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests/op_page_test.rs docs/adr/0062-hold-bn-opens-operator-pages.md docs/adr/README.md
git commit -m "Hold B1-B6 opens that operator's page (ADR 0062)"
```

### Task 3: The operator page's cells and viz

**Files:**
- Modify:
  - `chimera-core/src/ui/block_registry.rs`;
  - `chimera-core/src/ui/viz.rs`;
  - `chimera-core/tests/all_pages_walk_test.rs`;
  - `chimera-core/tests/screen_golden_test.rs`.
- Create: `chimera-core/src/ui/op_page.rs`.
- Test:
  - `chimera-core/tests/op_page_test.rs`;
  - `chimera-core/tests/screen/` (a new golden, `algo_op_page_3`).

**Interfaces:**
- Produces:
  - `pub static ALGO_OP_PAGE: BlockDef`, with the next free id, name "Operator", short "OP", `viz: VizType::OpPage` and cells `SelectedOp(WAVE, COARSE, FINE, LEVEL, FEEDBACK, VELOCITY)` labelled WAVE, CRSE, FINE, LEVEL, FB and VEL;
  - `op_page::wave_cycle(wave: WaveId) -> [i8; 64]`, the table's mip 0 decimated for drawing;
  - `op_page::env_shape(r: EnvRates) -> [u8; 32]`, the AR, D1R, D1L, D2R and RR outline.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn op_page_cells_edit_their_operator() { /* for n in Op::ALL: open_op(n); each cell's encoder moves AlgoOp(n)'s WAVE, COARSE, FINE, LEVEL, FEEDBACK, VELOCITY, and no other op's */ }
#[test] fn the_all_pages_walk_reaches_six_operator_pages() {}
#[test] fn env_shape_rises_with_ar_and_falls_to_d1l() { /* AR 31: first point 255 within 1 column; D1L 15 plateau at 255 */ }
```

  Add the screen golden `algo_op_page_3`: INIT, operator 3.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test op_page_test --test all_pages_walk_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the page and the viz. The viz band draws one cycle beside the envelope, with no playhead (Ruling 3).
- [ ] **Step 4: Record the new golden only.** Run `cargo test -p chimera-core --test screen_golden_test`. Every existing golden must be unchanged. Then run `just check`.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "Operator pages show WAVE, COARSE, FINE, LEVEL, FB and VEL beside the wave and envelope"
```

- [ ] **Step 6: QA gate 0.** Run G1, G2, G3, G4 (the existing floor tests) and G6. G5 doesn't apply yet. Open the PR for `algo-op-pages`. The UAT items for this phase are in the end list.

---

# Phase 1 — Kinds, budget, parameters (branch `formant-ops`, rebased on `main` after Phase 0)

### Task 4: `OpKind`, the cost terms by kind, and the derived budget

**Files:**
- Create:
  - `chimera-core/src/dsp/algo/kinds.rs`;
  - `chimera-core/tests/kinds_test.rs`.
- Modify:
  - `chimera-core/src/dsp/algo/{mod.rs,engine.rs}`;
  - `chimera-core/src/modulation.rs` (`ModRouting::HEAVY`);
  - `chimera-core/src/dsp/fx_bus.rs` (`COST_TAPE` and `COST_NO_TAPE`);
  - `chimera-core/src/hw.rs` (`FLOOR_VOICES`);
  - `chimera-core/tests/cost_test.rs`.

**Interfaces:**
- Produces (`kinds.rs`):
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum OpKind { Table(WaveId), Formant, Noise }
  pub const OP_KIND_COUNT: usize = 18;
  pub static OP_KIND_NAMES: [&str; 18];   // WAVE_NAMES then "FRMT", "NOIS"
  pub static OP_KIND_IDENTS: [&str; 18];  // WAVE_IDENTS then "FRMT", "NOIS"
  impl OpKind {
      pub const fn from_code(c: u8) -> Option<OpKind>;  // 0–15 Table, 16 Formant, 17 Noise
      pub const fn clamped(c: u8) -> OpKind;            // from_code(min(c, 17))
      pub const fn code(self) -> u8;
      pub const fn is_formant(self) -> bool;            // Formant | Noise
      pub const fn charge(self) -> u32;                 // Table 0, Formant 70, Noise 10
  }
  pub const fn spend(k: &[OpKind; OPS]) -> u32;
  pub const fn fits(k: &[OpKind; OPS]) -> bool;         // spend ≤ FORMANT_BUDGET.0
  pub const FORMANT_BUDGET: Cost;
  ```
- The constants:
  - **the kind costs:** `AlgoEngine::{COST_FRMT_ONE: Cost(90), COST_FRMT: Cost(130), COST_NOIS: Cost(70)}`, each with the comment "estimate, ADR 0056's method; bench at ship";
  - **the charges:** Formant `COST_FRMT − COST_OP`, and Noise `COST_NOIS − COST_OP`;
  - **the heavy routes:** `ModRouting::HEAVY` = BASE + CLAMP + ENV_B + CURVE + BURST + SLIDE + DEST_FIRST + 15·DEST + 3·FUNC (315);
  - **the bus:** `FxBus::COST_TAPE` = `Cost(1470)` and `FxBus::COST_NO_TAPE` = `Cost(1160)`, with `FxBus::COST` cfg-selecting one;
  - **the floor:** `hw::FLOOR_VOICES: u32 = 4` (ADR 0026).
- `FORMANT_BUDGET` is:

  ```
  (for_cpu(CPU_HZ_REV_V).as_cost().0 − COST_TAPE.0) / FLOOR_VOICES
  − (HEAVY.0 + Voice::CHAIN_COST.0)
  − AlgoEngine::cost(&COSTLIEST_TABLE, &[false; OPS]).0
  ```

  `COSTLIEST_TABLE` is a `const AlgoParams` (Task 5 makes it const-constructible; until then a `const fn` builds it): A16/A17, six table operators at LEVEL 99 and FDBK 1. That gives 1,382 − 325 − 832 = 225.
- `AlgoEngine::cost` prices each active operator by `OpKind::clamped(p.ops[i].wave)`:
  - Table is `COST_OP`, Formant `COST_FRMT` and Noise `COST_NOIS`;
  - `COST_FEEDBACK` is billed for Table and Formant, never for Noise.

  Task 6 adds the SKIRT 0 price.

- [ ] **Step 1: Write the failing tests.**

```rust
// kinds_test.rs
#[test]
fn op_kind_codes_are_frozen() {
    assert_eq!(OpKind::from_code(15), Some(OpKind::Table(WaveId::SSAW)));
    assert_eq!(OpKind::from_code(16), Some(OpKind::Formant));
    assert_eq!(OpKind::from_code(17), Some(OpKind::Noise));
    assert_eq!(OpKind::from_code(18), None);
    assert_eq!(OpKind::clamped(200), OpKind::Noise);
    assert_eq!(&OP_KIND_IDENTS[16..], &["FRMT", "NOIS"]);
}
#[test]
fn the_budget_is_derived_and_its_rule_exact() {
    assert_eq!(FORMANT_BUDGET, Cost(225));
    assert_eq!(ModRouting::HEAVY, Cost(315));
    use OpKind::*; let t = Table(WaveId::W1);
    assert!(fits(&[Formant, Formant, Formant, Noise, t, t]));      // 220
    assert!(!fits(&[Formant, Formant, Formant, Noise, Noise, t])); // 230
    assert!(fits(&[Formant, Formant, Noise, Noise, Noise, Noise])); // 180
    assert!(!fits(&[Formant, Formant, Formant, Formant, t, t]));
    assert!(fits(&[Noise; 6]));
}
// cost_test.rs
#[test] fn heavy_routes_bill_their_named_sum() { /* heavy_routes(): ENV B BURST on the VCA, SHAPE off centre, a TIME route (slide), 16 destinations, LFOs 1–3 FUNC → ModRouting::cost == HEAVY */ }
#[test]
fn formant_budget_keeps_four_voices() {
    // All 3^6 assignments over {Table(W1), Formant, Noise}; A16/A17; LEVEL 99; FDBK 1 where billed.
    // fits(k) ⇒ voices(k, COST_TAPE) ≥ 4 and voices(k, COST_NO_TAPE) ≥ 4, on rev V,
    //   where voices = (7000 − bus) / (AlgoEngine::cost + HEAVY + CHAIN_COST).
    // The refused [F,F,F,F,t,t] and [F,F,F,N,N,t] bill 3 voices with the tape.
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test kinds_test --test cost_test`. Expected: FAIL, because `kinds` doesn't exist.
- [ ] **Step 3: Implement.** WAVE's spec keeps `max` 15 in this task, so codes 16 and 17 are unreachable until Task 5 checks them.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test kinds_test --test cost_test`, then `just check`. Expected: PASS, with every existing cost literal unchanged.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src chimera-core/tests/kinds_test.rs chimera-core/tests/cost_test.rs
git commit -m "Operator kinds, their cost terms, and a formant budget derived from them"
```

### Task 5: `Kinds` owns the operators' kinds; every write path checks the budget

**Files:**
- Modify:
  - `chimera-core/src/dsp/algo/{kinds.rs,params.rs,engine.rs}`;
  - `chimera-core/src/addr.rs`;
  - `chimera-core/src/params.rs`;
  - `chimera-core/src/preset.rs`;
  - `chimera-core/src/storage/{block_codec.rs,sound.rs,system.rs}`;
  - `chimera-core/src/ui/{page.rs,part_page.rs,mod.rs}`;
  - `chimera-core/src/dsp/voice.rs`;
  - `chimera-core/src/factory.rs`;
  - every other `Blocks` impl and caller;
  - `chimera-stm32/src/bench.rs`;
  - `chimera-core/tests/fixtures/disk_codes_v1.txt`;
  - `chimera-core/tests/disk_codes_test.rs`.
- Test: `chimera-core/tests/kinds_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Kinds([OpKind; OPS]);
  #[derive(Debug, PartialEq)] pub struct OverBudget;
  impl Kinds {
      pub const TABLE: Kinds;                                // all W1
      pub const fn all(&self) -> [OpKind; OPS];
      pub const fn new(k: [OpKind; OPS]) -> Kinds;           // panics if !fits: a const is checked at compile time
      pub const fn get(&self, op: usize) -> OpKind;
      pub fn with(self, op: usize, kind: OpKind) -> Result<Kinds, OverBudget>;
      pub fn any_formant(&self) -> bool;
  }
  pub struct KindStep { pub to: OpKind, pub skipped: bool }
  pub fn step_kind(k: Kinds, op: usize, delta: i8) -> KindStep;  // walks codes by delta's sign, skipping refused kinds; stays put if none fits
  // params.rs
  impl AlgoParams {
      pub fn kinds(&self) -> Kinds;
      pub fn try_set_kind(&mut self, op: usize, kind: OpKind) -> Result<(), OverBudget>;
      pub fn op(&self, op: Op) -> OpView<'_>;
      pub fn op_mut(&mut self, op: Op) -> OpViewMut<'_>;
      pub const fn single(wave: WaveId) -> Self;   // made const
  }
  // addr.rs
  pub enum BlockView<'a> { Plain(&'a dyn Block), Op(OpView<'a>) }          // Deref<Target = dyn Block + 'a>
  pub enum BlockViewMut<'a> { Plain(&'a mut dyn Block), Op(OpViewMut<'a>) } // DerefMut
  pub trait Blocks {
      fn block(&self, b: BlockRef) -> Option<BlockView<'_>>;
      fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>>;
  }
  ```
- **The guards.** `OpView` and `OpViewMut` implement `Block` over `ALGO_OP_SPECS`:
  - WAVE reads `kinds.get(op).code()`;
  - the WAVE write, `set_enum_code` and `nudge` go through `try_set_kind`. `nudge` uses `step_kind`, and a refused write leaves the kind as it was;
  - every other id delegates to `ops[op]`.
- **The WAVE spec** becomes `ParamSpec::choice(0, "WAVE", ValFmt::Names(&OP_KIND_NAMES), 17.0, 0.0)`, and the const assert becomes `ALGO_OP_SPECS[0].max as usize == OP_KIND_COUNT - 1`.
- **The encoder toast.** UiState, on an encoder turn of a `(AlgoOp(n), WAVE)` cell, calls `step_kind` first. If `skipped`, it toasts `FORMANT BUDGET FULL`.
- **The decoder.** `decode_block` returns `Result<Applied, FileError>`, with `pub struct Applied { pub refused_kind: bool }`. `refused_kind` is true when an `AlgoOp` WAVE code is a valid `OpKind` and `set_enum_code` refused it. `SoundDecoder::over_budget(&self) -> bool` ORs it over the file, and the Sound-load path toasts `FORMANT BUDGET FULL` when it is set.
- **The engine.** `AlgoEngine.waves: [WaveId; OPS]` becomes `kinds: [OpKind; OPS]`, and `Swap` compares kinds. A non-Table kind gets `gain_to = 0.0`, so it is silent until Tasks 10 and 11. Its mip and table fields use W1.
- **The factory.** Sounds build their kinds as `const` `Kinds::new([...])`, and `factory::op` no longer takes a wave.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn try_set_kind_refuses_overspend_and_changes_nothing() {
    let mut p = AlgoParams::default();
    for i in 0..3 { p.try_set_kind(i, OpKind::Formant).unwrap(); }
    let before = p.kinds();
    assert!(p.try_set_kind(3, OpKind::Formant).is_err());
    assert_eq!(p.kinds(), before);
    p.try_set_kind(3, OpKind::Noise).unwrap();
    assert!(p.try_set_kind(4, OpKind::Noise).is_err());
}
#[test]
fn every_write_path_checks_the_budget() {
    // 10_000 seeded random (op, code 0..=17) writes, rotating through:
    // block_mut(AlgoOp).set(WAVE, v), .set_enum_code(WAVE, c), .nudge(WAVE, ±1),
    // decode_block on a one-entry OP_x record, and the sweep's Bench::set.
    // After each: fits(&p.kinds().all()) holds.
}
#[test]
fn the_wave_encoder_steps_over_refused_kinds() {
    // 3 FRMT on ops 1–3; op 4 at SSAW: step_kind(+1) → Noise, skipped = true.
    // 3 FRMT + NOIS on op 4; op 5 at SSAW: step_kind(+1) → Table(SSAW), skipped = true.
    // op 4 NOIS: step_kind(−1) → Table(SSAW), skipped = true (FRMT refused).
    // UI: the same turn shows the toast "FORMANT BUDGET FULL".
}
#[test]
fn an_over_budget_file_decodes_its_excess_as_w1() {
    // a hand-built Sound file with ops 1–4 FRMT → ops 1–3 Formant, op 4 Table(W1); over_budget() is true
}
#[test]
#[should_panic]
fn kinds_new_rejects_overspend() { Kinds::new([OpKind::Formant; 6]); }
```

  Add a `compile_fail` doctest on `Kinds::new`: `const K: Kinds = Kinds::new([OpKind::Formant; 6]);`. Append the lines `E k 0 16 FRMT # FRMT` and `E k 0 17 NOIS # NOIS` for `k` = 3–8 to `disk_codes_v1.txt`. Point `wave_and_algo_idents_are_their_own_tables` at `OP_KIND_IDENTS`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test kinds_test --test disk_codes_test`. Expected: FAIL.
- [ ] **Step 3: Implement**, migrating every `Blocks` impl and caller. Callers mostly compile unchanged through `Deref`. `if let Some(b)` becomes `if let Some(mut b)` where it writes.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, and G6 shows no golden, factory hash or `codec_compat` hash moved.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src chimera-core/tests chimera-stm32/src/bench.rs
git commit -m "AlgoParams owns the operator kinds; every write checks the formant budget"
```

### Task 6: The new parameters, their formats, INIT, the frozen codes, the demo recipes and the sweep fixture

**Files:**
- Modify:
  - `chimera-core/src/dsp/algo/{params.rs,engine.rs}`;
  - `chimera-core/src/block.rs` (`ValFmt`);
  - `chimera-core/src/ui/fmt.rs`;
  - `chimera-core/src/factory.rs`;
  - `chimera-core/tests/fixtures/disk_codes_v1.txt`;
  - `chimera-core/tests/common/sweep.rs`;
  - `chimera-core/tests/param_sweep_test.rs`;
  - `chimera-core/tests/codec_compat_test.rs`;
  - `chimera-core/tests/valfmt_snap_test.rs`.
- Create:
  - `chimera-core/src/dsp/algo/vowel.rs` (only `VowelSlot` in this task);
  - `chimera-core/tests/formant_params_test.rs`;
  - `chimera-core/tests/formant_demo_test.rs`.

**Interfaces:**
- **`AlgoOpParams` fields:** `frmt: u8`, `bw: u8`, `skirt: u8`, `key: u8`, `res: u8`, `vslot: u8`, with `ParamId`s 13–18.
- **`AlgoParams` field:** `vowel: u8`, with `AlgoParams::VOWEL = ParamId(4)`.
- **The specs** follow § 2's tables exactly: labels, idents, ranges and defaults.
  - FRMT is `ValFmt::LogHz { base: 100, per_octave: 24 }` and BW `LogHz { base: 20, per_octave: 24 }`.
  - SKIRT is `Int(3)`, KEY and RES `Int(100)`, VSLOT `Names(&VSLOT_NAMES)` and VOWEL `ValFmt::Vowel`.
  - FRMT, BW and VOWEL are `modulatable: true`. `AlgoEngine` reads them every block from Task 10 on, and the modulatable test binds in Task 14.
- **`vowel.rs`:** `pub enum VowelSlot { Auto, F1, F2, F3, Off }` with `from_code`, `code`, and `VSLOT_NAMES` and `VSLOT_IDENTS` (`AUTO`, `F1`, `F2`, `F3`, `OFF`).
- **`params.rs`:** `pub fn frmt_hz(v: f32) -> f32` (`100·2^(v/24)`) and `pub fn bw_hz(v: f32) -> f32` (`20·2^(v/24)`). Both take the live, unrounded value.
- **Snap points:** `LogHz` snaps each octave (every 24 steps), and `Vowel` each vowel (0, 25, 50, 75, 100).
- **INIT:** FRMT `[69, 83, 111, 123, 123, 123]` and VOWEL 50. The other new fields take their spec defaults.
- **Billing:** `AlgoEngine::cost` bills a Formant operator with stored SKIRT 0 at `COST_FRMT_ONE`.
- **`factory.rs`:**
  ```rust
  pub enum Demo { Choir, TalkBass, Breath }
  pub fn demo_sound(d: Demo) -> Sound;   // the spec's § Demo clips recipes; kinds are const Kinds
  ```
  Their LEVELs start at 99 on carriers. VOL is tuned in Task 18.
- **`sweep.rs`:** `Patch::Formant`, named `"FORMANT"`: operators 1–3 FRMT, operator 4 NOIS, A1, operators 5 and 6 at LEVEL 0. `Patch::note(self) -> u8` is 48 for `Formant` and 60 otherwise, and `play` uses it for the single note. `patches()` doesn't list it yet: Task 10 adds it, once its operators sound, for `Algo` and `AlgoOp` params, both quick and thorough.
- **`formant_demo_test.rs`:**
  - `render_phrase(d: Demo) -> Vec<f32>` plays each demo's phrase through the sweep `Bench`: CHOIR a held C3–G3–C4 chord for 3 s; TALK BASS C2, D#2, G2 as 400 ms notes; BREATH one C4 held 2 s.
  - `demo_clips_are_clean` asserts finite output, peak ≤ 1.0 and no `Tape::click`. Tasks 11 and 18 add assertions.
  - `render_formant_demo_clips` is `#[ignore]` and writes 48 kHz 16-bit WAVs to `target/demo/`. The header is written by hand, with no new dependency.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn formant_params_follow_the_spec() {
    let p = AlgoOpParams::default();
    assert_eq!((p.frmt, p.bw, p.skirt, p.key, p.res, p.vslot), (72, 72, 1, 0, 0, 0));
    assert!((frmt_hz(72.0) - 800.0).abs() < 1e-3 && (frmt_hz(168.0) - 12_800.0).abs() < 0.1);
    assert!((bw_hz(72.0) - 160.0).abs() < 1e-3 && (bw_hz(216.0) - 10_240.0).abs() < 0.1);
    let ids: Vec<_> = ALGO_OP_SPECS[13..].iter().map(|s| (s.id.0, s.ident)).collect();
    assert_eq!(ids, [(13,"FRMT"),(14,"BW"),(15,"SKIRT"),(16,"KEY"),(17,"RES"),(18,"VSLOT")]);
    assert_eq!((ALGO_SPECS[4].id.0, ALGO_SPECS[4].ident, ALGO_SPECS[4].max), (4, "VOWEL", 100.0));
}
#[test]
fn init_carries_vowel_a() {
    let p = AlgoParams::default();
    assert_eq!(p.ops.map(|o| o.frmt), [69, 83, 111, 123, 123, 123]);
    assert_eq!(p.vowel, 50);
}
#[test]
fn log_hz_and_vowel_read_as_the_spec_says() {
    // fmt_val(69, LogHz{100,24}) == "734"; 83 → "1.10k"; 168 → "12.8k"; BW 0 → "20"
    // Vowel: 0 "U", 25 "O", 50 "A", 62 "A", 63 "E", 100 "I"
}
#[test]
fn old_patches_load_unchanged() {
    // Each v1 fixture decodes to its factory Sound. Its new fields equal INIT's (69, 83, 111, 123, 123, 123;
    // BW 72, SKIRT 1, KEY 0, RES 0, VSLOT AUTO; VOWEL 50), and its render hash is codec_compat's.
}
#[test] fn skirt_0_bills_the_one_lane_term() { /* one FRMT op, SKIRT 0 → COST_BASE + COST_FRMT_ONE */ }
#[test] fn the_demo_sounds_fit_the_budget() {}
```

  Append the `B 3..8 13..18` lines, the `E k 18 0..4` lines (`AUTO` to `OFF`) and `B 2 4 VOWEL # VOWEL` to `disk_codes_v1.txt`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_params_test --test disk_codes_test --test valfmt_snap_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`, then `cargo test --release -p chimera-core --test param_sweep_test the_sweep_covers_the_registry`. Expected: PASS. The goldens and `codec_compat` hashes are unchanged, because the new fields are inert on table operators.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src chimera-core/tests
git commit -m "FRMT, BW, SKIRT, KEY, RES, SLOT and VOWEL are stored with frozen codes"
```

- [ ] **Step 6: QA gate 1.** Run G1, G2 (the new params, swept on the existing patches, where they are inert), G3, G4 and G6. G5 runs only `demo_clips_are_clean`; silence passes it, and the kinds sound from Task 10.

---

# Phase 2 — The pure laws

### Task 7: The window, the SKIRT constants and the curves

**Files:**
- Create:
  - `chimera-core/src/dsp/algo/formant.rs`;
  - `chimera-core/tests/formant_laws_test.rs`;
  - `chimera-core/tests/common/formant.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Skirt { S0, S1, S2, S3 }
  impl Skirt {
      pub const fn from_code(v: u8) -> Skirt;  // clamped to 3
      pub const fn q(self) -> u32;             // 2, 4, 8, 16
      pub const fn k(self) -> f32;             // 1.3748, 1.7341, 2.2910, 3.1209
      pub const fn c_q(self) -> f32;           // 0.40635, 0.29954, 0.21653, 0.15486
      pub const fn m_q(self) -> f32;           // 5.49, 2.62, 2.07, 1.97
  }
  pub fn window(x: f32, s: Skirt) -> f32;      // clamp x to [0,1], u = 4x(1−x), square s+1 times
  pub fn rho(s: Skirt, nu: f32) -> f32;        // W²'s normalised transform, 33 points on [0, 4], linear, flat past 4
  pub fn lane_peak(s: Skirt, nu: f32) -> f32;  // max |W(x)·sin(2πν(x − ½))|, same grid
  ```
- **`tests/common/formant.rs`:** `synth_level(f0, fc, bw, s) -> (level_db_vs_R, peak, crest_db)`. It is the probes' exact one-period synthesis (`fprobe/fix.py`'s `sim` and `capped`), run on the laws of Task 8. The tests below and in Task 8 share it.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn window_is_the_polynomial_bell() {
    for s in [Skirt::S0, Skirt::S1, Skirt::S2, Skirt::S3] {
        assert_eq!(window(0.5, s), 1.0);
        assert_eq!((window(0.0, s), window(1.0, s), window(-0.2, s), window(1.3, s)), (0.0, 0.0, 0.0, 0.0));
        assert!((window(0.25, s) - 0.75f32.powi(s.q() as i32)).abs() < 1e-6);
    }
}
#[test]
fn laws_are_pinned() {
    // A 4,000-point midpoint integration in the test:
    //   K (−3 dB full width × length) within 0.5 % of Skirt::k;
    //   c_q equal to 4^(2q)·((2q)!)²/(4q+1)! within 1e-5;
    //   the −60 dB upper extent / K within 0.5 % of m_q (scan ν in 0.005 steps);
    //   first sidelobes −27.7, −38.5, −56.4, −87.0 dB (±0.3 dB);
    //   every rho and lane_peak grid point within 0.5 % (or 1e-4 absolute).
}
#[test]
fn curves_hold_past_four() { /* rho(s, 5.0) == rho(s, 4.0); lane_peak likewise */ }
#[test]
#[ignore]
fn print_curves() { /* prints RHO and PEAK literal tables for formant.rs */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_laws_test`. Expected: FAIL, because `formant` doesn't exist.
- [ ] **Step 3: Implement.** Generate the literals with `cargo test -p chimera-core --test formant_laws_test -- --ignored print_curves --nocapture` and paste them into `formant.rs` as `static RHO: [[f32; 33]; 4]` and `static PEAK: [[f32; 33]; 4]`.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test formant_laws_test`, then `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/algo chimera-core/tests/formant_laws_test.rs chimera-core/tests/common
git commit -m "The formant window and its SKIRT constants, measured and pinned"
```

### Task 8: The width floor, the level and peak gains, and the edge laws (FRMT and NOIS)

**Files:**
- Modify: `chimera-core/src/dsp/algo/formant.rs`.
- Create: `chimera-core/src/dsp/algo/noise.rs`.
- Test: `chimera-core/tests/formant_laws_test.rs`.

**Interfaces:**
- Produces (`formant.rs`):
  ```rust
  pub const R_REF: f32 = 0.251_19;             // 10^(−12/20)
  pub const BRACKET_FLOOR: f32 = 0.05;
  pub const NYQUIST_HZ: f32 = 24_000.0;
  pub fn width_floor(f0: f32) -> f32;          // Skirt::S3.k() · f0 / 2
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct FormantLaw {
      pub fc: f32,        // Hz, after the edge clamp, ≥ 20
      pub bw_eff: f32,    // Hz
      pub d: f32,         // window length in periods
      pub r_w: f32,       // window cycles per sample
      pub slack: f32,     // 2 − d, periods
      pub g_m: f32,       // 1/p̂
      pub g_c: f32,       // min(R_REF·g_rms, 1/p̂)
      pub edge: f32,      // 0..=1
  }
  pub fn formant_law(f0: f32, fc: f32, bw: f32, s: Skirt, sr: f32) -> FormantLaw;
  ```
- `formant_law` in order:
  1. `b_min = width_floor(f0)` and `bw_eff = hypot(bw, b_min)`.
  2. `room = (NYQUIST_HZ − fc)/m_q`.
     - If `room ≥ b_min`, then `bw_eff = min(bw_eff, room)` and `edge = 1`.
     - Otherwise `bw_eff = b_min`, `edge = clamp((room − b_min/2)/(b_min/2), 0, 1)`, and `fc = min(fc, NYQUIST_HZ − m_q·b_min/2)`.
  3. `fc = max(fc, 20)`, `L = k/bw_eff`, `d = L·f0`, `r_w = bw_eff/(k·sr)` and `slack = 2 − d`.
  4. `p̂ = min(1, lane_peak(s, fc·L)·(if d > 1 {2} else {1}))` and `g_m = 1/p̂`.
  5. `g_rms = 1/sqrt(d·c_q·max(1 − rho(s, 2·fc·L), BRACKET_FLOOR))` and `g_c = min(R_REF·g_rms, 1/p̂)`.
- Produces (`noise.rs`):
  ```rust
  pub const NOISE_GAIN: f32 = 0.251;
  pub const NOISE_CLAMP: f32 = 3.8;
  pub fn noise_coef(width: f32, sr: f32) -> (f32, f32); // (c, k_n): f_lp = 0.777·width, c = 1 − e^(−2π f_lp/sr), k_n = √3/(c²·sqrt((1+a²)/(1−a²)³)), a = 1 − c
  pub fn res_mix(res: u8) -> (f32, f32);                 // (α, β) = (sqrt(1 − R), sqrt(R)), R = res/100
  pub fn noise_edge(fc: f32, width: f32) -> f32;         // 1 at fc + width/2 ≤ 16 kHz, linear to 0 at 20 kHz
  pub fn noise_seed(voice: u8, op: usize) -> u32;        // nonzero, distinct per (voice, op)
  ```

- [ ] **Step 1: Write the failing tests.** The level numbers come from the review's probes (`fix.py`, `nois.py`).

```rust
#[test]
fn width_floor_is_skirt_free() {
    assert!((width_floor(220.0) - 343.3).abs() < 0.5);         // A3
    assert!((width_floor(98.0) - 152.9).abs() < 0.5);          // G2
    for s in ALL_SKIRTS { let l = formant_law(220.0, 1000.0, 20.0, s, 48_000.0);
        assert!((l.bw_eff - 343.9).abs() < 0.5); }              // hypot(20, 343.3), every SKIRT
}
#[test]
fn a_window_never_exceeds_two_periods() {
    // max d over f0 ∈ {33..2093 Hz}, bw = 0: [0.88, 1.11, 1.47, 2.0] (±0.01) by SKIRT
}
#[test]
fn rms_gain_uses_the_exact_energy() {
    // synth_level at C3 (130.8 Hz), FC 1 kHz, BW ∈ {524, 1000, 1500, 2000, 3000, 4000}, every SKIRT:
    //   level within ±0.1 dB of R.
    // Also (523.3, 270, 160, S1) and (523.3, 300, 160, S3): within ±0.1 dB.
    // The first draft's 1/sqrt(d·c_q) at (130.8, 1000, 4000, S1) is 7.47 dB low (sweep.py):
    //   assert that it is more than 6 dB off, so the test pins the fix.
}
#[test]
fn the_ceiling_bounds_the_peak() {
    // Every synth_level peak ≤ 1.0 + 1e-3 over f0 ∈ {C1..C7}, FC ∈ {270, 730, 1000, 2440, 8000}, BW ∈ {20, 160, 2000, 10000}, all SKIRTs.
    // Bound cases (fix.py capped): (130.8, 1000, 10000, S1) level −5.83 ± 0.5 dB vs R, peak 1.000 ± 0.005;
    //   (65.4, 1000, 2000, S1) −2.23 ± 0.5 dB.
    // Unbound: (130.8, 1000, 1000, S1) peak 0.746 ± 0.02; (523.3, 270, 160, S1) 0.287 ± 0.02.
}
#[test]
fn the_edge_narrows_then_fades() {
    let l = formant_law(130.8, 8000.0, 10_000.0, Skirt::S0, 48_000.0);
    assert!((l.bw_eff - 2914.0).abs() < 5.0 && l.edge == 1.0);   // (24k − 8k)/5.49
    // room = b_min/2 → edge 0; room = 0.75·b_min → edge 0.5 (±0.01)
}
#[test]
fn noise_band_laws() {
    // noise_coef: 20 s of uniform noise through two poles at x = k_n·n → y2 RMS 1.0 ± 1 %, WIDTH 20, 500, 4000
    // res_mix: α² + β² = 1 at 0, 50, 100; NOISE_GAIN·sqrt(3.8² + 1) ≤ 0.99
    // noise_edge(15_000.0, 2_000.0) == 1.0; noise_edge(17_000.0, 2_000.0) == 0.5; noise_edge(19_000.0, 2_000.0) == 0.0
    // noise_seed: nonzero and distinct over voices 0..8 × ops 0..6
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_laws_test`. Expected: FAIL.
- [ ] **Step 3: Implement** both modules as the interfaces above say. `hypot`, `exp` and `sqrt` come from `libm` or `dsp/algo/math.rs`.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test formant_laws_test`, then `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/algo chimera-core/tests
git commit -m "Formant and noise laws: SKIRT-free width floor, exact-energy level, peak ceiling, -60 dB edge"
```

### Task 9: `fc_hz`, the vowel table and slot resolution

**Files:**
- Modify: `chimera-core/src/dsp/algo/vowel.rs`.
- Test: `chimera-core/tests/formant_laws_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Formant { F1, F2, F3 }
  pub const KEY_REF: f32 = 60.0;
  pub static VOWEL_HZ: [[f32; 3]; 5];   // U O A E I: [300,870,2240],[570,840,2410],[730,1090,2440],[530,1840,2480],[270,2290,3010]
  pub static VOWEL_OCT: [[f32; 3]; 5];  // log2(VOWEL_HZ[v][s] / VOWEL_HZ[A][s]), literals
  pub fn vowel_delta(slot: Formant, vowel: f32) -> f32;   // octaves; x = vowel/25 clamped to [0,4], lerp between rows
  pub fn fc_hz(frmt: f32, vowel: f32, slot: Option<Formant>, key: f32, pitch: f32) -> f32;
  //   frmt_hz(frmt) · 2^(vowel_delta + (key/100)·(pitch − KEY_REF)/12), floored at 20 Hz; key 0 for NOIS
  pub fn resolve_slots(kinds: &Kinds, vslot: [VowelSlot; OPS]) -> [Option<Formant>; OPS];
  ```
- `resolve_slots` works like this:
  - a table operator gets `None`;
  - F1, F2 and F3 give that formant, and OFF gives `None`;
  - AUTO operators are counted among the formant-kind operators set to AUTO, from operator 1: the first gets F1, the second F2 and the third F3, and any further one gets `None`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn fc_law_is_exact() {
    let a = 72.0; // 800 Hz
    assert!((fc_hz(a, 50.0, None, 50.0, 72.0) - 1131.4).abs() / 1131.4 < 1e-3); // KEY 50, C5
    for (key, note, want) in [(0.0, 48.0, 800.0), (100.0, 48.0, 400.0), (100.0, 72.0, 1600.0), (50.0, 60.0, 800.0)] {
        assert!((fc_hz(a, 50.0, None, key, note) - want).abs() / want < 1e-3);
    }
    assert!((fc_hz(a, 0.0, Some(Formant::F1), 0.0, 60.0) - 800.0 * 300.0 / 730.0).abs() < 0.8); // U
    assert_eq!(fc_hz(0.0, 100.0, Some(Formant::F1), 100.0, 0.0), 20.0);                           // the floor
}
#[test]
fn vowel_table_is_peterson_barney_on_the_u_o_a_e_i_path() {
    // VOWEL_OCT recomputes from VOWEL_HZ within 1e-6; row A is all 0.
    // vowel_delta(F2, 75.0) == log2(1840/1090); vowel_delta(F2, 62.5) is the log-lerp midpoint of A and E.
}
#[test]
fn vowel_slots_resolve() {
    // FRMT on ops 2, 4, 5 (AUTO) → [None, F1, None, F2, F3, None]
    // FRMT on 1–5 AUTO → op 5 None; ops 1, 2 set F1, ops 3, 4 AUTO → [F1, F1, F1, F2, ..]
    // OFF → None; NOIS on AUTO counts like FRMT
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_laws_test`. Expected: FAIL.
- [ ] **Step 3: Implement.** `VOWEL_OCT` is literals, which the test recomputes.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test formant_laws_test`, then `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/algo/vowel.rs chimera-core/tests/formant_laws_test.rs
git commit -m "VOWEL runs U-O-A-E-I over Peterson and Barney's averages; slots resolve by setting"
```

- [ ] **Step 6: QA gate 2.** Run G1 and G6. G2–G5 are unchanged from gate 1, since nothing new sounds yet.

---

# Phase 3 — NOIS

### Task 10: The NOIS kernel path

**Files:**
- Modify:
  - `chimera-core/src/dsp/algo/{kernel.rs,engine.rs}`;
  - `chimera-core/src/dsp/engines.rs`;
  - `chimera-core/src/dsp/voice.rs` (`seed`);
  - `chimera-core/tests/algo_kernel_test.rs` (its `op()` helper and the model).
- Create: `chimera-core/tests/algo_noise_test.rs`.
- Test: `chimera-core/tests/param_sweep_test.rs` (`formant_fixture_level`).

**Interfaces:**
- Produces (`kernel.rs`):
  ```rust
  pub enum KindBlock {
      Table { lo: &'static Table, hi: &'static Table, xfade_from: f32, xfade_to: f32 },
      Formant(FormantBlock),   // Task 11
      Noise(NoiseBlock),
  }
  pub struct NoiseBlock { pub inc_c: u32, pub c_from: f32, pub c_to: f32, pub k_from: f32, pub k_to: f32, pub alpha: f32, pub beta: f32 }
  pub struct OpBlock { pub inc: u32, pub gain_from: f32, pub gain_to: f32, pub feedback: f32, pub kind: KindBlock }
  enum KindState { Table, Formant { w: [f32; 2], c: [u32; 2], older: u8 }, Noise { y1: f32, y2: f32 } }
  impl Kernel {
      pub fn seed(&mut self, voice: u8);                // rng[op] = noise_seed(voice, op)
      pub fn reset(&mut self, op: usize, kind: OpKind); // the state starts closed (§ 8 Swap)
  }
  pub fn sine(p: u32) -> f32;                           // W1 mip 0, interpolated, in [−1, 1]
  ```
- **`AlgoEngine`:**
  - `latch: [OpLatch; OPS]`, where `pub struct OpLatch { skirt: Skirt, slot: Option<Formant>, key: f32, mix: (f32, f32) }`;
  - `pub fn seed(&mut self, voice: VoiceIdx)`.
- **The per-block Noise block, built in `render`:**
  - `inc_c` comes from `fc_hz(frmt_live, vowel, slot, 0, pitch)`;
  - `c` and `k` come from `noise_coef(bw_hz(bw_live), sr)`;
  - `(alpha, beta)` is `latch.mix`;
  - `gain_to` is `level_gain(live level − VELOCITY's cut) · NOISE_GAIN · noise_edge`, with no `SAMPLE_SCALE`, since `sine` is already unit: so a NOIS operator at LEVEL 99 has RMS `R_REF` × a sine's.
- **The per-sample noise, in `run_noise::<N>` and the `step` arm of `render_samples`:**
  ```
  rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;          // xorshift32 (Marsaglia 2003)
  x = k · (rng as i32 as f32 · 2^−31);  k += dk
  y1 += c·(x − y1);  y2 += c·(y1 − y2);  c += dc
  y = (alpha · y2.clamp(−NOISE_CLAMP, NOISE_CLAMP) + beta) · sine(phase_c + pm_phase) · env · gain
  phase_c += inc_c
  ```
  The noise ignores FEEDBACK and has no grain clock, but its phase still advances by `inc`, as `idle` does. The modulator signal equals the carrier signal. `render_ops` and `render_samples` stay bit-identical to each other. `run_pair` pairs only table operators.
- **Swap:** a kind change ducks one block, as today. `reset` for Noise zeroes `y1` and `y2` and keeps `rng`.

- [ ] **Step 1: Write the failing tests.** The render helper is `common::formant::render_algo(p, note, blocks) -> Vec<f32>` (one `AlgoEngine` at 48 kHz). NOIS parameters are set at stored grid steps: CNTR 104 is 2,016 Hz, and WIDTH 111 is 494 Hz, 0 is 20 Hz and 183 is 3,948 Hz.

```rust
#[test] fn noise_band_centre_and_width() {
    // CNTR 104, WIDTH 111, RES 0, 4 s averaged 8192-point Hann spectrum:
    //   peak within 5 % of 2016 Hz; −3 dB full width within 15 % of 494 Hz; also WIDTH 48 and 183
}
#[test] fn noise_level_is_normalised() {
    // WIDTH ∈ {0, 111, 183} × RES ∈ {0, 50, 100}: RMS within ±0.5 dB of R_REF × a W1 op's at the same LEVEL;
    // RES 100: > 99 % of the energy within ±2 bins of CNTR; every peak ≤ 1.0
}
#[test] fn width_jumps_do_not_burst() {
    // WIDTH stored 183 ↔ 0 every 250 ms for 2 s: the worst 10 ms RMS ≤ steady + 8 dB, peak ≤ 1.0
    // (scaled after the poles it was +27.1 dB: nois.py)
}
#[test] fn repeated_notes_do_not_repeat_their_noise() {}
#[test] fn renders_are_reproducible_from_a_seed() {}
#[test] fn kind_swaps_under_a_held_note_do_not_click() { /* op 1 W1 ↔ NOIS every 100 ms, held C3: common::clicks is empty */ }
#[test] fn the_kernel_matches_the_model_bit_for_bit() { /* algo_kernel_test's model and random ops include Noise */ }
```

  In `param_sweep_test.rs`, add `Patch::Formant` to `patches()` for `Algo` and `AlgoOp` params (quick and thorough), and add `formant_fixture_level`. It prints `Patch::Formant`'s C3 LUFS against `REFERENCE_LUFS` and asserts the peak ≤ 1.0.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_noise_test --test algo_kernel_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above, then call `seed` from `Voice::init_in_place` and from `EngineSlot::rebuild`'s Algo arm.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with every golden unchanged and `voice_is_its_chain_plus_one_slot` passing.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src chimera-core/tests
git commit -m "NOIS: band noise ring-modulated to its centre, scaled before its poles"
```

- [ ] **Step 6: QA gate 3.** Run G1–G6. G5 covers BREATH: listen for a steady band, and check the clicks, the level and the peak.

---

# Phase 4 — FRMT

### Task 11: The FRMT grain train

**Files:**
- Modify: `chimera-core/src/dsp/algo/{kernel.rs,engine.rs}`, `chimera-core/tests/algo_kernel_test.rs`.
- Create: `chimera-core/tests/algo_formant_test.rs`.
- Test: `chimera-core/tests/formant_demo_test.rs` (aliasing).

**Interfaces:**
- Produces:
  ```rust
  pub struct FormantBlock {
      pub inv_inc: f32,                          // 1 / inc as f32
      pub inc_c: u32, pub c0: u32,               // carrier; c0 = −inc_c/(2·r_w) cycles
      pub r_w: (f32, f32), pub inv_d: (f32, f32), pub slack: (f32, f32),   // (from, to), ramped
      pub shift: f32,                            // FC/f0: carrier cycles per period of delay
      pub c_ratio: (f32, f32),                   // g_c / g_m, into the carrier-weight ramp
  }
  #[cfg(any(test, feature = "test-support"))] impl AlgoEngine { pub fn grain_cuts(&self) -> u32; }
  ```
- **In `AlgoEngine::render`, per Formant operator:**
  - it calls `formant_law(f0, fc_hz(frmt_live, vowel, slot, key, pitch), bw_hz(bw_live), latch.skirt, sr)`;
  - `gain_to` is `level gain · g_m · edge`, with no `SAMPLE_SCALE`, since `sine` is already unit;
  - the carrier weight is multiplied by `c_ratio`;
  - `mip_target` and `mip_step` are skipped.
- **Dispatch:** `run_formant::<N, S>` is specialised on `Skirt`, four variants, each squaring straight-line. `render_samples`'s `step` matches on the kind.
- **`Kernel::reset(op, OpKind::Formant)`** sets `phase = u32::MAX`, so the first sample wraps and fires a grain through the normal path. It sets `w = [CLOSED, CLOSED]` with `const CLOSED: f32 = f32::MAX`, and `older = 0`.
- **`idle`** advances the clock and envelope, and sets both lanes to `CLOSED`.
- **The per-sample algorithm.** This task feeds the links' PM into the carrier only, and the delay is the constant `slack/2`. Task 12 makes it bounded PM.
  ```
  let (ph, wrapped) = phase.overflowing_add(inc);        // the unmodulated clock
  phase = ph;
  if wrapped {
      let k = older as usize;                            // the lane that fired less recently
      debug: if (0.0..1.0).contains(&(w[k] − o·inv_d)) { cuts += 1 }
      let e = ph as f32 · inv_inc;                       // samples since the wrap, in [0, 1)
      w[k] = e · r_w;  c[k] = c0 + (e · inc_c as f32) as u32;  older ^= 1;
  }
  let o = slack · 0.5;
  let dc = (o · shift · 2^32) as u32;
  let mut y = 0.0;
  for k in 0..2 {
      let x = w[k] − o · inv_d;
      if (0.0..1.0).contains(&x) { y += window(x, S) · sine(c[k] + pm_phase − dc) }   // S: the const Skirt
      w[k] += r_w;  c[k] += inc_c;
  }
  out = y · env · gain;   acc += carrier · c_ratio · out
  ```
  Here `r_w`, `inv_d`, `slack`, `gain`, `carrier` and `c_ratio` step by their per-sample deltas.

- [ ] **Step 1: Write the failing tests** in `algo_formant_test.rs`. Tests use one FRMT carrier on A1, LEVEL 99, with `common::formant::grain_fc` (zero-crossing intervals inside grains) and `spectrum` (a naive Hann DFT, n = 8192).

```rust
#[test] fn harmonics_are_exact() { /* A3, FRMT 1 kHz, BW 160, S1: partials at k·220 within 0.1 cent; between partials ≥ 60 dB under the peak */ }
#[test] fn formant_peak_stays_put_across_pitch() { /* KEY 0, FRMT 1 kHz (stored 80 → 1000), BW 1 kHz: grain_fc = 1 kHz ± 1 % at C2, C3, C4 */ }
#[test] fn key_100_tracks_the_note() { /* KEY 100 and 50 at C3, C4, C5: grain_fc within 1 % of fc_hz */ }
#[test] fn first_grain_sounds_at_note_on() { /* C1: a sample with |y| > 1e-3 within the first 64 */ }
#[test] fn level_is_normalised() {
    // within ±0.5 dB of R at every SKIRT: C3, FC 1 kHz, BW 524 Hz–2 kHz; C4, BW 1,047 Hz–4 kHz;
    // C5 with FC 270 and 300 Hz (the first draft lost 3–5 dB here)
}
#[test] fn peak_never_exceeds_one() {
    // LEVEL 99 over C1–C7 × BW {20, 160, 2000, 10000} × SKIRT × FRMT {0, 69, 111, 168}: every |sample| ≤ 1.0;
    // where the ceiling binds, level = synth_level's prediction ± 0.5 dB
}
#[test] fn width_floor_keeps_skirt_a_shape() { /* A3 and C5, BW 20 Hz: measured −3 dB width within 10 % of bw_eff, equal across SKIRTs within 10 %; each BW step from 0 widens it */ }
#[test] fn bw_jumps_never_cut_a_grain() { /* BW stored jumping across d = 1 at A3 every 7 blocks (the worst phase is scanned over 7 offsets): grain_cuts() == 0, common::clicks empty */ }
#[test] fn high_formants_do_not_alias() { /* S0, BW 10 kHz, FC 6, 8, 10 kHz at C3: off-grid energy < −60 dB of the total */ }
#[test] fn formant_edge_fades() { /* room at b_min/2 → RMS < 1e-6; at 0.75·b_min → half the level (±0.5 dB of −6.02 dB) */ }
#[test] fn formant_extremes_stay_finite() { /* note 127, COARSE 63, TRNSP +24, FRMT 0/168, BW 0/216, KEY 100, every SKIRT: finite, |y| ≤ 1.0 */ }
#[test] fn retrigger_on_a_sounding_formant_voice_does_not_click() {}
#[test] fn kind_swaps_under_a_held_note_do_not_click() { /* W1 ↔ FRMT ↔ NOIS every 100 ms */ }
```

  `algo_kernel_test`'s model gains Formant, and `the_kernel_matches_the_model_bit_for_bit` covers it. `formant_demo_test` gains `demo_clips_do_not_alias`: CHOIR's held C3 has off-grid energy under −60 dB.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test --test algo_kernel_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with every golden unchanged.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src chimera-core/tests
git commit -m "FRMT: two alternating grain lanes on the unmodulated clock, exact sub-sample starts"
```

### Task 12: PM as a bounded grain delay, the modulator signal, and carrier-only feedback

**Files:**
- Modify: `chimera-core/src/dsp/algo/kernel.rs`.
- Test: `chimera-core/tests/algo_formant_test.rs`.

**Interfaces:**
- **The delay:** `o = (slack·0.5 + pm_links_cycles).clamp(0.0, slack)`, where `pm_links_cycles` is the links' PM only, divided by `PHASE_UNITS`.
- **The carrier:** it reads `c[k] + pm_links_phase + pm_fb_phase − dc`.
- **Feedback:** `pm_fb = feedback·(prev + hist)` reads the operator's own modulator-signal output (`out`, already scaled by `g_m`). It enters the carrier only, never `o`.
- **The two signals.** `out[op]`, which is both the PM source for other operators and the feedback signal, is `y·env·gain` with `g_m`. The carrier sum takes `c_ratio · out`, so a carrier is `y·g_c` and a modulator peaks at 1.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn pm_never_cuts_a_grain() {
    // Full-level ratio-1 and ratio-2 sine modulators into FRMT, SKIRT 0–3, A3:
    // grain_cuts() == 0, common::clicks empty, peak ≤ 1.0.
    // (With starts on the modulated wrap there were 2.97 cuts a period at a PM of 1 cycle: pm.py.)
}
#[test] fn pm_enters_grain_and_carrier() { /* ratio-1 modulator at 1 cycle: sidebands at k·f0 ± f_m around the harmonics, and a formant copy at FC ± f_m, each > −30 dB */ }
#[test] fn bw_does_not_change_modulation_depth() { /* FRMT modulator at fixed LEVEL into a sine carrier: the carrier's modulation index, measured by its sideband ratio, varies < 0.5 dB over BW 0–216; FDBK 5's swing likewise */ }
#[test] fn feedback_moves_the_formant_not_the_pitch() { /* FDBK 0 against 7: f0 within 0.1 cent; energy within ±10 % of 2·FC rises ≥ 6 dB */ }
#[test] fn a_modulated_high_formant_does_not_alias() { /* full-level ratio-1 into FC 5 kHz at C3: off the harmonic and sideband grid < −60 dB */ }
#[test] fn formant_operators_are_stable() { /* every new param at its ends, FDBK 7, five full modulators, 30 s: finite, each carrier ≤ 1.0 before carrier_norm, |DC| < 1e-3, last-second RMS ≤ first-second RMS + 0.5 dB */ }
#[test] fn morph_across_a_formant_carrier_is_smooth() { /* T1 ↔ A1 with FRMT on op 2, MORPH swept 0→127 over 1 s: common::clicks empty; the level is monotone within 0.5 dB steps */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test`. Expected: FAIL. `pm_never_cuts_a_grain` passes already, since starts sit on the unmodulated clock. The PM, depth and feedback tests fail.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/algo/kernel.rs chimera-core/tests/algo_formant_test.rs
git commit -m "PM delays FRMT grains within their slack and never cuts one; feedback drives the carrier"
```

- [ ] **Step 6: QA gate 4.** Run G1–G6. G5 now covers all three clips: clicks, level, peak and aliasing.

---

# Phase 5 — VOWEL, slots and modulation

### Task 13: VOWEL and slots in the engine

**Files:**
- Modify: `chimera-core/src/dsp/algo/engine.rs`.
- Test: `chimera-core/tests/algo_formant_test.rs`.

**Interfaces:**
- **At `note_on`:**
  - `latch[i].slot = resolve_slots(&p.kinds(), vslots)[i]`;
  - `latch[i].key = key as f32` and `latch[i].mix = res_mix(res)`, for every operator;
  - `latch[i].skirt` only when the operator is idle (Ruling 13).
- **Per block:** VOWEL's segment and fraction are computed once per voice. `fc_hz` takes `live.vowel`; Task 14 adds it, and this task uses `p.vowel as f32` until then.
- **Held edits:** a kind or slot edit while a note is held moves nothing until the next note.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn vowel_places_init_on_each_vowel() {
    // INIT, ops 1–3 FRMT, AUTO, KEY 0, BW 0, C2, one operator heard at a time: at VOWEL 0, 25, 50, 75, 100,
    // grain_fc is within 50 cents of that vowel's F1, F2, F3
}
#[test] fn a_slot_edit_while_held_moves_nothing_until_the_next_note() {}
#[test] fn level_holds_across_the_vowels_at_c5() { /* ops alone, VOWEL 0–100 in 25s: within ±0.5 dB of R at every SKIRT */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/algo/engine.rs chimera-core/tests/algo_formant_test.rs
git commit -m "VOWEL moves each slotted formant; slots latch at note-on"
```

### Task 14: VOWEL, FRMT and BW are mod destinations

**Files:**
- Modify:
  - `chimera-core/src/dsp/algo/engine.rs` (`AlgoLive`);
  - `chimera-core/tests/modulatable_test.rs`.
- Test: `chimera-core/tests/algo_formant_test.rs`.

**Interfaces:**
- `AlgoLive` gains `vowel: f32`, `frmt: [f32; OPS]` and `bw: [f32; OPS]`, filled by `from_params`.
- `AlgoLive::offset` gains the arms `(Algo, VOWEL)`, `(AlgoOp(n), FRMT)` and `(AlgoOp(n), BW)`, with ADR 0010's formula unchanged.
- `render` reads the live values everywhere Tasks 10–13 read stored ones.
- **`modulatable_test::recipe`:**
  - for `(AlgoOp(n), FRMT | BW)`, operator *n* is set to FRMT; a second case sets it to NOIS;
  - for `(Algo, VOWEL)`, operators 1–3 are FRMT on A1;
  - the budget always holds.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn vowel_sweeps_do_not_click() {
    // VOWEL 0→100 over 50 ms by an ENV route, and LFO 1 CLASSIC SQUARE 0↔100 at 4 Hz,
    // on FRMT and on NOIS operators: common::clicks empty
}
#[test] fn width_square_lfo_does_not_burst() { /* LFO 1 SQUARE into a NOIS op's BW, 4 kHz ↔ 20 Hz: worst 10 ms RMS ≤ steady + 8 dB, peak ≤ 1.0 */ }
#[test] fn bw_square_lfo_never_cuts_a_grain() { /* across d = 1 at A3: grain_cuts() == 0 */ }
#[test] fn a_route_into_a_table_operators_frmt_does_nothing() { /* bit-identical to no route */ }
```

  `modulatable_test` covers the three new destinations through its existing loop.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test --test modulatable_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/algo/engine.rs chimera-core/tests
git commit -m "VOWEL, FRMT and BW take modulation every block"
```

- [ ] **Step 6: QA gate 5.** Run G1–G6. G2 now sweeps VOWEL, FRMT and BW with fast jumps on `Patch::Formant`.

---

# Phase 6 — Pages

### Task 15: The VOWEL cell, dimming, and the FRMT group page

**Files:**
- Modify:
  - `chimera-core/src/ui/{block_registry.rs,chain.rs,view.rs}`;
  - `chimera-core/tests/{all_pages_walk_test.rs,screen_golden_test.rs,page_identity_test.rs}`.

**Interfaces:**
- `ALGO_ALG.params[5]` = `ParamSlot::param(BlockRef::Algo, AlgoParams::VOWEL)`.
- `pub static ALGO_FORMANT: BlockDef` = `group(<next free id>, "Formant", "FRM", AlgoOpParams::FRMT)`.
- `ALGO_FORMANT_CHAIN` is `ALGO_CHAIN` with `&ALGO_FORMANT` first in the OSC node's sub-pages.
- `PartChain::AlgoFormant`, with `PartChain::of` returning it when `sound.params.algo.kinds().any_formant()`.
- `pub fn algo_chain(p: &AlgoParams) -> PartChain` is the pure pick that `of` uses.
- **New `view::dimmed` arms:**
  - `(Algo, VOWEL)` when no operator is a formant kind;
  - `(AlgoOp(n), FRMT | BW | SKIRT | KEY | RES | VSLOT)` for a table operator;
  - `(AlgoOp(n), SKIRT | KEY)` for NOIS, and `(AlgoOp(n), RES)` for FRMT;
  - `(AlgoOp(n), COARSE | FINE | DETUNE | FEEDBACK)` for NOIS.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn pages_adapt_to_formant_kinds() {
    // INIT: PartChain::Algo, no FRM page, VOWEL cell dimmed.
    // On DETUNE, a Sound load that makes op 1 FRMT: PartChain::AlgoFormant, focus still ALGO_DETUNE.id.
    // FRM is the OSC node's first sub-page; the table ops' FRMT cells are dimmed; NOIS's COARSE, FINE, DETUNE and FDBK are dimmed.
    // Setting the op back to W1 on the FRM page: focus → the OSC home.
}
#[test] fn the_all_pages_walk_includes_frm_exactly_when_a_formant_exists() {}
```

  Re-record the golden `algo_home` for the dimmed VOWEL cell, and add `algo_formant_page`. Every other screen golden must be unchanged.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test page_identity_test --test all_pages_walk_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with exactly one re-recorded golden.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "VOWEL takes ALGO's sixth cell; a FRMT page joins OSC when an operator is a formant"
```

### Task 16: Operator page cells by kind, and the grain and band viz

**Files:**
- Modify:
  - `chimera-core/src/ui/{op_page.rs,block_def.rs,view.rs,viz.rs,block_registry.rs}`.
- Test:
  - `chimera-core/tests/op_page_test.rs`;
  - `chimera-core/tests/screen/` (new goldens `algo_op_page_frmt`, `algo_op_page_nois`).

**Interfaces:**
- Produces:
  ```rust
  pub fn op_page_cells(kind: OpKind) -> [(ParamId, &'static str); 6];
  //   Table: WAVE CRSE FINE LEVEL FB VEL; Formant: WAVE FRMT BW SKIRT KEY SLOT; Noise: WAVE CNTR WIDTH RES SLOT LEVEL
  pub fn grain_curve(s: Skirt, fc_l: f32) -> [i8; 64];     // one grain: window × carrier over one window length
  pub fn band_curve(fc: f32, bw: f32) -> [u8; 64];         // a bell on a 20 Hz–20 kHz log axis
  ```
- `SlotBinding::OpCell(u8)` resolves in `view` through `op_page_cells(ctx.op_kind)`.
- `SlotCtx` gains `op_kind: OpKind`, read from the selected operator's WAVE.
- `ALGO_OP_PAGE`'s cells become `OpCell(0..6)`.
- **The viz:**
  - FRMT draws `grain_curve` and prints FC and BW_eff from `formant_law` at C4 and the stored VOWEL (Ruling 3);
  - NOIS draws `band_curve` and prints its centre;
  - a table operator draws as Task 3 does.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn operator_pages_show_each_kinds_cells() {}
#[test] fn a_sound_load_on_an_operator_page_keeps_it() {
    // on op 2's page (Table) a load makes op 2 FRMT: still op 2's page, cells FRMT's; undo → Table's;
    // an engine switch to Modal closes it
}
#[test] fn grain_curve_is_one_odd_grain() { /* 0 at both ends, antisymmetric about the centre within 1 LSB */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test op_page_test`. Expected: FAIL.
- [ ] **Step 3: Implement**, and record only the two new goldens.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "Operator pages swap in each kind's cells and draw its grain or band"
```

- [ ] **Step 6: QA gate 6.** Run G1–G6. Also run `just desktop` and walk every page of `Patch::Formant`'s Sound, with the operator pages open, while a note holds.

---

# Phase 7 — Costs and the ADR

### Task 17: Recount the terms, add the bench rows, pin the voice counts, and write ADR 0063

**Files:**
- Modify:
  - `chimera-core/src/dsp/algo/engine.rs` (the terms, if the recount moves them);
  - `chimera-stm32/src/bench.rs`;
  - `chimera-core/tests/cost_test.rs`;
  - `chimera-core/tests/memory_budget_test.rs`;
  - `docs/adr/README.md`.
- Create: `docs/adr/0063-formant-operators-are-algo-kinds.md`.

**Interfaces:**
- **The bench rows,** in `PATCHES`:
  - `"FRMT0"`: one FRMT, SKIRT 0, BW 0;
  - `"FRMT3"`: one FRMT, SKIRT 3, BW 0;
  - `"FRMT PM"`: FRMT3 with a full-level ratio-1 modulator;
  - `"NOIS"`: one NOIS;
  - `"FRMT MAX"`: the worst allowed patch, § 9 (A16/A17, 3 FRMT, 1 NOIS, 2 table, feedback where billed);
  - `"CHOIR"`: `demo_sound(Demo::Choir)`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn formant_costs_bill_by_kind() {
    // engine cost on A1 unless noted: BREATH 510; TALK BASS 820; CHOIR 830; 2 FRMT + 4 NOIS 910; the worst allowed 1,051 (A16/A17)
    // voices beside the bus, no routes (voice = cost + 57), rev V / rev Y:
    //   BREATH 8/8, TALK BASS 6/5, CHOIR 6/5, 2F+4N 6/4, worst 5/4
    // worst under HEAVY (1,376): 4 with and without the tape on rev V; 3 on rev Y
}
#[test] fn the_costliest_table_patch_still_gets_six_voices_on_rev_v() {}   // ADR 0031, unchanged
#[test] fn the_algo_engine_still_fits_its_slot() { /* size_of::<AlgoEngine>() ≤ size_of::<ModalEngine>(); Voice's size unchanged */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test cost_test --test memory_budget_test`. Expected: the voice-table test FAILs until it is added. The others pass or fail on real regressions only.
- [ ] **Step 3: Recount from the release disassembly.**

```bash
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench
"$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objdump" -d --no-show-raw-insn -C --mcpu=cortex-m7 \
  target/thumbv7em-none-eabihf/release/chimera-stm32 > target/formant.dis
```

  In `target/formant.dis`, count the instructions in the per-sample loop body of `run_formant<1, S0>`, `run_formant<1, S3>` (the two-lane path, with the second lane open) and `run_noise<1>`, against `run_one<1>`.
  - Each term is `COST_OP + Δinstructions × 1.46 + per-block/64`, plus 10 %, rounded up to 10 (ADR 0056).
  - If a term rises, update the constant. `FORMANT_BUDGET` and N follow.
  - If N falls under 3, **stop**: CHOIR no longer fits, and the owner decides.
  - Write the counts in the ADR.
- [ ] **Step 4: Write ADR 0063**, "Formant operators are Algo operator kinds, after the FS1R patent". Its content follows the spec's § ADR:
  - the provenance split (patent; FSVR prose, including the measured skirt law and its GPL status; Peterson & Barney; ours);
  - `OpKind` and the codes, and VOWEL SLOT;
  - the −12 dB reference, the peak ceiling and the peak-referenced modulator;
  - the SKIRT-free floor;
  - the cost terms, as host estimates until the bench, with the voice-count table and billing by SKIRT;
  - the budget's derivation, N = 3, `Kinds`, and the heavy-route worst case;
  - the clean-peak decision (owner, 2026-09-30);
  - the partial supersessions of ADRs 0022, 0026 and 0031.

  Add its README row.
- [ ] **Step 5: Run the tests to verify they pass.** Run `just check`. Expected: PASS, and the firmware with `bench` builds.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src chimera-core/tests chimera-stm32/src/bench.rs docs/adr/0063-formant-operators-are-algo-kinds.md docs/adr/README.md
git commit -m "Formant cost terms recounted from the release build; bench rows; ADR 0063"
```

- [ ] **Step 7: QA gate 7.** Run G1–G6.

---

# Phase 8 — Factory Sounds, loudness and the UAT list

### Task 18: CHOIR, TALK BASS and BREATH ship in the bank

**Files:**
- Modify:
  - `chimera-core/src/factory.rs`;
  - `chimera-core/tests/{factory_test.rs,cost_test.rs,formant_demo_test.rs,codec_compat_test.rs,param_sweep_test.rs}`.

**Interfaces:**
- `FACTORY_LEN` becomes 11, and `factory_sound(8..=10)` = `demo_sound(Choir | TalkBass | Breath)`.
- `v1_fixtures_equal_factory` and `factory_level_test` iterate over the eight v1 fixtures only (`V1_FACTORY = 8`, Ruling 9).
- Each Sound's `OutParams::VOLUME` is tuned by hand: never LEVEL or `R_REF`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn formant_sounds_meet_the_loudness_reference() {
    // sweep::level(Patch::Factory(i), &[60], 100, BPS).lufs within ±1 dB of REFERENCE_LUFS, i = 8, 9, 10;
    // levels_are_safe covers them (peak ≤ ceiling, chord at 127); chord limited_db ≤ MAX_CHORD_LIMITED_DB
}
#[test] fn eleven_sounds_with_distinct_names() {}                // renamed from eight_…
#[test] fn every_factory_sound_gets_at_least_six_voices_on_rev_v() {}   // with their routes: CHOIR, TALK BASS 6; BREATH 8
#[test] fn formant_sounds_round_trip() { /* encode → decode bits_eq, i = 8..11 */ }
```

  `formant_demo_test::demo_clips_are_clean` gains the same loudness check on each phrase.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test --release -p chimera-core --test factory_test --test param_sweep_test --test formant_demo_test`. Expected: FAIL.
- [ ] **Step 3: Implement.** Tune VOL in whole steps until each Sound is within ±1 dB. If VOL tops out first, stop and ask (Ruling 8).
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`, then QA gates G1–G6. Render the clips (G5) and listen to all three on the desktop. Their clicks, level and aliasing numbers go in the PR.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/factory.rs chimera-core/tests
git commit -m "CHOIR, TALK BASS and BREATH join the factory bank at the loudness reference"
```

- [ ] **Step 6: Hand the owner the UAT list** below, with the three WAVs from `target/demo/`, and open the PR for `formant-ops`.

---

# Phase 9 — Ship

### Task 19: The shared ship flash

- [ ] **Step 1: Flash once.** Build the firmware with `just firmware`, and the bench build with `--features bench`. Flash them together with whatever else ships in this flash.
- [ ] **Step 2: The owner reads the bench screen once:** FRMT0, FRMT3, FRMT PM, NOIS, FRMT MAX, CHOIR and FLOOR.
- [ ] **Step 3: Update the terms.** Solve `COST_FRMT_ONE` from FRMT0, `COST_FRMT` from FRMT3, and `COST_NOIS` from NOIS, each less FLOOR and rounded up to 10. Write "measured <date>, bench, rev V at 480 MHz" beside each.
  - FRMT PM must be ≤ FRMT3 + 10 and FRMT MAX ≤ its model bill; otherwise file an issue and stop.
  - If N changes, `formant_budget_keeps_four_voices` says so. N < 3 stops the ship (CHOIR).
  - Change ADR 0063's "estimate" lines to the measured ones, since the ADR is still Proposed.
- [ ] **Step 4: Run the owner's UAT**, below.
- [ ] **Step 5: Commit** the measured terms.

```bash
git add chimera-core/src/dsp/algo/engine.rs docs/adr/0063-formant-operators-are-algo-kinds.md
git commit -m "Formant cost terms measured on the bench"
```

## Owner UAT (one list, at the ship flash; musical judgments only)

1. **CHOIR:** does it read as a choir pad? Is the O–A drift heard as vowels, and do the doubled F2s give it width?
2. **TALK BASS:** does each note say "wow"? Does the W1 body stay solid, and does the note start without a flam?
3. **BREATH:** airy and breathy, with no tone, and a clean release.
4. **VOWEL by hand on CHOIR:** a smooth U → O → A → E → I sweep, with each vowel recognisable and no stepping.
5. **The formant operators beside the FM factory Sounds:** do they sit at the same loudness?
6. **Operator pages:** is holding B1–B6 for half a second comfortable? Do PLUS and MINUS walk the operators, and does MENU bring you back where you were?
7. **The FRMT page:** it appears as you set a WAVE to FRMT or NOIS, a Sound load leaves you on your page, and "FORMANT BUDGET FULL" is clear when WAVE skips.
