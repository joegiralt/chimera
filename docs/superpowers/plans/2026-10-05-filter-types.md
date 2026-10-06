# Filter TYPEs Implementation Plan

**Goal:** The filter gets three TYPEs: CLEAN (today's ZDF/TPT SVF with the Cascadia's UX, self-oscillating, real notches), 2040 (the SSM2040) and DIRTY (a Chamberlin SVF). ALGO's OP 5 and OP 6 become audio-rate taps into FREQ, Q, VCA and FOLD. The matrix becomes the only patch bay, with four permanent columns.

**Architecture:**
- `dsp/filter.rs` becomes a module, `dsp/filter/`. Each type keeps its own flat state in its own file, and a `Filter` dispatcher runs one of them, or two during a TYPE or MODE crossfade.
- Coefficients are set once a block. A block whose `g` or `k` moves takes a second loop that divides exactly per sample; a steady block never divides. Both loops are chosen once a block, as the mode is.
- LEVEL (Filter 12) replaces DRIVE (Filter 2, retired). It is translated on load, and routes into DRIVE are re-addressed to it.
- `ModState` gains four reserved columns, FREQ, Q, VCA and FOLD, outside the 16 registry columns.
- Taps live in `ParamSnapshot::taps: TapRoutes`, not in the matrix's eight rows. The UI shows them as matrix rows 8 and 9 on ALGO.
- ALGO copies OP 5's and OP 6's rows into a stack `TapRows` each block. The filter, the folder and a new tap-VCA stage (before the DC blocker) read them per sample.

**Tech Stack:** Rust 2024 on stable; `no_std` `chimera-core`; host tests in `chimera-core/tests`; `chimera-stm32` built with `--features bench` for the static counts; `llvm-objdump -d --mcpu=cortex-m7`; `tools/` Python for the loop counter.

**Spec:** `docs/superpowers/specs/2026-10-05-filter-types-design.md` (approved at 3f6bad6b). ADR: `docs/adr/0096-filter-types-clean-2040-dirty-and-algo-taps.md` (supersedes 0037). Context: `.superpowers/sdd/filter-types/owner-answers.md`, `review-1.md`, `review-2.md`.

**Base:** branch `filter-types`, worktree `wt-filters`. **Execution starts only after `modal-stagger` has merged to `main`.** This branch is then rebased onto `main` (Task 0).

## Global Constraints

- **Order.** Do not start until `modal-stagger` is merged to `main`. Task 0 then rebases this branch onto it.
- **The in-loop saturate is divide-free (spec H3).** `modal-stagger` may replace `filter.rs`'s `saturate()` with stmlib's rational soft clipper, which divides. If it does, the filter keeps today's C1 curve as its own `sat_c1`. Every steady filter loop's probe shows 0 `vdiv` (Tasks 1, 2 and 18).
- **The audio thread.**
  - No allocation and no panic in audio.
  - Indices are const or clamped, and clamps are `max`/`min` (`vmaxnm`/`vminnm`), never `clamp` on a path that could see NaN.
  - Modes and paths are matched once a block.
- **`unsafe`.** None is expected. Any `unsafe` carries a `// SAFETY:` comment.
- **Never snap.**
  - Every coefficient, gain and tap amount ramps across its block.
  - TYPE and MODE warm-start and crossfade over two blocks (Task 10).
  - LEVEL's clipper fades over 20 ms.
  - UI moves go through the parameter lerp.
- **Disk codes are append-only (ADR 0009, 0045).**
  - DRIVE (Filter 2) is retired and never reused.
  - New codes: Filter ids 8 (FM3 IN), 11 (QM IN) and 12 (LEVEL); `FilterKind` 4 ("2040") and 6 ("DIRTY"); `FilterMode::Hp12` 9 ("HP12"); `Jack` 0–9; the `TapRoutes` record.
  - Filter ids 9 and 10 stay unused.
  - New lines are appended to `chimera-core/tests/fixtures/disk_codes_v1.txt`; the frozen lines never change.
- **Old patches load safely; they needn't sound the same** (memory: no legacy patch compat). The exceptions are the level rules in Task 6, which hold.
- **Hints (ADR 0086).** Every MIX or hold gesture on a touched cell shows its focus-band hint through `ui::hint`. ADR 0086 is Proposed, so its table is edited in place (Task 17).
- **Cost bills (spec § Cost, ADR 0056's method).**
  - Count statically from `llvm-objdump -d --mcpu=cortex-m7` of the release bench build (`opt-level = 2`, fat LTO). Count each loop from head to back-edge, and divide per-block work by 64.
  - `cycles = instr × 1.46 × 1.1 + 12 × (vdiv + vsqrt left per sample)`, rounded up to 5.
  - `bill = max(0, cycles − cycles(old LP24))`. The old LP24 is counted in Task 1.
  - Each `Cost` constant's doc comment shows its count and arithmetic.
  - The build may not exceed the spec's budget table (copied into Task 18).
- **Tests.**
  - TDD in every task.
  - **Mutation-check every new behaviour test.** Once a test passes, break the line it guards (flip a sign, drop a clamp, swap a mode arm), see the test fail, and revert. Note the mutation in the commit body, one line per test.
- **Goldens.**
  - Re-record only the cases a task changes, in that task's commit, with `GOLDEN_RECORD=1 … -- --nocapture`.
  - A comment on each re-recorded row names the reason.
  - Keep a running list of every re-recorded golden in § Measured, for the PR.
- **Build and verify.**
  - Every cargo command runs with `CARGO_INCREMENTAL=0`.
  - `CARGO_INCREMENTAL=0 just check` passes before every commit (ADR 0013).
- **Commits.**
  - Terse, with no type prefix (`feat:`).
  - Stage by path with `git add <paths>`. Never `git add -A`, and never `chimera.bin`.
- **Issues.** Bugs and follow-ups found on the way go to GitHub issues (joegiralt/chimera), never into repo files.
- **The unit.** **(unit)** steps flash the bench build with `just flash-bench` and read `just usb bench`. Auto mode has blocked flashing before, so **ask the owner's permission before each flash**. If it isn't given, record "held" in § Measured and go on: no host task depends on a unit reading.
- **Flash.** Report the delta in one line: `rust-size target/thumbv7em-none-eabihf/release/chimera-stm32`, `text`, before and after (Task 18). Escalate only if free flash falls under about 40 KB.

## Review Focus

1. **TYPE scrolled through all three and back inside one crossfade** (several changes within 2 blocks) lands on the last TYPE with no click and no burst. Test: Task 10, `type_scroll_inside_a_crossfade_lands_on_the_last_without_a_click`.
2. **A non-finite sample reaching the filter** (NaN or inf from an engine bug) must not silence the voice forever: the filter recovers next block. Test: Tasks 3, 8 and 9, `<type>_recovers_from_a_nan_block`.
3. **An old patch whose full 16-column registry includes DRIVE and CUTOFF** loads with every route: DRIVE's routes become LEVEL's, CUTOFF's go to FREQ, and two registry columns are freed. Test: Task 11, `full_registry_with_drive_and_cutoff_keeps_every_route`.
4. **An ALGO Sound with taps switched to MODAL and back** plays no tap on MODAL: its jacks act as LFO 1 and ENV 3, there is no NaN and no click, and the taps are back intact on ALGO. Test: Task 14, `taps_survive_an_engine_round_trip_and_are_inert_between`.
5. **Repatching FM3 IN when every candidate's FREQ cell is occupied** leaves the jack and every route unchanged, and the UI shows the jack where it was. Test: Task 12, `repatch_with_every_target_occupied_changes_nothing`.

---

## File map

| File | Responsibility | Task |
|---|---|---|
| `chimera-core/src/dsp/filter/mod.rs` | `FilterKind`, `FilterMode`, `kind_change`, `routed_cutoff`, `Filter` (the dispatcher, crossfade), bills | T1 (move), T4, T8–T10, T15 |
| `chimera-core/src/dsp/filter/coef.rs` | `tan_accurate`, `g_of`, `exp2_poly`, `sat_c1`, `Ramp` | T0, T2, T14 |
| `chimera-core/src/dsp/filter/clean.rs` | CLEAN: SVF stages, steady and moving loops, trims, LP1, NOTCH1, NOTCH2 | T2–T5 |
| `chimera-core/src/dsp/filter/level.rs` | LEVEL law, DRIVE translation, clipper, gate fade, peak | T6, T7 |
| `chimera-core/src/dsp/filter/k2040.rs` | 2040 | T8 |
| `chimera-core/src/dsp/filter/dirty.rs` | DIRTY | T9 |
| `chimera-core/src/dsp/filter/probe.rs` | `#[cfg(any(test, feature = "probe"))]` static-count probes | T1, extended T2–T9, T14 |
| `chimera-core/src/taps.rs` | `TapOp`, `Jack`, `TapRoutes`, `TapRows`, `TapLane`, `tap_vca` | T12–T14 |
| `chimera-core/src/modulation.rs` | `FixedCol`, `ModState`'s fixed columns | T11 |
| `chimera-core/src/ui/mod_grid.rs` | fixed columns, rows 8–9, X cells, geometry, hints | T11, T13, T17 |
| `chimera-core/src/ui/filter_panel.rs` | panels per type, `JackSlot`, `repatch`, KEYBD steps | T12, T13 |
| `tools/loop_count.py`, `tools/test_loop_count.py` | the instruction count from head to back-edge | T1 |
| `chimera-stm32/src/bench.rs` | `#[used]` probe table (bench build) | T1 |
| `chimera-core/tests/common/filter_ref.rs` | f64 per-sample references, response and level measures | T2 |
| `chimera-core/tests/filter_types_test.rs` | DSP tests 1, 2, 4, 6, 7 and 8 | T2–T9 |
| `chimera-core/tests/filter_switch_test.rs` | test 5, the crossfade | T10 |
| `chimera-core/tests/filter_level_test.rs` | LEVEL, translation, LED | T6, T7 |
| `chimera-core/tests/fixed_columns_test.rs` | the fixed columns and migration | T11 |
| `chimera-core/tests/taps_test.rs` | test 9 and the tap records | T13, T14 |

---

### Task 0: Rebase after `modal-stagger`, and the filter's own `sat_c1`

**Files:**
- Modify: `chimera-core/src/dsp/filter.rs` (only if `modal-stagger` changed `saturate`)

**Interfaces:**
- **Produces:** `pub(crate) fn sat_c1(x: f32) -> f32`, today's C1 curve (linear to ±1, then `1 + u − u²/2` for `u = |x| − 1`, flat at ±1.5 from ±2). Task 2 moves it to `dsp/filter/coef.rs`.

- [ ] **Step 1: Confirm the merge, then rebase.** Run `git -C <main checkout> log --oneline main | grep -i stagger`; expect the merge commit. Then run `git fetch && git rebase main`; expect a clean rebase, since this branch is docs only.
- [ ] **Step 2: Inspect `saturate`.** Run `git diff 3f6bad6b..HEAD -- chimera-core/src/dsp/filter.rs`.
  - If `saturate` changed, restore today's body as `sat_c1` in `filter.rs`, used only by the SVF. Leave the new clipper wherever `modal-stagger` put it.
  - If it didn't change, rename it to `sat_c1`.
- [ ] **Step 3: The divide-free test.** Keep the existing `saturate_is_smooth_and_bounded` test, renamed `sat_c1_is_smooth_and_bounded`. Add `sat_c1_is_identity_to_one`, which asserts `sat_c1(x) == x` bit for bit for 1,000 points in `[-1, 1]`; a rational clipper fails it.
- [ ] **Step 4: Verify.** Run `CARGO_INCREMENTAL=0 just check`; expect PASS.
- [ ] **Step 5: Commit** (only if Step 2 changed code).

```bash
git add chimera-core/src/dsp/filter.rs
git commit -m "Filter keeps its C1 sat_c1 in the loop"
```

### Task 1: Probes and today's LP24 count (H3 baseline)

**Files:**
- Move: `chimera-core/src/dsp/filter.rs` → `chimera-core/src/dsp/filter/mod.rs` (`git mv`, no edits)
- Create: `chimera-core/src/dsp/filter/probe.rs`, `tools/loop_count.py`, `tools/test_loop_count.py`
- Modify: `chimera-core/src/dsp/filter/mod.rs` (`#[cfg(any(test, feature = "probe"))] pub mod probe;`), `chimera-stm32/src/bench.rs`, this plan's § Measured

**Interfaces:**
- **Produces:**
  - In `probe.rs`: `#[inline(never)] pub fn old_lp24_probe(f: &mut SvfFilter, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams)`, which runs one block. A steady call (cutoff unchanged) and a ramped call (cutoff changed) take the two loops.
  - In `bench.rs`: `#[used] static FILTER_PROBES: &[fn(...)]`, which keeps every probe in the bench ELF. Later tasks append to it.
  - In `tools/loop_count.py`: `count(disasm: str, symbol: str) -> dict` with keys `loop_instr`, `vdiv`, `vsqrt` and `block_instr`, plus a CLI, `python3 tools/loop_count.py <elf> <symbol-substring>`. It prints the counts and `cycles = ceil5(loop_instr·1.46·1.1 + 12·(vdiv + vsqrt))`. The loop is the innermost backward branch's span, head to back-edge, and every instruction outside it is `block_instr`.
  - A recorded `OLD_LP24` (cycles a sample, steady), which every later bill subtracts.

- [ ] **Step 1: Write the failing tool test.** In `tools/test_loop_count.py`, `test_counts_a_known_loop`: a 9-line objdump fixture string with one loop of 6 instructions, one `vdiv.f32` inside it and 3 instructions outside. Assert `loop_instr == 6`, `vdiv == 1`, `block_instr == 3`, and `cycles(6, 1) == 25` (6·1.606 + 12 = 21.6, rounded up to 25).
- [ ] **Step 2: Run it and see it fail.** Run `python3 -m unittest tools/test_loop_count.py`; expect FAIL (no module).
- [ ] **Step 3: Implement** `loop_count.py`, the probe, and the bench `#[used]` table.
- [ ] **Step 4: Run it and see it pass.** Run `python3 -m unittest discover -s tools -p 'test_*.py'`; expect PASS.
- [ ] **Step 5: Count.**
  - Run `CARGO_INCREMENTAL=0 cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`.
  - Then run `python3 tools/loop_count.py target/thumbv7em-none-eabihf/release/chimera-stm32 old_lp24_probe`.
  - Expect two loops (steady and ramped), each about 50 instructions with 2 `vdiv` (the spec estimates ≈ 105 cycles).
  - Record both in § Measured, with the commit and date.
- [ ] **Step 6: Re-check the budgets** against `OLD_LP24`. Recompute the spec's "Est. bill" column as `max(0, est. cycles − OLD_LP24)` and write it under § Measured.
  - If any steady bill turns positive where the spec says 0 (CLEAN, DIRTY), note it there; Task 18 bills it.
  - If any path's budget would exceed `OLD_LP24 + 50`, stop and tell the owner before Task 2.
- [ ] **Step 7: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`; expect PASS.

```bash
git add chimera-core/src/dsp/filter chimera-stm32/src/bench.rs tools/loop_count.py tools/test_loop_count.py docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "Filter probes and today's LP24 count"
```

### Task 2: CLEAN coefficients, the tan clamp and the divide rule

**Files:**
- Create: `chimera-core/src/dsp/filter/coef.rs`, `chimera-core/src/dsp/filter/clean.rs`, `chimera-core/tests/common/filter_ref.rs`, `chimera-core/tests/filter_types_test.rs`
- Modify: `chimera-core/src/dsp/filter/mod.rs`, `chimera-core/src/dsp/voice.rs` (`SvfFilter` becomes `Clean`), `chimera-core/src/dsp/filter/probe.rs`, `chimera-core/tests/common/mod.rs`, `chimera-core/tests/filter_test.rs`, `chimera-core/tests/golden_test.rs` (re-record), `THIRD_PARTY.md` (stmlib, MIT)

**Interfaces:**
- **Consumes:** `sat_c1` (Task 0) and `OLD_LP24` (Task 1).
- **Produces:**
  - In `coef.rs`:
    - `pub fn tan_accurate(f: f32) -> f32`: `tan(π·f)` by stmlib's `FREQUENCY_ACCURATE` polynomial, coefficients copied with attribution.
    - `pub fn g_of(fc: f32, fs: f32) -> f32`: `tan_accurate(min(fc/fs, 0.497))`.
    - `pub(crate) fn sat_c1(x: f32) -> f32`, moved here.
  - In `clean.rs`:
    - `pub struct Clean { s1: [f32; 2], s2: [f32; 2], g: Option<f32>, k: Option<f32>, pre: Option<f32> }`.
    - `pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams, fs: u32)`.
    - `pub fn hold(&mut self)`.
    - `#[cfg(test)] last_g`, kept.
  - **The rule.** A block is steady when `g` and `k` are equal at both ends. Then `d = 1/(1 + kg + g²)` and `d₂ = 1/(1 + k₂g + g²)` are computed once. Otherwise `g` and `k` ramp per sample (`g_at`), and `d` and `d₂` are divided per sample, exactly. Each SVF stage takes `d` as an argument and never divides.
  - **Temporary.** The old `k = 2(1 − RES) + 0.01` stays until Task 3; only the divides move.
  - **In `tests/common/filter_ref.rs`:**
    - `pub fn clean_ref(x: &[f32], g: &[f64], k: &[f64], mode: FilterMode) -> Vec<f64>`: the same topology in f64, with exact divides and `sat_c1` in f64.
    - `pub fn g_track(from_hz: f64, to_hz: f64, fs: f64) -> Vec<f64>`, the per-sample `g` the ramp gives, through `tan_accurate`.
    - `pub fn mag_db(out: &[f32], f: f32, fs: f32) -> f32` (a single-bin DFT) and `pub fn rms_db(x: &[f32]) -> f32`.

- [ ] **Step 1: Write the failing tests** in `filter_types_test.rs`:

```rust
#[test] fn tan_accurate_beats_fast_tan_everywhere_and_is_tight_to_10k() {
    // f = 1k..20k step 500 Hz at 48 kHz: |tan_accurate(f/fs) − tan(π f/fs)| ≤ |fast_tan(π f/fs) − tan(π f/fs)|;
    // relative error ≤ 1e-3 for f ≤ 10 kHz; print the error at 15 and 20 kHz.
}
#[test] fn g_of_clamps_at_0_497() {
    // g_of(30_000.0, 48_000.0) == tan_accurate(0.497); g_of is finite and monotonic on 20 Hz..24 kHz.
}
#[test] fn clean_one_block_jumps_match_the_exact_reference_at_res_0() {
    // For LP2, LP4, BP4, HP4 on a held saw: FREQ 100 Hz → 20 kHz in one block, 20 kHz → 100 Hz,
    // and a 1 ms ENV 1 → CUTOFF at +100 % (cutoff path from the voice's routed_cutoff per block):
    // max |y − y_ref| / max|y_ref| ≤ 1e-3 against clean_ref.
}
#[test] fn steady_and_moving_agree_on_a_steady_block() {
    // The moving loop forced on a steady block equals the steady loop within 1e-6 relative.
}
```

- [ ] **Step 2: Run them and see them fail.** Run `CARGO_INCREMENTAL=0 cargo test -p chimera-core --test filter_types_test`; expect FAIL (unresolved `tan_accurate`, `Clean`).
- [ ] **Step 3: Implement** `coef.rs` and `clean.rs`, and switch `Voice` to `Clean`. `fast_tan` stays in `dsp/mod.rs` for its other users.
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Then mutation-check: turn the moving loop's per-sample divide into a linear lerp of `d`, and the jump test must fail.
- [ ] **Step 5: The probe.**
  - Add `clean_probe::<M, MOVING>` for LP4, steady and moving.
  - Run `python3 tools/loop_count.py … clean_probe`.
  - Expect the steady loop to have **0 `vdiv`** and ≤ 56 instructions, and the moving loop 2 `vdiv` and ≤ 56 + 6.
  - Record both in § Measured.
- [ ] **Step 6: Re-record the audio goldens.**
  - Run `GOLDEN_RECORD=1 CARGO_INCREMENTAL=0 cargo test -p chimera-core --test golden_test -- --nocapture`. Only rows with a filter in their chain change; the reason is "ACCURATE tan, exact divides (#57)".
  - Also re-record `filter_test::every_mode_renders_as_recorded`.
- [ ] **Step 7: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`; expect PASS.

```bash
git add THIRD_PARTY.md chimera-core/src/dsp/filter chimera-core/src/dsp/voice.rs chimera-core/tests/common/filter_ref.rs chimera-core/tests/common/mod.rs chimera-core/tests/filter_types_test.rs chimera-core/tests/filter_test.rs chimera-core/tests/golden_test.rs docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "CLEAN: ACCURATE tan clamped at 0.497; exact divides on moving blocks, none on steady"
```

### Task 3: CLEAN resonance, self-oscillation and per-mode trims

**Files:**
- Modify: `chimera-core/src/dsp/filter/clean.rs`, `chimera-core/tests/filter_types_test.rs`, `chimera-core/tests/golden_test.rs` (re-record)

**Interfaces:**
- **Consumes:** `Clean` (Task 2).
- **Produces:**
  - `pub fn clean_k(res: f32) -> f32` = `2·(1 − res/0.95)`: 2 at 0, 0 at 0.95, −0.105 at 1.
  - `pub fn clean_k2(k: f32) -> f32` = `max(1.2·k + 0.3, 0.1)`.
  - `pub(crate) const TRIM_OSC: [f32; 8]`, indexed by `FilterMode as usize`, pinned at build. NOTCH2's entry is 1.0 and unused.
  - `pub fn trim(mode: FilterMode, k: f32) -> f32`: 1 for `k ≥ 0.5`, linear to `TRIM_OSC[mode]` at `k ≤ 0`. It is ramped with `k` and is one multiply on the output.
  - `pub const OSC_BAND_DB: (f32, f32)`: the shared self-oscillation band, in `tests/common/filter_ref.rs`. It starts at **(−12.0, −4.0) dBFS**. Tasks 8 and 9 may widen it only to include their type, with the reason in § Measured.
  - At the end of each block, `Clean` zeroes its state if any state is non-finite.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn clean_k_law_has_its_onset_at_0_95() {
    // clean_k(0.0) == 2.0; clean_k(0.95).abs() < 1e-6; (clean_k(1.0) + 0.10526).abs() < 1e-4;
    // 1 + k g + g² ≥ 0.997 for k = clean_k(1.0), g over g_of(20..24k).
}
#[test] fn clean_self_oscillates_in_band_and_tracks_1v_oct() {
    // Every CLEAN mode but NOTCH2 (LP2 LP4 BP2 BP4 HP4 now; LP1 and NOTCH1 join in Task 4), RES 1, one impulse,
    // read 0.5 s after 0.25 s, FREQ 100 Hz..8 kHz in octaves: RMS level steady ±1 dB and inside OSC_BAND_DB (< 0 dBFS);
    // FM2 (NOTE → CUTOFF) +100 %, notes C2..C6: each octave ±10 cents, absolute ±25 cents, up to 5 kHz.
}
#[test] fn clean_onset_is_res_0_95() {
    // An impulse decays (level after 1 s < −60 dB of the peak) at RES 0.94, and sustains (> −20 dB) at RES 0.96.
}
#[test] fn clean_res_1_one_block_jumps_stay_bounded() {
    // Task 2's jumps at RES 1: every output finite and ≤ the steady bound pinned here (print the max, pin it × 1.1).
}
#[test] fn clean_is_bounded_on_the_grid() {
    // FREQ 24 log points × RES 11 × every CLEAN mode built; inputs: silence, a full-scale saw, a square at fs/4,
    // white noise ±1, impulses of ±10. Every output finite and ≤ CLEAN_BOUND (pinned at build); states ≤ 1.5 after sat_c1.
}
#[test] fn clean_recovers_from_a_nan_block() {
    // One block of f32::NAN input, then a saw: the second block after is finite and nonzero.
}
```

- [ ] **Step 2: Run them and see them fail.** Run `CARGO_INCREMENTAL=0 cargo test -p chimera-core --test filter_types_test clean_`; expect FAIL.
- [ ] **Step 3: Implement.**
  - Replace the `k` law, and ramp `k` as `g` is: a block is moving if `k` moved.
  - Add the `k₂` clamp, `trim` and the finite check.
  - Pin `TRIM_OSC` by printing each mode's untrimmed RES 1 level, then setting each trim so the mode sits at −8 dBFS ± 2.
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - drop the `max(…, 0.1)` in `clean_k2` (the bound test should fail);
  - use `k = 2(1 − RES)` (the onset test should fail);
  - set one `TRIM_OSC` to 1.0 (the band test should fail).
- [ ] **Step 5: Re-record the goldens** whose patches have RES > 0 (the reason is "CLEAN's k law, ADR 0096"). Then run `CARGO_INCREMENTAL=0 just check`; expect PASS.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/filter/clean.rs chimera-core/tests/common/filter_ref.rs chimera-core/tests/filter_types_test.rs chimera-core/tests/golden_test.rs
git commit -m "CLEAN self-oscillates from RES 0.95, trimmed per mode into one band"
```

### Task 4: CLEAN's modes: knob order, default, labels, LP1 and NOTCH1

**Files:**
- Modify: `chimera-core/src/dsp/filter/mod.rs`, `chimera-core/src/dsp/filter/clean.rs`, `chimera-core/src/params.rs` (MODE spec), `chimera-core/src/ui/view.rs` (MODE's per-type names), `chimera-core/src/ui/viz.rs` (`Response`), `chimera-core/tests/filter_types_test.rs`, `chimera-core/tests/filter_test.rs`, `chimera-core/tests/golden_test.rs`

**Interfaces:**
- **Produces:**
  - `FilterKind::modes(self) -> &'static [FilterMode]`, in knob order. CLEAN's is `[Lp6, Lp12, Lp24, Bp12, Bp24, Hp24, Notch, Phaser]`.
  - `FilterKind::default_mode(self) -> FilterMode`; CLEAN's is `Lp24`.
  - `FilterKind::mode_label(self, m: FilterMode) -> &'static str`.
  - `FilterKind::mode_labels(self) -> &'static [&'static str]`, in `modes()` order. CLEAN's is `["LP1","LP2","LP4","BP2","BP4","HP4","NOTCH1","NOTCH2"]`.
  - `kind_change` falls back to `default_mode()`, not `modes()[0]`.
  - **Removed:** `SVF_MODES`, `SVF_MODE_NAMES`, `SVF_MODE_NAMES_BY_ID` and `FilterMode::label` (#165).
  - **MODE's spec.** `ParamSpec::choice(7, "MODE", ValFmt::Names(FilterKind::Svf.mode_labels()), 7.0, 2.0)`: the static default 2.0 is LP4's index in CLEAN's order. The FLT view passes `ValFmt::Names(kind.mode_labels())` for MODE, so the labels and step count follow the type. Every reader of MODE's default goes through `kind.default_mode()`.
  - **The modes.** LP1 = `lp + bp` of one stage. NOTCH1 = `lp + 0.25·hp`. Disk codes and idents are unchanged ("LP6", "NOTCH" and so on).
  - **`Response`** gains `Lp1 { reso }`, `Notch2x` (a peak or dip at FREQ, a notch at 2 × FREQ, −12 dB treble) and `DoubleNotch` (the notches at 0.414 × and 2.414 × FREQ). `Response::of(kind, mode)` takes the kind.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn default_mode_is_in_modes_and_mode_stays_in_modes() {
    // for k in BUILT: modes().contains(&default_mode()); mode_labels().len() == modes().len();
    // every Block::write of MODE at 0..=7 and every set_enum_code 0..=255 leaves mode ∈ kind.modes().
}
#[test] fn clean_labels_name_what_they_do() {
    // mode_label(Lp6) == "LP1", (Notch) == "NOTCH1", (Phaser) == "NOTCH2"; disk_ident(Notch) == "NOTCH" (unchanged).
}
#[test] fn clean_responses_match_the_prototype() {
    // RES 0, FREQ 250 Hz, 1 kHz, 4 kHz, swept sines (mag_db): slopes over 2..4 × FREQ:
    // LP1 −6 ± 1.5, LP2 −12 ± 2, LP4 −24 ± 3, HP4 +24 ± 3 dB/oct; LP1 −3 dB at FREQ ± 3 %; BP2/BP4 peak at FREQ ± 3 %;
    // NOTCH1 zero at 2 × FREQ ± 3 % (digital: tan(π f/fs) = 2g), ≥ 30 dB deep; −8.6 ± 0.5 dB at FREQ at Q 0.
}
#[test] fn lp1_at_q0_is_the_one_pole_and_q_peaks_it() {
    // RES 0: |H(FREQ)| = −3.01 ± 0.2 dB; RES 0.5 (k = clean_k(0.5)): peak at FREQ ≈ 20·log10(√2/k) ± 0.5 dB.
}
```

  Also extend Task 3's band test to LP1 and NOTCH1, and add LP1 and NOTCH1 to Task 3's bound grid.
- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.** Use `filter_panel` only through `view.rs`'s MODE fmt; the panel layout is Task 12.
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - NOTCH1 back to `lp + hp` (the 2 × FREQ zero test should fail);
  - LP1 back to the plain one-pole (the Q-peak test should fail);
  - `default_mode` returning `modes()[0]` (the default test should fail).
- [ ] **Step 5: Re-record** the goldens of patches on LP6 or NOTCH (the reason is "LP1 and NOTCH1, ADR 0096"). Then run `CARGO_INCREMENTAL=0 just check`; expect PASS.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/filter chimera-core/src/params.rs chimera-core/src/ui/view.rs chimera-core/src/ui/viz.rs chimera-core/tests/filter_types_test.rs chimera-core/tests/filter_test.rs chimera-core/tests/golden_test.rs
git commit -m "CLEAN modes in knob order with LP4 default; LP1 resonant, NOTCH1 at 2x FREQ"
```

### Task 5: NOTCH2

**Files:**
- Modify: `chimera-core/src/dsp/filter/clean.rs`, `chimera-core/src/dsp/filter/probe.rs`, `chimera-core/tests/filter_types_test.rs`, `chimera-core/tests/golden_test.rs`

**Interfaces:**
- **Produces:**
  - `Clean` gains `ap: [f32; 4]` and `w: f32` (the last cascade output).
  - The loop follows spec § CLEAN's DSP NOTCH2:
    - `u = x + fb·w`;
    - four TPT all-passes at `G₁ = g/(1+g)` (per block, or per sample on a moving block);
    - `out = √(1 − fb)·½·(u + a)`;
    - `fb = 0.7·RES`, with `√(1 − fb)` computed once a block.
  - The warm start entering NOTCH2 (`ap[i] ← lp`) is Task 10's.

- [ ] **Step 1: Write the failing tests** (test 6):

```rust
#[test] fn notch2_has_two_exact_notches_that_sweep_in_octaves() {
    // White noise, averaged spectrum (2^15-point Welch), FREQ 200, 400, 800, 1600 Hz at Q 0, 0.5, 1:
    // exactly two minima ≥ 25 dB deep, at (fs/π)·atan(g·tan(π/8)) and (fs/π)·atan(g·tan(3π/8)) ± 3 %;
    // doubling FREQ moves each up 1 octave ± 0.05.
}
#[test] fn notch2_q_raises_the_peaks_but_not_the_loudness() {
    // Peak near FREQ rises with Q by ≤ 5.2 dB; for FREQ ≤ 2 kHz broadband RMS within ±1.5 dB across Q 0..1.
}
#[test] fn notch2_never_self_oscillates() {
    // RES 1, one impulse: level after 0.5 s < −80 dB of the peak, at every FREQ of the grid.
}
#[test] fn notch2_peak_slides_below_freq_as_pinned() {
    // At FREQ 8 kHz, Q 1: the peak's frequency / FREQ, printed, within ±2 % of the value pinned here.
}
```

  Add NOTCH2 to the bound grid.
- [ ] **Step 2: Run them and see them fail.** Expect FAIL (today's PHASER has no notches).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - take the dry signal before the feedback node (the notch-depth test should fail);
  - drop `√(1 − fb)` (the loudness test should fail).
- [ ] **Step 5: The probe.** Steady NOTCH2: 0 `vdiv`, ≤ 34 instructions. Record it in § Measured.
- [ ] **Step 6: Re-record** the PHASER goldens. Then run `CARGO_INCREMENTAL=0 just check` and commit.

```bash
git add chimera-core/src/dsp/filter/clean.rs chimera-core/src/dsp/filter/probe.rs chimera-core/tests/filter_types_test.rs chimera-core/tests/golden_test.rs docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "NOTCH2: dry plus four all-passes, two notches, Q as feedback to 0.7"
```

### Task 6: LEVEL as Filter 12; DRIVE retired, translated and re-addressed

**Files:**
- Create: `chimera-core/src/dsp/filter/level.rs`, `chimera-core/tests/filter_level_test.rs`
- Modify:
  - `chimera-core/src/params.rs` (FilterParams: `drive` → `level`, the specs);
  - `chimera-core/src/storage/codes.rs` (`RETIRED`, `TRANSLATIONS`, `READDRESS`);
  - `chimera-core/src/storage/sound.rs` (`addr_of` applies `READDRESS`);
  - `chimera-core/src/dsp/voice.rs` (`StageEase` eases `level`);
  - `chimera-core/src/dsp/filter/clean.rs` (the pre-gain);
  - `chimera-core/src/ui/filter_panel.rs` (FLT › MODE's extra becomes LEVEL);
  - `chimera-core/src/ui/page.rs:136`;
  - tests that name `FilterParams::DRIVE` (`addr_test.rs`, `ui_routing_test.rs`, `property_test.rs`, `param_sweep_test.rs` and `cost_test.rs`);
  - `chimera-core/tests/fixtures/disk_codes_v1.txt` (append);
  - `chimera-core/tests/fixtures/neutral_v1.txt` (the `drive` line becomes `level: 0.5`; nothing else moves).

**Interfaces:**
- **Produces:**
  - `FilterParams::LEVEL: ParamId = ParamId(12)` and `pub level: f32`, default 0.5. The spec is `ParamSpec::continuous(12, "LEVEL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0/128.0, true).ident("LEVEL")`.
  - `FilterParams::DRIVE` is removed. `RETIRED` gains `(10, 2)`.
  - `pub fn level_gain(level: f32) -> f32` = `fast_exp2(4·level − 2)`: ×¼ at 0, exactly 1.0 at 0.5, ×4 at 1.
  - `pub fn level_from_drive(drive: f32) -> f32` = `clamp((log2(1 + 4·drive) + 2)/4, 0, 1)` (`libm::log2f`; the load path, not audio).
  - `pub fn translate_drive(r: &Retired, b: &mut dyn Block)`, appended to `TRANSLATIONS` as `Translation { block: 10, apply }`. It writes LEVEL only when `r.get(ParamId(2))` is `Some`, and a file holding only ids 3–5 is untouched. The firmware never writes id 2 again, so a file with DRIVE has no LEVEL.
  - `pub const READDRESS: &[((u8, u8), (u8, u8))] = &[((10, 2), (10, 12))]`. `addr_of` maps through it before `ValidAddr::find`, so routes, registry entries and ModDests into DRIVE land on LEVEL with their amount and label kept.
  - **CLEAN's pre-gain** is `level_gain(level)`, ramped per sample as today's `pre` is.

- [ ] **Step 1: Write the failing tests** in `filter_level_test.rs`:

```rust
#[test] fn level_law_is_unity_at_half() {
    // level_gain(0.5) == 1.0 exactly; level_gain(0.0) == 0.25 ± 1e-6; level_gain(1.0) == 4.0 ± 1e-5.
}
#[test] fn drive_translates_to_the_same_gain_up_to_0_75() {
    // For drive in 0..=0.75 step 0.05: level_gain(level_from_drive(d)) == 1 + 4d within 0.1 dB;
    // level_from_drive(0.0) == 0.5; level_from_drive(1.0) == 1.0 (×4: 20·log10(5/4) = 1.94 dB quieter).
}
#[test] fn a_drive_only_file_loads_at_its_old_level() {
    // Hand-built Filter block record with ids 0, 1, 2 (DRIVE 0.3), no 12: decoded level == level_from_drive(0.3);
    // a held saw through the voice at LEVEL vs the old 1 + 4·0.3 pre-gain (computed in the test): RMS equal within 0.1 dB.
}
#[test] fn a_file_with_neither_decodes_to_unity() {
    // Filter block with ids 0 and 1 only: level == 0.5.
}
#[test] fn routes_into_drive_land_on_level() {
    // A file whose ModDests and Routes name (10, 2) at amount 40, and whose Registry names it: after decode the
    // route reads 40 at (Filter, LEVEL) and the registry entry's label is kept.
}
```

  Add `drive_id_2_is_retired` to `disk_codes_test.rs`: `RETIRED.contains(&(10, 2))`, and `ValidAddr::find(Filter, ParamId(2))` is `None`.
- [ ] **Step 2: Run them and see them fail.** Run `CARGO_INCREMENTAL=0 cargo test -p chimera-core --test filter_level_test --test disk_codes_test`; expect FAIL.
- [ ] **Step 3: Implement.** Append to the fixture the lines the test asks for (`B 10 12 LEVEL # LEVEL`). In `neutral_v1.txt`, change only `drive: 0.0` to `level: 0.5`; the diff must show nothing else.
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - drop the `READDRESS` lookup (the route test should fail);
  - make `translate_drive` write 0.5 always (the drive-only test should fail).
- [ ] **Step 5: Verify** with `CARGO_INCREMENTAL=0 just check`; expect PASS. Audio goldens must not change: DRIVE 0 is now LEVEL 0.5, exactly unity. If one moves, find out why before re-recording.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/filter chimera-core/src/params.rs chimera-core/src/storage/codes.rs chimera-core/src/storage/sound.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/filter_panel.rs chimera-core/src/ui/page.rs chimera-core/tests/filter_level_test.rs chimera-core/tests/disk_codes_test.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/tests/fixtures/neutral_v1.txt chimera-core/tests/addr_test.rs chimera-core/tests/ui_routing_test.rs chimera-core/tests/property_test.rs chimera-core/tests/param_sweep_test.rs chimera-core/tests/cost_test.rs
git commit -m "LEVEL (Filter 12) replaces DRIVE; DRIVE translated on load, its routes re-addressed"
```

### Task 7: LEVEL's clipper, gate fade and LED meter

**Files:**
- Modify: `chimera-core/src/dsp/filter/level.rs`, `chimera-core/src/dsp/filter/clean.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/src/meter.rs`, `chimera-core/src/instrument.rs`, `chimera-core/src/dsp/filter/probe.rs`, `chimera-core/tests/filter_level_test.rs`

**Interfaces:**
- **Produces:**
  - `pub struct FilterMods { pub level_routed: bool }`. Task 14 adds the tap lanes and a lifetime.
  - `pub struct LevelStage { fade: f32, peak: f32 }`, with `pub fn run(&mut self, buf: &mut [f32; BLOCK_SIZE], gain: (f32, f32), open: bool)`. It applies the ramped gain; while `open` or fading it blends `sat_c1(x·gain)` against `x·gain` by `fade`, which steps `1/15` a block (20 ms at 64 samples, 48 kHz); and it keeps `peak = max|x·gain|`.
  - **The gate:** `open = max(gain.0, gain.1) > 1.0 || mods.level_routed`.
  - `pub fn clip_amount(peak: f32) -> f32` = `clamp((peak − 1)/2, 0, 1)`. It reads 0 while the gate is closed and the fade is 0.
  - `Clean::process(&mut self, buf, p, fs, mods: &FilterMods) -> f32` returns the block's `clip_amount`.
  - `Voice::filter_clip(&self) -> f32`.
  - `pub static FILTER_CLIP: [Meter; MAX_PARTS]` in `meter.rs`. Each Part publishes the maximum over its voices once a block, beside `MASTER_GR`.
  - `Voice::render` passes `level_routed = mod_state.moves_addr(ParamAddr::new(Filter, LEVEL))`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn the_clipper_is_transparent_to_one() {
    // LEVEL 0.75 (×2), input amplitude 0.4 (peaks 0.8 after gain): output == input·2 within 1e-6 (after the fade is full).
}
#[test] fn crossing_unity_does_not_step() {
    // A full-scale saw; LEVEL 0.5 ↔ 0.51 every 100 ms via Block::set for 2 s: Tape::click finds no click (CLICK_RATIO, CLICK_FLOOR).
}
#[test] fn led_is_zero_when_off_and_rises_with_level() {
    // LEVEL 0.5 any signal: filter_clip == 0; signal ≤ 1 at any LEVEL: 0; full-scale saw at LEVEL 0.6, 0.7, 0.8, 0.9, 1.0:
    // strictly increasing, ≤ 1.
}
#[test] fn part_meter_is_the_max_over_its_voices() {
    // Two voices of one Part, one clipping at 0.3, one at 0.7: FILTER_CLIP[part].read() == 0.7 after the block.
}
```

- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - remove the fade, gating hard (the unity test should fail);
  - gate on `p.drive.drive` (the gate tests should fail).
- [ ] **Step 5: The probe.** The clipper with the meter and fade adds ≤ 12 instructions to steady LP4. Record it.
- [ ] **Step 6: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`.

```bash
git add chimera-core/src/dsp/filter chimera-core/src/dsp/voice.rs chimera-core/src/meter.rs chimera-core/src/instrument.rs chimera-core/tests/filter_level_test.rs docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "LEVEL clips past unity behind a 20 ms fade; per-Part FILTER_CLIP meter"
```

### Task 8: 2040

**Files:**
- Create: `chimera-core/src/dsp/filter/k2040.rs`
- Modify:
  - `chimera-core/src/dsp/filter/mod.rs` (`FilterKind::K2040 = 4`, `BUILT`, `Filter`);
  - `chimera-core/src/dsp/voice.rs` (`Clean` becomes `Filter`);
  - `chimera-core/src/params.rs` (`KIND_NAMES`, KIND's label "TYPE", max);
  - `probe.rs`;
  - `filter_types_test.rs`;
  - `disk_codes_test.rs` and the fixture.

**Interfaces:**
- **Produces:**
  - `FilterKind::K2040`, code 4, ident "2040", label "2040". `modes() = [Lp24]`, `default_mode() = Lp24`, `mode_labels() = ["LP4"]`, `key_default() = 0`.
  - `KIND_NAMES = ["CLEAN", "2040", "DIRTY"]` in `BUILT` order (DIRTY comes in Task 9). KIND's `ParamSpec` label is "TYPE"; its ident stays "KIND".
  - `pub struct K2040 { s: [f32; 4], g: Option<f32>, k: Option<f32>, pre: Option<f32> }`, with `pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams, fs: u32, mods: &FilterMods) -> f32`. The return is the clip amount from `max|(1 − G)(u − s₁)|`, under the same gate as CLEAN's.
  - The model is spec § 3's pseudocode, with `k = 4·RES/0.95` and `D = 1/(1 + k·G⁴)`. On a moving block `G` and `D` are divided per sample.
  - `sat_2040(v)` = `c − (4/27)·c³`, with `c = clamp(v, ±1.5)`.
  - **Constants:** `ALPHA_2040 ≈ 0.1` and `L_2040 = 0.25` are tuned at build by tests 2 and 7; `OUT_2040` is pinned so that LEVEL ½, RES 0 is 0 dB ± 0.5. LEVEL's gain feeds `x_c`, and the clipper does not run.
  - **`Filter`:**
    - `pub struct Filter { clean: Clean, k2040: K2040, level: LevelStage, last: Option<(FilterKind, FilterMode)> }`;
    - `pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams, fs: u32, mods: &FilterMods) -> f32`;
    - `pub fn hold(&mut self)`.
    - *Until Task 10* a TYPE change zeroes the incoming state, and the crossfade comes in Task 10.
  - `FilterKind::cost(Lp24)` for K2040 is the steady bill: 10, pending Task 18's recount.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn k2040_small_signal_corner_matches_the_linear_prototype() {
    // −3 dB point within ±5 % of the linear 4-pole-with-feedback prototype at input 0.01, FREQ 250/1k/4k, RES 0..0.8.
}
#[test] fn k2040_bass_loss_is_authentic_at_small_and_full_scale() {   // test 7
    // Gain at 100 Hz, FREQ 2 kHz, RES 0..=1 in tenths, input 0.01 and 1.0: monotonically falling, within ±1 dB of
    // −20·log10(1 + k); at RES 0.95 −14 ± 1 dB at both levels; resonant-peak gain at full scale for RES ≤ 0.8
    // within K2040_PEAK_DB (pinned here) of small-signal.
}
#[test] fn k2040_self_oscillates_in_band_and_on_pitch() {          // test 2
    // RES 1, impulse, as Task 3: level ±1 dB and inside OSC_BAND_DB over FREQ 100 Hz..8 kHz;
    // FM2 +100 %, C2..C6: octaves ±10 cents, absolute ±25 cents up to 5 kHz; onset RES 0.95 ± 0.01.
}
#[test] fn k2040_unity_at_level_half_res_0() {
    // A 100 Hz sine at −20 dBFS, FREQ 20 kHz, LEVEL 0.5, RES 0: gain 0 ± 0.5 dB.
}
#[test] fn k2040_one_block_jumps_and_bounds() {                     // tests 4 and 8
    // Task 2's jumps vs a K2040 f64 reference (common::filter_ref::k2040_ref) ≤ 1e-3 at RES 0; bounded at RES 1;
    // the bound grid × LEVEL 0, ½, 1: finite, ≤ K2040_BOUND (pinned).
}
#[test] fn k2040_recovers_from_a_nan_block() { /* as Task 3 */ }
#[test] fn kind_codes_round_trip() {
    // KIND 4 round-trips through a Sound file; MODE stays LP24; KIND_NAMES.len() == BUILT.len().
}
```

- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.** Tune `L_2040` with α first (the pitch test), then α (test 7 and the band), then `OUT_2040`. Write each tuned value and its reason in § Measured. If the 2040 can't land inside `OSC_BAND_DB`, widen the band only as far as it needs, and record that.
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - `L = ∞`, i.e. no limiter (the pitch test should fail: about −330 cents);
  - α = 0.5 (test 7 at full scale should fail);
  - `sat` on `(x − y)` without `(1 − G)` (the corner test should fail).
- [ ] **Step 5: The probe.** Steady: 0 `vdiv`, ≤ 70 instructions. Moving: 2 `vdiv`, ≤ +6. Then the fixture: run `disk_codes_test` and append `E 10 6 4 2040`.
- [ ] **Step 6: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`.

```bash
git add chimera-core/src/dsp/filter chimera-core/src/dsp/voice.rs chimera-core/src/params.rs chimera-core/tests/common/filter_ref.rs chimera-core/tests/filter_types_test.rs chimera-core/tests/disk_codes_test.rs chimera-core/tests/fixtures/disk_codes_v1.txt docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "2040: four OTA stages, a 3080 feedback limit, authentic bass loss"
```

### Task 9: DIRTY

**Files:**
- Create: `chimera-core/src/dsp/filter/dirty.rs`
- Modify: `chimera-core/src/dsp/filter/mod.rs` (`FilterKind::Dirty = 6`, `FilterMode::Hp12 = 9`), `probe.rs`, `filter_types_test.rs`, `disk_codes_test.rs` (`table()` emits MODE codes for every built kind), the fixture

**Interfaces:**
- **Produces:**
  - `FilterKind::Dirty`, code 6, ident "DIRTY". `modes() = [Lp12, Bp12, Hp12, Notch]`, `default_mode() = Lp12`, `mode_labels() = ["LP2","BP2","HP2","NOTCH1"]`.
  - `FilterMode::Hp12`, code 9, ident "HP12"; it is in no CLEAN list.
  - `pub struct Dirty { lp: f32, bp: f32, f: Option<f32>, q: Option<f32>, pre: Option<f32> }`, with `process(…) -> f32`, the same signature as `K2040`'s.
  - **Per block:**
    - `f = 2·fast_sin(π·fc/(2·fs))` and `q = 2·(1 − RES/0.95)`;
    - `f` is clamped to `0.95·(√(q₊² + 4) − q₊)`, where `q₊` is the block's largest `q` (one `vsqrt`);
    - both ramp per sample.
  - **Per sample:**
    - two sub-steps with the input held;
    - `lp`, `bp ∈ ±2`;
    - `out = clamp(½·½·(m₁ + m₂), ±1)`;
    - NOTCH1 = `lp + hp`.
  - LEVEL's clipper runs before DIRTY as on CLEAN.
  - **Fallbacks.** An unknown KIND code loads as CLEAN, and an unknown MODE code as the kind's `default_mode()`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn dirty_responses() {
    // RES 0: LP2/HP2 ±12 ± 2 dB/oct over 2..4 × FREQ; BP2 peak at FREQ ± 3 %; NOTCH1 at FREQ ± 5 % up to fs/8.
}
#[test] fn dirty_ceiling_is_12k_at_res_0_and_rises_with_res() {
    // The f clamp binds above 12.35 kHz ± 1 % at RES 0; at RES 0.95 no clamp below 20 kHz.
}
#[test] fn dirty_is_bounded_everywhere() {
    // The bound grid (incl. LEVEL 0, ½, 1): output ⊂ [−1, 1], lp and bp ⊂ [−2, 2], all finite.
}
#[test] fn dirty_self_oscillates_in_band_and_on_pitch() {
    // RES 1: level ±1 dB and inside OSC_BAND_DB; C2..C6 octaves ±10 cents, absolute ±25 cents up to fs/8; onset 0.95 ± 0.01.
}
#[test] fn every_type_self_oscillates_in_one_band() {
    // CLEAN's modes but NOTCH2, the 2040 and DIRTY at RES 1, FREQ 1 kHz: max level − min level ≤ OSC_BAND_DB width.
}
#[test] fn dirty_recovers_from_a_nan_block() { /* as Task 3 */ }
#[test] fn unknown_codes_load_as_clean_lp4() {
    // KIND code 5 → CLEAN at LP4; DIRTY with MODE code 2 (LP24) → LP2 (its default); MODE 9 round-trips on DIRTY.
}
```

- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - drop the `f` clamp (the bound test at RES 0 and 20 kHz should fail);
  - drop the output ½ (the band test should fail).
- [ ] **Step 5: The probe** (any mode: ≤ 30 instructions, 0 `vdiv`), then the fixture lines: `E 10 6 6 DIRTY` and `E 10 7 9 HP12`.
- [ ] **Step 6: Verify and commit.**

```bash
git add chimera-core/src/dsp/filter chimera-core/tests/filter_types_test.rs chimera-core/tests/common/filter_ref.rs chimera-core/tests/disk_codes_test.rs chimera-core/tests/fixtures/disk_codes_v1.txt docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "DIRTY: Chamberlin SVF at 2x, Jury-bounded f, hard clamps"
```

### Task 10: TYPE and MODE warm start and crossfade

**Files:**
- Modify: `chimera-core/src/dsp/filter/mod.rs`, `clean.rs`, `k2040.rs` and `dirty.rs` (the warm-start setters)
- Create: `chimera-core/tests/filter_switch_test.rs`

**Interfaces:**
- **Produces:**
  - `Filter` gains `xfade: u8` (blocks left, 0–2), `from: (FilterKind, FilterMode)` and `pending: Option<(FilterKind, FilterMode)>`.
  - **Warm-start setters:**
    - `Clean::outputs(&self) -> (f32, f32)` (`lp`, `bp` of stage 0);
    - `K2040::seed(&mut self, lp: f32)`, which sets every `sᵢ`;
    - `K2040::y4(&self) -> f32`;
    - `Dirty::seed(&mut self, lp: f32, bp: f32)`;
    - `Clean::seed(&mut self, lp: f32, bp: f32)` (both stages);
    - `Clean::seed_notch2(&mut self)` (`ap[i] ← lp`).
    - The map is spec § 1's.
    - A type left is zeroed.
  - **The crossfade.** It runs the old (kind, mode) and the new one for 2 blocks (128 samples), crossfaded linearly.
    - A MODE change within CLEAN runs the old mode on a stack copy of `Clean`; the copy and the second buffer are ≤ 256 B.
    - **A change during a crossfade is held in `pending`** and starts when the crossfade ends. That is at most 2 blocks (2.7 ms) late, and it never stacks.
  - **Billing.** Nothing is billed for the crossfade blocks.

- [ ] **Step 1: Write the failing tests** (test 5, plus the first Review Focus item):

```rust
#[test] fn every_type_and_mode_change_lands_near_steady_without_a_click() {
    // Held saw 110 Hz, FREQ 1 kHz, RES 0.5; for every (from, to) pair over all (type, mode): change mid-note.
    // From the crossfade's end: max |y − y_dest_steady| / max|y_dest_steady| ≤ 0.10 (y_dest_steady: the destination
    // run alone from the start); Tape::click finds no click anywhere.
}
#[test] fn type_scroll_inside_a_crossfade_lands_on_the_last_without_a_click() {
    // CLEAN → 2040 → DIRTY → CLEAN on consecutive blocks: no click; 4 blocks after the last pending starts the output is
    // within 10 % of CLEAN's steady.
}
#[test] fn a_type_change_leaves_mod_state_byte_identical() {
    // Sound::init(Algo) with routes; set KIND through Block::set; mod_state bits_eq before and after, and the TapRoutes equal.
}
#[test] fn random_changes_stay_bounded() {
    // 10 s, per-block random FREQ, RES, MODE and TYPE on white noise ±1: all finite, within each type's pinned bound.
}
```

  - **Task 3's review (F2):** the first test must include a MODE switch at RES 1, in self-oscillation, not only RES 0.5. Today the switch steps 0.62–1.42 (BP24 → LP12, LP24 → PHASER, HP24 → LP6 at 500 Hz) against a ring step of about 0.04: the trim steps with the mode on top of the topology.
  - **Task 4:** `param_sweep_test::clicks_ok` allows a Filter MODE jump (LP1 → NOTCH2, knob 0 → 7, steps 48× the steady state on TRI PROBE). Remove that allowance here; the crossfade must pass the sweep without it.
- [ ] **Step 2: Run them and see them fail.** Expect FAIL (today a TYPE change zeroes the state, so the deviation is about 47 %).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - skip the warm start (deviation > 10 %);
  - apply a pending change immediately (the scroll test should click).
- [ ] **Step 5: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`; also run `just stack-check`, which must still pass with the 256 B.

```bash
git add chimera-core/src/dsp/filter chimera-core/tests/filter_switch_test.rs
git commit -m "TYPE and MODE warm-start and crossfade over two blocks"
```

### Task 11: The four permanent matrix columns and full-registry migration

**Files:**
- Create: `chimera-core/tests/fixed_columns_test.rs`
- Modify:
  - `chimera-core/src/modulation.rs`;
  - `chimera-core/src/mod_path.rs` (`ModDestRegistry::add` refuses fixed addresses);
  - `chimera-core/src/ui/mod_grid.rs` (the `MatrixState` columns; `inert_dests` becomes `u32`);
  - `chimera-core/src/dsp/voice.rs` (offsets and `VcaRoutes` from fixed);
  - `chimera-core/src/preset.rs` (`Sound::init` routes into fixed FREQ, with no registry entry);
  - `chimera-core/src/storage/sound.rs` (decode into fixed; encode fixed as route records);
  - `chimera-core/src/ui/view.rs` (route views read fixed);
  - `chimera-core/tests/fixtures/neutral_v1.txt` (the new fields only);
  - tests that index CUTOFF as column 0 (`matrix_view_test.rs`, `ui_routing_test.rs`, `modulation_test.rs`, `cost_test.rs`).

**Interfaces:**
- **Produces, in `modulation.rs`:**
  - `#[repr(u8)] pub enum FixedCol { Freq = 0, Q = 1, Vca = 2, Fold = 3 }`.
    - `pub const ALL: [FixedCol; 4]`.
    - `pub const fn addr(self) -> ParamAddr`: CUTOFF, `(Filter, RESONANCE)`, `VCA` and `(Folder, FOLD)`.
    - `pub fn of(a: ParamAddr) -> Option<FixedCol>`.
    - `pub const fn tag(self) -> &'static str`: "FREQ", "Q", "VCA", "FOLD".
  - `ModState` gains `fixed: [[i8; 4]; MAX_MOD_SOURCES]` and `fixed_present: [u8; 4]`, plus:
    - `pub fn fixed_amount(&self, s: usize, c: FixedCol) -> i8`;
    - `pub fn fixed_present(&self, c: FixedCol) -> u8`;
    - `pub fn set_fixed(&mut self, s: usize, c: FixedCol, a: i8)` (sets presence);
    - `pub fn clear_fixed(&mut self, s: usize, c: FixedCol)`;
    - `pub fn fixed_sum(&self, c: FixedCol, v: &[f32; MAX_MOD_SOURCES]) -> f32`;
    - `pub fn set_route_addr(&mut self, s: usize, a: ParamAddr, amount: i8) -> bool` (fixed, else find-or-push).
  - **Fixed-aware:** `routes_into`, `moves_addr` and `offset_for` (by address); `bits_eq` (covers fixed).
  - **Registry-only:** `push`/`push_dest` refuse fixed addresses; `dest(d)`, `amount(s, d)` and `num_dests()` are unchanged.
  - **Billing.** `ModRouting::dest_cost` counts moving fixed columns other than VCA. `vca_cost` reads `routes_into(VCA)`.
- **Produces, in `mod_grid.rs`:**
  - `pub enum Col { Fixed(FixedCol), Reg(usize) }` and `pub fn col(&self, c: usize) -> Col` (`c < 4` is fixed). The columns total `4 + num_dests`.
  - `MatrixState` gains `fixed` and `fixed_present` as `ModState` has them.
  - `load_amounts` and `sync_from_matrix` carry them.
  - `pub fn inert_dests(…) -> u32`, with its assert on `u32::BITS`.
- **Voice.**
  - Fixed FREQ, Q and FOLD apply through `apply_offset` before the registry loop.
  - `VcaRoutes::of` reads `fixed_present(Vca)` and `fixed_amount`.
  - The lifetime rule reads `VcaRoutes`, unchanged.
- **Disk.**
  - On save, each fixed route is a route record with its address, and ModDests lists only registry columns.
  - On load, a ModDests or Registry entry for a fixed address is dropped, and its routes go into the fixed column through `set_route_addr`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn a_full_registry_patch_keeps_every_route_and_frees_four_columns() {
    // Hand-built file: 16 ModDests incl. CUTOFF, RESO, OUT VCA, FOLD + 12 others, routes on all: after decode every
    // route reads its amount (fixed or registry), num_dests() == 12, and four pushes succeed.
}
#[test] fn full_registry_with_drive_and_cutoff_keeps_every_route() {  // Review Focus 3
    // 16 dests incl. (10, 2) DRIVE and CUTOFF: DRIVE's routes at (Filter, LEVEL), CUTOFF's in fixed Freq, num_dests() == 15.
}
#[test] fn fixed_columns_round_trip_through_a_save() {
    // Sound with fixed routes on all four + registry routes: decode(encode(s)).bits_eq(&s).
}
#[test] fn init_routes_live_in_fixed_freq() {
    // Sound::init(e): fixed_present(Freq) has ENV1, LFO1, NOTE; dest_registry.len() == 0; num_dests() == 0.
}
#[test] fn fixed_routes_drive_the_voice_as_registry_routes_did() {
    // A voice with LFO1 → CUTOFF at 64 renders bit-identical to the same patch before (golden of the pre-change render,
    // recorded first in this task from the old code path).
}
#[test] fn vca_rule_reads_the_fixed_vca_column() {
    // ENV2 → VCA in fixed: the voice ends when ENV2 does (as vca_test's existing case, ported).
}
```

- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.** In `neutral_v1.txt`, add only the new fields (all zero). Port the existing tests that assumed CUTOFF at column 0 to `Col::Fixed(Freq)`.
- [ ] **Step 4: Run them and see them pass.** Then run `memory_budget_test`: `ModState` grows 36 B and `MatrixState` the same, and ADR 0051's assert must hold. Mutation-check:
  - let `push` accept fixed addresses (the free-columns test should fail);
  - drop fixed from `bits_eq` (the round trip should fail).
- [ ] **Step 5: Re-record** the matrix screen goldens that move (`mod_matrix`, `mod_matrix_wide` and `mod_matrix_morph_inert`; the reason is "four fixed columns"). Then run `CARGO_INCREMENTAL=0 just check`.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/modulation.rs chimera-core/src/mod_path.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/view.rs chimera-core/src/dsp/voice.rs chimera-core/src/preset.rs chimera-core/src/storage/sound.rs chimera-core/tests/fixed_columns_test.rs chimera-core/tests/fixtures/neutral_v1.txt chimera-core/tests/matrix_view_test.rs chimera-core/tests/ui_routing_test.rs chimera-core/tests/modulation_test.rs chimera-core/tests/cost_test.rs chimera-core/tests/screen_golden_test.rs
git commit -m "Four fixed matrix columns, FREQ Q VCA FOLD, outside the registry; old patches migrate"
```

### Task 12: Jacks, the per-type panels, and FM3 and QM as cell views

**Files:**
- Create: `chimera-core/src/taps.rs` (`TapOp`, `Jack` only)
- Modify:
  - `chimera-core/src/lib.rs`;
  - `chimera-core/src/params.rs` (Filter 8 and 11);
  - `chimera-core/src/block.rs` (`ParamSpec::patching()`);
  - `chimera-core/src/ui/filter_panel.rs`;
  - `chimera-core/src/ui/block_registry.rs` (FLT › MOD's six cells; its name, and a chain `map` label "MDE");
  - `chimera-core/src/ui/view.rs`;
  - `chimera-core/src/storage/sound.rs` (`infer_jacks`);
  - `chimera-core/tests/common/sweep.rs` (skip `patching` specs);
  - `disk_codes_test.rs` and the fixture;
  - `chimera-core/tests/flt_page_test.rs`.

**Interfaces:**
- **Produces, in `taps.rs`:**
  - `#[repr(u8)] pub enum TapOp { Op5 = 0, Op6 = 1 }`, with `pub const fn op_index(self) -> usize` (4 or 5) and `tag()` "OP5"/"OP6".
  - `pub enum Jack { Mod(ModSource), Tap(TapOp) }`.
    - `impl DiskCode`: codes 0–7 are `ModSource::disk_code`, Op5 is 8 and Op6 is 9. Idents are the ModSource idents, then "OP5" and "OP6".
    - `pub const ALL: [Jack; 10]`.
    - `pub const FM3_CHOICES: [Jack; 8] = [LFO1, LFO2, LFO3, ENV2, ENV3, VEL, OP5, OP6]`.
    - `pub const QM_CHOICES: [Jack; 10]`: `ModSource::ALL`, then OP5 and OP6.
    - `pub fn effective(self, engine: EngineType, default: Jack) -> Jack`: a Tap on a non-ALGO engine gives `default`.
- **Produces, in `params.rs`:**
  - `FilterParams::FM3_IN = ParamId(8)` and `QM_IN = ParamId(11)`.
  - Fields `fm3_src: Jack` (default `Mod(Lfo1)`) and `qm_src: Jack` (default `Mod(Env3)`).
  - Specs: `choice(8, "FM3 IN", Names(&FM3_IN_NAMES), 7.0, 0.0).ident("FM3IN").patching()` and `choice(11, "QM IN", Names(&QM_IN_NAMES), 9.0, 3.0).ident("QMIN").patching()`.
- **Produces, in `block.rs`:** `ParamSpec::patching(self) -> Self` and `pub patching: bool`. The sweep registry skips patching specs, and MIDI learn reads the flag when it lands.
- **Produces, in `filter_panel.rs`:**
  - `pub enum JackSlot { Fm3, Qm }`, with `col() -> FixedCol` (Freq, Q), `param() -> ParamId` and `default() -> Jack`.
  - `pub enum PanelTarget { Filter(ParamId), Route(ModSource), Keybd, JackAmount(JackSlot), Fixed(ParamId) }`.
  - `pub struct KindPanel { pub main: [PanelKnob; 5], pub mods: [Option<PanelKnob>; 6] }`.
  - `knob(kind, k)`: 0–4 are `main`, 5–10 are `mods`.
  - **The panels** (spec § 2, § 3, § 4):
    - CLEAN `main`: FREQ, Q, MODE, FM1 (`Route(Env1)`), FM2 (`Route(Note)`). `mods`: LEVEL, FM3, FM3 IN, QM, QM IN, none.
    - 2040 `main`: CUTOFF, RES, ENV AMT (`Route(Env1)`), KEYBD (`Keybd`), DRIVE (`Filter(LEVEL)`). `mods`: none, FM3, FM3 IN, QM, QM IN, MODE (`Fixed(MODE)`, dimmed).
    - DIRTY: CLEAN's words.
  - `pub fn jack_amount(sound: &Sound, slot: JackSlot) -> i8`: the cell (effective jack, `slot.col()`).
  - `pub fn set_jack_amount(sound: &mut Sound, slot: JackSlot, a: i8)`: writes that cell; no second copy.
  - `pub fn keybd_step(amount: i8, dir: i8) -> i8`: OFF 0, HALF 64, FULL 127, the next step in `dir`.
  - `pub fn repatch(sound: &mut Sound, slot: JackSlot, new: Jack) -> bool`. This task does Mod → Mod; Task 13 adds the Tap arms. It moves the amount and presence to the new source's cell, refuses an occupied cell (returns false and changes nothing), and sets the jack param.
- **Produces, in `storage/sound.rs`:**
  - `fn infer_jacks(s: &mut Sound, had_fm3: bool, had_qm: bool)`, run in `finish`. FM3 IN comes from the single fixed-FREQ route whose source is neither ENV 1 nor NOTE; QM IN comes from the single fixed-Q route; otherwise each defaults.
  - `SoundDecoder` notes whether the Filter block record held ids 8 and 11.
- **Route delete.** MIX + MINUS on any route view (FM1, FM2, FM3, QM, ENV AMT, KEYBD) deletes that cell's route, and the view's hint is `mod_grid::DELETE`. The drawing comes in Task 17.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn jack_codes_round_trip_and_extend_mod_source() {
    // for s in ModSource::ALL: Jack::Mod(s).disk_code() == s.disk_code(); Op5 = 8, Op6 = 9; from_disk_code(10) is None;
    // Filter 8 and 11 round-trip through a Sound file for every choice.
}
#[test] fn fm3_and_qm_are_views_of_one_cell() {
    // set_jack_amount(Fm3, 50) ⇒ mod_state.fixed_amount(LFO1, Freq) == 50; set the cell in MatrixState ⇒ jack_amount reads it;
    // the same for FM1 (ENV1, Freq), FM2 (NOTE, Freq) and QM (ENV3, Q).
}
#[test] fn repatch_moves_exactly_one_route() {
    // FM3 IN LFO1 → LFO2 with LFO1→FREQ at 40: LFO2→FREQ == 40 present, LFO1→FREQ absent; every other cell unchanged.
}
#[test] fn repatch_with_every_target_occupied_changes_nothing() { // Review Focus 5
    // Every FM3 choice's FREQ cell occupied: repatch to each returns false; mod_state and fm3_src unchanged.
}
#[test] fn load_without_jacks_infers_them() {
    // File with LFO2→CUTOFF only (no Filter 8): fm3_src == Mod(Lfo2); two such routes: default LFO1; ENV2→RES: qm_src == Mod(Env2).
}
#[test] fn panels_follow_the_type() {
    // knob(Svf, 0..=4) labels FREQ Q MODE FM1 FM2; knob(K2040, 4) is DRIVE → Filter(LEVEL); knob(K2040, 10) is Fixed(MODE);
    // knob(K2040, 5) is None; every label fits its cell (≤ the cell's text width).
}
#[test] fn keybd_steps_off_half_full() {
    // keybd_step(0, 1) == 64; (64, 1) == 127; (127, 1) == 127; (100, −1) == 64; (30, 1) == 64.
}
#[test] fn qm_from_a_stepped_lfo_square_does_not_click() {
    // QM IN LFO1 (square, 4 Hz), QM 100 %, held saw: Tape::click finds none.
}
#[test] fn repatching_mid_note_does_not_click() {
    // FM3 IN and QM IN repatched every 200 ms on a held saw: no click.
}
```

  Also: the jack params are absent from `common::sweep::registry`.
- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.** Fixture: append `B 10 8 FM3IN`, `B 10 11 QMIN`, their `E` lines, and a `C JACK` table of all ten codes (`coded("JACK", …)`).
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - let `repatch` overwrite an occupied cell (the occupied test should fail);
  - infer FM3 IN from ENV 1 (the inference test should fail).
- [ ] **Step 5: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`. The FLT goldens move here or in Task 17; re-record them in the task that moves them.

```bash
git add chimera-core/src/taps.rs chimera-core/src/lib.rs chimera-core/src/params.rs chimera-core/src/block.rs chimera-core/src/ui/filter_panel.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/view.rs chimera-core/src/storage/sound.rs chimera-core/tests/common/sweep.rs chimera-core/tests/disk_codes_test.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/tests/flt_page_test.rs
git commit -m "FM3 IN and QM IN jacks; per-type panels; FM3 and QM view their matrix cells"
```

### Task 13: Tap records, the OP5 and OP6 rows, and X cells

**Files:**
- Modify:
  - `chimera-core/src/taps.rs` (`TapRoutes`);
  - `chimera-core/src/params.rs` (`ParamSnapshot::taps`);
  - `chimera-core/src/storage/record.rs` (`RecordTag::TapRoutes`);
  - `chimera-core/src/storage/sound.rs` (encode and decode);
  - `chimera-core/src/ui/mod_grid.rs` (rows 8–9 and X cells, as logic);
  - `chimera-core/src/ui/filter_panel.rs` (`repatch`'s Tap arms, `jack_amount` on taps);
  - `chimera-core/src/preset.rs` (`bits_eq` covers taps);
  - `chimera-core/tests/fixtures/neutral_v1.txt` (the empty `taps` field).
- Create: `chimera-core/tests/taps_test.rs`

**Interfaces:**
- **Produces, in `taps.rs`:**
  - `pub struct TapRoutes { amount: [[i8; 4]; 2], present: u8 }`, indexed `[TapOp][FixedCol]`, with bit `op·4 + col`.
    - `pub fn get(&self, op: TapOp, c: FixedCol) -> Option<i8>`;
    - `pub fn set(&mut self, op: TapOp, c: FixedCol, a: i8)`;
    - `pub fn clear(&mut self, op: TapOp, c: FixedCol)`;
    - `pub fn moving(&self, c: FixedCol) -> bool` (any present, nonzero amount into `c`);
    - `pub fn wanted(&self) -> [bool; 2]` (ops with any moving entry).
  - Destinations are `FixedCol` only, so a tap into an op's pitch or level can't be expressed.
  - `pub fn to_tap_amount(a: i8) -> i8` = `clamp(round(a·10/4), ±127)` and `pub fn from_tap_amount(a: i8) -> i8` = `round(a·4/10)`, for FREQ only. Q's amount is unchanged.
- **`ParamSnapshot`** gains `pub taps: TapRoutes` (default empty). It is not a `Block`.
- **The record.** `RecordTag::TapRoutes`, at the next free low code in `record.rs`'s table (`0x000C` if still free), read once per Sound.
  - Its payload is 4-byte entries: `(Jack code, dest block code, dest param id, amount as u8)`.
  - On load each entry is validated: the Jack code must be 8 or 9, `addr_of` must give a valid address, and `FixedCol::of` must accept it. A bad entry is dropped.
  - An empty `TapRoutes` writes no record.
- **`MatrixState`:**
  - `pub fn rows(&self) -> usize`: 10 on ALGO, else 8.
  - `pub enum RowSource { Mod(usize), Tap(TapOp) }` and `pub fn row_source(&self, row: usize) -> RowSource`; rows 8 and 9 are OP5 and OP6, after `ROW_ORDER`.
  - `pub fn is_x(&self, row: usize, col: usize) -> bool`: a tap row and a non-fixed column.
  - `set`, `adjust_amount` and `delete_selected` ignore X cells.
  - `pub fn load_taps(&mut self, t: &TapRoutes)` and `pub fn sync_taps(&self, t: &mut TapRoutes)`.
  - `pub fn hint(&self) -> Hint` returns `X_OP5` or `X_OP6` on an X cell.
  - The hint consts are `pub const X_OP5: Hint = &[Gesture("OP5 · AUDIO RATE:", "FREQ Q VCA FOLD ONLY")]`, and `X_OP6` the same.
- **`repatch` arms.**
  - Mod → Tap moves `fixed_amount` into `TapRoutes` through `to_tap_amount` (FREQ) or unchanged (Q).
  - Tap → Mod is the reverse.
  - Tap → Tap moves the entry.
  - Each refuses an occupied target.

- [ ] **Step 1: Write the failing tests** in `taps_test.rs`:

```rust
#[test] fn tap_routes_round_trip_and_bad_entries_drop() {
    // Every (op, col) at ±127, 0, 50: decode(encode) equal; hand-built entries with jack 3, jack 10, dest (Algo op 5, LEVEL),
    // dest (Filter, LEVEL), a 3-byte tail: the bad ones dropped (a 3-byte tail is Bounds), the good ones kept.
}
#[test] fn x_cells_refuse_every_edit() {
    // ALGO MatrixState, row 9, every registry col: set, adjust_amount(±10), delete_selected leave taps and amounts unchanged.
}
#[test] fn tap_rows_only_on_algo() {
    // rows() == 10 on ALGO, 8 on MODAL; row_source(8) == Tap(Op5).
}
#[test] fn mod_tap_mod_keeps_the_depth() {
    // FM3 IN LFO1 at a, for a in −50..=50: repatch to OP6 then back: |a' − a| ≤ 1, and the tap read to_tap_amount(a).
}
#[test] fn no_mod_source_caller_sees_a_tap() {
    // ModSource::ALL.len() == MAX_MOD_SOURCES == 8; ModState::num_sources() ≤ 8 after loading a file with taps on all 8 entries.
}
#[test] fn qm_on_a_tap_views_the_tap_cell() {
    // qm_src = Tap(Op5): set_jack_amount(Qm, 30) ⇒ taps.get(Op5, Q) == Some(30); MatrixState row 8, col Q reads 30.
}
```

- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - let X cells accept `set` (that test should fail);
  - skip `to_tap_amount` (the depth test should fail).
- [ ] **Step 5: Verify and commit.**

```bash
git add chimera-core/src/taps.rs chimera-core/src/params.rs chimera-core/src/storage/record.rs chimera-core/src/storage/sound.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/filter_panel.rs chimera-core/src/preset.rs chimera-core/tests/taps_test.rs chimera-core/tests/fixtures/neutral_v1.txt
git commit -m "TapRoutes record; OP5 and OP6 matrix rows on ALGO with X cells"
```

### Task 14: Tap capture and the four per-sample destinations

**Files:**
- Modify:
  - `chimera-core/src/taps.rs` (`TapRows`, `TapLane`, `tap_vca`);
  - `chimera-core/src/dsp/algo/kernel.rs` and `chimera-core/src/dsp/algo/engine.rs` (capture);
  - `chimera-core/src/dsp/engines.rs` (`EngineSlot::render` takes `&mut TapRows`);
  - `chimera-core/src/dsp/filter/coef.rs` (`exp2_poly`);
  - `clean.rs`, `k2040.rs`, `dirty.rs` and `mod.rs` (`FilterMods` lanes);
  - `chimera-core/src/dsp/wavefolder.rs`;
  - `chimera-core/src/dsp/voice.rs` (the chain order, tap ramps);
  - `probe.rs`;
  - `chimera-core/tests/taps_test.rs`.

**Interfaces:**
- **Produces:**
  - `pub struct TapRows { pub want: [bool; 2], pub rows: [[f32; BLOCK_SIZE]; 2] }`, 512 B on the stack.
    - The kernel fills `rows[i]` only when `want[i]`, with the op's own output sample clamped to ±1: a row copy in `render_ops`, one store per sample in `render_samples`.
    - A silent op's row is zeros.
    - The MODAL slot leaves the rows untouched.
  - `pub struct TapLane<'a> { pub rows: &'a [[f32; BLOCK_SIZE]; 2], pub from: [f32; 2], pub to: [f32; 2] }`.
    - Amounts are `amount/127` and 0 when absent, ramped `from → to` across the block.
    - `pub fn at(&self, n: usize) -> f32` = Σ over ops of the ramped amount × `rows[op][n]`.
  - `FilterMods<'a>` gains `pub freq: Option<TapLane<'a>>` and `pub q: Option<TapLane<'a>>`.
  - `pub fn exp2_poly(x: f32) -> f32`: a 3rd-order polynomial on the fraction plus an exponent-bit add, divide-free, about 0.1 % accurate.
  - **FREQ:**
    - CLEAN, and NOTCH2's `g`: `g[n] = clamp(g_blk·exp2_poly(4·lane.at(n)), g_min, g_max)`, where `g_min = g_of(20, fs)` and `g_max = tan_accurate(0.45)`, computed per block.
    - 2040: the same on `g`.
    - DIRTY: on `f`, clamped to the block's `f` bound.
  - **Q:**
    - `RES[n] = clamp(RES_blk + lane.at(n), 0, 1)`, mapped per sample to `k` (CLEAN), `k` (2040), `fb` (NOTCH2) or `q` (DIRTY).
    - For DIRTY, `q₊` for the `f` bound takes RES 1's `q` while a Q tap is present.
  - **Moving.** A FREQ or Q tap makes the block a moving block.
  - `Wavefolder::process(&mut self, buf, p, on: bool, tap: Option<&TapLane>)`: `fold[n] = clamp(fold_blk + lane.at(n), 0, 1)` on its per-sample path.
  - `pub fn tap_vca(buf: &mut [f32; BLOCK_SIZE], lane: &TapLane)`: `x[n]·Π over ops (1 − |aᵢ| + aᵢ·rows[i][n])`, per op present.
  - **The voice chain:** engine → DRIVE → filter → FOLD → **tap-VCA** → DC blocker → VCA.
  - `Voice` keeps `tap_amt: [[f32; 4]; 2]`, the last block's amounts, so every amount ramps. A fresh voice starts them at 0.
  - **Live taps.** `TapRoutes` is read only when `p.engine() == Algo`; otherwise every lane is `None` and `want` is `[false; 2]`.
  - **Folder gate.** `Voice::stage_runs` treats `taps.moving(Fold)` as a FOLD route.

- [ ] **Step 1: Write the failing tests** (test 9):

```rust
#[test] fn vca_tap_sidebands_follow_am_and_ring_theory() {
    // ALGO, OP6 ratio r, carrier sine at f_c: tap → VCA at 50 %: sidebands at f_c ± r·f_note within ±1 dB of AM theory
    // (carrier −0 dB, sidebands 20·log10(a/2)); at 100 %: carrier suppressed ≤ −40 dB (ring); at 0 %: sidebands < −90 dB.
}
#[test] fn freq_tap_makes_sidebands_on_lp2() {
    // Saw through CLEAN LP2, tap → FREQ 50 %: energy at harmonic ± r·f_note > −60 dB; at 0 %: < −90 dB.
}
#[test] fn fold_tap_moves_the_fold_at_the_op_rate() {
    // FOLD 0.2, tap → FOLD 100 %: the fold's spectrum has components at r·f_note offsets (> −60 dB); FOLD off + tap ⇒ stage_runs fold.
}
#[test] fn tap_vca_leaves_no_dc() {
    // OP6 as a 1:1 carrier tapped onto the VCA at 100 %: output DC < −80 dB after DC_FROM blocks.
}
#[test] fn taps_never_blow_up_or_alias_past_the_margin() {
    // Every (type, mode), tap → FREQ and → Q at 100 %, OP6 at 8 kHz, ratio √2, FREQ 10 kHz: finite and within the type's
    // bound; off-bin energy ≤ the tap-free filter's + TAP_ALIAS_DB (pinned here).
}
#[test] fn tap_enable_disable_and_repatch_are_click_free() {
    // Amount 0 → 100 % → 0 in UI steps, and FM3 IN LFO1 ↔ OP6 mid-note: Tape::click finds none.
}
#[test] fn taps_never_hold_a_voice() {
    // Tap → VCA only, ENV routes none, note off: the voice ends as the engine's own envelopes end (same block as without the tap).
}
#[test] fn taps_survive_an_engine_round_trip_and_are_inert_between() { // Review Focus 4
    // ALGO Sound with FM3 IN = OP6 and taps on all four; switch the Part to MODAL: renders finite, equals the same patch
    // with taps cleared and FM3 IN LFO1 (bit for bit); back to ALGO: TapRoutes equal to before, taps audible again.
}
#[test] fn exp2_poly_is_within_a_tenth_of_a_percent() {
    // x over −4..=4 step 1/64: |exp2_poly(x)/2^x − 1| ≤ 1e-3; exp2_poly(0) == 1.0.
}
#[test] fn capture_matches_the_op_output() {
    // render_ops and render_samples (a delayed plan): rows equal each op's own output, clamped ±1, for want = [true, true].
}
```

- [ ] **Step 2: Run them and see them fail.** Expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. Mutation-check:
  - put the tap-VCA after the DC blocker (the DC test should fail);
  - use the block's amount unramped (the click test should fail);
  - let MODAL read the taps (the round-trip test should fail).
- [ ] **Step 5: The probes.**
  - Tap capture is +1 per tapped op.
  - FREQ is +12 plus the moving path, Q +4 plus the moving path, VCA +4 and FOLD +4: 0 `vdiv` beyond the moving path's.
  - Record them.
- [ ] **Step 6: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check` (and `just stack-check`, which now has 512 B more).

```bash
git add chimera-core/src/taps.rs chimera-core/src/dsp chimera-core/tests/taps_test.rs docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "OP5 and OP6 taps reach FREQ, Q, FOLD and a tap-VCA before the DC blocker"
```

### Task 15: Billing the types, the moving path, LEVEL and the taps

**Files:**
- Modify: `chimera-core/src/dsp/filter/mod.rs`, `chimera-core/src/taps.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/tests/cost_test.rs`

**Interfaces:**
- **Consumes:** `OLD_LP24` and the probe counts in § Measured.
- **Produces:**
  - `FilterKind::cost(self, mode) -> Cost`: the steady bill. CLEAN: every mode 0, NOTCH2 0. 2040: 10. DIRTY: 0. Each value is `max(0, cycles − OLD_LP24)` from § Measured; replace a spec estimate wherever the count differs.
  - `FilterKind::move_cost(self) -> Cost`: CLEAN 35 (`CLEAN_MOVE_COST`), 2040 35, DIRTY 0.
  - `pub const FILTER_LEVEL_COST: Cost = Cost(20)`, billed while the LEVEL gate can open (stored LEVEL > 0.5, or LEVEL routed), on CLEAN and DIRTY only.
  - In `taps.rs`:
    - `pub const TAP_FREQ_COST: Cost = Cost(20)`;
    - `FilterKind::tap_q_cost(self) -> Cost`: CLEAN 7, 2040 7, DIRTY 10;
    - `pub const TAP_VCA_COST: Cost = Cost(10)`;
    - `pub const TAP_FOLD_COST: Cost = Cost(10)`.
    - Each is billed per present, nonzero entry.
  - `pub fn Voice::filter_cost(p: &ParamSnapshot, mods: &ModState) -> Cost` =
    - steady,
    - + `move_cost` once, if CUTOFF or RES moves (routed) or a FREQ or Q tap is present (never twice),
    - + LEVEL,
    - + the tap terms.
  - It replaces `p.filter.kind().cost(p.filter.mode())` in `Voice::cost`.
  - **FOLD taps.** `stage_cost` counts a FOLD tap as a FOLD route: `FOLD_COST` 45 if the folder would otherwise be off, and `FOLD_RAMP_COST` 29 if not already billed.
  - **Off ALGO** the tap terms are 0.
  - Each constant's doc comment gives its count: instructions, `vdiv`s, the arithmetic, the date and the commit.

- [ ] **Step 1: Write the failing tests.** Replace `each_svf_mode_bills_its_delta_over_lp24`:

```rust
#[test] fn filter_bills_each_type_and_mode() {
    // for k in BUILT, m in k.modes(): cost(m) ≥ 0 and equals the table here (CLEAN all 0, K2040 10, DIRTY 0 — or Task 1's
    // recount); the listed pairs cover every built (kind, mode) (count asserted).
}
#[test] fn the_moving_path_bills_once() {
    // CLEAN, LFO1 → CUTOFF at 64: + 35; + FREQ tap and Q tap as well: 35 + 20 + 7 (not 2 × 35); DIRTY: + 0 + 20 + 10.
}
#[test] fn taps_bill_each_destination() {
    // ALGO: one VCA tap + 10; FOLD tap with FOLD 0: + 10 + 29 + 45; with FOLD 0.5 stored: + 10 + 29;
    // two VCA taps (OP5 and OP6): + 20; any taps on MODAL: + 0; no taps: Voice::cost unchanged from before this task.
}
#[test] fn level_bills_while_it_can_clip() {
    // LEVEL 0.5: + 0; LEVEL 0.6: + 20; LEVEL 0.5 routed: + 20; on K2040 at LEVEL 1: + 0.
}
```

  Then update the voice-count rows in `cost_test` (the `("SVF", …, 500)` row and the STRESS counts) to what the new bills give.
- [ ] **Step 2: Run them and see them fail.** Run `CARGO_INCREMENTAL=0 cargo test -p chimera-core --test cost_test`; expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and see them pass.** Expect PASS. **If any voice count in `cost_test` falls,** stop and hand it to the owner: it is a regression in the bill. Mutation-check: bill `move_cost` per tap (the once test should fail).
- [ ] **Step 5: Verify and commit.**

```bash
git add chimera-core/src/dsp/filter/mod.rs chimera-core/src/taps.rs chimera-core/src/dsp/voice.rs chimera-core/tests/cost_test.rs
git commit -m "Bills per type and mode, the moving path once, LEVEL and each tap"
```

### Task 16: The default TYPE per engine

**Files:**
- Modify: `chimera-core/src/dsp/filter/mod.rs`, `chimera-core/src/params.rs` (`for_engine`), `chimera-core/src/preset.rs` (`Sound::init`'s key default)
- Create: `chimera-core/tests/fixtures/neutral_filter_v1.txt`
- Test: `chimera-core/tests/sound_codec_test.rs`

**Interfaces:**
- **Produces:**
  - `pub const fn FilterKind::default_for(e: EngineType) -> FilterKind`: Algo → Svf, Modal → Svf. TAPE and VA (→ K2040) add their arms when those engines land.
  - `ParamSnapshot::for_engine` sets `filter` to `default_for(e)` at its `default_mode()`.
  - `Sound::init` seeds NOTE → FREQ from `default_for(e).key_default()`.
  - `Sound::neutral` and so the decode base follow, as does the spec's `neutral_params`, which is `Sound::neutral(e).params`.
  - **The fixture** has one line per engine, `<engine code> <KIND code> <MODE code>`: `0 0 2` (ALGO) and `1 0 2` (MODAL). It is append-only.

- [ ] **Step 1: Write the failing test:**

```rust
#[test] fn neutral_filter_rows_are_pinned() {
    // Every EngineType::ALL has a row; Sound::init(e) and Sound::neutral(e) filter kind/mode codes equal it;
    // a file with no Filter KIND decodes to its engine's row.
}
```

- [ ] **Step 2: Run it and see it fail.** Expect FAIL (no fixture).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run it and see it pass.** Expect PASS. Mutation-check: have `for_engine` skip `default_for` and set `default_mode()` to LP2. The rows test must fail, since both rows read MODE 2 (LP4). Today every engine maps to CLEAN, so the mapping itself can't be mutation-checked until VA adds a row.
- [ ] **Step 5: Verify and commit.**

```bash
git add chimera-core/src/dsp/filter/mod.rs chimera-core/src/params.rs chimera-core/src/preset.rs chimera-core/tests/fixtures/neutral_filter_v1.txt chimera-core/tests/sound_codec_test.rs
git commit -m "Default filter TYPE per engine, set in the neutral Sound"
```

### Task 17: The UI and the screen goldens

**Files:**
- Modify:
  - `chimera-core/src/ui/draw.rs` (`x_mark`, `clip_led`);
  - `chimera-core/src/ui/mod_grid.rs` (row height, fixed columns, X cells, the hint);
  - `chimera-core/src/ui/viz.rs` (the new responses, the LED corner);
  - `chimera-core/src/ui/view.rs` and the renderer (the KEYBD text, the fixed MODE dimmed, the LED beside LEVEL);
  - `chimera-core/src/ui/mod.rs` (the meter read and the hold);
  - `docs/adr/0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md` (two table rows);
  - `chimera-core/tests/hint_test.rs`, `chimera-core/tests/screen/mod.rs`, `chimera-core/tests/screen_golden_test.rs`, `chimera-core/tests/screen_atlas_test.rs`.

**Interfaces:**
- **Produces:**
  - `pub fn x_mark<D>(d: &mut D, cx: i32, cy: i32)`: a 5 × 5 px X of two 1 px diagonals in `theme::MID`.
  - `pub fn clip_led<D>(d: &mut D, cx: i32, cy: i32, fill: f32)`: a 3 px `theme::ALERT` ring whose fill is a radius.
  - `pub struct ClipHold { v: f32, held: u16 }`, with `pub fn step(&mut self, meter: f32, dt_ms: u16) -> f32`: it holds the peak, then falls to 0 over 150 ms.
  - The LED isn't drawn while the meter reads 0 and the hold has ended.
  - `pub fn row_h(rows: usize) -> i32`: 14 for 10 rows, 17 for 8. Rows always fill 50 to 190; `CELL_INSET` is 4, which leaves a 10 px cell at 14.
  - **Viz.** `Response` draws LP1's resonant 6 dB slope, CLEAN NOTCH1's 2 × FREQ notch with the −12 dB treble, NOTCH2's two notches with Q-raised peaks, DIRTY NOTCH1 at FREQ, and the 2040's pass band lowered by `1/(1 + k)`.
  - **Labels.** KIND shows "TYPE" and FLT › MODE shows "MOD"; the chain `map` label stays "MDE".
  - **ADR 0086's table** gains two rows:
    - `| FLT and FLT › MOD | any route knob | MIX + MINUS DELETES THE ROUTE |`
    - `| MOD MATRIX | an X cell | OP5 · AUDIO RATE: FREQ Q VCA FOLD ONLY |`

- [ ] **Step 1: Write the failing tests:**

```rust
// hint_test.rs
#[test] fn route_knobs_name_the_delete() { /* FM1, FM2, FM3, QM on CLEAN, ENV AMT and KEYBD on 2040: band hint == mod_grid::DELETE */ }
#[test] fn x_cell_names_what_a_tap_reaches() { /* cursor on (row 8, registry col): hint == X_OP5; hint::width ≤ 216
   — if it fails, shorten both to Gesture("OP5:", "FREQ Q VCA FOLD ONLY") and keep the test */ }
// screen_golden_test.rs: new cases (recorded in this task)
// flt_clean, flt_mod_clean, flt_2040, flt_mod_2040 (MODE fixed and dimmed), flt_dirty, flt_mod_dirty,
// flt_led_0, flt_led_half, flt_led_full, flt_mod_repatched (FM3 IN = LFO 2, QM IN = OP 6 on ALGO),
// mod_matrix_algo_taps (10 rows, four fixed columns, X cells, cursor on an X cell with its hint),
// mod_matrix_modal (8 rows, four fixed columns).
#[test] fn clip_hold_holds_then_falls_in_150_ms() { /* step(1.0, 0) == 1; then step(0, 50)×3 reaches 0 at 150 ms, monotone */ }
```

- [ ] **Step 2: Run them and see them fail.** Run `CARGO_INCREMENTAL=0 cargo test -p chimera-core --features orbit --test hint_test --test screen_golden_test`; expect FAIL (missing cases and rows).
- [ ] **Step 3: Implement.** Look at every new screen with `just screens`, and check it against spec § 5 and § 7 before recording.
- [ ] **Step 4: Record.**
  - Run `GOLDEN_RECORD=1 CARGO_INCREMENTAL=0 cargo test -p chimera-core --features orbit --test screen_golden_test -- --nocapture`.
  - Paste in only the new rows and the rows this task moves (`bigviz_filter`, `flt_mode`, and the matrix cases if they move again), each with a reason comment.
  - Do the same for `screen_atlas_test`.
- [ ] **Step 5: Run them and see them pass.** Expect PASS. Mutation-check: draw the X with no cell inset (the golden should differ); take the hint off X cells (`x_cell_names…` should fail).
- [ ] **Step 6: Verify and commit.**

```bash
git add chimera-core/src/ui docs/adr/0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md chimera-core/tests/hint_test.rs chimera-core/tests/screen chimera-core/tests/screen_golden_test.rs chimera-core/tests/screen_atlas_test.rs
git commit -m "Filter screens per type, the LEVEL LED, tap rows and X cells in the matrix"
```

### Task 18: The cost bench

**Files:**
- Modify: this plan's § Measured, the `Cost` constants' doc comments (if a recount moves them), `chimera-core/tests/cost_test.rs`, `chimera-stm32/src/bench.rs` (filter rows)

- [ ] **Step 1: Recount every probe** on the final code.
  - Run `CARGO_INCREMENTAL=0 cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`.
  - Then run `python3 tools/loop_count.py target/thumbv7em-none-eabihf/release/chimera-stm32 <probe>` for each probe.
  - Expect every steady loop to show **0 `vdiv`**, and every count to be within the budget:

| Path | Budget (instr) | Per-sample `vdiv` |
|---|---|---|
| CLEAN LP4 / BP4 / HP4, steady | ≤ 56 | 0 |
| CLEAN LP1 / LP2 / BP2 / NOTCH1, steady | ≤ 30 | 0 |
| CLEAN NOTCH2, steady | ≤ 34 | 0 |
| CLEAN, moving (LP4) | +6 | 2 |
| 2040, steady | ≤ 70 | 0 |
| 2040, moving | +6 | 2 |
| DIRTY, any mode | ≤ 30 | 0 |
| LEVEL clipper + meter + fade | ≤ +12 | 0 |
| Tap capture (per tapped op) | +1 | 0 |
| Tap → FREQ | +12 + moving | 0 |
| Tap → Q | +4 + moving | 0 |
| Tap → VCA | +4 | 0 |
| Tap → FOLD | +4 + `FOLD_RAMP_COST` | 0 |

  Over budget: stop, and report the path and its count to the owner. Do not raise a budget.
- [ ] **Step 2: Bills.** Recompute each bill by the Global Constraints rule. If one differs from Task 15's, change the constant and its doc comment, and run `cost_test`.
- [ ] **Step 3: Flash.** Run `rust-size` on the default release build. Write one line in § Measured: `text` before (Task 0's base) and after, and the delta (the spec expects about 2.5 KB).
- [ ] **Step 4: Bench rows (unit).**
  - Add bench rows `SVF` (now CLEAN LP4), `CLEAN MOVE`, `2040`, `DIRTY`, `TAP FREQ` and `TAP VCA` to the bench screen (#177).
  - **Ask the owner's permission to flash.** With it: run `just flash-bench`, wait about 4 minutes, run `just usb bench`, and record each row.
  - Without it: record "held, ship time".
- [ ] **Step 5: Verify and commit.** Run `CARGO_INCREMENTAL=0 just check`.

```bash
git add docs/superpowers/plans/2026-10-05-filter-types.md chimera-core/src/dsp/filter/mod.rs chimera-core/src/taps.rs chimera-core/tests/cost_test.rs chimera-stm32/src/bench.rs
git commit -m "Filter counts against budget; bench rows for the types and taps"
```

### Task 19: Final QA and hand-off

**Files:**
- Modify: this plan's § Measured, `docs/adr/README.md` (only if the owner accepts 0096)

- [ ] **Step 1: Desktop QA** (memory: QA before UAT).
  - Run `CARGO_INCREMENTAL=0 cargo test -p chimera-core --test param_sweep_test`; expect PASS. The jack params are skipped.
  - Render each (type, mode) at RES 0, 0.5 and 1 on a held saw through `chimera-desktop`'s offline render, and tabulate RMS and peak in § Measured as the per-pair levels table.
  - Flag any pair more than 6 dB from CLEAN LP4 at RES 0.
- [ ] **Step 2: Full check.** Run `CARGO_INCREMENTAL=0 just check`; expect PASS.
- [ ] **Step 3: Issues.**
  - Comment on #57 that the filter now uses ACCURATE `tan` and that the Modal resonators' approximation remains.
  - Comment on #126 that it was redirected to the SSM2040 and is done.
  - Comment on #128 with the three types.
  - Do not close #57.
- [ ] **Step 4: PR notes** (not pushed).
  - Draft the PR body in § Measured: every re-recorded golden with its reason, the flash line, the bills, and the old-patch changes: LP1, NOTCH1 and NOTCH2 sound different; the top octave moves; DRIVE above 0.75 is up to 1.94 dB quieter; translated DRIVE gets the clipper's grit.
  - ADR 0096 stays Proposed until the owner accepts it.
- [ ] **Step 5: Commit** § Measured.

```bash
git add docs/superpowers/plans/2026-10-05-filter-types.md
git commit -m "Filter types: QA levels, PR notes"
```

---

## Self-review against the spec

| Spec requirement | Task |
|---|---|
| Starts after `modal-stagger`; divide-free in-loop saturate; probes assert 0 `vdiv` | T0, T1, T2, T18 |
| TYPE label, KIND ident and codes unchanged; CLEAN, 2040 and DIRTY modes and defaults | T4, T8, T9, T17 |
| Mode labels by type (`mode_label`), `default_mode`, MODE's default through it | T4 |
| New codes K2040 4, Dirty 6, Hp12 9; unknown loads as CLEAN LP4 | T8, T9 |
| TYPE change keeps MODE or the default; routes untouched (`ModState` byte-identical) | T4, T10 |
| Warm start and 2-block crossfade, unbilled, 256 B stack | T10 |
| Cascadia panel map, FLT and FLT › MOD, the 2040's fixed MODE, ids 9 and 10 unused | T12, T17 |
| FM3 IN and QM IN `Jack`s, choices, repatch with unit conversion, exclusions, load inference, non-ALGO fallback | T12, T13 |
| FM3 and QM as single-cell views; FREQ and Q permanent | T11, T12 |
| MIX + MINUS route delete hint; X cell hint; ADR 0086 rows | T12, T13, T17 |
| LEVEL law, id 12, DRIVE retired, translation, re-address, neutral frozen | T6 |
| Clipper curve, gate, 20 ms fade, bill, LED meter and drawing | T7, T15, T17 |
| 2040 LEVEL feeds OTAs, no clipper; LED from OTA error | T8 |
| CLEAN core: TPT SVF, `sat_c1`, `k₂`; the `k` law and self-oscillation; stability; one onset | T2, T3 |
| Per-mode trims at onset | T3 |
| ACCURATE `tan` clamped at 0.497 (#57 for the filter) | T2 |
| Steady vs moving: exact per-sample divides | T2 (CLEAN), T5 (NOTCH2), T8 (2040) |
| LP1 = `lp + bp`; NOTCH1 = `lp + ¼·hp`; NOTCH2 | T4, T5 |
| 2040 model, L, α, OUT_2040, bass loss, bounds | T8 |
| DIRTY: ×2, `f` and `q` laws, Jury clamp, hard clamps, ½ gain | T9 |
| Viz responses | T4 (enum), T17 (drawing) |
| Screen goldens list | T17 (and T11 re-records) |
| Taps: value, four destinations, tap-VCA before the blocker, never hold a voice | T14 |
| `TapRoutes` in `ParamSnapshot`, record entries validated through `ValidAddr` | T13 |
| Order each block, 512 B rows, amounts ramped, `exp2_poly`, clamps per type | T14 |
| FOLD tap gates and bills; VCA tap billed; no double move bill | T14, T15 |
| `Jack` codes 0–9 in the fixture; `ModSource` unchanged | T12, T13 |
| Four permanent columns, 36 B, migration of full registries, save as routes, `inert_dests` u32 | T11 |
| Ten rows on ALGO, row height 14, X cells refuse edits | T13, T17 |
| Default TYPE per engine, `default_for`, neutral rows fixture | T16 |
| Cost method, budgets, bills, `cost_test` names, doc comments | T1, T15, T18 |
| Memory: Voice and ModState growth under ADR 0051 | T11 (memory_budget_test), T14 (stack-check) |
| Flash line | T18 |
| Tests 1–12 | 1: T4, T8, T9 · 2: T3, T8, T9 · 3: T3–T9, T12, T14 · 4: T2, T3, T8, T9 · 5: T10 · 6: T5 · 7: T8 · 8: T3, T5, T8–T10 · 9: T13, T14 · 10: T15, T18 · 11: T4, T8–T13 · 12: T7, T17 |
| Desktop QA before the owner's ear | T19 |
| Bench rows at ship time (#177) | T18 (unit) |

### Ambiguities resolved here

1. **Cost rounding.** The spec rounds cycles up to 5, and its budget table (35, 20, 10) is in fives. The brief said 10, after ADR 0056. This plan follows the spec's 5. The `+12` applies per `vdiv` or `vsqrt` left per sample.
2. **TAPE and VA in `default_for`.** Neither engine exists yet (`EngineType` is Algo and Modal). Their arms and fixture rows land with them.
3. **"`neutral_params`"** is `Sound::neutral(e).params`. `neutral_v1.txt` is a Debug dump, so it changes shape as fields are added. Each edit only adds fields at their neutral values (or swaps `drive: 0.0` for `level: 0.5`), and the per-engine rows go in a new append-only fixture.
4. **MIDI learn** doesn't exist on this branch. FM3 IN and QM IN carry a `ParamSpec::patching()` flag, which the sweep honours now and learn will honour when it lands.
5. **Two taps on one destination.** OP5 and OP6 can both reach one column through the matrix. FREQ, Q and FOLD sum the two terms; the tap-VCA multiplies their two factors, two VCAs in series. Each entry is billed.
6. **A TYPE or MODE change during a crossfade** is deferred until the crossfade ends, at most 2 blocks, rather than stacking three filters.
7. **`TapRoutes` in memory** is `[[i8; 4]; 2]` plus a presence byte (9 B), not 8 × 4 B entries. The spec's 4-byte entries are the disk form only.
8. **`g_min`** for the FREQ tap clamp is `g_of(20 Hz)`, FREQ's floor; the spec names `g_min` without a value.
9. **The 2040's LED** uses the same gate as CLEAN's clipper (LEVEL gain > 1 or routed), and reads the OTA error. Its peak tracking is in its steady loop's ≤ 70 budget, with no `FILTER_LEVEL_COST`.
10. **`K2040::key_default()`** is 0. The spec leaves it open, and it matters only when VA lands.
11. **The FLT › MOD rename.** The page shows "MOD". The Algo map keeps "MDE" through the chain's `map` label, since "MOD" is the MOD node's.
12. **`OSC_BAND_DB`** starts at (−12, −4) dBFS, around the 2040's expected −8, and is widened only to include a type that can't reach it, with the reason recorded.
13. **The DIRTY Q-tap bill** is 10 and its move cost 0, against CLEAN's and the 2040's 7 + 35, read from the spec's totals (FREQ 55/55/20, Q 42/42/10).
14. **Every DSP test** also checks recovery from a non-finite input (Review Focus 2). The spec covers finite inputs only.

## Measured

_Filled in during execution: Task 1's `OLD_LP24` (steady and ramped, with instructions and `vdiv`s), the recomputed bills, each probe's count, the 2040's tuned α, `L` and `OUT_2040`, `TRIM_OSC`, `OSC_BAND_DB` changes, the re-recorded goldens, the flash line, the bench rows (or "held"), the per-pair level table, and the PR notes._

### Task 1: today's LP24 (2026-10-05, filter code as at 4adacaea, rustc 1.100.0-nightly e457a7b0d)

`python3 tools/loop_count.py target/thumbv7em-none-eabihf/release/chimera-stm32 old_lp24_probe`, release bench build (`opt-level = 2`, fat LTO). Static counts from `llvm-objdump --mcpu=cortex-m7`. Each loop runs 64 times a block (not unrolled), so a loop's path is one sample's executed instructions.

- **The loop has an `if` per `saturate`.** LLVM rotates each of the four `saturate`s into a skippable tail, so a sample's count depends on how many integrator states are past ±1:
  - **fast path** (all four linear): the core and four compares;
  - **slow path** (all four bending): +10 instructions each, plus one `b`.
- **Both loops have 2 `vdiv` a sample** (the SVF's `/ (1 + k·g + g²)`, one a stage; LLVM doesn't turn a divide by an invariant into a multiply without fast-math). `saturate` has none (`vminnm`, `bfi`).
- **Per-block work:** 98 instructions and 1 `vdiv` (`fast_tan`) outside the loops, both preludes counted: 1.53 instructions and 0.016 `vdiv` a sample.

| Path | Loop span | Fast path | Slow path | `vdiv` | Cycles, fast (with block) | Cycles, slow (with block) |
|---|---|---|---|---|---|---|
| Steady | 82 | 41 | 82 | 2 | 90 (95) | 160 (160) |
| Ramped | 98 | 57 | 98 | 2 | 120 (120) | 185 (185) |

- **`OLD_LP24` = 95 cycles a sample:** the steady fast path with per-block work, `ceil5((41 + 98/64) × 1.606 + 12 × (2 + 1/64))` = ceil5(92.5). The fast path is the low end, so bills against it are the conservative ones. With every state bending, today's LP24 costs 160.
- The spec's hand estimate (≈ 50 instructions + 2 `vdiv` ≈ 105) sits between the two loops' fast paths (41 and 57).

**Recomputed bills,** `max(0, est. cycles − 95)`:

| Path | Est. cycles | Spec's bill | Bill against 95 |
|---|---|---|---|
| CLEAN LP4 / BP4 / HP4, steady | 90 | 0 | 0 |
| CLEAN LP1 / LP2 / BP2 / NOTCH1, steady | 50 | 0 | 0 |
| CLEAN NOTCH2, steady | 55 | 0 | 0 |
| 2040, steady | 115 | 10 | **20** |
| DIRTY, any mode | 50 | 0 | 0 |

The moving, LEVEL and tap terms are deltas, not over `OLD_LP24`, and don't change. No steady bill turns positive where the spec says 0.

**Over `OLD_LP24 + 50` = 145 (Step 6: owner's call before Task 2):**
- 2040 moving: 70 + 6 instructions and 2 `vdiv` = ceil5(146.1) = **150**.
- 2040 with a FREQ tap: 70 + 12 + 6 instructions and 2 `vdiv` = ceil5(165.3) = **170**. With a Q tap: 70 + 4 + 6 = ceil5(152.5) = **155**.
- CLEAN stays under: moving 125, with a FREQ tap 56 + 12 + 6 = ceil5(142.8) = 145 (on the line), with a Q tap 130.
- For comparison, today's LP24 ramped is 120 on the fast path and 185 on the slow; the 2040 moving at 150 is +30 over today's ramp.

### Task 2: CLEAN LP4 (2026-10-05, same toolchain and build)

`python3 tools/loop_count.py target/thumbv7em-none-eabihf/release/chimera-stm32 'clean_probe::<2, false>'` (and `true`). `old_lp24_probe` went with `SvfFilter`; its count above stands, reproducible at 22ea0dc0.

| Path | Loop span | Fast path | Slow path | `vdiv` in loop | Block (instr, `vdiv`) | Cycles, fast | Cycles, slow |
|---|---|---|---|---|---|---|---|
| Steady | 87 | 46 | 87 | **0** | 77, 3 (`fc/fs`, `d`, `d₂`) | 80 | 145 |
| Moving | 104 | 63 | 104 | 2 | 76, 1 (`fc/fs`) | 130 | 195 |

- Steady: 46 ≤ 56, 0 `vdiv`. Bill over `OLD_LP24` (95): 0.
- Moving: 63 against the budget's 56 + 6 = 62, one over. The ramps of `g`, `k` and DRIVE, `k₂` and the two `1 + kg + g²` cost 17 instructions over steady, not 6: the move costs **+50** over the steady fast path, not the spec's 35. Task 15 bills `CLEAN_MOVE_COST` from this.
- DRIVE ramps on the steady loop (a multiply), so a DRIVE move alone never divides.
- Flash: `text` 707,984 → 708,552 (+568 B), default release build.

**Re-recorded goldens (Task 2, "ACCURATE tan, exact divides (#57)"):** every `golden_test` row (all 24: each case runs the voice's filter; the voice and instrument harnesses share the table), `filter_test::every_mode_renders_as_recorded` (all 8 modes), and `codec_compat_test::FIXTURE_RENDERS` (all 10 fixtures, which render through the same voice). `factory_level_test::KNOWN` gains a dB allowance for the six factory Sounds that matched the old norm to 1e-5: each moves 0.0003–0.09 dB.
- **STRING's `out_gain` 5.95 → 5.7** (ADR 0063's peak rule): the brighter 20 kHz took its eight-note v127 chord from 2.8 to 3.2 dB of gain reduction, over the rule's 3; 5.7 puts it back at 2.8 (C4 now 5.7 dB under the reference, was 5.4). Re-recorded again for it: `golden_test` `modal_init`, `modal_lfo_cutoff`, `algo_to_modal_switch`; `instrument_test` `two_parts_two_pairs` (both tables); `codec_compat_test` `init_modal.snd`.
- **`live_param_test::test_filter_resonance_mid_note`** read a triangle's absent 4th harmonic at 1 kHz, where only a snapped RES's step put energy; it now reads C4's 3rd and 5th either side of the cutoff and wants 2× (it gets 5×).

### Task 3: CLEAN's k law, self-oscillation and trims (2026-10-05)

- **`TRIM_OSC`** = `[1.0, 0.4756, 0.3949, 0.2374, 0.191, 0.3367, 1.0, 1.0]`: each mode's untrimmed RES 1 range over 100 Hz – 8 kHz centred on −8 dBFS.

  | Mode | Untrimmed, dBFS | Trimmed, dBFS |
  |---|---|---|
  | LP12 | −2.35 to −0.74 | −8.80 to −7.20 |
  | LP24 | −1.59 to +1.73 | −9.66 to −6.34 |
  | BP12 | +3.70 to +5.28 | −8.79 to −7.21 |
  | BP24 | +5.00 to +7.76 | −9.37 to −6.62 |
  | HP24 | +0.62 to +2.29 | −8.84 to −7.17 |

  LP6 (no `k`) and NOTCH (−21 dBFS) stay 1.0 until Task 4; PHASER (−8 to −10 dBFS, untrimmed) until Task 5. `OSC_BAND_DB` unchanged at (−12, −4).
- **Above the band:** at the 0.497 clamp (24 kHz), RES 1 rings at −22.5 to −25 dBFS (NOTCH −37), about 16 dB under the band, and recovers when FREQ comes back down (Task 3's review, F4). The band test stops at 8 kHz, the spec's range; a sweep to the top dips audibly.
- **Pinned bounds** (DRIVE 0): `CLEAN_BOUND` 13.8 (grid max 12.55, the ±10 impulses), `CLEAN_STATE_BOUND` 4.22 (3.84), `CLEAN_JUMP_BOUND` 0.81 (RES 1 jumps, 0.733 on the 0.8 saw).
- **Probes:** steady 47 instructions (Task 2: 46), 0 `vdiv`, 80 / 145 cycles; moving 67 (63), 2 `vdiv`, 135 / 200. The trim is ramped end to end across a block with `k`. **`CLEAN_MOVE_COST` = 55** (was 50). The voice-end fade (fix round 1) runs once per voice end, outside the filter's loops: no per-sample bill.
- **Flash:** `text` 708,552 → 709,240 (+688 B), default release build; fix round 1 below.
- **Re-recorded goldens ("CLEAN's k law and trims, ADR 0096"):** all 24 `golden_test` rows (RES 0's `k` moved from 2.01 to 2); `filter_test::every_mode_renders_as_recorded` (7 of 8, not LP6) and `full_resonance_rings_at_its_level`'s four levels; `instrument_test` `GOLDENS` and `PRE_LIMITER` (5 each); `codec_compat_test::FIXTURE_RENDERS` (all 10); `factory_level_test::KNOWN` factory 4 0.001 → 0.07 dB and SQR BASS 0.56 → 0.74 (the law: +0.70 dB at its cutoff). STRING's 8-note v127 chord still takes 2.8 dB of gain reduction (ADR 0063's cap 3).
- **Voice end (fix round 1):** a voice whose engine ends while its filter still rings (a state above −80 dB) fades that block to 0 instead of cutting; the next note starts the filter from rest (`Clean::reset`). Re-recorded for the fade: `golden_test` `factory_5` and `codec_compat_test` `factory_5.snd` (SQR BASS ends on a −65 dB filter tail). The ring after note-off itself is #374.

### Task 4: CLEAN's modes, LP1 and NOTCH1 (2026-10-05)

- **Knob order** `[LP1, LP2, LP4, BP2, BP4, HP4, NOTCH1, NOTCH2]`, default LP4 (`default_mode()`), MODE's static default 2.0. Disk codes and idents unchanged, so stored Sounds load the same mode.
- **`TRIM_OSC`** gains LP1 0.3277 (untrimmed +0.89 to +2.49 dBFS → −8.80 to −7.20) and NOTCH1 0.6324 (−4.82 to −3.22 → −8.80 to −7.20). PHASER stays 1.0 until Task 5.
- **Responses at RES 0** (250 Hz, 1 kHz and 4 kHz alike, on the prototype's axis, `tan(π f/fs) = w·g`): slopes over 4..8 × FREQ LP1 −5.82, LP2 −11.65, LP4 −22.78, HP4 +22.78 dB/oct. Over 2..4 × LP4 reads −20.15 (the second stage, `k₂` 2.7, short of its asymptote), outside the brief's −24 ± 3, so the test reads 4..8 ×. NOTCH1's zero lands on `tan(π f/fs) = 2g` (499.9, 1,991.5 and 7,516.5 Hz), −150 dB deep.
- **Pinned bounds:** `CLEAN_JUMP_BOUND` 0.81 → 0.92 (RES 1 jumps now include LP1 and NOTCH1: max 0.833). `CLEAN_BOUND` and `CLEAN_STATE_BOUND` unchanged (grid max 12.55, states 3.84).
- **Probes** (LP4) unchanged: steady 47, moving 67. **`CLEAN_MOVE_COST`** stays 55.
- **Flash:** `text` 709,928 → 710,096 (+168 B), default release build.
- **Re-recorded ("LP1 and NOTCH1, ADR 0096"):** `filter_test::every_mode_renders_as_recorded` LP6 and NOTCH rows; screen goldens `bigviz_filter` and `flt_mode` (the MODE label only: LP4, HP4). No `golden_test` patch is on LP6 or NOTCH: none moved.
- **Voice end (Task 3's F5):** the end fade now also runs inside a steal, kill or switch fade, which stops with the engine half way down.

### Task 5: NOTCH2 (2026-10-05)

- **DSP:** `u = x + fb·w[n−1]`, four TPT all-passes at `G₁ = g/(1 + g)`, `out = √(1 − fb)·½·(u + a)`, `fb = 0.7·RES` (read from `k`, so it ramps with it). `√(1 − fb)·½` is NOTCH2's `trim`, set at each block end and ramped, so it is computed once a block end (`libm::sqrtf`, software: no `vsqrt` in `core`). Each all-pass is four operations: `e = v − s; s += 2G₁·e; v = s − e`.
- **Notches** (2^15-point Welch H1 on white noise, FREQ 200–1600 Hz, Q 0, 0.5, 1): exactly two, at `(fs/π)·atan(g·tan(π/8))` and `(fs/π)·atan(g·tan(3π/8))` within 0.2 % (e.g. 200 Hz: 82.73 and 482.73 Hz, want 82.85 and 482.71), the same at every Q.
- **Peaks** rise with Q by at most 5.218 dB (400 Hz), 4.40 dB at 8 kHz. The law's ceiling is `10·log10(1/0.3)` = 5.229 dB, over the brief's 5.2, so the bound is pinned at 5.23.
- **Loudness** (pink, 20 Hz – 20 kHz, re Q 0) at Q 1: 200 Hz −0.31, 400 Hz −0.57, 800 Hz +0.08, 1.6 kHz +1.14, 2 kHz +1.47 dB. White noise weights the top octave as all the others together, where the one-sample delay, not the notches, sets the level: there Q 1 reads −1.3 to −1.6 dB.
- **Peak at 8 kHz, Q 1:** 0.834 × FREQ (6,671 Hz), pinned ±2 %.
- **No self-oscillation:** at RES 1 an impulse is down at least 90.4 dB after 0.5 s over the bound grid's 24 FREQs.
- **Probe** `clean_probe::<7, false>` (steady NOTCH2): the loop is unrolled 4 ×, 96 instructions for 4 samples: **24 a sample**, 0 `vdiv`, 40 cycles (budget ≤ 34). Block: 77 instructions, 2 `vdiv` (`fc/fs`, `G₁`) and one `sqrtf` call. Moving NOTCH2 (counted, not linked): 74 instructions for 2 samples, 1 `vdiv` a sample, 75 cycles: +35 over steady, under `CLEAN_MOVE_COST` 55. LP4's probes unchanged: steady 47, moving 67.
- **Pinned bounds** ("NOTCH2, ADR 0096"): `CLEAN_BOUND` 13.8 → 11.43 (grid max 12.55, the old PHASER, → 10.39, LP1); `CLEAN_STATE_BOUND` 4.22 → 3.85 (3.84 → 3.50); new `NOTCH2_STATE_BOUND` 5.54 (5.03 × 1.1). NOTCH2 joins the exact-reference jump tests, and at RES 1 is held to the reference instead of `CLEAN_JUMP_BOUND` (unchanged, 0.92).
- **Re-recorded ("NOTCH2, ADR 0096"):** `filter_test::every_mode_renders_as_recorded` PHASER row. No `golden_test` patch is on PHASER: none moved.
- **Flash:** `text` 710,096 → 710,304 (+208 B), default release build.
- **Stack** (stack-check on `target/t5`): bench,orbit 60,672 B of 63,232, unchanged.

### Task 7: LEVEL's clipper, gate fade and LED (2026-10-06)

- **The fade is `ease::Gate`** (fix round 1), the drive's and fold's 20 ms law: a fresh `LevelStage` lands where its gate is, and `Clean::hold` resets it.
- **Where it runs.** LEVEL's gain left CLEAN's loops for its own pass before them (`LevelStage::run`), so the 2040 can take the gain into its stages without the clipper (`LevelRamp` carries the gain and its gate; only CLEAN and DIRTY own a `LevelStage`). Three loops: the gain alone (gate shut, LEVEL off ½; skipped at exactly ½), the clipper faded in, and the clipper fading. The clipper is `sat_c1` branch-free (`c + u − ½·u·|u|`, four `vmaxnm`/`vminnm`); the ramps step by adding.
- **Probes** (`tools/loop_count.py`, which now reads an unrolled loop's stride, bench ELF in `target/t7`), a sample:

  | Loop | Unrolled | Instructions a sample | `vdiv` |
  |---|---|---|---|
  | LP4 steady filter (no pre-gain now) | 1 × | 41 fast / 82 slow (was 47 / 88) | 0 |
  | LP4 moving filter | 1 × | 64 / 105 (was 67 / 108) | 2 |
  | NOTCH2 steady filter | 4 × | 18.0 (was 24) | 0 |
  | NOTCH2 moving filter (`clean_probe::<7, true>`, now linked) | 2 × | 36.0 | 1 |
  | LEVEL gain alone | 16 × | 4.3 | 0 |
  | LEVEL clipper, faded in (+ peak) | 8 × | 16.6 | 0 |
  | LEVEL clipper, fading (+ peak, 20 ms) | 4 × | 20.0 | 0 |

  Steady LP4 with the clipper held in: 41 + 16.6 = 57.6, **+10.6** over Task 3's 47 (budget +12), ceil5((57.6 + 3.19) × 1.606 + 12 × 0.047) = 95 cycles against 80: +15, under `FILTER_LEVEL_COST` 20. While the gate's 20 ms fade runs: 61.0, +14.0 (2 over), 105 cycles. At LEVEL ½, gate shut: 41, cheaper than before.
- **The LED.** `clip_amount(peak) = clamp((peak − 1)/2, 0, 1)` per block, a `meter::Clip`. The engine's full-scale saw (0.94) through a voice reads 0.11, 0.31, 0.57, 0.91 and 1 at LEVEL 0.6–1.0. **Fix round 1:** `FILTER_CLIP` is a `ClipMeter`: audio raises it each block (`fetch_max` on the bits, which `Clip` keeps non-negative so they order as values), and the UI takes it once a frame (`swap(0)`), so no one-block clip between frames is missed. Task 17's `ClipHold` reads `take()`. `MASTER_GR` is a `DbMeter`: the dB and the clip have their own types. The 2040's LED raises the same meter from its own stage clip (task-8-amendment).
- **The gate in a voice** shuts only when `StageEase` lands LEVEL on ½ (within 1e-5): measured with the real `Ease`, up to 163 blocks after it is set back from LEVEL 1 (152 from 0.75, 104 from 0.51), then 15 more of fade. Until then the clipper runs, transparent. A bill on "stored LEVEL > ½ or routed" misses that tail, so Task 15 bills `FILTER_LEVEL_COST` from `Voice::filter_level_running()` (`LevelStage::running`: the gate open or the fade not yet out), the spec's "while it runs". (Fix round 1.)
- **The bill is the held figure.** The fading loop is +14.0 (2 over the +12 budget, about +5 cycles over the 20 bill) for 20 ms after each gate change; `FILTER_LEVEL_COST` stays 20 from the held clipper's +10.6. (Fix round 1.)
- **NOTCH2 (Task 5's review):** `notch2_fb` is held to ±0.7, so RES 1.5 or −3 decay like RES 1 (−90.4 dB after 0.5 s at worst); `rings()` holds NOTCH2's tail.
- **Re-recorded ("LEVEL clips past unity, ADR 0096"):** `filter_test::every_mode_renders_as_recorded`, all 8 rows: LEVEL ×3 drives its ±1 saw to ±3 into the clipper. No `golden_test`, `instrument_test` or `codec_compat_test` row moved: every factory and fixture Sound is at LEVEL ½.
- **Flash:** `text` 710,368 → 710,960 (+592 B), default release build.
- **Fix round 1 flash:** `text` 710,960 → 711,368 (+408 B). Probes unchanged (gain 4.31, held 16.62, fading 20.00, LP4 41 / 82).
- **Stack** (stack-check on `target/t7`): default 48,240, no default 47,196, bench 57,472, bench,master-tape 57,488, orbit 49,032, bench,orbit 60,704 B, +24 to +32 B each.
