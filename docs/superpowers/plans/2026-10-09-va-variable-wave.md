# VA Variable-Wave Core Implementation Plan

**Goal:** Add VA, Chimera's fourth engine: two band-limited variable-wave oscillators (WAVE morph + SHAPE), five combine modes (MIX, SYNC, RING, XOR, BITS) with a ducked MODE change, a sub an octave below OSC 1 and STABILITY (drift, trim, HF tracking, jitter), plugged in on TAPE's pattern.

**Architecture:**
- **Techniques first, pure and rate-independent.** These files take no product types, set their coefficients in `prepare(rate)` and each declare `PROVENANCE`. When ADR 0114 is accepted they move to `chimera-dsp` unchanged.
  - `dsp/va/blep.rs`: the 2-point polyBLEP and polyBLAMP, and a one-sample-late `Corrector`.
  - `osc.rs`: the variable-wave oscillator, sync and the sub.
  - `stability.rs`: walks, trim, HF tracking, jitter and seeding.
- **Then the product layer** in `chimera-engine` (ADR 0108 opt 2):
  - `combine.rs`: the five operators.
  - `mod.rs`: `VaParams`, and `VaEngine` (per-block pitch, per-sample ramps, the MODE duck, the carry across a slot rebuild).
- **Then the plumbing, TAPE's way (ADR 0093):**
  - `BlockRef::Va` (block code 35), `EngineType::Va` (engine code 3) and a `SlotKind::Va` arm that carries VA's state as TAPE carries its strip;
  - the VA chain's three pages under one node;
  - host-counted bills, and `VA_OUT_GAIN` at the reference loudness.

**Tech Stack:** Rust `no_std` (`chimera-engine`, `chimera-core`), host tests in `chimera-core/tests`, `tools/loop_count.py` (ADR 0056 count) on the `thumbv7em-none-eabihf` release ELF.

**Spec:** `docs/superpowers/specs/2026-10-09-va-variable-wave-design.md`.
- It is a Draft, revised after the adversarial review (private notes). The owner approved the direction and the HF-tracking amendment (f85cb91b); the revision awaits the owner's review, and no task starts before it.
- Read the spec, `CLAUDE.md` and TAPE's plan (`docs/superpowers/plans/2026-10-05-tape-core.md`, the template) before any task.
- Codebase map: (private notes) § 1 and § 4.
- The numbers marked *sim* come from f64 simulations in the session scratchpad (`vafix/walk.rs`, `wave.rs`, `exp.rs`, `ring7.rs`, `pw.rs`, `split.rs`); f32 may lose up to about 1 dB of them.

**Decided here (the spec is silent or loose):**
- **The alias gate** is the 09-27 draft's: 2¹⁵ samples after a 2¹² settle, a Blackman–Harris 4-term window, and bins within ±4 of every multiple of each oscillator's frequency (and f0/2 with SUB) masked. The rest, from 20 Hz to 10 kHz, must be ≤ **−45 dB** of the total.
  - Where the uncorrected render folds more than −60 dB, the corrected one is also ≥ 12 dB below it (a test-only `Correction::Off`).
  - MIX runs at FINE2 = 0, so one series is masked. SYNC runs at OSC 2 CRSE +7 and at 2.37×.
  - RING runs detuned at C5, with `alias_db_lattice` masking ±4 bins around every |i·f1 + j·f2| up to 24 kHz.
- **The pitch method** is the draft's: a 2¹⁷-point Hann DFT, quadratic interpolation on the log magnitude.
- **The waves** are those of spec § Oscillator, with w the window width and every edge at a fixed place in the cycle:
  - **sine:** `g_s(s)·((1−s)·sin θ + s·sin((1+2s)·sin θ))`, with `g_s = R / SINE_RMS(s)`. `SINE_RMS` is a 17-point table at s = k/16, lerped: `0.7071 0.7052 0.7090 0.7171 0.7279 0.7398 0.7512 0.7607 0.7668 0.7686 0.7650 0.7558 0.7409 0.7209 0.6974 0.6731 0.6517`.
  - **triangle:** w = ½ + 0.45s, and `p′ = frac(p + w/2)`. It is `R√3·(−1 + 2p′/w)` for p′ < w, else `R√3·(1 − 2(p′−w)/(1−w))`. Its corners are at −w/2 (trough) and +w/2 (peak).
  - **saw:** two saws, `a·saw(p − e₁) + b·saw(p − e₂)`, with `saw(x) = 1 − 2·frac(x)`, `a = 1 − s/2`, `b = s/2`, `q = s/4`, `e₁ = −q·b` and `e₂ = q·a`.
    - Its steps are `+2a·g` at e₁ and `+2b·g` at e₂.
    - `g = R / √((a²+b²)/3 + 2ab·(1/3 − 2q + 2q²))`.
  - **pulse:** w = max(½ − 0.45s, inc), and `g = R/(2√(w(1−w)))`. It is `2(1−w)·g` on `[¼ − w/2, ¼ + w/2)`, else `−2w·g`. Its steps are `+2g` at ¼ − w/2 and `−2g` at ¼ + w/2.
  - **sim:** level within 0.01 dB of R across SHAPE; |DC| ≤ 9·10⁻⁵; peak ≤ 0.872; every anchor's fundamental at 0° at SHAPE 0, ½ and 1.
- **Gains and reciprocals:**
  - The sine's and the saw's gains are smooth in s, so they are computed once a block and lerped per sample.
  - The pulse's `√(w(1−w))` and the triangle's `1/(w(1−w))` are computed exactly per sample (`vsqrt`, `vdiv`) while that oscillator's SHAPE ramp moves, and held as block constants while it is still. A lerped reciprocal leaves a step at the triangle's peak.
  - A peak bound `pk` per oscillator per block is Σ weight × each wave's peak (sine `g_s`, triangle `R√3`, saw `g`, pulse `2(1−w)g`), the larger of the block's two ends.
- **Events**, every sample, for each live wave's thresholds (the wrap, the steps, the corners):
  - Each threshold keeps its own last position, per wave and per edge: never another threshold's, and not across a WAVE segment change. A wave entering the morph starts at its current positions, with weight 0.
  - With `d = phase − threshold` (unwrapped), a sign change from d₀ to d₁ is an event at `t = d₁/(d₁ − d₀)` samples ago. The step is multiplied by `sign(d₁ − d₀)`.
  - A corner's slope jump is its two branches' difference in `∂y/∂p·inc + ∂y/∂w·Δw`.
  - Step and slope are lerped to the crossing instant (from the last sample's to this sample's values).
  - Events at the same place (two waves' wraps) may be merged into one `edge` call.
- **A SYNC reset sample** is split in three parts (spec § Combine):
  1. The slave's events in `(φ_prev, φ⁻]` are corrected at `t + (φ⁻ − e)/inc`.
  2. The reset gets `blep` × `y(0) − y(φ⁻)` and `blamp` × `(y′(0) − y′(φ⁻))·inc`, both from `wave_at`.
  3. Events in `(0, t·inc]` are corrected at `(t·inc − e)/inc`; phase 0 is not one.
  - The sample's SHAPE values hold through it (no threshold motion), and only the slave's natural wraps draw jitter.
- **Corrections superpose:** `Corrector::edge` may be called any number of times per sample.
- **RING is the plain product of the corrected oscillators.** A per-interval cross term (exact for coincident steps) was simulated: −31 → −62 dB phase-locked, but 5 dB worse detuned (sim). So it is a follow-up issue (Task 11), not built.
- **The sine fold has no ADAA:** its input is the phase ramp, and Jacobi–Anger bounds its alias (spec § Oscillator).
- **BITS' log curve:** `bits(x) = 16 · 8^(−x)` (16, 8, 4 and 2 at x = 0, ⅓, ⅔ and 1), continuous. `levels = 2^(bits − 1)` and `y = round(v·levels)/levels`.
- **Increments clamp to `[1e-6, 0.45]`** cycles a sample. The upper bound is the draft's rule, so note 127 + CRSE +24 + PITCH +24 stays finite. The floor keeps `t = d/inc` finite under the chip's FTZ.
- **Clamping:** every clamp is `max(lo).min(hi)` (voice.rs's idiom), never `f32::clamp`, so a NaN clamps too.
- **The Part reaches the engine through `EngineCtx`** (`part: PartId`). `VaEngine` seeds its character from (Part, voice slot) when the Part differs from the one it was last seeded for.
- **The carry:** `EngineSlot::build`'s Va arm takes `carry()` from a Va slot before it rebuilds, and gives it back with `restore`, as TAPE's strip does.
  - A carry holds both phases, the sub's flip-flop, `Stability` (walks, seeds, character) and the seeded Part. The correctors and the ramps start fresh.
  - Phases are kept, not advanced by the idle time: the slot has no clock, and the last note's length already decorrelates them.
  - A slot rebuilt from another engine calls `place(voice)` and seeds at its next note.
- **The VA node is VA ↓ OSC 1 ↓ OSC 2 ↓ PIT.** PIT is the voice-pitch sub-page every engine node carries (ADR 0042); the spec's three pages come first.
- **No hints and an arc on WAVE.** VA adds no MIX or hold gesture, so under ADR 0086 no cell has a hint (spec § Pages, an owner call in the spec review). WAVE keeps the default `FocusGlyph::Arc`; `Wave` would draw an ALGO table.
- **Value bars come free:** the cell reads its spec's `Motion` (ADR 0112 point 8): bars on the nine continuous cells, none on CRSE and MODE.
- **`Provenance`** has no type on `main` (ADR 0114 is Proposed).
  - Task 2 adds ADR 0114's closed enum in `chimera-engine/src/dsp/provenance.rs`, with the three variants VA uses: `Textbook { cite }`, `CircuitModel { circuit, refs, check }` and `Original { author }`.
  - It adds a separate `Patent` line per technique under 20 years old, and a `GPL_CHECK` note per file.
  - It moves to `chimera-dsp` with the technique files.
- **Bills:**
  - Task 6 commits paper estimates: MIX 360, SYNC 410, RING 370, XOR 380, BITS 380, each with LP24's 95 included.
  - Task 9 replaces them with host counts: bill = the per-sample fast path + Σ (event cost × the event's maximum rate at MAX_INC) + per-block work ÷ 64 + 95.
  - The maximum rates at MAX_INC 0.45 are 1.8 events a sample per oscillator (4 a cycle) and 0.45 resets a sample.
  - The moving-SHAPE path is billed always.
- **Neutral vs INIT:**
  - `VaParams::default()` (neutral, frozen) has STAB 1 and FINE2 0.
  - `Sound::init(Va)` writes FINE2 +6 and STAB 0.7 (preset.rs), so they can change by ear without a migration.
- **The neutral fixture stays one text.** `neutral_is_a_contract` maps its one `kind: Svf,` line to `FilterKind::default_for(e)` per engine, so no existing line changes.

**Rejected (one line each):**
- **m14, tests as code:** a one-line test comment that carries its exact values and bounds is unambiguous. Code blocks for ~80 tests would make the plan a transcript (writing-plans proportion). The new criteria (B2, M3, M7, M8) are spelled out with values.
- **m16, the band to 20 kHz:** above 10 kHz the 2-point kernel is weak by construction (pulse at C7: −26 dB to 20 kHz, sim). The 4-point kernel is the named upgrade, an owner call. The band stays the draft's perceptual 10 kHz.
- **m18, a desktop listening pass:** the desktop runs the same `Instrument` the tests drive. Task 10 builds it and the owner listens at UAT; a VA listen tool is not this plan.

## Review resolutions

| Id | Resolution |
|---|---|
| B1 | VA's state rides `build` as TAPE's strip does (`carry`/`restore`, Decided here). Phases are kept through idle; the walks run on. Tests: `a_voice_va_runs_on`, `stab_one_replays_on_an_idle_voice` (Task 8), `carry_restores_the_state` (Task 6). |
| B2 | The walk is two one-poles at 0.2 Hz with a closed-form variance gain (RMS 0.30, independent of rate). The bounds come from simulation with margin (Task 5). |
| B3 | The RING gate runs detuned (+6 cents at CRSE2 0 and +12; WAVE2 = WAVE1 or triangle) at C5, lattice-masked, SHAPE 0 and ½ (sim worst −49.6 dB). Coincident edges and RING's 5% pulse are documented exceptions. The XOR tests use the +6-cent inputs. |
| M1 | Seeds come from a splitmix32 hash of (Part, slot, salt), with fixed salts. Test: `seeds_spread` (std ≥ 0.25, pairwise > 10⁻³; sim min 0.309, 0.0014). |
| M2 | The spec is restated: STAB 1 is bit-identical; below 1 the character recalls, and drift and phase differ per take. Tests at the voice level (Tasks 6, 8). |
| M3 | WAVE/SHAPE sweeps are measured against the triangle-kernel reference (≤ 2·10⁻³, sim 6.3·10⁻⁴); a 0.005 step fails it (`the_reference_catches_a_step`). The other knobs use sine-only Δ² (+10⁻⁶ floor); a 0.002 step fails it. |
| M4 | One RMS R = 0.2 for every wave and SHAPE, DC-free; a fundamental-preserving split saw; RMS-safe operators. Tests: level ±1 dB across SHAPE, ≥ −2.5 dB across WAVE, DC ≤ 10⁻³R, fundamental within 1 dB of the anchor lerp. |
| M5 | `default_for(Va) => K2040` (ADR 0096 as accepted, no supersede); the code comment is fixed; INIT and the budget carry REV2's +15; the neutral fixture is mapped per engine. |
| M6 | The rooms are 830/636/802/779; the estimate is 500–650; the billing method is the reviewer's; `tick` is `#[inline(always)]`; the moving path has exact `vdiv`/`vsqrt`. |
| M7 | The three-part split, with a test of fixed values (`sync_splits_around_the_reset`). |
| M8 | Signed crossings with t from both motions. Tests: `a_threshold_crossing_backwards_is_an_edge`, `fast_shape_at_c1_tracks_the_reference` (≤ 0.02; sim 8.2·10⁻³, forward-only 0.18). |
| M9 | STAB 1 resets phase only on an idle voice. Tests: `a_restrike_keeps_phase` (Task 6), the re-strike click rule. |
| M10 | WAVE uses the default arc; a morph scope is a follow-up issue. Test: `wave_cell_draws_an_arc`. |
| m1 | Shorts WV1 WV2 CR1 FN1 SH1 CR2 FN2 SH2 (Task 1); the matrix header test is listed in Task 1's verify. |
| m2 | FINE is `ParamSpec::stepped(.., Motion::Continuous(KNOB)).cents(100.0)`, PITCH FINE's precedent. |
| m3 | ADR 0114's variants; HF tracking and scale error are `CircuitModel`; a `PATENT` line and the `GPL_CHECK` clean-room note. |
| m4 | INIT is decoupled from neutral (neutral STAB 1, FINE2 0; INIT writes 0.7 and +6). |
| m5 | No hints, stated in spec § Pages as an owner call in the spec review. |
| m6 | The spec stays Draft until the owner approves it; Task 11 no longer flips it. |
| m7 | `project/parts.rs` (`part_block`, `part_block_mut`) and `engines.rs` `rebuild`'s `old` match are in Task 8's files. |
| m8 | `max/min` clamps, and the increment floor at 10⁻⁶ (Decided here; `out_of_range_values_clamp` adds NaN). |
| m9 | The sub is ±R, mixed with `/√(1+SUB²)`. |
| m10 | XOR's DC is documented (spec § Combine), by design. |
| m11 | BITS' block-rate step is documented as exempt (spec § Combine). |
| m12 | The STAB = 1 threshold is recorded in ADR 0118 (Task 11). |
| m13 | The onset step at STAB 1 is documented (spec § Stability), by design. |
| m14 | Fixed: the recall tests are now voice-level; the drift bound tests the distribution, not the clamp; `hf_tracking` compares in f64 with \|Δu\| > 0.05; "0 vs 1" is min vs max (FINE ±100 cents). Tests as code: rejected above. |
| m15 | The loudness is measured over 4 whole beats of the +6-cent pair at C4 (4.415 s). |
| m16 | Margins are reported per gate (SYNC's is thin: stop and report). The 20 kHz band is rejected above. |
| m17 | `Knobs::COST` 12 → 13 (the re-read scales 66 → 75 knobs). The AXI and D2 headroom are in Task 8's verify. |
| m18 | Task 10 builds the desktop. The listening pass is rejected above. |
| m19 | `Unit` newtype and `Morph { wave: Unit, shape: Unit }`. |
| m20 | Task 9 reports the flash delta of the five loops. Over 8 KB, RING/XOR/BITS share one loop. |
| m21 | Task 8 checks the goldens and CRCs pinned on `test_support::full()` and re-records only what moved. |
| m22 | The spec wording is fixed (events are conditional work, billed per event). |

## Global Constraints

- **Code rules (CLAUDE.md):** no heap, blocking or panic path in audio; no libc; no `unsafe` without a true `// SAFETY:`; parameters lerp and never snap (the owner's never-snap and filter-never-steppy rules).
- **Type-driven:** `VaMode` (`#[repr(u8)]`, 0 MIX, 1 SYNC, 2 RING, 3 XOR, 4 BITS; an unknown code loads as MIX), `OscId { One, Two }`, `Seed`, `Unit` (an `f32` in 0..1, NaN → 0), `Morph { wave: Unit, shape: Unit }`. No bare `u8` mode or `usize` oscillator index in the product layer.
- **Every param declares `Motion` (ADR 0112):**
  - WAVE1/2, SHAPE1/2, AMOUNT, SUB, STAB and FINE1/2 are `Continuous(Smoothing::KNOB)`.
  - CRSE1/2 are `Stepped(Change::Snap)`, and MODE is `Stepped(Change::Crossfade)` (the duck).
  - Eleven are mod destinations; MODE is not.
- **Names:**
  - `EngineType::Va` (code 3, ident `"VA"`, label `"VA"`) and `BlockRef::Va` (code 35, ident `"VA"`);
  - `SlotKind::Va`, and `EngineSlot::Va(VaEngine) => rebuild_va, init_va`;
  - `VA_OUT_GAIN`, `VA_SPECS`, `VaParams`, `VaCarry`, and the directory `dsp/va/`.
  - Param ids, frozen once written: WAVE1 0, WAVE2 1, MODE 2, AMOUNT 3, SUB 4, STAB 5, CRSE1 6, FINE1 7, SHAPE1 8, CRSE2 9, FINE2 10, SHAPE2 11. The idents are the same tokens (`"STAB"` for STABILITY).
  - MODE's idents are `MIX SYNC RING XOR BITS`.
- **Neutral (a format contract, ADR 0087):** WAVE ⅔ both, SHAPE 0 both, CRSE 0, FINE 0 both, MIX, AMOUNT ½, SUB 0, STAB 1; filter 2040 (`FilterKind::K2040`, `FilterMode::Lp24`, VOICE REV2).
- **INIT (`Sound::init(Va)`):** neutral plus FINE2 +6, STAB 0.7, and ENV 1 → VCA at 127.
- **Spec constants:**
  - R = 0.2, and an oscillator peak ≤ 0.9;
  - `d = (1 − STAB)²`;
  - walks: two one-poles at 0.2 Hz, RMS 0.30, clamp ±1; drift ×12 cents shared and ×6 own;
  - trim ±3 cents, jitter ±0.5 cents a cycle;
  - HF tracking `f_eff = f/(1 + f·τ)`, `τ = d · 4 µs · u`, u seeded in −0.25…1 per oscillator (4 µs is an estimate from the CEM3340 datasheet, not a measurement);
  - scale error `ε = ±0.2 % · d` per oscillator on pitch about C4;
  - sub ±R, with `y = (c + SUB·s)/√(1 + SUB²)`.
- **Budget:** the rooms beside the reserve are rev V 4,984, rev Y 3,817, rev V + ORBIT 4,813 and rev V + master tape 4,674 (830, 636, 802 and 779 a voice at 6). The worst VA voice leaves 6 on each (ADR 0040), counted by the ADR 0056 method (`tools/loop_count.py`), with LP24's 95 carried in each engine term.
- **Memory:**
  - `VaEngine` is no larger than `ModalEngine` (the slot must not grow).
  - The voice chain stays within `VOICE_CHAIN_BYTES` 2,112 (ADR 0101; 2,016 today), and a Sound record within 640 B (ADR 0102).
  - `MAX_SOUND_COST` is re-pinned by `sparse_bound`.
  - The AXI headroom (the bench builds sit within 8 KiB of full) and D2 under the knobs publisher (it grows with `KNOB_COUNT`) are checked from `rust-size`.
  - If an assert fails, stop and report: never raise a budget in passing.
- **Commits:** a terse subject, no type prefix. Stage by path, never `git add -A`; never stage `chimera.bin` or `docs/chimera-ui-ux-spec.md`.
- **Stale cache:** `touch chimera-engine/src/lib.rs` before each test run that follows an engine edit.
- **Flash:** each firmware-touching task reports one line, `flash: <free> B free (<±delta>)`, from `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf` then `rust-size target/thumbv7em-none-eabihf/release/chimera-stm32`. Cut only real waste; escalate only if free flash falls under ~40 KB.
- **Knobs:** VA's nine knobs join the `KNOBS` registry (66 + 9 = 75 ≤ 128).
- **No hardware.** Host only. The on-unit bench rows come at ship (Task 11 note).
- **Verification:** `just test`, `just clippy`; `just check` green at the end.

## Review Focus

1. **Pitch at the MIDI extremes** (note 127, CRSE +24, PITCH +24, FINE +100; note 0, CRSE −24): every increment stays within `[1e-6, 0.45]`, the output is finite and |y| ≤ 0.95. *Task 6: `midi_extremes_stay_finite`.*
2. **Routes that push a param past its range** (an LFO at full into WAVE, SHAPE, AMOUNT, SUB or STAB stored at 1, or at 0, or a NaN): the engine clamps each to 0..1; the output is finite and peak-safe, with no NaN from `sin`, `exp2`, `sqrt` or the crush. *Task 6: `out_of_range_values_clamp`.*
3. **MODE scrolled every block under a held note** (a fast encoder): no click, and the voice settles on the last MODE within two blocks of the last turn. *Task 6: `mode_scrolled_every_block_settles`.*
4. **A voice that moves between Parts and engines** (VA on Part 1, then TAPE, then VA on Part 2): the slot rebuilds in place, the character follows Part 2's seed, and Part 1 played on the same slot after a fresh boot has Part 1's character. *Task 8: `a_voice_moving_parts_takes_the_parts_character`.*
5. **ENV 1 → VCA deleted:** VA's raw output never decays, so the key ends the note through the voice fade (TAPE's `key_gates`), and no voice sticks. *Task 8: `va_with_no_vca_route_ends_at_key_up`.*

---

### Task 1: The VA params block, on disk

Spec § Pages, § Params and Motion, § Voice and engine (codes, neutral).

**Files:**
- Create: `chimera-engine/src/dsp/va/mod.rs` (`VaParams`, `VaMode`, `OscId`, `VA_SPECS`, `MODE_NAMES`, `impl Block`; later tasks add the engine).
- Modify:
  - `chimera-engine/src/dsp/mod.rs`: `pub mod va;`.
  - `chimera-engine/src/addr.rs`: `BlockRef::Va` in the enum, `ALL` [34 → 35], `specs`, `in_sound` → true, `voice_reads` → true.
  - `chimera-engine/src/storage/codes.rs`: `disk_code` 35, `disk_ident` `"VA"`.
  - `chimera-engine/src/params.rs`: `ParamSnapshot.va: VaParams` after `tape`, and its arms in `BlockRead::block`, `Blocks::block_mut` and `Default`.
  - `chimera-engine/src/project/parts.rs`: the exhaustive `part_block`/`part_block_mut` arms (:231, :265), `BlockRef::Va` → None.
  - `chimera-engine/src/storage/sound.rs`: `MAX_SOUND_COST` and its doc's figures.
  - `chimera-engine/src/dsp/knobs.rs`: `Knobs::COST` 12 → 13. Its doc gets the re-read 6,000 × 75/66 ≈ 6,820 → 818/64 = 12.8.
  - `chimera-core/src/ui/mod_grid.rs`: `block_tag` `"VA"`, `block_name` `"VA"`.
- Fixtures:
  - `chimera-core/tests/fixtures/disk_codes_v1.txt`: append block 35's rows.
  - `chimera-core/tests/fixtures/neutral_v1.txt`: the `va: VaParams { … }` field after `tape`. No existing line changes.
- Test:
  - `chimera-core/tests/va_params_test.rs` (new);
  - `addr_test.rs`: `modulatable_addresses_are_exactly_the_spec_list` gains VA's eleven;
  - `sound_codec_test.rs`: `sparse_bound`'s `2284` becomes the measured worst.

**Interfaces:**
- Produces (`dsp::va`):
  - `pub enum OscId { One = 0, Two = 1 }`, `OscId::ALL`, `const fn index(self) -> usize`.
  - `#[repr(u8)] pub enum VaMode { Mix = 0, Sync = 1, Ring = 2, Xor = 3, Bits = 4 }`, `VaMode::ALL`, `impl DiskCode` (idents above).
  - `pub const MODE_NAMES: [&str; 5] = ["MIX", "SYNC", "RING", "XOR", "BITS"]`.
  - `pub struct VaParams { pub wave: [f32; 2], pub shape: [f32; 2], pub coarse: [i8; 2], pub fine: [f32; 2], pub mode: VaMode, pub amount: f32, pub sub: f32, pub stability: f32 }`, indexed by `OscId::index`. `Default` = neutral (Global Constraints). Id consts `VaParams::WAVE1 … SHAPE2`.
  - `pub static VA_SPECS: [ParamSpec; 12]`:
    - WAVE/SHAPE/AMOUNT/SUB/STAB: `ParamSpec::continuous(id, label, ValFmt::Uni, 0.0, 1.0, default, 1.0 / 128.0, true, Smoothing::KNOB)`.
    - FINE: `ParamSpec::stepped(.., ValFmt::Signed(100), −100.0, 100.0, 0.0, true, Motion::Continuous(Smoothing::KNOB)).cents(100.0)` (PITCH FINE's precedent).
    - CRSE: `ParamSpec::stepped(.., ValFmt::Signed(24), −24.0, 24.0, 0.0, true, Motion::Stepped(Change::Snap)).semitones(24.0)`.
    - MODE: `ParamSpec::choice(2, "MODE", ValFmt::Names(&MODE_NAMES), 4.0, 0.0, Change::Crossfade).glyph(FocusGlyph::None)`.
    - Labels: `WAVE1 WAVE2 MODE AMNT SUB STAB CRSE1 FINE1 SHAPE1 CRSE2 FINE2 SHAPE2`.
    - Matrix shorts: `.short("WV1")`, `"WV2"`, `"CR1"`, `"FN1"`, `"SH1"`, `"CR2"`, `"FN2"`, `"SH2"`.
    - Glyphs: AMOUNT `Crossfader`, SUB `LevelBar`; WAVE keeps the default `Arc`.
  - `impl Block for VaParams`: CRSE writes `round(v).max(−24).min(24) as i8` (a modulated CRSE is stepped); MODE goes through `enum_code`/`enum_ident`/`set_enum_code`.

- [ ] **Step 1: Write the failing tests** (`va_params_test.rs`):

```rust
#[test] fn neutral_is_the_spec() {
    let p = VaParams::default();
    assert_eq!((p.wave, p.shape, p.coarse, p.fine), ([2.0 / 3.0; 2], [0.0; 2], [0; 2], [0.0; 2]));
    assert_eq!((p.mode, p.amount, p.sub, p.stability), (VaMode::Mix, 0.5, 0.0, 1.0));
}
#[test] fn every_param_declares_its_motion() // the nine Continuous(KNOB), CRSE1/2 Stepped(Snap), MODE Stepped(Crossfade); modulatable exactly the eleven non-MODE
#[test] fn all_twelve_round_trip_on_disk()  // Sound::init(Algo) with every VA param off neutral (WAVE 0.1/0.9, SHAPE 0.3/0.7, CRSE −24/+24, FINE −100/+100, BITS, AMOUNT 1, SUB 1, STAB 0): encode, decode, bits_eq
#[test] fn unknown_mode_code_loads_as_mix() // on a neutral VaParams, set_enum_code(VaParams::MODE, 9) is false and mode stays VaMode::Mix; VaMode::from_disk_code(9) == None
#[test] fn crse_rounds_a_modulated_value()  // Block::write(CRSE1, 6.4) reads back 6.0; write(CRSE1, −24.6) reads −24.0; write(CRSE1, NaN) reads a value in −24..=24
#[test] fn block_35_is_va()                 // BlockRef::Va.disk_code() == Some(35), disk_ident() == Some("VA"), BlockRef::from_disk_code(35) == Some(BlockRef::Va)
#[test] fn wave_cell_draws_an_arc()         // VA_SPECS[WAVE1].glyph == FocusGlyph::Arc (not Wave: that draws an ALGO table)
```

- [ ] **Step 2: Run them.** `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test va_params_test`. Expected: FAIL to compile (no `dsp::va`).
- [ ] **Step 3: Implement** the Interfaces and the arms in the Files list.
  - Run `cargo test -p chimera-core --test disk_codes_test --test sound_codec_test --test addr_test --test matrix_view_test`.
  - Append block 35's printed rows to `disk_codes_v1.txt`, and add the `va:` field to `neutral_v1.txt`.
  - Set `sparse_bound`'s worst and `MAX_SOUND_COST` (its next multiple of 16) to the printed values.
  - No other fixture row may change. If `every_destination_header_fits_and_is_unique_in_its_block` names a short too wide, report its width.
- [ ] **Step 4: Verify.** `just test` and `just clippy`. Expected: PASS. Every golden stays bit-identical (a neutral VA block is never written, so no saved Sound changes), and `param_motion_test` passes with nine more knobs.
- [ ] **Step 5: Commit.** `git add` the files above; `git commit -m "VA params block, block code 35"`. Report the flash line.

### Task 2: Provenance and the 2-point kernels

Spec § Oscillator (Antialiasing), § Code layout. ADR 0114 points 4 and 6.

**Files:**
- Create: `chimera-engine/src/dsp/provenance.rs`, `chimera-engine/src/dsp/va/blep.rs`
- Modify: `chimera-engine/src/dsp/mod.rs` (`pub mod provenance;`), `chimera-engine/src/dsp/va/mod.rs` (`pub mod blep;`)
- Test: `chimera-core/tests/va_blep_test.rs`

**Interfaces:**
- Produces (`dsp::provenance`):
  - `pub enum Provenance { Textbook { cite: &'static str }, CircuitModel { circuit: &'static str, refs: &'static str, check: &'static str }, Original { author: &'static str } }`;
  - `pub enum Patent { NotChecked, Checked }`;
  - `pub struct PatentLine { pub technique: &'static str, pub patent: Patent }`.
- Produces (`dsp::va::blep`):
  - `pub const PROVENANCE: &[Provenance]`: Välimäki & Huovilainen 2007 (polyBLEP) and Esqueda, Välimäki & Bilbao, EUSIPCO 2015 (2-point polyBLAMP), each `Textbook`.
  - `pub const PATENT: &[PatentLine]`: both `NotChecked`.
  - `pub const GPL_CHECK: &str = "clean room: papers [3][4] only; no GPL source read"`.
  - `pub fn blep(t: f32) -> [f32; 2]` and `pub fn blamp(t: f32) -> [f32; 2]`: the residuals for the sample before and the sample after an event `t` samples (0..1) before the later one.
  - `pub struct Corrector`, `Corrector::NEW`, `pub const LATENCY: usize = 1`.
  - `pub fn edge(&mut self, t: f32, step: f32, slope: f32)`: `slope` is the jump in output per sample. It is callable any number of times between two `push`es; the corrections superpose.
  - `pub fn push(&mut self, naive: f32) -> f32`: the corrected sample, `LATENCY` samples late.
  - `pub fn clear(&mut self)`.
  - `#[cfg(any(test, feature = "test-support"))] pub fn last_edges(&self) -> &[(f32, f32, f32)]`: the `(t, step, slope)` passed since the last `push`, up to 8.
- **The slot for a better kernel:** every caller goes through `Corrector::edge`/`push`. A 4-point polyBLAMP (Hohnerlein et al., DAFx-17) replaces `blep`/`blamp` and `LATENCY` here and nothing else; the oscillator tests (Tasks 3–4) are the gate.

The algorithm:
- `blep(t) = [½t², −½(1−t)²]` and `blamp(t) = [t³/6, (1−t)³/6]`.
- `Corrector` keeps the previous naive sample. `edge` adds `step·blep(t)[0] + slope·blamp(t)[0]` to it, and the `[1]` terms to an accumulator. `push` returns the held sample and holds `naive + accumulator`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn kernels_at_their_ends()   // blep(0) == [0, −0.5], blep(1) == [0.5, 0]; blamp(0) == [0, 1/6], blamp(1) == [1/6, 0]; blep(0.5) == [0.125, −0.125]
#[test] fn blep_keeps_the_edge_time()  // t in 0..=1 step 1/64: blep(t)[0] + blep(t)[1] == t − 0.5 within 1e-6
#[test] fn blamp_integrates_blep()     // blamp(t)[0] == ∫₀ᵗ blep(u)[0] du and blamp(t)[1] == −∫ₜ¹ blep(u)[1] du, by a 1 024-step f64 midpoint sum, within 1e-5
#[test] fn corrector_is_one_sample_late() // push(1.0), push(2.0), push(3.0) with no edges returns 0.0, 1.0, 2.0
#[test] fn corrector_applies_both_halves() // edge(0.25, 2.0, 0.0) between push(−1.0) and push(1.0): the two outputs are −1 + 2·blep(0.25)[0] and 1 + 2·blep(0.25)[1]
#[test] fn corrections_superpose()     // two edges between the same pair of pushes sum; edges in adjacent intervals both reach the shared sample: out == naive + s1·blep(t1)[1] + s2·blep(t2)[0] within 1e-6
#[test] fn provenance_is_declared()    // blep::PROVENANCE non-empty and every entry Textbook; PATENT lists both kernels NotChecked; GPL_CHECK non-empty
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_blep_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA kernels: 2-point polyBLEP and polyBLAMP"`. Flash: host only (nothing links it yet).

### Task 3: The variable-wave oscillator

Spec § Oscillator (WAVE, SHAPE, Anchor phase, Level, Antialiasing), § Testing (alias gate, level and DC, no clicks, pitch, library-ready). The precedent is Ambika's variable-waveform oscillator, architecture only (from the manual), no code or tables (ADR 0114).

**Files:**
- Create:
  - `chimera-engine/src/dsp/va/osc.rs`;
  - `chimera-core/tests/common/spectrum.rs`: a test helper with an in-place radix-2 `fft`, `alias_db`, `alias_db_lattice`, `peak_hz`, `fundamental`, and `tri_reference`. No new dependency.
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`pub mod osc;`), `chimera-core/tests/common/mod.rs` (`pub mod spectrum;`)
- Test: `chimera-core/tests/va_osc_test.rs`

**Interfaces:**
- Consumes: `Corrector`, `blep`, `blamp` (Task 2); `crate::dsp::fast_sin_turns`.
- Produces (`dsp::va::osc`):
  - `pub const PROVENANCE`: the phase accumulator `Textbook`; the waves and SHAPE laws `Original { author: "AI-assisted, owner-directed and reviewed" }`. `PATENT` is empty; `GPL_CHECK`: "clean room: Ambika manual (architecture) only; MI firmware not read".
  - `pub const R: f32 = 0.2`, and `pub const SINE_RMS: [f32; 17]` (Decided here).
  - `pub struct Unit(f32)`: `Unit::new(v)` is `v.max(0.0).min(1.0)`, with NaN → 0. `pub struct Morph { pub wave: Unit, pub shape: Unit }`.
  - `pub fn wave_at(phase: f32, m: Morph) -> (f32, f32)`: the naive value and its slope per cycle (the morph's two waves, weighted, gains included). It is the reference for tests and for sync. It takes no `inc`, so the pulse floor is not applied.
  - `pub fn peak_bound(m: Morph, inc: f32) -> f32`.
  - `pub enum Correction { On, Off }` (`Off` is for tests only, the gate's baseline).
  - `pub struct VarOsc`; `VarOsc::NEW`; `pub fn prepare(&mut self, sample_rate: f32)`.
  - `pub fn inc_for(&self, hz: f32) -> f32`, clamped to `[MIN_INC, MAX_INC]`, with `pub const MAX_INC: f32 = 0.45` and `MIN_INC: f32 = 1e-6`.
  - `pub fn reset(&mut self)`: phase 0, corrector cleared, thresholds re-anchored.
  - `pub fn phase(&self) -> f32`, `pub fn set_phase(&mut self, p: f32)`, `pub fn with_correction(self, c: Correction) -> Self`.
  - `pub struct Tick { pub out: f32, pub wrap: Option<f32> }`: `wrap` is `t`, the samples since the phase wrapped this sample.
  - `#[inline(always)] pub fn tick(&mut self, inc: f32, m: Morph, moving: bool, sync: Option<f32>) -> Tick`:
    - `moving` selects the exact per-sample `vsqrt`/`vdiv` (Decided here);
    - `sync` is Task 4's; this task passes `None`.
  - `#[cfg(any(test, feature = "test-support"))] pub fn last_edges(&self) -> &[(f32, f32, f32)]`, the corrector's.

The waves, their gains, edges and events are those in "Decided here". Only the two weighted waves are evaluated, `k = min(⌊3x⌋, 2)`, `f = 3x − k`. Each edge's step and slope are scaled by its wave's weight. The sine is not corrected.

- [ ] **Step 1: Write the failing tests** (`f0 = 1046.50` is C6; *sim* values in brackets):

```rust
#[test] fn corrected_saw_beats_naive()       // Task 2's Corrector on a 1 kHz falling saw at 48 kHz (step +2 at each wrap): alias_db ≤ −45 and ≥ 12 dB under the naive saw
#[test] fn anchors_are_their_plain_selves()   // SHAPE 0, inc 1/4800, Correction::Off, samples ≥ 2 from any edge: WAVE 0 → 0.2828·sin 2πp; ⅓ → 0.3464·tri (0 rising at p=0, peak at ¼); ⅔ → 0.3464·(1 − 2p); 1 → +0.2 on [0, ½), −0.2 else; |err| ≤ 1e-4
#[test] fn wave_morph_is_a_linear_crossfade() // Correction::Off: WAVE 1/6 == ½ sine + ½ triangle, WAVE 0.5 == ½ triangle + ½ saw, within 1e-6
#[test] fn shape_does_what_the_table_says()   // SHAPE 1, inc 1/4800: sine == 0.3069·sin(3 sin θ) within 1e-3; triangle trough at p = 0.525, peak at 0.475; pulse high on [0.225, 0.275); saw steps at 0.875 and 0.125, each 0.5237 high; SHAPE 0 == the plain anchor
#[test] fn level_holds_across_shape()        // each anchor, SHAPE 0..=1 step 1/20, one cycle at inc 1/4800, Off: rms within ±1 dB of R [±0.01]; |mean| ≤ 1e-3·R [≤ 4.6e-4·R]; |y| ≤ 0.9 [0.872]
#[test] fn level_across_wave()               // SHAPE {0, ½, 1}, WAVE 0..=1 step 1/60: rms ≥ R·10^(−2.5/20) [−0.58, −0.93, −1.87 dB]
#[test] fn fundamental_follows_the_anchors()  // SHAPE {0, ½, 1}, WAVE step 1/60: |F| ≥ 10^(−1/20) × lerp of the neighbouring anchors' |F|, and the in-phase part (projection on sin 2πp) > 0 [0.00 dB]
#[test] fn split_saw_keeps_its_fundamental()  // |F| of WAVE ⅔ at SHAPE 1 within ±1 dB of SHAPE 0's [+0.58 dB]
#[test] fn in_tune_at_every_rate()            // each anchor and midpoint at A4, SHAPE 0, at 44_100, 48_000, 96_000, 192_000 Hz: spectrum::peak_hz within 1 cent of 440
#[test] fn alias_gate_at_c6()                 // 48 kHz, WAVE in {0, 1/6, 1/3, 1/2, 2/3, 5/6, 1} × SHAPE in {0, 0.5, 1}: alias_db ≤ −45 [worst −53.0]; where Off folds > −60 dB, ≥ 12 dB below it
#[test] fn edges_never_overshoot()            // same grid at C2, C4, C6, C7: every |out| ≤ 0.9
#[test] fn narrow_pulse_and_split_saw_pass_high() // SHAPE 1: pulse (WAVE 1, one-sample floor) and split saw (WAVE ⅔) at C7 and C8: alias_db ≤ −45 [pulse C8 −53.6, saw C8 −59.2], ≥ 12 dB under Off
#[test] fn pulse_never_narrower_than_a_sample() // WAVE 1, SHAPE 1 at C8 (inc 0.0872): the high run is ≥ 1 sample: last_edges' fall follows its rise by ≥ 1 sample of phase (w ≥ inc)
#[test] fn morphing_tracks_the_reference()    // A4, 100 ms (4 800 samples), set per sample: WAVE 0 → 1 at SHAPE 0, ½, 1; SHAPE 0 → 1 at WAVE 0, ⅓, ½, ⅔, ⅚, 1: |y[n] − tri_reference[n]| ≤ 2e-3 [6.3e-4]
#[test] fn the_reference_catches_a_step()     // the WAVE sweep above with 0.005 added from sample 2 000 on: max |y − ref| > 2e-3 [5.2e-3]
#[test] fn a_threshold_crossing_backwards_is_an_edge() // inc = MIN_INC, set_phase(0.28), WAVE 1, SHAPE 1 → 0.9 in one tick (moving): last_edges has exactly one event, t = 0.778 ± 1e-3, step in [0.682, 0.918] (positive: low → high)
#[test] fn fast_shape_at_c1_tracks_the_reference() // C1, SHAPE 0→1 in 48 samples, held 192, 1→0 in 48, held 192, repeating, 4 800 samples, at WAVE 1, ⅔, ⅓, ⅚: |y − tri_reference| ≤ 0.02 [8.2e-3; forward-only crossings 0.18]
```

The helpers, all in f64:
- `spectrum::alias_db(x: &[f32], sample_rate: f32, f0s: &[f32]) -> f64` and `spectrum::peak_hz(x: &[f32], sample_rate: f32, near: f32) -> f64` implement the two methods in "Decided here".
- `spectrum::alias_db_lattice(x: &[f32], sample_rate: f32, f1: f32, f2: f32) -> f64` masks ±4 bins around every |i·f1 + j·f2|, for |i|, |j| ≤ ⌈24 kHz / min(f1, f2)⌉ + 1.
- `spectrum::tri_reference(f: impl Fn(f64) -> f64, n: usize) -> Vec<f64>`. Element k is `∫ f(k − 1 + u)·(1 − |u|) du` over u in [−1, 1], by a 2 048-point midpoint sum. Pass `f(t) = wave_at(frac(p0 + inc·t), morph(t))`, with the settings linear in t. It is the 2-point kernel's own filter, one sample late. Coarser sums err by up to step/(4·points).

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_osc_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS. If a gate fails, stop and report (the 4-point kernel is Task 2's swap, an owner call); never loosen −45 dB or the reference bounds.
- [ ] **Step 5: Commit.** `git commit -m "VA oscillator: WAVE morph and SHAPE, band-limited, one level"`. Flash: host only.

### Task 4: Sync, the sub and the five operators

Spec § Combine (table, operator scaling, SYNC reset, RING's coincident edges, `xor16`, BITS, Sub), § Testing (peak-safe, alias gate in SYNC and RING).

**Files:**
- Create: `chimera-engine/src/dsp/va/combine.rs`
- Modify: `chimera-engine/src/dsp/va/osc.rs` (`tick`'s `sync`, `Sub`), `chimera-engine/src/dsp/va/mod.rs` (`pub mod combine;`)
- Test: `chimera-core/tests/va_combine_test.rs`

**Interfaces:**
- Consumes: `VarOsc`, `Tick`, `Morph`, `wave_at`, `peak_bound`, `R`, `Corrector` (Tasks 2–3); `VaMode` (Task 1).
- Produces (`osc`):
  - `tick(.., sync: Some(t))`: the three-part split in "Decided here". If the gate fails for a sine slave, La Pastina & D'Angelo's exact residual replaces the reset's correction.
  - `pub struct Sub`, `Sub::NEW`, `pub fn reset(&mut self)` (+R).
  - `#[inline(always)] pub fn tick(&mut self, wrap: Option<f32>) -> f32`: ±R, flipped at each master wrap, BLEP-corrected (step ±2R). It has its own `Corrector`, so it lands one sample late like the oscillators.
  - `pub fn high(&self) -> bool` and `pub fn set_high(&mut self, h: bool)`, for the carry.
- Produces (`combine`):
  - `pub fn xor16(a: f32, b: f32) -> f32`: each input becomes `(v.max(−1).min(1) · 32767.0) as i16`; the words are XORed and the result is `as f32 / 32768.0`.
  - `pub fn bits(x: f32) -> f32`: `16 · 8^(−x)` via `fast_exp2`.
  - `pub struct Crusher` (`Crusher::NEW`; it holds the per-block level, so an ADAA step can later add state): `pub fn set(&mut self, x: f32)` once a block, and `pub fn process(&self, v: f32) -> f32`.
  - `pub struct Peaks { pub a: f32, pub b: f32 }`: the block's `peak_bound`s.
  - `pub fn combine(mode: VaMode, a: f32, b: f32, x: f32, pk: Peaks, crush: &Crusher) -> f32`: the spec's table, with `k`, `p` and `q` from `pk`. SYNC combines as MIX.
  - `pub fn with_sub(c: f32, s: f32, sub: f32) -> f32`: `(c + sub·s)/√(1 + sub²)`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn operators_are_the_table()   // a, b on [−0.87, 0.87] step 1/16, x on a 1/16 grid, pk (0.87, 0.35): Mix/Sync == (1−x)a + xb; Ring == (1−x)·½(a+b) + x·min(5, 0.9/(0.87·0.35))·a·b; Xor == (1−x)·½(a+b) + x·0.87·xor16(a/0.87, b/0.87); Bits == 0.61·crush(½(a+b)/0.61); exact to 1e-6
#[test] fn xor16_works_on_words()      // xor16(0.5, 0.25) == 8192.0 / 32768.0; xor16(a, a) == 0.0; xor16(−1.0, 0.0) == −32767.0 / 32768.0
#[test] fn bits_follow_the_log_curve() // bits(0) == 16, bits(1/3) ≈ 8, bits(2/3) ≈ 4, bits(1) == 2 (1e-4); at x = 1 the crush outputs only {−1, −½, 0, ½, 1}; at x = 0, |crush(v) − v| ≤ 2^−15
#[test] fn every_operator_is_peak_safe() // every mode, |a| ≤ pk.a, |b| ≤ pk.b on a 1/64 grid, pk in {(0.87, 0.87), (0.28, 0.87), (0.2, 0.2)}, x in {0, ¼, ½, ¾, 1}, with_sub at sub {0, ½, 1} and s = ±R: |y| ≤ 0.95
#[test] fn sync_restarts_the_slave()   // master inc 0.0123, slave 0.031: after each master wrap at t, slave.phase() == t · 0.031 within 1e-6
#[test] fn sync_splits_around_the_reset() // master set_phase(0.95), inc 0.125 (wraps t = 0.6); slave WAVE ⅔, SHAPE 1, set_phase(0.80), inc 0.25: one tick's last_edges == [(0.7, +0.5237, 0), (0.6, −0.1047, 0), (0.1, +0.5237, 0)] within 1e-4 (own step, reset, own step after it); slave.phase() == 0.15
#[test] fn sync_passes_the_alias_gate() // master C6, slave at 2^(7/12)× and 2.37×, slave WAVE in the Task 3 grid, SHAPE 0, ½ and 1: alias_db (master's series masked) ≤ −45 [worst −46.8: thin; if f32 fails it, stop and report], ≥ 12 dB under Off
#[test] fn ring_passes_the_alias_gate_detuned() // C5 (523.25 Hz), slave at 2^(6/1200)× and 2·2^(6/1200)×, WAVE1 in the Task 3 grid, WAVE2 = WAVE1 or ⅓, SHAPE 0 and ½, both phase 0, x ∈ {½, 1}: alias_db_lattice ≤ −45 [worst −49.6]
#[test] fn sub_is_an_octave_down()     // master at 440: the sub's peak_hz within 1 cent of 220; its sign flips exactly at the master's wraps; |s| == R between flips
#[test] fn sub_passes_the_alias_gate()  // the sub alone at C6's f0/2: ≤ −45 dB
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_combine_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA sync, sub and the five operators"`. Flash: host only.

### Task 5: STABILITY

Spec § Stability (drift, tolerance, HF tracking, scale error, jitter, seeding, what recalls), § Testing (Stability, library-ready).

**Files:**
- Create: `chimera-engine/src/dsp/va/stability.rs`
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`pub mod stability;`)
- Test: `chimera-core/tests/va_stability_test.rs`

**Interfaces:**
- Produces (`dsp::va::stability`). It takes no product types; the oscillators are `[_; 2]`.
  - `pub const PROVENANCE`: the walks, trim and jitter are `Original`. HF tracking and scale error are `CircuitModel { circuit: "VCO core reset time; exponential-converter scale", refs: "CEM3340 datasheet; SSI2130 datasheet", check: "C7 at τ 4 µs = −14.4 cents" }`. `GPL_CHECK`: "clean room; no GPL source read".
  - Constants: `pub const DRIFT_SHARED_CENTS: f32 = 12.0`, `DRIFT_OWN_CENTS = 6.0`, `TRIM_CENTS = 3.0`, `JITTER_CENTS = 0.5`, `TAU_MAX_S = 4.0e-6` (an estimate, not a measurement), `TAU_U_LO = -0.25`, `SCALE_ERR = 0.002`, `WALK_HZ = 0.2`, `WALK_RMS = 0.30`.
  - Salts: `pub const SALT_TRIM: [u32; 2] = [0x7121, 0x7122]`, `SALT_TAU = [0x7A01, 0x7A02]`, `SALT_SCALE = [0xE501, 0xE502]`, `SALT_WALK: [u32; 3] = [0x3A1C, 0x3A1D, 0x3A1E]` (shared, OSC 1, OSC 2), `SALT_JITTER = 0x1177`.
  - `pub fn depth(stability: f32) -> f32`: `(1 − s.max(0).min(1))²`.
  - `pub const fn mix32(z: u32) -> u32`:
    - `z += 0x9E37_79B9`;
    - `z = (z ^ z >> 16) * 0x85EB_CA6B`;
    - `z = (z ^ z >> 13) * 0xC2B2_AE35`;
    - `z ^ z >> 16`, all wrapping.
  - `pub fn uni(x: u32) -> f32`: `(x as i32) as f32 / 2³¹`.
  - `pub struct Seed(u32)`:
    - `pub const fn of(part: u32, slot: u32) -> Seed` is `mix32(part << 8 | slot)`;
    - `pub fn draw(self, salt: u32) -> f32` is `uni(mix32(self.0 ^ salt))`;
    - `pub fn stream(self, salt: u32) -> u32` is `mix32(self.0 ^ salt)`, with 0 → `0x6D2B_79F5`: a xorshift32 state.
  - `pub struct Character { pub trim: [f32; 2], pub tau_u: [f32; 2], pub scale: [f32; 2] }` (`PartialEq`), with `pub fn of(s: Seed) -> Character`:
    - trims `draw(SALT_TRIM[k])`;
    - `u = −0.25 + 0.625·(draw(SALT_TAU[k]) + 1)`;
    - scale `draw(SALT_SCALE[k])`.
  - `pub struct Stability`; `Stability::NEW`.
  - `pub fn prepare(&mut self, update_hz: f32)`: the rate `step` is called at (the block rate for VA). It sets `a = 1 − exp(−2π·WALK_HZ/update_hz)`, `b = 1 − a`, and the gain `g = WALK_RMS / √((1/3)·a⁴·(1 + b²)/(a(2 − a))³)`. `a(2 − a)` is `1 − b²` without cancellation.
  - `pub fn seed(&mut self, s: Seed)`: the character, and the four xorshift streams. The walks' filter states are left as they are.
  - `pub fn character(&self) -> Character`.
  - `pub fn step(&mut self, d: f32) -> [f32; 2]`: cents, drift + trim, per oscillator, once a block. A walk is uniform noise `uni(xorshift)`, then two cascaded one-poles `y += a(x − y)`, then `g·y₂`, then `max(−1).min(1)`.
  - `pub fn walk_raw(&self, k: usize) -> f32`: `g·y₂`, before the clamp (tests).
  - `pub fn tracked(&self, hz: [f32; 2], d: f32) -> [f32; 2]`: `f/(1 + f·τ)`, with τ signed and `1 + f·τ` floored at 0.5.
  - `pub fn scale_cents(&self, cents: [f32; 2], d: f32) -> [f32; 2]`: the extra cents `(cents − 6000)·SCALE_ERR·scale·d`.
  - `pub fn jitter(&mut self, d: f32) -> f32`: the cents for the cycle a wrap starts.
  - With `d == 0.0`, `step`, `jitter` and `scale_cents` return exactly 0.0, and `tracked` returns `hz` unchanged (the walks still advance), so STAB 1 is bit-identical.

- [ ] **Step 1: Write the failing tests** (a walk run settles 30 s first; *sim* in brackets):

```rust
#[test] fn depth_curve()               // depth(1) == 0, depth(0) == 1, depth(0.5) == 0.25, depth(0.7) within 1e-6 of 0.09, depth(NaN) == 1
#[test] fn walk_is_the_band()          // 600 s at 48_000/64, Seed::of(0, s) for s in 0..8, walk 0: each rms in 0.25..0.35 [0.28..0.33]; hysteretic (±0.1) zero crossings 0.12..0.40 a second [0.18..0.27]; |walk_raw| > 1 on ≤ 1.5 % of steps [≤ 0.51 %]
#[test] fn walks_are_rate_independent() // 44_100/64, 48_000/64, 96_000/64, 192_000/64, same 8 seeds: the 8-seed mean rms within 10 % of 48 kHz's [2 %], mean crossing rate within 20 % [6 %]
#[test] fn drift_has_its_depth()       // d = 1, 600 s, 8 seeds, trims excluded (shared walk × 12): rms 3.0..4.2 cents [3.74]; d = 0.25: rms scales by 0.25 within 1e-3 relative
#[test] fn the_pair_drifts_together()  // d = 1, 600 s: corr(step[0], step[1]) ≥ 0.6 [0.82]
#[test] fn seeds_spread()              // for part in 0..8 and each salt in SALT_TRIM, SALT_TAU, SALT_SCALE: the 8 slots' draws have std ≥ 0.25 [min 0.309] and pairwise |Δ| > 1e-3 [min 0.0014]; 2 048 (part, slot) pairs' SALT_TRIM draws: mean within ±0.05 of 0 [0.008]
#[test] fn jitter_in_bounds()          // 10^5 draws at d = 1 and 0.25: |jitter| ≤ 0.5 · d, mean within 0.02 · d of 0
#[test] fn hf_tracking()               // d = 1, 32 seeds: every τ factor in [−0.25, 1]; τ alone puts C7 (2093.0 Hz) between −14.5 and +3.7 cents (−14.43 at u = 1, +3.63 at u = −0.25); A2 (110 Hz) within 1 cent of true; d = 0: tracked(hz) == hz bit for bit; where |u₁ − u₂| > 0.05, |f1 − f2| at C7 > at C4 (261.63 Hz), compared in f64
#[test] fn scale_error()               // d = 1, 32 seeds: scale_cents at C4 (6000) == 0; at C7 (9600) |x| ≤ 7.2; at A2 (4500) |x| ≤ 3.0; d = 0: exactly 0.0
#[test] fn hf_tracking_is_rate_independent() // tracked() at C7 is the same at all four rates (τ is seconds, not samples)
#[test] fn character_is_the_seeds()    // Stability seeded with Seed::of(2, 3) after 10^4 steps: character() == Character::of(Seed::of(2, 3)); Seed::of(1, 3)'s differs
#[test] fn depth_zero_is_silent_bits() // d = 0: step returns [0.0; 2] exactly and jitter 0.0, over 10^4 calls
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_stability_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA STABILITY: drift, trim, HF tracking, jitter, seeds"`. Flash: host only.

### Task 6: `VaEngine`

Spec § Oscillator (Pitch), § Combine (MODE changes, Sub), § Stability (phase at note-on, free-run, sub follows OSC 1, seeding), § Testing (no clicks, every knob audible, stability, peak-safe, alias gate, pitch).

**Files:**
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`VaEngine`, `VaCarry`, `VA_OUT_GAIN`, the bills)
- Test: `chimera-core/tests/va_engine_test.rs`

**Interfaces:**
- Consumes: Tasks 1–5; `crate::dsp::ease::{Ramp, at, step_of}`, `crate::dsp::glide::Glide`, `crate::dsp::fast_exp2`, `crate::project::PartId`, `crate::voice_alloc::VoiceIdx`, `crate::hw::Cost`.
- Produces (`dsp::va`):
  - `pub const VA_OUT_GAIN: f32 = 1.0` (Task 10 sets it).
  - `pub const COST_MIX: Cost = Cost(360)`, `COST_SYNC = Cost(410)`, `COST_RING = Cost(370)`, `COST_XOR = Cost(380)`, `COST_BITS = Cost(380)`: paper estimates with LP24's 95 included. Task 9 re-bills.
  - `#[derive(Clone, Copy)] pub(crate) struct VaCarry` (Decided here).
  - `pub struct VaEngine`, with:
    - setup: `pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self`; `pub fn place(&mut self, voice: VoiceIdx)`; `pub(crate) fn carry(&self) -> VaCarry`; `pub(crate) fn restore(&mut self, c: VaCarry)`;
    - notes: `pub fn set_pitch(&mut self, cents: f32)`; `pub fn note_on(&mut self, note: u8, p: &VaParams, part: PartId, sample_rate: u32)`; `pub fn glide_on(&mut self, note: u8, p: &VaParams, part: PartId, (sample_rate, tau): (u32, f32))`; `pub fn glide_to(&mut self, note: u8, (sample_rate, tau): (u32, f32))`; `pub fn note_off(&mut self)` (does nothing: the VCA ends the note); `pub fn hold(&mut self)` (a fresh note's ramps start on their values);
    - render: `pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], p: &VaParams, sample_rate: u32)`;
    - state: `pub fn is_active(&self) -> bool` (from note-on on; VA never decays); `pub fn mode(&self) -> VaMode` (the one playing);
    - bills: `pub const fn mode_cost(m: VaMode) -> Cost`; `pub fn cost(p: &VaParams) -> Cost`; `pub fn playing_cost(&self) -> Option<Cost>` (the playing mode's, while active);
    - test-only, `#[cfg(any(test, feature = "test-support"))]`: `pub fn slide(&self) -> f32`, `pub fn uncorrected(&mut self)`, `pub fn osc_phase(&self) -> [f32; 2]`, `pub fn character(&self) -> Option<Character>`.
  - `const _: () = assert!(size_of::<VaEngine>() <= size_of::<ModalEngine>());`

The render (the algorithm the signatures leave open):
- **Once a block:**
  - Clamp every value with `max/min`, and set `d = depth(stab)`.
  - `cents_k = note·100 + pitch + coarse_k·100 + fine_k + stab.step(d)[k]`, then `cents_k += stab.scale_cents(cents, d)[k]`.
  - `hz_k = 440 · fast_exp2((cents_k − 6900)/1200) · slide`, then `hz = stab.tracked(hz, d)`.
  - `inc_k = osc.inc_for(hz_k)`, ramped linearly across the block from the last block's (`ease::Ramp`). WAVE, SHAPE, AMOUNT and SUB ramp per sample the same way.
  - `moving_k` = SHAPE_k's ramp is not flat. `pk = Peaks` from `peak_bound`.
  - `crusher.set(amount)` in BITS.
- **Per sample:**
  - OSC 1 ticks. At its wrap the sub flips, OSC 1 draws `stab.jitter(d)` for its next cycle (`inc · (1 + jitter·ln2/1200)`), and in SYNC its `t` is OSC 2's `sync`.
  - OSC 2 ticks (its own natural wrap draws its jitter).
  - `c = combine(mode, a, b, amount, pk, &crusher)`, then `y = with_sub(c, s, sub)`, times the duck's gain.
- **One dispatch a block:** `render` matches the playing mode once and calls one `#[inline(never)]` loop per mode (`render_mix`, `render_sync`, `render_ring`, `render_xor`, `render_bits`), each with the `#[inline(always)]` ticks, so Task 9 counts each.
- **The duck:**
  - A MODE that differs from the playing one starts `Down` (gain 1 → 0 across the block).
  - At its end the mode swaps (entering SYNC clears OSC 2's corrector), and `Up` runs 0 → 1 the next block.
  - A change during `Down` retargets it; one during `Up` starts `Down` next block.
- **Note-on:**
  - A new `sample_rate` re-runs `osc.prepare(sr)` and `stab.prepare(sr / BLOCK_SIZE)`.
  - If `stability ≥ 1.0 && !is_active()`, both oscillators and the sub reset (phase 0, +R, correctors cleared). Otherwise the phases run on: a re-strike or a steal of a sounding voice never resets.
  - If `part` differs from the last seeded Part, `stab.seed(Seed::of(part.index() as u32, voice.index() as u32))`.
  - `glide_on`/`glide_to` glide `slide` as TAPE does (`Glide`, log₂, once a block).

**"The click rule"** applies to a sine-only render: WAVE 0, SHAPE 0, A4, STAB 1, FINE2 0, SUB 0 unless swept. The swept render's max `|x[n] − 2x[n−1] + x[n−2]|` must be ≤ 1.5 × the largest in held renders at 0, ¼, ½, ¾ and 1 of the swept knob, + 1e-6.

- [ ] **Step 1: Write the failing tests** (A4, 48 kHz, neutral unless said):

```rust
#[test] fn every_anchor_in_tune_at_a4()      // OSC 1 alone (AMOUNT 0, STAB 1, SUB 0), WAVE 0, ⅓, ⅔, 1: peak_hz within 1 cent of 440
#[test] fn alias_gate_in_mix_and_sync()      // C6, STAB 1, SUB 0, FINE2 0; SYNC with OSC 2 CRSE +7: WAVE1 = WAVE2 over the Task 3 grid × SHAPE {0, ½, 1} × {Mix, Sync}: ≤ −45 dB, ≥ 12 dB under uncorrected() where it folds > −60 dB; plus MIX at SUB 1 with f0/2 masked
#[test] fn ring_gate_detuned_at_c5()          // C5, STAB 1, FINE2 +6, CRSE2 0 and +12, WAVE2 = WAVE1 or ⅓, SHAPE {0, ½}, Ring, AMOUNT ½ and 1: alias_db_lattice ≤ −45
#[test] fn knobs_sweep_without_clicks()       // AMOUNT, STAB, FINE1/2 (−100 → +100 cents), SHAPE1/2, each over 100 ms set per block, in Mix, Sync and Ring: the click rule
#[test] fn sub_sweeps_without_clicks()        // note 0 (8.18 Hz: the sub has no edge in the first 100 ms at STAB 1), SUB 0 → 1 over 100 ms from block 1: the click rule
#[test] fn the_click_rule_catches_a_step()    // the AMOUNT sweep with 0.002 added from sample 2 000 on: the click rule fails
#[test] fn mode_changes_never_click()        // every ordered pair of Mix, Sync, Ring changed mid-note: the click rule
#[test] fn the_duck_is_one_block_each_way()  // a change at block k: |y| in block k ≤ (1 − (i+1)/64) · steady peak + 1e-6; block k+1 ≤ ((i+1)/64) · peak + 1e-6; mode() is the new one from block k+1
#[test] fn every_knob_is_audible()           // each of the nine at its min vs max (FINE −100 vs +100 cents), others 0.3, 1 s, STAB 1: rms(y0 − y1) ≥ 0.1 · rms(y0); AMOUNT also in Ring and Xor at FINE2 +6 (detuned inputs)
#[test] fn xor_of_identical_oscillators_is_silent() // STAB 1, equal WAVE/SHAPE/tuning, Xor, AMOUNT 1, SUB 0: rms < 1e-3 (a XOR a = 0, by design)
#[test] fn peak_safe_everywhere()            // every MODE × AMOUNT {0, ½, 1} × SUB {0, ½, 1} × WAVE anchors × SHAPE {0, 1}, 0.25 s at C2 and C6: |y| ≤ 0.95
#[test] fn a_restrike_keeps_phase()          // STAB 1, note held 0.1 s, note_on again (active): osc_phase() right after == the phase before advanced by one tick's increment (1e-6), and the click rule holds across the re-strike
#[test] fn stab_one_resets_an_idle_engine()  // STAB 1, fresh engine: note_on, render 0.1 s, then a fresh engine on another (part, voice): the first 0.5 s match bit for bit
#[test] fn carry_restores_the_state()        // STAB 0.5, render 0.3 s; a second engine restore(first.carry()): osc_phase() and character() equal the first's; both render the next 0.2 s from the same note_on bit-identically
#[test] fn sub_follows_osc1_tracked()        // STAB 0, C7: the sub's peak_hz is half OSC 1's within 0.5 cent
#[test] fn steal_glide_glides()              // glide_on at tau 0.1 from an octave up: slide reaches 63 % of the way in 0.1 s ± 1 block
#[test] fn bills_follow_the_mode()           // mode_cost: Mix 360, Sync 410, Ring 370, Xor 380, Bits 380; playing_cost == Some(mode_cost(mode())) while active, None before note_on
#[test] fn midi_extremes_stay_finite()       // Review Focus 1: note 127 + CRSE +24 + set_pitch(2400) + FINE +100; note 0 + CRSE −24: 1 s each, all finite, |y| ≤ 0.95, the output still moves (rms > 1e-3)
#[test] fn out_of_range_values_clamp()       // Review Focus 2: wave 1.7/−0.3/NaN, shape −0.4/1.9/NaN, amount 2, sub 3, stability −1, 2 and NaN: finite, |y| ≤ 0.95
#[test] fn mode_scrolled_every_block_settles() // Review Focus 3: MODE stepped Mix→Sync→Ring→Mix… every block for 40 blocks, then held at Ring: the click rule over the whole run (XOR and BITS left out, exempt), mode() == Ring within 2 blocks of the last change
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_engine_test`. Expected: FAIL to compile (no `VaEngine`).
- [ ] **Step 3: Implement** the Interfaces. If a gate fails, stop and report; never loosen it.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA engine: two oscillators, operator, sub, duck, STABILITY"`. Flash: host only.

### Task 7: The VA pages

Spec § Pages. ADR 0086 (hints), ADR 0112 point 8 (bars).

**Files:**
- Modify: `chimera-core/src/ui/block_registry.rs` (`VA_HOME`, `VA_OSC1`, `VA_OSC2`, `VA_SUB_PAGES`, `VA_BLOCKS`, `VA_CHAIN`, `FIXED` [14 → 15] with `&VA_CHAIN`)
- Test: `chimera-core/tests/va_pages_test.rs` (new; Task 8 adds the walk)

**Interfaces:**
- Consumes: `BlockRef::Va`, `VaParams` ids (Task 1).
- Produces (`ui::block_registry`):
  - `pub static VA_HOME: BlockDef`: id 87, name `"VA"`, short `"VA"`, `CellGrid`, `VizType::None`. Cells: WAVE1, WAVE2, MODE, AMOUNT `.with_label("AMNT")`, SUB, STAB.
  - `pub static VA_OSC1: BlockDef`: id 88, `"Osc 1"`, `"OSC1"`. Cells: CRSE1 `.with_label("CRSE")`, FINE1 `"FINE"`, SHAPE1 `"SHAPE"`, then `EMPTY` ×3.
  - `VA_OSC2`: id 89, `"Osc 2"`, `"OSC2"`, the same for OSC 2.
  - Ids 87–89 are free and under the focus table's top (95), so it doesn't grow.
  - `static VA_SUB_PAGES: [&BlockDef; 3] = [&VA_OSC1, &VA_OSC2, &PITCH]`
  - `static VA_BLOCKS: [ChainBlock; 5]`: `with_subs(&VA_HOME, &VA_SUB_PAGES)`, `page(&DRIVE)`, `with_subs(&FILTER, &FILTER_SUB_PAGES)`, `page(&FOLDER)`, and MOD as TAPE's.
  - `pub static VA_CHAIN: ChainDef2 = ChainDef2::new("VA", &VA_BLOCKS, &PART_MOD_SOURCES).with_home_def(&VA_HOME)`

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn chain_is_va_then_the_voice_chain() // map labels ["VA", "DRV", "FLT", "AMP", "MOD"]; home node 0 sub 0; subs ids [88, 89, PITCH.id]; (VA_HOME.id, VA_OSC1.id, VA_OSC2.id) == (87, 88, 89)
#[test] fn va_page_cells()          // labels ["WAVE1", "WAVE2", "MODE", "AMNT", "SUB", "STAB"]; each addr in BlockRef::Va with the Task 1 id
#[test] fn osc_page_cells()         // OSC1: ["CRSE", "FINE", "SHAPE", "", "", ""] on CRSE1/FINE1/SHAPE1; OSC2 on CRSE2/FINE2/SHAPE2
#[test] fn bars_follow_motion()     // across the three pages: motion().is_continuous() for WAVE1/2, AMNT, SUB, STAB, FINE, SHAPE; not for MODE or CRSE
#[test] fn va_pages_have_no_gestures() // every cell's view(..).hint() == hint::NONE
#[test] fn mode_names_fit_the_cell() // each MODE_NAMES entry and each label fits the cell (draw::text_width as tape_pages_test's model_names_fit_the_cell)
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_pages_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS; `no_page_id_is_reserved` passes and the focus table is unchanged in size.
- [ ] **Step 5: Commit.** `git commit -m "VA pages: VA, OSC 1, OSC 2"`. Report the flash line.

### Task 8: `EngineType::Va` in the voice

Spec § Voice and engine, § Stability (free-run, what recalls). ADR 0051 (slot), ADR 0093 (TAPE's pattern), ADR 0096 (VA's 2040).

**Files:**
- Modify:
  - `chimera-engine/src/params.rs`: `EngineType::Va = 3`, `DiskCode` (3, `"VA"`), `ALL` [3 → 4], `label` `"VA"`.
  - `chimera-engine/src/dsp/filter/mod.rs:225`: `default_for` → `FilterKind::K2040` for `Va`. Its doc becomes "VA starts on the 2040 (ADR 0096)".
  - `chimera-engine/src/dsp/engines.rs`:
    - `SlotKind::Va` (`of`, `engine`), and `Va(VaEngine) => rebuild_va, init_va;` in `in_place_enum!`;
    - `init_in_place` (`place(VoiceIdx::ALL[0])`);
    - `rebuild`'s `old` match (:178) gains `| Self::Va(_) => None`;
    - `build`'s Va arm: take `carry()` from a Va slot, rebuild, `place(voice)`, then `restore` it if there was one (Decided here);
    - `kind`, `out_gain` (`VA_OUT_GAIN`), `sym_note_on_clear`;
    - `note_on`: `(Va, None)` → `note_on(.., ctx.part, ..)`, `(Va, Some(tau))` → `glide_on`;
    - `retune`, `retune_clear`, `slide`, `note_off`, `heads` (NONE), `source` (None), `sustains` (false), `hold`, `render`, `set_pitch` (cents, as TAPE), `is_active`;
    - `playing_cost`, `model_cost` (`VaEngine::cost(&p.va)`), `cost` (`VaEngine::cost(&p.va)`);
    - `EngineCtx` gains `pub part: PartId`;
    - the in-file tests' `kinds`, tag 3 and `lease_slot` arms.
  - `chimera-engine/src/instrument.rs` (each `EngineCtx { .. }`, at lines 874, 914, 985, 1038 and 1252): `part: PartId::ALL[q]`, the Part the voice plays (`sounding[v]`).
  - `chimera-engine/src/dsp/voice.rs`: `key_gates` (`:751`) matches `EngineSlot::Tape(_) | EngineSlot::Va(_)`; the no-route arm (`:999`) gains `| EngineSlot::Va(_)`; its test's `EngineCtx`.
  - `chimera-engine/src/preset.rs:39`: ENV 1 → VCA at 127 for `Tape | Va`, and for `Va` also `params.va.fine[1] = 6.0` and `params.va.stability = 0.7`.
  - `chimera-core/src/ui/nav.rs:887`: `EngineType::Va => &block_registry::VA_CHAIN`.
  - Tests' `EngineCtx { .. }` (`engines_test.rs`, `in_place_test.rs`, `tape_source_test.rs`, `param_sweep_test.rs`, `tape_voice_test.rs`, `common/rig.rs`) gain `part: PartId::ALL[0]`, and `common/mod.rs` gains `init_params` and `expects_sound` arms.
  - `chimera-core/tests/sound_codec_test.rs`: `neutral_is_a_contract` also replaces `"kind: Svf,"` with `format!("kind: {:?},", FilterKind::default_for(e))`.
- Test: `engines_test.rs`, `va_pages_test.rs`, `chimera-core/tests/va_voice_test.rs` (new)

**Interfaces:**
- Consumes: `VaEngine`, `VaCarry` (Task 6); `VA_CHAIN` (Task 7).
- Produces: `EngineType::Va`, `SlotKind::Va`, `EngineCtx::part`. The browser's INIT rows (`INIT_TYPES = EngineType::ALL`) gain `VA` without a code change.

- [ ] **Step 1: Write the failing tests:**

```rust
// engines_test.rs: all_lists_every_engine_once_in_order and every_engine_renders_per_its_expectation now cover Va (ALL.len() == EngineType::Va as usize + 1)
#[test] fn va_row_lives_until_its_vca_ends() // va_voice_test: Sound::init(Va) through common::rig: sounds while held; silent (< 1e-4) within ENV 1's release + the fade after key-up
#[test] fn va_with_no_vca_route_ends_at_key_up() // Review Focus 5: ENV 1 → VCA deleted: the voice fades out within Voice::FADE samples of key-up and goes idle
#[test] fn a_voice_va_runs_on()               // B1, a_voice_strip_runs_on's shape: Sound::init(Va) at STAB 0.5, C4 200 blocks, note_off, play until idle (rebuilds() grew); again: the two takes differ (rms(y1 − y2) ≥ 0.1·rms(y1)) and the second's osc_phase() at note-on ≠ [0, 0]
#[test] fn stab_one_replays_on_an_idle_voice() // STAB 1: the same two takes are bit-identical
#[test] fn a_voice_moving_parts_takes_the_parts_character() // Review Focus 4: STAB 0.5. Slot: VA on Part 1, TAPE, MODAL, VA on Part 2 (rebuild in place each time, kind() follows): character == Character::of(Seed::of(1, slot)) on Part 2; a fresh Instrument playing Part 1 on the same slot: character == Character::of(Seed::of(0, slot))
#[test] fn va_init_pins_engine_code_3()      // EngineType::Va.disk_code() == 3, disk_ident() == "VA", from_disk_code(3) == Some(Va)
#[test] fn init_is_neutral_plus_three()      // Sound::init(Va): ENV 1's VCA cell == 127, va.fine == [0, 6], va.stability == 0.7, filter K2040 / Lp24 / Rev2; Sound::neutral(Va): stability 1, fine [0, 0], K2040
// va_pages_test.rs
#[test] fn a_va_load_lands_on_va()           // load_init(&mut ui, EngineType::Va): page_def_id == VA_HOME.id; nav::chain_def_for(Va) is &VA_CHAIN
#[test] fn edit_walks_and_wraps()            // EDIT from VA: OSC1, OSC2, PIT, then VA again (tap(&mut ui, ButtonId::Edit))
```

- [ ] **Step 2: Run them.** `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test engines_test --test va_voice_test --test va_pages_test`. Expected: FAIL to compile (no `EngineType::Va`).
- [ ] **Step 3: Implement** the Files list. The `size_of` assert from Task 6 holds, and the voice-chain assert holds: `VaParams` adds about 40 B to the played snapshot (2,016 → about 2,056 of 2,112).
- [ ] **Step 4: Verify.**
  - Run `just test` and `just clippy`. Expected: PASS.
  - Screen goldens that draw the browser's INIT rows gain `INIT VA`. Re-record those cases only (`GOLDEN_RECORD=1`, then `just screens`), and name them in the commit body.
  - `test_support::full()` skips `FACTORY_LEN + EngineType::ALL.len()`, which moves the COPY slots. Re-record only the goldens and CRCs pinned on it that moved, and name them.
  - From `rust-size`, report the AXI and D2 headroom. If either falls under 1 KiB, stop and report.
- [ ] **Step 5: Commit.** `git commit -m "VA plays: EngineType::Va in the slot, the voice and the chain"`. Report the flash line.

### Task 9: Bills by count

Spec § Budget and billing. ADR 0056 (method), ADR 0040 (voices).

**Files:**
- Modify: `chimera-engine/src/dsp/va/mod.rs` (the five `COST_*` from the count, each with its doc line: `fast N + events M + block B/64 → instr × 1.46 × 1.1 → cycles, + 95 LP24`)
- Test: `chimera-core/tests/cost_test.rs` (`va_bills_each_mode`, `a_held_va_note_bills_its_playing_mode`), `va_engine_test.rs` (`bills_follow_the_mode`'s values)

**Interfaces:**
- Consumes: `VaEngine::{mode_cost, cost, playing_cost}` (Task 6), the `EngineSlot` arms (Task 8), `Voice::held_model_extra`.
- Produces: the committed `COST_MIX … COST_BITS`.

- [ ] **Step 1: Count.**
  - Run `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`, then for each mode `python3 tools/loop_count.py target/thumbv7em-none-eabihf/release/chimera-stm32 render_<mode>`.
  - Confirm the loops contain no `bl` to `tick`; if one does, the `#[inline(always)]` is missing.
  - Count the per-sample path with no event (both waves of a mid-morph live, SHAPE moving: its `vdiv`/`vsqrt` included) and each event branch (a step, a corner, a sync reset).
  - **The bill** = fast + 2 × 1.8 × event (each oscillator at MAX_INC, 4 events a cycle) + (SYNC) 0.45 × reset + per-block ÷ 64 + 95, rounded up to 10.
- [ ] **Step 2: Write the failing tests** with the counted bills:

```rust
#[test] fn va_bills_each_mode() {
    // tape_bills_each_model's rooms and `at`: [4_984, 3_817, 4_813, 4_674]
    // for (mode, bill) in [(Mix, COUNTED), (Sync, COUNTED), (Ring, ..), (Xor, ..), (Bits, ..)]:
    //   EngineSlot::cost(&va_sound(mode), &ModState::new()) == Cost(bill)
    //   at(voice total) ≥ 6 in every room for the worst mode, with INIT's 2040 (+15) (ADR 0040's done-when)
}
#[test] fn a_held_va_note_bills_its_playing_mode() // note held in SYNC, MODE stored MIX: held_model_extra == COST_SYNC − COST_MIX until the duck swaps, then 0
```

- [ ] **Step 3: Run them.** `cargo test -p chimera-core --test cost_test va_`. Expected: FAIL (the estimates differ from the counts).
- [ ] **Step 4: Set the five constants** to the counts and `bills_follow_the_mode`'s values to match. If the worst mode leaves fewer than 6 voices on rev Y (a voice total over 636), stop and report: that is the spec's done-when, and the owner's call.
- [ ] **Step 5: Run** `just test`, `just clippy`. Expected: PASS.
- [ ] **Step 6: Commit.** `git commit -m "VA bills by count"`. Report the flash line, its delta since Task 8 (if the five loops cost over 8 KB, RING, XOR and BITS share one loop with a per-sample match; re-count), and the five bills.

### Task 10: INIT at the reference loudness, sanity and goldens

Spec § Voice and engine (INIT), § Budget and billing (Loudness), § Testing (plumbing and sanity). ADR 0011 (sanity gate), ADR 0063/0109 (loudness).

**Files:**
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`VA_OUT_GAIN`, and its doc: what it was measured on)
- Tests:
  - `chimera-core/tests/param_sweep_test.rs`: `Patch::VaInit` in `SOUNDS`; `patches` → `(BlockRef::Va, true) => vec![Patch::VaInit]`; `va_init_matches_the_loudness_reference`
  - `chimera-core/tests/common/sweep.rs`: `Patch::VaInit` (`"VA INIT"`, `Sound::init(EngineType::Va)`)
  - `chimera-core/tests/common/mod.rs`: `Case::VaInit` (`"va_init"`) in `Case::ALL` [24 → 25], `setup` → `init_params(Va)`
  - `chimera-core/tests/sanity_test.rs`: `bound` → `VA_OUT_GAIN` for `Va`; `va_is_finite_bounded_audible`, `va_is_silent_after_note_off`, `va_is_pitched`
  - `chimera-core/tests/golden_test.rs`: the `va_init` row
  - `chimera-core/tests/loudness_test.rs`: `("VA INIT", init_perf(Va), None)` in `engines()`, `take(3)` → `take(4)`
  - `chimera-core/tests/codec_compat_test.rs`: `init_va.snd` in `sources()`, its byte and render rows, `init_va_fixture_pins_engine_code_3`; fixture `chimera-core/tests/fixtures/v1/init_va.snd`

**Interfaces:**
- Consumes: everything above.
- Produces: `VA_OUT_GAIN` (final), the `va_init` golden, `init_va.snd`.

- [ ] **Step 1: Write the failing tests:**

```rust
/// ADR 0063 at ADR 0109's gain: VA INIT, one C4 at velocity 100 on P1, LEVEL 45/128, within ±1 dB of
/// REFERENCE_LUFS (−11.0), over 4 whole beats of the +6-cent pair (C4: 0.906 Hz, 4.415 s).
#[test] fn va_init_matches_the_loudness_reference() {
    let one = level(Patch::VaInit, &[60], 100, BPS * 4415 / 1000);
    let d = one.lufs + one.limited_db - REFERENCE_LUFS;
    assert!(d.abs() <= 1.0, "VA INIT {d:+.2} dB off the reference");
}
#[test] fn va_is_finite_bounded_audible() { assert_finite_bounded_audible(Case::VaInit) }
#[test] fn va_is_silent_after_note_off()  { assert_silent_after_note_off(Case::VaInit) }
#[test] fn va_is_pitched()                 { assert_pitched(Case::VaInit) }
#[test] fn init_va_fixture_pins_engine_code_3() // decode(init_va.snd).engine() == Va and bits_eq(Sound::init(Va))
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test param_sweep_test va_init --test sanity_test va_`. Expected: the loudness test FAILS at `VA_OUT_GAIN` 1.0 and prints `d`.
- [ ] **Step 3: Set** `VA_OUT_GAIN = 10^(−d/20)` from the printed `d`, to three decimals (expect about 4–5: R is −14 dBFS); rerun until `|d| ≤ 0.25` (TAPE's margin).
- [ ] **Step 4: Record** the new rows only, and no existing golden, fixture or row:
  - `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test -- --nocapture`, then paste `va_init`;
  - `FIXTURE_WRITE=1 cargo test -p chimera-core --test codec_compat_test write_v1_fixtures -- --ignored` writes `init_va.snd` (never an existing file); add its byte and render rows.
- [ ] **Step 5: Verify.** `just test` (the sweep's `levels_are_safe` and `sweep_fast` now include VA INIT), `just clippy`, and `cargo build -p chimera-desktop` (the desktop links VA). Expected: PASS.
- [ ] **Step 6: Commit.** `git add` the files above and `init_va.snd`; `git commit -m "VA INIT at the reference loudness, sanity and goldens"`. Report the flash line.

### Task 11: ADR 0118, the record and the follow-ups

Spec § ADRs, § Later.

**Files:**
- Create: `docs/adr/0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md` (from `docs/adr/0000-template.md`)
- Modify: `docs/adr/README.md` (its row after 0117, or after the last row on `main`)

- [ ] **Step 1: Write ADR 0118,** Status `Accepted (<date>)`, Deciders owner (the spec approval) and firmware. The decision, one point each, each checkable in code:
  1. VA is two variable-wave oscillators, an operator (MODE), a sub locked to OSC 1 and STABILITY. It is an open nod to the Ambika's architecture, from its manual only: no GPL code, no wavetables (ADR 0114).
  2. Every wave, at every SHAPE, is DC-free at one RMS, R = 0.2, with the anchors' fundamentals in phase. The operators scale to stay within 0.9, and `VA_OUT_GAIN` restores the level.
  3. MIX and SYNC pass the −45 dB gate at C6, and RING detuned at C5. XOR and BITS are exempt. RING's coincident edges and its 5% pulse are known exceptions; a product correction is the follow-up.
  4. STABILITY seeds from a splitmix32 hash of (Part, slot, salt). STAB 1 renders bit-identically. Below 1 the character recalls, and drift and phase run free. VA's state rides the slot rebuild as TAPE's strip does. The phase reset switches at exactly STAB = 1, a threshold on a continuous knob.
  5. The technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for `chimera-dsp` (no product types, `prepare(rate)`, `PROVENANCE`) and stay in `chimera-engine/src/dsp/va/` until ADR 0114 is accepted; `dsp/provenance.rs` moves with them.
  6. The rulings that constrain later work: the sine's SHAPE crossfade, the split saw, the one-sample pulse floor, signed threshold crossings, the three-part sync split, `Corrector` as the one kernel slot, PIT under the VA node, and no cell hints without a gesture.
  - Alternatives: the 09-27 draft's 11 one-per-voice models (kept as later macro-oscillator candidates); `chimera-dsp` now (ADR 0114 not accepted); a per-interval RING product correction (simulated, 5 dB worse detuned).
  - Sources: the spec, this plan, Välimäki 2007, Esqueda/Välimäki/Bilbao EUSIPCO 2015, La Pastina & D'Angelo DAFx-22 (CC-BY 4.0), Hohnerlein et al. DAFx-17 (the 4-point upgrade path), the Ambika manual.
- [ ] **Step 2: Add the README row** in the table's form: `| [0118](0118-….md) | VA is two variable-wave oscillators, an operator, a sub and STABILITY (Ambika's architecture, our code); one level for every wave; XOR/BITS alias-exempt; STABILITY seeded by Part and slot; techniques written for chimera-dsp | Accepted |`.
- [ ] **Step 3: File the follow-ups** in joegiralt/chimera, then cite their URLs in ADR 0118:
  - `gh issue create --title "VA RING: correct coincident edges in the product"`, with the body: −31…−38 dB at C6 phase-locked; the per-interval cross term fixes coincidence but is 5 dB worse detuned.
  - `gh issue create --title "VA WAVE: a morph scope focus glyph"`.
- [ ] **Step 4: Verify.** `just check`. Expected: green end to end (tools, every feature set, clippy, loom, fmt, release-bin, stack-check).
- [ ] **Step 5: Commit.** `git add docs/adr/0118-*.md docs/adr/README.md`; `git commit -m "ADR 0118: the VA variable-wave core"`.

**At ship, not in this plan:**
- on-unit bench rows for each MODE (`chimera-stm32/src/bench.rs`, a `bench-va` area), checked against Task 9's bills on rev V and rev Y;
- the owner's listening pass (and the INIT STAB and FINE2 by ear);
- issues #130–#136 and #219 re-scoped to § Later's macro-oscillator list.
