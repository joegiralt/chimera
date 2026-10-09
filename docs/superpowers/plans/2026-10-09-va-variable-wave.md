# VA Variable-Wave Core Implementation Plan

**Goal:** Add VA, Chimera's fourth engine: two band-limited variable-wave oscillators (WAVE morph + SHAPE), five combine modes (MIX, SYNC, RING, XOR, BITS) with a ducked MODE change, a sub an octave below OSC 1 and STABILITY (drift, trim, HF tracking, jitter), plugged in on TAPE's pattern.

**Architecture:**
- **Techniques first, pure and rate-independent.** `dsp/va/blep.rs` (2-point polyBLEP/polyBLAMP and a one-sample-late `Corrector`), `osc.rs` (the variable-wave oscillator, sync and the sub) and `stability.rs` (walks, trim, HF tracking, jitter) take no product types and set their coefficients in `prepare(rate)`. Each declares `PROVENANCE`. When ADR 0114 is accepted they move to `chimera-dsp` unchanged.
- **Then the product layer.** `combine.rs` (the five operators) and `mod.rs` (`VaParams`, `VaEngine`: per-block pitch, per-sample ramps, the MODE duck, seeding) in `chimera-engine` (ADR 0108 opt 2).
- **Then the plumbing, TAPE's way (ADR 0093).** `BlockRef::Va` (block code 35), `EngineType::Va` (engine code 3), a `SlotKind::Va` arm, the VA chain's three pages under one node, host-counted bills, `VA_OUT_GAIN` at the reference loudness.

**Tech Stack:** Rust `no_std` (`chimera-engine`, `chimera-core`), host tests in `chimera-core/tests`, `tools/loop_count.py` (ADR 0056 count) on the `thumbv7em-none-eabihf` release ELF.

**Spec:** `docs/superpowers/specs/2026-10-09-va-variable-wave-design.md` (owner-approved 2026-10-09, with the HF-tracking amendment f85cb91b). Read it, `CLAUDE.md` and TAPE's plan (`docs/superpowers/plans/2026-10-05-tape-core.md`, the template) before any task. Codebase map: (private notes) § 1 and § 4 (every path there was checked against `origin/main` @ 028f2e82 for this plan).

**Decided here (the spec is silent or loose):**
- **The alias gate** is the 09-27 draft's (spec 2026-09-27 § Host tests): 2¹⁵ samples after a 2¹² settle, Blackman–Harris 4-term, bins within ±4 of every multiple of each oscillator's period frequency (and f0/2 with SUB) masked, the rest from 20 Hz to 10 kHz ≤ **−45 dB** of the total; where the uncorrected render folds more than −60 dB, the corrected one is also ≥ 12 dB below it (a test-only `Correction::Off`). RING and MIX run at FINE2 = 0, so one harmonic series is masked; SYNC at OSC 2 CRSE +7.
- **The pitch method** is the draft's: a 2¹⁷-point Hann DFT, quadratic interpolation on the log magnitude.
- **SHAPE 0 is the plain sine.** `sin(g·sin θ)` at g = 1 is not a sine, so the sine's SHAPE crossfades: `y = (1−s)·sin θ + s·sin(g·sin θ)`, g = 1 + 2s. At s = 1 it is the spec's `sin(3·sin θ)`.
- **A SYNC reset gets BLAMP as well as BLEP** (La Pastina & D'Angelo, DAFx-22, CC-BY 4.0): a sine or triangle slave has a slope jump at the reset; the spec's "polyBLAMP at every corner" covers it.
- **Waves share a phase:** sine `sin 2πp`; triangle 0 at p = 0 rising, its peak at `w − ¼` (w = ½ + 0.45·SHAPE in the frame where the trough is at 0); saw falling, `1 − 2p`; square +1 for `p < w`, w = ½ − 0.45·SHAPE. All four fundamentals are in phase, so a crossfade never cancels.
- **Saw lift:** a second rising step at `q = ½·s`, height `h = s`, the whole wave less its mean `h·(1−q)`: peak-safe and DC-free at every s. Square PWM keeps its DC (the voice's 5 Hz blocker takes it, as on analog).
- **BITS' log curve:** `bits(x) = 16 · 8^(−x)` (16, 8, 4, 2 at x = 0, ⅓, ⅔, 1), continuous; `levels = 2^(bits − 1)`, `y = round(v·levels)/levels`.
- **Increments clamp to 0.45** cycles a sample (the draft's rule), so note 127 + CRSE +24 + PITCH +24 stays finite.
- **The Part reaches the engine through `EngineCtx`** (`part: PartId`), so STABILITY seeds by (Part, voice slot) as the spec says. A voice that plays another Part's VA note reseeds; the same Part keeps its walks running.
- **The VA node is VA ↓ OSC 1 ↓ OSC 2 ↓ PIT.** PIT is the voice-pitch sub-page every engine node carries (ADR 0042); the spec's three pages come first.
- **Hints:** VA adds no MIX or hold gesture, so under ADR 0086 no cell has a hint; `va_pages_have_no_gestures` pins it, as TAPE's pages do. If the owner wants descriptive cell help, that is a new ADR, not this plan.
- **Value bars come free:** the cell reads its spec's `Motion` (ADR 0112 point 8): bars on the nine continuous cells, none on CRSE and MODE.
- **`Provenance`** has no type on `main` (ADR 0114 is Proposed). Task 2 adds the minimal closed enum in `chimera-engine/src/dsp/provenance.rs` with the two variants VA uses; it moves to `chimera-dsp` with the technique files.
- **Bills:** Task 6 commits the spec's paper estimates (MIX 310, SYNC 340, RING 320, XOR 320, BITS 330, LP24's 95 included); Task 9 replaces them with host counts.

## Global Constraints

- **Code rules (CLAUDE.md):** no heap, blocking or panic path in audio; no libc; no `unsafe` without a true `// SAFETY:`; parameters lerp and never snap (the owner's never-snap and filter-never-steppy rules).
- **Type-driven:** `VaMode` (`#[repr(u8)]`, 0 MIX, 1 SYNC, 2 RING, 3 XOR, 4 BITS; an unknown code loads as MIX), `OscId { One, Two }`, `Seed`, `Shape`. No bare `u8` mode or `usize` oscillator index in the product layer.
- **Every param declares `Motion` (ADR 0112):** WAVE1/2, SHAPE1/2, AMOUNT, SUB, STAB, FINE1/2 `Continuous(Smoothing::KNOB)`; CRSE1/2 `Stepped(Change::Snap)`; MODE `Stepped(Change::Crossfade)` (the duck). Eleven are mod destinations; MODE is not.
- **Names:** `EngineType::Va` (code 3, ident `"VA"`, label `"VA"`); `BlockRef::Va` (code 35, ident `"VA"`); `SlotKind::Va`; `EngineSlot::Va(VaEngine) => rebuild_va, init_va`; `VA_OUT_GAIN`; `VA_SPECS`; `VaParams`; `dsp/va/`. Param ids, frozen once written: WAVE1 0, WAVE2 1, MODE 2, AMOUNT 3, SUB 4, STAB 5, CRSE1 6, FINE1 7, SHAPE1 8, CRSE2 9, FINE2 10, SHAPE2 11; idents the same tokens (`"STAB"` for STABILITY). MODE's idents `MIX SYNC RING XOR BITS`.
- **INIT = neutral (a format contract, ADR 0087):** WAVE ⅔ both, SHAPE 0 both, CRSE 0, FINE1 0, FINE2 +6, MIX, AMOUNT ½, SUB 0, STAB 0.85; filter CLEAN LP24 (`FilterKind::Svf`, `FilterMode::Lp24`); `Sound::init(Va)` routes ENV 1 → VCA at 127 as TAPE does.
- **Spec constants:** `d = (1 − STAB)²`; drift ±12 cents shared, ±6 each; trim ±3; jitter ±0.5 a cycle; walks 0.05–0.5 Hz; HF tracking `f_eff = f/(1 + f·τ)`, `τ = d · 4 µs · k`, k seeded in 0.6–1.0 per oscillator; sub `y = (c + SUB·s)/(1 + SUB)`.
- **Budget:** 7,000 cycles a sample on rev V, 5,833 on rev Y; the worst VA voice leaves 6 voices beside the bus on both (ADR 0040). Bills by the ADR 0056 method (`tools/loop_count.py`), LP24's 95 carried in each engine term.
- **Memory:** `VaEngine` no larger than `ModalEngine` (the slot must not grow); the voice chain stays within `VOICE_CHAIN_BYTES` 2,112 (ADR 0101; 2,016 today); a Sound record ≤ 640 B (ADR 0102); `MAX_SOUND_COST` re-pinned by `sparse_bound`. If an assert fails, stop and report: never raise a budget in passing.
- **Commits:** a terse subject, no type prefix. Stage by path, never `git add -A`; never stage `chimera.bin` or `docs/chimera-ui-ux-spec.md`.
- **Stale cache:** `touch chimera-engine/src/lib.rs` before each test run that follows an engine edit.
- **Flash:** each firmware-touching task reports one line, `flash: <free> B free (<±delta>)`, from `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf` then `rust-size target/thumbv7em-none-eabihf/release/chimera-stm32`. Cut only real waste; escalate only if free flash falls under ~40 KB.
- **Knobs-rare:** ADRs 0115–0117 sit on the unmerged `knobs-rare` branch (`dsp/knobs.rs`, `voice.rs`). If it merges first, rebase; VA's knobs ride whatever path it lands (the `KNOBS` registry picks them up; 66 + 9 = 75 ≤ 128).
- **No hardware.** Host only. On-unit bench rows come at ship (Task 11 note).
- **Verification:** `just test`, `just clippy`; `just check` green at the end.

## Review Focus

1. **Pitch at the MIDI extremes** (note 127, CRSE +24, PITCH +24, FINE +100; note 0, CRSE −24): every increment clamps to 0.45, never reaches 0, output finite and |y| ≤ 1. *Task 6: `midi_extremes_stay_finite`.*
2. **Routes that push a param past its range** (an LFO at full into WAVE, SHAPE, AMOUNT, SUB or STAB stored at 1, or 0): the engine clamps each to 0..1; finite, peak-safe, no NaN from `sin`, `exp2` or the crush. *Task 6: `out_of_range_values_clamp`.*
3. **MODE scrolled every block under a held note** (a fast encoder): no click, and the voice settles on the last MODE within two blocks of the last turn. *Task 6: `mode_scrolled_every_block_settles`.*
4. **A voice that moves between Parts and engines** (VA on Part 1, then TAPE, then VA on Part 2): the slot rebuilds in place, the walks reseed for Part 2, and Part 1 replayed on the same slot from a fresh boot renders bit-identically. *Task 8: `a_voice_moving_parts_reseeds`.*
5. **ENV 1 → VCA deleted:** VA's raw output never decays, so the key ends the note through the voice fade (TAPE's `key_gates`); no stuck voice. *Task 8: `va_with_no_vca_route_ends_at_key_up`.*

---

### Task 1: The VA params block, on disk

Spec § Pages, § Params and Motion, § Voice and engine (codes).

**Files:**
- Create: `chimera-engine/src/dsp/va/mod.rs` (`VaParams`, `VaMode`, `OscId`, `VA_SPECS`, `MODE_NAMES`, `impl Block`; later tasks add the engine)
- Modify: `chimera-engine/src/dsp/mod.rs` (`pub mod va;`); `chimera-engine/src/addr.rs` (`BlockRef::Va` in the enum, `ALL` [34 → 35], `specs`, `in_sound` → true, `voice_reads` → true); `chimera-engine/src/storage/codes.rs` (`disk_code` 35, `disk_ident` `"VA"`); `chimera-engine/src/params.rs` (`ParamSnapshot.va: VaParams` after `tape`, its arms in `BlockRead::block`, `Blocks::block_mut`, `Default`); `chimera-engine/src/storage/sound.rs` (`MAX_SOUND_COST` and its doc's figures); `chimera-core/src/ui/mod_grid.rs` (`block_tag` `"VA"`, `block_name` `"VA"`)
- Fixtures: `chimera-core/tests/fixtures/disk_codes_v1.txt` (append block 35's rows), `chimera-core/tests/fixtures/neutral_v1.txt` (the `va: VaParams { … }` field after `tape`; no existing line changes)
- Test: `chimera-core/tests/va_params_test.rs` (new), `addr_test.rs` (`modulatable_addresses_are_exactly_the_spec_list` gains VA's eleven), `sound_codec_test.rs` (`sparse_bound`'s `2284` becomes the measured worst)

**Interfaces:**
- Produces (`dsp::va`):
  - `pub enum OscId { One = 0, Two = 1 }`, `OscId::ALL`, `const fn index(self) -> usize`
  - `#[repr(u8)] pub enum VaMode { Mix = 0, Sync = 1, Ring = 2, Xor = 3, Bits = 4 }`, `VaMode::ALL`, `impl DiskCode` (idents above)
  - `pub const MODE_NAMES: [&str; 5] = ["MIX", "SYNC", "RING", "XOR", "BITS"]`
  - `pub struct VaParams { pub wave: [f32; 2], pub shape: [f32; 2], pub coarse: [i8; 2], pub fine: [f32; 2], pub mode: VaMode, pub amount: f32, pub sub: f32, pub stability: f32 }`, indexed by `OscId::index`; `Default` = INIT (Global Constraints); id consts `VaParams::WAVE1 … SHAPE2`
  - `pub static VA_SPECS: [ParamSpec; 12]`: WAVE/SHAPE/AMOUNT/SUB/STAB `ParamSpec::continuous(id, label, ValFmt::Uni, 0.0, 1.0, default, 1.0 / 128.0, true, Smoothing::KNOB)`; FINE `continuous(.., ValFmt::Signed(100), −100.0, 100.0, default, 1.0, true, Smoothing::KNOB).cents(100.0)`; CRSE `ParamSpec::stepped(.., ValFmt::Signed(24), −24.0, 24.0, 0.0, true, Motion::Stepped(Change::Snap)).semitones(24.0)` (PIT's PITCH precedent); MODE `ParamSpec::choice(2, "MODE", ValFmt::Names(&MODE_NAMES), 4.0, 0.0, Change::Crossfade).glyph(FocusGlyph::None)`. Labels: `WAVE1 WAVE2 MODE AMNT SUB STAB CRSE1 FINE1 SHAPE1 CRSE2 FINE2 SHAPE2` (the matrix tells the two oscillators apart; pages relabel, Task 7). Glyphs: WAVE `Wave`, AMOUNT `Crossfader`, SUB `LevelBar`.
  - `impl Block for VaParams`: CRSE writes `round(v).clamp(−24, 24) as i8` (a modulated CRSE is stepped); MODE through `enum_code`/`enum_ident`/`set_enum_code`.

- [ ] **Step 1: Write the failing tests** (`va_params_test.rs`):

```rust
#[test] fn init_is_the_spec() {
    let p = VaParams::default();
    assert_eq!((p.wave, p.shape, p.coarse, p.fine), ([2.0 / 3.0; 2], [0.0; 2], [0; 2], [0.0, 6.0]));
    assert_eq!((p.mode, p.amount, p.sub, p.stability), (VaMode::Mix, 0.5, 0.0, 0.85));
}
#[test] fn every_param_declares_its_motion() // the nine Continuous(KNOB), CRSE1/2 Stepped(Snap), MODE Stepped(Crossfade); modulatable exactly the eleven non-MODE
#[test] fn all_twelve_round_trip_on_disk()  // Sound::init(Algo) with every VA param off neutral (WAVE 0.1/0.9, SHAPE 0.3/0.7, CRSE −24/+24, FINE −100/+100, BITS, AMOUNT 1, SUB 1, STAB 0): encode, decode, bits_eq
#[test] fn unknown_mode_code_loads_as_mix() // on a neutral VaParams, set_enum_code(VaParams::MODE, 9) is false and mode stays VaMode::Mix; VaMode::from_disk_code(9) == None
#[test] fn crse_rounds_a_modulated_value()  // Block::write(CRSE1, 6.4) reads back 6.0; write(CRSE1, −24.6) reads −24.0
#[test] fn block_35_is_va()                 // BlockRef::Va.disk_code() == Some(35), disk_ident() == Some("VA"), BlockRef::from_disk_code(35) == Some(BlockRef::Va)
```

- [ ] **Step 2: Run them.** `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test va_params_test`. Expected: FAIL to compile (no `dsp::va`).
- [ ] **Step 3: Implement** the Interfaces above and the arms in the Files list. Run `cargo test -p chimera-core --test disk_codes_test --test sound_codec_test --test addr_test`; append block 35's printed rows to `disk_codes_v1.txt`, add the `va:` field to `neutral_v1.txt`, and set `sparse_bound`'s worst and `MAX_SOUND_COST` (its next multiple of 16) to the printed values. No other fixture row may change.
- [ ] **Step 4: Verify.** `just test` and `just clippy`. Expected: PASS; every golden bit-identical (a neutral VA block is never written, so no saved Sound changes), `param_motion_test` passes with nine more knobs.
- [ ] **Step 5: Commit.** `git add` the files above; `git commit -m "VA params block, block code 35"`. Report the flash line.

### Task 2: Provenance and the 2-point kernels

Spec § Oscillator (Antialiasing), § Code layout. ADR 0114 point 4.

**Files:**
- Create: `chimera-engine/src/dsp/provenance.rs`, `chimera-engine/src/dsp/va/blep.rs`
- Modify: `chimera-engine/src/dsp/mod.rs` (`pub mod provenance;`), `chimera-engine/src/dsp/va/mod.rs` (`pub mod blep;`)
- Test: `chimera-core/tests/va_blep_test.rs`

**Interfaces:**
- Produces (`dsp::provenance`): `pub enum Patent { NotChecked, Checked, OlderThan20Years }`; `pub enum Provenance { Textbook { cite: &'static str, patent: Patent }, Original { author: &'static str } }`.
- Produces (`dsp::va::blep`):
  - `pub const PROVENANCE: &[Provenance]`: Välimäki & Huovilainen 2007 (polyBLEP) and Esqueda, Välimäki & Bilbao, EUSIPCO 2015 (2-point polyBLAMP), each `Textbook`, `Patent::NotChecked`.
  - `pub fn blep(t: f32) -> [f32; 2]`, `pub fn blamp(t: f32) -> [f32; 2]`: the residuals for the sample before and the sample after an event `t` samples (0..1) before the later one.
  - `pub struct Corrector`, `Corrector::NEW`, `pub const LATENCY: usize = 1`, `pub fn edge(&mut self, t: f32, step: f32, slope: f32)` (`slope` = the jump in output per sample), `pub fn push(&mut self, naive: f32) -> f32` (the corrected sample `LATENCY` samples late), `pub fn clear(&mut self)`.
- **The slot for a better kernel:** every caller goes through `Corrector::edge`/`push`. A 4-point polyBLAMP (Hohnerlein et al., DAFx-17) or another corrected kernel replaces `blep`/`blamp` and `LATENCY` here and nothing else; the oscillator tests (Tasks 3–4) are the gate.

The algorithm (not fixed by the signatures): with the bandlimited step `B` from the integrated triangular kernel, the residual `B − H` is `½t²` on the sample before the event and `−½(1−t)²` on the sample after; integrated once more (the ramp), `t³/6` and `(1−t)³/6`. So `blep(t) = [½t², −½(1−t)²]`, `blamp(t) = [t³/6, (1−t)³/6]`. `Corrector` keeps the previous naive sample: `edge` adds `step·blep(t)[0] + slope·blamp(t)[0]` to it and the `[1]` terms to an accumulator; `push` returns the held sample and holds `naive + accumulator`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn kernels_at_their_ends()   // blep(0) == [0, −0.5], blep(1) == [0.5, 0]; blamp(0) == [0, 1/6], blamp(1) == [1/6, 0]; blep(0.5) == [0.125, −0.125]
#[test] fn blep_keeps_the_edge_time()  // t in 0..=1 step 1/64: blep(t)[0] + blep(t)[1] == t − 0.5 within 1e-6 (the step's centroid lands at the event)
#[test] fn blamp_integrates_blep()     // blamp(t)[0] == ∫₀ᵗ blep(u)[0] du and blamp(t)[1] == −∫ₜ¹ blep(u)[1] du, by a 1 024-step f64 midpoint sum, within 1e-5
#[test] fn corrector_is_one_sample_late() // push(1.0), push(2.0), push(3.0) with no edges returns 0.0, 1.0, 2.0
#[test] fn corrector_applies_both_halves() // edge(0.25, 2.0, 0.0) between push(−1.0) and push(1.0): the two outputs are −1 + 2·blep(0.25)[0] and 1 + 2·blep(0.25)[1]
#[test] fn provenance_is_declared()    // blep::PROVENANCE non-empty, every Textbook entry Patent::NotChecked
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_blep_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA kernels: 2-point polyBLEP and polyBLAMP"`. Flash: host only (nothing links it yet).

### Task 3: The variable-wave oscillator

Spec § Oscillator (WAVE, SHAPE, Antialiasing), § Testing (alias gate, pitch, library-ready). Precedent: Ambika's variable-waveform oscillator, architecture only (manual), no code or tables (ADR 0114).

**Files:**
- Create: `chimera-engine/src/dsp/va/osc.rs`, `chimera-core/tests/common/spectrum.rs` (test helper: an in-place radix-2 `fft`, `alias_db`, `peak_hz`; no new dependency)
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`pub mod osc;`), `chimera-core/tests/common/mod.rs` (`pub mod spectrum;`)
- Test: `chimera-core/tests/va_osc_test.rs`

**Interfaces:**
- Consumes: `Corrector`, `blep`, `blamp` (Task 2); `crate::dsp::fast_sin_turns`.
- Produces (`dsp::va::osc`):
  - `pub const PROVENANCE`: the phase accumulator `Textbook`; the waves and SHAPE laws `Original { author: "AI-assisted, owner-directed and reviewed" }`.
  - `pub struct Shape { pub wave: f32, pub shape: f32 }` (each 0..1, clamped on use)
  - `pub fn wave_at(phase: f32, s: Shape) -> (f32, f32)`: the naive value and its slope per cycle (the morph's two waves, weighted); the reference for tests and for sync.
  - `pub enum Correction { On, Off }` (`Off`: tests only, the gate's baseline)
  - `pub struct VarOsc`; `VarOsc::NEW`; `pub fn prepare(&mut self, sample_rate: f32)`; `pub fn inc_for(&self, hz: f32) -> f32` (clamped to `[0, MAX_INC]`); `pub const MAX_INC: f32 = 0.45`; `pub fn reset(&mut self)` (phase 0, corrector cleared); `pub fn phase(&self) -> f32`; `pub fn with_correction(self, c: Correction) -> Self`
  - `pub struct Tick { pub out: f32, pub wrap: Option<f32> }` (`wrap`: `t`, samples since the phase wrapped this sample)
  - `pub fn tick(&mut self, inc: f32, s: Shape, sync: Option<f32>) -> Tick`: `sync` is Task 4's; this task passes `None`.
- **The slot for ADAA:** the sine fold is a private `VarOsc` method, so an antiderivative step with state can replace it inside the struct.

The waves and their edges (the algorithm): WAVE `x` picks segment `k = min(⌊3x⌋, 2)` and `f = 3x − k` between anchors k and k+1 (sine, triangle, saw, square); only the two weighted waves are evaluated. Shapes as in "Decided here". Each wave reports its steps and corners crossed between the last sample and this one at their fractional `t`: triangle corners (slope jump `±(2/w + 2/(1−w))·inc`, BLAMP), saw wrap (step `+(2 − h)`) and lift (step `+h` at `q`, BLEP), square edges (`+2` at the wrap, `−2` at `w`, BLEP). Each edge's step and slope are scaled by its wave's morph weight before `Corrector::edge`. The sine (SHAPE fold included) is not corrected.

- [ ] **Step 1: Write the failing tests** (`f0 = 1046.50` is C6):

```rust
#[test] fn corrected_saw_beats_naive()       // Task 2's Corrector on a 1 kHz falling saw at 48 kHz (step +2 at each wrap): alias_db ≤ −45 and ≥ 12 dB under the naive saw
#[test] fn anchors_are_their_plain_selves()   // SHAPE 0, inc 1/4800, Correction::Off, samples ≥ 2 from any edge: WAVE 0 → sin 2πp, ⅓ → triangle (0 rising at p=0, +1 at ¼), ⅔ → 1 − 2p, 1 → square; |err| ≤ 1e-4
#[test] fn wave_morph_is_a_linear_crossfade() // WAVE 1/6: out == ½ sine + ½ triangle (Correction::Off) within 1e-6; WAVE 0.5: ½ triangle + ½ saw
#[test] fn shape_does_what_the_table_says()   // SHAPE 1: sine == sin(3 sin θ) within 1e-3; triangle's peak at p = 0.7 (= 0.95 − ¼); square's +1 share 0.05 ± 0.005; saw lift's second step at p = ½, height 1; SHAPE 0 == the plain anchor
#[test] fn in_tune_at_every_rate()            // each anchor and midpoint at A4, SHAPE 0, at 44_100, 48_000, 96_000, 192_000 Hz: spectrum::peak_hz within 1 cent of 440
#[test] fn alias_gate_at_c6()                 // 48 kHz, WAVE in {0, 1/6, 1/3, 1/2, 2/3, 5/6, 1} × SHAPE in {0, 0.5, 1}: alias_db ≤ −45; where Correction::Off folds > −60 dB, ≥ 12 dB below it
#[test] fn edges_never_overshoot()            // same grid at C2, C4, C6, C7: every |out| ≤ 1 + 1e-6
#[test] fn morphing_never_clicks()            // WAVE, then SHAPE, 0 → 1 over 100 ms ramped per sample at A4: max |x[n] − 2x[n−1] + x[n−2]| ≤ 1.5 × the largest in held renders at 0, ¼, ½, ¾, 1
```

`spectrum::alias_db(x: &[f32], sample_rate: f32, f0s: &[f32]) -> f64` and `spectrum::peak_hz(x: &[f32], sample_rate: f32, near: f32) -> f64` implement the two methods in "Decided here".

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_osc_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS. If the gate fails on narrow pulses at SHAPE 1, stop and report (the 4-point kernel is Task 2's swap, an owner call), never loosen −45 dB.
- [ ] **Step 5: Commit.** `git commit -m "VA oscillator: WAVE morph and SHAPE, band-limited"`. Flash: host only.

### Task 4: Sync, the sub and the five operators

Spec § Combine (table, `xor16`, BITS, Sub), § Testing (peak-safe, alias gate in SYNC).

**Files:**
- Create: `chimera-engine/src/dsp/va/combine.rs`
- Modify: `chimera-engine/src/dsp/va/osc.rs` (`tick`'s `sync`, `Sub`), `chimera-engine/src/dsp/va/mod.rs` (`pub mod combine;`)
- Test: `chimera-core/tests/va_combine_test.rs`

**Interfaces:**
- Consumes: `VarOsc`, `Tick`, `Shape`, `wave_at`, `Corrector` (Tasks 2–3); `VaMode` (Task 1).
- Produces (`osc`):
  - `tick(.., sync: Some(t))`: the slave's phase restarts `t` samples ago (`phase = t·inc`), with a BLEP for the value jump and a BLAMP for the slope jump between `wave_at(old)` and `wave_at(new)` at the reset.
  - `pub struct Sub`, `Sub::NEW`, `pub fn reset(&mut self)` (+1), `pub fn tick(&mut self, wrap: Option<f32>) -> f32`: ±1, flipped at each master wrap, BLEP-corrected (step ±2), its own `Corrector`, so it lands one sample late like the oscillators.
- Produces (`combine`):
  - `pub fn xor16(a: f32, b: f32) -> f32`: each `(clamp(v, −1, 1) · 32767.0) as i16`, XORed, `as f32 / 32768.0`
  - `pub fn bits(x: f32) -> f32`: `16 · 8^(−x)` via `fast_exp2`
  - `pub struct Crusher` (`Crusher::NEW`; holds the per-block level so an ADAA step can later add state): `pub fn set(&mut self, x: f32)` once a block, `pub fn process(&self, v: f32) -> f32`
  - `pub fn combine(mode: VaMode, a: f32, b: f32, x: f32, crush: &Crusher) -> f32`: the spec's table; SYNC as MIX.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn operators_are_the_table()   // a, b, x on a 1/16 grid: Mix/Sync == (1−x)a + xb; Ring == (1−x)·½(a+b) + x·a·b; Xor == (1−x)·½(a+b) + x·xor16(a,b); Bits with x == crush of ½(a+b); exact to 1e-6
#[test] fn xor16_works_on_words()      // xor16(0.5, 0.25) == 8192.0 / 32768.0; xor16(a, a) == 0.0; xor16(−1.0, 0.0) == −32767.0 / 32768.0
#[test] fn bits_follow_the_log_curve() // bits(0) == 16, bits(1/3) ≈ 8, bits(2/3) ≈ 4, bits(1) == 2 (1e-4); at x = 1 the crush outputs only {−1, −½, 0, ½, 1}; at x = 0, |crush(v) − v| ≤ 2^−15
#[test] fn every_operator_is_peak_safe() // every mode, a and b on [−1, 1] step 1/64, x in {0, ¼, ½, ¾, 1}: |y| ≤ 1
#[test] fn sync_restarts_the_slave()   // master inc 0.0123, slave 0.031: after each master wrap at t, slave.phase() == t · 0.031 within 1e-6
#[test] fn sync_passes_the_alias_gate() // master C6, slave at 2^(7/12)× and 2.37×, slave WAVE in the Task 3 grid, SHAPE 0 and 1: alias_db (master's series masked) ≤ −45, ≥ 12 dB under Correction::Off
#[test] fn sub_is_an_octave_down()     // master at 440: the sub's peak_hz within 1 cent of 220; its sign flips exactly at the master's wraps
#[test] fn sub_passes_the_alias_gate()  // the sub alone at C6's f0/2: ≤ −45 dB
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_combine_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA sync, sub and the five operators"`. Flash: host only.

### Task 5: STABILITY

Spec § Stability (drift, tolerance, HF tracking, jitter, recallable), § Testing (Stability, HF tracking, library-ready).

**Files:**
- Create: `chimera-engine/src/dsp/va/stability.rs`
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`pub mod stability;`)
- Test: `chimera-core/tests/va_stability_test.rs`

**Interfaces:**
- Produces (`dsp::va::stability`), no product types; oscillators are `[_; 2]`:
  - `pub const PROVENANCE`: `Original` (the walks, trim and jitter); `Textbook` for the HF-tracking law (a VCO core's reset time added to each cycle).
  - `pub const DRIFT_SHARED_CENTS: f32 = 12.0`, `DRIFT_OWN_CENTS = 6.0`, `TRIM_CENTS = 3.0`, `JITTER_CENTS = 0.5`, `TAU_MAX_S = 4.0e-6`, `WALK_LO_HZ = 0.05`, `WALK_HI_HZ = 0.5`
  - `pub fn depth(stability: f32) -> f32`: `(1 − clamp(s, 0, 1))²`
  - `pub struct Seed(u32)`, `pub const fn Seed::new(raw: u32) -> Seed` (forced odd, never 0)
  - `pub struct Stability`; `Stability::NEW`; `pub fn prepare(&mut self, update_hz: f32)` (the rate `step` is called at: the block rate for VA); `pub fn seed(&mut self, s: Seed)` (the walks' state, the trims in −1..1 and the τ factors in 0.6..1.0, all from one xorshift stream in a fixed order); `pub fn step(&mut self, d: f32) -> [f32; 2]` (cents: drift + trim, per oscillator, once a block); `pub fn tracked(&self, hz: [f32; 2], d: f32) -> [f32; 2]` (`f/(1 + f·τ)`); `pub fn jitter(&mut self, d: f32) -> f32` (cents for the cycle a wrap starts); `pub fn tau_factors(&self) -> [f32; 2]` (tests)
  - With `d == 0.0`, `step` and `jitter` return exactly 0.0 and `tracked` returns `hz` unchanged (the walks still advance), so STAB 1 is bit-identical.

A walk: white xorshift noise each step, a one-pole high-pass at `WALK_LO_HZ` then a one-pole low-pass at `WALK_HI_HZ` (coefficients from `update_hz` in `prepare`), a fixed gain and a `clampf` to ±1. Choose the gain so `walk_is_the_band` passes.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn depth_curve()               // depth(1) == 0, depth(0) == 1, depth(0.5) == 0.25, depth(0.85) within 1e-6 of 0.0225
#[test] fn walk_is_the_band()          // 600 s at 48 kHz / 64: |w| ≤ 1, rms in 0.25..0.6, zero crossings 0.1..1.0 a second
#[test] fn walks_are_rate_independent() // update rates 44_100/64, 48_000/64, 96_000/64, 192_000/64: crossing rate and rms each within 25 % of 48 kHz's
#[test] fn drift_stays_in_bounds()     // d in {1, 0.25}, 600 s, 8 seeds: every |step(d)[k]| ≤ d · (12 + 6 + 3)
#[test] fn the_pair_drifts_together()  // d = 1, 600 s: corr(step[0], step[1]) ≥ 0.5 (the shared walk dominates)
#[test] fn jitter_in_bounds()          // 10^5 draws at d = 1 and 0.25: |jitter| ≤ 0.5 · d, mean within 0.02 · d of 0
#[test] fn hf_tracking()               // d = 1, 32 seeds: C7 (2093.0 Hz) between −15 and −8 cents; A2 (110 Hz) within 1 cent of true; d = 0: tracked(hz) == hz bit for bit; where the two τ factors differ, |f1 − f2| at C7 > at C4 (261.63 Hz)
#[test] fn hf_tracking_is_rate_independent() // tracked() at C7 is the same at all four rates (τ is seconds, not samples)
#[test] fn seeded_is_recallable()      // two Stability with Seed::new(0x0203), same prepare, 10^4 steps: identical bits; Seed::new(0x0103) differs
#[test] fn depth_zero_is_silent_bits() // d = 0: step returns [0.0; 2] exactly and jitter 0.0, over 10^4 calls
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_stability_test`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "VA STABILITY: drift, trim, HF tracking, jitter"`. Flash: host only.

### Task 6: `VaEngine`

Spec § Oscillator (Pitch), § Combine (MODE changes, Sub), § Stability (phase at note-on, sub follows OSC 1, seeding), § Testing (no clicks, every knob audible, stability, peak-safe, alias gate in MIX/SYNC/RING, pitch).

**Files:**
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`VaEngine`, `VA_OUT_GAIN`, the bills)
- Test: `chimera-core/tests/va_engine_test.rs`

**Interfaces:**
- Consumes: Tasks 1–5; `crate::dsp::ease::{Ramp, at, step_of}`, `crate::dsp::glide::Glide`, `crate::dsp::fast_exp2`, `crate::project::PartId`, `crate::voice_alloc::VoiceIdx`, `crate::hw::Cost`.
- Produces (`dsp::va`):
  - `pub const VA_OUT_GAIN: f32 = 1.0` (Task 10 sets it)
  - `pub const COST_MIX: Cost = Cost(310)`, `COST_SYNC = Cost(340)`, `COST_RING = Cost(320)`, `COST_XOR = Cost(320)`, `COST_BITS = Cost(330)` (the spec's paper estimates, LP24's 95 included; Task 9 re-bills)
  - `pub struct VaEngine`; `pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self`; `pub fn place(&mut self, voice: VoiceIdx)`; `pub fn set_pitch(&mut self, cents: f32)`; `pub fn note_on(&mut self, note: u8, p: &VaParams, part: PartId, sample_rate: u32)`; `pub fn glide_on(&mut self, note: u8, p: &VaParams, part: PartId, (sample_rate, tau): (u32, f32))`; `pub fn glide_to(&mut self, note: u8, (sample_rate, tau): (u32, f32))`; `pub fn note_off(&mut self)` (nothing: the VCA ends it); `pub fn hold(&mut self)` (a fresh note's ramps start on their values); `pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], p: &VaParams, sample_rate: u32)`; `pub fn is_active(&self) -> bool` (from note-on on; VA never decays); `pub fn mode(&self) -> VaMode` (the one playing); `pub const fn mode_cost(m: VaMode) -> Cost`; `pub fn cost(p: &VaParams) -> Cost`; `pub fn playing_cost(&self) -> Option<Cost>` (the playing mode's, while active); `#[cfg(any(test, feature = "test-support"))] pub fn slide(&self) -> f32` and `pub fn uncorrected(&mut self)`
  - `const _: () = assert!(size_of::<VaEngine>() <= size_of::<ModalEngine>());`

The render (the algorithm the signatures leave open):
- **Once a block:** clamp every value to its range; `d = depth(stab)`; `cents_k = note·100 + pitch + coarse_k·100 + fine_k + stab.step(d)[k]`; `hz_k = 440 · fast_exp2((cents_k − 6900)/1200) · slide`; `hz = stab.tracked(hz, d)`; `inc_k = osc.inc_for(hz_k)`, ramped linearly across the block from last block's (`ease::Ramp`). WAVE, SHAPE, AMOUNT and SUB ramp per sample the same way; `crusher.set(amount)` in BITS.
- **Per sample:** OSC 1 ticks; at its wrap the sub flips, OSC 1 draws `stab.jitter(d)` for its next cycle (`inc · (1 + jitter·ln2/1200)`), and in SYNC its `t` is OSC 2's `sync`; OSC 2 ticks (its own wrap draws its jitter); `c = combine(mode, a, b, amount, &crusher)`; `y = (c + sub·s)/(1 + sub)`; times the duck's gain.
- **One dispatch a block:** `render` matches the playing mode once and calls one `#[inline(never)]` inner loop per mode (`render_mix`, `render_sync`, `render_ring`, `render_xor`, `render_bits`), so Task 9 counts each.
- **The duck:** a MODE that differs from the playing one starts `Down` (gain 1 → 0 across the block); at its end the mode swaps (entering SYNC clears OSC 2's corrector) and `Up` runs 0 → 1 the next block. A change during `Down` retargets it; one during `Up` starts `Down` next block.
- **Note-on:** a new `sample_rate` re-runs `osc.prepare(sr)` and `stab.prepare(sr / BLOCK_SIZE)`. `stability ≥ 1.0` resets both oscillators and the sub (phase 0, +1, correctors cleared); below, they free-run. The seed is `Seed::new((part.index() as u32) << 8 | voice.index() as u32)` (the `VoiceIdx` from `place`), set when the Part differs from the last seeded one. `glide_on`/`glide_to` glide `slide` as TAPE does (`Glide`, log₂, once a block).

- [ ] **Step 1: Write the failing tests** (A4, 48 kHz, INIT unless said; "the click rule" = max second difference ≤ 1.5 × the largest of held renders at 0, ¼, ½, ¾, 1):

```rust
#[test] fn every_anchor_in_tune_at_a4()      // OSC 1 alone (AMOUNT 0, STAB 1, SUB 0), WAVE 0, ⅓, ⅔, 1: peak_hz within 1 cent of 440
#[test] fn alias_gate_in_mix_sync_ring()     // C6, STAB 1, SUB 0, FINE2 0; SYNC with OSC 2 CRSE +7: WAVE1 = WAVE2 over the Task 3 grid × SHAPE {0, ½, 1} × {Mix, Sync, Ring}: ≤ −45 dB, ≥ 12 dB under uncorrected() where it folds > −60 dB; plus MIX at SUB 1 with f0/2 masked
#[test] fn every_knob_sweeps_without_clicks() // WAVE1/2, SHAPE1/2, AMOUNT, SUB, STAB, FINE1/2, each 0 → 1 over 100 ms set per block, others 0.3, in Mix, Sync and Ring: the click rule
#[test] fn mode_changes_never_click()        // every ordered pair of Mix, Sync, Ring changed mid-note: the click rule
#[test] fn the_duck_is_one_block_each_way()  // a change at block k: |y| in block k ≤ (1 − (i+1)/64) · steady peak + 1e-6; block k+1 ≤ ((i+1)/64) · peak + 1e-6; mode() is the new one from block k+1
#[test] fn every_knob_is_audible()           // each of the nine at 0 vs 1, others 0.3, 1 s, same seed: rms(y0 − y1) ≥ 0.1 · rms(y0)
#[test] fn peak_safe_everywhere()            // every MODE × AMOUNT {0, ½, 1} × SUB {0, ½, 1} × WAVE anchors × SHAPE {0, 1}, 0.25 s at C2 and C6: |y| ≤ 1 + 1e-6
#[test] fn stability_one_is_bit_identical()  // STAB 1: two fresh engines on different (part, voice) and three re-strikes on one engine render the same first 0.5 s, bit for bit
#[test] fn below_one_is_recallable()         // STAB 0.5, Part 2 voice 3, the same notes on two fresh engines: identical bits; Part 1 voice 3: differs
#[test] fn below_one_free_runs()             // STAB 0.5: a second note's first sample is not phase 0 (OSC 1's phase ≠ 0 at note-on)
#[test] fn sub_follows_osc1_tracked()        // STAB 0, C7: the sub's peak_hz is half OSC 1's within 0.5 cent
#[test] fn steal_glide_glides()              // glide_on at tau 0.1 from an octave up: slide reaches 63 % of the way in 0.1 s ± 1 block
#[test] fn bills_follow_the_mode()           // mode_cost: Mix 310, Sync 340, Ring 320, Xor 320, Bits 330; playing_cost == Some(mode_cost(mode())) while active, None before note_on
#[test] fn midi_extremes_stay_finite()       // Review Focus 1: note 127 + CRSE +24 + set_pitch(2400) + FINE +100; note 0 + CRSE −24: 1 s each, all finite, |y| ≤ 1 + 1e-6, the output still moves (rms > 1e-3)
#[test] fn out_of_range_values_clamp()       // Review Focus 2: wave 1.7/−0.3, shape −0.4/1.9, amount 2, sub 3, stability −1 and 2: finite, |y| ≤ 1 + 1e-6
#[test] fn mode_scrolled_every_block_settles() // Review Focus 3: MODE stepped Mix→Sync→Ring→Mix… every block for 40 blocks, then held at Ring: the click rule over the whole run (XOR and BITS left out, exempt), mode() == Ring within 2 blocks of the last change
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test va_engine_test`. Expected: FAIL to compile (no `VaEngine`).
- [ ] **Step 3: Implement** the Interfaces. If RING fails the gate on saw × saw, stop and report; never loosen it.
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
  - `pub static VA_HOME: BlockDef` id 87, name `"VA"`, short `"VA"`, `CellGrid`, `VizType::None`: WAVE1, WAVE2, MODE, AMOUNT `.with_label("AMNT")`, SUB, STAB
  - `pub static VA_OSC1: BlockDef` id 88, `"Osc 1"`, `"OSC1"`: CRSE1 `.with_label("CRSE")`, FINE1 `"FINE"`, SHAPE1 `"SHAPE"`, then `EMPTY` ×3; `VA_OSC2` id 89, `"Osc 2"`, `"OSC2"`, the same for OSC 2. Ids 87–89 are free and under the focus table's top (95), so it doesn't grow.
  - `static VA_SUB_PAGES: [&BlockDef; 3] = [&VA_OSC1, &VA_OSC2, &PITCH]`
  - `static VA_BLOCKS: [ChainBlock; 5]`: `with_subs(&VA_HOME, &VA_SUB_PAGES)`, `page(&DRIVE)`, `with_subs(&FILTER, &FILTER_SUB_PAGES)`, `page(&FOLDER)`, MOD as TAPE's
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
- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS; `no_page_id_is_reserved` and the focus table unchanged in size.
- [ ] **Step 5: Commit.** `git commit -m "VA pages: VA, OSC 1, OSC 2"`. Report the flash line.

### Task 8: `EngineType::Va` in the voice

Spec § Voice and engine. ADR 0051 (slot), ADR 0093 (TAPE's pattern).

**Files:**
- Modify:
  - `chimera-engine/src/params.rs`: `EngineType::Va = 3`, `DiskCode` (3, `"VA"`), `ALL` [3 → 4], `label` `"VA"`
  - `chimera-engine/src/dsp/filter/mod.rs:225`: `default_for` → `Svf` for `Va`
  - `chimera-engine/src/dsp/engines.rs`: `SlotKind::Va` (`of`, `engine`); `Va(VaEngine) => rebuild_va, init_va;` in `in_place_enum!`; an arm in `init_in_place` (`place(VoiceIdx::ALL[0])`, a lone slot being voice 0's, as TAPE's `seed(0)`), `kind`, `out_gain` (`VA_OUT_GAIN`), `build` (`place(voice)`), `sym_note_on_clear`, `note_on` (`(Va, None)` → `note_on(.., ctx.part, ..)`, `(Va, Some(tau))` → `glide_on`), `retune`, `retune_clear`, `slide`, `note_off`, `heads` (NONE), `source` (None), `sustains` (false), `hold`, `render`, `set_pitch` (cents, as TAPE), `is_active`, `playing_cost`, `model_cost` (`VaEngine::cost(&p.va)`), `cost` (`VaEngine::cost(&p.va)`); `EngineCtx` gains `pub part: PartId`; the in-file tests' `kinds`, tag 3 and `lease_slot` arms
  - `chimera-engine/src/instrument.rs` (each `EngineCtx { .. }`, lines 874, 914, 985, 1038, 1252): `part: PartId::ALL[q]`, the Part the voice plays (`sounding[v]`)
  - `chimera-engine/src/dsp/voice.rs`: `key_gates` (`:751`) matches `EngineSlot::Tape(_) | EngineSlot::Va(_)`; the no-route arm (`:999`) gains `| EngineSlot::Va(_)`; its test's `EngineCtx`
  - `chimera-engine/src/preset.rs:39`: ENV 1 → VCA at 127 for `Tape | Va`
  - `chimera-core/src/ui/nav.rs:887`: `EngineType::Va => &block_registry::VA_CHAIN`
  - Tests' `EngineCtx { .. }` (`engines_test.rs`, `in_place_test.rs`, `tape_source_test.rs`, `param_sweep_test.rs`, `tape_voice_test.rs`, `common/rig.rs`): `part: PartId::ALL[0]`; `common/mod.rs` `init_params` and `expects_sound` arms
- Test: `engines_test.rs`, `va_pages_test.rs`, `chimera-core/tests/va_voice_test.rs` (new)

**Interfaces:**
- Consumes: `VaEngine` (Task 6), `VA_CHAIN` (Task 7).
- Produces: `EngineType::Va`; `SlotKind::Va`; `EngineCtx::part`. The browser's INIT rows (`INIT_TYPES = EngineType::ALL`) gain `VA` without a code change.

- [ ] **Step 1: Write the failing tests:**

```rust
// engines_test.rs: all_lists_every_engine_once_in_order and every_engine_renders_per_its_expectation now cover Va (ALL.len() == EngineType::Va as usize + 1)
#[test] fn va_row_lives_until_its_vca_ends() // va_voice_test: Sound::init(Va) through common::rig: sounds while held; silent (< 1e-4) within ENV 1's release + the fade after key-up
#[test] fn va_with_no_vca_route_ends_at_key_up() // Review Focus 5: ENV 1 → VCA deleted: the voice fades out within Voice::FADE samples of key-up and goes idle
#[test] fn a_voice_moving_parts_reseeds()    // Review Focus 4: STAB 0.5. Slot: VA on Part 1, TAPE, MODAL, VA on Part 2 (rebuild in place each time, kind() follows); a fresh Instrument replaying Part 1's note on the same slot renders bit-identically to the first; Part 2's differs
#[test] fn va_init_pins_engine_code_3()      // EngineType::Va.disk_code() == 3, disk_ident() == "VA", from_disk_code(3) == Some(Va)
#[test] fn init_routes_env1_to_vca()         // Sound::init(Va).mod_state's fixed VCA cell for ENV 1 == 127; filter Svf, mode Lp24
// va_pages_test.rs
#[test] fn a_va_load_lands_on_va()           // load_init(&mut ui, EngineType::Va): page_def_id == VA_HOME.id; nav::chain_def_for(Va) is &VA_CHAIN
#[test] fn edit_walks_and_wraps()            // EDIT from VA: OSC1, OSC2, PIT, then VA again (tap(&mut ui, ButtonId::Edit))
```

- [ ] **Step 2: Run them.** `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test engines_test --test va_voice_test --test va_pages_test`. Expected: FAIL to compile (no `EngineType::Va`).
- [ ] **Step 3: Implement** the Files list. The `size_of` assert from Task 6 holds; the voice-chain assert holds (VaParams adds about 40 B to the played snapshot: 2,016 → about 2,056 of 2,112).
- [ ] **Step 4: Verify.** `just test`, `just clippy`. Expected: PASS. Screen goldens that draw the browser's INIT rows gain `INIT VA`: re-record those cases only (`GOLDEN_RECORD=1`, then `just screens`), and say which in the commit body.
- [ ] **Step 5: Commit.** `git commit -m "VA plays: EngineType::Va in the slot, the voice and the chain"`. Report the flash line.

### Task 9: Bills by count

Spec § Budget and billing. ADR 0056 (method), ADR 0040 (voices).

**Files:**
- Modify: `chimera-engine/src/dsp/va/mod.rs` (the five `COST_*` from the count, each with its doc line `instr × 1.46 × 1.1 → cycles, + 95 LP24`)
- Test: `chimera-core/tests/cost_test.rs` (`va_bills_each_mode`, `a_held_va_note_bills_its_playing_mode`), `va_engine_test.rs` (`bills_follow_the_mode`'s values)

**Interfaces:**
- Consumes: `VaEngine::{mode_cost, cost, playing_cost}` (Task 6), the `EngineSlot` arms (Task 8), `Voice::held_model_extra`.
- Produces: the committed `COST_MIX … COST_BITS`.

- [ ] **Step 1: Count.** `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`, then for each mode `python3 tools/loop_count.py target/thumbv7em-none-eabihf/release/chimera-stm32 render_<mode>` (`render_mix`, `render_sync`, `render_ring`, `render_xor`, `render_bits`). Take the worst path (a wrap and every edge taken in one sample, both waves of a mid-morph live); bill = its cycles + per-block work ÷ 64 + 95, rounded up to 10. Record each count in the const's doc line.
- [ ] **Step 2: Write the failing tests** with the counted bills:

```rust
#[test] fn va_bills_each_mode() {
    // tape_bills_each_model's rooms and `at`: [4_984, 3_817, 4_813, 4_674]
    // for (mode, bill) in [(Mix, COUNTED), (Sync, COUNTED), (Ring, ..), (Xor, ..), (Bits, ..)]:
    //   EngineSlot::cost(&va_sound(mode), &ModState::new()) == Cost(bill)
    //   voices on rev Y (room 3_817) ≥ 6 for the worst mode, and ≥ 6 on every room (ADR 0040's done-when)
}
#[test] fn a_held_va_note_bills_its_playing_mode() // note held in SYNC, MODE stored MIX: held_model_extra == COST_SYNC − COST_MIX until the duck swaps, then 0
```

- [ ] **Step 3: Run them.** `cargo test -p chimera-core --test cost_test va_`. Expected: FAIL (the estimates differ from the counts).
- [ ] **Step 4: Set the five constants** to the counts and `bills_follow_the_mode`'s values to match. If the worst mode leaves fewer than 6 voices on rev Y, stop and report: that is the spec's done-when.
- [ ] **Step 5: Run** `just test`, `just clippy`. Expected: PASS.
- [ ] **Step 6: Commit.** `git commit -m "VA bills by count"`. Report the flash line and the five bills.

### Task 10: INIT at the reference loudness, sanity and goldens

Spec § Voice and engine (INIT), § Budget and billing (Loudness), § Testing (plumbing and sanity). ADR 0011 (sanity gate), ADR 0063/0109 (loudness).

**Files:**
- Modify: `chimera-engine/src/dsp/va/mod.rs` (`VA_OUT_GAIN`, its doc: what it was measured on)
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
/// ADR 0063 at ADR 0109's gain: VA INIT, one C4 at velocity 100 on P1, LEVEL 45/128, within ±1 dB of REFERENCE_LUFS (−11.0).
#[test] fn va_init_matches_the_loudness_reference() {
    let one = level(Patch::VaInit, &[60], 100, BPS);
    let d = one.lufs + one.limited_db - REFERENCE_LUFS;
    assert!(d.abs() <= 1.0, "VA INIT {d:+.2} dB off the reference");
}
#[test] fn va_is_finite_bounded_audible() { assert_finite_bounded_audible(Case::VaInit) }
#[test] fn va_is_silent_after_note_off()  { assert_silent_after_note_off(Case::VaInit) }
#[test] fn va_is_pitched()                 { assert_pitched(Case::VaInit) }
#[test] fn init_va_fixture_pins_engine_code_3() // decode(init_va.snd).engine() == Va and bits_eq(Sound::init(Va))
```

- [ ] **Step 2: Run them.** `cargo test -p chimera-core --test param_sweep_test va_init --test sanity_test va_`. Expected: the loudness test FAILS at `VA_OUT_GAIN` 1.0 and prints `d`.
- [ ] **Step 3: Set** `VA_OUT_GAIN = 10^(−d/20)` from the printed `d`, to three decimals; rerun until `|d| ≤ 0.25` (TAPE's margin).
- [ ] **Step 4: Record** the new rows only: `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test -- --nocapture` (paste `va_init`); `FIXTURE_WRITE=1 cargo test -p chimera-core --test codec_compat_test write_v1_fixtures -- --ignored` (writes `init_va.snd`, never an existing file); add its byte and render rows. No existing golden, fixture or row changes.
- [ ] **Step 5: Verify.** `just test` (the sweep's `levels_are_safe` and `sweep_fast` now include VA INIT), `just clippy`. Expected: PASS.
- [ ] **Step 6: Commit.** `git add` the files above and `init_va.snd`; `git commit -m "VA INIT at the reference loudness, sanity and goldens"`. Report the flash line.

### Task 11: ADR 0118 and the record

Spec § ADRs, § Later.

**Files:**
- Create: `docs/adr/0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md` (from `docs/adr/0000-template.md`)
- Modify: `docs/adr/README.md` (its row after 0114; 0115–0117 are on `knobs-rare`, so 0118 is next), `docs/superpowers/specs/2026-10-09-va-variable-wave-design.md` (Status → `Approved (owner, 2026-10-09)`; "Ships in" unchanged)

- [ ] **Step 1: Write ADR 0118,** Status `Accepted (2026-10-09)`, Deciders owner (spec approval), firmware. Decision, one point each, each checkable in code:
  1. VA is two variable-wave oscillators, an operator (MODE), a sub locked to OSC 1 and STABILITY: an open nod to the Ambika's architecture, from its manual only; no GPL code, no wavetables (ADR 0114).
  2. XOR and BITS alias by design and are exempt from the alias gate and the click rule; MIX, SYNC and RING pass the −45 dB gate at C6.
  3. STABILITY's randomness is xorshift seeded by (Part, voice slot); STAB 1 renders bit-identically, and below 1 a Sound recalls identically on the same Part and slot.
  4. The technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for `chimera-dsp` (no product types, `prepare(rate)`, `PROVENANCE`) and stay in `chimera-engine/src/dsp/va/` until ADR 0114 is accepted; `dsp/provenance.rs` moves with them.
  5. This plan's rulings that constrain later work: the sine's SHAPE crossfade, BLAMP at the sync reset, `Corrector` as the one kernel slot, PIT under the VA node, no cell hints without a gesture.
  Alternatives: the 09-27 draft's 11 one-per-voice models (kept as later macro-oscillator candidates); `chimera-dsp` now (ADR 0114 not accepted). Sources: the spec, this plan, Välimäki 2007, Esqueda/Välimäki/Bilbao EUSIPCO 2015, La Pastina & D'Angelo DAFx-22 (CC-BY 4.0), Hohnerlein et al. DAFx-17 (the 4-point upgrade path), the Ambika manual.
- [ ] **Step 2: Add the README row** in the table's form: `| [0118](0118-….md) | VA is two variable-wave oscillators, an operator, a sub and STABILITY (Ambika's architecture, our code); XOR/BITS alias-exempt; STABILITY seeded by Part and slot; techniques written for chimera-dsp | Accepted |`.
- [ ] **Step 3: Verify.** `just check`. Expected: green end to end (tools, every feature set, clippy, loom, fmt, release-bin, stack-check).
- [ ] **Step 4: Commit.** `git add docs/adr/0118-*.md docs/adr/README.md docs/superpowers/specs/2026-10-09-va-variable-wave-design.md`; `git commit -m "ADR 0118: the VA variable-wave core"`.

**At ship, not in this plan:** on-unit bench rows for each MODE (`chimera-stm32/src/bench.rs`, a `bench-va` area), checked against Task 9's bills on rev V and rev Y; the owner's listening pass; issues #130–#136 and #219 re-scoped to § Later's macro-oscillator list.
