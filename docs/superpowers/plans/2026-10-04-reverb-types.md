# Reverb TYPEs Implementation Plan

**Goal:** Give the master reverb a TYPE: today's QUAD, plus ROOM, HALL (with PLATE as a zone of it), BLOOM, GATE and REVERSE. They share one i16 arena, switch without a click, and never re-pitch a tail. GRIT is redesigned as the reverb's converters, QUAD included.

**Architecture:**
- The three new engines (LOOP, BURST, BLOOM) live in `chimera-core/src/dsp/rev/`, beside today's `dsp::ring` (QUAD). A dispatcher, `dsp::rev::Reverb`, runs one engine at a time over a shared `ReverbArena`, and replaces `RingReverb` in `FxBus`.
- Lengths move by pitch-free crossfades between fixed reads. Only the wobble's reads are interpolated.
- The level plan bounds every stored value with a soft clip at each loop's re-entry, normalised in-loop allpass states, and f32 diffusers and decorrelators.
- GRIT is the ADC and DAC converters (high cut, TPDF dither, bit depth), outside the engines.
- One parameter block of semantic slots, with a per-TYPE `const` panel.

**Tech Stack:** Rust 2024 on stable; `no_std` `chimera-core`; `chimera-stm32` (bench build, `--features bench`); host tests in `chimera-core/tests`; `tools/chimera-usb.py` for the unit.

**Spec:** `docs/superpowers/specs/2026-10-04-reverb-types-design.md`. ADR: `docs/adr/0085-the-master-reverb-picks-a-type.md`.

**Base:** `reverb-types`, rebased onto `orbit-qa2` (e2891b0d). On that base:
- ADR 0086's `ui::hint`, ADR 0089's fat LTO, ORBIT's clock and runner, and `cost_test`'s numbers are present.
- `tests/common/orbit.rs` has `write_wav`.
- `bench.rs` has `algo_pair` but no `a16_a17`.

## Global Constraints

- **Safety and the audio thread.** No `unsafe` without `// SAFETY:`. No heap and no libc in `chimera-core`. The audio thread never blocks or allocates. Stable Rust only: no `#![feature]`.
- **No snapping.** Parameter changes are lerped, never snapped:
  - every continuous knob eases;
  - lengths move only by crossfade, in steps of at most 5 ms;
  - TYPE moves only through the switch.
- **The arena** is `[i16; 23_220]` (46,440 B, `RING_LEN`) and never grows. Each TYPE's layout is a `const` asserted to fit.
- **Ids and codes.**
  - Reverb ParamId 0 stays retired. The new ids are 6–13.
  - `ReverbType` codes: QUAD 0, ROOM 1, HALL 2, GATE 3, REVERSE 4, BLOOM 5. Code 6 is never assigned.
  - `RevSync` codes: OFF 0, TIME 1, ALL 2.
  - All are frozen in `chimera-core/tests/fixtures/disk_codes_v1.txt` (ADR 0045).
- **Provenance.** No code, instruction listing, or ROM coefficient, offset or tap table may come from github.com/thement/midiverb_emulator or commit 7119f7ad, whole or in part. Every engine task writes a provenance report: each table, its spec source, and each ear change.
- **CPU.**
  - The target is a TYPE's REV row against REV QUAD on the same bench (Task 1: QUAD reads 66 as a delta over the MIX-0 default; ADR 0031's absolute 485 is the old frame).
  - The per-TYPE gate is ΔCOST ≤ headroom: **76** in the default build (BOWED 8 × 717 on rev V), **23** with `master-tape` (Task 1, § Measured).
  - The final gate is `cost_test` passing with `FxBus::COST` at the costliest TYPE's BUS+V reading.
  - The STRESS row is a second gate.
- **Flash.** Measure the plain release build's `text`: `rust-size target/thumbv7em-none-eabihf/release/chimera-stm32`, column `text` (ADR 0089, which replaces the spec's git-archive method). A TYPE may add at most 3 KB.
- **Hints** follow ADR 0086. The reverb adds no gesture, so `View::hint` is `hint::NONE` for every reverb cell.
- **Names.** User-facing names are sound names, and QUAD keeps `QUAD`.
- **Commits.** Terse, with no type prefix. Stage by path with `git add <paths>`; never `git add -A`.
- **The unit.** **(unit)** steps flash the bench build with `just flash-bench`, wait about 4 minutes, and read `just usb bench`. They are hands-free. Each reading goes into § Measured: row, value, build (default or tape), commit, date.

## Review Focus

1. **Scrolling TYPE through all six mid-tail at MIX 1** ends on the last TYPE with no click and no burst. Test: Task 7, `scrolling_types_mid_tail_lands_on_the_last_without_a_click`.
2. **A project with an unknown TYPE code or NaN slots** loads as QUAD and renders finite. Test: Task 6, `unknown_type_code_and_nan_slots_load_as_safe_quad`.
3. **A switch at MIX 0, then MIX up,** plays the new TYPE from the current send, with no stale tail. Test: Task 7, `switch_at_mix_0_then_mix_up_has_no_stale_tail`.
4. **GRIT moved across the converters' idle mute** gives no step in the return. Test: Task 5, `dither_mute_fades_without_a_step`.
5. **The MIDI clock dropping out while synced** moves a synced length once, with no fold flapping and no click. Test: Task 12, `clock_dropout_moves_synced_length_once`.

---

## File map

| File | Responsibility | Created in |
|---|---|---|
| `chimera-core/src/dsp/rev/mod.rs` | `Reverb`, `EngineState`, `SwitchPhase`, the half-bands, the MIX ease | T2 (stub), T4 |
| `chimera-core/src/dsp/rev/arena.rs` | `ReverbArena`, f32 regions, `clear` | T2 |
| `chimera-core/src/dsp/rev/parts.rs` | soft clip, allpasses, pair read, `Xfade`, `Wobble`, `Ease1`, `one_pole` | T2 |
| `chimera-core/src/dsp/rev/sync.rs` | `Tempo`, then `Division`, the lists, `fold`, `FoldState`, then tempo quantising | T4, T6, T12 |
| `chimera-core/src/dsp/rev/grit.rs` | `Converters` | T5 |
| `chimera-core/src/dsp/rev/burst.rs` | `BurstEngine` | T8 |
| `chimera-core/src/dsp/rev/loop_engine.rs` | `LoopEngine<LOOPS, STAGES>` | T9 |
| `chimera-core/src/dsp/rev/bloom.rs` | `BloomEngine` | T11 |
| `chimera-core/src/dsp/reverb.rs` | `ReverbParams`, `ReverbType`, `RevSync`, specs, `with_type`, `with_sync`, `KnobMemory`, `EngineKnobs`, `knobs` | T6 |
| `chimera-core/src/ui/reverb_panel.rs` | panels, `label`, `value_text`, steps | T6 |
| `chimera-core/tests/common/rev.rs` | the shared reverb test helpers | T2 |
| `chimera-stm32/src/bench_rev.rs` | throwaway kernels | T3; deleted in T8–T10 |
| `tools/ab/stimuli.py`, `tools/ab/render.sh` | A/B stimuli and renders | T5 |

---

### Task 1: Baselines and the REVERB bench screen (unit)

**Files:**
- Modify: `chimera-stm32/src/bench.rs`, `docs/superpowers/plans/2026-10-04-reverb-types.md` (§ Measured)

**Interfaces:**
- **Produces:**
  - `fn a16_a17() -> ParamSnapshot`, which returns `algo_pair(AlgoId::A16, AlgoId::A17)`;
  - `fn time_bus_with_voices(&mut self, each: fn(&mut AudioShared, u32)) -> u32`;
  - `const REV: [FxRow; REV_ROWS]`, the **REVERB screen's** rows, starting as `("REV QUAD", worst_reverb)`. Later tasks append to it.
  - `fn stress(&mut self) -> (u32, u32)`, the maximum block against the budget;
  - recorded figures: QUAD_REV, QUAD_BUS, QUAD_BUS_V and STRESS.

- [ ] **Step 1: Add the REVERB screen.**
  - It is a bench screen of its own after the FX screen, three rows to a line from y 46 in the FX screen's layout, ≤ 5 lines.
  - Its rows are `REV QUAD`, `BUS+V`, then `STRESS max/budget`.
  - **BUS+V** is `time_bus_with_voices`: `time_bus` with six `a16_a17()` voices rendered by `Instrument::render` each block, so the voices' cache traffic sits between the FX blocks.
  - **STRESS** is the maximum whole-callback block in cycles over 7,500 blocks, against `BlockBudget::for_cpu(rev).budget_cycles()`. It runs six `a16_a17()` voices, every `worst_*` FX, and an `OrbitRunner` playing NEW's groove (ADR 0073).
  - The FX screen keeps its REVERB row, labelled `REV QUAD`.
- [ ] **Step 2: Build both bench builds.**
  - Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`, then the same command with `--features bench,master-tape`.
  - Expected: both build with no warnings.
- [ ] **Step 3: Measure (unit).** Run `just flash-bench`, wait for "bench running", wait about 4 minutes, then run `just usb bench`. Repeat on the tape build, flashing it with `flash-bench`'s dfu line.
  - Expected: STRESS max < budget, and `just stats` shows OVER 0.
  - Record every figure.
- [ ] **Step 4: Headroom.**
  - Record the minimum over `cost_test`'s voice-count assertions: expected **96** (FOLD at 954 × 6 on rev V) and **23** (tape: MORPH KEYS at 864 × 5 on rev Y).
  - If BUS+V reads above `FxBus::COST`:
    - commit `COST` = that reading first, with a comment in ADR 0031's style;
    - if a `cost_test` assertion then fails, stop and hand it to the owner before Task 3.
- [ ] **Step 5: Commit.**

```bash
git add chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "Bench: REVERB screen with REV QUAD, BUS+V and STRESS; baselines"
```

### Task 2: Reverb parts and shared test helpers (host)

**Files:**
- Create:
  - `chimera-core/src/dsp/rev/mod.rs` (`pub mod arena; pub mod parts;` only);
  - `chimera-core/src/dsp/rev/parts.rs`;
  - `chimera-core/src/dsp/rev/arena.rs`;
  - `chimera-core/tests/common/rev.rs`;
  - `chimera-core/tests/reverb_parts_test.rs`.
- Modify: `chimera-core/src/dsp/mod.rs` (add `pub mod rev;`), `chimera-core/tests/common/mod.rs` (add `pub mod rev;`)

**Interfaces:**
- **Produces, in `parts.rs`:**
  - `pub fn soft_clip(x: f32) -> f32`;
  - `pub fn ap_norm(x: f32, w_d: f32, g: f32, inv_1mg: f32) -> (f32 /*y*/, f32 /*w*/)` and `pub fn ap_f32(x: f32, z: f32, g: f32) -> (f32 /*y*/, f32 /*v*/)`;
  - `pub fn pair(lines: &[i16], base: usize, d: f32) -> f32`, which requires `d ≥ 2.0` and wraps;
  - `pub struct Xfade { pub from: u16, pub to: u16, left: u16 }`, with `pub const XFADE: u16 = 720`, `start(&mut self, to: u16)`, `weight(&self) -> f32`, `tick(&mut self)` and `busy(&self) -> bool`;
  - `pub struct Wobble { phase: u32, inc: u32 }`, with `set_rate(&mut self, hz: f32, fs: f32)` and `block(&mut self) -> [f32; 2]` (L and R, 90° apart, in −1..1);
  - `pub struct Ease1 { v: f32, primed: bool }`, with `step(&mut self, target: f32, k: f32) -> (f32, f32)`, the per-block one-pole every continuous knob uses, and `pub fn ease_k(tau_s: f32) -> f32` (64-sample blocks at 48 kHz);
  - `pub fn one_pole(fc: f32, fs: f32) -> f32`.
- **Produces, in `arena.rs`:** `#[repr(C, align(4))] pub struct ReverbArena { lines: [i16; RING_LEN] }`, with `lines()`, `lines_mut()`, `f32_region(&mut self, offset: usize, len: usize) -> &mut [f32]` and `clear(&mut self)`.
- **Produces, in `tests/common/rev.rs`** (every engine task uses these; none re-implements them):
  - **Render plumbing:**
    - `pub type Render<'a> = &'a mut dyn FnMut(&[f32; BLOCK_SIZE]) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]);`
    - `pub fn run(r: Render, input: &mut dyn FnMut(usize) -> f32, seconds: f32) -> (Vec<f32>, Vec<f32>)`.
  - **Measures:** `rt60_t20`, `onset_ms` (1 ms RMS > −40 dB relative to the peak), `peak_ms`, `end_20db_ms`, `corr`, `rms_db`, `max_d2`, `thd`, `alias_db`, `settled_peak_hz`, `autocorr_peak_db(x, lags_ms)`, `pitch_per_pass_cents(x, f0, loop_s)`.
  - **Shared checks:**
    - `check_settles_on_pitch(r, step: &mut dyn FnMut(usize), loop_s: f32)`: the settled peak is within 1/L Hz of 1 kHz, and the in-crossfade deviation is printed, not asserted.
    - `check_no_slap(moved: &[f32], fixed: &[f32])`: lags of 1–10 ms are ≤ −20 dB and ≤ `fixed` + 3 dB; the 10 ms energy dips ≤ 3 dB.
    - `check_wobble_bound(r, loop_s)`: ±25 cents on every pass, and an LFO-cycle mean of ±1 cent.
    - `check_level_plan(r, set_max: &mut dyn FnMut(), r_unclipped: Render, r_f32: Render, loop_mode_hz: f32)`:
      - a held 0 dBFS loop-mode sine gives THD ≤ 3 % and 0 clamp hits;
      - a 5 kHz loop mode gives alias ≤ −40 dB;
      - −12 dBFS pink at the default DECAY is bit-identical to `r_unclipped`;
      - at maximum DECAY it is ≤ −40 dB from `r_f32`;
      - a swept sine gives clamp hits ≤ 0.1 % of writes.
    - `check_noise_and_loudness(r: Render, quad: Render, extra_db: f32)`: the arena noise is ≤ QUAD's + `extra_db`, and the default RMS is within ±1 dB of QUAD's.
    - `check_knobs_ease(r, ids: &[ParamId], set: &mut dyn FnMut(ParamId, f32))`: each continuous knob stepped 0 → 1 mid-tail keeps `max_d2` within `click_free_test`'s bound.

- [ ] **Step 1: Write the failing tests** in `reverb_parts_test.rs`:

```rust
#[test] fn soft_clip_is_identity_to_the_knee_and_reaches_two_flat() {
    for x in [-1.0f32, -0.5, 0.0, 0.3, 1.0] { assert_eq!(soft_clip(x), x); }
    assert!((soft_clip(3.0) - 2.0).abs() < 1e-6);
    assert_eq!((soft_clip(10.0), soft_clip(-10.0)), (2.0, -2.0));
    assert!(((soft_clip(3.0) - soft_clip(2.999)) / 0.001).abs() < 1e-2);
    assert!((1..400).all(|i| { let x = i as f32 * 0.01; soft_clip(x) >= soft_clip(x - 0.01) }));
}
#[test] fn ap_norm_matches_the_plain_allpass_and_bounds_its_state() {
    // 10,000 samples of noise in ±1, g = 0.75, M = 37: y equals the plain allpass (v = x − g·z, y = z + g·v) within 1e-6;
    // a sine at the AP's worst frequency (z^-M = −1), amplitude 1: max |w| ≤ 1.0 + 1e-6 while max |v| ≈ 4.0 (> 3.9).
}
#[test] fn pair_reads_fixed_at_fraction_0_and_the_mean_at_half_across_the_wrap() {
    // lines = 0, 100, 200, …; pair(.., base 3, d 2.0) == lines[1]; d 2.5 == (lines[1] + lines[0]) / 2;
    // base 0, d 2.5 wraps to (lines[N−2] + lines[N−3]) / 2.
}
#[test] fn xfade_blends_linearly_over_720_and_lands() {
    // start(10): weight() after k ticks == k / 720 within 1e-6; busy() true until tick 720; then weight() == 1.0 and from == to == 10.
}
#[test] fn wobble_is_quadrature_and_its_block_lerp_errs_under_1e4_at_2_hz() {
    // set_rate(2.0, 24_000.0): over 1 s, max |L|, |R| within 1 ± 1e-3; L leads R by 90° ± 1°;
    // the linear lerp between block ends differs from the exact sine by < 1e-4.
}
#[test] fn ease1_lands_and_never_snaps() {
    // a step from 0 to 1 with ease_k(0.02): from == 0 on the first block; within 1e-5 after 0.2 s; per-block change ≤ 1 − e^(−64/(0.02·48000)) + 1e-6.
}
#[test] fn f32_region_round_trips_and_clear_zeroes_both_views() {
    // f32_region(100, 50)[i] = i as f32 + 0.5 reads back exactly; clear(); every i16 == 0, every f32 == 0.0;
    // f32_region(101, 1) panics (odd offset).
}
#[test] fn helpers_measure_known_signals() {
    // rt60_t20 of exp(−6.91 t / 1.5) noise == 1.5 s ± 5 %; thd of a sine clipped at 0.9 > 1 % and of a clean sine < 0.01 %;
    // settled_peak_hz of 1 kHz == 1000 ± 1; corr(x, x) == 1; corr(x, −x) == −1.
}
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_parts_test`.
  - Expected: FAIL (unresolved imports).
- [ ] **Step 3: Implement.**
  - `soft_clip`, above the knee: `copysign(1 + u − u²/4, x)`, where `u = min(|x| − 1, 2)`; branch-free.
  - `f32_region` uses `from_raw_parts_mut` with a `// SAFETY:` comment covering:
    - alignment (`align(4)`, plus an asserted even offset);
    - bounds (`offset + 2·len ≤ RING_LEN`, asserted);
    - no live i16 view (the method takes `&mut self`).
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core --test reverb_parts_test`.
  - Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/mod.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/dsp/rev/parts.rs chimera-core/src/dsp/rev/arena.rs chimera-core/tests/common/mod.rs chimera-core/tests/common/rev.rs chimera-core/tests/reverb_parts_test.rs
git commit -m "Reverb parts and shared test helpers"
```

### Task 3: Throwaway worst-case kernels on the chip (unit)

**Files:**
- Create: `chimera-stm32/src/bench_rev.rs` (`#[cfg(feature = "bench")]`)
- Modify: `chimera-stm32/src/main.rs` (`#[cfg(feature = "bench")] mod bench_rev;`), `chimera-stm32/src/bench.rs` (rows `K PAIR`, `K BURST`, `K ROOM`, `K HALL` on the REVERB screen), plan § Measured

**Interfaces:**
- **Consumes:** Task 2's parts and `ReverbArena`.
- **Produces:**
  - `pub struct KBurst`, `KRoom`, `KHall` and `KPair`, each with `fn run(&mut self, arena: &mut ReverbArena, u: &[f32; HALF], l: &mut [f32; HALF], r: &mut [f32; HALF])`.
  - The kernels are fixed worst-case constants: no `EngineKnobs`, no params. They are throwaway: Tasks 8–10 delete them.

- [ ] **Step 1: Write the kernels** to the spec's § CPU budget worst cases, using the spec's lengths:
  - **KBurst:**
    - 12 taps: tap 0 from the register, tap 11 shared with REGEN, 4 taps always crossfading;
    - 2 wobbled f32 decorrelator APs per side;
    - a TAIL stage with a normalised AP;
    - 2 soft clips and a truncating line write;
    - converter stubs: 3 quantisations, 3 one-poles and 2 xorshift steps.
  - **KRoom:**
    - PRE-DLY and 4 stage delays always crossfading;
    - 2 wobbled and 2 fixed normalised stage APs;
    - a 4-AP f32 diffuser, the rotation, 2 soft clips and 6 fixed taps;
    - the converter stubs.
  - **KHall:** as KRoom, but 1 loop × 4 stages with 1 soft clip.
  - **KPair:** 16 moving pairs against 32 fixed reads, so (row − 32 fixed) / 16 is a pair's marginal cost.
- [ ] **Step 2: Build check.**
  - Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`.
  - Expected: it builds.
- [ ] **Step 3: Measure (unit).**
  - Run `just flash-bench`, then `just usb bench`, on both builds.
  - Record each kernel's ΔCOST = row − QUAD_BUS_V, and the pair cost (+7 or +10).
- [ ] **Step 4: Gate.**
  - Every kernel must have ΔCOST ≤ 96, and ≤ 23 in the tape build.
  - **K ROOM over 23 (tape).** Apply ROOM-T and re-measure, one stage at a time:
    1. Wobble loop B's AP only.
    2. If still over: 2 output taps a side; drop (A2, 0.83) on the left and (B2, 0.79) on the right.

    Record the stage taken. Task 9 builds ROOM with it.
  - **K BURST over its headroom:** 3 crossfading taps.
  - **K HALL over its headroom:** one wobbled AP (stage 3).
- [ ] **Step 5: Commit.**

```bash
git add chimera-stm32/src/bench_rev.rs chimera-stm32/src/main.rs chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "Bench: throwaway reverb kernels and the moving-pair cost"
```

### Task 4: The arena moves out of the ring; the dispatcher (QUAD bit-identical) (unit check)

**Files:**
- Create: `chimera-core/src/dsp/rev/sync.rs` (`Tempo` only), `chimera-core/tests/reverb_dispatch_test.rs`
- Modify: `chimera-core/src/dsp/ring.rs`, `chimera-core/src/dsp/rev/mod.rs`, `chimera-core/src/dsp/fx_bus.rs`, `chimera-core/src/instrument.rs` (`mix_parts` passes `Tempo::DEFAULT`), `chimera-core/tests/in_place_test.rs` and `chimera-core/tests/memory_budget_test.rs` (the new type names)

**Interfaces:**
- **Consumes:** `ReverbArena`.
- **Produces:**
  - `Ring::process(&mut self, arena: &mut ReverbArena, u, blk, l, r)`. `Ring` no longer has `lines`.
  - `pub struct Reverb`, with:
    - `fn process(&mut self, send: &[f32; BLOCK_SIZE], p: &ReverbParams, mix: f32, tempo: Tempo, sample_rate: u32, out: &mut Stereo)`;
    - `fn arena(&self) -> &ReverbArena`;
    - `fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self`.
  - `#[repr(C, u8)] pub enum EngineState { Quad(QuadState) }`.
  - `pub struct Tempo(pub f32)` (samples per beat), with `pub const DEFAULT: Tempo = Tempo(24_000.0)` (120 BPM at 48 kHz).
  - `FxBus::process(&mut self, sends, params, tempo: Tempo, sample_rate, ret)`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn zeroed_engine_state_is_quad_default() {
    let z: EngineState = unsafe { core::mem::MaybeUninit::zeroed().assume_init() }; // SAFETY: the test checks exactly this claim
    assert_eq!(z, EngineState::Quad(QuadState::default()));
}
#[test] fn engine_state_fits() { assert!(core::mem::size_of::<EngineState>() <= 160); }
#[test] fn tempo_is_threaded_but_inert_for_quad() {
    // Reverb::process with Tempo(12_000.0) and Tempo(48_000.0): QUAD's outputs are bit-identical.
}
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_dispatch_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement the move.**
  - `RingReverb`'s body becomes `Reverb`'s QUAD path, with its arithmetic unchanged.
  - `FxBus` holds `Reverb`, so update its `field_list!` and `init_in_place`.
  - The zeroing in `init_in_place` carries a `// SAFETY:` comment: variant 0 is `Quad`, and every field of every variant is an integer or a float.
- [ ] **Step 4: Check the refactor lock.**
  - Run: `cargo test -p chimera-core --test reverb_test --test fx_golden_test --test gain_staging_test --test fx_bus_test --test in_place_test --test memory_budget_test --test reverb_dispatch_test`.
  - Expected: PASS, with no golden changed.
  - Record the commit hash as the **T4 golden reference** in § Measured.
- [ ] **Step 5: Bench (unit).**
  - Run `just flash-bench`, then `just usb bench`.
  - Gate: REV QUAD ≤ QUAD_REV + 1 %.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/ring.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/dsp/rev/sync.rs chimera-core/src/dsp/fx_bus.rs chimera-core/src/instrument.rs chimera-core/tests/reverb_dispatch_test.rs chimera-core/tests/in_place_test.rs chimera-core/tests/memory_budget_test.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "Reverb arena out of the ring; Reverb dispatches QUAD"
```

### Task 5: GRIT becomes the converters (unit check)

**Files:**
- Create:
  - `chimera-core/src/dsp/rev/grit.rs`;
  - `chimera-core/tests/reverb_grit_test.rs`;
  - `chimera-core/tests/reverb_render_test.rs` (the A/B renderer, using `common::orbit::write_wav`);
  - `tools/ab/stimuli.py`;
  - `tools/ab/render.sh`.
- Modify: `chimera-core/src/dsp/ring.rs`, `chimera-core/src/dsp/rev/mod.rs`, `chimera-core/tests/reverb_test.rs`, `chimera-core/tests/fx_golden_test.rs`

**Interfaces:**
- **Produces:**
  - `pub struct Converters`, with:
    - `fn adc(&mut self, x: &mut [f32; HALF], grit: f32)`;
    - `fn dac(&mut self, l: &mut [f32; HALF], r: &mut [f32; HALF], grit: f32)`.
  - `pub fn bits(grit: f32) -> f32`, which is `16.0 − 6.0·grit`.
  - `pub fn cutoff_hz(grit: f32) -> f32`, which is `11_000.0 · exp2(grit · log2(8/11))`.
  - `pub const IDLE_HOLD: u32 = 2_400` (100 ms at 24 kHz) and `pub const IDLE_FADE: u32 = 1_200` (50 ms).
- **Where the converters run in `Reverb`:**
  - `adc` on the decimated send, in dBFS units, before the ½ injection.
  - `dac` on the return, after `WET` and before MIX.
  - Both are skipped when the eased GRIT is exactly 0.
- **The ring's storage:** `Grid` becomes a fixed Q3 quantiser at Δ = 1. Its `new(grit)` is removed, and `Grid::FIXED` replaces it.

- [ ] **Step 1: Write the failing tests** in `reverb_grit_test.rs`:

```rust
#[test] fn grit_0_bypasses_bit_for_bit() {
    // fx_golden_test's reverb cases at grit 0.0 hash exactly as at the T4 golden reference.
}
#[test] fn adc_lands_on_its_step() {
    for (g, b) in [(0.3f32, 14.2f32), (0.6, 12.4), (1.0, 10.0)] {
        // dither forced off: a ramp −1..1 through adc; every output y satisfies |y / 2^(1−b) − round(y / 2^(1−b))| < 1e-6
    }
}
#[test] fn cutoff_tracks_grit() {
    assert!((cutoff_hz(1.0) - 8_000.0).abs() < 1.0);
    // measured −3 dB point of the converter one-pole at 24 kHz: 11 kHz ± 3 % at grit 0.01, 8 kHz ± 3 % at grit 1
}
#[test] fn dither_keeps_a_minus_80_dbfs_sine_as_a_sine() {
    // grit 1, a 997 Hz sine at −80 dBFS, 1 s: its bin ≥ 10 dB over the noise median; no harmonic bin > median + 3 dB
}
#[test] fn a_grit_1_tail_has_no_correlated_products() {
    // QUAD defaults, a 1 kHz burst at −12 dBFS for 0.5 s, then 4 s of decay; each 100 ms window:
    // the bins at 2 kHz, 3 kHz and 1 kHz ± (1 / QUAD's stage-1 round trip) are ≤ median(±200 Hz) + 3 dB
}
#[test] fn idle_mute_lets_the_tail_reach_zero() {
    // grit 1: 150 ms after the send stops, the converter dither gain == 0.0; falls_silent_within(1.0, 20.0) holds
}
#[test] fn dither_mute_fades_without_a_step() {
    // Review Focus 4: send stops; GRIT eases 1 → 0.3 → 1 across IDLE_HOLD; max_d2 of the return ≤ click_free_test's bound
}
#[test] fn noise_floor_matches_the_table() {
    // QUAD, MIX 1, silent input before the mute: dBFS of the return at grit 0.3, 0.6, 1 == −66, −64, −54 ± 2
}
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_grit_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Noise:**
    - one xorshift32, stepped twice per ring sample;
    - TPDF = `(b0 as f32 − b1 as f32) / 256.0 · Δ_c`;
    - bytes 0–1 feed the ADC, 2–3 DAC L, and 4–5 DAC R.
  - **Quantise:** `rint(x / Δ_c) · Δ_c`, with `Δ_c = 2^(1−b)`.
  - **Idle mute:**
    - a counter per converter resets on any non-zero input;
    - after `IDLE_HOLD` samples, a linear gain falls to 0 over `IDLE_FADE`;
    - any non-zero input restores the gain to 1 over 24 samples.
- [ ] **Step 4: Replace the grid tests in `reverb_test.rs`.**
  - Delete `grit_rounds_onto_its_grid`, `grit_truncates_toward_zero_onto_its_grid`, `grit_1_stores_only_multiples_of_64`, `grit_raises_the_noise_floor` and `no_limit_cycle_at_grit_1`.
  - Keep `no_limit_cycle_at_grit_0` and `falls_silent_within` at 0, and change its grit-1 case to run after the idle mute.
- [ ] **Step 5: Re-record the goldens.**
  - Re-record `fx_golden_test`'s reverb cases at grit ≠ 0, using `tests/common/golden.rs`'s update procedure.
  - Expected: only those cases change.
  - List them in the commit message.
- [ ] **Step 6: Write the renderer and the owner's before-and-after artifact.**
  - `reverb_render_test.rs` has `#[test] #[ignore] fn render()`. It reads `AB_STIMULUS` and `AB_GRIT` from the environment, writes `target/ab/<stimulus>_g<grit>.wav` through `common::orbit::write_wav`, and asserts that the file exists and every sample is finite.
  - `tools/ab/stimuli.py` writes the spec's four stimuli (48 kHz, 12 s tails) from seed 343.
  - `tools/ab/render.sh <tag>` runs the renderer at grit 0, 0.3, 0.6 and 1.
  - The **before** renders come from a `git worktree` at the T4 golden reference; the **after** renders from HEAD.
  - `target/ab/grit_report.md` lists, for all eight renders, the noise floor and the correlated-product figure from `a_grit_1_tail_has_no_correlated_products`'s measure.
  - Hand the report and the WAVs to the owner. This replaces any ignored assertion on the old GRIT.
- [ ] **Step 7: Run the suite.**
  - Run: `cargo test -p chimera-core`.
  - Expected: PASS.
- [ ] **Step 8: Bench (unit).**
  - Run `just flash-bench`, then `just usb bench`.
  - Record REV QUAD with the new GRIT (estimate: 447).
- [ ] **Step 9: Commit.**

```bash
git add chimera-core/src/dsp/rev/grit.rs chimera-core/src/dsp/ring.rs chimera-core/src/dsp/rev/mod.rs chimera-core/tests/reverb_grit_test.rs chimera-core/tests/reverb_render_test.rs chimera-core/tests/reverb_test.rs chimera-core/tests/fx_golden_test.rs tools/ab/stimuli.py tools/ab/render.sh docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "GRIT is the reverb's converters; goldens re-recorded away from GRIT 0: <cases>"
```

### Task 6: TYPE, slots, knob maps, panels and pages

**Files:**
- Create: `chimera-core/src/ui/reverb_panel.rs`, `chimera-core/tests/reverb_params_test.rs`, `chimera-core/tests/reverb_panel_test.rs`
- Modify:
  - `chimera-core/src/dsp/reverb.rs`;
  - `chimera-core/src/dsp/rev/sync.rs` (adds `Division`, the lists, `fold`, `FoldState`);
  - `chimera-core/src/storage/codes.rs`;
  - `chimera-core/tests/fixtures/disk_codes_v1.txt`;
  - in `chimera-core/src/ui/`: `mod.rs` (the panel, plus the UI's TYPE and SYNC edits and `KnobMemory`), `block_def.rs`, `view.rs`, `block_registry.rs`, `glyph.rs` and `renderer.rs`.

**Interfaces:**
- **Produces, in `dsp::reverb`:**
  - `ReverbType { Quad, Room, Hall, Bloom, Gate, Reverse }` and `RevSync { Off, Time, All }`, both with explicit `DiskCode` matches;
  - `ReverbParams` gains `kind`, `sync`, `tone`, `diffuse`, `predelay`, `shape`, `wobble` and `rate`;
  - id constants `TYPE = 6` through `SYNC = 13`;
  - `pub fn with_type(self, t: ReverbType, mem: &KnobMemory) -> Self` and `pub fn with_sync(self, s: RevSync, tempo: Tempo) -> Self`;
  - `pub struct KnobMemory { slots: [[u16; 9]; 6] }`:
    - exactly **108 B**;
    - a slot stores `round(p · 65_534)`, and `u16::MAX` in a TYPE's first slot marks it unvisited;
    - `new()` fills it with `u16::MAX`;
    - methods `store(&mut self, p: &ReverbParams)`, `recall(&self, t) -> Option<[f32; 9]>` and `clear(&mut self)`;
  - `pub struct EngineKnobs { … }` (the mapped quantities) and `pub fn knobs(p: &ReverbParams, tempo: Tempo, fold: &mut FoldState) -> EngineKnobs`. This covers every row of the spec's per-TYPE tables, the wobble caps and the plate zone.
- **Produces, in `dsp::rev::sync`:**
  - `pub struct Division { pub num: u8, pub den: u8, pub kind: DivKind }`, with `DivKind { Straight, Triplet, Dotted }`;
  - the lists `ROOM_PRE`, `HALL_PRE`, `GATE_LENGTH`, `REVERSE_LENGTH` and `DECAY_BEATS: &[u8] = &[1, 2, 3, 4, 6, 8, 12, 16, 24, 32]`;
  - `pub struct FoldState(i8)`;
  - `pub fn fold(len: f32, lo: f32, hi: f32, st: &mut FoldState) -> f32`, with 2 % hysteresis.
- **Produces, in `ui::reverb_panel`:**
  - `pub enum RevPage { Main, More }`;
  - `pub struct RevKnob { pub id: ParamId, pub label: &'static str, pub short: Option<&'static str>, pub step: f32 }`;
  - `pub fn knob(t: ReverbType, page: RevPage, k: u8) -> Option<&'static RevKnob>`;
  - `pub fn label(t: ReverbType, id: ParamId) -> &'static str`, the label any CC map shows;
  - `pub fn value_text(t: ReverbType, id: ParamId, p: &ReverbParams, tempo: Tempo, fold: FoldState) -> FmtBuf`;
  - `pub fn types_on_screen() -> &'static [ReverbType]`, which is `&[Quad]` until Tasks 8–11 each add their TYPEs.
- **Produces in the UI:**
  - `SlotBinding::ReverbPanel(RevPage, u8)`;
  - `SlotCtx.rev: ReverbType`;
  - `EFX_MORE: BlockDef` (id 92, short `MORE`), chained with `ChainBlock::with_subs(&EFX, &EFX_SUB_PAGES)`;
  - `CompositeId::ReverbCube` renamed to `CompositeId::Reverb`.

- [ ] **Step 1: Write the failing tests.**

```rust
// reverb_params_test.rs
#[test] fn new_ids_and_idents_are_frozen() {
    let want = [(6,"TYPE"),(7,"TONE"),(8,"DIFFUSE"),(9,"PREDELAY"),(10,"SHAPE"),(11,"WOBBLE"),(12,"RATE"),(13,"SYNC")];
    for (id, ident) in want { assert_eq!(find_spec(&REVERB_SPECS, ParamId(id)).unwrap().ident, ident); }
    assert!(find_spec(&REVERB_SPECS, ParamId(0)).is_none());
    assert_eq!(REVERB_SPECS.len(), 13);
}
#[test] fn type_and_sync_codes_are_frozen() {
    use ReverbType::*;
    for (t, c) in [(Quad,0),(Room,1),(Hall,2),(Gate,3),(Reverse,4),(Bloom,5)] { assert_eq!(t.disk_code(), c); assert_eq!(ReverbType::from_disk_code(c), Some(t)); }
    assert_eq!(ReverbType::from_disk_code(6), None);
    for (s, c) in [(RevSync::Off,0),(RevSync::Time,1),(RevSync::All,2)] { assert_eq!(s.disk_code(), c); }
}
#[test] fn an_old_project_loads_as_quad() {
    // decode a Reverb block carrying ids 1–5 only (grit .3, time .7, damp .3, size .5, mix .5) into FxParams::default(): kind == Quad, the five values set, the rest at spec defaults
}
#[test] fn unknown_type_code_and_nan_slots_load_as_safe_quad() {
    // Review Focus 2: id 6 code 9, ids 7–13 NaN: kind == Quad; Reverb renders 1 s of noise send with every sample finite
}
#[test] fn with_type_loads_starting_knobs_and_keeps_mix_grit_sync() {
    let mut p = ReverbParams::default(); (p.mix, p.grit, p.sync) = (0.7, 0.9, RevSync::Time);
    let h = p.with_type(ReverbType::Hall, &KnobMemory::new());
    assert_eq!((h.size, h.time, h.tone, h.diffuse, h.predelay, h.wobble, h.rate, h.damping), (0.65, 0.45, 0.3, 0.4, 0.15, 0.35, 0.4, 0.3));
    assert_eq!((h.mix, h.grit, h.sync), (0.7, 0.9, RevSync::Time));
}
#[test] fn knob_memory_is_108_bytes_restores_and_clears() {
    assert_eq!(core::mem::size_of::<KnobMemory>(), 108);
    let mut m = KnobMemory::new(); assert!(m.recall(ReverbType::Room).is_none());
    let mut r = ReverbParams::default().with_type(ReverbType::Room, &m); r.size = 0.9; m.store(&r);
    let back = r.with_type(ReverbType::Hall, &m).with_type(ReverbType::Room, &m);
    assert!((back.size - 0.9).abs() <= 1.0 / 65_534.0);
    m.clear(); assert!(m.recall(ReverbType::Room).is_none());
}
#[test] fn knobs_hit_the_spec_endpoints() {
    // within 1e-3 relative, each (TYPE, slot, p) → value from the spec tables, e.g.:
    // Room size 0 → 100 ms, .5 → 141 ms, 1 → 200 ms; Room decay .5 → 0.548 s; Hall size .65 → 342 ms; Hall decay .45 → 2.49 s, 1 → 10 s;
    // Gate length .55 → 177 ms; Reverse curve .67 → 24.06 dB; Bloom swell .7 → 514 ms; Bloom decay .6 → 5.86 s; tone 0 → 10 kHz, 1 → 2.5 kHz (Room)
}
#[test] fn plate_zone_pulls_diffuse_tone_predelay() {
    // Hall, stored diffuse .4, tone .6, predelay .5: at size 0, effective (1.0, 0.0, 0.0); at size .15 and above, (0.4, 0.6, 0.5); size .075 → (0.7, 0.3, 0.25)
}
#[test] fn wobble_caps_hold_at_every_size() {
    // for size in 0..=1 step 1/128, wobble 1: Room D ≤ 0.0072·L, Hall D ≤ 0.0051·L, Bloom D ≤ 0.0072·L (L the round trip)
}
#[test] fn fold_halves_doubles_and_holds() {
    let mut st = FoldState(0);
    assert_eq!(fold(667.0, 40.0, 600.0, &mut st), 333.5);                 // GATE 1/4 at 90 BPM plays 1/8
    assert_eq!(fold(20.0, 40.0, 600.0, &mut st), 40.0);                   // doubled once
    let mut st = FoldState(0);
    for ms in [600.0, 606.0, 594.0, 611.0] { let _ = fold(ms, 40.0, 600.0, &mut st); }
    assert_eq!(st, FoldState(0));                                          // within 2 %: no flip
    assert_eq!(fold(613.0, 40.0, 600.0, &mut st), 306.5);                 // past 2 %: one halving
}

// reverb_panel_test.rs
#[test] fn every_type_shows_its_cells() {
    // Main and More labels per the spec, e.g. Hall More == ["PRE-DLY","DAMP","WOBBLE","RATE","SYNC","GRIT"]; Quad More all None; Gate More[0] None
}
#[test] fn room_sync_steps_off_and_time_only() { /* nudging SYNC under Room visits Off, Time, Off; a stored All reads as Time in value_text */ }
#[test] fn labels_and_values_fit_their_cells() { /* every label/short/value_text at its range ends ≤ the cell width; incl. "PRE", "DENS", "DIFF", "▾1/8", "PLATE 120ms" */ }
#[test] fn no_reverb_cell_has_a_hint() { /* for every TYPE, page and k: View::hint == hint::NONE */ }
#[test] fn label_follows_the_type() { assert_eq!(label(ReverbType::Room, ParamId(3)), "SIZE"); assert_eq!(label(ReverbType::Gate, ParamId(3)), "LENGTH"); }
#[test] fn quad_size_still_ticks_by_one_table_step() { /* mixer_page_test's +1/31 nudge holds; REVERB_SPECS slot 3 step == 1/128 */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_params_test --test reverb_panel_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The specs, codes and fixture rows (`B 22 6 TYPE` … `B 22 13 SYNC`, and their `E` rows).
  - `with_type`, `with_sync`, `KnobMemory`, `knobs`, `fold`, the panels and the pages.
  - The UI's TYPE edit stores the old TYPE's knobs into `KnobMemory`, then calls `with_type`. A project load calls `clear`.
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core --test reverb_params_test --test reverb_panel_test --test disk_codes_test --test block_test --test block_spec_test --test focus_glyph_test --test mixer_page_test`.
  - Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add chimera-core/src/dsp/reverb.rs chimera-core/src/dsp/rev/sync.rs chimera-core/src/storage/codes.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/src/ui/reverb_panel.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/block_def.rs chimera-core/src/ui/view.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/glyph.rs chimera-core/src/ui/renderer.rs chimera-core/tests/reverb_params_test.rs chimera-core/tests/reverb_panel_test.rs
git commit -m "Reverb TYPE, semantic slots, knob maps, panels, REV › MORE"
```

### Task 7: The switch (unit check)

**Files:**
- Create: `chimera-core/tests/reverb_switch_test.rs`
- Modify: `chimera-core/src/dsp/rev/mod.rs`, `chimera-stm32/src/bench.rs` (the `REV SWITCH MAX` row), plan § Measured

**Interfaces:**
- **Produces:**
  - `pub enum SwitchPhase { Running, FadeOut { left: u8 }, Clear, FadeIn { left: u8 } }`, with `pub const FADE_BLOCKS: u8 = 15`;
  - `Reverb::phase(&self) -> SwitchPhase`;
  - `#[doc(hidden)] pub fn force_restart(&mut self)`, which runs the same sequence QUAD → QUAD;
  - `latched: ReverbParams`, taken when a switch starts;
  - in `reverb_switch_test.rs`, `const TYPES: &[ReverbType]`. Tasks 8, 9, 10 and 11 each append their TYPEs to it.

- [ ] **Step 1: Write the failing tests.**
  - `TYPES` starts as `&[Quad]`, and each pair test also runs `force_restart`.

```rust
#[test] fn every_ordered_pair_switches_without_a_click() { /* for a, b in TYPES (and the forced restart): mid-tail at MIX 1, max_d2 over the switch window ≤ max_d2 of the 30 blocks before */ }
#[test] fn the_phases_run_15_1_15() { /* after a TYPE change: 15 blocks FadeOut, 1 Clear, 15 FadeIn, then Running */ }
#[test] fn clear_block_returns_exact_zero() { /* every L and R sample of the Clear block == 0.0, and arena().lines() all 0 after it */ }
#[test] fn fade_in_is_bit_identical_to_a_fresh_engine() { /* the pre-gain output from the first FadeIn block == a fresh Reverb of the new TYPE fed the same send from that block */ }
#[test] fn fade_out_plays_the_latched_knobs() { /* with_type overwrote the slots; FadeOut output == the old TYPE run on the old params, bit for bit */ }
#[test] fn a_switch_during_fade_in_fades_from_the_current_gain() { /* switch at FadeIn{left: 5}: the next FadeOut's first gain == the gain FadeIn had reached (10/15) ± 1e-6 */ }
#[test] fn scrolling_types_mid_tail_lands_on_the_last_without_a_click() { /* Review Focus 1: six TYPE changes on six consecutive blocks; ends Running on the last; max_d2 bound holds */ }
#[test] fn switch_at_mix_0_then_mix_up_has_no_stale_tail() { /* Review Focus 3: tail at MIX 1, MIX → 0, switch, 40 blocks, MIX → 1: output equals a fresh engine fed the same send since the switch */ }
#[test] fn a_project_swap_changing_type_takes_the_same_path() { /* fx_swap_test's harness: the phase sequence is 15/1/15 */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_switch_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement.** CLEAR, in one block, runs `arena.clear()`, resets the engine state, and zeroes both interpolators' histories.
- [ ] **Step 4: Run the tests and see them pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Add REV SWITCH MAX (unit).**
  - The row takes the maximum whole-callback block over 2,000 blocks, with six `a16_a17()` voices playing and `force_restart` every 40 blocks. Tasks 8–11 change it to step through their TYPEs.
  - Gate: ≤ `BlockBudget::budget_cycles`.
  - If it fails, make CLEAR 4 quarter-arena blocks, and change `the_phases_run_15_1_15` to `15_4_15`.
  - Run `just flash-bench`, then `just usb bench`, and record the result.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/rev/mod.rs chimera-core/tests/reverb_switch_test.rs chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "Reverb switch: latch, fade out, one-block clear, fade in"
```

### Task 8: GATE and REVERSE on the BURST engine (unit check)

**Files:**
- Create: `chimera-core/src/dsp/rev/burst.rs`, `chimera-core/tests/reverb_burst_test.rs`
- Modify:
  - `chimera-core/src/dsp/rev/mod.rs` (adds `EngineState::Burst(BurstState)`);
  - `chimera-core/src/ui/glyph.rs` and `chimera-core/src/ui/renderer.rs` (TAIL);
  - `chimera-core/src/ui/reverb_panel.rs` (`types_on_screen` adds Gate and Reverse);
  - `chimera-core/tests/reverb_switch_test.rs` (`TYPES` adds Gate and Reverse);
  - `chimera-core/tests/fx_golden_test.rs`;
  - `chimera-stm32/src/bench.rs` (REV GATE and REV RVRS; REV SWITCH MAX steps QUAD ↔ GATE);
  - `chimera-stm32/src/bench_rev.rs` (deletes `KBurst` and its row);
  - plan § Measured.

**Interfaces:**
- **Consumes:** `parts`, `ReverbArena`, `EngineKnobs` and `tests/common/rev`.
- **Produces:**
  - `pub struct BurstEngine`, with `fn prime(&mut self, k: &EngineKnobs)` and `fn run(&mut self, arena: &mut ReverbArena, u: &[f32; HALF], k: &EngineKnobs, l: &mut [f32; HALF], r: &mut [f32; HALF])`;
  - `pub const TAP_SEED: u32 = 0x5EED_0343`;
  - `pub const fn tap_table(seed: u32) -> [(f32, i8); 12]`;
  - `pub fn tail_shape(t: ReverbType, k: &EngineKnobs) -> [u8; 60]`, in `ui::glyph`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn tap_table_is_seeded_and_bounded() {
    let t = tap_table(TAP_SEED); assert_eq!(t, tap_table(TAP_SEED));
    assert_eq!((t[0].0, t[11].0), (0.0, 0.0)); assert!(t.iter().all(|&(j, s)| j.abs() <= 0.3 && (s == 1 || s == -1)));
}
#[test] fn weights_keep_sum_of_squares() { /* Σw² within 0.1 dB of 1.0 over SHAPE, CURVE, DENSITY ∈ {0, .25, .5, .75, 1}³ */ }
#[test] fn density_fades_taps_continuously() { /* stepping DENSITY by 1/128: no weight changes more than 1/64 */ }
#[test] fn scheduler_moves_at_most_four_taps_by_120() { /* LENGTH 40 → 600 ms: every crossfade window ≤ 4 taps moving, each ≤ 120 samples; all taps at target within 1 s */ }
#[test] fn gate_window() { /* LENGTH .55 (177 ms), TAIL 0, SHAPE 0: end_20db_ms ∈ [185, 660]; energy in the 50 ms after end ≤ plateau − 20 dB */ }
#[test] fn reverse_window() { /* defaults: peak_ms within ±15 % of 346 ms; 0.3 ≤ |corr(l, r)| ≤ 0.75 */ }
#[test] fn regen_settles_on_its_input_pitch() { /* check_settles_on_pitch: Reverse, REGEN 1, LENGTH 150 → 800 ms, L = 0.8 s */ }
#[test] fn no_slap_after_a_length_move() { /* check_no_slap: Reverse REGEN .7 after LENGTH .2 → .9 vs a static Reverse at .9 */ }
#[test] fn regen_1_falls_to_exact_zero() { /* Reverse REGEN 1, LENGTH 1: returns exact 0 within 24 s at grit 0, and at grit 1 after the idle mute */ }
#[test] fn level_plan_holds_for_burst() { /* check_level_plan for Gate and Reverse at REGEN 1, DECAY 1, smallest LENGTH; loop mode = nearest k / LENGTH to 500 Hz */ }
#[test] fn burst_knobs_ease() { /* check_knobs_ease over ids 1, 3, 7, 8, 10, 11, 12 */ }
#[test] fn burst_noise_and_loudness() { /* check_noise_and_loudness(gate, quad, 4.0); same for reverse — sets WET[Gate], WET[Reverse] */ }
#[test] fn tail_shape_moves_with_every_knob() { /* for Gate and Reverse, changing each present knob changes tail_shape */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_burst_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement** `BurstEngine` to the spec's § BURST engine and § Level plan.
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core --test reverb_burst_test --test reverb_switch_test`.
  - Expected: PASS.
- [ ] **Step 5: Goldens.**
  - Add `rev_gate`, `rev_gate_flat_tail0`, `rev_reverse` and `rev_reverse_regen` to `fx_golden_test`.
  - Run: `cargo test -p chimera-core --test fx_golden_test`.
  - Expected: PASS.
- [ ] **Step 6: Delete the kernel.** Remove `KBurst` and its `K BURST` row from `bench_rev.rs` and `bench.rs`.
- [ ] **Step 7: Bench and flash (unit).**
  - Run `just flash-bench`, then `just usb bench`. Record REV GATE, REV RVRS, STRESS with GATE, and REV SWITCH MAX.
  - Gates:
    - ΔCOST ≤ headroom on both builds;
    - STRESS < budget;
    - flash `text` delta since Task 7 ≤ 6 KB (two TYPEs).
- [ ] **Step 8: Provenance report.** In the task report:
  - list the tap seed, the jitter and sign table, the lengths (pre-diffuser 4.3 and 7.1 ms; TAIL 37 and 11 ms; decorrelators L 3.1 and 8.3, R 4.7 and 6.1 ms), the curves and every range, each with its spec section;
  - list every value the ear changed;
  - state that nothing came from the emulator or 7119f7ad.
- [ ] **Step 9: Commit.**

```bash
git add chimera-core/src/dsp/rev/burst.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/ui/glyph.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_burst_test.rs chimera-core/tests/reverb_switch_test.rs chimera-core/tests/fx_golden_test.rs chimera-stm32/src/bench.rs chimera-stm32/src/bench_rev.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "GATE and REVERSE on the BURST engine"
```

### Task 9: ROOM on the LOOP engine (unit check)

**Files:**
- Create: `chimera-core/src/dsp/rev/loop_engine.rs`, `chimera-core/tests/reverb_loop_test.rs`
- Modify:
  - `chimera-core/src/dsp/rev/mod.rs` (adds `EngineState::Room`);
  - `chimera-core/src/ui/reverb_panel.rs` (adds Room);
  - `chimera-core/tests/reverb_switch_test.rs` (adds Room);
  - `chimera-core/tests/fx_golden_test.rs`;
  - `chimera-stm32/src/bench.rs` (REV ROOM);
  - `chimera-stm32/src/bench_rev.rs` (deletes `KRoom`);
  - plan § Measured.

**Interfaces:**
- **Produces:**
  - `pub struct LoopEngine<const LOOPS: usize, const STAGES: usize>`, with `prime` and `run`, taking the same arguments as `BurstEngine`'s;
  - `pub type Room = LoopEngine<2, 2>` and `pub type Hall = LoopEngine<1, 4>`;
  - `pub fn rotation(theta: f32) -> (f32, f32)`, with `pub const THETA: f32 = 0.25`.
- ROOM uses the ROOM-T stage that Task 3 recorded (none, 1, or 1 + 2).

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn room_layout_fits_and_lengths_are_distinct_primes() {
    // every line length at k = 1 is prime, all distinct; Σ (max_len + 2) ≤ RING_LEN; diffuser and stage AP lengths match the spec ms × 24 to the nearest distinct prime
}
#[test] fn rotation_preserves_energy() { /* gains 1, dampers off: loop-pair energy constant to 1e-6 over 10 round trips; (c, s) = rotation(THETA) has c² + s² == 1 ± 1e-7 */ }
#[test] fn round_trip_moves_at_most_5_ms_per_crossfade() { /* SIZE 0 → 1: each crossfade changes Σ stage delays by ≤ 120 samples, all four in the same crossfade; PRE-DLY the same */ }
#[test] fn room_settles_on_its_input_pitch() { /* check_settles_on_pitch: SIZE sweep 0 → 1, then PRE-DLY 0 → 1, then tempo 120 → 90 BPM under SYNC TIME; L = 0.2 s */ }
#[test] fn room_wobble_is_bounded_without_drift() { /* check_wobble_bound at WOBBLE 1, RATE 1, SIZE 0 (L = 0.1 s) */ }
#[test] fn no_slap_after_a_size_sweep() { /* check_no_slap: Room after SIZE 0 → 1 vs a static Room at 1 */ }
#[test] fn room_window_and_rt60() {
    // defaults: rt60_t20 == 0.548 s ± 30 %, onset_ms ≤ 34;
    // DECAY .25, .5, .75 and 1: rt60 within ±35 % of 0.2·7.5^p (floored at t_min); DECAY 0 within ±35 % of 0.2 s (checks the rotation)
}
#[test] fn level_plan_holds_for_room() { /* check_level_plan at DECAY 1, SIZE 0, DIFFUSE 1; plus every stored quantity's recorded peak within its § Level plan bound */ }
#[test] fn room_knobs_ease() { /* check_knobs_ease over ids 1, 3, 7, 8, 9, 11, 12 */ }
#[test] fn room_noise_and_loudness() { /* check_noise_and_loudness(room, quad, 4.0) at defaults; extra 6.0 at DIFFUSE 1 */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_loop_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement** `LoopEngine` and `Room` to the spec's § LOOP engine.
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core --test reverb_loop_test --test reverb_switch_test`.
  - Expected: PASS.
- [ ] **Step 5: Golden.** Add `rev_room`, then run `--test fx_golden_test`. Expected: PASS.
- [ ] **Step 6: Delete the kernel.** Remove `KRoom` and its row.
- [ ] **Step 7: Bench and flash (unit).** Record REV ROOM, STRESS with ROOM, and flash.
  - Gates:
    - ΔCOST ≤ 96 (default) and ≤ 23 (tape). If it is over, take the next ROOM-T stage and re-measure.
    - STRESS < budget.
    - The flash delta is ≤ 3 KB.
- [ ] **Step 8: Provenance report.** List ROOM's lengths, taps and ranges with their spec sections, the ROOM-T stage taken, and every ear change. State that nothing came from the emulator or 7119f7ad.
- [ ] **Step 9: Commit.**

```bash
git add chimera-core/src/dsp/rev/loop_engine.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_loop_test.rs chimera-core/tests/reverb_switch_test.rs chimera-core/tests/fx_golden_test.rs chimera-stm32/src/bench.rs chimera-stm32/src/bench_rev.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "ROOM on the LOOP engine"
```

### Task 10: HALL and its plate zone (unit check)

**Files:**
- Modify:
  - `chimera-core/src/dsp/rev/loop_engine.rs` (Hall);
  - `chimera-core/src/dsp/rev/mod.rs` (adds `EngineState::Hall`);
  - `chimera-core/src/ui/reverb_panel.rs` (adds Hall and the `PLATE` value text);
  - `chimera-core/tests/reverb_loop_test.rs`;
  - `chimera-core/tests/reverb_switch_test.rs` (adds Hall);
  - `chimera-core/tests/fx_golden_test.rs`;
  - `chimera-stm32/src/bench.rs` (REV HALL);
  - `chimera-stm32/src/bench_rev.rs` (deletes `KHall` and `KPair`; the whole file goes, as does `mod bench_rev;` in `chimera-stm32/src/main.rs`);
  - plan § Measured.

**Interfaces:**
- **Consumes:** `LoopEngine` and `knobs`' plate zone.
- **Produces:** `EngineState::Hall(Hall)`.

- [ ] **Step 1: Write the failing tests** in `reverb_loop_test.rs`.

```rust
#[test] fn hall_reaches_10_s_at_size_0() { /* SIZE 0, DECAY 1: no stage gain above 0.985; rt60_t20 == 10 s ± 35 % */ }
#[test] fn plate_corner_onset_is_short() { /* SIZE 0, DECAY .35, TONE 0, DIFFUSE 1, DAMP .1, PRE 0, WOBBLE .5, RATE .6: onset_ms ≤ 8 */ }
#[test] fn plate_zone_moves_predelay_by_crossfade() { /* PRE .5, SIZE .15 → 0: the PRE read length changes only at crossfade starts, by ≤ 120 samples each */ }
#[test] fn hall_decorrelates() { /* defaults, noise burst: |corr(l, r)| ≤ 0.2 */ }
#[test] fn hall_settles_on_its_input_pitch() { /* check_settles_on_pitch: SIZE 0 → 1 then PRE 0 → 1; L = 0.6 s */ }
#[test] fn hall_wobble_is_bounded_without_drift() { /* check_wobble_bound at SIZE 0 (L = 0.12 s) */ }
#[test] fn level_plan_holds_for_hall() { /* check_level_plan at DECAY 1, SIZE 0, DIFFUSE 1; stored-quantity peaks within their bounds */ }
#[test] fn hall_knobs_ease() { /* check_knobs_ease over ids 1, 2, 3, 7, 8, 9, 11, 12 */ }
#[test] fn hall_noise_and_loudness() { /* check_noise_and_loudness(hall, quad, 4.0); 6.0 at DIFFUSE 1 */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_loop_test`.
  - Expected: FAIL on the new tests.
- [ ] **Step 3: Implement** Hall and the plate zone.
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core --test reverb_loop_test --test reverb_switch_test`.
  - Expected: PASS.
- [ ] **Step 5: Goldens.** Add `rev_hall` and `rev_hall_plate`, then run `--test fx_golden_test`. Expected: PASS.
- [ ] **Step 6: Delete the kernels.** Remove `KHall`, `KPair`, `bench_rev.rs` and its `mod` line.
- [ ] **Step 7: Bench and flash (unit).** Record REV HALL, STRESS with HALL, and flash.
  - Gates: ΔCOST ≤ headroom on both builds, STRESS < budget, and a flash delta ≤ 3 KB.
- [ ] **Step 8: Provenance report.** List HALL's lengths, taps, the plate-zone law and ranges with their spec sections, and every ear change. State that nothing came from the emulator or 7119f7ad.
- [ ] **Step 9: Commit.**

```bash
git add chimera-core/src/dsp/rev/loop_engine.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_loop_test.rs chimera-core/tests/reverb_switch_test.rs chimera-core/tests/fx_golden_test.rs chimera-stm32/src/bench.rs chimera-stm32/src/bench_rev.rs chimera-stm32/src/main.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "HALL and its plate zone; bench kernels retired"
```

### Task 11: BLOOM (unit check)

**Files:**
- Create: `chimera-core/src/dsp/rev/bloom.rs`, `chimera-core/tests/reverb_bloom_test.rs`
- Modify:
  - `chimera-core/src/dsp/rev/mod.rs` (adds `EngineState::Bloom`);
  - `chimera-core/src/ui/reverb_panel.rs`;
  - `chimera-core/tests/reverb_switch_test.rs` (adds Bloom);
  - `chimera-core/tests/fx_golden_test.rs`;
  - `chimera-stm32/src/bench.rs` (REV BLOOM);
  - plan § Measured.

**Interfaces:**
- **Produces:** `pub struct BloomEngine`, with `prime` and `run`, taking the same arguments as `BurstEngine`'s; and `EngineState::Bloom`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn bloom_window() { /* defaults: onset_ms ∈ [140, 300], peak_ms ∈ [450, 750] */ }
#[test] fn swell_moves_one_delay_by_crossfade() { /* SWELL 0 → 1: only the SWELL delay and PRE-DLY change length, at crossfade starts, by ≤ 120 samples; the ten AP lengths never change */ }
#[test] fn bloom_settles_on_its_input_pitch() { /* check_settles_on_pitch: SWELL 0 → 1; L = 0.7 s */ }
#[test] fn bloom_wobble_is_bounded_without_drift() { /* check_wobble_bound at SWELL 0 (L = 0.25 s); APs 3, 6, 9 at 0°, 90°, 180° */ }
#[test] fn bloom_rt60() { /* DECAY .6: rt60 == 5.86 s ± 35 %; DECAY 1: 12 s ± 35 % */ }
#[test] fn level_plan_holds_for_bloom() { /* check_level_plan at DECAY 1, SWELL 0, DIFFUSE 1 */ }
#[test] fn bloom_knobs_ease() { /* check_knobs_ease over ids 1, 3, 7, 8, 9, 11, 12 */ }
#[test] fn bloom_noise_and_loudness() { /* check_noise_and_loudness(bloom, quad, 4.0); 6.0 at DIFFUSE 1 */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_bloom_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement** to the spec's § BLOOM engine.
  - The split-SWELL fallback is built only if the A/B in Task 13 asks for it. In the tape build it would also need an owner ruling.
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core --test reverb_bloom_test --test reverb_switch_test`.
  - Expected: PASS.
- [ ] **Step 5: Golden.** Add `rev_bloom`, then run `--test fx_golden_test`. Expected: PASS.
- [ ] **Step 6: Bench and flash (unit).** Record REV BLOOM, STRESS with BLOOM, and flash.
  - Gates: ΔCOST ≤ headroom, STRESS < budget, and a flash delta ≤ 3 KB.
- [ ] **Step 7: Provenance report.** List BLOOM's ten allpass lengths, the SWELL range, the decorrelators and every range with their spec sections, and every ear change. State that nothing came from the emulator or 7119f7ad.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/rev/bloom.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_bloom_test.rs chimera-core/tests/reverb_switch_test.rs chimera-core/tests/fx_golden_test.rs chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "BLOOM"
```

### Task 12: Tempo sync on ORBIT's clock

**Files:**
- Create: `chimera-core/tests/reverb_sync_test.rs`
- Modify:
  - `chimera-core/src/dsp/rev/sync.rs` (adds `Tempo::from_bpm` and `Tempo::quantised`);
  - `chimera-core/src/instrument.rs` (`pub fn set_tempo(&mut self, t: Tempo)`, plus a `tempo` field passed into `mix_parts`);
  - `chimera-core/src/orbit/runner.rs` (adds `pub fn tempo(&self) -> Tempo`, and `block` calls `inst.set_tempo(self.tempo())`);
  - `chimera-core/src/ui/reverb_panel.rs` (shows SYNC on screen);
  - `chimera-core/tests/fx_golden_test.rs`.

**Interfaces:**
- **Consumes:** Task 6's `Division`, lists, `fold` and `FoldState`.
- **Produces:**
  - `Tempo::from_bpm(bpm: f32, sample_rate: u32) -> Tempo`;
  - `Tempo::quantised(self, held: Tempo) -> Tempo`, in 0.25 BPM steps that move only once the tempo is 0.15 BPM past the held step;
  - `OrbitRunner::tempo`: the leading MIDI clock's `bar_len() / 4` under EXT, else the internal `Bpm`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn division_lists_sit_in_range_at_120() {
    let t = Tempo::from_bpm(120.0, 48_000);
    // every GATE_LENGTH entry's ms ∈ [40, 600]; REVERSE_LENGTH ∈ [150, 800]; ROOM_PRE ∈ [0, 50]; HALL_PRE ∈ [0, 125]
    // GATE_LENGTH.first() == 1/32T (41.7 ms), .last() == 1/4 (500 ms)
}
#[test] fn fold_keeps_every_division_in_range_from_30_to_300_bpm() { /* for bpm 30..=300 step 1, every list entry: fold(..) ∈ [lo, hi] */ }
#[test] fn tempo_quantiser_holds_through_jitter() {
    let held = Tempo::from_bpm(120.0, 48_000);
    for b in [119.9, 120.1, 119.95] { assert_eq!(Tempo::from_bpm(b, 48_000).quantised(held), held); }
    assert_eq!(Tempo::from_bpm(120.4, 48_000).quantised(held), Tempo::from_bpm(120.5, 48_000));
}
#[test] fn decay_in_beats_clamps_and_room_has_no_all() {
    // Hall, SYNC All, 32 beats at 120 BPM (16 s) → 10 s; 1 beat at 300 BPM (0.2 s) → 0.8 s; Room with stored All maps PRE as Time and DECAY in seconds
}
#[test] fn sync_toggle_keeps_the_length_within_one_division() {
    // Gate LENGTH .55 (177 ms) at 120 BPM: with_sync(Time) picks 1/16T (167 ms); back to Off restores 167 ms ± 1 ms
}
#[test] fn division_encoding_round_trips() { /* for every list of n entries and every i: round(p·(n−1)) == i with p = i/(n−1) */ }
#[test] fn gate_sync_eighth_at_120_is_250_ms() { /* Gate, SYNC Time, LENGTH at 1/8: knobs(..).length_ms == 250.0 ± 0.1 */ }
#[test] fn clock_dropout_moves_synced_length_once() {
    // Review Focus 5: OrbitRunner under SYNC EXT at 100 BPM, Gate 1/4 synced; ticks stop; after 500 ms the internal 120 BPM takes over:
    // exactly one change of the LENGTH target; the taps reach it by crossfades; max_d2 ≤ click_free bound; FoldState unchanged throughout
}
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_sync_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement**, wiring `OrbitRunner::block` → `Instrument::set_tempo` → `mix_parts` → `FxBus::process`.
- [ ] **Step 4: Run the tests and see them pass.**
  - Run: `cargo test -p chimera-core`, which includes ORBIT's tests.
  - Expected: PASS.
- [ ] **Step 5: Golden.** Add `rev_gate_sync_8th_120`, then run `--test fx_golden_test`. Expected: PASS.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/rev/sync.rs chimera-core/src/instrument.rs chimera-core/src/orbit/runner.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_sync_test.rs chimera-core/tests/fx_golden_test.rs
git commit -m "Reverb tempo sync on ORBIT's clock"
```

### Task 13: Sweeps, screens, A/B, final bench and the ADR (unit)

**Files:**
- Modify:
  - `chimera-core/tests/param_sweep_test.rs` and `chimera-core/tests/common/sweep.rs`;
  - `chimera-core/tests/screen_golden_test.rs`, with images under `docs/screens/`;
  - `chimera-core/src/dsp/fx_bus.rs` (`COST`) and `chimera-core/tests/cost_test.rs` (the `bus` literal, both arms: `if TAPE { … } else { … }`);
  - `tools/ab/render.sh`;
  - `docs/adr/0085-the-master-reverb-picks-a-type.md`;
  - plan § Measured.

- [ ] **Step 1: Make the sweeps TYPE-aware.**
  - `sweep_fast` sweeps every present knob under every TYPE.
  - `sweep_thorough` adds every SYNC value at 30, 120 and 300 BPM.
  - Allowed behaviours, each listed with its reason:
    - TYPE's switch dips the return by design;
    - TAIL 0 and REGEN 0 give short tails.
  - Run: `cargo test -p chimera-core --test param_sweep_test`, then the same with `-- --ignored`.
  - Expected: PASS.
- [ ] **Step 2: Screen goldens.**
  - Add REV and REV › MORE under QUAD, HALL and GATE.
  - Run: `just screens`, then `cargo test -p chimera-core --test screen_golden_test`.
  - Expected: PASS.
- [ ] **Step 3: The A/B renders.**
  - Render Chimera's side of the spec's eleven pairs to `target/ab/` with `tools/ab/render.sh pairs`.
  - Write `target/ab/report.md`, with RT60 (T20), onset, peak, −20 dB end, tilt and ρ for each pair, measured with `tests/common/rev.rs`'s measures, and a VST column left blank.
  - Hand it to the owner.
- [ ] **Step 4: Final bench (unit).**
  - On both builds, record:
    - every REV row;
    - BUS+V with the costliest TYPE;
    - STRESS with each TYPE;
    - REV SWITCH MAX, stepping through all six TYPEs.
  - If the costliest BUS+V reading exceeds `FxBus::COST`, set `COST` to it and update both arms of `cost_test`'s `bus` literal. Then run `cargo test -p chimera-core --test cost_test` and `cargo test -p chimera-core --features master-tape --test cost_test`. Expected: PASS.
  - Record the flash total (`rust-size` `text`).
- [ ] **Step 5: The ADR.**
  - Write the measured CPU, flash and headroom into ADR 0085 § Consequences.
  - Its status stays Proposed until the owner's A/B and UAT.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/tests/param_sweep_test.rs chimera-core/tests/common/sweep.rs chimera-core/tests/screen_golden_test.rs docs/screens chimera-core/src/dsp/fx_bus.rs chimera-core/tests/cost_test.rs tools/ab/render.sh docs/adr/0085-the-master-reverb-picks-a-type.md docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "Reverb TYPEs: sweeps, screens, A/B renders, measured costs"
```

---

## Measured

(Filled in by Tasks 1, 3, 4, 5 and 7–13: row, value, build, commit, date. Task 4 also records the T4 golden reference commit here.)

### Task 1: baselines (2026-10-04, rev V at 480 MHz)

Bench at `2431fedc` (FX rows from every effect off). FX rows are cycles per
sample less MIX; MIX, BUS and BUS+V as read; STRESS in cycles per block
against `BlockBudget::budget_cycles` (448,000).

| Row | default (`bench`) | tape (`bench,master-tape`) | ORBIT (`bench,orbit`) |
|---|---|---|---|
| MIX | 868 | 861 | 871 |
| REV QUAD (QUAD_REV) | **66** | 57 | 61 |
| BUS (QUAD_BUS) | **1118** | 1430 | 1117 |
| BUS+V (QUAD_BUS_V) | **1188** | **1483** | 1186 |
| STRESS (max/budget) | **419,471 / 448,000** | **438,953 / 448,000** | **536,475 / 448,000: over** |
| `stats` OVER after | 0 | 0 | 0 |

- **REV QUAD is 66, not 485.** Since `9e004b98` every effect runs at MIX 0,
  so MIX already holds the reverb at its defaults and the row is QUAD's
  worst less QUAD's default. ADR 0031's 485 was the whole reverb, read
  when an effect off was skipped. ΔCOST between TYPE rows stays valid; the
  485 target is in the old frame.
- **BUS+V 1188 > `FxBus::COST` 1180:** COST is now 1188 (`f23499a2`); every
  `cost_test` assertion passes. Tape 1483 ≤ 1490: unchanged.
- **STRESS with ORBIT is over budget.** A diagnostic run (not committed):
  1,432 of 7,500 blocks over; the worst block had 7 voices sounding, ORBIT's
  block 77,986 and the render 458,419 alone, over budget by itself. The
  allocator bills neither ORBIT's block nor enough for the seventh voice.
  Without ORBIT both builds hold.
- **Headroom** (min over `cost_test`'s voice-count assertions, host,
  `f23499a2`): default **76** (Modal BOWED, BODY 0.3, ENS 0.5: 8 × 717 on
  rev V; FOLD 6 × 954 is 88); at the old COST 1180 it was 84, not 96. Tape
  **23** (MORPH KEYS 5 × 864 on rev Y).
- **Flash:** plain release `text` 701,428 B at `f23499a2`.
- **Card** (`card`, bench build): streams 10, seq_16k 2,281 KB/s,
  rand_4k p95 5,410 µs, card 15,931 MB, free 15,912 MB
  (`.superpowers/sdd/2026-10-04-reverb-types/card-run.txt`).

### Task 4: the dispatcher (2026-10-04, host)

- **T4 golden reference: `a2abe82f`.** QUAD through `Reverb` hashes as
  `RingReverb` did at `695a6ef3`: `reverb_dispatch_test`'s three goldens
  (a 48 kHz and a 44.1 kHz sweep of every control with SIZE crossfades,
  MIX ramps and a saturating burst; the whole bus with REV SEND) and
  every `fx_golden_test` case, unchanged.
- **Flash:** plain release `text` 701,012 B (+240 over `695a6ef3`'s
  700,772); `--features orbit` 720,596 B (+352 over 720,244).
- **REV QUAD bench:** deferred to the next unit session (the unit was
  busy); gate REV QUAD ≤ QUAD_REV + 1 %.

### Task 5: the converters (2026-10-04, host)

- **GRIT 0 is bit for bit the T4 reference:** `reverb_grit_test`'s
  `grit_0_bypasses_bit_for_bit` holds `fx_golden_test`'s two reverb renders
  at GRIT 0 to their `a2abe82f` hashes, and the A/B's GRIT 0 WAVs are
  byte-identical before and after.
- **Re-recorded** (every one at GRIT ≠ 0): `fx_golden_test`'s
  `reverb_ring`, `reverb_ring_max_size`, `reverb_ring_full_grit`;
  `reverb_dispatch_test`'s `quad_sweep_48k`, `quad_sweep_44k1`, `quad_bus`
  (the sweep walks GRIT); `instrument_test`'s `reverb_send_on` and its
  pre-limiter row (the default GRIT 0.3).
- **Floor** (QUAD defaults, MIX 1, under a steady −12 dBFS 1 kHz sine,
  20 Hz–10.5 kHz): GRIT 0 −68.1, 0.3 −67.5, 0.6 −61.1, 1 −47.8 dBFS.
  The § GRIT table's −64 and −54 assumed the ADC's noise reaches the return
  at unity; QUAD's steady-state gain to white noise at its defaults is
  about +8.5 dB. The old grid read −57.3, −46.8 and −33.1.
- **Products** (2 and 3 kHz in a 1 kHz burst's 4 s tail, summed over
  100 ms windows): new 0.2 dB at GRIT 1; old 3.6, 7.7 and 12.5 dB at 0.3,
  0.6 and 1.
- **Landing on GRIT 0** fades the converters out across that block: the
  kink in a settled tone's period-to-period residual reads 1.0e−3 against
  3.7e−3 for a hard switch (16-bit dither alone, 5e−4).
- **The high cut** is a prewarped bilinear one-pole: the brief's one-pole
  `y += a·(x − y)` cannot reach −3 dB near 11 kHz at 24 kHz without
  shelving 1.4 dB off 5 kHz.
- **AXI:** 442,156 B (+24; the ratchet moves with it). The DAC dithers
  from its own noise word, so no per-block byte buffer.
- **Flash:** plain release `text` 702,948 B (+1,936 over T4's 701,012);
  `--features orbit` 722,332 B (+1,736 over 720,596).
  `Converters::set` is 784 B of it. `#[inline]` for `#[inline(always)]`
  on the per-sample helpers builds a byte-identical binary.
- **REV QUAD bench:** deferred (the unit was busy).

---

## Self-review

- **Spec coverage.** Each spec section maps to its tasks:

| Spec section | Tasks |
|---|---|
| TYPE list and codes | T6 |
| Engines | T8–T11 |
| Moving lengths and pitch bounds | T2, T8–T11 |
| Level plan | T2 (`check_level_plan`), T8–T11 |
| GRIT | T5 |
| Arena and f32 regions | T2, T4 |
| Parameters, `with_type` and memory | T6 |
| Switch | T7 |
| Smoothing | T2 (`Ease1`, `check_knobs_ease`), T8–T11 |
| Wobble | T2, T9–T11 |
| Modulation | deferred, nothing to build |
| Tempo sync | T6 (`fold`), T12 |
| UI, glyph and hints | T6, T8 |
| CPU, step 0, ROOM-T and bench rows | T1, T3, T7–T13 |
| Flash | T8–T11, T13 |
| Storage | T6 |
| A/B | T5, T13 |

  The spec's build order maps to the tasks as follows:

| Spec step | Tasks |
|---|---|
| 0 | T1–T3 |
| 1a | T4–T5 |
| 1b | T6–T7 |
| 2 | T8 |
| 3 | T9 |
| 4 | T10 |
| 5 | T11 |
| 6 | T12 |
| 7 | T13 |

- **Steps.** Every test has its assertions and the spec's values. Function bodies appear only for the soft clip, TPDF and idle mute, and ROOM-T. Every commit stages its paths.
- **Types.** Each is defined once, in one task, and used under that name after: `Tempo` (T4), `FoldState`/`fold`/`Division` (T6), `EngineKnobs`/`knobs` (T6), `KnobMemory` (T6, 108 B, `u16::MAX` sentinel), `SwitchPhase` (T7), `LoopEngine<LOOPS, STAGES>` (T9), `Render` and the `check_*` helpers (T2).
- **Review Focus.** Each of the five lines has its test, in T7, T6, T7, T5 and T12.
- **Proportion.** The code blocks are test assertions and signatures, with no engine bodies.
