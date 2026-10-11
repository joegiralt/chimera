# Formant Operators Implementation Plan

**Goal:** Any of Algo's six operators can be a voiced formant (FRMT) or a noise band (NOIS), moved together by a VOWEL macro. Every patch you can build keeps ADR 0026's four voices. The operator pages (OP1–OP6 sub-pages) land first.

**Revised 2026-09-30** after the plan review (`scratchpad/formant-plan-review.md`) and the owner's decision on operator pages. The changes:
- OP1–OP6 are OSC sub-pages. There is no hold gesture and no hold ADR.
- The curves reach past 4: ρ is 0 and p̂ is 1 beyond the table.
- Grains are clock-relative and latched per grain.
- The budget is derived at build time. Task 11b optimises FRMT so that 3 fit, and falls back to N = 2 without stopping (owner, 2026-09-30).
- The first grain has no base delay.
- Phase casts are wrapping fixed-point.
- Latching moved to Task 9, and `modulatable` flips in Task 12.
- Every test body is written out.
- Alias and width tests are measurable.
- The golden list is corrected.
- The viz shows live values.
- ADR numbers are placeholders.

**Refreshed 2026-10-11** against main 4473f0a1 (ADR 0108's crate split, ADR 0044's ladder, and Modal 2, ORBIT, projects-core and the param sweep merged). The changes:
- Paths: the DSP, model, storage and factory are in `chimera-engine/src`; the UI is in `chimera-core/src/ui`; the tests are in `chimera-core/tests`, through `chimera_core`'s re-exports.
- Navigation: `ChainNav` is gone. Task 1 works on `NavCtx`, `Recall`, `Location` and `PageAt` (`ui/nav.rs`, `ui/block_def.rs`).
- Costs: the budget is re-derived on today's terms (spec § 1). It is 72 at 70 % and 197 at 75 %, so Task 3 waits for `audio-share-75` and stops for the owner if N < 2.
- Labels follow the full-word rule (spec § 2, § 6).
- Motion (ADR 0112), neutral omission (ADR 0087), the Sound cap and the theme tokens (ADR 0126) are new constraints.
- Counting uses `tools/loop_count.py` and the `bench-algo` build. Claude reads the bench over the USB console.
- Flash is the tight resource: 30 kernel variants are capped at 10 KB (Task 10).
- The session scratchpad's `fprobe/` scripts are gone. The tests carry their numbers as literals, and `synth_level` is written in Rust from spec § 3. The durable ledger is (private notes).

**Architecture:**
- **Functional core:** pure modules in `dsp/algo/`:
  - `kinds.rs` holds `OpKind` and `Kinds`;
  - `formant.rs` holds the window, SKIRT constants, curves, width floor, gains and edge law;
  - `vowel.rs` holds the vowel table, `fc_hz` and slots;
  - `noise.rs` holds the band, gain and seed laws.

  All four are in `chimera-engine/src/dsp/algo/`. `chimera_engine::budget` (`chimera-engine/src/budget.rs`) holds the budget derived from the cost model. Tests call each law directly.
- **Imperative shell:** `AlgoEngine` builds one kind block per operator per block from those laws. `Kernel` runs a `KindState` per operator and dispatches once per block: `run_one` (table, unchanged), `run_formant::<N, S>` or `run_noise::<N>`.
- **UI:** the Part's chain is picked per Sound (`PartChain` in `NavCtx`), and page focus is kept by `BlockDef.id` (`refocus`). OP1–OP6 are OSC sub-pages whose cells follow each operator's kind (`SlotBinding::OpPanel`, as `ModalPanel`).

**Tech Stack:** Rust 2024, `no_std` `chimera-engine` (f32 DSP, `libm`, built opt 2) and `chimera-core` (UI, opt z), `thumbv7em-none-eabihf`, `just`. No new dependency.

**Spec:** `docs/superpowers/specs/2026-09-30-formant-operators-design.md`. It was owner-approved at e67b2a1, with the 2026-09-30 amendments this revision makes:
- § 6 operator pages;
- § 3 per-grain BW, curves and the first grain;
- § 1 the budget.

It was refreshed 2026-10-11 against main 4473f0a1.

**Where it runs:**
- **Phase 0:** branch `algo-op-pages`, off `main` (its prerequisites have all merged).
- **Phases 1–9:** branch `formant-ops` (worktree (private notes)), rebased onto `main` after Phase 0 and `audio-share-75` merge. Commands run from the worktree root.

## Global Constraints

- **The table path doesn't move.** A `Table` operator runs today's code. Every Algo golden, factory render hash and `codec_compat` hash stays bit-identical.
- **Goldens re-recorded, and only these:**
  - **Task 2:** the goldens that draw the OSC node's sub-page branch or map, because OP1–OP6 are added. The step lists them by name from the failing run, and every other golden stays.
  - **Task 5:** the pool and library goldens that draw `MAX_SOUND_COST` (`sound_browser_pool_full`, `library_sounds`, `library_this_build`, `library_newer` and `library_pool_full`, as VA's raise moved them). The step names the ones that fail.
  - **Task 13:** `engine_algo`, `algo_alg`, `algo_alg_morph_dimmed` and `algo_out_p3` (the dimmed VOWEL cell).
  - Light goldens (`light_golden_test`) follow the dark ones, and every new screen passes `every_screen_colour_is_a_theme_token_or_a_grey` (ADR 0126).
- **No GPL.** No FSVR code, tables or constants (ADR 0032). The sources are FSVR's prose docs, US5610354 and Peterson & Barney (1952). `THIRD_PARTY.md` doesn't change.
- **Frozen codes (ADR 0045):**
  - WAVE 16 `FRMT` and 17 `NOIS`;
  - operator ids 13 `FRMT`, 14 `BW`, 15 `SKIRT`, 16 `KEY`, 17 `RES` and 18 `VSLOT`;
  - VSLOT 0–4: `AUTO`, `F1`, `F2`, `F3` and `OFF`;
  - Algo id 4 `VOWEL`.

  `disk_codes_v1.txt` is append-only. INIT's new values are part of the decode base, `Sound::neutral`, a format contract (ADR 0087), so they are frozen. `neutral_v1.txt` gains them in the same commit, with no existing value changed.
- **The spec's values:**
  - FRMT `100·2^(v/24)` Hz (0–168, default 72); BW `20·2^(v/24)` Hz (0–216, default 72);
  - SKIRT 0–3 (default 1), with `q = 2^(SKIRT+1)`;
  - KEY and RES 0–100 (default 0); VSLOT 0–4 (default AUTO); VOWEL 0–100 (default 50, `x = v/25`);
  - INIT's FRMT `[69, 83, 111, 123, 123, 123]`;
  - `R` = −12 dB (0.25119) of a full-scale sine's RMS, and `g_n` = 0.251;
  - KEY_REF is MIDI 60, and FC's floor is 20 Hz;
  - the labels FORMANT, CENTRE, WIDTH, SKIRT, KEY, RES, SLOT and VOWEL, and the WAVE names FRMT and NOISE (full words where they fit 62 px, spec § 2);
  - the motions: FRMT, BW and VOWEL `Continuous(Smoothing::KNOB)`, KEY and RES `Continuous(Smoothing::NoteOn)`, and SKIRT and VSLOT `Stepped(Change::NoteOn)` (ADR 0112).
- **The budget.** The rule is `charge(FRMT)·n_FRMT + charge(NOIS)·n_NOIS ≤ FORMANT_BUDGET`, where each charge is its cost minus `COST_OP`.
  - `FORMANT_BUDGET` is derived at build time from the live cost constants, and never typed in; `FORMANT_N` = ⌊budget/charge(FRMT)⌋.
  - The cost estimates are `COST_FRMT_ONE` 90, `COST_FRMT` 130 and `COST_NOIS` 70.
  - The heavy-route overhead is 472: `ModRouting` 326, the filter's `CLEAN_MOVE_COST` 75 and `FILTER_LEVEL_COST` 35, and `Voice::CHAIN_COST` 36.
  - The budget is 72 at main's 70 %, which gives N = 1 at either term. At `audio-share-75`'s 75 % it is 197. The estimate `COST_FRMT` 130 gives N = 2 there until Task 11b.
  - **Task 11b's target** is a SKIRT 1–3 FRMT term ≤ 120 (charge ≤ 60, so 3·60 + 10 for CHOIR's NOIS = 190 ≤ 197), counted by ADR 0056's method.
  - **Outcomes (owner, 2026-09-30):** if the target is met, N = 3; if not, N = 2 with the two-formant voicings. The plan continues either way, and ADR F records which.
  - **N < 2 stops the plan for the owner** (refresh 2026-10-11). That is what main's 70 % gives.
- **Audio thread:**
  - no heap, no blocking, no libc, and no per-sample table read beyond W1's;
  - no new `unsafe`;
  - every per-block value that moves is ramped, and every phase is continuous;
  - no `f32 → u32` cast in a phase path unless its range is proven in `[0, 2^32)`. Otherwise use `cycles_to_phase`, which is wrapping fixed-point.
- **Functional core, imperative shell.** Every law in the spec's § Principles is a `pub fn` with unit tests. The engine and kernel only call them.
- **Type-driven.** `OpKind`, `Kinds`, `Skirt`, `VowelSlot`, `Formant` and `PartChain` are enums or newtypes. A kind is read only through `OpKind`.
- **No MIDI CC source** (#247).
- **No hardware** before the one shared ship flash (Task 17).
- **Green per task.** `just check` passes before every commit: the crates with and without `master-tape`, desktop, every firmware build and bench set, clippy and `just stack-check`.
- **Flash.** About 58 KB is free on main at 4473f0a1. Report each task's delta in a line. If free flash falls under 40 KB, stop for the owner.
- **Theme.** Every new pixel is a `ui::theme` token (ADR 0126).
- **Commits:**
  - terse and plain, with no type prefix;
  - comments only where genuinely needed;
  - stage named paths only, never `docs/chimera-ui-ux-spec.md` or `chimera.bin`;
  - follow-ups go in GitHub issues, referenced by URL.
- **ADRs:** `0000-template.md`, `Proposed`, with a README row; never edit an accepted ADR.
  - On 2026-10-11 the highest number on any branch is 0127 (`screensaver`); 0123 is `sound-sends`' and 0124 is `audio-share-75`'s. So this plan's are **ADR P** (Phase 0, expected 0128) and **ADR F** (formant, expected 0129).
  - Before writing each, list `docs/adr/` on `main` and on every open branch (`git for-each-ref refs/heads` then `git ls-tree`), take the next free number, and use it in the file name and README.

## Review Focus

1. **A kind change under a held note, including a fast WAVE spin.** Expect a duck and swap, no click, and the new kind starting closed. Test: `kind_swaps_under_a_held_note_do_not_click` (Tasks 9 and 10).
2. **MORPH carrying a FRMT operator between modulator and carrier.** The level glides. Test: `morph_across_a_formant_carrier_is_smooth` (Task 11).
3. **Extreme pitch and ratio:** note 127, COARSE 63, TRNSP +24, f0 above FC, B_min past the Nyquist room. Expect finite and bounded output. Test: `formant_extremes_stay_finite` (Task 10).
4. **Retrigger or steal on a sounding formant voice.** No click and no cut. Test: `retrigger_on_a_sounding_formant_voice_does_not_click` (Task 10).
5. **A WAVE edit, project load or engine switch while an OP page or the FORMANT page is open.** Focus is kept by identity, and the OP page's cells re-resolve. There is no undo on main. Test: `a_kind_change_on_an_op_page_keeps_it` (Task 14).

## Rulings

1. **Operator pages (owner, 2026-09-30).** OP1–OP6 are OSC sub-pages after the group pages, each bound to its operator. EDIT and SEQ step through them, and there is no new gesture: B*n* stays Part *n*. The spec's § 6 is amended to match.
   - Today's twelve group pages stay unchanged: CRS, FIN, DET, LVL, VEL, AR, D1R, D1L, D2R, RR, RS and FBK. OP1–OP6 follow them.
   - There is no envelope-page merge. This is the controller's default, which the owner may revisit (2026-09-30).
2. **Phase 0 owns the dynamic sub-row and focus by `BlockDef.id`**, plus OP1–OP6 and ADR P, which records operator pages as sub-pages and focus by identity.
   - #37's hold, envelope playhead dot, gang edit, LEVEL SCALE and living diagram stay in #37.
   - Phase 0's hardware UAT joins the single end list at the shared ship flash.
3. **`(AlgoOp(n), WAVE)` goes through `AlgoParams`.** `BlockRead::block` and `Blocks::block_mut` (`chimera-engine/src/addr.rs`, split since 2026-09-30) return guards (`BlockView`, `BlockViewMut`).
   - Their `Op` variants read and write WAVE through `Kinds`, and `AlgoParams::kinds` is private.
   - 19 impls change: `ParamSnapshot`, `PartEdit` and `PartRead`, the project codec's `FxBlocks` and `MixBlocks`, ORBIT's `Scene`, `SystemBlocks`, the codec's test `Order`, and the UI's.
4. **Naming in the kernel.** The spec's kernel `OpState` enum is named `KindState`, since kernel.rs has an `OpState`.
   - Noise generators live in `Kernel::rng: [u32; OPS]`. `Kernel::new` seeds them with `noise_seed(0, op)`, and `AlgoEngine::seed(VoiceIdx)` reseeds them per voice. `EngineSlot::build`'s Algo arm calls it, as its VA arm calls `place` and its TAPE arm `seed`.
   - A NOIS centre runs on the operator's own `phase` with `inc` = the centre's increment, since COARSE is dimmed for NOIS.
5. **Grains follow the controller's C2 ruling:** clock-relative and latched per grain, with no `w` accumulator and no per-sample `r_w`, `1/D` or slack ramps.
   - The reset grain's base delay is 0 (I2).
6. **The budget** lives in `chimera-engine/src/budget.rs`. It uses `Kinds::TABLE` for its costliest-table const, so there is no const cycle.
   - The demo recipes are chosen at compile time by `FORMANT_N`: the spec's voicings at N ≥ 3, and the two-FRMT voicings below at N = 2.
   - Nothing stops on N ≥ 2. Task 11b sets the terms, and N follows from them at build time: 3 if its target is met, else 2. N < 2 (main's 70 % share) stops for the owner.
7. **SKIRT latches when the operator is idle** at note-on, so a retrigger never changes an open grain's exponent.
   - Billing stays by the stored SKIRT. `AlgoEngine::held_skirt_extra(&self, p) -> Cost` bills `COST_FRMT − COST_FRMT_ONE` for each operator latched at SKIRT ≥ 1 whose stored SKIRT is 0. It is added in `Voice::held_model_extra` (`chimera-engine/src/dsp/voice.rs`, beside `held_ops_extra` and `held_level_extra`), as #183 does for Modal.
8. **The viz shows live values.** The OP page viz shows the live FC and BW_eff of the edited Part's most recently started voice. They are published once per block, audio to UI, in a `TripleBuffer<FormantView>`, as `OrbitView` and the scope are. With no voice sounding it shows the stored parameters at C4 (spec § 6).
9. **The loudness reference** is `param_sweep_test`'s `REFERENCE_LUFS`: −11.0, ADR 0063's median at ADR 0109's gain. Each demo Sound's VOL is tuned to within ±1 dB of it. If VOL can't reach it with the peak ≤ 1.0, stop for the owner.
10. **The v1 fixture tests** stay on the existing fixtures (`codec_compat_test`'s `sources()`: the eight factory Sounds and the library fixture).
11. **The sweep's formant fixture** plays its single note at C3 (48). It joins the sweep in Task 9, when its operators sound.
12. **Labels.** Cell labels are full words when `draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING) ≤ theme::CELL_BAR_W` (62 px). The OP pages' Table row is WAVE, COARSE, FINE, LEVEL, FDBK and VELOCITY; FEEDBACK is 65 px, so it stays FDBK.

## Rebase points

Re-read each before the task that touches it. The ones below are open on 2026-10-11.

- **`audio-share-75`** (ADR 0124): `hw::AUDIO_BUDGET_PERCENT` 70 to 75. Task 3 derives the budget after it lands.
- **`sound-sends`** (ADR 0123) touches the same files as Tasks 4 and 5:
  - `addr.rs`, `params.rs`, `storage/sound.rs` (`MAX_SOUND_COST` 2,352 to 2,368) and `storage/codes.rs`;
  - `disk_codes_v1.txt`, `neutral_v1.txt` and `common/sweep.rs`;
  - `param_declarations_test`, `sound_codec_test` and `chimera-stm32/src/bench.rs`.

  Rebase onto it, and recompute `sparse_bound`.
- **`master-vol-tune`** (#438) moves master VOL and PAN after the limiter and adds SYSTEM TUNING to every engine's pitch (`EngineSlot::set_pitch(p, tune)`).
  - KEY then follows TUNING, as it should.
  - Re-check `REFERENCE_LUFS` before tuning the demo VOLs.
- **`va-glyphs`**: `FocusGlyph` and `ui/glyph.rs` (WAVE's gauge, Task 13).
- **`screensaver`** (ADR 0127): an audio-to-UI reader beside the OP page's live view, and an AXI line in `memory_budget_test`.
- **`browser-init-top`**: `ui/browser.rs` and `ui/mod.rs` (Task 1).
- **Costs:** `Voice::cost` is engine + `CHAIN_COST` 36 + `filter_cost` + `ModRouting::cost` + `stage_cost` + `tap_cost`. `FxBus::COST` is 1,204, or 1,494 with the tape.
- **`tests/common/sweep.rs`:**
  - `Patch` (AlgoInit, ModalInit, BusyAlgo, BusyModal, Factory, VaInit, Probe);
  - `play(bench, p, mv, t)` with the bench's note 60;
  - `level(patch, notes, vel, blocks)`, `lufs`, `within_ceiling`, `Tape::click` and `CHORD`.

  `param_sweep_test` holds `silences`, `patches` and `REFERENCE_LUFS`.
- **Storage:**
  - `disk_codes_v1.txt` and `neutral_v1.txt` appends: ours go last;
  - `decode_block(payload, migrations, translations, target) -> Result<u16, FileError>`;
  - `FACTORY_LEN` (8), `factory.bin` and the `authored` Sounds.
- **`BlockDef.id`:** take the next free ids. The id-uniqueness test decides.

## QA gate (every phase ends with it; note any item that doesn't apply yet)

- **G1:** `just check`.
- **G2:** `cargo test --release -p chimera-core --test param_sweep_test -- --include-ignored`. New registry params are swept automatically. `Patch::Formant` is swept from Task 9. No new defect is allowed.
- **G3:**

  ```
  cargo test --release -p chimera-core --test param_sweep_test -- modal_models_match_the_loudness_reference levels_are_safe formant_fixture_level
  ```

  ALGO INIT must stay at `REFERENCE_LUFS` (−11.0) ± 0.1 LUFS. `formant_fixture_level` exists from Task 9, and pins its recorded LUFS ±1.5 dB from Task 10.
- **G4:** `cargo test -p chimera-core --test cost_test formant_budget_keeps_four_voices`, and again with `--features master-tape`.
- **G5:** `cargo test --release -p chimera-core --test formant_demo_test`, then `-- --ignored render_formant_demo_clips`. Listen to `target/demo/*.wav`. BREATH is checked from Task 9, and CHOIR and TALK BASS from Task 10.
- **G6:** `cargo test -p chimera-core --test golden_test --test screen_golden_test --test light_golden_test --test codec_compat_test --test sound_codec_test`. No diff but the listed re-records.
- **G7:** the flash delta in one line (`llvm-size` of `just firmware`'s ELF against main's), and free flash ≥ 40 KB.

A defect a gate finds is fixed before the next phase, or filed as an issue and named in the PR if the owner defers it.

## File structure

| File | Responsibility |
|---|---|
| `chimera-core/src/ui/nav.rs` | `PartChain` in `NavCtx`, `Recall` by page id (Tasks 1, 13) |
| `chimera-core/src/ui/block_def.rs` | `ChainDef2::locate`, `refocus`, `SlotBinding::OpPanel` (Tasks 1, 14) |
| `chimera-core/src/ui/mod.rs` | `nav_cx`, `project_replaced` through `refocus`, the WAVE toast (Tasks 1, 4) |
| `chimera-core/src/ui/block_registry.rs` | `ALGO_OP_PAGES`, `ALGO_FORMANT`, `ALGO_FORMANT_CHAIN`, VOWEL on `ALGO_ALG` (Tasks 2, 13) |
| `chimera-core/src/ui/op_page.rs` (new) | `page_cells`, `grain_curve`, `band_curve` (Tasks 2, 14) |
| `chimera-core/src/ui/view.rs` | `SlotCtx.op_kinds`, the `OpPanel` arm, `dimmed` arms (Tasks 13–14) |
| `chimera-core/src/ui/glyph.rs` | WAVE's gauge by kind (Task 13) |
| `chimera-core/src/ui/fmt.rs` | `LogHz` and `Vowel` display (Task 5) |
| `chimera-engine/src/addr.rs` | `BlockView`, `BlockViewMut`, `BlockRead`, `Blocks` (Task 4) |
| `chimera-engine/src/block.rs` | `ValFmt::LogHz`, `ValFmt::Vowel` (Task 5) |
| `chimera-engine/src/budget.rs` (new) | `FORMANT_BUDGET`, `FORMANT_N`, `COSTLIEST_TABLE`, `HEAVY_OVERHEAD` (Task 3) |
| `chimera-engine/src/dsp/algo/kinds.rs` (new) | `OpKind`, tables, `Kinds`, `OverBudget`, `spend`, `fits`, `step_kind`, `reads` (Tasks 3–4, 13) |
| `chimera-engine/src/dsp/algo/params.rs` | new fields, INIT, `OpView`, `OpViewMut`, `try_set_kind` (Tasks 4–5) |
| `chimera-engine/src/dsp/algo/formant.rs` (new) | `Skirt`, `window`, constants, curves, `formant_law`, `cycles_to_phase` (Tasks 6–7) |
| `chimera-engine/src/dsp/algo/vowel.rs` (new) | `VowelSlot`, `Formant`, vowel table, `fc_hz`, `resolve_slots` (Tasks 5, 8) |
| `chimera-engine/src/dsp/algo/noise.rs` (new) | noise laws (Task 7) |
| `chimera-engine/src/dsp/algo/kernel.rs` | `KindBlock`, `KindState`, `run_formant`, `run_noise` (Tasks 9–11) |
| `chimera-engine/src/dsp/algo/engine.rs` | kinds, latches, kind blocks, cost by kind, `AlgoLive`, `formant_view` (Tasks 3, 9–12, 14) |
| `chimera-engine/src/storage/sound.rs` | `MAX_SOUND_COST` (Task 5) |
| `chimera-engine/src/factory.rs`, `factory.bin` | `Demo`, `demo_sound` in `authored`, the blob (Tasks 5, 16) |
| `chimera-core/tests/common/formant.rs` (new) | shared test helpers (Task 6) |
| `chimera-stm32/src/bench.rs` | FRMT0, FRMT3, FRMT PM, NOIS, FRMT MAX, CHOIR in the `bench-algo` area (Task 15) |

---

# Phase 0 — OP pages and page identity (branch `algo-op-pages`)

### Task 1: Page focus by identity; the Part's chain is picked per Sound

**Files:**
- Modify:
  - `chimera-core/src/ui/{block_def.rs,nav.rs,mod.rs}`;
  - `chimera-core/tests/nav_test.rs`.
- Create: `chimera-core/tests/page_identity_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  // block_def.rs (PageAt's fields are private: only ChainDef2 builds one)
  impl ChainDef2 { pub fn locate(&self, id: u16) -> Option<PageAt>; }
  pub fn refocus(from: &ChainDef2, at: PageAt, to: &ChainDef2) -> PageAt;
  // nav.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum PartChain { Algo, Modal, Tape, Va }   // Task 13 adds AlgoFormant
  impl PartChain {
      pub fn of(sound: &Sound) -> PartChain;
      pub fn base(engine: EngineType) -> PartChain;
      pub fn def(self) -> &'static ChainDef2;
      pub fn engine(self) -> EngineType;
  }
  pub struct NavCtx { pub chains: [PartChain; MAX_PARTS], /* rev, dyn_rows, orbit as today */ }
  ```
- `refocus` keeps the page with the same id. If the page is gone it goes to `page(node, 0)`, and if the node is gone to `to.home()`.
- `NavCtx.engines` becomes `chains`. `NavCtx::engine(p)` is `chains[p].engine()`.
- Every `chain_def_for(cx.engine(p))` in `nav.rs` reads `cx.chain(p).def()`: `next`, `page`, `Recall::pages_of` and `Location::part_home`'s callers.
- `chain_def_for(engine)` stays, as `PartChain::base(engine).def()`, for `browser.rs`, `mod_grid.rs` and the tests. No test changes its call.
- `Recall`'s `SoundPage { engine, at }` becomes `{ chain: PartChain, at }`. `pages_of` keeps the page on the same engine through `refocus(old.def(), at, new.def())`, and goes home on an engine change, as today.
- `nav_cx(project)` fills `chains` with `PartChain::of(&project.part(p).sound)`.
- `UiState::project_replaced` maps a Part page through `refocus` when the chain changed but the engine didn't.
  - After an encoder edit, the shown page is mapped the same way, so a WAVE edit that changes the list keeps the page.

- [ ] **Step 1: Write the failing tests.** Build three test chains from `block_registry`'s defs:
  - `OLD`: home `ALGO_WAVE`, subs `[COARSE, FINE, DETUNE]`;
  - `NEW`: home `ALGO_WAVE`, subs `[LEVEL, COARSE, FINE, DETUNE]`;
  - `SHORT`: one node.

```rust
#[test]
fn refocus_keeps_the_page_by_identity() {
    let at = OLD.locate(ALGO_DETUNE.id).unwrap();            // node 1, sub 3
    assert_eq!(NEW.def_at(refocus(&OLD, at, &NEW)).unwrap().id, ALGO_DETUNE.id);
    let lvl = NEW.locate(ALGO_LEVEL.id).unwrap();
    assert_eq!(refocus(&NEW, lvl, &OLD), OLD.page(1, 0).unwrap());
    assert_eq!(refocus(&OLD, at, &SHORT), SHORT.home());
    assert_eq!(OLD.locate(ALGO_LEVEL.id), None);
}
#[test]
fn an_engine_change_still_goes_home() {
    // Recall left on ALGO's DETUNE; the Part becomes MODAL: its pages open at MODAL's home.
}
#[test]
fn the_mixer_round_trip_returns_by_identity() {
    // On Part 1's DETUNE: NavKey::Part(1) to its mixer, the Part's chain changes meanwhile
    // (NEW's list), NavKey::Part(1) back: the page shown is DETUNE.
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test page_identity_test`. Expected: FAIL, because `refocus` and `PartChain` aren't defined.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with every screen golden unchanged (`nav_test`, `dpad_nav_test` and `part_button_test` unchanged).
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests/page_identity_test.rs chimera-core/tests/nav_test.rs
git commit -m "Page focus is kept by page id; the Part's chain is picked per Sound"
```

### Task 2: OP1–OP6 are OSC sub-pages (ADR P)

**Files:**
- Modify:
  - `chimera-core/src/ui/{block_registry.rs,block_def.rs,view.rs,viz.rs,renderer.rs}`;
  - `chimera-core/tests/{all_pages_walk_test.rs,screen_golden_test.rs}`;
  - `docs/adr/README.md`.
- Create:
  - `chimera-core/src/ui/op_page.rs`;
  - `chimera-core/tests/op_page_test.rs`;
  - `docs/adr/<P>-operator-pages-are-osc-sub-pages.md`.

**Interfaces:**
- `pub static ALGO_OP_PAGES: [BlockDef; 6]`, with the next six free ids.
  - Names are "Operator 1" … "Operator 6", shorts "OP1" … "OP6", and `viz: VizType::OpPage(Op)`.
  - The cells are `ParamSlot::param(BlockRef::AlgoOp(op), id).with_label(..)` for WAVE, COARSE, FINE, LEVEL, FEEDBACK and VELOCITY, labelled WAVE, COARSE, FINE, LEVEL, FDBK and VELOCITY (Ruling 12). Task 14 turns them into `SlotBinding::OpPanel(op, k)`.
- `ALGO_OSC_SUB_PAGES` (`[&BlockDef; 12]`) becomes the twelve group pages, then the six OP pages (`[&BlockDef; 18]`).
- The viz band draws the wave with `glyph::wave_trace` (the WAVE glyph's, ADR 0070) and the envelope with `viz::op_env` (the `VizType::OpEnv` band's). Both exist on main, so there is no new curve code. There is no playhead (#37).
- Every colour is a `ui::theme` token (ADR 0126).

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn osc_sub_row_ends_with_the_six_operator_pages() {
    let osc = ALGO_CHAIN.block_at(1).unwrap();
    let shorts: Vec<_> = osc.sub_pages.iter().map(|d| d.short).collect();
    assert_eq!(&shorts[shorts.len() - 6..], ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"]);
    assert_eq!(&shorts[..12], ["CRS", "FIN", "DET", "LVL", "VEL", "AR", "D1R", "D1L", "D2R", "RR", "RS", "FBK"]);
}
#[test]
fn op_page_cells_edit_their_operator() {
    let ids = [AlgoOpParams::WAVE, AlgoOpParams::COARSE, AlgoOpParams::FINE, AlgoOpParams::LEVEL, AlgoOpParams::FEEDBACK, AlgoOpParams::VELOCITY];
    for (n, op) in Op::ALL.into_iter().enumerate() {
        for (k, id) in ids.into_iter().enumerate() {
            let mut s = Sound::init(EngineType::Algo);
            let before = s.params.clone();
            part_page::apply_encoder(&ALGO_OP_PAGES[n], k, 1, &mut s.params, Op::A);   // signature as on main
            for m in Op::ALL {
                let moved = s.params.block(BlockRef::AlgoOp(m)).unwrap().get(id)
                    != before.block(BlockRef::AlgoOp(m)).unwrap().get(id);
                assert_eq!(moved, m == op, "OP{} cell {k}", n + 1);
            }
        }
    }
}
#[test]
fn edit_and_seq_walk_into_and_out_of_the_op_pages() {
    // on OSC (sub 0), EDIT 13 times lands on OP1 (sub 13, after the 12 group pages); EDIT 5 more → OP6;
    // EDIT again wraps to OSC (ADR 0081); from OP6, SEQ 6 → FBK
}
```

  `all_pages_walk_test` gains six pages.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test op_page_test --test all_pages_walk_test`. Expected: FAIL.
- [ ] **Step 3: Implement**, then run `cargo test -p chimera-core --test screen_golden_test --test light_golden_test`. Re-record only the goldens that draw the OSC branch or the map (`algo_wave`, `algo_osc_last` and the like), and put their names in the commit body. Add the goldens `algo_op_page_3` (INIT, OP3) and `algo_op_page_3_held` (held note).
- [ ] **Step 4: Write ADR P**, "Operator pages are OSC sub-pages; page focus is kept by identity". It records:
  - the owner's decision (no gesture; B*n* stays Part *n*);
  - OP1–OP6 after the group pages;
  - focus by `BlockDef.id` across dynamic sub-rows;
  - that #37's hold is dropped.

  Add the README row.
- [ ] **Step 5: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests docs/adr/README.md docs/adr/<P>-operator-pages-are-osc-sub-pages.md
git commit -m "OP1-OP6 are sub-pages of OSC: WAVE, COARSE, FINE, LEVEL, FB and VEL beside the wave and envelope"
```

- [ ] **Step 7: QA gate 0.** Run G1, G2, G3, G4 (the existing floor tests), G6 and G7. Open the PR for `algo-op-pages`.

---

# Phase 1 — Kinds, budget, parameters (branch `formant-ops`, rebased on `main` after Phase 0 and `audio-share-75`)

### Task 3: `OpKind`, cost terms by kind, and the budget derived at build time

**Files:**
- Create:
  - `chimera-engine/src/dsp/algo/kinds.rs`;
  - `chimera-engine/src/budget.rs`;
  - `chimera-core/tests/kinds_test.rs`.
- Modify:
  - `chimera-engine/src/{lib.rs,hw.rs,modulation.rs}`;
  - `chimera-engine/src/dsp/{fx_bus.rs,algo/mod.rs,algo/engine.rs}`;
  - `chimera-core/tests/cost_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  // kinds.rs
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum OpKind { Table(WaveId), Formant, Noise }
  pub const OP_KIND_COUNT: usize = 18;
  pub static OP_KIND_NAMES: [&str; 18];   // WAVE_NAMES, "FRMT", "NOIS"
  pub static OP_KIND_IDENTS: [&str; 18];  // WAVE_IDENTS, "FRMT", "NOIS"
  impl OpKind {
      pub const fn from_code(c: u8) -> Option<OpKind>;
      pub const fn clamped(c: u8) -> OpKind;   // from_code(min(c, 17))
      pub const fn code(self) -> u8;
      pub const fn is_formant(self) -> bool;
      pub const fn charge(self) -> u32;        // Table 0; Formant COST_FRMT − COST_OP; Noise COST_NOIS − COST_OP
  }
  pub const fn spend(k: &[OpKind; OPS]) -> u32;
  pub const fn fits(k: &[OpKind; OPS]) -> bool;   // spend ≤ budget::FORMANT_BUDGET.0
  // budget.rs
  pub const COSTLIEST_TABLE: AlgoParams;   // A16/A17, Kinds::TABLE, LEVEL 99, FDBK 1 on all six: 832
  pub const HEAVY_OVERHEAD: Cost;          // ModRouting::HEAVY + CLEAN_MOVE_COST + FILTER_LEVEL_COST + Voice::CHAIN_COST: 472
  pub const FORMANT_BUDGET: Cost;          // (for_cpu(CPU_HZ_REV_V) − FxBus::COST_TAPE)/FLOOR_VOICES
                                           //   − HEAVY_OVERHEAD − AlgoEngine::cost(&COSTLIEST_TABLE, &[false; OPS])
                                           //   72 at 70 %, 197 at 75 %
  pub const FORMANT_N: u32;                // FORMANT_BUDGET.0 / OpKind::Formant.charge()
  ```
- The cost constants:
  - `AlgoEngine::{COST_FRMT_ONE: Cost(90), COST_FRMT: Cost(130), COST_NOIS: Cost(70)}`, each commented "estimate, ADR 0056's method; bench at ship";
  - `ModRouting::HEAVY` = BASE + CLAMP + ENV_B + BURST + LEVEL + SLIDE + DEST_FIRST + 17·DEST + 3·FUNC = 326 (`chimera-engine/src/modulation.rs`). CURVE is not in it: `env_slot_cost` bills CURVE or BURST, never both. The 17 are the 16 registry columns plus Q after FREQ's DEST_FIRST.
  - The filter's terms come from `chimera-engine/src/dsp/filter/mod.rs`: `CLEAN_MOVE_COST` (75, a FREQ or Q route on the SVF) and `FILTER_LEVEL_COST` (35, LEVEL routed);
  - `FxBus::COST_TAPE` (1,494) and `FxBus::COST_NO_TAPE` (1,204) hold the two literals already on `main`, and `COST` cfg-selects one;
  - `hw::FLOOR_VOICES: u32 = 4`.
- `AlgoEngine::cost` prices each active operator by `OpKind::clamped(p.ops[i].wave)`.
  - `COST_FEEDBACK` is billed for Table and Formant only.
  - WAVE's spec keeps max 15 in this task.

- [ ] **Step 0: Compute the budget on the rebased `main`.** Add `budget.rs`, and print `FORMANT_BUDGET` and `FORMANT_N` from a scratch test.
  - With `audio-share-75` merged it reads 197, and N = 2 at the estimate of 130.
  - On 70 % it reads 72, and N = 1. **Stop for the owner** (refresh 2026-10-11): the owner's fallback is N = 2, and it doesn't hold there.
  - At N ≥ 2, don't stop. Every test and recipe is N-generic, and Task 11b settles N (owner, 2026-09-30).
  - Put the printed values in the commit body.
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
fn the_budget_is_derived_from_the_cost_model() {
    let share = (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - FxBus::COST_TAPE.0) / FLOOR_VOICES;
    let table = AlgoEngine::COST_BASE.0 + 12 * AlgoEngine::COST_LINK.0 + 6 * (AlgoEngine::COST_OP.0 + AlgoEngine::COST_FEEDBACK.0);
    let heavy = ModRouting::HEAVY.0 + CLEAN_MOVE_COST.0 + FILTER_LEVEL_COST.0 + Voice::CHAIN_COST.0;
    assert_eq!(HEAVY_OVERHEAD.0, heavy);
    assert_eq!(FORMANT_BUDGET.0, share - heavy - table);
    assert_eq!(OpKind::Formant.charge(), AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_OP.0);
    assert_eq!(FORMANT_N, FORMANT_BUDGET.0 / OpKind::Formant.charge());
}
#[test]
fn the_budget_rule_is_exact() {
    use OpKind::*; let t = Table(WaveId::W1);
    let n = FORMANT_N as usize;
    let mut k = [t; OPS];
    for i in 0..n { k[i] = Formant; }
    assert!(fits(&k));
    k[n] = Formant;
    assert!(!fits(&k));
    assert_eq!(fits(&[Noise; OPS]), 6 * OpKind::Noise.charge() <= FORMANT_BUDGET.0);
}
// cost_test.rs
#[test]
fn heavy_routes_bill_their_named_sum() {
    // ENV B BURST on the VCA with its LEVEL routed, a TIME route, FREQ and Q, the 16 registry
    // columns (the filter's LEVEL among them; no FOLD or DRIVE param), LFOs 1–3 FUNC; LP24 SVF
    let (p, mods) = heavy_routes();
    assert_eq!(ModRouting::cost(&p, &mods), ModRouting::HEAVY);
    assert_eq!(Voice::cost(&p, &mods).0 - EngineSlot::cost(&p, &mods).0, HEAVY_OVERHEAD.0);
}
#[test]
fn formant_budget_keeps_four_voices() {
    let kinds = [OpKind::Table(WaveId::W1), OpKind::Formant, OpKind::Noise];
    for code in 0..3usize.pow(6) {
        let k: [OpKind; OPS] = core::array::from_fn(|i| kinds[code / 3usize.pow(i as u32) % 3]);
        let p = a16_a17_with(k);   // LEVEL 99, FDBK 1 on all
        let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
        let v = |bus: Cost| (budget - bus.0) / (AlgoEngine::cost(&p, &[false; OPS]).0 + HEAVY_OVERHEAD.0);
        if fits(&k) {
            assert!(v(FxBus::COST_TAPE) >= FLOOR_VOICES && v(FxBus::COST_NO_TAPE) >= FLOOR_VOICES, "{k:?}");
        }
    }
    let mut over = [OpKind::Table(WaveId::W1); OPS];
    for i in 0..=FORMANT_N as usize { over[i] = OpKind::Formant; }
    let p = a16_a17_with(over);
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    assert!((budget - FxBus::COST_TAPE.0) / (AlgoEngine::cost(&p, &[false; OPS]).0 + HEAVY_OVERHEAD.0) < FLOOR_VOICES);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test kinds_test --test cost_test`. Expected: FAIL, because `kinds` doesn't exist.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with every existing cost literal unchanged.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests/kinds_test.rs chimera-core/tests/cost_test.rs
git commit -m "Operator kinds, their cost terms, and a formant budget derived from the cost model"
```

### Task 4: `Kinds` owns the operators' kinds; every write path checks

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{kinds.rs,params.rs,engine.rs}`;
  - `chimera-engine/src/{addr.rs,params.rs,preset.rs,factory.rs}`;
  - `chimera-engine/src/storage/{block_codec.rs,sound.rs,system.rs}`;
  - `chimera-core/src/ui/{page.rs,part_page.rs,mod.rs}`;
  - `chimera-engine/src/dsp/voice.rs`;
  - `chimera-engine/src/project/{codec.rs,parts.rs}`, `chimera-engine/src/orbit/scene.rs` and the UI's impls (`ui/mod.rs`, `ui/renderer.rs`);
  - every other `BlockRead` or `Blocks` impl and caller;
  - `chimera-stm32/src/bench.rs`;
  - `chimera-core/tests/{fixtures/disk_codes_v1.txt,disk_codes_test.rs,common/sweep.rs}`.
- Test: `chimera-core/tests/kinds_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Kinds([OpKind; OPS]);
  #[derive(Debug, PartialEq)] pub struct OverBudget;
  impl Kinds {
      pub const TABLE: Kinds;
      pub const fn new(k: [OpKind; OPS]) -> Kinds;   // panics unless fits: a const is checked at compile time
      pub const fn get(&self, op: usize) -> OpKind;
      pub const fn all(&self) -> [OpKind; OPS];
      pub fn with(self, op: usize, kind: OpKind) -> Result<Kinds, OverBudget>;
      pub fn any_formant(&self) -> bool;
  }
  pub struct KindStep { pub to: OpKind, pub skipped: bool }
  pub fn step_kind(k: Kinds, op: usize, delta: i8) -> KindStep;
  //   walks |delta| allowed kinds in delta's direction, clamped at 0 and 17; skipped when a refused kind was passed or the walk was cut short
  impl AlgoParams {
      pub fn kinds(&self) -> Kinds;
      pub fn try_set_kind(&mut self, op: usize, kind: OpKind) -> Result<(), OverBudget>;
      pub fn op(&self, op: Op) -> OpView<'_>;
      pub fn op_mut(&mut self, op: Op) -> OpViewMut<'_>;
      pub const fn single(wave: WaveId) -> Self;
  }
  pub enum BlockView<'a> { Plain(&'a dyn Block), Op(OpView<'a>) }          // Deref<Target = dyn Block + 'a>
  pub enum BlockViewMut<'a> { Plain(&'a mut dyn Block), Op(OpViewMut<'a>) } // DerefMut
  pub trait BlockRead { fn block(&self, b: BlockRef) -> Option<BlockView<'_>>; }            // today Option<&dyn Block>
  pub trait Blocks: BlockRead { fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>>; }
  ```
- **The guards.** `OpView` and `OpViewMut` implement `Block` over `ALGO_OP_SPECS`.
  - WAVE reads the kind's code.
  - The WAVE `write`, `set_enum_code` and `nudge` go through `try_set_kind`, and `nudge` uses `step_kind`. A refused write changes nothing.
  - Every other id delegates to `ops[op]`, which has no wave byte.
- **The WAVE spec** is `ParamSpec::choice(0, "WAVE", ValFmt::Names(&OP_KIND_NAMES), 17.0, 0.0, Change::Crossfade).ident("WAVE").glyph(FocusGlyph::Wave)`, and the const assert becomes `== OP_KIND_COUNT - 1`. `OP_KIND_NAMES[17]` is "NOISE" (full word), and its ident is `NOIS`.
- **The encoder toast.** On an encoder turn of an `(AlgoOp(n), WAVE)` cell, `UiState` calls `step_kind` first and toasts `FORMANT BUDGET FULL` when `skipped`.
- **The decoder.** `decode_block` (`chimera-engine/src/storage/block_codec.rs`, today `-> Result<u16, FileError>`) also reports a refused kind: `Result<Applied, FileError>`, with `Applied { count: u16, refused_kind: bool }`. `SoundDecoder::over_budget()` ORs it over the file, and the load path toasts.
- **The engine.** `AlgoEngine.waves` becomes `kinds: [OpKind; OPS]`, and `Swap` compares kinds. A non-Table kind gets `gain_to = 0.0`: silent until Tasks 9 and 10.
- **The sweep.** `silences()` gains the reason "formant kind before its kernel" for WAVE 16 and 17. Task 9 removes it.
- **The factory.** Sounds build `const` `Kinds::new([...])`.

- [ ] **Step 1: Write the failing tests.**

```rust
fn fits_all(p: &AlgoParams) -> bool { fits(&p.kinds().all()) }
#[test]
fn try_set_kind_refuses_overspend_and_changes_nothing() {
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N as usize { p.try_set_kind(i, OpKind::Formant).unwrap(); }
    let before = p.kinds();
    assert_eq!(p.try_set_kind(FORMANT_N as usize, OpKind::Formant), Err(OverBudget));
    assert_eq!(p.kinds(), before);
}
#[test]
fn every_write_path_checks_the_budget() {
    let mut rng = 0x1234_5678u32;
    let mut s = Sound::init(EngineType::Algo);
    let mut ui = test_ui_with(&s);   // UiState on an OP page's WAVE cell (screen harness)
    for i in 0..10_000 {
        rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
        let op = Op::ALL[(rng % 6) as usize];
        let code = ((rng >> 8) % 18) as u8;
        let mut b = s.params.block_mut(BlockRef::AlgoOp(op)).unwrap();
        match i % 6 {
            0 => b.set(AlgoOpParams::WAVE, code as f32),
            1 => { let _ = b.set_enum_code(AlgoOpParams::WAVE, code); }
            2 => b.nudge(AlgoOpParams::WAVE, if rng & 1 == 0 { 1 } else { -3 }),
            3 => { drop(b); decode_one(&mut s.params, op, code); }   // a one-entry OP_x record through decode_block
            4 => { drop(b); ui.turn_wave(op, if rng & 1 == 0 { 2 } else { -1 }); s = ui.sound().clone(); }
            _ => { drop(b); sweep_set(&mut s, op, code); }           // sweep::Bench::set's path
        }
        assert!(fits_all(&s.params.algo), "step {i}");
    }
}
#[test]
fn the_wave_encoder_steps_over_refused_kinds() {
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N as usize { p.try_set_kind(i, OpKind::Formant).unwrap(); }
    let n = FORMANT_N as usize;
    p.try_set_kind(n, OpKind::Table(WaveId::SSAW)).unwrap();
    let s = step_kind(p.kinds(), n, 1);
    assert_eq!(s.to, OpKind::Noise); assert!(s.skipped);
    p.try_set_kind(n, OpKind::Noise).unwrap();
    let s = step_kind(p.kinds(), n, -1);
    assert_eq!(s.to, OpKind::Table(WaveId::SSAW)); assert!(s.skipped);
    let s = step_kind(p.kinds(), n, -2);
    assert_eq!(s.to, OpKind::Table(WaveId::RSQR));
}
#[test]
fn an_over_budget_file_decodes_its_excess_as_w1() {
    let file = sound_file_with_kinds(&[16; 6]);   // hand-built bytes: six FRMT
    let (s, over) = decode_sound(&file).unwrap();
    let k = s.params.algo.kinds().all();
    let n = FORMANT_N as usize;
    assert!(k[..n].iter().all(|&x| x == OpKind::Formant));
    assert!(k[n..].iter().all(|&x| x == OpKind::Table(WaveId::W1)));
    assert!(over);
}
#[test]
#[should_panic]
fn kinds_new_rejects_overspend() { let _ = Kinds::new([OpKind::Formant; 6]); }
```

  Add a `compile_fail` doctest on `Kinds::new`: `const K: Kinds = Kinds::new([OpKind::Formant; 6]);`. Append `E k 0 16 FRMT # FRMT` and `E k 0 17 NOIS # NOISE` for `k` = 3–8 (`OP_A` … `OP_F`). `neutral_v1.txt` takes the kinds' new representation, with every value unchanged. `wave_and_algo_idents_are_their_own_tables` reads `OP_KIND_IDENTS`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test kinds_test --test disk_codes_test`. Expected: FAIL.
- [ ] **Step 3: Implement**, migrating every `BlockRead` and `Blocks` impl and caller. A writing `if let Some(b)` becomes `if let Some(mut b)`. Report the flash delta (G7): the guards touch every block path.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with G6 showing no change.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests chimera-stm32/src/bench.rs
git commit -m "AlgoParams owns the operator kinds; every write checks the formant budget"
```

### Task 5: The new parameters, formats, INIT, frozen codes and demo recipes

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{params.rs,engine.rs}`;
  - `chimera-engine/src/{block.rs,factory.rs}`, `chimera-engine/src/storage/sound.rs` (`MAX_SOUND_COST`);
  - `chimera-core/src/ui/fmt.rs`;
  - `chimera-core/tests/{fixtures/disk_codes_v1.txt,fixtures/neutral_v1.txt,common/sweep.rs,codec_compat_test.rs,sound_codec_test.rs,valfmt_snap_test.rs,param_declarations_test.rs,screen_golden_test.rs}`.
- Create:
  - `chimera-engine/src/dsp/algo/vowel.rs` (`VowelSlot` only);
  - `chimera-core/tests/{formant_params_test.rs,formant_demo_test.rs}`.

**Interfaces:**
- **The new fields.** `AlgoOpParams` gains `frmt`, `bw`, `skirt`, `key`, `res` and `vslot: u8` (ids 13–18). `AlgoParams` gains `vowel: u8` (`AlgoParams::VOWEL = ParamId(4)`).
- **The specs follow § 2 exactly.**
  - The labels are FORMANT, BW, SKIRT, KEY, RES, SLOT and VOWEL. The idents are `FRMT`, `BW`, `SKIRT`, `KEY`, `RES`, `VSLOT` and `VOWEL`.
  - FRMT is `LogHz { base: 100, per_octave: 24 }` and BW is `LogHz { base: 20, per_octave: 24 }`.
  - SKIRT is `Int(3)`, KEY and RES `Int(100)`, VSLOT `Names(&VSLOT_NAMES)` and VOWEL `ValFmt::Vowel`.
  - The motions (ADR 0112):
    - FRMT, BW and VOWEL: `ParamSpec::stepped(.., Motion::Continuous(Smoothing::KNOB))`;
    - KEY and RES: `Motion::Continuous(Smoothing::NoteOn)`;
    - SKIRT: `Motion::Stepped(Change::NoteOn)`;
    - VSLOT: `ParamSpec::choice(.., Change::NoteOn)`, with `.glyph(FocusGlyph::None)` as on SKIRT.
  - **All are `modulatable: false` in this task.** Task 12 flips FRMT, BW and VOWEL, and gives `AlgoLive::slot` their knob glides.
- **The Sound cap.** 37 new params at 5 B take `sparse_bound`'s worst from 2,349 to 2,534 B, so `MAX_SOUND_COST` goes from 2,352 to 2,544. Re-record the pool and library goldens that fail, and name them in the commit body.
- **Neutral.** `neutral_v1.txt` gains the new fields at INIT's values (`neutral_is_a_contract`).
- **`vowel.rs`:** `pub enum VowelSlot { Auto, F1, F2, F3, Off }`, with `from_code`, `code`, `VSLOT_NAMES` and `VSLOT_IDENTS`.
- **The laws:** `pub fn frmt_hz(v: f32) -> f32` and `pub fn bw_hz(v: f32) -> f32`.
- **Snap points:** `LogHz` snaps each octave, and `Vowel` each vowel.
- **INIT:** FRMT `[69, 83, 111, 123, 123, 123]` and VOWEL 50.
- **Billing:** `AlgoEngine::cost` bills a Formant operator with stored SKIRT 0 at `COST_FRMT_ONE`.
- **`factory.rs`**, in its `authored` module (test-support), beside `authored_sound`:
  ```rust
  pub enum Demo { Choir, TalkBass, Breath }
  pub fn demo_sound(d: Demo) -> Sound;
  ```
  The recipes are chosen at compile time by `FORMANT_N` (Ruling 6).
  - **At N ≥ 3:** the spec's § Demo clips.
  - **CHOIR at N = 2:**
    - op 2 FRMT F2 DETUNE −2 and op 3 FRMT F2 DETUNE +2;
    - op 1 NOIS on F1 (WIDTH 60 ≈ 113 Hz, RES 70);
    - op 4 NOIS on F3 (WIDTH 3 kHz, low).
  - **TALK BASS at N = 2:** op 1 W1 at 0.50, op 2 FRMT F1, op 3 FRMT F2, op 4 NOIS F3 (RES 70).
  - BREATH is the same at any N.
  - Every recipe's `Kinds` is `const`, and carrier LEVELs start at 99.
- **`sweep.rs`:** `Patch::Formant`: ops 1–2 FRMT, op 3 FRMT if N ≥ 3 (else NOIS), op 4 NOIS, A1, ops 5–6 LEVEL 0. `Patch::note(self)` is 48 for `Formant`, 60 otherwise, and `play`'s `bench.note(60, ..)` uses it. `Patch::Formant` isn't in `param_sweep_test`'s `patches()` until Task 9.
- **`formant_demo_test.rs`:**
  - `render_phrase(d)` plays through the sweep `Bench`: CHOIR a C3–G3–C4 chord for 3 s; TALK BASS C2, D#2, G2 at 400 ms each; BREATH C4 for 2 s.
  - `demo_clips_are_clean` asserts finite, peak ≤ 1.0 and no `Tape::click`.
  - `#[ignore] render_formant_demo_clips` writes 48 kHz 16-bit WAVs to `target/demo/` with a hand-written header.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn formant_params_follow_the_spec() {
    let p = AlgoOpParams::default();
    assert_eq!((p.frmt, p.bw, p.skirt, p.key, p.res, p.vslot), (72, 72, 1, 0, 0, 0));
    assert!((frmt_hz(72.0) - 800.0).abs() < 1e-3 && (frmt_hz(168.0) - 12_800.0).abs() < 0.1);
    assert!((bw_hz(72.0) - 160.0).abs() < 1e-3 && (bw_hz(216.0) - 10_240.0).abs() < 0.1);
    let ids: Vec<_> = ALGO_OP_SPECS[13..].iter().map(|s| (s.id.0, s.ident, s.max)).collect();
    assert_eq!(ids, [(13, "FRMT", 168.0), (14, "BW", 216.0), (15, "SKIRT", 3.0), (16, "KEY", 100.0), (17, "RES", 100.0), (18, "VSLOT", 4.0)]);
    assert_eq!((ALGO_SPECS[4].id.0, ALGO_SPECS[4].ident, ALGO_SPECS[4].max), (4, "VOWEL", 100.0));
    assert!(ALGO_OP_SPECS[13..].iter().all(|s| !s.modulatable) && !ALGO_SPECS[4].modulatable);
}
#[test]
fn init_carries_vowel_a() {
    let p = AlgoParams::default();
    assert_eq!(p.ops.map(|o| o.frmt), [69, 83, 111, 123, 123, 123]);
    assert_eq!(p.vowel, 50);
}
#[test]
fn log_hz_and_vowel_read_as_the_spec_says() {
    let f = |v: f32, fmt| { let mut b = FmtBuf::new(); fmt_val(&mut b, v, fmt); b.as_str().to_string() };
    let hz = ValFmt::LogHz { base: 100, per_octave: 24 };
    assert_eq!((f(69.0, hz), f(83.0, hz), f(168.0, hz)), ("734 Hz".into(), "1.10 kHz".into(), "12.8 kHz".into()));   // as Law::Cutoff
    assert_eq!(f(0.0, ValFmt::LogHz { base: 20, per_octave: 24 }), "20 Hz");
    let v: Vec<_> = [0.0, 25.0, 50.0, 62.0, 63.0, 100.0].map(|x| f(x, ValFmt::Vowel)).into();
    assert_eq!(v, ["U", "O", "A", "A", "E", "I"]);
}
#[test]
fn old_patches_load_unchanged() {   // in codec_compat_test, over its `sources()`
    for i in 0..8 {
        let d = decode(&fixture(&format!("factory_{i}.snd"))).unwrap();
        let a = &d.params.algo;
        assert_eq!(a.ops.map(|o| o.frmt), [69, 83, 111, 123, 123, 123]);
        assert!(a.ops.iter().all(|o| (o.bw, o.skirt, o.key, o.res, o.vslot) == (72, 1, 0, 0, 0)));
        assert_eq!(a.vowel, 50);
        assert_eq!(fnv1a(&render_sound(&d.params, &d.mod_state)), V1_HASHES[i]);
    }
}
#[test]
fn skirt_0_bills_the_one_lane_term() {
    let mut p = AlgoParams::single(WaveId::W1);
    p.try_set_kind(0, OpKind::Formant).unwrap();
    p.ops[0].skirt = 0;
    assert_eq!(AlgoEngine::cost(&p, &[false; OPS]), AlgoEngine::COST_BASE + AlgoEngine::COST_FRMT_ONE);
    p.ops[0].skirt = 1;
    assert_eq!(AlgoEngine::cost(&p, &[false; OPS]), AlgoEngine::COST_BASE + AlgoEngine::COST_FRMT);
}
#[test]
fn the_demo_sounds_fit_the_budget() {
    for d in [Demo::Choir, Demo::TalkBass, Demo::Breath] { assert!(fits(&demo_sound(d).params.algo.kinds().all())); }
}
```

  Append the lines `B k 13..18` (with the labels FORMANT, BW, SKIRT, KEY, RES and SLOT as the comments), `E k 18 0..4` (`AUTO` … `OFF`) and `B 2 4 VOWEL # VOWEL` to `disk_codes_v1.txt`. Assert each new label's width ≤ `theme::CELL_BAR_W` in `formant_params_test`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_params_test --test disk_codes_test --test valfmt_snap_test --test sound_codec_test --test param_declarations_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with the hashes unchanged and only the pool and library goldens re-recorded.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests
git commit -m "FRMT, BW, SKIRT, KEY, RES, SLOT and VOWEL are stored with frozen codes"
```

- [ ] **Step 6: QA gate 1.** Run G1, G2, G3, G4, G6 and G7. G5 runs `demo_clips_are_clean` only, which silent clips pass.

---

# Phase 2 — The pure laws

### Task 6: The window, SKIRT constants, curves, and the test helpers

**Files:**
- Create:
  - `chimera-engine/src/dsp/algo/formant.rs`;
  - `chimera-core/tests/formant_laws_test.rs`;
  - `chimera-core/tests/common/formant.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Skirt { S0, S1, S2, S3 }
  impl Skirt {
      pub const ALL: [Skirt; 4];
      pub const fn from_code(v: u8) -> Skirt;
      pub const fn q(self) -> u32;     // 2, 4, 8, 16
      pub const fn k(self) -> f32;     // 1.3748, 1.7341, 2.2910, 3.1209
      pub const fn c_q(self) -> f32;   // 0.40635, 0.29954, 0.21653, 0.15486
      pub const fn m_q(self) -> f32;   // 5.49, 2.62, 2.07, 1.97
  }
  pub fn window(x: f32, s: Skirt) -> f32;        // clamp to [0,1], u = 4x(1−x), square s+1 times
  pub fn rho(s: Skirt, nu: f32) -> f32;          // ν = 2·FC·L: 49 points on [0, 12], linear; 0 past 12
  pub fn lane_peak(s: Skirt, fcl: f32) -> f32;   // FC·L: 65 points on [0, 16], linear; 1.0 past 16
  pub fn cycles_to_phase(x: f32) -> u32;         // ((x − floor(x)) · 2^32), wrapping; x any finite f32
  ```
- **`tests/common/formant.rs`** is shared by Tasks 6–12:
  ```rust
  pub fn render_algo(p: &AlgoParams, note: u8, blocks: usize) -> Vec<f32>;
  pub fn render_algo_with(p: &AlgoParams, note: u8, blocks: usize,
                          each_block: impl FnMut(usize, &mut AlgoParams, &mut AlgoLive, &mut AlgoEngine)) -> (Vec<f32>, u32); // (out, grain_cuts)
  pub fn one(kind: OpKind, frmt: u8, bw: u8, skirt: u8) -> AlgoParams;   // op 1 alone, A1, LEVEL 99, KEY 0, VSLOT OFF
  pub fn grain_fc(out: &[f32]) -> f32;                    // mean zero-crossing rate inside grains (|y| > 1 % of peak)
  pub fn spectrum_db(x: &[f32], n: usize) -> Vec<f32>;    // Hann, naive DFT magnitude, dB re max
  pub fn off_grid_db(x: &[f32], grid: &[f32]) -> f32;     // energy outside ±2 bins (n = 8192) of each grid Hz, dB re total
  pub fn harmonic_db(x: &[f32], hz: f32) -> f32;          // dB of common::goertzel
  pub fn level_db_vs_r(x: &[f32]) -> f32;                 // 20·log10(common::rms / (R_REF / √2))
  pub fn worst_10ms_db(x: &[f32], steady_rms: f32) -> f32;
  pub fn synth_level(f0: f32, fc: f32, bw: f32, s: Skirt) -> (f32, f32);  // (level dB vs R, peak): the exact one-period synthesis of spec § 3 on formant_law, in Rust
  ```
  - Reuse `common::{goertzel, rms, clicks}` and `common::spectrum::{alias_db, peak_hz}` where they fit.
  - `render_algo*` drive `AlgoEngine::render(out, p, live, SR, &mut TapRows)` (its signature on main) with no taps wanted.
  - The session scratchpad's `fprobe/` scripts are gone, so the tests' numbers stand as literals. `synth_level` is written from spec § 3: one period of both lanes at the latched law, its RMS against R and its peak.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn window_is_the_polynomial_bell() {
    for s in Skirt::ALL {
        assert_eq!(window(0.5, s), 1.0);
        assert_eq!([window(0.0, s), window(1.0, s), window(-0.2, s), window(1.3, s)], [0.0; 4]);
        assert!((window(0.25, s) - 0.75f32.powi(s.q() as i32)).abs() < 1e-6);
    }
}
#[test]
fn laws_are_pinned() {
    for s in Skirt::ALL {
        let m = measure(s);   // 4,000-point midpoint integration and a ν scan in 0.005 steps
        assert!((m.k / s.k() - 1.0).abs() < 0.005);
        assert!((m.c_q - closed_form_c_q(s.q())).abs() < 1e-5 && (s.c_q() - m.c_q).abs() < 1e-5);
        assert!((m.m60 / s.k() / s.m_q() - 1.0).abs() < 0.005);
        assert!((m.sidelobe_db - [-27.7, -38.5, -56.4, -87.0][s as usize]).abs() < 0.3);
        for (nu, want) in m.rho_grid { assert!((rho(s, nu) - want).abs() < 1e-4 + 0.005 * want.abs()); }
        for (fcl, want) in m.peak_grid { assert!((lane_peak(s, fcl) - want).abs() < 1e-4 + 0.005 * want); }
    }
}
#[test]
fn curves_are_safe_past_the_table() {
    for s in Skirt::ALL {
        assert_eq!((rho(s, 12.5), lane_peak(s, 16.5)), (0.0, 1.0));
        for i in 0..4000 {
            let fcl = i as f32 * 0.01;   // to 40
            assert!(lane_peak(s, fcl) >= true_lane_peak(s, fcl) - 1e-4, "{s:?} {fcl}");
        }
    }
}
#[test]
fn cycles_to_phase_wraps() {
    assert_eq!(cycles_to_phase(7.25), 0x4000_0000);
    assert_eq!(cycles_to_phase(-3.25), 0xC000_0000);
    assert_eq!(cycles_to_phase(0.0), 0);
    assert!(cycles_to_phase(1e6 + 0.5).abs_diff(0x8000_0000) < 1 << 12);
}
#[test] #[ignore]
fn print_curves() { /* prints RHO [[f32; 49]; 4] and PEAK [[f32; 65]; 4] for formant.rs */ }
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_laws_test`. Expected: FAIL.
- [ ] **Step 3: Implement.** Paste the literals printed by `-- --ignored print_curves --nocapture`.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test formant_laws_test`, then `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src/dsp/algo chimera-core/tests/formant_laws_test.rs chimera-core/tests/common
git commit -m "The formant window, its SKIRT constants and curves, measured and pinned"
```

### Task 7: The width floor, gains and edge laws (FRMT and NOIS)

**Files:**
- Modify: `chimera-engine/src/dsp/algo/formant.rs`.
- Create: `chimera-engine/src/dsp/algo/noise.rs`.
- Test: `chimera-core/tests/formant_laws_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub const R_REF: f32 = 0.251_19;
  pub const BRACKET_FLOOR: f32 = 0.05;
  pub const NYQUIST_HZ: f32 = 24_000.0;
  pub fn width_floor(f0: f32) -> f32;     // Skirt::S3.k() · f0 / 2
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct FormantLaw { pub fc: f32, pub bw_eff: f32, pub d: f32, pub slack: f32, pub g_m: f32, pub g_c: f32, pub edge: f32 }
  pub fn formant_law(f0: f32, fc: f32, bw: f32, s: Skirt) -> FormantLaw;
  // noise.rs
  pub const NOISE_GAIN: f32 = 0.251;
  pub const NOISE_CLAMP: f32 = 3.8;
  pub fn noise_coef(width: f32, sr: f32) -> (f32, f32);   // (c, k_n), spec § 4
  pub fn res_mix(res: u8) -> (f32, f32);                  // (α, β)
  pub fn noise_edge(fc: f32, width: f32) -> f32;          // 1 at fc + width/2 ≤ 16 kHz, linear to 0 at 20 kHz
  pub fn noise_seed(voice: u8, op: usize) -> u32;         // nonzero
  ```
- **`formant_law`, in order:**
  1. `b_min = width_floor(f0)` and `bw_eff = hypot(bw, b_min)`.
  2. `room = (NYQUIST_HZ − fc)/m_q`.
     - If `room ≥ b_min`: `bw_eff = min(bw_eff, room)` and `edge = 1`.
     - Otherwise: `bw_eff = b_min`, `edge = clamp((room − b_min/2)/(b_min/2), 0, 1)`, and `fc = min(fc, NYQUIST_HZ − m_q·b_min/2)`.
  3. `fc = max(fc, 20)`, `L = k/bw_eff`, `d = L·f0` and `slack = 2 − d`.
  4. `p̂ = min(1, lane_peak(s, fc·L)·(if d > 1 {2} else {1}))` and `g_m = 1/p̂`.
  5. `g_c = min(R_REF / sqrt(d·c_q·max(1 − rho(s, 2·fc·L), BRACKET_FLOOR)), 1/p̂)`.

- [ ] **Step 1: Write the failing tests.** The numbers came from the review's probes (`fix.py`, `nois.py`, `rtail.py`), which are now gone, and stand here as literals.

```rust
const C1: f32 = 32.70; const C2: f32 = 65.41; const C3: f32 = 130.81; const A3: f32 = 220.0; const C5: f32 = 523.25;
#[test]
fn width_floor_is_skirt_free() {
    assert!((width_floor(A3) - 343.3).abs() < 0.5);
    assert!((width_floor(98.0) - 152.9).abs() < 0.5);
    for s in Skirt::ALL { assert!((formant_law(A3, 1000.0, 20.0, s).bw_eff - 343.9).abs() < 0.5); }
}
#[test]
fn a_window_never_exceeds_two_periods() {
    for (s, dmax) in Skirt::ALL.into_iter().zip([0.881, 1.111, 1.468, 2.0]) {
        for f0 in [33.0, 110.0, 440.0, 2093.0] { assert!(formant_law(f0, 1000.0, 0.0, s).d <= dmax + 1e-3); }
    }
}
#[test]
fn rms_gain_uses_the_exact_energy() {
    for s in Skirt::ALL { for bw in [524.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0] {
        assert!(synth_level(C3, 1000.0, bw, s).0.abs() < 0.02, "{s:?} {bw}");
    } }
    assert!(synth_level(C3, 1000.0, 524.0, Skirt::S3).0.abs() < 0.02);   // was +1.56 dB with the curves flat past 4
    assert!(synth_level(C5, 270.0, 160.0, Skirt::S1).0.abs() < 0.02);
    assert!(synth_level(C5, 300.0, 160.0, Skirt::S3).0.abs() < 0.02);
    let l = formant_law(C3, 1000.0, 4000.0, Skirt::S1);
    let draft = 1.0 / (l.d * Skirt::S1.c_q()).sqrt() * R_REF;
    assert!((20.0 * (draft / l.g_c).log10()).abs() > 6.0);   // the first draft's law was 7.47 dB off here
}
#[test]
fn the_ceiling_bounds_the_peak() {
    for f0 in [C1, C2, C3, 261.6, C5, 1046.5, 2093.0] { for fc in [270.0, 730.0, 1000.0, 2440.0, 8000.0, 12_800.0] {
        for bw in [20.0, 160.0, 2000.0, 10_000.0] { for s in Skirt::ALL {
            assert!(synth_level(f0, fc, bw, s).1 <= 1.0 + 1e-3, "{f0} {fc} {bw} {s:?}");
        } } } }
    assert!(synth_level(C1, 12_800.0, 2000.0, Skirt::S3).1 <= 1.0 + 1e-3);   // was 1.22 with p̂ flat past 4
    let (l, pk) = synth_level(C3, 1000.0, 10_000.0, Skirt::S1);
    assert!((l + 5.83).abs() < 0.5 && (pk - 1.0).abs() < 0.005);
    assert!((synth_level(C2, 1000.0, 2000.0, Skirt::S1).0 + 2.23).abs() < 0.5);
    assert!((synth_level(C3, 1000.0, 1000.0, Skirt::S1).1 - 0.746).abs() < 0.02);
    assert!((synth_level(C5, 270.0, 160.0, Skirt::S1).1 - 0.287).abs() < 0.02);
}
#[test]
fn the_edge_narrows_then_fades() {
    let l = formant_law(C3, 8000.0, 10_000.0, Skirt::S0);
    assert!((l.bw_eff - 2914.0).abs() < 5.0 && l.edge == 1.0);
    let b = width_floor(C3); let m = Skirt::S0.m_q();
    assert!(formant_law(C3, NYQUIST_HZ - m * b / 2.0, 0.0, Skirt::S0).edge.abs() < 1e-6);
    assert!((formant_law(C3, NYQUIST_HZ - m * 0.75 * b, 0.0, Skirt::S0).edge - 0.5).abs() < 0.01);
}
#[test]
fn noise_band_laws() {
    for w in [20.0, 500.0, 4000.0] { assert!((steady_two_pole_rms(w, 0x1234) - 1.0).abs() < 0.01, "{w}"); }  // 20 s at x = k_n·n
    for r in [0, 50, 100] { let (a, b) = res_mix(r); assert!((a * a + b * b - 1.0).abs() < 1e-6); }
    assert!(NOISE_GAIN * (NOISE_CLAMP * NOISE_CLAMP + 1.0).sqrt() <= 0.99);
    assert_eq!([noise_edge(15_000.0, 2_000.0), noise_edge(17_000.0, 2_000.0), noise_edge(19_000.0, 2_000.0)], [1.0, 0.5, 0.0]);
    let seeds: Vec<u32> = (0..8).flat_map(|v| (0..6).map(move |o| noise_seed(v, o))).collect();
    assert!(seeds.iter().all(|&s| s != 0));
    let mut u = seeds.clone(); u.sort(); u.dedup(); assert_eq!(u.len(), 48);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_laws_test`. Expected: FAIL.
- [ ] **Step 3: Implement** both modules.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test formant_laws_test`, then `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src/dsp/algo chimera-core/tests
git commit -m "Formant and noise laws: SKIRT-free width floor, exact-energy level, peak ceiling, -60 dB edge"
```

### Task 8: `fc_hz`, the vowel table and slots

**Files:**
- Modify: `chimera-engine/src/dsp/algo/vowel.rs`.
- Test: `chimera-core/tests/formant_laws_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Formant { F1, F2, F3 }
  pub const KEY_REF: f32 = 60.0;
  pub static VOWEL_HZ: [[f32; 3]; 5];   // U O A E I: [300,870,2240],[570,840,2410],[730,1090,2440],[530,1840,2480],[270,2290,3010]
  pub static VOWEL_OCT: [[f32; 3]; 5];  // log2(Hz / A's Hz), literals
  pub fn vowel_delta(slot: Formant, vowel: f32) -> f32;
  pub fn fc_hz(frmt: f32, vowel: f32, slot: Option<Formant>, key: f32, pitch: f32) -> f32;
  pub fn resolve_slots(kinds: &Kinds, vslot: [VowelSlot; OPS]) -> [Option<Formant>; OPS];
  ```
- **Slots:** a Table operator gets `None`, and OFF gets `None`. AUTO operators are counted among the formant-kind operators set to AUTO, from op 1: F1, F2, F3, then `None`.

- [ ] **Step 1: Write the failing tests.** Every kind mix fits the budget at N ≥ 2.

```rust
#[test]
fn fc_law_is_exact() {
    let a = 72.0;
    assert!((fc_hz(a, 50.0, None, 50.0, 72.0) - 1131.4).abs() / 1131.4 < 1e-3);
    for (key, note, want) in [(0.0, 48.0, 800.0), (100.0, 48.0, 400.0), (100.0, 72.0, 1600.0), (50.0, 60.0, 800.0)] {
        assert!((fc_hz(a, 50.0, None, key, note) - want).abs() / want < 1e-3);
    }
    for (v, row) in [0.0, 25.0, 50.0, 75.0, 100.0].into_iter().zip(VOWEL_HZ) {
        for (s, hz) in [Formant::F1, Formant::F2, Formant::F3].into_iter().zip(row) {
            let want = 800.0 * hz / VOWEL_HZ[2][s as usize];
            assert!((fc_hz(a, v, Some(s), 0.0, 60.0) - want).abs() / want < 1e-3);
        }
    }
    assert_eq!(fc_hz(0.0, 100.0, Some(Formant::F1), 100.0, 0.0), 20.0);
}
#[test]
fn vowel_table_is_peterson_barney_on_the_u_o_a_e_i_path() {
    for v in 0..5 { for s in 0..3 {
        assert!((VOWEL_OCT[v][s] - (VOWEL_HZ[v][s] / VOWEL_HZ[2][s]).log2()).abs() < 1e-6);
    } }
    assert_eq!(VOWEL_OCT[2], [0.0; 3]);
    assert!((vowel_delta(Formant::F2, 75.0) - (1840.0f32 / 1090.0).log2()).abs() < 1e-6);
    assert!((vowel_delta(Formant::F2, 62.5) - 0.5 * (1840.0f32 / 1090.0).log2()).abs() < 1e-6);
}
#[test]
fn vowel_slots_resolve() {
    use OpKind::*; use VowelSlot::*; let t = Table(WaveId::W1);
    let k = Kinds::new([t, Formant, t, Noise, Noise, t]);
    assert_eq!(resolve_slots(&k, [Auto; 6]), [None, Some(F1), None, Some(F2), Some(F3), None]);
    let k = Kinds::new([Formant, Formant, Noise, Noise, Noise, t]);          // five AUTO formant kinds
    assert_eq!(resolve_slots(&k, [Auto; 6])[3..5], [None, None]);
    let k = Kinds::new([Formant, Formant, Noise, Noise, t, t]);
    assert_eq!(resolve_slots(&k, [F1, F1, Auto, Auto, Auto, Auto])[..4], [Some(F1), Some(F1), Some(F1), Some(F2)]);
    assert_eq!(resolve_slots(&k, [Off, Auto, Auto, Auto, Auto, Auto])[..2], [None, Some(F1)]);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test formant_laws_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src/dsp/algo/vowel.rs chimera-core/tests/formant_laws_test.rs
git commit -m "VOWEL runs U-O-A-E-I over Peterson and Barney's averages; slots resolve by setting"
```

- [ ] **Step 6: QA gate 2.** Run G1 and G6.

---

# Phase 3 — NOIS

### Task 9: The NOIS kernel path, and note-on latching

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{kernel.rs,engine.rs}`;
  - `chimera-engine/src/dsp/{engines.rs,voice.rs}`;
  - `chimera-core/tests/{algo_kernel_test.rs,common/sweep.rs,param_sweep_test.rs}`.
- Create: `chimera-core/tests/algo_noise_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub enum KindBlock {
      Table { lo: &'static Table, hi: &'static Table, xfade_from: f32, xfade_to: f32 },
      Formant(FormantBlock),    // Task 10
      Noise(NoiseBlock),
  }
  pub struct NoiseBlock { pub c: (f32, f32), pub k: (f32, f32), pub alpha: (f32, f32), pub beta: (f32, f32) }  // (from, to), ramped
  pub struct OpBlock { pub inc: u32, pub gain_from: f32, pub gain_to: f32, pub feedback: f32, pub kind: KindBlock }   // today's lo, hi, xfade_* move into KindBlock::Table
  enum KindState { Table, Formant(FormantState), Noise { y1: f32, y2: f32 } }
  impl Kernel {
      pub const fn new() -> Self;                       // rng[op] = noise_seed(0, op)
      pub fn seed(&mut self, voice: u8);
      pub fn reset(&mut self, op: usize, kind: OpKind);   // today reset(op)
  }
  pub fn sine(p: u32) -> f32;                           // W1 mip 0, interpolated, unit
  pub struct OpLatch { pub skirt: Skirt, pub slot: Option<Formant>, pub key: f32, pub mix: (f32, f32) }
  impl AlgoEngine {
      pub fn seed(&mut self, voice: VoiceIdx);
      pub fn held_skirt_extra(&self, p: &AlgoParams) -> Cost;   // Ruling 7
  }
  ```
- **`AlgoEngine` gains per-operator state:**
  - `latch: [OpLatch; OPS]`;
  - `noise_c: [f32; OPS]`, `noise_k: [f32; OPS]` and `mix: [(f32, f32); OPS]` (the ramps' "from" values);
  - `c_ratio: [f32; OPS]` (used by Task 10);
  - `view: [(f32, f32); OPS]`: the last block's `(fc, bw_eff)` for Task 14.
- **Latching at `note_on`,** for every operator:
  - `slot` comes from `resolve_slots(&p.kinds(), vslots)`;
  - `key = key as f32`;
  - `mix = res_mix(res)`, ramped from the old value across the first block;
  - `skirt = Skirt::from_code(skirt)` only when the operator is idle.
- **Per-block Noise values:**
  - FC is `fc_hz(frmt, vowel, slot, 0, pitch)`, with the stored FRMT, BW and VOWEL until Task 12 makes them live;
  - `inc` is FC as the operator's increment (proven < 2^31, since FC < 24 kHz);
  - `c` and `k` come from `noise_coef(bw_hz(bw), sr)`;
  - `gain_to` is `level_gain(level − VELOCITY cut) · NOISE_GAIN · noise_edge(fc, bw_hz)`, with no `SAMPLE_SCALE`.
- **Per sample** (in `run_noise::<N>` and the `step` arm; `run_pair` pairs tables only):
  ```
  rng ^= rng << 13; rng ^= rng >> 17; rng ^= rng << 5;
  x = k · (rng as i32 as f32 · 2^−31)
  y1 += c·(x − y1); y2 += c·(y1 − y2)
  phase = phase.wrapping_add(inc)
  y = (alpha·y2.clamp(−NOISE_CLAMP, NOISE_CLAMP) + beta) · sine(phase + pm_phase) · env · gain
  k, c, alpha, beta, gain += their per-sample deltas
  ```
  FEEDBACK is ignored. `reset` to Noise zeroes `y1` and `y2` and keeps `rng`.
- **Rows.** `run_noise` writes its block into the operator's row, as `run_one` does, so links and ALGO taps (`TapRows`, ADR 0096) read it.
- **Seeding:** `EngineSlot::init_in_place` (voice 0) and `EngineSlot::build`'s Algo arm (its `voice`) call `seed`, as the VA arm calls `place`.
- **In place:** `AlgoEngine::init_in_place` writes the new fields, and `field_list!` fails to compile until it does.
- **The sweep:** `Patch::Formant` joins `param_sweep_test`'s `patches()` for `Algo` and `AlgoOp` params, both quick and thorough, and the "formant kind before its kernel" `silences()` reason goes.

- [ ] **Step 1: Write the failing tests.** The stored grid steps used: CENTRE 104 = 2,016 Hz; WIDTH 111 = 494 Hz; 0 = 20 Hz; 48 = 80 Hz; 183 = 3,948 Hz.

```rust
fn nois(frmt: u8, bw: u8, res: u8) -> AlgoParams { let mut p = one(OpKind::Noise, frmt, bw, 1); p.ops[0].res = res; p }
#[test]
fn noise_band_centre_and_width() {
    for bw in [48u8, 111, 183] {
        let out = render_algo(&nois(104, bw, 0), 48, 3000);
        let (peak_hz, width_hz) = band_peak_and_width(&out[BLOCK_SIZE * 10..]);   // averaged 8192-point spectra
        assert!((peak_hz / frmt_hz(104.0) - 1.0).abs() < 0.05, "{bw}");
        assert!((width_hz / bw_hz(bw as f32) - 1.0).abs() < 0.15, "{bw}");
    }
}
#[test]
fn noise_level_is_normalised() {
    for bw in [0u8, 111, 183] { for res in [0u8, 50, 100] {
        let out = render_algo(&nois(104, bw, res), 48, 1500);
        let held = &out[BLOCK_SIZE * 100..];
        assert!(level_db_vs_r(held).abs() < 0.5, "{bw} {res}");
        assert!(held.iter().all(|x| x.abs() <= 1.0));
        if res == 100 { assert!(harmonic_db(held, frmt_hz(104.0)) > -0.05 + db_total(held)); }
    } }
}
#[test]
fn width_jumps_do_not_burst() {
    let steady = rms(&render_algo(&nois(104, 0, 0), 48, 1500)[BLOCK_SIZE * 750..]);   // 20 Hz band, ≥ 1 s after settling
    let (out, _) = render_algo_with(&nois(104, 183, 0), 48, 1500, |b, p, _, _| {
        if b % 188 == 0 { p.ops[0].bw = if p.ops[0].bw == 0 { 183 } else { 0 }; }   // every 250 ms
    });
    assert!(worst_10ms_db(&out, steady) <= 8.0);   // +27.1 dB scaled after the poles (nois.py)
    assert!(out.iter().all(|x| x.abs() <= 1.0));
}
#[test]
fn repeated_notes_do_not_repeat_their_noise() {
    let mut e = AlgoEngine::new(); let p = nois(104, 111, 0);
    let a = play_note(&mut e, &p, 48, 20); let b = play_note(&mut e, &p, 48, 20);
    assert_ne!(a, b);
}
#[test]
fn renders_are_reproducible_from_a_seed() {
    let p = nois(104, 111, 0);
    let mut e1 = AlgoEngine::new(); e1.seed(VoiceIdx::ALL[3]);
    let mut e2 = AlgoEngine::new(); e2.seed(VoiceIdx::ALL[3]);
    assert_eq!(play_note(&mut e1, &p, 48, 50), play_note(&mut e2, &p, 48, 50));
    let mut e3 = AlgoEngine::new(); e3.seed(VoiceIdx::ALL[4]);
    assert_ne!(play_note(&mut e1, &p, 48, 50), play_note(&mut e3, &p, 48, 50));
}
#[test]
fn kind_swaps_under_a_held_note_do_not_click() {
    let (out, _) = render_algo_with(&one(OpKind::Table(WaveId::W1), 104, 111, 1), 48, 1200, |b, p, _, _| {
        if b % 75 == 0 { let k = if p.kinds().get(0) == OpKind::Noise { OpKind::Table(WaveId::W1) } else { OpKind::Noise };
                         p.try_set_kind(0, k).unwrap(); }
    });
    assert!(common::clicks(&out).is_empty());
}
#[test]
fn a_res_change_on_retrigger_ramps() {
    let mut e = AlgoEngine::new(); let mut p = nois(104, 111, 0);
    let a = play_note(&mut e, &p, 48, 40); p.ops[0].res = 100;
    let b = play_note_held(&mut e, &p, 48, 40);   // retrigger without release
    assert!(common::clicks(&[a, b].concat()).is_empty());
}
```

  `algo_kernel_test`'s `op()` helper builds `KindBlock::Table`, and its model and `random_ops` include Noise, so `the_kernel_matches_the_model_bit_for_bit` covers it. `param_sweep_test` gains:

```rust
#[test]
fn formant_fixture_level() {
    let l = level(Patch::Formant, &[48], 100, BPS);
    println!("FORMANT: {:+.2} dB vs the reference", l.lufs - REFERENCE_LUFS);
    assert!(l.peak.iter().all(|&p| within_ceiling(p)) && l.rms[0] > SILENT_RMS);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_noise_test --test algo_kernel_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with goldens unchanged and `voice_is_its_chain_plus_one_slot` passing.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests
git commit -m "NOIS: band noise ring-modulated to its centre, scaled before its poles; operators latch at note-on"
```

- [ ] **Step 6: QA gate 3.** Run G1–G6, with G5 covering BREATH.

---

# Phase 4 — FRMT

### Task 10: The FRMT grain train (clock-relative, per-grain latch)

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{kernel.rs,engine.rs}`;
  - `chimera-core/tests/{algo_kernel_test.rs,formant_demo_test.rs,param_sweep_test.rs}`.
- Create: `chimera-core/tests/algo_formant_test.rs`.

**Interfaces:**
- Produces:
  ```rust
  pub struct FormantBlock {
      pub inv_inc: f32,               // 1 / inc
      pub inc_c: u32,                 // FC per sample; FC < 24 kHz, so < 2^31
      pub inv_d: f32, pub slack: f32, // latched by a grain that starts in this block
      pub shift: f32,                 // FC / f0: carrier cycles per period of delay
      pub c0: u32,                    // cycles_to_phase(−d·shift/2): the carrier's zero sits at the window's centre
      pub c_ratio: (f32, f32),        // g_c/g_m, ramped into the carrier weight
  }
  struct Grain { c: u32, inv_d: f32, slack: f32, o_base: f32, shift: f32, live: bool }
  struct FormantState { lanes: [Grain; 2], newest: u8, fire_now: bool }
  #[cfg(any(test, feature = "test-support"))] impl AlgoEngine { pub fn grain_cuts(&self) -> u32; }
  ```
- **In `AlgoEngine::render`, per Formant operator:**
  - it computes `formant_law(f0, fc_hz(frmt, vowel, slot, key, pitch), bw_hz(bw), latch.skirt)`;
  - `gain_to` is `level gain · g_m · edge`;
  - the carrier weight is multiplied by the `c_ratio` ramp;
  - it skips `mip_target`;
  - it stores `view[i] = (fc, bw_eff)`.
- **Dispatch:** `run_formant::<N, S>` is specialised on `Skirt` (four variants).
- **Reset:** `Kernel::reset(op, OpKind::Formant)` sets both lanes dead and `fire_now = true`. The first sample fires lane 0 as a restart with `e = 0` and `o_base = 0`. So the reset grain opens at once at every pitch (I2), and every later grain has `o_base = slack/2`.
- **Idle:** `idle` advances the clock and envelope, and kills both lanes.
- **The per-sample algorithm:**
  ```
  let (ph, wrapped) = phase.overflowing_add(inc);          // the unmodulated clock
  phase = ph;
  if wrapped || fire_now {
      let k = 1 - newest;                                  // the lane that fired less recently
      if lanes[k].live && window(read_x(k), S) > 1e-6 { cuts += 1 }   // debug only
      let e = if fire_now { 0.0 } else { ph as f32 * inv_inc };        // samples since the wrap, < 1
      lanes[k] = Grain { c: c0.wrapping_add((e * inc_c as f32) as u32),   // e·inc_c < 2^31: proven range
                         inv_d, slack, o_base: if fire_now { 0.0 } else { slack * 0.5 }, shift, live: true };
      newest = k; fire_now = false;
  }
  let t_new = ph as f32 * (1.0 / 4_294_967_296.0);          // periods since the newest lane's start
  let mut y = 0.0;
  for k in 0..2 {
      let g = &mut lanes[k];
      if g.live {
          let t = if k == newest { t_new } else { 1.0 + t_new };
          let o = (g.o_base + pm_cycles).clamp(0.0, g.slack);      // Task 10: pm_cycles = 0 in the delay
          let x = (t - o) * g.inv_d;
          if x >= 1.0 { g.live = false } else if x > 0.0 {
              y += window(x, S) * sine(g.c.wrapping_add(pm_phase).wrapping_sub(cycles_to_phase(o * g.shift)));
          }
          g.c = g.c.wrapping_add(inc_c);
      }
  }
  out = y · env · gain;   acc += carrier · c_ratio · out;   gain, carrier, c_ratio step
  ```
  In this task the links' PM enters the carrier only (`pm_phase`). Task 11 adds `pm_cycles` to the delay.

- [ ] **Step 1: Write the failing tests.** The shared fixture is `one(OpKind::Formant, frmt, bw, skirt)` on A1.

```rust
const A3: u8 = 57; const C1: u8 = 24; const C2: u8 = 36; const C3: u8 = 48; const C4: u8 = 60; const C5: u8 = 72;
fn f0(n: u8) -> f32 { 440.0 * 2f32.powf((n as f32 - 69.0) / 12.0) }
#[test]
fn harmonics_are_exact() {
    let out = render_algo(&one(OpKind::Formant, 80, 72, 1), A3, 800);
    let s = spectrum_db(&out[BLOCK_SIZE * 100..], 32_768);
    for k in 1..20 { assert!(peak_bin_cents(&s, k as f32 * 220.0).abs() < 0.1); }
    assert!(between_partials_db(&s, 220.0) < -60.0);
}
#[test]
fn formant_peak_stays_put_across_pitch() {
    for n in [C2, C3, C4] {
        let out = render_algo(&one(OpKind::Formant, 80, 120, 0), n, 400);   // BW 1,008 Hz, SKIRT 0: d < 1, single lane
        assert!((grain_fc(&out[BLOCK_SIZE * 50..]) / frmt_hz(80.0) - 1.0).abs() < 0.01, "{n}");
    }
}
#[test]
fn key_100_tracks_the_note() {
    for key in [50u8, 100] { for n in [C3, C4, C5] {
        let mut p = one(OpKind::Formant, 72, 120, 0); p.ops[0].key = key;
        let want = fc_hz(72.0, 50.0, None, key as f32, n as f32);
        assert!((grain_fc(&render_algo(&p, n, 400)[BLOCK_SIZE * 50..]) / want - 1.0).abs() < 0.01);
    } }
}
#[test]
fn first_grain_sounds_at_note_on() {
    for n in [C1, C2, C3, C4, C5] {
        let out = render_algo(&one(OpKind::Formant, 72, 72, 1), n, 2);
        assert!(out[..64].iter().any(|x| x.abs() > 1e-3), "{n}");
    }
}
#[test]
fn level_is_normalised() {
    for s in 0..4u8 {
        for bw in [111u8, 120, 131, 139] { lvl_ok(one_fc(1000.0, bw, s), C3, s); }    // 524 Hz–2 kHz
        for bw in [135u8, 144, 155, 168] { lvl_ok(one_fc(1000.0, bw, s), C4, s); }    // 1,047 Hz–4 kHz
        for fc in [270.0, 300.0] { lvl_ok(one_fc(fc, 72, s), C5, s); }
    }
    // lvl_ok: level_db_vs_r of the held second within ±0.5 dB
}
#[test]
fn peak_never_exceeds_one() {
    for n in [24u8, 36, 48, 60, 72, 84, 96] { for bw in [0u8, 72, 144, 216] { for s in 0..4u8 { for fr in [0u8, 69, 111, 168] {
        let out = render_algo(&one(OpKind::Formant, fr, bw, s), n, 200);
        assert!(out.iter().all(|x| x.abs() <= 1.0), "{n} {bw} {s} {fr}");
    } } } }
    let out = render_algo(&one_fc(1000.0, 216, 1), C3, 400);
    assert!((level_db_vs_r(&out[BLOCK_SIZE * 100..]) + 5.83).abs() < 0.5);   // the ceiling binds
}
#[test]
fn width_floor_keeps_skirt_a_shape() {
    // The formant's envelope, sampled by one fixed harmonic: at A3, BW 0, hold harmonic 5 (1,100 Hz)
    // while FRMT steps across 1,100 Hz ± 700 Hz in stored steps. Its −3 dB width is BW_eff.
    let want = formant_law(220.0, 1100.0, 20.0, Skirt::S1).bw_eff;
    let widths: Vec<f32> = (0..4u8).map(|s| transfer_width(A3, 5, 0, s)).collect();
    for w in &widths { assert!((w / want - 1.0).abs() < 0.10, "{widths:?}"); }
    let (lo, hi) = widths.iter().fold((f32::MAX, 0f32), |(a, b), &w| (a.min(w), b.max(w)));
    assert!(hi / lo < 1.10);
    assert!(transfer_width(C2, 9, 100, 1) > transfer_width(C2, 9, 80, 1));   // below the floor, each BW step widens it
}
#[test]
fn bw_jumps_never_cut_a_grain() {
    for s in 1..4u8 { for off in 0..7 {
        let (out, cuts) = render_algo_with(&one(OpKind::Formant, 80, 0, s), A3, 700, |b, p, _, _| {
            if (b + off) % 7 == 0 { p.ops[0].bw = if p.ops[0].bw == 0 { 144 } else { 0 }; }   // 20 ↔ 2,000 Hz
        });
        assert_eq!(cuts, 0, "{s} {off}");
        assert!(common::clicks(&out).is_empty());
    } }
}
#[test]
fn pitch_jumps_never_cut_a_grain() {
    for s in [1u8, 3] {
        let (_, cuts) = render_algo_with(&one(OpKind::Formant, 80, 0, s), A3, 700, |b, _, _, e| {
            if b % 7 == 0 { e.set_pitch(if b % 14 == 0 { 12.0 } else { 0.0 }); }
        });
        assert_eq!(cuts, 0, "{s}");
    }
}
#[test]
fn high_formants_do_not_alias() {
    for n in [A3, C3] { for fc in [6000.0, 8000.0, 10_000.0] {
        let out = render_algo(&one_fc_skirt(fc, 216, 0), n, 800);
        let grid: Vec<f32> = (1..).map(|k| k as f32 * f0(n)).take_while(|&h| h < 24_000.0).collect();
        assert!(off_grid_db(&out[BLOCK_SIZE * 100..], &grid) < -60.0, "{n} {fc}");   // A3's folds land 40 Hz off the grid
    } }
}
#[test]
fn formant_edge_fades() {
    let b = width_floor(f0(C3)); let m = Skirt::S0.m_q();
    let silent = render_algo(&one_fc_skirt(NYQUIST_HZ - m * b / 2.0, 0, 0), C3, 200);
    assert!(rms(&silent[BLOCK_SIZE * 50..]) < 1e-6);
    let half = render_algo(&one_fc_skirt(NYQUIST_HZ - m * 0.75 * b, 0, 0), C3, 200);
    let full = render_algo(&one_fc_skirt(NYQUIST_HZ - m * 2.0 * b, 0, 0), C3, 200);
    assert!((db(rms(&half[BLOCK_SIZE * 50..]) / rms(&full[BLOCK_SIZE * 50..])) + 6.02).abs() < 0.5);
}
#[test]
fn formant_extremes_stay_finite() {
    for n in [0u8, 127] { for co in [0u8, 63] { for tr in [-24i8, 24] { for fr in [0u8, 168] { for bw in [0u8, 216] { for s in 0..4u8 {
        let mut p = one(OpKind::Formant, fr, bw, s); p.ops[0].coarse = co; p.transpose = tr; p.ops[0].key = 100;
        let out = render_algo(&p, n, 60);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    } } } } } }
}
#[test]
fn retrigger_on_a_sounding_formant_voice_does_not_click() {
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N.min(3) as usize { p.try_set_kind(i, OpKind::Formant).unwrap(); }
    p.alg_a = AlgoId::A1.get();
    let (out, cuts) = render_algo_with(&p, C3, 400, |b, _, _, e| {
        if b == 188 { e.note_on(MidiNote::new(C3).unwrap(), Velocity::new(100).unwrap(), &p, SR); }   // 250 ms
    });
    assert!(common::clicks(&out).is_empty());
    assert_eq!(cuts, 0);
}
#[test]
fn kind_swaps_under_a_held_note_do_not_click() {
    let order = [OpKind::Table(WaveId::W1), OpKind::Formant, OpKind::Noise];
    let (out, _) = render_algo_with(&one(OpKind::Table(WaveId::W1), 80, 72, 1), C3, 1200, |b, p, _, _| {
        if b % 75 == 0 { p.try_set_kind(0, order[(b / 75) % 3]).unwrap(); }
    });
    assert!(common::clicks(&out).is_empty());
}
```

  `algo_kernel_test`'s model gains Formant. `formant_demo_test` gains `demo_clips_do_not_alias`: CHOIR's held A3 chord root has off-grid energy < −60 dB. `formant_fixture_level` also asserts that LUFS is within ±1.5 dB of the value recorded now, as a `const`.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test --test algo_kernel_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 4b: Measure the variants' flash** (G7). Take `just firmware`'s ELF and sum `llvm-nm -S -C` over `Kernel::render_ops` and the new `run_formant`/`run_noise` symbols, against main's 14.5 KB `render_ops`.
  - If the formant variants pass 10 KB, specialise `run_formant` on `S` only, with the link count a runtime loop (4 variants, not 24), and re-measure.
  - Put the delta in the commit body.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests
git commit -m "FRMT: grains on the unmodulated clock, latched per grain, so no BW or pitch change cuts one"
```

### Task 11: PM as a bounded grain delay, the modulator signal, and carrier-only feedback

**Files:**
- Modify: `chimera-engine/src/dsp/algo/kernel.rs`.
- Test: `chimera-core/tests/algo_formant_test.rs`.

**Interfaces:**
- **The delay:** `pm_cycles` is the links' PM only, in cycles: `pm_links / PHASE_UNITS`. It enters `o = (o_base + pm_cycles).clamp(0, slack)`.
- **The carrier:** `pm_phase` = links + feedback. Feedback is `feedback·(prev + hist)` on the operator's own `out`, which already carries `g_m`. It never enters `o`.
- **The two signals:** `out[op]`, the PM source and the feedback, is `y·env·gain`, with `g_m`, so it peaks at 1. The carrier sum takes `c_ratio · out`, which is `y·g_c`.

- [ ] **Step 1: Write the failing tests.**

```rust
fn modded(ratio_coarse: u8, level: u8, frmt: u8, bw: u8, s: u8) -> AlgoParams {
    let mut p = one(OpKind::Formant, frmt, bw, s);
    p.alg_a = AlgoId::T1.get();   // op 2 → op 1
    (p.ops[1].coarse, p.ops[1].level) = (ratio_coarse, level);
    p
}
#[test]
fn pm_never_cuts_a_grain() {
    for c in [4u8, 8] { for s in 0..4u8 {   // ratio 1, ratio 2
        let (out, cuts) = render_algo_with(&modded(c, 99, 80, 72, s), A3, 400, |_, _, _, _| {});
        assert_eq!(cuts, 0); assert!(common::clicks(&out).is_empty()); assert!(out.iter().all(|x| x.abs() <= 1.0));
    } }
}
#[test]
fn pm_enters_grain_and_carrier() {
    let out = render_algo(&modded(4, 83, 80, 0, 0), A3, 800);   // about 1 cycle of PM
    let held = &out[BLOCK_SIZE * 100..];
    let f = frmt_hz(80.0);
    assert!(harmonic_db(held, f + 220.0) - harmonic_db(held, f) > -30.0);   // formant copies at FC ± f_m
    assert!(sideband_energy_db(held, 220.0, 220.0) > -30.0);                 // sidebands round the harmonics
}
#[test]
fn bw_does_not_change_modulation_depth() {
    let idx = |bw: u8| { let mut p = AlgoParams::single(WaveId::W1); p.alg_a = AlgoId::T1.get();
        p.try_set_kind(1, OpKind::Formant).unwrap(); (p.ops[1].frmt, p.ops[1].bw, p.ops[1].level) = (80, bw, 70);
        sideband_ratio_db(&render_algo(&p, A3, 400)[BLOCK_SIZE * 100..], 220.0) };
    let v: Vec<f32> = [0u8, 36, 72, 108, 144, 180, 216].map(idx).into();
    assert!(v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min) < 0.5, "{v:?}");
    // FDBK 5's swing likewise: the op's own spectral centroid varies < 0.5 dB-equivalent across BW
}
#[test]
fn feedback_moves_the_formant_not_the_pitch() {
    let mut p = one(OpKind::Formant, 80, 72, 1);
    let a = render_algo(&p, A3, 400); p.ops[0].feedback = 7; let b = render_algo(&p, A3, 400);
    assert!(fundamental_cents(&b, 220.0).abs() < 0.1);
    assert!(harmonic_db(&b[BLOCK_SIZE * 50..], 2.0 * frmt_hz(80.0)) - harmonic_db(&a[BLOCK_SIZE * 50..], 2.0 * frmt_hz(80.0)) >= 6.0);
}
#[test]
fn a_modulated_high_formant_does_not_alias() {
    let p = modded(4, 99, 158, 72, 0);   // FC ≈ 5 kHz
    let out = render_algo(&p, A3, 800);
    let grid: Vec<f32> = (1..).map(|k| k as f32 * 220.0).take_while(|&h| h < 24_000.0).collect();   // PM at ratio 1 stays on the harmonic grid
    assert!(off_grid_db(&out[BLOCK_SIZE * 100..], &grid) < -60.0);
}
#[test]
fn formant_operators_are_stable() {
    let mut p = AlgoParams::default(); p.alg_a = AlgoId::A17.get();
    p.try_set_kind(0, OpKind::Formant).unwrap();
    for o in &mut p.ops { (o.level, o.feedback, o.frmt, o.bw, o.skirt) = (99, 7, 168, 216, 3); }
    let out = render_algo(&p, C3, 30 * 750);
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(mean(&out).abs() < 1e-3);
    assert!(db(rms(&out[out.len() - 48_000..]) / rms(&out[48_000..96_000])) < 0.5);
    assert!(carrier_peak_before_norm(&p, C3) <= 1.0);
}
#[test]
fn morph_across_a_formant_carrier_is_smooth() {
    let mut p = one(OpKind::Formant, 80, 72, 1);
    (p.alg_a, p.alg_b) = (AlgoId::T1.get(), AlgoId::A1.get());
    p.try_set_kind(1, OpKind::Formant).unwrap(); p.ops[1].level = 90;
    let (out, _) = render_algo_with(&p, C3, 750, |b, _, live, _| live.morph = (b as f32 / 750.0 * 127.0).min(127.0));
    assert!(common::clicks(&out).is_empty());
    assert!(steps_10ms_db(&out).iter().all(|d| d.abs() <= 1.0));
}
#[test]
fn grains_stay_odd_under_pm() {
    let out = render_algo(&modded(4, 99, 34, 0, 1), C5, 800);   // FC ≈ 267 Hz at C5
    assert!(mean(&out[BLOCK_SIZE * 100..]).abs() < 1e-4);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test`. Expected: FAIL on depth, feedback and the formant copies.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src/dsp/algo/kernel.rs chimera-core/tests/algo_formant_test.rs
git commit -m "PM delays FRMT grains within their slack; feedback drives the carrier only"
```

- [ ] **Step 6: QA gate 4.** Run G1–G6, with G5 covering all three clips.

### Task 11b: Make FRMT cheap: spans, hoisting, bit-exact against the per-sample path

This follows Modal 2's Task 11b: block-at-a-time spans, per-block and per-grain work hoisted, and output bit-exact against a per-sample reference.

**Target (owner, 2026-09-30):** `COST_FRMT` (SKIRT 1–3, both lanes live) ≤ 120 by ADR 0056's instruction-count method. That gives N = 3 on today's costs at 75 % (CHAIN 36, bus 1,204/1,494, heavy overhead 472, budget 197). `COST_FRMT_ONE` and `COST_NOIS` are recounted in the same pass.

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{kernel.rs,engine.rs}`;
  - `chimera-core/tests/algo_kernel_test.rs`.
- Create: `chimera-core/tests/algo_formant_perf_test.rs`.

**Interfaces:**
- **The reference path.** Keep Tasks 10–11's per-sample loop as `fn formant_tick_ref`, under `#[cfg(any(test, feature = "test-support"))]`.
- **`run_formant::<N, S>` renders in spans:**
  - `m = min(samples left in the block, samples to the next clock wrap)`, where the samples to the wrap are `(u32::MAX − phase) / inc + 1`, integer. So the wrap test and the restart leave the inner loop.
  - Per span, each live lane's latched `inv_d`, `slack`, `o_base` and `shift` sit in registers.
  - With `N == 0` (no incoming links), `o` is constant per grain. So `cycles_to_phase(o·shift)` and `o·inv_d` are hoisted to the grain start, and each lane's open interval `[x > 0, x < 1)` is computed as sample indices. The inner loop then runs only the open samples, with no liveness test.
  - With `N > 0`, only the delay and its phase stay per sample.
- **Unchanged:** the sine read (W1 mip 0, one table, no crossfade) and the squaring chain, which is straight-line per `S`.
- **`run_noise::<N>`** hoists its ramps' deltas, and its `rng` and poles stay in registers across the block.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn spans_are_bit_exact_against_the_per_sample_path() {
    for seed in 0..200u32 {
        let (p, note, blocks, jumps) = random_formant_case(seed);   // kinds, SKIRT, BW, FRMT, PM links, BW and pitch jumps
        let (fast, cuts_fast) = render_algo_with(&p, note, blocks, jumps.clone());
        let (slow, cuts_slow) = render_algo_ref(&p, note, blocks, jumps);   // formant_tick_ref throughout
        assert!(fast.iter().zip(&slow).all(|(a, b)| a.to_bits() == b.to_bits()), "seed {seed}");
        assert_eq!(cuts_fast, cuts_slow);
    }
}
#[test]
fn frmt_meets_its_target_or_falls_back_to_two() {
    assert!(AlgoEngine::COST_FRMT.0 <= 120 || FORMANT_N == 2);
    assert_eq!(FORMANT_N >= 3, AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_OP.0 <= FORMANT_BUDGET.0 / 3);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_perf_test`. Expected: FAIL, because `render_algo_ref` isn't defined.
- [ ] **Step 3: Implement the spans.** Every existing formant and kernel test stays green, and `the_kernel_matches_the_model_bit_for_bit` is unchanged.
- [ ] **Step 4: Count.** Build the `bench-algo` area (`cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features --features bench-algo`), with `#[cfg_attr(feature = "bench", inline(never))]` on `run_formant` and `run_noise` added here. Then run `python3 tools/loop_count.py <elf> run_formant` and `python3 tools/loop_count.py <elf> run_noise`, which give ADR 0056's count per inner loop: `ceil5(instr × 1.46 × 1.1 + 12 × divides)`.
  - Count the per-sample instructions of the inner span loops (`<0, S0>`, `<0, S3>`, `<1, S3>`, `run_noise<0>`) against `run_one<0>`.
  - Each term is `COST_OP + Δ × 1.46 + per-block and per-grain work over 64`, plus 10 %, rounded up to 10.
  - Bill `COST_FRMT` at the costlier of `<0, S3>` and `<1, S3>`.
  - Set `COST_FRMT_ONE`, `COST_FRMT` and `COST_NOIS`, with "estimate, ADR 0056's method, <commit>" beside each.
- [ ] **Step 5: The outcome.** `FORMANT_N` follows at build time.
  - **N = 3:** CHOIR and TALK BASS take the spec's three-formant voicings.
  - **N = 2:** they take the two-formant voicings (Task 5), and the plan continues (owner, 2026-09-30).
  - Put the counts, the terms and N in the commit body. Task 15's ADR records both the target and the outcome.
- [ ] **Step 6: Run the tests to verify they pass.** Run `just check`. `formant_budget_keeps_four_voices` must still hold at the new terms.
- [ ] **Step 7: Commit.**

```bash
git add chimera-engine/src/dsp/algo chimera-core/tests/algo_formant_perf_test.rs chimera-core/tests/algo_kernel_test.rs
git commit -m "FRMT renders in spans between grain starts; terms recounted"
```

- [ ] **Step 8: QA gate 4b.** Run G1–G6. G4 checks the floor at the new terms, and G5 renders whichever voicings N chose.

---

# Phase 5 — Modulation

### Task 12: VOWEL, FRMT and BW are live mod destinations

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{engine.rs,params.rs}`;
  - `chimera-core/tests/modulatable_test.rs`.
- Test: `chimera-core/tests/algo_formant_test.rs`.

**Interfaces:**
- **`AlgoLive`** gains `vowel: f32`, `frmt: [f32; OPS]` and `bw: [f32; OPS]`, filled by `from_params`.
  - `slot` gains the three arms, so the Part's knobs glide them (ADR 0112, as FINE and DETUNE).
  - `offset` gains them too (ADR 0010's formula).
- **`render`** reads the live values wherever Tasks 9–11 read stored ones.
- **Specs:** FRMT, BW and VOWEL flip to `modulatable: true` here.
- **`modulatable_test::recipe`:**
  - for `(AlgoOp(n), FRMT | BW)`, operator *n* is FRMT, with a second case as NOIS;
  - for `(Algo, VOWEL)`, operators 1 and 2 are FRMT and op 3 is FRMT if `FORMANT_N` ≥ 3 (else NOIS), on A1.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn vowel_places_each_operator_on_its_vowel() {
    for (v, row) in [0u8, 25, 50, 75, 100].into_iter().zip(VOWEL_HZ) {
        for (i, s) in [VowelSlot::F1, VowelSlot::F2, VowelSlot::F3].into_iter().enumerate() {
            let mut p = one(OpKind::Formant, AlgoParams::default().ops[i].frmt, 120, 0);
            (p.ops[0].vslot, p.vowel) = (s.code(), v);
            let fc = grain_fc(&render_algo(&p, C2, 400)[BLOCK_SIZE * 50..]);
            assert!((1200.0 * (fc / row[i]).log2()).abs() < 50.0, "{v} {s:?}");
        }
    }
}
#[test]
fn a_slot_edit_while_held_moves_nothing_until_the_next_note() {
    let mut p = one(OpKind::Formant, 83, 120, 0); (p.ops[0].vslot, p.vowel) = (VowelSlot::F2.code(), 100);
    let (out, _) = render_algo_with(&p, C2, 400, |b, p, _, _| if b == 200 { p.ops[0].vslot = VowelSlot::Off.code() });
    assert!((grain_fc(&out[BLOCK_SIZE * 300..]) / grain_fc(&out[BLOCK_SIZE * 100..BLOCK_SIZE * 200]) - 1.0).abs() < 0.01);
}
#[test]
fn level_holds_across_the_vowels_at_c5() {
    for s in 0..4u8 { for v in [0u8, 25, 50, 75, 100] {
        let mut p = one(OpKind::Formant, 69, 72, s); (p.ops[0].vslot, p.vowel) = (VowelSlot::F1.code(), v);
        assert!(level_db_vs_r(&render_algo(&p, C5, 400)[BLOCK_SIZE * 100..]).abs() < 0.5, "{s} {v}");
    } }
}
#[test]
fn vowel_sweeps_do_not_click() {
    for kind in [OpKind::Formant, OpKind::Noise] {
        let mut p = one(kind, 83, 72, 1); p.ops[0].vslot = VowelSlot::F2.code();
        let (sweep, _) = render_algo_with(&p, C3, 300, |b, _, live, _| live.vowel = (b as f32 * 100.0 / 37.5).min(100.0));   // 50 ms
        let (jump, _) = render_algo_with(&p, C3, 750, |b, _, live, _| live.vowel = if (b / 94) % 2 == 0 { 0.0 } else { 100.0 });   // 4 Hz square
        assert!(common::clicks(&sweep).is_empty() && common::clicks(&jump).is_empty(), "{kind:?}");
    }
}
#[test]
fn width_square_lfo_does_not_burst() {
    let p = nois(104, 183, 0);
    let steady = rms(&render_algo(&nois(104, 0, 0), 48, 1500)[BLOCK_SIZE * 750..]);
    let (out, _) = render_algo_with(&p, 48, 1500, |b, _, live, _| live.bw[0] = if (b / 94) % 2 == 0 { 183.0 } else { 0.0 });
    assert!(worst_10ms_db(&out, steady) <= 8.0 && out.iter().all(|x| x.abs() <= 1.0));
}
#[test]
fn bw_square_lfo_never_cuts_a_grain() {
    let (_, cuts) = render_algo_with(&one(OpKind::Formant, 80, 0, 1), A3, 750, |b, _, live, _| live.bw[0] = if (b / 7) % 2 == 0 { 0.0 } else { 144.0 });
    assert_eq!(cuts, 0);
}
#[test]
fn a_route_into_a_table_operators_frmt_does_nothing() {
    let p = AlgoParams::default();
    let (a, _) = render_algo_with(&p, C3, 200, |_, _, _, _| {});
    let (b, _) = render_algo_with(&p, C3, 200, |b, _, live, _| { live.frmt[0] = (b % 168) as f32; live.bw[0] = 216.0; });
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test algo_formant_test --test modulatable_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src/dsp/algo chimera-core/tests
git commit -m "VOWEL, FRMT and BW take modulation every block"
```

- [ ] **Step 6: QA gate 5.** Run G1–G6.

---

# Phase 6 — Pages

### Task 13: The VOWEL cell, dimming, the FORMANT group page, and WAVE's glyph

**Files:**
- Modify:
  - `chimera-core/src/ui/{block_registry.rs,nav.rs,view.rs,glyph.rs}`;
  - `chimera-engine/src/dsp/algo/kinds.rs` (`reads`);
  - `chimera-core/tests/{all_pages_walk_test.rs,screen_golden_test.rs,page_identity_test.rs,binding_test.rs,focus_glyph_test.rs}`.

**Interfaces:**
- `ALGO_ALG.params[5]` (`EMPTY` on main) = `ParamSlot::param(BlockRef::Algo, AlgoParams::VOWEL)`.
- `pub static ALGO_FORMANT: BlockDef` = `group(<next id>, "Formant", "FRM", AlgoOpParams::FRMT)`.
- `ALGO_FORMANT_CHAIN` is `ALGO_CHAIN` with `&ALGO_FORMANT` first in OSC's sub-pages.
- `PartChain::AlgoFormant`. `PartChain::of` returns it when `sound.params.algo.kinds().any_formant()`.
- **WAVE's glyph.** `ui/glyph.rs`'s `FocusGlyph::Wave` gauge takes `OpKind::clamped(v)`, not `WaveId::clamped(v)`, which today would draw SSAW for codes 16 and 17.
  - A table wave draws `wave_trace` as today.
  - FRMT draws `op_page::grain_curve(Skirt::S1, 4.0)`, and NOISE a fixed band. Both are drawn in theme tokens.
  - Rebase on `va-glyphs` first if it has landed.
- **`kinds::reads(kind, id) -> bool`** is a pure function, as `modal::reads` and `tape_engine::reads` are. `view::dimmed` gains `(BlockRef::AlgoOp(op), id) => !kinds::reads(sound.params.algo.kinds().get(op.index()), id)`.
- **New `view::dimmed` arms:**
  - `(Algo, VOWEL)` when there is no formant kind;
  - `(AlgoOp(n), FRMT | BW | SKIRT | KEY | RES | VSLOT)` for Table, on the group pages; OP pages never show them;
  - `(AlgoOp(n), SKIRT | KEY)` for NOIS, and `RES` for FRMT;
  - `(AlgoOp(n), COARSE | FINE | DETUNE | FEEDBACK)` for NOIS.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn pages_adapt_to_formant_kinds() {
    // `walker`: the screen harness's UiState on Part 1; `page_id()` is the shown BlockDef's id.
    let mut w = walker(Sound::init(EngineType::Algo));
    assert_eq!(PartChain::of(w.sound()), PartChain::Algo);
    assert!(PartChain::Algo.def().locate(ALGO_FORMANT.id).is_none());
    assert!(dimmed(ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL), w.sound()));
    w.goto(ALGO_DETUNE.id);
    w.set_kind(Op::A, OpKind::Formant);                       // a WAVE edit through the encoder path
    assert_eq!(w.page_id(), ALGO_DETUNE.id);
    let at = PartChain::AlgoFormant.def().locate(ALGO_FORMANT.id).unwrap();
    assert_eq!((at.node(), at.sub()), (1, 1));
    assert!(dimmed(ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::FRMT), w.sound()));
    assert!(!dimmed(ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL), w.sound()));
    w.goto(ALGO_FORMANT.id);
    w.set_kind(Op::A, OpKind::Table(WaveId::W1));
    assert_eq!(w.page_id(), ALGO_WAVE.id);
}
#[test]
fn nois_dims_what_it_ignores() {
    let mut s = Sound::init(EngineType::Algo); s.params.algo.try_set_kind(0, OpKind::Noise).unwrap();
    for id in [AlgoOpParams::COARSE, AlgoOpParams::FINE, AlgoOpParams::DETUNE, AlgoOpParams::FEEDBACK, AlgoOpParams::SKIRT, AlgoOpParams::KEY] {
        assert!(dimmed(ParamAddr::new(BlockRef::AlgoOp(Op::A), id), &s));
    }
}
#[test]
fn the_all_pages_walk_includes_frm_exactly_when_a_formant_exists() {
    assert!(!walk(Sound::init(EngineType::Algo)).contains(&ALGO_FORMANT.id));
    assert!(walk(demo_sound(Demo::Breath)).contains(&ALGO_FORMANT.id));
}
```

  `binding_test` covers `ALGO_FORMANT_CHAIN` as it covers `ALGO_CHAIN`. `focus_glyph_test` covers WAVE's gauge at codes 16 and 17. Re-record exactly `engine_algo`, `algo_alg`, `algo_alg_morph_dimmed` and `algo_out_p3`, and add `algo_formant_page` (its light twin follows).
- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test page_identity_test --test all_pages_walk_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS, with only those four goldens re-recorded.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "VOWEL takes ALGO's sixth cell; a FORMANT page joins OSC when an operator is a formant"
```

### Task 14: OP page cells by kind, and the live grain and band viz

**Files:**
- Modify:
  - `chimera-core/src/ui/{op_page.rs,block_def.rs,view.rs,viz.rs,block_registry.rs,mod.rs}`;
  - `chimera-engine/src/dsp/algo/engine.rs`;
  - `chimera-engine/src/instrument.rs`;
  - `chimera-board/src/shared.rs` (the new triple buffer, beside `ORBIT`'s) and the board's UI loop.
- Test:
  - `chimera-core/tests/op_page_test.rs`;
  - `chimera-core/tests/screen/` (goldens `algo_op_page_frmt`, `algo_op_page_nois`).

**Interfaces:**
- Produces:
  ```rust
  pub fn page_cells(kind: OpKind) -> [Option<(ParamId, &'static str)>; 6];   // as modal::page_cells
  //   Table: WAVE COARSE FINE LEVEL FDBK VELOCITY; Formant: WAVE FORMANT BW SKIRT KEY SLOT;
  //   Noise: WAVE CENTRE WIDTH RES SLOT LEVEL (full words where they fit 62 px; FEEDBACK is 65)
  pub fn grain_curve(s: Skirt, fc_l: f32) -> [i8; 64];
  pub fn band_curve(fc: f32, bw: f32) -> [u8; 64];      // 20 Hz–20 kHz log axis
  impl AlgoEngine { pub fn formant_view(&self) -> [(f32, f32); OPS]; }   // Task 9's `view`
  pub struct FormantView { pub fc: [f32; OPS], pub bw_eff: [f32; OPS], pub sounding: bool }
  ```
- `ALGO_OP_PAGES[n]`'s cells become `SlotBinding::OpPanel(op, k)`, resolved in `view::view` through `page_cells(ctx.op_kinds[op])`, as `SlotBinding::ModalPanel` is through `modal::page_cells`.
- `SlotCtx` gains `op_kinds: [OpKind; OPS]`, read in `SlotCtx::read`.
- `ParamSlot::spec`, `label` and `format` return the Table cell's values for `OpPanel`. Every path that edits, primes (MIX + PLUS) or focuses a cell goes through `view::view(def, i, &ctx)`.
- **Telemetry, audio to UI.** The Instrument publishes `FormantView` for the edited Part's most recently started voice once per block. It goes through a `TripleBuffer<FormantView>` in `chimera-board/src/shared.rs`, as `OrbitView` does.
  - `AudioShared` runs UI to audio, so it isn't the place.
  - The edited Part's index already reaches the audio side.
  - `memory_budget_test`'s `axi_report` gains its line, beside `screensaver`'s.
- **The viz:** it uses the published values when `sounding`, and otherwise `formant_law` at C4 on the stored params. FRMT draws `grain_curve` and prints FC and BW_eff. NOIS draws `band_curve` and prints its centre.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn op_pages_show_each_kinds_cells() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.try_set_kind(1, OpKind::Formant).unwrap();
    s.params.algo.try_set_kind(2, OpKind::Noise).unwrap();
    let ctx = SlotCtx::read(&s.params, Op::A);
    let labels = |n: usize| (0..6).map(|i| view(&ALGO_OP_PAGES[n], i, &ctx).label()).collect::<Vec<_>>();
    assert_eq!(labels(0), ["WAVE", "COARSE", "FINE", "LEVEL", "FDBK", "VELOCITY"]);
    assert_eq!(labels(1), ["WAVE", "FORMANT", "BW", "SKIRT", "KEY", "SLOT"]);
    assert_eq!(labels(2), ["WAVE", "CENTRE", "WIDTH", "RES", "SLOT", "LEVEL"]);
    for n in 0..3 { for l in labels(n) {
        assert!(draw::text_width(&theme::FONT_LABEL, l, theme::LABEL_TRACKING) <= theme::CELL_BAR_W, "{l}");
    } }
}
#[test]
fn a_kind_change_on_an_op_page_keeps_it() {
    let mut w = walker(Sound::init(EngineType::Algo));
    w.goto(ALGO_OP_PAGES[1].id);
    w.turn_wave_to(Op::B, OpKind::Formant);                   // the OP page's own WAVE cell
    assert_eq!(w.page_id(), ALGO_OP_PAGES[1].id);              // the list gained FORMANT: refocused
    assert_eq!(w.cell_label(1), "FORMANT");
    w.turn_wave_to(Op::B, OpKind::Table(WaveId::W1));
    assert_eq!((w.page_id(), w.cell_label(1)), (ALGO_OP_PAGES[1].id, "COARSE"));
    w.load_project_with(Sound::init(EngineType::Modal));      // project_replaced: the engine changed
    assert_eq!(w.page_id(), MODAL_1.id);                       // MODAL's home (ADR 0066)
}
#[test]
fn grain_curve_is_one_odd_grain() {
    let g = grain_curve(Skirt::S1, 4.0);
    assert_eq!((g[0], g[63]), (0, 0));
    for i in 0..32 { assert!((g[i] as i16 + g[63 - i] as i16).abs() <= 1); }
}
#[test]
fn the_viz_follows_the_live_voice() {
    let mut w = walker(demo_sound(Demo::TalkBass));
    w.goto(ALGO_OP_PAGES[1].id);
    w.hold_note(36); let low = w.viz_fc(); w.release(); w.hold_note(60); let high = w.viz_fc();
    assert_eq!(low, high);           // KEY 0 in TALK BASS
    w.set_param(Op::B, AlgoOpParams::KEY, 100.0); w.hold_note(72);
    assert!(w.viz_fc() > high * 1.9);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test op_page_test`. Expected: FAIL.
- [ ] **Step 3: Implement**, and record the two new goldens only.
- [ ] **Step 4: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests
git commit -m "OP pages swap in each kind's cells and draw the live grain or band"
```

- [ ] **Step 6: QA gate 6.** Run G1–G7, then `just desktop`: walk every page of `Patch::Formant`'s Sound with a note held, in dark and in INVERT.

---

# Phase 7 — Costs and the ADR

### Task 15: Recount the terms, add the bench rows, and write ADR F

**Files:**
- Modify:
  - `chimera-engine/src/dsp/algo/{engine.rs,kernel.rs}`;
  - `chimera-stm32/src/bench.rs`;
  - `chimera-core/tests/{cost_test.rs,memory_budget_test.rs}`;
  - `docs/adr/README.md`.
- Create: `docs/adr/<F>-formant-operators-are-algo-kinds.md`.

**Interfaces:**
- **Bench rows**, in `chimera-stm32/src/bench.rs`'s `bench-algo` area (one area a build):
  - `"FRMT0"`: one FRMT, SKIRT 0, BW 0;
  - `"FRMT3"`: SKIRT 3, BW 0;
  - `"FRMT PM"`: FRMT3 with a full ratio-1 modulator;
  - `"NOIS"`: one NOIS;
  - `"FRMT MAX"`: A16/A17 with `FORMANT_N` FRMT, the NOIS the budget still allows, table operators in the rest, and feedback where billed;
  - `"CHOIR"`: `demo_sound(Demo::Choir)`.
- `run_formant` and `run_noise` already carry `#[cfg_attr(feature = "bench", inline(never))]` (Task 11b).

- [ ] **Step 1: Write the failing tests.**

```rust
fn voices(p: &AlgoParams, bus: Cost, extra: u32) -> u32 {
    (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - bus.0) / (AlgoEngine::cost(p, &[false; OPS]).0 + extra)
}
#[test]
fn formant_costs_bill_by_kind() {
    let d = |x: Demo| demo_sound(x).params.algo;
    assert_eq!(AlgoEngine::cost(&d(Demo::Breath), &[false; OPS]).0, AlgoEngine::COST_BASE.0 + 2 * AlgoEngine::COST_NOIS.0);
    let floor = HEAVY_OVERHEAD.0;
    let worst = frmt_max();   // the FRMT MAX row's patch
    assert!(voices(&worst, FxBus::COST_TAPE, floor) >= FLOOR_VOICES && voices(&worst, FxBus::COST_NO_TAPE, floor) >= FLOOR_VOICES);
    for (name, p) in [("BREATH", d(Demo::Breath)), ("CHOIR", d(Demo::Choir)), ("TALK BASS", d(Demo::TalkBass)), ("MAX", worst)] {
        println!("{name}: rev V {} / rev Y {}", voices(&p, FxBus::COST, ModRouting::BASE.0 + Voice::CHAIN_COST.0),
                 (SampleBudget::for_cpu(CPU_HZ_REV_Y).as_cost().0 - FxBus::COST.0) / (AlgoEngine::cost(&p, &[false; OPS]).0 + ModRouting::BASE.0 + Voice::CHAIN_COST.0));
    }
}
#[test]
fn the_costliest_table_patch_still_gets_six_voices_on_rev_v() {
    assert!(voices(&costliest(), FxBus::COST, ModRouting::BASE.0 + Voice::CHAIN_COST.0) >= 6);
}
#[test]
fn the_algo_engine_still_fits_its_slot() {
    assert!(size_of::<AlgoEngine>() <= size_of::<ModalEngine>());
    assert_eq!(size_of::<Voice>(), VOICE_SIZE_BEFORE);   // 6,304 on main at 4473f0a1; re-read at rebase
}
#[test]
fn held_skirt_extra_bills_a_latched_two_lane_grain() {
    let mut p = one(OpKind::Formant, 80, 72, 3);
    let mut e = AlgoEngine::new(); e.note_on(MidiNote::new(48).unwrap(), Velocity::new(100).unwrap(), &p, SR);
    p.ops[0].skirt = 0;
    assert_eq!(e.held_skirt_extra(&p).0, AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_FRMT_ONE.0);
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test cost_test --test memory_budget_test`. Expected: FAIL until `frmt_max` and the size literal are added.
- [ ] **Step 3: Recount from the release disassembly.**

```bash
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features --features bench-algo
elf=target/thumbv7em-none-eabihf/release/chimera-stm32
python3 tools/loop_count.py "$elf" run_formant
python3 tools/loop_count.py "$elf" run_noise
python3 tools/loop_count.py "$elf" run_one
```

  Count the per-sample loop body instructions of `run_formant<1, S0>`, `run_formant<1, S3>` (both lanes live) and `run_noise<1>`, against `run_one<1>`.
  - Each term is `COST_OP + Δ × 1.46 + per-block/64`, plus 10 %, rounded up to 10 (`loop_count.py` prints each loop's `ceil5`).
  - This re-checks Task 11b's counts against the final code, with bench rows added.
  - Update any term that moved. `FORMANT_BUDGET` and N follow.
  - If N changes from Task 11b's outcome, the demo voicings switch at compile time. Rerun G5, and note it in the ADR.
- [ ] **Step 4: Write ADR F**, following the spec's § ADR:
  - the provenance split, including FSVR's measured skirt law and its GPL status;
  - `OpKind`, codes and VOWEL SLOT;
  - the −12 dB reference, the ceiling and the peak-referenced modulator;
  - the SKIRT-free floor, per-grain BW and the extended curves;
  - the terms, with the counts, the derived budget and N, Task 11b's outcome (N = 3 with its target met, or the N = 2 fallback, with the counts and the two-formant voicings if it fell back), and the voice table printed by `formant_costs_bill_by_kind`;
  - `Kinds` and the heavy-route case;
  - the clean-peak decision;
  - the partial supersessions of ADRs 0022, 0026 and 0031.

  Take the number as Global Constraints says, and add the README row.
- [ ] **Step 5: Run the tests to verify they pass.** Run `just check`. Expected: PASS.
- [ ] **Step 6: Commit.**

```bash
git add chimera-engine/src chimera-core/src chimera-core/tests chimera-stm32/src/bench.rs docs/adr/README.md docs/adr/<F>-formant-operators-are-algo-kinds.md
git commit -m "Formant cost terms recounted from the release build; bench rows; the formant ADR"
```

- [ ] **Step 7: QA gate 7.** Run G1–G6.

---

# Phase 8 — The factory Sounds and the UAT list

### Task 16: CHOIR, TALK BASS and BREATH ship in the bank

**Files:**
- Modify:
  - `chimera-engine/src/factory.rs`;
  - `chimera-engine/src/factory.bin` (regenerated);
  - `chimera-core/tests/{factory_test.rs,cost_test.rs,formant_demo_test.rs,codec_compat_test.rs,param_sweep_test.rs,factory_level_test.rs}`.

**Interfaces:**
- `FACTORY_LEN` becomes 11, and `factory_sound(8..=10)` = `demo_sound(Choir | TalkBass | Breath)`.
  - The `authored` module lists them after the eight, and `GOLDEN_RECORD=1` rewrites `factory.bin`.
  - The INIT entries follow them, so `new_sound`'s indices past 8 shift by 3.
  - `blob_matches_authored`, `new_holds_the_blob_bytes` and `the_ui_starts_with_the_bank_in_the_pool` cover it.
- `v1_fixtures_equal_factory` and `factory_level_test` use `V1_FACTORY = 8`, and `eight_algo_sounds_with_distinct_names` becomes the test below.
- Only `OutParams::VOLUME` is tuned.
- The existing six-voice test covers Sounds 0–7. The formant Sounds are held to the floor.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn formant_sounds_meet_the_loudness_reference() {
    for i in 8..11u8 {
        let l = level(Patch::Factory(i), &[60], 100, BPS);
        assert!((l.lufs - REFERENCE_LUFS).abs() <= 1.0, "F{i}: {:+.2}", l.lufs - REFERENCE_LUFS);
        let c = level(Patch::Factory(i), &CHORD, 127, BPS);
        assert!(c.limited_db <= MAX_CHORD_LIMITED_DB && c.peak.iter().all(|&p| within_ceiling(p)));
    }
}
#[test]
fn eleven_sounds_with_distinct_names() {
    let names: Vec<String> = (0..FACTORY_LEN).map(|i| factory_sound(i).unwrap().name.as_str().to_string()).collect();
    let mut u = names.clone(); u.sort(); u.dedup();
    assert_eq!((FACTORY_LEN, u.len()), (11, 11));
    assert_eq!(&names[8..], ["CHOIR", "TALK BASS", "BREATH"]);
}
#[test]
fn formant_sounds_keep_the_floor_with_their_routes() {
    for i in 8..11 {
        let s = factory_sound(i).unwrap();
        let v = (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - FxBus::COST_TAPE.0) / Voice::cost(&s.params, &s.mod_state).0;
        assert!(v >= FLOOR_VOICES, "F{i}: {v}");
    }
}
#[test]
fn formant_sounds_round_trip() {
    for i in 8..11 { let s = factory_sound(i).unwrap(); assert!(decode(&encode(&s)).unwrap().bits_eq(&s)); }
}
```

  `demo_clips_are_clean` gains the same loudness check on each phrase.
- [ ] **Step 2: Run them to see them fail.** Run `cargo test --release -p chimera-core --test factory_test --test param_sweep_test --test formant_demo_test --test cost_test`. Expected: FAIL.
- [ ] **Step 3: Implement.** Tune VOL in whole steps. Stop for the owner if VOL tops out (Ruling 9).
- [ ] **Step 4: Run the tests and QA gate 8.** Run `just check` and G1–G6. Render and listen to the three clips, and put their click, level and alias numbers in the PR.
- [ ] **Step 5: Commit.**

```bash
git add chimera-engine/src/factory.rs chimera-engine/src/factory.bin chimera-core/tests
git commit -m "CHOIR, TALK BASS and BREATH join the factory bank at the loudness reference"
```

- [ ] **Step 6: Hand the owner the UAT list,** with the three WAVs, and open the PR for `formant-ops`.

---

# Phase 9 — Ship

### Task 17: The shared ship flash

This is one sitting at the chip, per the hardware-at-ship rule.

- [ ] **Step 1: The bench image.** Claude flashes the `bench-algo` build (`just flash-bench algo`, no jumper) and reads FLOOR, FRMT0, FRMT3, FRMT PM, NOIS, FRMT MAX and CHOIR over the USB console (`just usb bench`, ADR 0068).
- [ ] **Step 2: The ship image.** In the same sitting, Claude flashes `just firmware`, carrying whatever else ships in this flash. The owner runs the UAT below by ear.
- [ ] **Step 3: Update the terms.**
  - Solve `COST_FRMT_ONE` from FRMT0 − FLOOR, `COST_FRMT` from FRMT3 − FLOOR, and `COST_NOIS` from NOIS − FLOOR, each rounded up to 10. Write "measured <date>, bench, rev V at 480 MHz" beside each.
  - FRMT PM must be ≤ FRMT3 + 10, and FRMT MAX ≤ its model bill; otherwise file an issue and stop.
  - If N changes, the voicings follow at compile time. Rerun G5 on the desktop, and file an issue if CHOIR or TALK BASS changes voicing after the owner's UAT.
  - Change ADR F's estimate lines, since it is still Proposed.
- [ ] **Step 4: Commit.**

```bash
git add chimera-engine/src/dsp/algo/engine.rs docs/adr/<F>-formant-operators-are-algo-kinds.md
git commit -m "Formant cost terms measured on the bench"
```

## Owner UAT (one list, at the ship flash; musical judgments only)

1. **CHOIR:** does it read as a choir pad? Is the O–A drift heard as vowels, with width from the doubled F2?
2. **TALK BASS:** does each note say "wow"? Is the body solid, with no flam at the start?
3. **BREATH:** airy and breathy, with no tone and a clean release.
4. **VOWEL by hand on CHOIR:** a smooth U → O → A → E → I, with each vowel recognisable.
5. **The formant Sounds beside the FM factory Sounds:** do they sit at the same loudness?
6. **OP1–OP6 at the end of OSC's sub-row:** easy to reach with EDIT and SEQ? Is the page you were on kept when a Sound loads?
7. **The FORMANT page:** it appears as you set a WAVE to FRMT or NOISE, and "FORMANT BUDGET FULL" is clear when WAVE skips.
9. **The labels:** FORMANT, CENTRE, COARSE and VELOCITY in full; BW, RES, SLOT and FDBK short because the words don't fit. Do they read well, in dark and in INVERT?
8. **The OP page viz:** does the grain or band follow the note and VOWEL as you play?
