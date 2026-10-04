# Reverb TYPEs Implementation Plan

**Goal:** Give the master reverb a TYPE: today's QUAD plus ROOM, HALL (PLATE as a zone of it), BLOOM, GATE and REVERSE. They share one i16 arena, switch without a click and never re-pitch a tail. GRIT is redesigned as the reverb's converters, QUAD included.

**Architecture:**
- Three new engines (LOOP, BURST, BLOOM) live in `chimera-core/src/dsp/rev/`, beside today's `dsp::ring` (QUAD). A dispatcher, `dsp::rev::Reverb`, runs one engine at a time over a shared `ReverbArena`, and replaces `RingReverb` in `FxBus`.
- Lengths move by pitch-free crossfades between fixed reads. The only moving (interpolated) reads are the wobble's.
- The level plan bounds every stored value with a soft clip at each loop's re-entry, normalised in-loop allpass states, and f32 diffusers and decorrelators.
- GRIT becomes ADC and DAC converters (high cut, TPDF dither, bit depth) outside the engines.
- One parameter block of semantic slots, with a per-TYPE `const` panel for labels, ranges and steps.

**Tech Stack:** Rust 2024, `no_std` `chimera-core`; `chimera-stm32` (bench build, `--features bench`); host tests under `chimera-core/tests`; `tools/chimera-usb.py` for the unit.

**Spec:** `docs/superpowers/specs/2026-10-04-reverb-types-design.md`. ADR: `docs/adr/0085-the-master-reverb-picks-a-type.md`.

**Base:** `reverb-types` rebased onto `orbit-qa2`. On that base, ADR 0086's `ui::hint`, ADR 0089's fat LTO, the ORBIT clock and runner, and `cost_test`'s current numbers are present. Every path below exists on that base unless the task creates it.

## Global Constraints

- **Safety.** No `unsafe` without a `// SAFETY:` comment. No heap and no libc in `chimera-core`. The audio thread never blocks or allocates.
- **Smoothing.** Parameter changes are lerped, never snapped. Every continuous knob eases; lengths move only by crossfade; TYPE moves only through the switch.
- **The arena.** It is `[i16; 23_220]` (46,440 B, `RING_LEN`) and never grows. Each TYPE's layout is a `const` asserted to fit.
- **Ids and codes.** Reverb ParamId 0 stays retired. New ids are 6–13. Disk codes: `ReverbType` QUAD 0, ROOM 1, HALL 2, GATE 3, REVERSE 4, BLOOM 5 (6 is never assigned); `RevSync` OFF 0, TIME 1, ALL 2. They are frozen in `tests/fixtures/disk_codes_v1.txt` (ADR 0045).
- **Provenance.** No code, instruction listing, or ROM coefficient, offset or tap table from github.com/thement/midiverb_emulator or commit 7119f7ad, whole or in part. Every constant comes from the spec's tables or from the ear. Every engine task's report lists where its tables came from.
- **CPU.** The target is the pre-refactor REV QUAD row: 485 cycles per output sample, re-measured in Task 1. The hard gate is `cost_test`'s voice-count assertions with `FxBus::COST` set to the costliest TYPE's BUS reading. Headroom is 96 cycles (default build) and 23 (`master-tape`). The STRESS row is the second gate.
- **Flash.** Measure the plain release build's `text` (ADR 0089: fat LTO, one codegen unit, no environment variables). A TYPE adds ≤ 3 KB, and the total is ≈ 9.5–12.5 KB.
- **Hints.** They follow ADR 0086. The reverb adds no gesture, so `View::hint` returns `hint::NONE` for every reverb cell.
- **Names.** User-facing names are sound names. QUAD keeps the word `QUAD`.
- **Commits.** Terse, no type prefix, staged by path. Never use `git add -A`.
- **The unit.** Tasks marked **(unit)** flash the bench build with `just flash-bench`, wait about 4 minutes, then read `just usb bench`; both are hands-free. They record every reading in this plan's § Measured, with the commit, the date and the build (`default` or `master-tape`).

## Review Focus

1. **TYPE scrolled through all six mid-tail at MIX 1** should end on the last TYPE with no click and no burst. Pinned in Task 7: `scrolling_types_mid_tail_lands_on_the_last_without_a_click`.
2. **A project carrying an unknown TYPE code, or NaN in slots 6–13,** loads as QUAD and renders finite. Pinned in Task 6: `unknown_type_code_and_nan_slots_load_as_safe_quad`.
3. **A switch at MIX 0, then MIX up,** plays the new TYPE from the current send, with no stale tail. Pinned in Task 7: `switch_at_mix_0_then_mix_up_has_no_stale_tail`.
4. **GRIT moved while the converters' idle mute fades** gives no step in the return. Pinned in Task 5: `dither_mute_fades_without_a_step`.
5. **The MIDI clock dropping out mid-sync** (EXT → internal fallback, ADR 0072) moves synced lengths without fold flapping or a click. Pinned in Task 12: `clock_dropout_moves_synced_length_once`.

---

## File map

| File | Responsibility |
|---|---|
| `chimera-core/src/dsp/rev/mod.rs` (new) | `Reverb` dispatcher, `EngineState`, the switch (`SwitchPhase`), half-bands, MIX ease |
| `chimera-core/src/dsp/rev/arena.rs` (new) | `ReverbArena`, f32 regions, `clear` |
| `chimera-core/src/dsp/rev/parts.rs` (new) | soft clip, normalised allpass, plain f32 allpass, pair read, `Xfade`, `Wobble`, one-pole coefficient |
| `chimera-core/src/dsp/rev/grit.rs` (new) | `Converters`: ADC and DAC high cut, TPDF dither, bit depth, idle mute |
| `chimera-core/src/dsp/rev/sync.rs` (new) | `Tempo`, `Division`, the division lists, `fold`, `FoldState` |
| `chimera-core/src/dsp/rev/loop_engine.rs` (new) | `LoopEngine<SHAPE>`: ROOM, HALL and the plate zone |
| `chimera-core/src/dsp/rev/burst.rs` (new) | `BurstEngine`: GATE and REVERSE, the tap scheduler |
| `chimera-core/src/dsp/rev/bloom.rs` (new) | `BloomEngine` |
| `chimera-core/src/dsp/reverb.rs` | `ReverbParams`, `ReverbType`, `RevSync`, specs, `with_type`, `KnobMemory`, pure knob mappings (`EngineKnobs`) |
| `chimera-core/src/dsp/ring.rs` | QUAD: borrows the arena; `Grid` retired as GRIT |
| `chimera-core/src/dsp/fx_bus.rs` | holds `Reverb`; `process` takes `Tempo` |
| `chimera-core/src/instrument.rs` | `Instrument::set_tempo`, passing `Tempo` into `mix_parts` |
| `chimera-core/src/orbit/runner.rs` | `OrbitRunner::block` calls `inst.set_tempo(self.tempo())` |
| `chimera-core/src/ui/reverb_panel.rs` (new) | per-TYPE panels, value text, steps |
| `chimera-core/src/ui/{block_def.rs, view.rs, block_registry.rs, glyph.rs, renderer.rs}` | `SlotBinding::ReverbPanel`, REV › MORE, `CompositeId::Reverb`, TAIL |
| `chimera-stm32/src/bench.rs` | REV rows, kernels, STRESS, REV SWITCH MAX, voice traffic |
| `tools/ab/stimuli.py`, `tools/ab/render.sh` (new) | A/B stimuli and renders |

---

### Task 1: Baselines on the unit (unit)

**Files:**
- Modify: `chimera-stm32/src/bench.rs`, `docs/superpowers/plans/2026-10-04-reverb-types.md` (§ Measured)

**Interfaces:**
- Produces:
  - bench rows `REV QUAD` (today's `worst_reverb`, renamed label), `BUS`, `BUS+V` (BUS with six A16 ∪ A17 voices rendering between blocks) and `STRESS`;
  - the numbers `QUAD_REV`, `QUAD_BUS` and `QUAD_BUS_V`, recorded in § Measured.

- [ ] **Step 1: Add the rows.**
  - `fn time_bus_with_voices(&mut self, each: fn(&mut AudioShared, u32)) -> u32` is `time_bus` with six voices of `a16_a17()` (the bench's `algo_pair(AlgoId::A16, AlgoId::A17)`) rendered by `Instrument::render` each block. That puts the voices' cache traffic between the FX blocks.
  - `STRESS` is the maximum whole-callback block, in cycles, over 10 s (7,500 blocks). It runs those six voices, every `worst_*` FX, and an `OrbitRunner` playing NEW's groove (ADR 0073). It is printed as `STRESS max/budget` against `BlockBudget::for_cpu(rev).budget_cycles()`.
  - The FX screen's REVERB label becomes `REV QUAD`. Move STRESS and BUS+V to the line after the FX rows, keeping y ≤ 310.
- [ ] **Step 2: Host build check.**
  - Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`, then the same with `--features bench,master-tape`.
  - Expected: both build, with no warnings.
- [ ] **Step 3: Measure (unit).**
  - Run: `just flash-bench`, wait for "bench running", wait about 4 minutes, then run `just usb bench`.
  - Record REV QUAD, BUS, BUS+V and STRESS, and repeat on a `master-tape` bench build: `cargo build … --features bench,master-tape`, then the `flash-bench` recipe's dfu line.
  - Expected: STRESS max < budget, with OVER 0 in `just stats`.
- [ ] **Step 4: Record the headroom.**
  - Recompute the minimum, over `cost_test`'s voice-count assertions, of `budget − FxBus::COST − n × voice`. Expect 96 (default; FOLD 954 × 6 on rev V) and 23 (tape; MORPH KEYS 864 × 5 on rev Y).
  - **If BUS+V reads above `FxBus::COST`:** raise `COST` to the new BUS+V reading first, as its own commit, with a comment like ADR 0031's. If that breaks a `cost_test` voice assertion, stop and hand it to the owner before Task 3 (spec § Target and hard limit).
- [ ] **Step 5: Commit.**

```bash
git add chimera-stm32/src/bench.rs docs/superpowers/plans/2026-10-04-reverb-types.md
git commit -m "Bench: REV QUAD, BUS with voices and STRESS rows; reverb baselines"
```

### Task 2: Reverb parts, host only

**Files:**
- Create: `chimera-core/src/dsp/rev/mod.rs` (only `pub mod` lines for now), `chimera-core/src/dsp/rev/parts.rs`, `chimera-core/src/dsp/rev/arena.rs`
- Modify: `chimera-core/src/dsp/mod.rs` (`pub mod rev;`)
- Test: `chimera-core/tests/reverb_parts_test.rs`

**Interfaces:**
- Produces, in `parts.rs`:
  - `pub fn soft_clip(x: f32) -> f32`;
  - `pub fn ap_norm(x: f32, w_d: f32, g: f32, inv_1mg: f32) -> (f32, f32)`, returning `(y, w_write)`;
  - `pub fn ap_f32(x: f32, z: f32, g: f32) -> (f32, f32)`, returning `(y, v_write)`;
  - `pub fn pair(lines: &[i16], base: usize, d: f32) -> f32`, a linear read at `d ≥ 2` samples behind `base`, wrapping;
  - `pub struct Xfade { pub from: u16, pub to: u16, left: u16 }`, with `XFADE: u16 = 720`, `fn start(&mut self, to: u16)`, `fn weight(&self) -> f32` and `fn tick(&mut self)`;
  - `pub struct Wobble { phase: u32, inc: u32 }`, with `fn set_rate(&mut self, hz: f32, fs: f32)` and `fn block(&mut self) -> [f32; 2]` (L and R values in −1..1, 90° apart, at the block's end);
  - `pub fn one_pole(fc: f32, fs: f32) -> f32`.
- Produces, in `arena.rs`:
  - `#[repr(C, align(4))] pub struct ReverbArena { lines: [i16; RING_LEN] }`;
  - `fn lines(&self) -> &[i16; RING_LEN]` and `fn lines_mut(&mut self)`;
  - `fn f32_region(&mut self, offset: usize, len: usize) -> &mut [f32]` (offset even);
  - `fn clear(&mut self)`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn soft_clip_is_identity_to_the_knee_and_reaches_two_flat() {
    for x in [-1.0f32, -0.5, 0.0, 0.3, 1.0] { assert_eq!(soft_clip(x), x); }
    assert!((soft_clip(3.0) - 2.0).abs() < 1e-6 && soft_clip(10.0) == 2.0 && soft_clip(-10.0) == -2.0);
    let d = (soft_clip(3.0) - soft_clip(2.999)) / 0.001; assert!(d.abs() < 1e-2);
    assert!((1..400).all(|i| { let x = i as f32 * 0.01; soft_clip(x) >= soft_clip(x - 0.01) }));
}
#[test] fn ap_norm_matches_the_plain_allpass_and_bounds_the_state() {
    // Same y as v = x − g·z, y = z + g·v with z = w_d / (1 − g); |w| ≤ max|x| for a 1 kHz sine at the AP's worst frequency.
}
#[test] fn pair_reads_fixed_at_fraction_0_and_the_mean_at_half_across_the_wrap() {}
#[test] fn xfade_blends_linearly_over_720_and_lands() { /* weight 0 → 1 in 720 ticks, exactly 1.0 after */ }
#[test] fn wobble_l_and_r_are_90_degrees_apart_and_its_lerp_error_is_under_1e4_at_2_hz() {}
#[test] fn f32_region_round_trips_and_clear_zeroes_both_views() {}
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p chimera-core --test reverb_parts_test`. Expected: FAIL, unresolved imports.
- [ ] **Step 3: Implement.**
  - The soft clip, for |x| > 1, is `sign(x)·(1 + u − u²/4)` with `u = min(|x| − 1, 2)`, branch-free via `min`, `abs` and `copysign`.
  - `f32_region` casts with `core::slice::from_raw_parts_mut`. Its `// SAFETY:` comment covers three things: alignment, from `align(4)` and the even offset (asserted); bounds, from `offset + 2·len ≤ RING_LEN` (asserted); and that `&mut self` keeps any i16 view from being alive.
- [ ] **Step 4: Run them to see them pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit.** `git add` the four files, then `git commit -m "Reverb parts: soft clip, normalised allpass, pair read, crossfade, wobble, arena"`.

### Task 3: Worst-case kernels on the chip (unit)

**Files:**
- Create: `chimera-stm32/src/bench_rev.rs` (bench-only, `#[cfg(feature = "bench")]`)
- Modify: `chimera-stm32/src/bench.rs`, `chimera-stm32/src/main.rs` (`mod bench_rev;` under the feature), plan § Measured

**Interfaces:**
- Consumes: Task 2's `parts` and `ReverbArena`.
- Produces: bench rows `K PAIR` (the marginal cost of one moving pair over two fixed reads), `K BURST`, `K ROOM` and `K HALL`. Each kernel is timed through `time_bus_with_voices`, in place of the reverb's processing.

- [ ] **Step 1: Write the kernels** to the spec's § CPU budget worst cases, using the spec's lengths and Task 2's parts. Each has the shape `fn run(&mut self, arena: &mut ReverbArena, u: &[f32; HALF], l: &mut [f32; HALF], r: &mut [f32; HALF])`.
  - **BURST:** 12 taps (tap 0 from the register; tap 11 is also REGEN's read), 4 taps crossfading, 2 wobbled f32 decorrelator APs per side, a TAIL stage with a normalised AP, 2 soft clips, a truncating line write, and the GRIT converters as stubs (3 quantisations, 3 one-poles, 2 xorshift steps).
  - **ROOM:** PRE-DLY and 4 stage delays crossfading, 2 wobbled normalised stage APs and 2 fixed ones, an f32 diffuser of 4 APs, the rotation, 2 soft clips, 6 fixed taps and the converter stubs.
  - **HALL:** as ROOM, with 1 loop of 4 stages and 1 soft clip.
- [ ] **Step 2: Build check.** Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`. Expected: it builds.
- [ ] **Step 3: Measure (unit).** Run `just flash-bench`, wait about 4 minutes, then `just usb bench`, on both builds. Record each kernel's ΔCOST = row − QUAD_BUS_V.
- [ ] **Step 4: Gate ("sound first", spec § Build order step 0).**
  - Each kernel's ΔCOST must be ≤ 96, and ≤ 23 in the `master-tape` build.
  - **K ROOM over 23 in the tape build:** apply ROOM-T stage 1 (wobble loop B's AP only) in the kernel and re-measure. If it is still over, apply stage 2 as well (2 output taps a side: drop (A2, 0.83) left and (B2, 0.79) right).
  - Record the stage taken. Task 9 builds ROOM with it.
  - Any other kernel over its headroom takes its spec fallback the same way: BURST 3 crossfading taps, HALL one wobbled AP.
- [ ] **Step 5: Commit** `bench_rev.rs`, `bench.rs`, `main.rs` and the plan: `git commit -m "Bench: reverb kernels and the moving-pair cost"`.

### Task 4: The arena moves out of the ring, and the dispatcher (QUAD bit-identical)

**Files:**
- Modify: `chimera-core/src/dsp/ring.rs`, `chimera-core/src/dsp/fx_bus.rs`, `chimera-core/src/dsp/rev/mod.rs`
- Test: existing `reverb_test`, `fx_golden_test`, `gain_staging_test`, `fx_bus_test`, `in_place_test`, `memory_budget_test`; new `chimera-core/tests/reverb_dispatch_test.rs`

**Interfaces:**
- Consumes: `ReverbArena`.
- Produces:
  - `Ring::process(&mut self, arena: &mut ReverbArena, u: &[f32; HALF], blk: &RingBlock, l: &mut [f32; HALF], r: &mut [f32; HALF])` (`Ring` no longer has `lines`);
  - `pub struct Reverb` in `dsp::rev`, replacing `RingReverb`, with `fn process(&mut self, send: &[f32; BLOCK_SIZE], p: &ReverbParams, mix: f32, tempo: Tempo, sample_rate: u32, out: &mut Stereo)`, `fn arena(&self) -> &ReverbArena` and `fn init_in_place(slot)`;
  - `#[repr(C, u8)] pub enum EngineState { Quad(QuadState), … }`, with only `Quad` in this task;
  - `pub struct Tempo(pub f32)`, samples per beat, in `dsp::rev::sync`, with `Tempo::DEFAULT` = 120 BPM at 48 kHz (24,000.0);
  - `FxBus::process(.., tempo: Tempo, ..)`, called by `instrument::mix_parts` with `Tempo::DEFAULT` until Task 12.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn zeroed_engine_state_is_quad_default() { /* MaybeUninit zeroed EngineState == EngineState::Quad(Default::default()) */ }
#[test] fn reverb_fits_its_budget() { assert!(size_of::<EngineState>() <= 160); }
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p chimera-core --test reverb_dispatch_test`. Expected: FAIL.
- [ ] **Step 3: Move `lines` into `ReverbArena`.** `RingReverb`'s body becomes `Reverb`'s QUAD path, unchanged in arithmetic. `FxBus` holds `Reverb`. Update `fx_bus.rs`'s `field_list!` and `init_in_place`. `EngineState`'s zeroing carries a `// SAFETY:` comment: variant 0 is `Quad`, and every field is an integer or a float.
- [ ] **Step 4: The refactor lock.** Run: `cargo test -p chimera-core --test reverb_test --test fx_golden_test --test gain_staging_test --test fx_bus_test --test in_place_test --test memory_budget_test --test reverb_dispatch_test`. Expected: PASS, with no golden changed.
- [ ] **Step 5: Bench (unit).** Run `just flash-bench`, then `just usb bench`. REV QUAD must be ≤ QUAD_REV + 1 %. Record it.
- [ ] **Step 6: Commit.** `git commit -m "Reverb arena out of the ring; Reverb dispatches QUAD"`.

### Task 5: GRIT becomes the converters (QUAD refactor, second commit)

**Files:**
- Create: `chimera-core/src/dsp/rev/grit.rs`, `chimera-core/tests/reverb_grit_test.rs`, `tools/ab/stimuli.py`, `tools/ab/render.sh`
- Modify: `chimera-core/src/dsp/ring.rs` (storage at a fixed 1 LSB; `Grid` stays only as the fixed Q3 quantiser), `chimera-core/src/dsp/rev/mod.rs`, `chimera-core/tests/reverb_test.rs`, `chimera-core/tests/fx_golden_test.rs` (re-recorded)

**Interfaces:**
- Produces:
  - `pub struct Converters`, with `fn adc(&mut self, x: &mut [f32; HALF], grit: f32)` and `fn dac(&mut self, l: &mut [f32; HALF], r: &mut [f32; HALF], grit: f32)`;
  - `pub fn bits(grit: f32) -> f32 { 16.0 − 6.0 * grit }`;
  - `pub fn cutoff_hz(grit: f32) -> f32 { 11_000.0 * (8.0 / 11.0f32).powf(grit) }`, via `exp2`;
  - `pub const IDLE_HOLD: u32 = 4_800` (100 ms at 24 kHz) and `IDLE_FADE: u32 = 1_200` (50 ms).
- `Reverb` runs `adc` on the decimated send (in dBFS units, before ½) and `dac` on the return after `WET` and before MIX. Both are skipped entirely when eased GRIT is exactly 0.

- [ ] **Step 1: Write the failing tests** in `reverb_grit_test.rs`.

```rust
#[test] fn grit_0_bypasses_bit_for_bit() { /* QUAD at GRIT 0 == the Task 4 golden hashes */ }
#[test] fn adc_lands_on_its_step() { for (g, b) in [(0.3, 14.2), (0.6, 12.4), (1.0, 10.0)] { /* dither off: every output a multiple of 2^(1−b) within 1e-9 */ } }
#[test] fn cutoff_tracks_grit() { assert!((cutoff_hz(1.0) - 8_000.0).abs() < 1.0); /* measured −3 dB within ±3 % at GRIT 0.01 and 1 */ }
#[test] fn dither_keeps_a_minus_80_dbfs_sine_as_a_sine() { /* GRIT 1: its bin ≥ 10 dB over the noise median; no harmonic > median + 3 dB */ }
#[test] fn a_grit_1_tail_has_no_correlated_products() { /* 1 kHz burst at −12 dBFS, each 100 ms window of the decay: 2 kHz, 3 kHz and 1 kHz ± mode spacing bins ≤ median(±200 Hz) + 3 dB */ }
#[test] #[ignore] fn todays_in_loop_grit_shows_correlated_products() { /* the same test on the pre-Task-5 grid: documented failing reference */ }
#[test] fn idle_mute_lets_the_tail_reach_zero() { /* 150 ms after the send stops, dither is off; falls_silent_within at GRIT 1 holds */ }
#[test] fn dither_mute_fades_without_a_step() { /* Review Focus 4: GRIT 0 → 1 → 0 across the mute boundary; max second difference ≤ click_free bound */ }
#[test] fn noise_floor_matches_the_table() { /* QUAD at MIX 1, GRIT 0.3, 0.6, 1: −66, −64, −54 dBFS ± 2 dB */ }
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p chimera-core --test reverb_grit_test`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Noise.** One xorshift32 is stepped twice per ring sample. A TPDF value is `(b0 as f32 − b1 as f32) / 256.0 · Δ_c` from two bytes. The ADC takes bytes 0–1, DAC L bytes 2–3 and DAC R bytes 4–5.
  - **Quantise** with `rint(x / Δ_c) · Δ_c`, using `ring::rint`.
  - **Idle mute.** A per-converter counter resets on any non-zero input. Past `IDLE_HOLD`, a linear gain over `IDLE_FADE` scales the dither to 0.
  - **The ring's storage.** `Grid` is constructed only at `GRIT = 0` (Δ = 1).
- [ ] **Step 4: Replace the grid tests.** In `reverb_test.rs`, remove `grit_rounds_onto_its_grid`, `grit_truncates_toward_zero_onto_its_grid`, `grit_1_stores_only_multiples_of_64`, `grit_raises_the_noise_floor` and `no_limit_cycle_at_grit_1`. Keep `no_limit_cycle_at_grit_0` and `falls_silent_within` at 0 and 1.
- [ ] **Step 5: Re-record the goldens away from GRIT 0.** Run: `cargo test -p chimera-core --test fx_golden_test` with the golden-update procedure in `tests/common/golden.rs`. Expected: only the reverb cases at GRIT ≠ 0 change. Name them in the commit message.
- [ ] **Step 6: The A/B renders.**
  - `tools/ab/stimuli.py` writes the spec's four stimuli (48 kHz, 12 s tails) from a fixed seed, with no audio committed.
  - `tools/ab/render.sh <stimulus> <grit>` renders QUAD at its defaults through `chimera-desktop`'s offline render (host test binary `reverb_render`, gated by `#[ignore]`).
  - Render GRIT 0, 0.3, 0.6 and 1, before (at Task 4's commit, via `git worktree`) and after, to `target/ab/`. Hand the eight WAVs over with each one's noise floor and correlated-product figure.
- [ ] **Step 7: Run the suite.** Run: `cargo test -p chimera-core`. Expected: PASS.
- [ ] **Step 8: Bench (unit).** Run `just flash-bench`, then `just usb bench`. Record REV QUAD with the new GRIT (estimate 447).
- [ ] **Step 9: Commit.** `git commit -m "GRIT is the reverb's converters: high cut, TPDF dither, bit depth; no grid in the loop"`.

### Task 6: TYPE, slots, panels and pages

**Files:**
- Modify: `chimera-core/src/dsp/reverb.rs`, `chimera-core/src/storage/codes.rs`, `chimera-core/tests/fixtures/disk_codes_v1.txt`, `chimera-core/src/ui/block_def.rs`, `chimera-core/src/ui/view.rs`, `chimera-core/src/ui/block_registry.rs`, `chimera-core/src/ui/glyph.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/mod.rs` (the UI's TYPE edit and `KnobMemory`), the MIDI MAP label lookup
- Create: `chimera-core/src/ui/reverb_panel.rs`, `chimera-core/tests/reverb_params_test.rs`, `chimera-core/tests/reverb_panel_test.rs`

**Interfaces:**
- Produces, in `dsp::reverb`:
  - `ReverbType { Quad, Room, Hall, Bloom, Gate, Reverse }` and `RevSync { Off, Time, All }`, both `DiskCode`;
  - `ReverbParams` gains `kind`, `sync`, `tone`, `diffuse`, `predelay`, `shape`, `wobble` and `rate`;
  - `ReverbParams::{TYPE = ParamId(6), TONE = 7, DIFFUSE = 8, PREDELAY = 9, SHAPE = 10, WOBBLE = 11, RATE = 12, SYNC = 13}`, with the spec defaults 0.3, 0.4, 0.0, 0.2, 0.0, 0.5 and OFF;
  - `pub fn with_type(self, t: ReverbType, mem: &KnobMemory) -> Self` and `pub fn with_sync(self, s: RevSync, tempo: Tempo) -> Self`;
  - `pub struct KnobMemory { slots: [[u16; 9]; 6], seen: u8 }`, with `store(&mut self, p: &ReverbParams)`, `clear(&mut self)` and `size_of == 108` (padded ≤ 110);
  - `pub struct EngineKnobs { … }`, the mapped quantities, and `pub fn knobs(p: &ReverbParams, tempo: Tempo, fold: &mut FoldState) -> EngineKnobs`. These are the spec's per-TYPE tables, with every `exp`/`lin` law, the wobble caps and the plate zone.
- Produces, in `ui::reverb_panel`:
  - `pub enum RevPage { Main, More }`;
  - `pub struct RevKnob { pub id: ParamId, pub label: &'static str, pub short: Option<&'static str>, pub step: f32 }`;
  - `pub fn knob(t: ReverbType, page: RevPage, k: u8) -> Option<&'static RevKnob>`;
  - `pub fn value_text(t: ReverbType, id: ParamId, p: &ReverbParams, tempo: Tempo) -> FmtBuf`.
- Produces in the UI: `SlotBinding::ReverbPanel(RevPage, u8)`; `SlotCtx.rev: ReverbType`; `EFX_MORE: BlockDef` (id 92, short `MORE`) chained with `ChainBlock::with_subs(&EFX, &EFX_SUB_PAGES)`; `CompositeId::ReverbCube` renamed `CompositeId::Reverb`.

- [ ] **Step 1: Write the failing tests.**

```rust
// reverb_params_test.rs
#[test] fn new_ids_and_idents_are_frozen() { /* (6,"TYPE"),(7,"TONE"),(8,"DIFFUSE"),(9,"PREDELAY"),(10,"SHAPE"),(11,"WOBBLE"),(12,"RATE"),(13,"SYNC"); ParamId(0) absent; specs.len() == 13 */ }
#[test] fn type_and_sync_codes_are_frozen() { /* Quad 0, Room 1, Hall 2, Gate 3, Reverse 4, Bloom 5; code 6 → None; Off 0, Time 1, All 2 */ }
#[test] fn an_old_project_loads_as_quad() { /* decode a v1 Reverb block with ids 1–5 only → kind Quad, sound unchanged */ }
#[test] fn unknown_type_code_and_nan_slots_load_as_safe_quad() { /* Review Focus 2: code 9 for id 6, NaN in 7–13 → kind Quad; Reverb renders finite 1 s */ }
#[test] fn with_type_loads_starting_knobs_and_keeps_mix_grit_sync() { /* Hall from Quad: size 0.65, time 0.45, tone 0.3, diffuse 0.4, predelay 0.15, wobble 0.35, rate 0.4, damp 0.3; mix, grit, sync unchanged */ }
#[test] fn knob_memory_restores_a_visited_type_and_clears_on_load() {}
#[test] fn knobs_hit_the_spec_endpoints() { /* every row of every TYPE table, both ends and default, within 1e-3: e.g. Room size 0.5 → 141 ms, Hall decay 0.45 → 2.49 s, Gate length 0.55 → 177 ms, Bloom swell 0.7 → 514 ms */ }
#[test] fn plate_zone_pulls_diffuse_tone_predelay() { /* Hall size 0: diffuse_eff = 1, tone_eff = 0, pre_eff = 0; size 0.15: equal to stored; continuous between */ }
#[test] fn wobble_caps_hold_at_every_size() { /* Room D ≤ 0.0072·L; Hall D ≤ 0.0051·L; Bloom D ≤ 0.0072·L */ }

// reverb_panel_test.rs
#[test] fn every_type_shows_its_cells() { /* Main and More per the spec's knob lists; Quad's More all None; Room's sync steps Off↔Time */ }
#[test] fn labels_and_values_fit_their_cells() { /* incl. short forms PRE, DENS, DIFF; "▾1/8"; "PLATE 120ms" */ }
#[test] fn no_reverb_cell_has_a_hint() { /* View::hint == hint::NONE for every reverb slot under every TYPE (ADR 0086) */ }
#[test] fn midi_map_shows_the_panel_label() { /* slot 3 reads SIZE under Room, LENGTH under Gate */ }
#[test] fn quad_size_still_ticks_by_one_table_step() { /* mixer_page_test's +1/31 holds; static step of slot 3 is 1/128 */ }
```

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p chimera-core --test reverb_params_test --test reverb_panel_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the specs, codes, fixture rows (`B 22 6 TYPE` … `B 22 13 SYNC` and the `E` rows), `with_type`, `KnobMemory`, `knobs`, panels and pages. While Task 7 is pending, `ReverbType` exposes only QUAD on screen: the panel's TYPE list is `&[Quad]`.
- [ ] **Step 4: Run them to see them pass,** along with `disk_codes_test`, `block_test`, `block_spec_test`, `focus_glyph_test` and `mixer_page_test`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "Reverb TYPE, semantic slots, per-TYPE panels, REV › MORE"`.

### Task 7: The switch

**Files:**
- Modify: `chimera-core/src/dsp/rev/mod.rs`, `chimera-stm32/src/bench.rs`
- Test: `chimera-core/tests/reverb_switch_test.rs`

**Interfaces:**
- Produces:
  - `pub enum SwitchPhase { Running, FadeOut { left: u8 }, Clear, FadeIn { left: u8 } }`, with `FADE_BLOCKS: u8 = 15`;
  - `Reverb::phase(&self) -> SwitchPhase`;
  - a latch: `Reverb` keeps `latched: ReverbParams` for FADE OUT.
- Clear: `arena.clear()`, the engine state, and both interpolators' histories, in one block.

- [ ] **Step 1: Write the failing tests.** Until Tasks 8–11 land, they switch QUAD → QUAD through `Reverb::force_restart(&mut self)`, which is `#[doc(hidden)] pub` and runs the same sequence. Each later engine task adds itself to the pair loop.

```rust
#[test] fn every_ordered_pair_switches_without_a_click() { /* mid-tail, MIX 1: max d2 in the switch window ≤ steady neighbours' */ }
#[test] fn clear_block_returns_exact_zero() {}
#[test] fn fade_in_is_bit_identical_to_a_fresh_engine() { /* pre-gain output from the first FADE IN block == fresh engine fed the same send */ }
#[test] fn fade_out_plays_the_latched_knobs() { /* with_type overwrote the slots; FADE OUT == old engine on old knobs, bit for bit */ }
#[test] fn a_switch_during_fade_in_fades_from_the_current_gain() {}
#[test] fn scrolling_types_mid_tail_lands_on_the_last_without_a_click() { /* Review Focus 1: six TYPE changes in 6 blocks */ }
#[test] fn switch_at_mix_0_then_mix_up_has_no_stale_tail() { /* Review Focus 3 */ }
#[test] fn a_project_swap_changing_type_takes_the_same_path() { /* via fx_swap_test's harness */ }
```

- [ ] **Step 2: Run them to see them fail,** with `cargo test -p chimera-core --test reverb_switch_test`. Then implement, and run them again to see them pass.
- [ ] **Step 3: Add `REV SWITCH MAX` (unit).** It is a bench row that keeps the maximum whole-callback block over 2,000 blocks. Six A16 ∪ A17 voices play while TYPE steps QUAD → HALL → GATE → … every 40 blocks; until those exist it steps QUAD → QUAD, forced. Gate: ≤ `BlockBudget::budget_cycles`. If it fails, CLEAR becomes 4 quarter blocks. Run `just flash-bench`, then `just usb bench`, and record the result.
- [ ] **Step 4: Commit.** `git commit -m "Reverb switch: latch, fade out, one-block clear, fade in"`.

### Task 8: BURST: GATE and REVERSE

**Files:**
- Create: `chimera-core/src/dsp/rev/burst.rs`, `chimera-core/tests/reverb_burst_test.rs`
- Modify: `dsp/rev/mod.rs` (add `EngineState::Burst`), `ui/glyph.rs` and `ui/renderer.rs` (TAIL), `ui/reverb_panel.rs` (GATE and REVERSE on screen), `chimera-stm32/src/bench.rs` (`REV GATE` and `REV RVRS` on the new REVERB screen), `tests/fx_golden_test.rs`

**Interfaces:**
- Consumes: `parts`, `ReverbArena`, `EngineKnobs`, and Task 3's kernel, promoted here.
- Produces:
  - `pub struct BurstEngine`, with `fn prime(&mut self, k: &EngineKnobs)` and `fn run(&mut self, arena: &mut ReverbArena, u: &[f32; HALF], k: &EngineKnobs, l: &mut [f32; HALF], r: &mut [f32; HALF])`;
  - `pub const TAP_SEED: u32 = 0x5EED_0343`;
  - `pub fn tap_table(seed: u32) -> [(f32, i8); 12]` (jitter, sign), a `const fn`;
  - `pub fn tail_shape(t: ReverbType, k: &EngineKnobs) -> [u8; 60]` in `ui::glyph`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn tap_table_is_seeded_and_bounded() { /* j_0 = j_11 = 0, |j_k| ≤ 0.3, signs ±1, deterministic for TAP_SEED */ }
#[test] fn weights_keep_sum_of_squares() { /* Σw² within 0.1 dB across SHAPE, CURVE, DENSITY grids */ }
#[test] fn density_fades_taps_continuously() { /* no weight jumps > 1/64 per 1/128 of DENSITY */ }
#[test] fn scheduler_moves_at_most_four_taps_by_120() { /* every crossfade ≤ 4 taps, step ≤ 120 samples, all reach target */ }
#[test] fn gate_window() { /* LENGTH 177 ms: −20 dB end in 185–660 ms; TAIL 0: 50 ms after end ≥ 20 dB under the plateau */ }
#[test] fn reverse_window() { /* envelope peak within ±15 % of LENGTH; 0.3 ≤ |ρ| ≤ 0.75 */ }
#[test] fn regen_settles_on_its_input_pitch() { /* 1 kHz at REGEN 1, LENGTH 150 → 800 ms: settled spectral peak within 1/L Hz */ }
#[test] fn no_slap_after_a_length_move() { /* autocorrelation lags 1–10 ms ≤ −20 dB and ≤ static + 3 dB; 10 ms energy dips ≤ 3 dB */ }
#[test] fn regen_1_falls_to_exact_zero() { /* truncating line write; within 24 s at GRIT 0 and 1 */ }
#[test] fn level_plan_holds_for_burst() { /* held loop-mode sine at 0 dBFS: zero clamp hits, THD ≤ 3 %; 5 kHz held: aliases ≤ −40 dB; −12 dBFS pink at default: bit-identical to no-clip build; at max DECAY: ≤ −40 dB vs f32 reference; swept sine: clamp hits ≤ 0.1 % */ }
#[test] fn burst_noise_within_bound() { /* arena floor ≤ QUAD + 4 dB */ }
#[test] fn burst_loudness_parity() { /* default RMS within ±1 dB of QUAD's (sets WET[Gate], WET[Reverse]) */ }
```

- [ ] **Step 2: Run them to see them fail,** then implement, then run `cargo test -p chimera-core --test reverb_burst_test --test reverb_switch_test` (with GATE and REVERSE added to the pairs) and see them pass.
- [ ] **Step 3: Goldens.** Add `rev_gate`, `rev_gate_flat_tail0`, `rev_reverse` and `rev_reverse_regen` to `fx_golden_test`.
- [ ] **Step 4: Bench (unit).** Run `just flash-bench`, then `just usb bench`. Record REV GATE, REV RVRS and STRESS with GATE. Gate: ΔCOST ≤ headroom and STRESS < budget. Measure flash with `cargo size --release -p chimera-stm32 --target thumbv7em-none-eabihf -- -A` (`.text`), and check the delta is ≤ 2.5 KB.
- [ ] **Step 5: Commit.** `git commit -m "GATE and REVERSE on the BURST engine"`.

### Task 9: LOOP: ROOM

**Files:**
- Create: `chimera-core/src/dsp/rev/loop_engine.rs`, `chimera-core/tests/reverb_loop_test.rs`
- Modify: `dsp/rev/mod.rs` (`EngineState::Room`), `ui/reverb_panel.rs`, `bench.rs` (`REV ROOM`), `fx_golden_test.rs`

**Interfaces:**
- Produces:
  - `pub struct LoopShape { loops: u8, stages: u8 }`, with `pub const ROOM: LoopShape` and `HALL: LoopShape`;
  - `pub struct LoopEngine<const S: LoopShape>`, with `prime` and `run`, the same signatures as `BurstEngine`;
  - `pub fn rotation(theta: f32) -> (f32, f32)`, with `θ = 0.25`.
- ROOM is built with whatever ROOM-T stage Task 3 recorded.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn room_layout_fits_and_lengths_are_distinct_primes() {}
#[test] fn rotation_preserves_energy() { /* gains 1, dampers off: energy constant to 1e-6 over 10 round trips */ }
#[test] fn round_trip_moves_at_most_5_ms_per_crossfade() { /* all stage delays switch in one crossfade; PRE-DLY too */ }
#[test] fn room_settles_on_its_input_pitch() { /* 1 kHz through a full SIZE sweep, then PRE-DLY, then a tempo step: settled peak within 1/L Hz; in-crossfade deviation reported */ }
#[test] fn room_wobble_is_bounded_without_drift() { /* WOBBLE 1, RATE 1, SIZE 0: every pass ±25 cents; per LFO cycle mean ±1 cent */ }
#[test] fn no_slap_after_a_size_sweep() {}
#[test] fn room_window_and_rt60() { /* defaults: RT60 0.55 s ± 30 %, onset ≤ 34 ms; RT60 at DECAY 0, .25, .5, .75, 1 within ±35 % */ }
#[test] fn level_plan_holds_for_room() { /* as Task 8's level test, plus every stored quantity's peak within its § Level plan bound */ }
#[test] fn room_noise_within_bound() { /* ≤ QUAD + 4 dB at defaults, + 6 dB at DIFFUSE 1 */ }
#[test] fn room_loudness_parity() {}
```

- [ ] **Step 2: Run them to see them fail,** implement, and run `--test reverb_loop_test --test reverb_switch_test` to see them pass.
- [ ] **Step 3: Golden** `rev_room`.
- [ ] **Step 4: Bench (unit).** Record REV ROOM and STRESS, and check ΔCOST against 96 and 23. If it is still over, apply the next ROOM-T stage and re-record. Then record flash.
- [ ] **Step 5: Commit.** `git commit -m "ROOM on the LOOP engine"`.

### Task 10: HALL and the plate zone

**Files:**
- Modify: `loop_engine.rs` (`HALL`), `dsp/rev/mod.rs`, `ui/reverb_panel.rs` (the `PLATE` value text), `bench.rs` (`REV HALL`), `fx_golden_test.rs`, `tests/reverb_loop_test.rs`

**Interfaces:**
- Consumes: `LoopEngine`, and `knobs`' plate zone from Task 6.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn hall_reaches_10_s_at_size_0() { /* stage cap 0.985 */ }
#[test] fn plate_corner_onset_is_short() { /* SIZE 0, DECAY .35, TONE 0, DIFFUSE 1, DAMP .1, PRE 0, WOBBLE .5, RATE .6: onset ≤ 8 ms */ }
#[test] fn plate_zone_moves_predelay_by_crossfade() { /* SIZE 0.15 → 0 with PRE 0.5: PRE length changes only in ≤ 5 ms crossfade steps */ }
#[test] fn hall_decorrelates() { /* defaults: |ρ| ≤ 0.2 */ }
#[test] fn hall_settles_on_its_input_pitch() {}
#[test] fn level_plan_holds_for_hall() {}
#[test] fn hall_noise_and_loudness() {}
```

- [ ] **Step 2: Run them to see them fail,** implement, then see them pass.
- [ ] **Step 3: Goldens** `rev_hall` and `rev_hall_plate`.
- [ ] **Step 4: Bench (unit)** for REV HALL, STRESS and flash.
- [ ] **Step 5: Commit.** `git commit -m "HALL and its plate zone"`.

### Task 11: BLOOM

**Files:**
- Create: `chimera-core/src/dsp/rev/bloom.rs`, `chimera-core/tests/reverb_bloom_test.rs`
- Modify: `dsp/rev/mod.rs`, `ui/reverb_panel.rs`, `bench.rs` (`REV BLOOM`), `fx_golden_test.rs`

**Interfaces:**
- Produces: `pub struct BloomEngine`, with `prime` and `run`, the same signatures as `BurstEngine`.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn bloom_window() { /* defaults: onset 140–300 ms, peak 450–750 ms */ }
#[test] fn swell_moves_one_delay_by_crossfade() {}
#[test] fn bloom_settles_on_its_input_pitch() {}
#[test] fn bloom_wobble_is_bounded_without_drift() { /* ±25 cents, 0/90/180° APs */ }
#[test] fn level_plan_holds_for_bloom() {}
#[test] fn bloom_noise_and_loudness() { /* ≤ QUAD + 4 dB default, + 6 at DIFFUSE 1 */ }
```

- [ ] **Step 2: Run them to see them fail,** implement, then see them pass. The split-SWELL fallback is not built unless the A/B (Task 13) asks for it; in the tape build it also needs an owner ruling.
- [ ] **Step 3: Golden** `rev_bloom`.
- [ ] **Step 4: Bench (unit),** then **commit**: `git commit -m "BLOOM"`.

### Task 12: Tempo sync

**Files:**
- Create: `chimera-core/src/dsp/rev/sync.rs` (it gains the division lists and `fold`), `chimera-core/tests/reverb_sync_test.rs`
- Modify: `chimera-core/src/instrument.rs` (`pub fn set_tempo(&mut self, t: Tempo)`, a `tempo` field passed into `mix_parts`), `chimera-core/src/orbit/runner.rs` (`OrbitRunner::tempo(&self) -> Tempo`, from the leading clock's `bar_len()/4` or the internal `Bpm`, and `inst.set_tempo` in `block`), `dsp/reverb.rs` (`with_sync`), `ui/reverb_panel.rs` (synced steps and the `▾`/`▴` marks), `fx_golden_test.rs`

**Interfaces:**
- Produces:
  - `pub struct Division { pub num: u8, pub den: u8, pub kind: DivKind }`, with `DivKind { Straight, Triplet, Dotted }`;
  - `pub const ROOM_PRE: &[Division]`, `HALL_PRE`, `GATE_LENGTH`, `REVERSE_LENGTH` and `DECAY_BEATS: &[u8] = &[1, 2, 3, 4, 6, 8, 12, 16, 24, 32]`;
  - `pub struct FoldState(i8)` (halvings, negative for doublings);
  - `pub fn fold(len: f32, lo: f32, hi: f32, st: &mut FoldState) -> f32`, with 2 % hysteresis;
  - `impl Tempo { pub fn from_bpm(bpm: f32, sr: u32) -> Self; pub fn quantised(self, held: Tempo) -> Tempo }`, in 0.25 BPM steps that move only 0.15 BPM past the held one.

- [ ] **Step 1: Write the failing tests.**

```rust
#[test] fn division_lists_sit_in_range_at_120() {}
#[test] fn fold_halves_and_doubles_into_range_30_to_300_bpm() {}
#[test] fn fold_does_not_flap_on_jitter() { /* GATE 1/4 at 100 BPM ± 1 %: one state throughout */ }
#[test] fn tempo_quantiser_holds_through_jitter() { /* ±0.1 BPM → unchanged */ }
#[test] fn decay_in_beats_clamps_and_room_has_no_all() {}
#[test] fn sync_toggle_keeps_the_length_within_one_division() {}
#[test] fn division_encoding_round_trips() { /* p = i/(n−1), i = round(p(n−1)) */ }
#[test] fn clock_dropout_moves_synced_length_once() { /* Review Focus 5: EXT 100 BPM, ticks stop 600 ms, internal 120 takes over: exactly one target change, crossfaded, no click */ }
#[test] fn gate_sync_eighth_at_120_is_250_ms() {}
```

- [ ] **Step 2: Run them to see them fail,** implement, then see them pass. Run `cargo test -p chimera-core` (ORBIT's tests included).
- [ ] **Step 3: Golden** `rev_gate_sync_8th_120`.
- [ ] **Step 4: Commit.** `git commit -m "Reverb tempo sync on ORBIT's clock"`.

### Task 13: Sweeps, A/B, final bench and the ADR (unit)

**Files:**
- Modify: `chimera-core/tests/param_sweep_test.rs`, `chimera-core/tests/screen_golden_test.rs` (REV and REV › MORE under QUAD, HALL and GATE), `tools/ab/render.sh`, `docs/adr/0085-the-master-reverb-picks-a-type.md` (§ Consequences: measured CPU and flash), plan § Measured

- [ ] **Step 1: Make the sweeps TYPE-aware.**
  - `sweep_fast` sweeps every present knob under every TYPE. `sweep_thorough` adds every SYNC at 30, 120 and 300 BPM.
  - Allowed behaviours, each listed with its reason: TYPE's switch dip, and TAIL 0 and REGEN 0 short tails.
  - Run: `cargo test -p chimera-core --test param_sweep_test`, then `cargo test -p chimera-core --test param_sweep_test -- --ignored`. Expected: PASS.
- [ ] **Step 2: Screen goldens.** Run: `just screens`, then `cargo test -p chimera-core --test screen_golden_test`. Expected: PASS, with new images for the six pages.
- [ ] **Step 3: The A/B renders.**
  - Render the spec's eleven pairs on Chimera's side to `target/ab/`.
  - Write `target/ab/report.md` with the measurement table: RT60 (T20), onset (1 ms RMS > −40 dB rel. peak), peak, −20 dB end, tilt and ρ.
  - Leave the VST column blank for the owner, and hand it over.
- [ ] **Step 4: Final bench (unit).**
  - On both builds, record every REV row, BUS+V with the costliest TYPE, STRESS with each TYPE, and REV SWITCH MAX.
  - Set `FxBus::COST` to the costliest TYPE's BUS+V reading if it exceeds `COST`, update `cost_test`'s `bus` literal, and run `cargo test -p chimera-core --test cost_test` on both builds. Expected: PASS.
  - Record the flash total, with `cargo size` on the release build.
- [ ] **Step 5: The ADR.** Record the measured CPU, flash and headroom in ADR 0085 § Consequences. Its status stays Proposed until the owner's A/B and UAT pass.
- [ ] **Step 6: Commit.** `git commit -m "Reverb TYPEs: sweeps, screens, A/B renders, measured costs"`.

---

## Measured

(Tasks 1, 3–5 and 7–13 fill this in: row, reading, build, commit, date.)

---

## Self-review

- **Spec coverage.** Every spec item maps to a task:
  - TYPE list and codes: Task 6.
  - Engines: Tasks 8–11.
  - Moving lengths and pitch bounds: Tasks 2 and 8–11.
  - The level plan: Tasks 2 and 8–11.
  - GRIT: Task 5.
  - The arena and f32 regions: Tasks 2 and 4.
  - Parameters, `with_type` and memory: Task 6.
  - The switch: Task 7.
  - Smoothing: Tasks 2 and 6–7.
  - Wobble: Tasks 2 and 9–11.
  - Modulation: deferred, nothing to build.
  - Tempo sync: Task 12.
  - UI, glyph and hints: Tasks 6 and 8.
  - CPU, the step 0 gates, ROOM-T and the bench rows: Tasks 1, 3, 7 and 8–13.
  - Flash: Tasks 8–13.
  - Storage: Task 6.
  - Tests: per task.
  - A/B: Tasks 5 and 13.
  - Build order: Tasks 1–3 are step 0, Tasks 4–5 are 1a, Tasks 6–7 are 1b, then 2 → 8, 3 → 9, 4 → 10, 5 → 11, 6 → 12 and 7 → 13.
- **Steps.** Every step names a signature, a test with its assertions, or a command with its expected result. Bodies are given only for the TPDF and idle-mute rule, the soft clip and ROOM-T.
- **Types.** `EngineKnobs`, `Tempo`, `FoldState`, `ReverbArena`, `EngineState`, `SwitchPhase`, `RevPage`/`RevKnob` and `KnobMemory` are defined once and used with the same names after.
- **Review Focus.** Each of the five lines has its test in Tasks 6, 7, 5, 7 and 12.
- **Proportion.** The plan is about 500 lines against the spec's 1,150. Its code blocks are test signatures and assertions only.
