# Reverb TYPEs Implementation Plan

**Goal:** Give the master reverb a TYPE: today's QUAD, plus ROOM, HALL (with PLATE as a zone of it), BLOOM, GATE and REVERSE. They share one i16 arena, switch without a click, and never re-pitch a tail. GRIT leaves the reverb, and SMEAR (random wander) comes to every TYPE, QUAD included.

**Architecture:**
- The three new engines (LOOP, BURST, BLOOM) live in `chimera-core/src/dsp/rev/`, beside today's `dsp::ring` (QUAD). A dispatcher, `dsp::rev::Reverb`, runs one engine at a time over a shared `ReverbArena`, and replaces `RingReverb` in `FxBus`.
- Lengths move by pitch-free crossfades between fixed reads. Only SMEAR's reads are interpolated.
- The level plan bounds every stored value with a soft clip at each loop's re-entry, normalised in-loop allpass states, and f32 diffusers and decorrelators.
- GRIT leaves the reverb (owner, 2026-10-04): no converters, no `Grid`, and ParamId 5 retired. SMEAR, an independent random wander per modulated line, replaces the sine wobble on every TYPE, QUAD included.
- One parameter block of semantic slots, with a per-TYPE `const` panel.

**Tech Stack:** Rust 2024 on stable; `no_std` `chimera-core`; `chimera-stm32` (bench build, `--features bench`); host tests in `chimera-core/tests`; `tools/chimera-usb.py` for the unit.

**Spec:** `docs/superpowers/specs/2026-10-04-reverb-types-design.md`. ADRs: `docs/adr/0085-the-master-reverb-picks-a-type.md`, and `docs/adr/0095-every-reverb-type-matches-its-midiverb-ii-presets.md` (the MidiVerb II reference; § Amendment, 2026-10-05).

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
- **Provenance.** Through Task 10: no code, instruction listing, or ROM coefficient, offset or tap table may come from github.com/thement/midiverb_emulator or commit 7119f7ad, whole or in part. From Task 9b on, ADR 0095 governs: structure numbers (delay and allpass lengths, gains, tap offsets) read from the decompiled MidiVerb II programs may be used, by owner approval of 2026-10-05, and only those named in ADR 0095. No emulator code, listing, harness code or rendered output ever enters the repository, and knob values are fitted, never copied. Every engine task writes a provenance report: each table, its source (spec or ADR 0095), and each ear change.
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
4. **SIZE moved on QUAD while SMEAR is up** eases the smear out, crossfades, and eases it back, with no click and no block running both. Test: Task 5, `quad_size_move_under_smear_does_not_click`.
5. **The MIDI clock dropping out while synced** moves a synced length once, with no fold flapping and no click. Test: Task 12, `clock_dropout_moves_synced_length_once`.

---

## File map

| File | Responsibility | Created in |
|---|---|---|
| `chimera-core/src/dsp/rev/mod.rs` | `Reverb`, `EngineState`, `SwitchPhase`, the half-bands, the MIX ease | T2 (stub), T4 |
| `chimera-core/src/dsp/rev/arena.rs` | `ReverbArena`, f32 regions, `clear` | T2 |
| `chimera-core/src/dsp/rev/parts.rs` | soft clip, allpasses, pair read, `Xfade`, `Wander` (T5; replaces T2's `Wobble`), `Ease1`, `one_pole` | T2, T5 |
| `chimera-core/src/dsp/rev/sync.rs` | `Tempo`, then `Division`, the lists, `fold`, `FoldState`, then tempo quantising | T4, T6, T12 |
| `chimera-core/src/dsp/rev/grit.rs` | removed in T5 (kept on `converters-wip`) | T5 |
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
  - `pub struct Wobble { phase: u32, inc: u32 }`, with `set_rate(&mut self, hz: f32, fs: f32)` and `block(&mut self) -> [f32; 2]` (L and R, 90° apart, in −1..1). Built here; Task 5 replaces it with `Wander`;
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
    - `check_smear_bound(r, loop_s)` (Task 5 renames T2's `check_wobble_bound`): ±25 cents on every pass, a mean over each 4/RATE window of ±1 cent, and no line in the pitch-deviation spectrum above its median + 10 dB.
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

### Task 5: GRIT out; SMEAR into QUAD (unit check)

Owner ruling, 2026-10-04: GRIT leaves the reverb, and SMEAR (Lexicon-style random wander) replaces the wobble, QUAD included. This task undoes the converters built at `46540d87`/`ae8edcba` and lands SMEAR on QUAD. The parts are already built, so `Wobble` is **replaced** by `Wander` in `parts.rs`, not kept beside it: nothing else uses it, and keeping both would leave a dead periodic modulator.

**Files:**
- Delete: `chimera-core/src/dsp/rev/grit.rs` and `chimera-core/tests/reverb_grit_test.rs`. First run `git branch -f converters-wip ae8edcba`, so the code survives for the master-stage issue.
- Create: `chimera-core/tests/reverb_smear_test.rs`
- Modify:
  - `chimera-core/src/dsp/rev/parts.rs` (`Wobble` → `Wander`) and `chimera-core/tests/reverb_parts_test.rs`;
  - `chimera-core/tests/common/rev.rs` (`check_wobble_bound` → `check_smear_bound`; drop `sine_floor_dbfs`'s GRIT use, `burst_products_db` and `products_db`);
  - `chimera-core/src/dsp/rev/mod.rs` (no converters; QUAD's SMEAR);
  - `chimera-core/src/dsp/ring.rs` (`Grid` removed; fixed Q3 at 1 LSB by `q_round_fixed` / `q_trunc_fixed`; the third monomorph);
  - `chimera-core/src/dsp/reverb.rs` (GRIT out; SMEAR and RATE in for QUAD);
  - `chimera-core/src/storage/codes.rs` (`(22, 5)` added to `RETIRED`);
  - `chimera-core/tests/fixtures/disk_codes_v1.txt` (the `B 22 5 GRIT` row goes);
  - `chimera-core/tests/reverb_test.rs`;
  - `chimera-core/tests/fx_golden_test.rs`, `chimera-core/tests/reverb_dispatch_test.rs` and `chimera-core/tests/instrument_test.rs` (re-recorded);
  - `chimera-core/tests/mixer_page_test.rs` (QUAD's cells);
  - `chimera-core/src/ui/block_registry.rs` (`EFX`: SIZE · TIME · DAMP · SMEAR; QUAD's RATE is placed on MORE by Task 6) and `chimera-core/src/ui/glyph.rs` (`CUBE_PARAMS`: GRIT → SMEAR; the cube draws SMEAR as a slow random jitter of its far edges);
  - `chimera-core/tests/reverb_render_test.rs` and `tools/ab/render.sh` (`AB_GRIT` → `AB_SMEAR`);
  - `chimera-stm32/src/bench.rs` (the REV QUAD S row), plan § Measured.

**Interfaces:**
- **Produces, in `parts.rs`:**
  - `pub struct Wander { rng: u32, target: f32, lp1: f32, lp2: f32, left: u32, period: u32, a: f32 }`
  - `fn new(seed: u32, phase: f32) -> Self`
  - `fn set_rate(&mut self, hz: f32, fs: f32)`, which sets `period = fs / (2·hz)` samples and `a = one_pole(hz, fs / HALF as f32)`.
  - `fn block(&mut self) -> f32`: the smoothed drift at this block's end, in −1..1.
  - `pub fn wander_seeds<const N: usize>(engine_seed: u32) -> [u32; N]`: one xorshift32 seed per line, decorrelated.
- **Produces in `dsp::reverb`:**
  - `ReverbParams` drops `grit` and gains `smear` and `rate` (for QUAD now; Task 6 adds the rest);
  - `ReverbParams::SMEAR = ParamId(11)`, ident `SMEAR`, default 0.0; `RATE = ParamId(12)`, default 0.5;
  - `REVERB_SPECS` has no id 5.
- **Produces in `ring.rs`:**
  - `Ring::run::<const XF: bool, const SMEAR: bool>`. `<true, true>` is never instantiated.
  - The SMEAR path reads each stage delay at `d − D·(1 + x_k)`, a moving pair, with `x_k` from its own `Wander`.
  - `D = min(depth_ms·24, 0.0018·ring_round_trip)`.
- **Produces in `Reverb`:**
  - A SIZE request while the eased SMEAR depth is above 0 first eases the depth to 0 over 20 ms.
  - When the depth is exactly 0, it runs the crossfade (`XF`, no SMEAR), then eases the depth back up.

- [ ] **Step 1: Write the failing tests.**

```rust
// reverb_parts_test.rs (replaces the wobble test)
#[test] fn wander_is_smooth_bounded_and_decorrelated() {
    // four Wanders from wander_seeds::<4>(0x5EED_0343), set_rate(1.0, 24_000.0), 120 s of blocks:
    // every value in −1..1; block-to-block step ≤ 2π·1·(32/24_000)·1.1; pairwise |corr| ≤ 0.2;
    // the spectrum of each trace has no line above its median + 10 dB (no period)
}

// reverb_smear_test.rs
#[test] fn quad_at_smear_0_is_the_t4_reference() {
    // fx_golden_test's GRIT-0 reverb renders and reverb_dispatch_test's GRIT-0 goldens hash as at a2abe82f
}
#[test] fn grit_is_retired() {
    // REVERB_SPECS has no ParamId(5); RETIRED contains (22, 5); decoding a v1 block with id 5 = 0.8 succeeds and leaves params == default
}
#[test] fn quad_smear_reads_only_shorten_and_respect_the_cap() {
    // SMEAR 1 at SIZE 0 and 1: every stage-delay read in [d − 2D, d] with d ≤ BASE[j]; Σ 2D ≤ 0.0145 · ring round trip
}
#[test] fn quad_smear_has_no_vibrato_and_no_drift() {
    // check_smear_bound(quad at SMEAR 1, RATE 1, SIZE 0): per pass ±25 cents; mean over each 4/RATE window ±1 cent;
    // pitch-deviation spectrum has no line above median + 10 dB
}
#[test] fn quad_size_move_under_smear_does_not_click() {
    // Review Focus 4: SMEAR 1, SIZE 0.2 → 0.8 mid-tail: no block runs XF and SMEAR together (Reverb::debug_paths());
    // depth reaches 0 before the first crossfade block; max_d2 ≤ click_free_test's bound
}
#[test] fn smear_spreads_a_held_tone() {
    // QUAD defaults with SMEAR 0 vs 1, 1 kHz held: the tail's −20 dB spectral width at 1 kHz is ≥ 2× wider at SMEAR 1
}
#[test] fn quad_falls_silent_with_smear() { /* SMEAR 1: exact zero within falls_silent_within's QUAD bound */ }
```

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p chimera-core --test reverb_smear_test --test reverb_parts_test`.
  - Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **GRIT out:**
    - delete `grit.rs` and its use in `Reverb`;
    - `ring.rs` stores through fixed `q_round_fixed`/`q_trunc_fixed`, which give the same bits `Grid::new(0.0)` gave;
    - retire id 5.
  - **SMEAR in:**
    - `Wander`, set out below;
    - QUAD's third monomorph, with the 4 stage-delay reads as `parts::pair`;
    - the depth ease and the SIZE-move sequencing in `Reverb`.
  - **`Wander::block`:**
    - `left` counts down by HALF. At 0, it draws a new `target` (an xorshift32 step, mapped to −1..1) and resets `left = period`.
    - Then `lp1 += a·(target − lp1)` and `lp2 += a·(lp1 − lp2)`, and it returns `lp2`.
    - The phase offsets: line k starts at `left = k·period/n`.
- [ ] **Step 4: Replace the tests.**
  - Delete `reverb_grit_test.rs`.
  - In `reverb_test.rs`, the grid tests are gone already. Keep `no_limit_cycle_at_grit_0` (renamed `no_limit_cycle`) and `falls_silent_within`.
- [ ] **Step 5: Re-record the goldens.**
  - Every reverb golden that ran at GRIT ≠ 0 is re-recorded, using `tests/common/golden.rs`'s procedure: `fx_golden_test`'s `reverb_ring`, `reverb_ring_max_size` and `reverb_ring_full_grit` (renamed `reverb_ring_smear`, run at SMEAR 1); `reverb_dispatch_test`'s three; `instrument_test`'s `reverb_send_on`.
  - The GRIT-0 cases must keep their `a2abe82f` hashes.
  - List the re-recorded cases in the commit message.
- [ ] **Step 6: Renders for the owner.**
  - Run `tools/ab/render.sh smear`. It renders QUAD on the saw chord and the snare at SMEAR 0, 0.3 and 1, and the T4 reference (`git worktree` at `a2abe82f`, GRIT 0) beside SMEAR 0.
  - Write `target/ab/smear_report.md`, with each render's pitch-deviation spread (`pitch_per_pass_cents`) and the SMEAR-0 versus reference match.
  - Hand it over.
- [ ] **Step 7: Run the suite.**
  - Run: `cargo test -p chimera-core`.
  - Expected: PASS.
- [ ] **Step 8: Bench (unit).**
  - Add **REV QUAD S** to the REVERB screen: QUAD at SMEAR 1, RATE 1, with SIZE still.
  - Run `just flash-bench`, then `just usb bench`, and record REV QUAD and REV QUAD S.
  - Gates:
    - REV QUAD S ≤ REV QUAD;
    - REV QUAD ≤ QUAD_REV + 1 % (estimate: below it, since `Grid` is gone);
    - both ΔCOST ≤ 76 (23 tape).
  - Record flash: the delta since `ae8edcba` should be negative, since `grit.rs` goes.
- [ ] **Step 9: Commit.**

```bash
git rm chimera-core/src/dsp/rev/grit.rs chimera-core/tests/reverb_grit_test.rs
git add chimera-core/src/dsp/rev/parts.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/dsp/ring.rs chimera-core/src/dsp/reverb.rs chimera-core/src/storage/codes.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/tests/common/rev.rs chimera-core/tests/reverb_parts_test.rs chimera-core/tests/reverb_smear_test.rs chimera-core/tests/reverb_test.rs chimera-core/tests/fx_golden_test.rs chimera-core/tests/reverb_dispatch_test.rs chimera-core/tests/instrument_test.rs chimera-core/tests/mixer_page_test.rs chimera-core/tests/reverb_render_test.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/glyph.rs tools/ab/render.sh chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "GRIT out of the reverb, id 5 retired; SMEAR into QUAD: <re-recorded cases>"
```

### Task 6: TYPE, slots, knob maps, panels and pages

> **Scope cut (review of `e932ed5f`, 2026-10-04).** Task 6 ships the slots, codes, `KnobMemory`, `with_type`, REV › MORE, and QUAD's panel, laws and value texts only. Each other TYPE's panel (its words, shorts such as `DIFF`, `DENS` and `PRE`, and `label`'s per-TYPE words) lands with its engine. Each TYPE's laws move to its engine task (8–11), and SYNC (`with_sync`, `fold`, `Division`, the lists) moves to Task 12, each with its tests below. `Tempo` is BPM, free of the sample rate. A TYPE not yet on screen shows QUAD's panel and plays QUAD.

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
  - `ReverbParams` gains `kind`, `sync`, `tone`, `diffuse`, `predelay` and `shape` (SMEAR and RATE came in Task 5);
  - id constants `TYPE = 6` through `SYNC = 13`;
  - `pub fn with_type(self, t: ReverbType, mem: &KnobMemory) -> Self` and `pub fn with_sync(self, s: RevSync, tempo: Tempo) -> Self`;
  - `pub struct KnobMemory { slots: [[u16; 9]; 6] }`:
    - exactly **108 B**;
    - a slot stores `round(p · 65_534)`, and `u16::MAX` in a TYPE's first slot marks it unvisited;
    - `new()` fills it with `u16::MAX`;
    - methods `store(&mut self, p: &ReverbParams)`, `recall(&self, t) -> Option<[f32; 9]>` and `clear(&mut self)`;
  - `pub struct EngineKnobs { … }` (the mapped quantities) and `pub fn knobs(p: &ReverbParams, tempo: Tempo, fold: &mut FoldState) -> EngineKnobs`. This covers every row of the spec's per-TYPE tables, the SMEAR caps and the plate zone.
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
    let want = [(6,"TYPE"),(7,"TONE"),(8,"DIFFUSE"),(9,"PREDELAY"),(10,"SHAPE"),(11,"SMEAR"),(12,"RATE"),(13,"SYNC")];
    for (id, ident) in want { assert_eq!(find_spec(&REVERB_SPECS, ParamId(id)).unwrap().ident, ident); }
    assert!(find_spec(&REVERB_SPECS, ParamId(0)).is_none() && find_spec(&REVERB_SPECS, ParamId(5)).is_none());
    assert_eq!(REVERB_SPECS.len(), 12);
}
#[test] fn type_and_sync_codes_are_frozen() {
    use ReverbType::*;
    for (t, c) in [(Quad,0),(Room,1),(Hall,2),(Gate,3),(Reverse,4),(Bloom,5)] { assert_eq!(t.disk_code(), c); assert_eq!(ReverbType::from_disk_code(c), Some(t)); }
    assert_eq!(ReverbType::from_disk_code(6), None);
    for (s, c) in [(RevSync::Off,0),(RevSync::Time,1),(RevSync::All,2)] { assert_eq!(s.disk_code(), c); }
}
#[test] fn an_old_project_loads_as_quad() {
    // decode a Reverb block carrying ids 1–5 only (grit .3, time .7, damp .3, size .5, mix .5) into FxParams::default(): kind == Quad, time/damp/size/mix set, GRIT ignored, smear == 0, the rest at spec defaults
}
#[test] fn unknown_type_code_and_nan_slots_load_as_safe_quad() {
    // Review Focus 2: id 6 code 9, ids 7–13 NaN: kind == Quad; Reverb renders 1 s of noise send with every sample finite
}
#[test] fn with_type_loads_starting_knobs_and_keeps_mix_and_sync() {
    let mut p = ReverbParams::default(); (p.mix, p.sync) = (0.7, RevSync::Time);
    let h = p.with_type(ReverbType::Hall, &KnobMemory::new());
    assert_eq!((h.size, h.time, h.tone, h.diffuse, h.predelay, h.smear, h.rate, h.damping), (0.65, 0.45, 0.3, 0.4, 0.15, 0.35, 0.4, 0.3));
    assert_eq!((h.mix, h.sync), (0.7, RevSync::Time));
    let q = h.with_type(ReverbType::Quad, &KnobMemory::new()); assert_eq!((q.smear, q.rate), (0.0, 0.5));
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
#[test] fn smear_caps_hold_at_every_size() {
    // for size in 0..=1 step 1/128, smear 1: Room D ≤ 0.0072·L, Hall D ≤ 0.0036·L, Bloom D ≤ 0.0024·L, Quad D ≤ 0.0018·L (L the round trip)
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
    // Main and More labels per the spec, e.g. Hall More == ["PRE-DLY","DAMP","SMEAR","RATE","SYNC",None]; Quad Main == ["SIZE","TIME","DAMP","SMEAR"], Quad More == [None,None,None,"RATE",None,None]; Gate More[0] None
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
  - `pub enum SwitchPhase { Running, FadeOut { left: Left<FADE_BLOCKS> }, Clear { left: Left<CLEAR_BLOCKS> }, FadeIn { left: Left<FADE_BLOCKS> } }`, with `pub const FADE_BLOCKS: u8 = 15`, `pub const CLEAR_BLOCKS: u8 = 4` and `Left<N>` a count bounded to 1..=N;
  - `Reverb::phase(&self) -> SwitchPhase`;
  - `#[doc(hidden)] pub fn force_restart(&mut self)`, which runs the same sequence QUAD → QUAD;
  - `latched: ReverbParams`, taken when a switch starts;
  - in `reverb_switch_test.rs`, `const TYPES: &[ReverbType]`. Tasks 8, 9, 10 and 11 each append their TYPEs to it.

- [ ] **Step 1: Write the failing tests.**
  - `TYPES` starts as `&[Quad]`, and each pair test also runs `force_restart`.

```rust
#[test] fn every_ordered_pair_switches_without_a_click() { /* for a, b in TYPES (and the forced restart): mid-tail at MIX 1, max_d2 over the switch window ≤ max_d2 of the 30 blocks before */ }
#[test] fn the_phases_run_15_4_15() { /* after a TYPE change: 15 blocks FadeOut, 4 Clear, 15 FadeIn, then Running */ }
#[test] fn clear_blocks_return_exact_zero() { /* every L and R sample of each Clear block == 0.0, each zeroes a quarter, and arena().lines() all 0 after the last */ }
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

> **Owns, moved from Task 6:** its panel in `ui::reverb_panel` (and `View::short`'s reverb words where it brings `DENS`); GATE's and REVERSE's rows in a `const` `Ranges` table read by one law function, in `dsp::reverb::knobs`; their `EngineKnobs` fields (`tail`, `regen`, `taps`, `shape_db`, the pre-diffuser's g); their value texts; adding them to `types_on_screen`; their rows of `knobs_hit_the_spec_endpoints`, and their NaN rows in `unknown_type_code_and_nan_slots_load_as_safe_quad`.

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
#[test] fn regen_1_falls_to_exact_zero() { /* Reverse REGEN 1, LENGTH 1: returns exact 0 within 24 s */ }
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

> Done (2026-10-05). Its ROOM structure is replaced by Task 9b (ADR 0095); its LOOP-engine machinery stays.

> **Owns, moved from Task 6:** its panel in `ui::reverb_panel` (and `View::short`'s reverb words where it brings `DENS`); ROOM's `Ranges` row (Task 8's table), `tone_hz`, `diffuse_g`, `predelay_ms` and the SMEAR cap (0.72 % of the loop) in `knobs`; its value texts; `types_on_screen`; its rows of `knobs_hit_the_spec_endpoints` and `smear_caps_hold_at_every_size`. Check ROOM's `t_min` floor against its 0.2 s DECAY.

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
#[test] fn room_smear_is_bounded_without_drift_or_vibrato() { /* check_smear_bound at SMEAR 1, RATE 1, SIZE 0 (L = 0.1 s) */ }
#[test] fn no_slap_after_a_size_sweep() { /* check_no_slap: Room after SIZE 0 → 1 vs a static Room at 1 */ }
#[test] fn room_window_and_rt60() {
    // defaults: rt60_t20 == 0.548 s ± 30 %, onset_ms ≤ 34;
    // DECAY .25, .5, .75 and 1: rt60 within ±35 % of 0.2·7.5^p (floored at t_min); DECAY 0 within ±35 % of 0.2 s (checks the rotation)
}
#[test] fn level_plan_holds_for_room() { /* check_level_plan at DECAY 1, SIZE 0, DIFFUSE 1; plus every stored quantity's recorded peak within its § Level plan bound */ }
#[test] fn room_knobs_ease() { /* check_knobs_ease over ids 1, 3, 7, 8, 9, 11 (SMEAR), 12 */ }
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

> Done on the host (2026-10-05). Its held unit step and the fit to the MidiVerb halls are Task 10b (ADR 0095).

> **Owns, moved from Task 6:** its panel in `ui::reverb_panel` (and `View::short`'s reverb words where it brings `DENS`); HALL's `Ranges` row, its DAMP and SMEAR cap (0.36 %), the plate zone (`plate_pull`, `plate_zone_pulls_diffuse_tone_predelay`), and `PLATE` shown in the SIZE cell's label (not the value, ADR 0085); its value texts; `types_on_screen`.

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
#[test] fn plate_corner_onset_is_short() { /* SIZE 0, DECAY .35, TONE 0, DIFFUSE 1, DAMP .1, PRE 0, SMEAR .5, RATE .6: onset_ms ≤ 8 */ }
#[test] fn plate_zone_moves_predelay_by_crossfade() { /* PRE .5, SIZE .15 → 0: the PRE read length changes only at crossfade starts, by ≤ 120 samples each */ }
#[test] fn hall_decorrelates() { /* defaults, noise burst: |corr(l, r)| ≤ 0.2 */ }
#[test] fn hall_settles_on_its_input_pitch() { /* check_settles_on_pitch: SIZE 0 → 1 then PRE 0 → 1; L = 0.6 s */ }
#[test] fn hall_smear_is_bounded_without_drift_or_vibrato() { /* check_smear_bound at SIZE 0 (L = 0.12 s); 2 lines, D ≤ 0.36 % of L */ }
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

### Task 11: BLOOM

Rewritten for ADR 0095 and moved after Task 12: see § Amendment, 2026-10-05.

### Task 12: Tempo sync on ORBIT's clock

> **Owns, moved from Task 6:** `Division`, `DivKind`, the lists (`ROOM_PRE`, `HALL_PRE`, `GATE_LENGTH`, `REVERSE_LENGTH`, `DECAY_BEATS`), `FoldState`, `fold` (2 % hysteresis), `with_sync` and its nearest-division search, the synced arms of `knobs` and `value_text` (`▾1/8`, beats), the per-division `step`, ROOM's OFF/TIME-only SYNC walk, and their tests (`fold_halves_doubles_and_holds`, `room_sync_steps_off_and_time_only`, the folded mark, `with_sync`'s). `Tempo` is now BPM (`Tempo(120.0)`, `beat_ms`): `Tempo::from_bpm` takes no sample rate, and each engine converts with its own.

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

## Amendment, 2026-10-05: the MidiVerb II reference (ADR 0095)

The MidiVerb II is now the reference for every TYPE, and the tasks below replace Task 9's ROOM structure and Task 11's BLOOM. They run in this order: **9b → 10b → 11 → 11b → 13**. Task 12 is done; Task 11 picks up its deferred BLOOM rows.

**The rule.** A named knob setting must be indistinguishable from the same-named preset. The harness's `verify.py` prints PASS only when all of these hold:
- band RT60 is within ±5 % from 250 Hz to 4 kHz;
- pre-delay is within ±1 ms;
- C50 is within ±1 dB;
- NED is within ±10 % at 20, 50 and 100 ms;
- the centroid is within ±5 %.

A preset that still fails after its task's near-miss steps goes to the owner with its residuals, and passes only by the owner's signed-off exception.

**The harness** is outside the repository: `$S/midiverb-ref/`, where `S=/tmp/scratch-1000/-HOME-user-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad`. It renders the *Rust* `FxBus` through the ignored test `zz_mvref.rs::mvref_batch`, which lives untracked in the `$S/wt-reverb-ref` worktree. Its job line already sets every slot: `size time tone diffuse predelay smear rate shape damping` (slots 3, 1, 7, 8, 9, 11, 12, 10, 2). Before each fit, point that worktree at the task's commit:

```bash
git -C $S/wt-reverb-ref checkout --detach <task HEAD>   # zz_mvref.rs stays, untracked
cd $S/midiverb-ref
rm -f fits/<type>_sweep.json                            # the sweep was of the old engine
./run.sh <TYPE> <programs>                              # sweep, fit, verify, A/B renders
$S/mvenv/bin/python verify.py <TYPE>                    # re-verify after any change: PASS/FAIL per preset
```

`fit.py`'s `DIMS` lists the knobs it fits for each TYPE. Nothing from the harness, the emulator or their outputs is committed. Fitted knobs, verdicts and residuals go into § Measured, and each passing preset's knobs become a golden.

**Tests that bite.** Every task here ends with a hand mutation check:
1. Apply each listed mutation to the engine.
2. Run the task's tests, and see at least one fail.
3. Revert.

Record which test catches each mutation in the report. A mutation that no test catches gets a test.

### Task 9b: ROOM v2 (unit check)

> **ADR 0095.** ROOM becomes 4 loops × 2 stages, with a 5-AP diffuser, reflection-coupled pairs, 8 taps a side and the new DAMP and LOOPS knobs. It keeps Task 9's machinery: the crossfade scheduler (one stage delay a loop, ≤ 5 ms, every loop in the same crossfade), `store_settle` re-entries, f32 stage allpasses, unrounded dampers, SMEAR's in-engine bound and the level plan.

**Files:**
- Modify:
  - `chimera-core/src/dsp/rev/loop_engine.rs`. ROOM becomes `LoopEngine<4, 2>` with the pairs' reflection and moving taps. It moves to its own `rev/room.rs` only if sharing would change HALL: the `rev_hall` golden must stay bit-identical;
  - `chimera-core/src/dsp/reverb.rs`: ROOM's `Ranges` (RT60 0.2–2.0 s, `damp_hz` 1.5–10 kHz, `diffuse_g` 0.45–0.70, θ 0–90° on SHAPE's slot) and ROOM's `START`;
  - `chimera-core/src/ui/reverb_panel.rs`: ROOM's MORE page becomes PRE-DLY · DAMP · SMEAR · RATE · SYNC · LOOPS, with LOOPS in degrees;
  - `chimera-core/tests/reverb_loop_test.rs`, `reverb_panel_test.rs` (the per-TYPE cell rows), `fx_golden_test.rs`;
  - plan § Measured.
- Outside the repo: `$S/midiverb-ref/fit.py`, whose `DIMS["ROOM"]` gains `"damping", "shape"`.

**The structure** (ADR 0095; ms at 24 kHz, rounded to the nearest sample):
- diffuser APs, g 0.75: 0.81, 1.92, 3.59, 4.91 and 6.24 ms;
- stage delays at SIZE 1: A 37.1/57.5, B 59.4/42.3, C 52.7/41.9 and D 41.5/43.6 ms, × k with k = 0.4 + 0.6·SIZE;
- stage APs, fixed: A 29.0/13.3, B 28.9/18.4, C 30.4/16.3 and D 27.1/14.2 ms;
- one one-pole per loop end;
- pairs (A, B) and (C, D) re-enter through [[cos θ, sin θ], [sin θ, −cos θ]];
- the send enters every loop at +1;
- taps × k, weight ¾. R: 0.51 (fixed), 31.4, 10.8, 20.4, 15.8, 21.3, 30.9 and 9.1 ms. L: 5.5, 5.6, 36.7, 11.8, 24.1, 8.9, 18.0 and 38.1 ms. Tap j of a side reads stage line j: A1, A2, B1, B2, C1, C2, D1, D2;
- SMEAR moves one stage AP a loop (A2, B1, C2 and D1), each with its own wander. Each is capped at 1.45 % of its loop's trip at θ 0, the shortest trip.

- [ ] **Step 1: Write the failing tests** in `reverb_loop_test.rs`. They replace `room_layout_fits_and_lengths_are_distinct_primes`, `rotation_preserves_energy` and `room_window_and_rt60`.

```rust
#[test] fn room_layout_matches_adr_0095() { /* every diffuser, stage-delay (at k 0.4 and 1) and stage-AP length == round(ms × 24) of ADR 0095's tables; delays distinct; every tap ≤ its line at k 0.4 and 1; layout incl. f32 slots ≤ RING_LEN (about 19.6 k) */ }
#[test] fn reflection_preserves_energy_at_every_loops() { /* gains 1, dampers open: pair energy constant to 1e-6 over 10 round trips at θ 0°, 30°, 60°, 90°; H·H == I to 1e-7 */ }
#[test] fn loops_0_keeps_four_loops_and_90_makes_two() { /* send into A's line only, diffuser bypassed: θ 0 → B's line stays exactly 0; θ 90° → A's output reaches only B, so A's line is silent on every second trip */ }
#[test] fn taps_move_with_their_lines() { /* SIZE 0 → 1: each tap's read changes only in its line's crossfade, by ≤ 120 samples; R's first tap stays at 12 samples (0.51 ms) */ }
#[test] fn room_first_arrival_is_under_a_millisecond() { /* PRE-DLY 0, click: first sample within −20 dB of the peak ≤ 1.0 ms (as built: 2.9 ms) */ }
#[test] fn room_rt60_tracks_decay_to_2_s() { /* 1 kHz octave, interrupted noise: DECAY 0, .5, 1 → 0.2, 0.63, 2.0 s ± 10 %; no stage gain above 0.985 */ }
#[test] fn damp_darkens_the_loop_and_tone_the_input() { /* DAMP 1.5 vs 10 kHz: the 4 kHz RT60 falls ≥ 30 %, 500 Hz within 5 %; TONE 10 → 2.5 kHz moves the centroid down with the 1 kHz RT60 within 5 % */ }
```

  Task 9's other ROOM tests stay, adapted:
  - `round_trip_moves_at_most_5_ms_per_crossfade` covers 8 delays;
  - `room_knobs_ease` covers ids 1, 2, 3, 7, 8, 9, 10, 11 and 12;
  - `level_plan_holds_for_room` runs at DECAY 1, SIZE 0, DIFFUSE 1, at both LOOPS 0 and 1;
  - `room_noise_and_loudness` sets `WET_ROOM` again;
  - unchanged: `room_settles_on_its_input_pitch`, `room_smear_reads_stay_within_the_cap`, `room_smear_does_not_move_the_tails_peak`, `no_slap_after_a_size_sweep`, `room_falls_silent` and `room_tail_draws_its_predelay_gap`.

  In `reverb_panel_test.rs`, `every_type_shows_its_cells` and `labels_and_values_fit_their_cells` gain DAMP and LOOPS under ROOM.
- [ ] **Step 2: Run the tests and see them fail.** Run: `cargo test -p chimera-core --test reverb_loop_test`. Expected: FAIL on the new tests.
- [ ] **Step 3: Implement** to ADR 0095 § ROOM v2.
- [ ] **Step 4: Run the tests and see them pass.** Run: `cargo test -p chimera-core --test reverb_loop_test --test reverb_switch_test --test reverb_panel_test --test reverb_sync_test`. Expected: PASS.
- [ ] **Step 5: Fit the nine ROOM presets on the Rust render.**
  - Run: `./run.sh ROOM 1 2 3 5 8 9 10 11 14`, then `verify.py ROOM`.
  - The float model's fits (`fits/room_v2_best.json`) are the seeds. Its stage gain and loop pole map to DECAY and DAMP through the Rust laws, and are never copied:

    | # | Preset | SIZE | θ | stage gain | loop pole | PRE (ms) | stage AP g | float verdict |
    |---|---|---|---|---|---|---|---|---|
    | 1 | Small Bright .1 | 0.357 | 0° | 0.239 | 0.569 | 0.94 | 0.457 | PASS |
    | 2 | Small Bright .2 | 0.862 | 0° | 0.456 | 0.474 | 5.04 | ½ | PASS |
    | 3 | Small Bright .3 | 0.937 | 8.8° | 0.471 | 0.424 | 4.58 | ½ | PASS |
    | 5 | Medium Bright .6 | 0.631 | 90° | 0.637 | 0.263 | 4.76 | ½ | PASS |
    | 8 | Medium Dark .6 | 0.629 | 90° | 0.628 | 0.256 | 5.12 | ½ | PASS |
    | 9 | Medium Dark .5 | 0.630 | 90° | 0.578 | 0.149 | 4.34 | 0.691 | C50 −1.04 dB |
    | 10 | Medium Bright .6 | 0.779 | 90° | 0.631 | 0.143 | 4.55 | 0.575 | 4 kHz RT −9.8 %, NED 20 ms −15 % |
    | 11 | Medium Bright .8 | 0.737 | 86.6° | 0.666 | 0.268 | 3.80 | 0.543 | PASS |
    | 14 | Medium Dark 1.0 | 0.751 | 52.6° | 0.742 | 0.149 | 5.18 | 0.511 | 4 kHz RT −9.9 %, C50 −1.41 dB |

  - The Dark presets (8, 9 and 14) put TONE at about 2.6 kHz, the unit's input one-pole.
  - **Pass:** at least #1, 2, 3, 5, 8 and 11 PASS, as they do in the float model.
- [ ] **Step 6: The near misses** (#9, #10 and #14), in order, re-verifying all nine after each, since each moves the others:
  1. DAMP's top end opens the damper fully (a bypass, not 10 kHz). Refit #10 and #14.
  2. The tap weight follows θ: ¾ at 0°, rising to ¾ × 9/8 at 90°. Refit #9, #10 and #14.

  Keep whichever step passes more presets without losing one. If any still fail, they go to the owner with their residuals.
- [ ] **Step 7: Starting knobs and goldens.**
  - ROOM's `START` becomes #5's fitted knobs (Medium Bright .6), with SMEAR 0.3 kept. The owner confirms at UAT.
  - `fx_golden_test` re-records `rev_room` and adds `rev_room_mv<NN>`, one per passing preset at its fitted knobs, from a `MV2_ROOM` table of (program, knobs).
- [ ] **Step 8: Mutation check.** Each must fail a test:
  - A1's delay +1 sample;
  - B2's AP swapped with C2's;
  - the reflection's −cos → +cos;
  - θ ignored (held at 0);
  - L's tap 3 dropped;
  - tap weight ¾ → ½;
  - the damper ignoring DAMP;
  - the diffuser cut to 4 APs;
  - DECAY's top back to 1.5 s;
  - a tap left fixed through a SIZE move.
- [ ] **Step 9: Bench and flash (unit).**
  - Flash the bench build, and record REV ROOM, STRESS with ROOM and REV SWITCH MAX (QUAD → GATE → ROOM → HALL), on both builds.
  - **Gates:** ΔCOST ≤ 76 (default) and ≤ 23 (tape). ADR 0095 estimates REV ROOM at about 34 and 82, ΔCOST −21 and +11. STRESS must stay under budget as it was before (#355 is not this task).
  - Record the flash delta over `ba2d1acf` in one line.
  - Over the tape gate goes to the owner, before anything is cut.
- [ ] **Step 10: Provenance report.** Every ROOM table against ADR 0095's tables, with no other number from the emulator, and the fitted knobs shown fitted, not copied.
- [ ] **Step 11: Commit.**

```bash
git add chimera-core/src/dsp/rev/loop_engine.rs chimera-core/src/dsp/reverb.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_loop_test.rs chimera-core/tests/reverb_panel_test.rs chimera-core/tests/fx_golden_test.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "ROOM v2: four loops, DAMP and LOOPS, fitted to the MidiVerb II rooms"
```

### Task 10b: HALL fitted to the Large and Xlarge presets (unit check)

> **ADR 0095.** HALL is judged against programs 4, 6, 7, 12, 13, 15–29: twenty presets. Its design (ADR 0085, "HALL, as built") stands until this task's investigation shows a change is needed. Task 10's held unit step runs here.

**Files:** `chimera-core/src/dsp/rev/loop_engine.rs` (and `reverb.rs`, `reverb_panel.rs`) only if Step 3 changes the structure; `reverb_loop_test.rs`, `fx_golden_test.rs`, ADR 0095 (§ HALL, as fitted), plan § Measured.

- [ ] **Step 1: Baseline.**
  - Run: `./run.sh HALL 4 6 7 12 13 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29`, then `verify.py HALL`.
  - Record each preset's verdict and residuals in § Measured.
- [ ] **Step 2: Investigate before changing anything.** For each preset family that fails, read its program's structure (`$S/mvenv/bin/python structure.py <N>`). Compare it with HALL's 1 loop × 4 stages, 4-AP diffuser and 3 taps a side. Answer each question with numbers:
  - **Loops and stages.** Are the Large programs 1 × 4, or 4 × 2 / 2 × 4 like ROOM v2? Does LOOPS belong on HALL too?
  - **Taps.** What tap count and C50 do the presets need? ROOM as built ran 0.3–3.2 dB short with 3 taps a side.
  - **DECAY's top.** #29 measures 14.9 s at 250 Hz, past HALL's 10 s. #28 measures 7.6 s.
  - **Tilt.** #28 and #29 fall 1.9× and 2.4× from 250 Hz to 4 kHz (7.63 → 4.11 s, 14.93 → 6.14 s). Can DAMP's range reach that?
  - **Pre-delay.** The presets use 1.0–73.6 ms, inside 0–125 ms; check the fits land.
  - **The plate zone.** It has no MidiVerb counterpart. It must survive any change, with `plate_corner_onset_is_short` holding.
- [ ] **Step 3: Decide.**
  - If the structure must change, build a float model first, as `room_v2.py` did (`model.py`, `configs.py`), and show its pass count.
  - Write the proposal into ADR 0095 as "HALL, as fitted", and take it to the owner before any code.
  - With no structural change, go to Step 6.
- [ ] **Step 4: Write the failing tests** for the approved structure, in the pattern of Task 9b's: `hall_layout_matches_adr_0095`, plus energy, coupling and tap tests for whatever changed. `hall_reaches_10_s_at_size_0` moves to DECAY's new top if it changes. Run `cargo test -p chimera-core --test reverb_loop_test`, and see them fail.
- [ ] **Step 5: Implement**, then run `--test reverb_loop_test --test reverb_switch_test --test reverb_panel_test --test reverb_sync_test`. Expected: PASS, with `rev_room` unchanged unless the change is shared.
- [ ] **Step 6: Fit and verify.** Run `./run.sh HALL …` (the programs of Step 1), then `verify.py HALL`. **Pass:** every HALL preset PASSes or is owner-signed. Near misses try DAMP and tap weight first, as Task 9b's Step 6 does.
- [ ] **Step 7: Goldens.** `rev_hall` and `rev_hall_plate` are re-recorded if the sound moved, and `rev_hall_mv<NN>` is added for each passing preset (`MV2_HALL`).
- [ ] **Step 8: Mutation check:** one stage delay +1 sample; a tap dropped; the damper ignoring DAMP; DECAY's top lowered; the plate pull disabled.
- [ ] **Step 9: Bench and flash (unit)**, Task 10's held step:
  - record REV HALL, STRESS with HALL and REV SWITCH MAX (QUAD → GATE → ROOM → HALL) on both builds;
  - gates: ΔCOST ≤ 76 / 23;
  - record the flash delta in one line.
- [ ] **Step 10: Provenance report and commit:** `git commit -m "HALL fitted to the MidiVerb II halls"`, staging only the files changed.

### Task 11: BLOOM (unit check), rewritten for ADR 0095

> **Owns**, moved from Task 6:
> - its panel, SWELL · DECAY · TONE · DIFFUSE, with MORE: PRE-DLY · — · SMEAR · RATE · SYNC · —;
> - BLOOM's `Ranges` row: SWELL × 0.5–1.2 on the cascade, DECAY RT60 2–12 s, TONE 10 → 1.5 kHz on both one-poles, DIFFUSE as the cascade's g at 0.45–0.65, PRE-DLY 0–125 ms, SMEAR capped at 0.24 % of the loop, RATE 0.05–0.5 Hz;
> - its value texts and `types_on_screen`;
> - the Bloom arms of `ranges()` and `panel()`, which today play QUAD's.
>
> **Owns, deferred from Task 12:**
> - `synced()`'s Bloom arm: `(P::PREDELAY, HALL_PRE)`, and `beats` true under ALL;
> - `beats_s` clamping BLOOM's DECAY in beats to 2–12 s;
> - BLOOM's SYNC cell, with all three of OFF, TIME and ALL.

**The structure** (ADR 0095):
- an input one-pole, then PRE-DLY;
- 15 series unity allpasses at g ½, 0.56–89 ms and 580 ms in all, with no plain delay;
- feedback from the cascade's end through a one-pole and a gain;
- outputs from the cascade's end, through 3-AP decorrelators: L 10.6/2.1/0.9 ms and R 6.3/1.8/2.4 ms, stored f32.

The cascade stores normalised i16 states, since f32 needs about 29 k slots. Its re-entry and states settle as ROOM's do (`store_settle`). SMEAR moves the three longest cascade APs, each with its own wander. The arena takes about 14.4 k slots at × 1.031 and 16.7 k at × 1.2, plus 3 k for PRE-DLY and about 0.5 k f32: about 20.7 k at most. This replaces the spec's ten APs plus SWELL delay, and the split-SWELL fallback is dropped.

**Files:**
- Create: `chimera-core/src/dsp/rev/bloom.rs`, `chimera-core/tests/reverb_bloom_test.rs`.
- Modify:
  - `chimera-core/src/dsp/rev/mod.rs` (`EngineState::Bloom`);
  - `chimera-core/src/dsp/reverb.rs` (the Bloom row, `ranges`, `synced`, `beats_s`, `START`);
  - `chimera-core/src/ui/reverb_panel.rs`;
  - `chimera-core/tests/reverb_switch_test.rs`, `reverb_sync_test.rs`, `reverb_panel_test.rs`, `fx_golden_test.rs`;
  - `chimera-stm32/src/bench.rs` (REV BLOOM, and BLOOM in STRESS and REV SWITCH MAX);
  - plan § Measured.
- Outside the repo: `fit.py`'s `DIMS["BLOOM"]` becomes `["size", "time", "tone", "diffuse", "predelay"]`, since BLOOM has no DAMP.

- [ ] **Step 1: Write the failing tests.**

```rust
// reverb_bloom_test.rs
#[test] fn bloom_layout_matches_adr_0095() { /* 15 cascade lengths == round(ms × 24 × swell) at × 0.5, 1.031 and 1.2, distinct, sum 580 ms × swell ± 1 sample a line; decorrelators L 10.6/2.1/0.9, R 6.3/1.8/2.4 ms; layout at × 1.2 with PRE 125 ms ≤ RING_LEN */ }
#[test] fn cascade_is_flat_with_the_loop_open() { /* feedback 0, one-poles open: |H| flat within 0.1 dB, 50 Hz–10 kHz */ }
#[test] fn swell_scales_the_cascade_by_crossfade() { /* SWELL 0 → 1: only cascade lengths and PRE change, at crossfade starts, the cascade's total by ≤ 120 samples a crossfade; decorrelators never change */ }
#[test] fn bloom_settles_on_its_input_pitch() { /* check_settles_on_pitch: SWELL 0 → 1; L = 0.7 s */ }
#[test] fn bloom_smear_is_bounded_without_drift_or_vibrato() { /* check_smear_bound at SWELL 0 (L = 0.29 s); the three longest APs, each its own Wander; D ≤ 0.24 % of L */ }
#[test] fn bloom_rt60_tracks_decay() { /* 1 kHz octave, interrupted noise: DECAY 0, .5, 1 → 2, 4.9, 12 s ± 10 % */ }
#[test] fn bloom_falls_silent() { /* DECAY 1: exact 0 within 2 × RT60 + 1 s (the i16 cascade states: Task 10's limit cycle) */ }
#[test] fn level_plan_holds_for_bloom() { /* check_level_plan at DECAY 1, SWELL 0, DIFFUSE 1 */ }
#[test] fn bloom_knobs_ease() { /* check_knobs_ease over ids 1, 3, 7, 8, 9, 11, 12 */ }
#[test] fn bloom_noise_and_loudness() { /* check_noise_and_loudness(bloom, quad, 4.0); 6.0 at DIFFUSE 1; sets WET_BLOOM */ }

// reverb_sync_test.rs
#[test] fn bloom_syncs_predelay_and_decay_in_beats() {
    // 120 BPM, SYNC TIME: PRE-DLY walks HALL_PRE, folded into 0–125 ms; DECAY stays in seconds
    // SYNC ALL: 32 beats (16 s) → 12 s; 4 beats → 2 s; 1 beat at 300 BPM (0.2 s) → 2 s
}
```

  - `every_type_shows_its_cells`, `labels_and_values_fit_their_cells`, `every_ordered_pair_switches_without_a_click` and `decay_in_beats_clamps_and_room_has_no_all` gain BLOOM.
- [ ] **Step 2: Run the tests and see them fail.** Run: `cargo test -p chimera-core --test reverb_bloom_test --test reverb_sync_test`. Expected: FAIL.
- [ ] **Step 3: Implement** to ADR 0095 § BLOOM.
- [ ] **Step 4: Run the tests and see them pass.** Run: `cargo test -p chimera-core --test reverb_bloom_test --test reverb_switch_test --test reverb_sync_test --test reverb_panel_test`. Expected: PASS.
- [ ] **Step 5: Fit Bloom 1 and Bloom 2.**
  - Run: `./run.sh BLOOM 45 49`, then `verify.py BLOOM`.
  - The seeds are the float model's (`fits/bloom_model_final.json`): cascade × 1.031, feedback 0.662, both one-poles about 3.1 kHz, PRE-DLY 2.3 ms (#45) and 87.8 ms (#49). Only PRE-DLY differs between the two.
  - **Pass:** both PASS.
  - `START` becomes #45's knobs, with SMEAR 0.3 kept, and the owner confirms at UAT.
- [ ] **Step 6: Goldens.** Add `rev_bloom`, `rev_bloom_mv45` and `rev_bloom_mv49`, then run `--test fx_golden_test`. Expected: PASS.
- [ ] **Step 7: Mutation check:**
  - one cascade AP +1 sample;
  - a cascade AP removed;
  - a plain delay put back in the loop;
  - g ½ → 0.6 fixed (ignoring DIFFUSE);
  - TONE driving only one of the one-poles;
  - SWELL scaling all but one AP;
  - a decorrelator AP dropped;
  - `synced()` returning no Bloom arm;
  - `beats_s` unclamped.
- [ ] **Step 8: Bench and flash (unit).**
  - Record REV BLOOM, STRESS with BLOOM and REV SWITCH MAX through all six, on both builds.
  - Gates: ΔCOST ≤ 76 / 23. ADR 0095 expects about 5 more allpass updates a sample than ADR 0085's +14.
  - Record the flash delta in one line.
- [ ] **Step 9: Provenance report and commit.**

```bash
git add chimera-core/src/dsp/rev/bloom.rs chimera-core/src/dsp/rev/mod.rs chimera-core/src/dsp/reverb.rs chimera-core/src/ui/reverb_panel.rs chimera-core/tests/reverb_bloom_test.rs chimera-core/tests/reverb_switch_test.rs chimera-core/tests/reverb_sync_test.rs chimera-core/tests/reverb_panel_test.rs chimera-core/tests/fx_golden_test.rs chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "BLOOM: the MidiVerb's allpass cascade, Bloom 1 and 2 matched; synced"
```

### Task 11b: GATE and REVERSE fitted (unit check)

> **ADR 0095. Scope.** Task 8's BURST engine is fitted to programs 30–39 (GATE) and 40–44, 46, 47 and 48 (REVERSE). GATE is expected to need knob and range changes. REVERSE may need a structural change: the unit's silent onset and its end hit are not in BURST. As with HALL, any structural change is investigated, proposed in ADR 0095, and approved by the owner before any code. The harness also reports burst metrics (the −20 dB cut, body slope and peak time). They are reported beside the verdict, and their tolerances are proposed in Step 2 for the owner to set.

- [ ] **Step 1: Baseline.**
  - Run: `./run.sh GATE 30 31 32 33 34 35 36 37 38 39` and `./run.sh REVERSE 40 41 42 43 44 46 47 48`, then `verify.py` for each.
  - Record the verdicts.
- [ ] **Step 2: Investigate** with `structure.py <N>` and the recon (`fits/recon.json`):
  - **GATE.** Today's default is nearest Fast Gate 150 (#35) on cut and slope, but:
    - it has no pre-delay, while the presets measure 6.9–71 ms. Does GATE take PRE-DLY on slot 9, its empty MORE cell?
    - it is twice as dense early. Is that DENSITY's tap count or the pre-diffuser g?
    - it is 25 % brighter. Is that TONE's range, or the unit's two-sample average?
  - **REVERSE.** It is far off. The unit:
    - is silent for about 60 ms (an onset delay, or zero-weight first taps);
    - rises 45 dB over 400 ms, against CURVE's 12–30 dB;
    - fires a loud delayed copy at 483 ms (an end-hit tap above the ramp);
    - then cuts hard.

    Regen 2 s and 8 s (#47 and #48) measure 2.4 and 10.9 s.
  - Write the findings and any proposed structure into ADR 0095 ("GATE and REVERSE, as fitted"), then go to the owner.
- [ ] **Step 3: Write the failing tests** for the approved changes. They are provisional until Step 2's ruling:
  - `gate_predelay_reaches_71_ms`;
  - `reverse_is_silent_before_its_onset` (the first −40 dB arrival at ≥ 55 ms at the #40 fit);
  - `reverse_ends_on_its_hit` (the envelope's peak in the last 10 % of LENGTH, then −20 dB within 20 ms);
  - `curve_reaches_45_db`.

  Task 8's tests are kept wherever they still hold. `weights_keep_sum_of_squares` and `reverse_window` change with any end hit, and each change is listed with its reason. Run `cargo test -p chimera-core --test reverb_burst_test`, and see them fail.
- [ ] **Step 4: Implement**, then run `--test reverb_burst_test --test reverb_switch_test --test reverb_panel_test --test reverb_sync_test`. Expected: PASS.
- [ ] **Step 5: Fit and verify** both TYPEs, with Step 1's commands. **Pass:** every preset PASSes or is owner-signed.
- [ ] **Step 6: Goldens.** Re-record `rev_gate`, `rev_reverse` and `rev_gate_sync_8th_120` if they moved, and add `rev_gate_mv<NN>` and `rev_reverse_mv<NN>` per passing preset.
- [ ] **Step 7: Mutation check:**
  - a tap's offset +1 sample;
  - PRE-DLY ignored;
  - the onset delay removed;
  - the end hit at the ramp's weight;
  - CURVE's top back to 30 dB;
  - REGEN's write rounding instead of truncating.
- [ ] **Step 8: Bench and flash (unit).**
  - Record REV GATE, REV RVRS and STRESS with each, on both builds.
  - Gates: ΔCOST ≤ 76 / 23.
  - Record the flash delta in one line.
- [ ] **Step 9: Provenance report and commit:** `git commit -m "GATE and REVERSE fitted to the MidiVerb II gates and reverses"`, staging only the files changed.

### Task 13: Sweeps, screens, A/B, final bench and the ADRs (unit)

> Runs last, after Tasks 9b, 10b, 11 and 11b (§ Amendment, 2026-10-05).

**Files:**
- Modify:
  - `chimera-core/tests/param_sweep_test.rs` and `chimera-core/tests/common/sweep.rs`;
  - `chimera-core/tests/screen_golden_test.rs`, with images under `docs/screens/`;
  - `chimera-core/src/dsp/fx_bus.rs` (`COST`) and `chimera-core/tests/cost_test.rs` (the `bus` literal, both arms: `if TAPE { … } else { … }`);
  - `chimera-core/src/orbit/runner.rs` (`OrbitRunner::COST`);
  - `chimera-stm32/src/bench.rs` (the SYNC rows);
  - `tools/ab/render.sh`;
  - `docs/adr/0085-the-master-reverb-picks-a-type.md` and `docs/adr/0095-every-reverb-type-matches-its-midiverb-ii-presets.md`;
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
  - Render every TYPE on the saw chord and the snare at SMEAR 0, at its default and at 1, with `tools/ab/render.sh smear-all`, for the owner's SMEAR on/off judgement.
  - Write `target/ab/report.md`, with RT60 (T20), onset, peak, −20 dB end, tilt and ρ for each pair, measured with `tests/common/rev.rs`'s measures, and a VST column left blank.
  - For every TYPE with a MidiVerb counterpart, ADR 0095's harness verdicts replace the ±30 % pass. Re-run `verify.py` for ROOM, HALL, GATE, REVERSE and BLOOM on this task's HEAD (the harness § Amendment shows how), and put the per-preset table into the report. The VST pairs remain only for QUAD, HALL's plate corner and SMEAR on/off.
  - Hand it to the owner.
- [ ] **Step 4: Final bench (unit).**
  - On both builds, record:
    - every REV row;
    - BUS+V with the costliest TYPE;
    - STRESS with each TYPE;
    - REV SWITCH MAX, stepping through all six TYPEs.
  - If the costliest BUS+V reading exceeds `FxBus::COST`, set `COST` to it and update both arms of `cost_test`'s `bus` literal. Then run `cargo test -p chimera-core --test cost_test` and `cargo test -p chimera-core --features master-tape --test cost_test`. Expected: PASS.
  - Record the flash total (`rust-size` `text`).
- [ ] **Step 4b: Every TYPE's on-unit cost rows.** On both builds, record:
  - REV QUAD, REV QUAD S, REV ROOM, REV HALL, REV GATE, REV RVRS and REV BLOOM;
  - STRESS with each;
  - REV SWITCH MAX, QUAD → GATE → ROOM → HALL → REVERSE → BLOOM.

  Record them in one table in § Measured, each with its ΔCOST against REV QUAD. Gate: ΔCOST ≤ 76 (default) and ≤ 23 (tape) for every TYPE.
- [ ] **Step 4c: The SYNC bench.**
  - Add two rows at **97.25 BPM**, a tempo on the 0.25 grid whose beat is no whole number of samples (29,614.4 at 48 kHz):
    - **REV GATE SYNC:** GATE, SYNC TIME, LENGTH at 1/8, with the tempo stepping 97.25 ↔ 97.5 every 2 s so that folds and crossfades run;
    - **REV HALL ALL:** HALL, SYNC ALL, PRE-DLY at 1/16, DECAY at 8 beats, with the same tempo steps.
  - Each row's ΔCOST against REV QUAD fits 76 / 23.
- [ ] **Step 4d: Re-measure `OrbitRunner::COST`.**
  - Task 12 put the reverb tempo into `OrbitRunner::block` (`set_tempo`, the rev clock).
  - Read ORBIT PHASES on the unit over five runs, STRESS keys first, as `COST`'s comment describes, with a synced TYPE playing.
  - If the worst block less its notes' handling, / 64 and rounded up, exceeds today's 171, raise `COST` and its comment.
  - Then run `cargo test -p chimera-core --test cost_test` and `cargo test -p chimera-core --features master-tape --test cost_test`. Expected: PASS.
- [ ] **Step 5: The ADRs.**
  - Write the measured CPU, flash and headroom into ADR 0085 § Consequences, and each preset's verdict and every TYPE's cost row into ADR 0095 § Consequences.
  - Both stay Proposed until the owner's A/B and UAT.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/tests/param_sweep_test.rs chimera-core/tests/common/sweep.rs chimera-core/tests/screen_golden_test.rs docs/screens chimera-core/src/dsp/fx_bus.rs chimera-core/tests/cost_test.rs chimera-core/src/orbit/runner.rs chimera-stm32/src/bench.rs tools/ab/render.sh docs/adr/0085-the-master-reverb-picks-a-type.md docs/adr/0095-every-reverb-type-matches-its-midiverb-ii-presets.md docs/superpowers/plans/2026-10-04-reverb-types.md
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

### Task 3: worst-case kernels (2026-10-04, rev V at 480 MHz, unit)

Bench at `3f73921b` plus `bench_rev` (this task's commit), builds
`bench,orbit` ("default") and `bench,orbit,master-tape` ("tape"). Each K
row is one kernel's ring block timed alone, in cycles per output sample,
with six A16+17 voices rendered untimed between blocks (BUS+V's cache
traffic). K QUAD is QUAD's crossfading ring (`QuadState`, `worst_reverb`'s
settings) in the same harness: ΔCOST = K row − K QUAD, and the row in
BUS+V's frame is QUAD_BUS_V + ΔCOST, so ΔCOST = that row − QUAD_BUS_V.

| Row | default | tape | ΔCOST default / tape |
|---|---|---|---|
| K QUAD (reference) | 304 | 303 | — |
| K ROOM | 287 | 288 | **−17 / −15** |
| K HALL | 278 | 280 | **−26 / −23** |
| K BURST | 241 | 242 | **−63 / −61** |
| K PAIR (16 moving pairs) | 265 | 265 | — |
| K FIX (32 fixed reads) | 132 | 132 | — |
| K CONV (converter stubs alone) | 33 | 33 | — |

- **Pair cost:** (K PAIR − K FIX) / 16 = **8.3 cycles per output sample**
  a pair over its two fixed reads, both builds: about +4.5 instructions in
  the § CPU budget model (1.86 cycles per instruction per ring sample),
  under both the +7 and the +10 estimate.
- **The kernels carry GRIT's converter stubs** (3 quantisations, 3
  one-poles, 2 xorshifts: K CONV, 33), as the brief bills them; GRIT has
  since left, so the engines will read about 33 under these. Without them
  ROOM is about −50.
- **Gate:** every kernel's ΔCOST ≤ 0, inside 76 (default) and 23 (tape).
- **ROOM-T: not needed.** K ROOM reads 15 under K QUAD on the tape build
  (gate ≤ 23), with the stubs in. Task 9 builds ROOM as specified: both
  stage APs smeared, 3 output taps a side. BURST keeps 4 crossfading
  taps, HALL two smeared APs.
- **Kernel shapes** (`chimera-stm32/src/bench_rev.rs`): ROOM, PRE-DLY and
  4 stage delays always crossfading (720 samples, 5 ms round-trip steps),
  A2's and B1's stage APs smeared, A1's and B2's fixed, all normalised;
  4-AP f32 diffuser, rotation, 2 soft clips, 6 fixed taps. HALL, 1 loop ×
  4 stages, APs 1 and 3 smeared, 1 soft clip. BURST, 12 taps (tap 0 the
  register, tap 11 REGEN's read too, taps 8–11 crossfading every 128),
  REGEN's one-pole into a clipped truncating write, TAIL's delay and
  normalised AP with its clip, a 2-AP f32 pre-diffuser, 2 f32
  decorrelator APs a side with each side's second smeared (the spec's 2
  moving pairs). Lengths are the spec's at SIZE 1 / LENGTH 800 ms.

### Unit session readings (2026-10-04, same flashes as Task 3)

| Row | default (`bench,orbit`) | tape (`bench,orbit,master-tape`) |
|---|---|---|
| MIX | 793 | 794 |
| REV QUAD | 58 | 55 |
| REV QUAD S | 51 | 51 |
| BUS | 1039 | 1356 |
| BUS+V | 1084 | 1395 |
| STRESS (max/budget) | 529,497 / 448,000: over | 489,640 / 448,000: over |
| REV SWITCH MAX (max/budget) | **414,093 / 448,000** | **435,137 / 448,000** |

- **BUS+V fell about 100** against Task 1's ORBIT column (1186 → 1084;
  MIX 871 → 793): GRIT's removal and the dispatcher. `FxBus::COST` (1188)
  now bills above the reading; it is left as is here.
- **STRESS is over with ORBIT** on both builds, as Task 1 found (536,475
  then); not this task's.

### Task 4: the dispatcher (2026-10-04, host)

- **T4 golden reference: `a2abe82f`.** QUAD through `Reverb` hashes as
  `RingReverb` did at `695a6ef3`: `reverb_dispatch_test`'s three goldens
  (a 48 kHz and a 44.1 kHz sweep of every control with SIZE crossfades,
  MIX ramps and a saturating burst; the whole bus with REV SEND) and
  every `fx_golden_test` case, unchanged.
- **Flash:** plain release `text` 701,012 B (+240 over `695a6ef3`'s
  700,772); `--features orbit` 720,596 B (+352 over 720,244).
- **REV QUAD bench** (unit, 2026-10-04, at `3f73921b` + Task 3): 58 on
  `bench,orbit` against Task 1's ORBIT 61, gate ≤ 61.6: **passes**; 55 on
  `bench,orbit,master-tape` (Task 1's tape, no ORBIT, read 57).

### Task 5, first build: the converters (2026-10-04, host). Superseded the same day: GRIT leaves the reverb; this code is on `converters-wip`

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

### Task 5: GRIT out, SMEAR into QUAD (2026-10-04, host)

- **SMEAR 0 is the T4 reference at GRIT 0, bit for bit:** `fx_golden_test`'s
  `reverb_ring` and `reverb_ring_max_size` hash as `a2abe82f` did at GRIT 0,
  and `reverb_dispatch_test`'s three sweeps at SMEAR 0 as its GRIT-0
  sweeps there (`quad_*_smear_0`). The A/B's SMEAR 0 WAVs are
  byte-identical to `a2abe82f`'s GRIT 0 renders.
- **Re-recorded** (each ran at GRIT ≠ 0): `fx_golden_test`'s
  `reverb_ring_full_grit` (now `reverb_ring_smear`, SMEAR 1);
  `reverb_dispatch_test`'s `quad_sweep_48k`, `quad_sweep_44k1`, `quad_bus`
  (they walk SMEAR now); `instrument_test`'s `reverb_send_on` and its
  pre-limiter row (the old default GRIT 0.3); `screen_golden_test`'s
  `mixer_fx_reverb` (the page now reads SIZE · TIME · DAMP · SMEAR).
  `fx_golden_test`'s `reverb_ring` and `reverb_ring_max_size` ran at GRIT
  0.3 and now hold the GRIT 0 hashes above.
- **Pitch** (QUAD at SIZE 0, TIME 1, SMEAR 1, RATE 1): every pass within
  ±25 cents of the frozen wander's tail; one 4 s window's mean within
  ±1 cent; one sinusoid explains 13 % of the pitch trace (a 2 Hz vibrato
  on the test comb: 85 %).
- **Spread of a held 1 kHz tone** (−20 dB width): 1.5 Hz at SMEAR 0,
  3.3 Hz at SMEAR 1 with RATE 1; 2.2 Hz at the default RATE.
- **AXI:** 442,228 B (+96 over 2dc9eefd's 442,132: four wanders and the
  depth; the ratchet moves with it).
- **SMEAR scales to the loop:** D = SMEAR × 0.18 % of the round trip, so
  1 is the cap at every SIZE and the whole knob does something (the
  ±2 ms law capped at SMEAR 0.3 at SIZE 0; the cap is under 2 ms at
  every SIZE, so ±2 ms never binds on QUAD).
- **Flash:** plain release `text` 702,584 B (+1,572 over T4's 701,012);
  `--features orbit` 721,968 B (+1,372 over 720,596). SMEAR is a runtime
  branch at the four stage reads (a third `run` monomorph cost +4.3 KB);
  the cube's jitter is about 300 B of it.
- **REV QUAD S bench** (unit, 2026-10-04, at `3f73921b` + Task 3): 51 on
  both `bench,orbit` and `bench,orbit,master-tape`, under REV QUAD (58,
  55): **passes**; ΔCOST against REV QUAD < 0.

### Task 7: the switch (2026-10-04, host)

- **Phases:** FADE OUT 15, CLEAR 4, FADE IN 15 (review: one 11.6 k-cycle
  CLEAR block outran the master tape's ~9 k STRESS headroom). CLEAR zeroes
  a quarter of the arena a block; its last block also resets the state and
  the half-bands, the decimator's included, so FADE IN is bit for bit a
  fresh engine fed the send from its first block. `Left<N>` bounds each
  phase's count to 1..=N, so no ramp under- or overruns.
- **AXI:** `FxBus` +48 B (the latched `ReverbParams`; the TYPE and the
  phase sit in padding).
- **Flash:** `--features orbit` `.text` 575,016 → 574,848 (**−168**),
  `.rodata` 150,112 (±0), against `9aba0a0b`; the quartered CLEAR and
  `Left<N>` cost +144 over the one-block CLEAR. MIX's ramp and the
  switch's share one out-of-line loop, which more than pays for the
  switch.
- **REV SWITCH MAX bench** (unit, 2026-10-04, at `3f73921b` + Task 3):
  414,093 on `bench,orbit` and 435,137 on `bench,orbit,master-tape`,
  against 448,000: **passes** both. The bench
  build carries the row (QUAD → QUAD every 40 blocks over 2,000, six
  A16+17 voices, every effect at its worst, the TAPE too in the
  master-tape build); gate ≤ `BlockBudget::budget_cycles`, run on both
  `bench` and `bench,master-tape`.

### Task 8: GATE and REVERSE on the BURST engine (2026-10-05, rev V at 480 MHz, unit)

- **Bench** at this task's second commit, builds `bench,orbit` ("default")
  and `bench,orbit,master-tape` ("tape"). Rows are cycles per sample less
  MIX. Each FX row now warms through the switch to its TYPE (34 blocks)
  before it is timed (`.superpowers/sdd/2026-10-04-reverb-types/t8-bench-orbit.txt`, `t8-bench-tape.txt`).

| Row | default | tape |
|---|---|---|
| REV QUAD | 50 | 67 |
| REV QUAD S | 50 | 49 |
| REV GATE | **16** (ΔCOST −34) | **38** (−29) |
| REV RVRS | **16** (−34) | **39** (−28) |
| BUS+V | 1,076 | 1,386 |
| STRESS (max / 448,000), QUAD | 527,408: over | 491,126: over |
| STRESS with GATE | 526,596: over | 489,250: over |
| REV SWITCH MAX (QUAD ↔ GATE) | **411,261: passes** | **432,894: passes** |

- **ΔCOST ≤ headroom** on both builds: GATE and REVERSE cost less than QUAD.
  STRESS is over budget with ORBIT on both builds, with QUAD and with GATE
  alike: that is #355, not this task (GATE reads under QUAD).
- **Flash** (`--features orbit`, release, against `fbeb4f8a`): `.text`
  +8,680, `.rodata` +1,000.
- **AXI:** `FxBus` +88 B (`BurstEngine`, 152 B, is `EngineState`'s largest).
- **WET:** GATE 6.75, REVERSE 7.05: QUAD's short-term loudness (the loudest
  400 ms) on a held −12 dBFS pink pad at MIX 0.5, peaks within 0.6 dB of
  QUAD's (`check_sustained_loudness`).
- **Host readings** (`reverb_burst_test`): GATE at 177 ms, TAIL 0, SHAPE 0:
  −20 dB end at 250 ms, the 50 ms after it 24.8 dB under the plateau.
  REVERSE at defaults: peak at 375 ms, |ρ| 0.38. Noise floor (QUAD 12.1 dB
  in `floor_db`'s frame): GATE 9.3, REVERSE 5.0. LENGTH 40 → 600 ms lands
  every tap in 901 ms. REGEN settles within 1/L of 1 kHz.
- **Goldens:** `rev_gate`, `rev_gate_flat_tail0`, `rev_reverse`,
  `rev_reverse_regen` at MIX 0.3; QUAD's unchanged.
- **Deviations accepted** (ADR 0085 § Consequences, BURST as built):
  DENSITY's fade, the shared decorrelator, the per-block eased weights, the
  fixed 24 kHz layout, whole-dB SHAPE/CURVE, and loudness by sustained
  input.

### Task 9: ROOM on the LOOP engine (2026-10-05, rev V at 480 MHz, unit)

- **Bench** at `9cc3e42e` (fix round 1), builds `bench,orbit` ("default")
  and `bench,orbit,master-tape` ("tape"); rows are cycles per sample less
  MIX, each warmed through the switch (`.superpowers/sdd/2026-10-04-reverb-types/t9-bench-orbit.txt`, `t9-bench-tape.txt`).

| Row | default | tape |
|---|---|---|
| MIX | 800 | 785 |
| REV QUAD | 55 | 71 |
| REV QUAD S | 49 | 49 |
| REV GATE | 34 | 55 |
| REV RVRS | 34 | 56 |
| REV ROOM | **17** (ΔCOST **−38**) | **41** (**−30**) |
| BUS+V | 1,078 | 1,423 |
| STRESS (max / 448,000), QUAD | 529,252: over | 491,223: over |
| STRESS with GATE | 528,090: over | 490,279: over |
| STRESS with ROOM | 527,059: over | 488,153: over |
| REV SWITCH MAX (QUAD → GATE → ROOM) | **414,364: passes** | **436,583: passes** |

- **ΔCOST ≤ headroom** on both builds: ROOM costs less than QUAD (76 and
  23 untouched). REV ROOM runs DECAY held with SIZE and PRE-DLY always
  crossfading; the bench has no DECAY-moving row (host estimate: about
  +390 instructions a block, under 11 cycles a sample). STRESS is over
  with ORBIT for every TYPE alike, ROOM the lowest: #355, not this task.
- The host estimate before the unit read −9 (steady): the chip reads
  −38 and −30.
- **Flash:** plain release `text` 720,404 B (+5,220 over `13b55276`'s
  715,184); `--features orbit` `.text` +4,656, `.rodata` +396. The
  engine is 4.3 KB of it (`run` 3.1 KB, unrolled).
- **WET_ROOM 10.8:** short-term loudness −6.73 dB against QUAD's −6.73,
  peak +5.36 dBFS against +5.95 (`check_sustained_loudness`). ROOM's
  0.55 s holds less than QUAD's 1.9 s: 7.5 dB over QUAD's 4.54.
- **Host readings** (`reverb_loop_test`): RT60 from the last tap (100 ms
  after the burst) 0.60 s at defaults (0.548 target), 0.18, 0.26, 0.60,
  0.88 and 1.48 s at DECAY 0–1, 0.23 s at DECAY 0 DIFFUSE 1; onset 7 ms.
  Noise floor 15.4 dB (QUAD 12.1), 15.5 at DIFFUSE 1. SIZE 0 → 1 lands
  in 22 crossfades (0.67 s); after it and PRE-DLY 0 → 1 the tail's peak
  sits within 5 Hz of 1 kHz, 3.9 cents at most while moving. Slap −21.7
  dB moved, −21.6 static. Level plan: PRE 0.50, delays 0.50, allpass
  states 0.52 of full scale at most.
- **Goldens:** `rev_room` (MIX 0.3); every other hash unchanged.

### Task 10: HALL and its plate zone (2026-10-05, host; unit held)

- **Unit held** (coordinator, 2026-10-05: the MidiVerb II presets are the
  reference now; preset matching may change the algorithm). REV HALL,
  `STRESS ROOM … HALL` and REV SWITCH MAX (QUAD → GATE → ROOM → HALL)
  are built into the bench, both builds; `bench_rev` and its kernels are
  gone.
- **Static count** (Thumb, `bench,orbit` release): the per-sample loop is
  HALL 280 instructions (166 VFP), ROOM 306 (188; it read 350 at Task 9,
  before the f32 stage allpasses); per segment 214; `run` outside it 379.
  HALL's per-sample body is 70 under the ROOM that read 17 / 41 on the
  unit (ΔCOST −38 / −30): at BURST's 0.8 cycles an instruction, about 28
  cycles an output sample less. Estimate, not a gate: ΔCOST well under
  0 on both builds (76 / 23).
- **Flash:** plain release `text` 726,688 B, **+5,492** over `ba2d1acf`
  (721,196): HALL's `run` monomorph 2.8 KB, its `ease_to`, `prime` and
  `schedule` 1.2 KB, the zone, panel and bench rows the rest.
- **WET_HALL 9.7** (+6.6 dB over QUAD's 4.54): short-term loudness
  −6.76 dB against QUAD's −6.73, peak +4.91 dBFS against +5.95.
- **Host readings** (`reverb_loop_test`, fix round 1): RT60 11.3 s at
  SIZE 0 DECAY 1 (10 s ± 35 %; round trip 0.920, no stage over 0.985);
  at SIZE 0.65, DECAY 0–1: 0.79, 1.40, 2.28, 2.58, 4.76, 8.82 s against
  0.80, 1.50, 2.49, 2.83, 5.32, 10.0; plate corner onset 4 ms; corr(L, R)
  −0.001; PRE-DLY 62.5 ms → 0 through the zone in 13 crossfades of ≤ 120;
  after SIZE 0 → 1 and PRE-DLY 0 → 1 the tail's 1 kHz ± 25 Hz centroid
  reads 1000.06, 1000.10, 1000.10 Hz in three 1.2 s windows 43–27 dB over
  the LSB; the plate at 10 s falls to exact 0 near 19 s; level plan PRE
  0.535 (pad peak 1.13), delays 1.72, a held 500 Hz mode at DECAY 1 THD
  16.4 %; noise floor 14.8 dB (QUAD 12.0), 14.9 at DIFFUSE 1. ROOM now:
  floor 14.0 (13.8 at DIFFUSE 1).
- **Arena:** HALL 15,958 i16 slots + 1,908 f32 = 19,774 of 23,220; ROOM
  9,076 + 2,232 f32 = 13,540.
- **Goldens:** `rev_hall`, `rev_hall_plate` (MIX 0.3); `rev_room`
  re-recorded (f32 stage allpasses, unrounded dampers).

### Task 9b: ROOM v2 (2026-10-05, host; unit deferred)

- **MidiVerb II fit** (reference harness, on the Rust render, 16 noise
  seeds; ADR 0095's bar). All nine ROOM programs pass:

| # | Preset | RT60 250–4k (%) | Pre (ms) | C50 (dB) | NED 20/50/100 (%) | Centroid (%) |
|---|---|---|---|---|---|---|
| 1 | Small Bright .1 | +1, +2, −4, +4, +4 | +0.7 | +0.5 | −4, −8, −6 | −0 |
| 2 | Small Bright .2 | +4, −1, −3, +1, −0 | +0.2 | −0.4 | −2, +4, +3 | −2 |
| 3 | Small Bright .3 | −2, −1, +1, −2, +2 | +0.0 | +0.0 | −1, −5, +7 | −3 |
| 5 | Medium Bright .6 | −3, −0, −3, +0, +3 | −0.1 | +1.0 | −8, −5, −3 | −5 |
| 8 | Medium Dark .6 | −2, −1, −1, +1, +1 | −0.1 | +0.5 | −8, −4, +4 | −1 |
| 9 | Medium Dark .5 | −2, −1, +2, +4, −5 | +0.0 | −0.2 | −7, +7, +2 | +0 |
| 10 | Medium Bright .6 | −3, +4, +2, −5, −1 | −0.2 | −0.7 | −4, +5, +5 | +3 |
| 11 | Medium Bright .8 | −3, +3, +2, +2, −0 | +0.0 | +0.8 | −0, −2, +7 | −3 |
| 14 | Medium Dark 1.0 | +3, −0, −2, +1, −3 | −0.1 | −0.8 | −3, +0, +0 | +3 |

  Several pass with little margin: #1's 1 kHz and 4 kHz RT60 sit −4.2
  and +4.3 %, #5's C50 +0.97 dB and centroid −4.9 %, #9's 4 kHz and #10's
  2 kHz −5 %. Rounding the knobs to 3 places tipped #3 and #9 over; both
  were refit on that grid.
- **Fitted knobs** (SIZE, DECAY, TONE, DIFFUSE, PRE-DLY, DAMP, LOOPS;
  SMEAR 0), fitted on the Rust render from the float model's fits mapped
  through the Rust laws, never copied: `fx_golden_test`'s `MV2_ROOM`.
  START is program 5's, with SMEAR 0.3.
- **Near misses:** DAMP's top opening the dampers passed #10 and #14
  outright and let #9 pass after a refit. The tap weight tied to θ was not
  built: it scales every tap alike, and every judged metric is level-free.
- **Cost (no unit reading; instruction counts of the bench's REV ROOM
  settings in a Cortex-M7 emulator, `bench,orbit`-equivalent release):**
  QUAD at its defaults (the MIX row) 276.5, QUAD at the REV QUAD settings
  328.2, Task 9's ROOM 312.8, ROOM v2 524.2 instructions an output sample
  (`Room::run` about 600 a ring sample). Against Task 9's ROOM's unit
  reading (17 cycles for 36 instructions over the MIX row), REV ROOM
  reads about 117 cycles: **ΔCOST about +62 (default, gate 76)**. The
  tape build read Task 9's ROOM 24 cycles higher: about 141, **ΔCOST
  about +70 against the tape's 23: over**, for the owner. The unit decides.
- **Arena:** 10,530 i16 + 5,124 f32 = 20,778 of 23,220 slots (each line
  keeps one block's guard: +1,120).
- **Flash:** plain release `text` 752,512 B, +12,768 over `aeb85960`.
- **WET_ROOM 2.42:** short-term loudness −6.72 dB against QUAD's −6.73,
  peak +3.99 dBFS against +5.95. Noise floor 5.3 dB (QUAD 12.1), 4.0 at
  DIFFUSE 1.
- **Host readings** (`reverb_loop_test`): 1 kHz RT60 0.209, 0.643 and
  1.960 s at DECAY 0, ½ and 1 (targets 0.2, 0.632, 2.0); first arrival
  0.52 ms past the bus's half-bands at every SIZE; SIZE 0 → 1 lands in 14
  crossfades; θ 90° holds the pulse in A alone 164 blocks and B alone
  153, never both.
- **Goldens:** `rev_room` re-recorded, `rev_room_mv01`–`mv14` added;
  `rev_hall` and `rev_hall_plate` unchanged.

### Task 11: BLOOM (2026-10-05, host; unit folded into Task 13)

- **MidiVerb II fit** (reference harness, on the Rust render, 16 noise
  seeds, a 20 s click and 2 s + 20 s noise as the references; ADR 0095's
  bar). Both pass:

| # | Preset | RT60 250–4k (%) | Pre (ms) | C50 (dB) | NED 20/50/100 (%) | Centroid (%) |
|---|---|---|---|---|---|---|
| 45 | Bloom 1 8 Sec | +1, +0, −1, +0, +3 | +0.2 | −0.1 | −2, −1, +2 | +1 |
| 49 | Bloom 2 7 Sec | +0, +2, −1, −4, +2 | +0.1 | −0.2 | −0, +2, +2 | −1 |

- **Fitted knobs** (SWELL, DECAY, TONE, DIFFUSE, PRE-DLY; SMEAR 0), on
  the 3-place grid: #45 0.789, 0.956, 0.597, 0.165, 0; #49 0.809, 0.956,
  0.619, 0.226, 0.674 (`fx_golden_test`'s `MV2_BLOOM`). START is #45's,
  with SMEAR 0.3 and RATE 0.4: 579 ms · 11 s · 3.2k · 0.48.
- **Cost (no unit reading; Cortex-M7 emulator instructions an output
  sample, `orbit` release):** BLOOM at the bench's REV BLOOM settings
  404.3, at its start 392.1 (QUAD's MIX row 278.4, REV QUAD 330.1, ROOM v2
  404.8). `Bloom::run` is 8.4 KB of text (`Room::run` 21.5 KB), prime,
  ease and schedule 1.4 KB more. On Task 9's ROOM anchor (17 cycles for
  36 instructions, 0.47 a cycle): REV BLOOM about 60 default, 84 tape,
  **ΔCOST about +5 (gate 76) and +13 (gate 23)**; at 1.06 cycles an
  instruction about +59 and +67: the tape gate fails there. The unit
  decides (Task 13).
- **Arena:** 19,750 i16 + 782 f32 = 21,314 of 23,220 slots.
- **Flash:** plain release `text` 769,800 B, +10,464 over `d78331d9`.
- **WET_BLOOM 6.43:** short-term loudness −6.35 dB against QUAD's −6.73,
  peak +5.5 dBFS against +5.95. Noise floor at DECAY 0 16.7 dB (QUAD
  12.0), 19.2 at DIFFUSE 1: over the 4 and 6 dB asked (ADR 0095, BLOOM as
  built).
- **Host readings** (`reverb_bloom_test`): 1 kHz RT60 2.04, 4.82 and 11.73
  s at SWELL 0, DECAY 0, ½ and 1; SWELL 0 → 1 lands in 89 crossfades;
  the engine matches an f64 model of ADR 0095's structure to 10⁻⁴ of the
  peak over 2.5 s; SMEAR's window mean 1.25 cents.
- **Goldens:** `rev_bloom`, `rev_bloom_mv45` and `rev_bloom_mv49` added;
  every other golden unchanged.

---

## Self-review

- **Spec coverage.** Each spec section maps to its tasks:

| Spec section | Tasks |
|---|---|
| TYPE list and codes | T6 |
| Engines | T8–T11 |
| Moving lengths and pitch bounds | T2, T8–T11 |
| Level plan | T2 (`check_level_plan`), T8–T11 |
| GRIT retired, SMEAR | T5 (QUAD), T6, T8–T11 |
| Arena and f32 regions | T2, T4 |
| Parameters, `with_type` and memory | T6 |
| Switch | T7 |
| Smoothing | T2 (`Ease1`, `check_knobs_ease`), T8–T11 |
| SMEAR | T5 (`Wander`), T8–T11 |
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

- **Amendment (ADR 0095).** Tasks 9b, 10b, 11 (rewritten) and 11b fit every TYPE with a counterpart to the MidiVerb II presets by the reference harness. The order is 9b → 10b → 11 → 11b → 13. Task 11 takes Task 12's deferred BLOOM sync rows, and Task 13 adds every TYPE's cost rows, the SYNC bench and `OrbitRunner::COST`.
- **Steps.** Every test has its assertions and the spec's values. Function bodies appear only for the soft clip, TPDF and idle mute, and ROOM-T. Every commit stages its paths.
- **Types.** Each is defined once, in one task, and used under that name after: `Tempo` (T4), `FoldState`/`fold`/`Division` (T6), `EngineKnobs`/`knobs` (T6), `KnobMemory` (T6, 108 B, `u16::MAX` sentinel), `SwitchPhase` (T7), `LoopEngine<LOOPS, STAGES>` (T9), `Render` and the `check_*` helpers (T2).
- **Review Focus.** Each of the five lines has its test, in T7, T6, T7, T5 and T12.
- **Proportion.** The code blocks are test assertions and signatures, with no engine bodies.
