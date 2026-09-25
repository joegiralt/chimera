# Issue Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the small, well-understood GitHub issues (#1, #7, #11, #15, #21 and the test half of #10) in one branch, one commit per issue.

**Architecture:** Independent fixes to existing code; no new subsystems. Order matters only where tasks share files (`ui/mod.rs`, `ui/mod_grid.rs`).

**Tech Stack:** Rust `no_std` workspace (chimera-core, chimera-hal, chimera-desktop, chimera-stm32).

**Spec:** none. The GitHub issues are the requirements (copies are in `/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/issues/<N>.md`).

## Global Constraints

- CLAUDE.md rules: no `unsafe` without `// SAFETY:`; no heap allocation or blocking in the audio path; no libc; parameter changes are lerped, never snapped.
- Build and test with: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`. Baseline: all pass (about 521 core+hal tests, 3 ignored; desktop 2) and the firmware links.
- Audio goldens must stay bit-identical unless a task says otherwise. Screen goldens are re-locked only where a task changes a page on purpose; regenerate the PNGs with `just screens` in that case.
- Never stage `docs/chimera-ui-ux-spec.md` (it holds unrelated uncommitted edits).
- One commit per task, message `fix(...)`/`refactor(...)`/`test(...)`/`chore(...)` naming the issue, e.g. `(#15)`.
- Follow up on anything out of scope by noting it in the report; do not file issues.

## Review Focus

- Matrix cursor past the last destination after a Part switch or un-prime: an encoder turn must not create a phantom route (#11).
- Un-priming a middle destination: the remaining routes keep their own amounts (#11).
- MIX+PLUS on a non-modulatable parameter, on an already-routed one, and with the matrix full: each gives its own message, and the message goes away when the focus changes (#21).
- Idle browser: zero flushes per frame; moving the cursor redraws (#7).
- `KsRenderParams` refactor: Modal goldens identical (#1).

---

### Task 1: Hygiene part 1 — SAFETY comments, warnings, clippy (#15)

**Files:** `chimera-core/src/ui/fmt.rs:22`, `chimera-core/src/ui/scope.rs:24`, `chimera-core/src/dsp/mod.rs:12,14`, `chimera-core/src/dsp/fm_tables.rs:15-16`, `chimera-stm32/src/main.rs:11,22`, `chimera-stm32/src/audio.rs:151`, `chimera-stm32/src/controls.rs:217`, plus whatever `just clippy` reports.

- [ ] `fmt.rs:22`: `// Safe:` → `// SAFETY:` with a real justification. `scope.rs:24`: add a `// SAFETY:` comment to the bare `unsafe` block.
- [ ] `dsp/mod.rs`: replace the hand-written PI/TAU literals with `core::f32::consts::{PI, TAU}`.
- [ ] `fm_tables.rs`: 3.14 and 6.28 are TX81Z frequency ratios, not pi. Keep the values; add `#[allow(clippy::approx_constant)]` with a one-line comment saying so.
- [ ] Add `Default` impls (delegating to `new()`) where clippy asks for them (`SoundPool`, `RegionSet`, `MatrixState`, …).
- [ ] Fix the 4 firmware warnings (`cargo build -p chimera-stm32 --target thumbv7em-none-eabihf` must be warning-free).
- [ ] Make `just clippy` pass (`-D warnings`). Prefer real fixes; `#[allow]` only with a comment, and never to hide a bug. Keep `#[allow(clippy::too_many_arguments)]` on `KsString::tick_full` (Task 3 removes it).
- [ ] No behaviour change: `just check` green, all goldens unchanged. Do not run `cargo fmt` (Task 8 does).
- [ ] Commit: `chore: SAFETY comments, clippy clean, firmware warnings (#15)`.

### Task 2: Modal tail test replaces the ignored note-off gate (#10, test half)

**Files:** `chimera-core/tests/sanity_test.rs` (around lines 58-97).

- [ ] Replace the ignored `modal_is_silent_after_note_off` with a physics-correct test: after note-off, the Modal init patch's block RMS trends down (e.g. RMS of later windows < RMS of earlier windows, measured over several windows, not a strict per-block monotone check), and a patch with higher damping (or shorter decay, whichever param the Modal block exposes) falls below a threshold sooner than the default.
- [ ] `modal_is_pitched` stays `#[ignore]` pointing at #10 (the body-comb octave bug is a design decision left to the user).
- [ ] Commit: `test(core): Modal tail decays after note-off, faster with more damping (#10)`.

### Task 3: `KsRenderParams` for `KsString::tick_full` (#1)

**Files:** `chimera-core/src/dsp/modal.rs` (`tick_full` at ~371-383; call sites ~757, ~834, ~856).

- [ ] Add a `#[derive(Clone, Copy)] pub struct KsRenderParams` with the nine values (damping, decay, body, stiffness, feedback, and the ensemble group rate/depth/spread/mix — an `Ensemble` sub-struct is fine if it reads better).
- [ ] `tick_full(&mut self, p: &KsRenderParams)` (plus any non-param args it already takes); build the struct once per block at each call site, not per sample. Remove the `too_many_arguments` allow.
- [ ] Modal goldens (`modal_init`, `modal_lfo_cutoff`, `pizza_to_modal_switch`) must stay bit-identical — this is a pure refactor.
- [ ] Commit: `refactor(core): KsRenderParams replaces tick_full's nine f32 args (#1)`.

### Task 4: Browser uses the dirty-region system (#7)

**Files:** `chimera-core/src/ui/mod.rs` (browser full redraw every frame at ~585-595), `chimera-core/src/ui/browser.rs`.

- [ ] Add a `browser_dirty` flag (or equivalent) set when the browser opens, when its cursor/scroll moves, and on save/load; the browser renders and flushes only when it is set, then clears it.
- [ ] Test first: an idle browser over N frames produces zero flushes/draws after the first; a cursor move produces a redraw; the rendered framebuffer after a move equals a full render (use the existing dirty == full pattern from the all-pages walk test).
- [ ] Screen goldens unchanged.
- [ ] Commit: `fix(ui): browser redraws only when it changes (#7)`.

### Task 5: Matrix cursor clamp and amounts after (un)priming (#11)

**Files:** `chimera-core/src/ui/mod.rs` (`load_matrix` ~157-162; priming/un-priming ~376-393), `chimera-core/src/ui/mod_grid.rs` (`adjust_amount` ~148, ~100-117).

- [ ] Tests first (they must fail before the fix):
  1. Prime 3 destinations on Part 1, move the cursor to column 2, switch to a Part with 1 destination, turn the amount encoder, then prime a new destination: the new route's amount is 0.
  2. Prime A, B, C with distinct amounts, un-prime B: A and C keep their own amounts (by destination, not by column), and the committed mod state agrees.
- [ ] `load_matrix` clamps `sel_col`/`scroll_x` to the destination count; `adjust_amount` is a no-op for columns `>= num_dests`.
- [ ] After prime and un-prime, rebuild amounts from the Part's routes (call `load_amounts` or equivalent) so columns stay keyed to destinations.
- [ ] Commit: `fix(ui): matrix cursor clamped, amounts follow routes on (un)prime (#11)`.

### Task 6: Mod priming feedback (#21)

**Files:** `chimera-core/src/ui/mod.rs` (`let _ = ...dest_registry.add(...)` ~379), `chimera-core/src/ui/mod_grid.rs` (hint at ~322), `chimera-core/src/mod_path.rs` (registry errors `NotModulatable`/`Full` at ~54-67), renderer focus band, `RegionData::Focus`.

- [ ] Matrix page hint text → `PRIME: MIX+PLUS ON A PARAM` (must fit its row; check pixel width).
- [ ] On MIX+PLUS, stop discarding the registry result. Show a short status in the focus band: `ADDED`, `ALREADY ROUTED`, `NOT MODULATABLE`, `MATRIX FULL`. It stays until the focus changes or another encoder/button action, no timer (matches the focus band's "no timer" rule). Include it in `RegionData::Focus` so the dirty-region system redraws it.
- [ ] Tests: each of the four outcomes produces its message; a later focus change clears it; dirty == full with a message shown.
- [ ] Re-lock the `mod_matrix` screen golden (hint changed) and regenerate its PNG with `just screens`. No other golden should change; if one does, explain it.
- [ ] Out of scope: a "modulatable" marker on labels (needs a user decision).
- [ ] Commit: `fix(ui): matrix hint says where to prime; MIX+PLUS reports its outcome (#21)`.

### Task 7: Hygiene part 2 — UI-refresh leftovers (#15)

Items from the issue comment:

- [ ] Delete the dead `VizType` variants no longer drawn (`DriveClip`, `WaveFold`, `ModalPeaks`, `WaveformPreview`, `EqResponse`, `RoutingMatrix`) and their registry entries, `PerfStats.render_us` if unread, and `ModDest::label`/`label_str` if unused.
- [ ] `Renderer::clear_region_fb`: literal `240` → `theme::SCREEN_W`.
- [ ] `draw.rs` `dot/ring/pill/round_outline`: guard `r` with `.max(0)` before casting to `u32`.
- [ ] System page P1–P6 CH values are 1-based (`CH 1`), matching the Mixer PART page formatter.
- [ ] Matrix `>` scroll arrow must not overlap the fifth column's name when scrolled (move it or shorten the column span; test the pixels don't overlap).
- [ ] Test gaps: pan L/C pixel assertions, INIT browser row colour, envelope 2-px gap in the span test, `text_right`/`text_center` with text wider than the box.
- [ ] Re-lock only the goldens this changes (expected: `system`, maybe `mod_matrix`) and regenerate their PNGs with `just screens`.
- [ ] Leave the Demo FM page's `ALGO` 0–7 alone (placeholder page).
- [ ] Commit: `chore(ui): drop dead viz/label code, 1-based System CH, matrix arrow (#15)`.

### Task 8: rustfmt and check gates (#15)

- [ ] `cargo fmt --all`, as its own commit, with no other changes: `style: cargo fmt (#15)`.
- [ ] `Justfile` `check`: add `cargo fmt --all -- --check` and the clippy recipe's command (`-D warnings`). `just check` green.
- [ ] Commit: `chore: just check runs fmt --check and clippy (#15)`.
