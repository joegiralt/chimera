# FX in the Filter Implementation Plan

**Goal:** Give every voice one FX slot (OFF · SOFT · FUZZ · FOLD), placed PRE or POST the filter. DRV and AMP's fold go away, and FLT's pages are reworked to layout A.

**Architecture:**
- **Functional core:** the per-type kernels in `chimera-engine/src/dsp/voice_fx/`. These are pure per-block loops and static `curve` functions.
  - SOFT and FOLD are today's DRV and wavefolder math, moved bit-exact, with a LEVEL pot added.
  - FUZZ is built to the research step.
  - The legacy translation (`legacy_fx`) is a pure function too.
- **Imperative shell:** `VoiceFx` holds the kernels' state, the ramps and the TYPE/PLACE crossfade. `Voice` calls it at both hooks: PRE before `Filter::process`, and POST where the folder sat.
- **Storage:**
  - A new stored block, `BlockRef::VoiceFx` (code 36, ident `VOICEFX`).
  - The DRIVE (9) and FOLDER (11) blocks are retired.
  - `SoundCheck::finish` translates old Sounds. `READDRESS` moves their routes, registry columns and MIDI-map entries.
- **UI:** it follows the TAPE-machine and reverb-panel precedent.
  - `SlotCtx.fx` picks the type's cells (`fx_panel`) and its matrix headers (`fx_cell`).
  - PLACE gets `FocusGlyph::Place` and a small cell glyph.
  - FLT › FX gets a band viz (`VizType::FxCurve`).
- **FUZZ's kernel lives in bank 2** behind the door (ADR 0142's reverb pattern), so bank 1 stays above the 40 KB line.

**Tech Stack:** Rust `no_std` (`chimera-engine`, `chimera-core`), host tests in `chimera-core/tests` and in-crate `#[cfg(test)]`, `tools/loop_count.py` (ADR 0056 host count), `chimera-utils` (A/B, `voice-stats`), the unit's bench over USB (`just flash-bench`, `just usb bench`).

**Spec:** `docs/superpowers/specs/2026-10-17-fx-in-filter-design.md` (copied onto this branch). Issue #614 (covers #573). Owner decisions: (private notes). Spike code, read-only reference: branch `drive-spike`, `chimera-desktop/src/bin/drive-spike.rs`.

**Decided here (the spec leaves these to the builder):**
- **OFF is a TYPE value** (code 0), first in the list.
- **The type switch crossfade is 20 ms** (`FX_FADE = 960` samples, the stage gate's). Old and new run together, and the crossfade blocks are not billed, as with the filter's fade.
- **PLACE takes the type's default only when TYPE leaves OFF.** Picking another type later keeps the PLACE you set (owner question 3).
- **LPG is not selectable** until DRUM. Its KIND code 7 and ident `LPG` are reserved in a comment and a test. Nothing loads as it.
- **The engine reference is `EngineType::fx_scale`:** the signal is multiplied by it into the slot and divided by it out of the slot. A stage at MIX 0 is therefore the dry signal.
- **The fixed matrix column FOLD becomes FX** (`FixedCol::Fx`), pointing at P1. The OP 5/6 taps on it act only while TYPE is FOLD; on SOFT and FUZZ they are inert, dimmed and not billed (owner question 2).
- **The ADRs:**
  - 0155: the FX slot and layout A.
  - 0156: FUZZ's model and provenance.
  - 0157: FUZZ in bank 2.

## Global Constraints

- **No `unsafe`** without a `// SAFETY:` comment. A new `Voice` field is written in `init_chain`, under its existing SAFETY note.
- **No heap, no blocking and no panic** in the audio path. Kernels and tables are static, and no `Vec` from the spike survives.
- **No libc.**
- **Every parameter is lerped, never snapped**, and filter and FX sweeps are never zippered:
  - P1–P3 and MIX are `Smoothing::STAGE` (20 ms), and they glide (ADR 0112/0117).
  - TYPE and PLACE are `Change::Crossfade`.
- **Type-driven design:** `FxType` and `Place` are enums with frozen disk codes, never bools or indices. The pots are a `[f32; 3]` reached only through `VoiceFxParams`.
- **Functional core, imperative shell:** kernels, curves, `legacy_fx`, the costs and `FxType::runs` are pure. `VoiceFx` and `Voice` hold the state.
- **Disk codes are forever:**
  - Block 36 `VOICEFX`.
  - Params TYPE 0, PLACE 1, P1 2, P2 3, P3 4, MIX 5.
  - `FxType` codes OFF 0, SOFT 1, FUZZ 2, FOLD 3, with 4 reserved for DELAY.
  - `Place` codes PRE 0, POST 1.
  - Blocks 9 and 11 are retired, never reused. `chimera-core/tests/fixtures/disk_codes_v1.txt` may only be appended to.
- **Old Sounds must load safely; they needn't sound the same** (no legacy compat). DRV loads as SOFT PRE, the fold as FOLD POST, and DRV wins if both ran.
- **ADRs:** use `docs/adr/0000-template.md`, numbers from **0155**, each added to `docs/adr/README.md`. Never edit an accepted ADR.
- **Full-word labels:** the cells say INPUT, SUSTAIN, VOLUME and SYMMETRY when they fit the cell (`the_fx_labels_fit`). They abbreviate only when they truly don't fit.
- **Every new hold or MIX gesture gets a focus-band hint.** This plan adds none; TYPE and PLACE are plain turns. A builder who adds one adds its hint in the same task.
- **Flash:**
  - Report the bank 1 and `.bk2` deltas of the ORBIT build in one line per task that moves them.
  - Bank 1 is at about 45 KB free on ORBIT. FUZZ goes to bank 2 (Task 12). Everything else stays in bank 1.
  - Escalate to the owner only if bank 1 drops below about 40 KB free. No byte-shaving rounds.
- **Builds:**
  - Before any `cargo` or `just` command, wait until `pgrep -x "cargo|just"` prints nothing (a Monitor until-loop, not `sleep`).
  - Run every build and test under `nice -n 15`. One heavy build at a time, ≤19 cores (`~/.cargo/config.toml`).
- **Commits:**
  - Terse, with no type prefix.
  - Never stage `chimera.bin` or `docs/chimera-ui-ux-spec.md`; stage paths explicitly.
- **Issues:** bugs and follow-ups go to joegiralt/chimera issues, referenced by URL, never into repo files.
- **Provenance:** no GPL code. Never read BYOD's or Surge's sources. FUZZ is written from circuits and papers.

## Review Focus

These are the inputs a person will meet that the spec implies but the task tests would otherwise miss. Each line names the input and the behaviour a reasonable person expects. Each has its pinning test in the owning task, marked **(RF n)**.

1. **An old Sound with DRV's TONE and MIX routed and the fold's SYM in the registry loads cleanly.** DRIVE's and FOLD's routes land on the one FX column, and TONE's and SYM's registry columns collapse to one P2 column. Nothing is lost except what DRV-wins drops (Task 10).
2. **Turning TYPE through all four while a held chord sounds** never clicks and never leaves a fade stuck. A change arriving mid-fade waits for the fade's end, and the last one wins (Task 5).
3. **Any TYPE at MIX 0 is the dry signal, bit-exact, on every engine.** The reference scale cancels, so no engine gains or loses level from a stage it can't hear (Task 5).
4. **The 2040 at INPUT LEVEL 1 with FUZZ PRE at full SUSTAIN and VOLUME** stays finite and under the sweep's ceiling, and the 2040's stages see a bounded level (Task 11).
5. **A Part's MIDI-map CC that pointed at the old DRIVE** now drives P1. A CC sweep under FUZZ glides without zipper steps (Task 10).

---

### Task 1: Measure today's bills on the unit (the FUZZ ceiling)

**Files:**
- Create: (private notes) (a durable ledger, git-ignored)

**Interfaces:**
- Produces: `FUZZ_CEILING`, a number in cycles a sample: the steady bill of the costliest filter kind a voice can run (`OLD_LP24` 95 + `FilterKind::K2040.cost(Rev2)` 110 = 205 today). Later tasks quote it.

- [ ] **Step 1: Build and flash the ALGO bench from `origin/main`** (no code change): `nice -n 15 just flash-bench algo`.
  Expected: `bench running ~4 min; console appears after`.
- [ ] **Step 2: Read the rows** with `just usb bench` once the console is up.
  Record `1 OP`, `FOLD`, `DRIVE`, `DRIVE LO`, `LP24 HOT` and `SVF HOT`, and the filter rows if the build has them. Record the row minus `1 OP` for each.
- [ ] **Step 3: Record the flash baseline:** `nice -n 15 just firmware-orbit && just check-bin-size target/chimera-orbit.bin && stat -c %s target/chimera-orbit.bk2`.
  Write both numbers to the ledger.
- [ ] **Step 4: Write the ledger.** Include the rows and deltas against `Voice::DRIVE_COST` (57), `FOLD_COST` (45) and the filter bills, then `FUZZ_CEILING = <n>`. If the unit's numbers exceed the constants, file an issue and link it.

### Task 2: FUZZ research (no code)

**Files:**
- Create: (private notes)

**Interfaces:**
- Produces, for Task 11:
  - the chosen topology: stage count, the diode law and its form, and the tone stack;
  - the anti-aliasing scheme with its factor;
  - the per-sample instruction estimate;
  - the verified stage gains;
  - each value's source.

- [ ] **Step 1: Read the strongest 3–6 sources.** Start from the DAFx reading list memory, and write one paragraph each on what it says to do.
  - Yeh, Abel & Smith, "Simulation of the diode limiter in guitar distortion circuits by numerical solution of ODEs", DAFx-07.
  - Parker, Zavalishin & Le Bivic, "Reducing the aliasing of nonlinear waveshaping using continuous-time convolution", DAFx-16, and Bilbao, Esqueda, Parker & Välimäki, "Antiderivative antialiasing for memoryless nonlinearities", IEEE SPL 2017 (ADAA).
  - Holters, "Antiderivative antialiasing for stateful systems", DAFx-19 (stateful ADAA).
  - D'Angelo, Gabrielli & Turchet, "Fast approximation of the Lambert W function for virtual analog modelling", DAFx-19. This is a closed-form diode law: the spike's 2 × 1,793-entry f32 tables (14 KB) are too large for flash.
  - Werner et al., wave-digital diode clipper and multiple-nonlinearity papers (DAFx-15 / AES 2015).
  - The Big Muff Pi schematic analysis (ElectroSmash) for the stage gains, the tone-stack values and the clipping stages.
- [ ] **Step 2: Verify the Muff's stage gains** against the published schematics. The spike's `g1 = 10·(0.3 + 40·s²)` and `g2 = 30` are unverified. Record the corrected values with their sources.
- [ ] **Step 3: Choose** a topology and an anti-aliasing scheme. The expected route is ADAA at 2× through `dsp::halfband` (35-tap), with a closed-form or ≤ 2 KB diode law. Estimate its cycles a sample by ADAA's host method (instructions × 1.46 × 1.1).
  - **If the estimate exceeds `FUZZ_CEILING`,** stop and bring the numbers to the owner (spec § Cost) before Task 11.

### Task 3: FX types and the VOICEFX block

**Files:**
- Create: `chimera-engine/src/dsp/voice_fx/mod.rs` (the module root; the slot comes in Task 5), `chimera-engine/src/dsp/voice_fx/params.rs`
- Modify:
  - `chimera-engine/src/dsp/mod.rs` (`pub mod voice_fx`);
  - `chimera-engine/src/addr.rs` (`BlockRef::VoiceFx` in the enum, `ALL`, `specs`, `in_sound`, `voice_reads`);
  - `chimera-engine/src/params.rs` (`ParamSnapshot.fx`, both dispatches, `Default`);
  - `chimera-engine/src/storage/codes.rs` (`disk_code` 36, `disk_ident` `"VOICEFX"`);
  - `chimera-engine/src/dsp/knobs.rs` (the `plan` arm);
  - `chimera-engine/src/project/parts.rs` (the two `BlockRef` matches);
  - `chimera-core/src/ui/mod_grid.rs` (`block_tag` `"FX"`, `block_name` `"FX"`);
  - `chimera-desktop/src/factory_sounds/source.rs` and `about.rs` (the block matches).
- Modify: `chimera-core/tests/fixtures/disk_codes_v1.txt` (append block 36, its params and its enum codes)
- Create: `docs/adr/0155-one-fx-slot-per-voice-pre-or-post-the-filter.md`
- Modify: `docs/adr/README.md`
- Test: `chimera-core/tests/voice_fx_params_test.rs`

**Interfaces:**
- Produces, in `chimera_engine::dsp::voice_fx::params`:
  - `enum FxType { Off = 0, Soft = 1, Fold = 3 }`:
    - implements `DiskCode` with idents `OFF`, `SOFT`, `FOLD`. Code 2 is FUZZ's (Task 11) and 4 is DELAY's, both in a comment.
    - `FxType::LIST: [FxType; 3]` is the TYPE knob's order (OFF, SOFT, FOLD; FUZZ goes between SOFT and FOLD in Task 11), with `FxType::NAMES` in step.
    - `const fn default_place(self) -> Place`: SOFT → PRE, FOLD → POST, OFF → PRE.
    - `fn from_index(v: f32) -> FxType`.
  - `enum Place { Pre = 0, Post = 1 }`: implements `DiskCode` with idents `PRE` and `POST`.
  - `struct VoiceFxParams { ty: FxType /* private */, pub place: Place, pub pots: [f32; 3], pub mix: f32 }`:
    - default: OFF, PRE, `[0.0, 0.5, 0.5]`, mix 1.0;
    - `fn ty(&self) -> FxType`;
    - `fn set_type(&mut self, t: FxType)`: from OFF, PLACE becomes `t.default_place()`; otherwise PLACE stays;
    - consts `TYPE = ParamId(0)`, `PLACE = ParamId(1)`, `P1 = ParamId(2)`, `P2 = ParamId(3)`, `P3 = ParamId(4)`, `MIX = ParamId(5)`;
    - `impl Block`, with `enum_code`, `enum_ident` and `set_enum_code` for TYPE and PLACE.
  - `static VOICE_FX_SPECS: [ParamSpec; 6]`:

    | Param | Kind | Label | Ident | Range and format | Default | Modulatable | Smoothing |
    |---|---|---|---|---|---|---|---|
    | TYPE | choice | `FX` | `TYPE` | `ValFmt::Names(&FxType::NAMES)` | — | no | `Change::Crossfade`, `FocusGlyph::None` |
    | PLACE | choice | `PLACE` | `PLACE` | `ValFmt::Names(&["PRE", "POST"])` | — | no | `Change::Crossfade` (glyph in Task 8) |
    | P1 | continuous | `AMOUNT` | `P1` | Uni 0–1 | 0 | yes | `Smoothing::STAGE` |
    | P2 | continuous | `SHAPE` | `P2` | Bi 0–1 | 0.5 | yes | `Smoothing::STAGE` |
    | P3 | continuous | `LEVEL` | `P3` | Uni 0–1 | 0.5 | yes | `Smoothing::STAGE` |
    | MIX | continuous | `MIX` | `MIX` | Bi 0–1 | 1.0 | yes | `Smoothing::STAGE`, `FocusGlyph::Crossfader` |

  - `fn pot_level(p3: f32) -> f32 = filter::level_gain(p3)`: ×¼ to ×4, exactly 1 at ½.
- Produces: `BlockRef::VoiceFx` and `ParamSnapshot.fx: VoiceFxParams`.

- [ ] **Step 1: Write the failing tests** in `voice_fx_params_test.rs`:

```rust
#[test] fn voice_fx_block_code_and_ident_are_frozen() {
    assert_eq!(BlockRef::VoiceFx.disk_code(), Some(36));
    assert_eq!(BlockRef::VoiceFx.disk_ident(), Some("VOICEFX"));
}
#[test] fn fx_type_and_place_codes_are_frozen() {
    assert_eq!([FxType::Off, FxType::Soft, FxType::Fold].map(|t| t.disk_code()), [0, 1, 3]);
    assert_eq!([Place::Pre, Place::Post].map(|p| p.disk_code()), [0, 1]);
    assert_eq!(FxType::from_disk_code(2), None); // FUZZ arrives in Task 11
}
#[test] fn picking_a_type_from_off_takes_its_place_later_ones_keep_yours() {
    let mut p = VoiceFxParams::default();
    p.set_type(FxType::Fold); assert_eq!(p.place, Place::Post);
    p.place = Place::Pre; p.set_type(FxType::Soft); assert_eq!(p.place, Place::Pre);
}
#[test] fn defaults_are_off_pre_and_unity_level() {
    let p = ParamSnapshot::default().fx;
    assert_eq!((p.ty(), p.place, p.pots, p.mix), (FxType::Off, Place::Pre, [0.0, 0.5, 0.5], 1.0));
    assert_eq!(pot_level(0.5), 1.0);
}
#[test] fn every_fx_param_round_trips_through_block() { /* set/get for P1–P3, MIX; enum codes for TYPE, PLACE */ }
```

- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_params_test`
  Expected: compile errors (`voice_fx` not found).
- [ ] **Step 3: Implement** the types, block, specs and the `BlockRef` plumbing listed under Files. Every exhaustive `BlockRef` match gains a `VoiceFx` arm, so the compiler lists them. Append to the fixture the lines that `disk_codes_test::table_matches_golden` prints for block 36.
- [ ] **Step 4: Run the tests to make sure they pass.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_params_test --test disk_codes_test --test addr_test --test mappable_test --test modulatable_test`
  Expected: PASS.
- [ ] **Step 5: Write ADR 0155**, "One FX slot per voice, PRE or POST the filter" (Proposed).
  - Context: spec § Why.
  - Decision:
    - DRV leaves the chain and FOLD leaves AMP;
    - the TYPE list, PLACE, and INPUT LEVEL;
    - layout A, per kind, and the 2040's MODE cell dropped;
    - routed cells are matrix views, and the FX fixed column;
    - each type bills its real cost, and the 20 ms switch crossfade;
    - PLACE follows a type only from OFF;
    - `fx_scale`;
    - old Sounds (DRV wins);
    - the LPG code 7 reserved and not selectable.
  - Alternatives: spec § Non-goals; two stages kept.
  - Add the README row.
- [ ] **Step 6: Commit.**

```bash
git add chimera-engine/src/dsp/voice_fx chimera-engine/src/dsp/mod.rs chimera-engine/src/addr.rs chimera-engine/src/params.rs chimera-engine/src/storage/codes.rs chimera-engine/src/dsp/knobs.rs chimera-engine/src/project/parts.rs chimera-core/src/ui/mod_grid.rs chimera-desktop/src/factory_sounds/source.rs chimera-desktop/src/factory_sounds/about.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/tests/voice_fx_params_test.rs docs/adr/0155-one-fx-slot-per-voice-pre-or-post-the-filter.md docs/adr/README.md
git commit -m "The voice FX block: TYPE, PLACE, three pots and MIX"
```

### Task 4: SOFT and FOLD kernels, moved bit-exact, with a LEVEL pot

**Files:**
- Create: `chimera-engine/src/dsp/voice_fx/soft.rs`, `chimera-engine/src/dsp/voice_fx/fold.rs`
- Modify: `chimera-engine/src/dsp/voice_fx/mod.rs` (the shared `Ends` type)
- Test: in-crate `#[cfg(test)]` in `soft.rs` and `fold.rs`

**Interfaces:**
- Consumes: `pot_level` (Task 3), `ease::{at, step_of}`, `filter::{SUB, step_to}`, `TapLane`.
- Produces:
  - `pub struct Ends { pub pots: [(f32, f32); 3], pub mix: (f32, f32), pub gate: (f32, f32) }`: a block's ramp ends.
  - `soft::run(buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool)`. The three loop shapes of today's `Drive::process_glide` (steady, glide steps and moving) are kept. P1 is DRIVE, P2 is TONE, and the wet is multiplied by `pot_level(P3)`.
  - `fold::run(buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool, lane: Option<&TapLane>)`. Today's `Wavefolder` loops are kept, including `tapped` and silence's offset (ADR 0060). P1 is FOLD, P2 is SYM, and P3 is LEVEL on the wet.
  - `soft::curve(x: f32, pots: [f32; 3]) -> f32` and `fold::curve(x: f32, pots: [f32; 3]) -> f32`: the memoryless transfer at MIX 1, for the viz.

- [ ] **Step 1: Write the failing tests:**
  - `soft_at_level_half_is_todays_drive_bit_for_bit`: for DRIVE in {0.01, 0.5, 1}, TONE in {0, 0.5, 1}, MIX in {0.3, 1}, run 40 blocks of a 0.7 sine at 110 Hz through `Drive::process` and `soft::run`, with P3 at 0.5 and the gate open. Assert sample bits are equal. Cover a moving block too: ramp DRIVE 0.2 → 0.8, with and without glide.
  - `fold_at_level_half_is_todays_folder_bit_for_bit`: the same against `Wavefolder::process`, with and without a `TapLane`.
  - `level_scales_only_the_wet`: at MIX 0, P3 at 0 and at 1 give the dry signal bit-exact.
  - Port `the_gate_never_clicks_at_any_tone` from `drive.rs` onto `soft::run`.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-engine --lib voice_fx`
  Expected: FAIL (`soft` not found).
- [ ] **Step 3: Implement** by moving the math, not rewriting it. `Drive` and `Wavefolder` stay until Task 6, so the comparison tests have a reference. Multiplying the wet by P3's gain must leave `x * 1.0` exact at ½.
- [ ] **Step 4: Run the tests to make sure they pass.**
  Run: `nice -n 15 cargo test -p chimera-engine --lib voice_fx`
  Expected: PASS.
- [ ] **Step 5: Commit:** `git commit -m "SOFT and FOLD kernels: today's drive and fold, with LEVEL"` (stage the two files and `mod.rs`).

### Task 5: The slot (`VoiceFx`) and the engine reference level

**Files:**
- Modify: `chimera-engine/src/dsp/voice_fx/mod.rs`
- Modify: `chimera-engine/src/params.rs` (`EngineType::fx_scale`)
- Test: `chimera-core/tests/voice_fx_slot_test.rs`

**Interfaces:**
- Consumes: `Ends`, `soft::run`, `fold::run`, `VoiceFxParams`.
- Produces:
  - `pub const FX_FADE: usize = 960`: 20 ms at 48 kHz.
  - `pub struct VoiceFx`: the ramps for P1–P3 and MIX, the run gate, the last setting `(FxType, Place)`, and the fade in progress. FUZZ's state is added in Task 11. Methods:
    - `fn new() -> Self`;
    - `fn hold(&mut self)`: a fresh note, no ramp from the last note;
    - `fn step(&mut self, p: &VoiceFxParams, on: bool) -> FxBlock`: once a block. It steps the ramps and the fade, and starts a fade when `(ty, place)` changed and none is running. A change arriving mid-fade is read again when the fade ends.
    - `fn apply(&mut self, b: &FxBlock, at: Place, buf: &mut [f32; BLOCK_SIZE], scale: f32, glide: bool, lane: Option<&TapLane>)`. At each hook, every side of the fade whose place is `at` runs on the same input `x`: `y = x + Σ w_side·(kernel_side(x·scale)/scale − x)`, with fade weights lerped per sample. `lane` reaches only a FOLD side.
  - `pub struct FxBlock` (opaque): the block's `Ends` per side and the fade weights.
  - `EngineType::fx_scale(self) -> f32`: a constant per engine, never a running normaliser. VA 1.0; MODAL from its measured pre-filter peak (about 1/0.29); ALGO and TAPE measured in Step 3. Each value is documented in its doc comment with the measurement.

- [ ] **Step 1: Write the failing tests:**
  - `a_type_change_mid_note_is_click_free`: hold a VA saw chord; at block 40 set SOFT → FOLD, then FOLD → OFF and OFF → SOFT. Assert `common::clicks(out)` is empty, and that the switch's second difference is < 1.5× the steady one (the `the_gate_never_clicks` measure).
  - `a_place_change_mid_note_is_click_free`: the same for PRE ↔ POST on SOFT.
  - **(RF 2)** `turning_type_through_all_while_a_chord_sounds_never_sticks`: change TYPE every block for 30 blocks, then hold. After `FX_FADE` plus two blocks, the output equals a fresh voice on the final setting (rms_diff < 1e-6), and no click is found.
  - **(RF 3)** `mix_zero_is_dry_on_every_engine`: for each `EngineType::ALL`, each TYPE and each PLACE at MIX 0, the voice output's bits equal the TYPE OFF output's.
  - `each_engine_meets_the_fx_reference`: INIT on each engine at C4, velocity 100. The pre-filter block peak × `fx_scale` lies in 0.7..=1.4.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_slot_test`
  Expected: compile errors.
- [ ] **Step 3: Implement `VoiceFx` and measure `fx_scale`.** Use a temporary `#[ignore]` print test, then drop it once the constants are in. These slot tests drive `VoiceFx` directly on rendered engine blocks. `Voice` wiring comes in Task 6.
- [ ] **Step 4: Run the tests to make sure they pass.** Same command. Expected: PASS.
- [ ] **Step 5: Commit:** `git commit -m "The FX slot: two hooks, a 20 ms switch, the engine reference"`.

### Task 6: The voice plays the slot, and each type bills its cost

**Files:**
- Modify: `chimera-engine/src/dsp/voice.rs`:
  - fields: `drive` and `folder` become `fx: VoiceFx`;
  - `init_chain`;
  - `trigger`, which calls `fx.hold()`;
  - `render_knobs` steps 2 and 4;
  - `cost`, `glide_extra`, `stage_runs` (becomes `fx_runs`) and `stage_cost` (becomes `fx_cost`).
- Modify: `chimera-engine/src/dsp/voice_fx/params.rs` (the costs on `FxType`).
- Modify: `chimera-engine/src/modulation.rs`: `FixedCol::Fold` becomes `FixedCol::Fx`, `addr()` is `(VoiceFx, P1)`, and `tag()` is `"FX"`.
- Modify: `chimera-engine/src/taps.rs` (doc and `TAP_FOLD_COST` comment) and `chimera-engine/src/dsp/knobs.rs` (`Gliding { filter, fx }`).
- Delete: `chimera-engine/src/dsp/drive.rs`, `chimera-engine/src/dsp/wavefolder.rs`. Before deleting them, record Task 4's bit-exact cases as FNV hashes, and switch those tests to compare against the hashes.
- Modify: `chimera-stm32/src/bench.rs`: the rows `SOFT`, `SOFT LO` and `FOLD` set `p.fx`, and the routing row's FOLD route targets `FixedCol::Fx`.
- Test:
  - Modify `chimera-core/tests/cost_test.rs`, `taps_test.rs` and `common/sweep.rs` (the LFO 3 routes target `(VoiceFx, P1)`).
  - Create `chimera-core/tests/voice_fx_golden_test.rs`.

**Interfaces:**
- Consumes: `VoiceFx::{step, apply}`, `EngineType::fx_scale`.
- Produces:
  - `FxType::runs(self, p1: f32, moved: bool) -> bool`: OFF false; SOFT and FOLD `p1 >= 0.001 || moved`.
  - `FxType::cost(self) -> Cost`: OFF 0, SOFT `Cost(57)` (today's `DRIVE_COST`), FOLD `Cost(45)` (today's `FOLD_COST`).
  - `FxType::ramp_cost(self) -> Cost`: SOFT 23, FOLD 35.
  - `FxType::glide_cost(self) -> Cost`: 1 each.
  - `Voice::fx_runs(p: &ParamSnapshot, mods: &ModState) -> bool`: `moved` is a route on P1 or, under FOLD on ALGO, a moving FX tap. The bill and the gate read this one answer.
  - `Voice::fx_cost`: the type's cost, plus its ramp cost when P1–P3 or MIX is routed or the FOLD tap moves, plus `TAP_FOLD_COST` under FOLD only.
  - Re-measure FOLD's bill on the unit in Task 13; the LEVEL multiply adds about 1.

- [ ] **Step 1: Write the failing tests:**
  - `voice_fx_golden_test`: one golden per (TYPE ∈ {SOFT, FOLD}) × (PRE, POST) on VA INIT C3 and ALGO INIT C4, as `(name, (fnv1a, spots))`. Record them on first green, through `common::golden::check`.
  - `soft_pre_is_todays_drv_and_fold_post_is_todays_fold`: run the old chain on `origin/main` once and paste its `fnv1a` for VA INIT with DRIVE 0.5 and FOLD 0.5. Then assert that SOFT PRE P1 0.5 (FOLD off) and FOLD POST P1 0.5 (SOFT off) match those hashes.
  - In `cost_test`, update and replace the fold and drive rows:
    - `assert_eq!(Voice::cost(&soft, &none), bare + FxType::Soft.cost())`;
    - FOLD routed adds `ramp_cost`;
    - a FOLD tap on ALGO adds `TAP_FOLD_COST + TAP_CAPTURE_COST`;
    - a FOLD tap under SOFT adds nothing (`fx_taps_are_inert_off_fold`);
    - OFF costs `bare`.
  - In `taps_test`, `stage_runs` becomes `fx_runs` with the same three asserts, under FOLD.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test cost_test --test taps_test --test voice_fx_golden_test`
  Expected: FAIL / compile errors.
- [ ] **Step 3: Implement the wiring.**
  - The PRE hook is `self.fx.apply(&b, Place::Pre, output, scale, glide, None)` before `filter.process`.
  - The POST hook is after it, with `lane(FixedCol::Fx)` passed only when `m.fx.ty() == FxType::Fold`.
  - `b = self.fx.step(&m.fx, fx_on)` once a block, before step 2.
  - `scale = m.engine().fx_scale()`.
  - Glide is `!routed(VoiceFx, [P1, P2, P3, MIX])`.
  - The old `drive` and `folder` fields of `ParamSnapshot` stay, inert, until Task 10.
- [ ] **Step 4: Run the tests to make sure they pass.**
  Run: `nice -n 15 cargo test -p chimera-core --test cost_test --test taps_test --test voice_fx_golden_test --test golden_test --test param_sweep_test`
  Expected: PASS, and `golden_test` is unchanged.
- [ ] **Step 5: Report the flash** in one line (`just firmware-orbit`, `check-bin-size`).
- [ ] **Step 6: Commit:** `git commit -m "The voice plays its FX slot PRE or POST; each type bills its own cost"`.

### Task 7: FLT's pages to layout A; DRV and AMP's fold leave the chains

**Files:**
- Modify: `chimera-core/src/ui/filter_panel.rs`:
  - `KindPanel { main: [PanelKnob; 3], mods: [PanelKnob; 6] }`;
  - `knob(kind, k)`: 0..=2 main, 3..=8 mods;
  - `PanelTarget::Fixed` deleted;
  - `kind_note(kind) -> Option<&'static str>`: `Some("4-POLE LP")` for the 2040.
- Create: `chimera-core/src/ui/fx_panel.rs`:
  - `pub struct FxCell { pub id: ParamId, pub label: &'static str, pub short: &'static str, pub fmt: ValFmt }`;
  - `pub fn cells(t: FxType) -> Option<&'static [FxCell; 3]>`: `None` for OFF;
  - `pub fn cell_of(t: FxType, id: ParamId) -> Option<&'static FxCell>`.
- Modify: `chimera-core/src/ui/block_def.rs` (`SlotBinding::FxPanel(u8)`, `ParamSlot::fx_panel(k)`).
- Modify: `chimera-core/src/ui/view.rs`:
  - `SlotCtx.fx: FxType`, read in `SlotCtx::read`;
  - the `FxPanel` arm: OFF reads `View::Empty`;
  - MIX reads dimmed while OFF.
- Modify: `chimera-core/src/ui/block_registry.rs`, as in the Interfaces below.
- Modify: `docs/chimera-sitemap.md` (VA, ALGO and TAPE chains; FLT's three pages; AMP's cells).
- Test: modify `chimera-core/tests/filter_types_test.rs`, `amp_page_test.rs`, `ui_routing_test.rs` and `screen_golden_test.rs` (re-record the FLT, FMD, AMP and chain-map screens); create `chimera-core/tests/fx_page_test.rs`.

**Interfaces:**
- Consumes: `VoiceFxParams`, `FxType`.
- Produces the pages and cells:

  | Page | Cells |
  |---|---|
  | `FILTER` (id 10) | `[KIND, filter_panel(0) MODE or VOICE, param(VoiceFx, TYPE), param(VoiceFx, PLACE), filter_panel(1) FREQ or CUTOFF, filter_panel(2) Q or RES]` |
  | `FILTER_MOD` (id 59) | `filter_panel(3..=8)`: CLEAN-likes FM1 · FM2 · FM3 · FM3 IN · QM · QM IN; the 2040 ENV AMT · KEYBD · FM3 · FM3 IN · QM · QM IN |
  | `FILTER_FX` (id 8, DRV's freed id, so page memory doesn't grow) | name `"FX"`, short `"FX"`, `PageLayout::CellGrid`, `VizType::FxCurve` (Task 8; `None` until then); `[param(Filter, LEVEL).with_label("INPUT"), fx_panel(0), fx_panel(1), fx_panel(2), param(VoiceFx, MIX), EMPTY]` |
  | `AMP` (FOLDER renamed, id 9) | `[SEND1, SEND2, SEND3, EMPTY, VCA_VEL, EMPTY]` |

  - `FILTER_SUB_PAGES = [&FILTER_MOD, &FILTER_FX]`.
  - `DRIVE`, `FOLDER_SYM` (id 98) and `AMP_SUB_PAGES` are deleted.
  - Chains:
    - VA · FLT · AMP · MOD;
    - ALG · OSC · FLT · AMP · MOD (`ALGO_BLOCKS` and `ALGO_FORMANT_BLOCKS` go to 5 entries);
    - SRC · DECK · FLT · AMP · MOD;
    - MODAL unchanged except AMP (`ChainBlock::page(&AMP)`).
  - `FILTER_SPECS` LEVEL: label `"INPUT LEVEL"`, `.short("INPUT")`; ident `LEVEL` unchanged.
  - The `fx_panel` cells:

    | Type | P1 | P2 | P3 |
    |---|---|---|---|
    | SOFT | DRIVE (Uni) | TONE (Bi) | LEVEL (Uni) |
    | FOLD | FOLD (Uni) | SYMMETRY, or SYM if it doesn't fit (Bi) | LEVEL (Uni) |

    Each has a `short` that fits `HEADER_MAX_W`.

- [ ] **Step 1: Write the failing tests** (`fx_page_test.rs` and the modified files):
  - `flt_front_reads_kind_mode_fx_place_freq_q_on_clean` and `..._voice_..._cutoff_res_on_2040`: assert each slot's `view(...).label()`.
  - `flt_mod_holds_the_routes_and_jacks`: labels per kind. ENV AMT and KEYBD are `View::Route` (matrix views).
  - `flt_fx_reads_input_then_the_types_pots_then_mix`: under SOFT the labels are `["INPUT", "DRIVE", "TONE", "LEVEL", "MIX"]`; under FOLD `[.., "FOLD", "SYMMETRY" or "SYM", "LEVEL", ..]`; under OFF the pots are `View::Empty` and MIX is dimmed.
  - `the_fx_labels_fit`: every `FxCell.label` fits the cell width and every `short` fits `HEADER_MAX_W`.
  - `amp_has_no_fold`: no slot of `AMP` addresses `VoiceFx` and none addresses the retired blocks.
  - `chains_have_no_drv`: for each chain, no `ChainBlock` def has short `"DRV"`, and the maps read the chains above.
  - `the_2040s_kind_band_names_its_mode`: `kind_note(K2040) == Some("4-POLE LP")`, drawn in the KIND focus band (screen golden).
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test fx_page_test --test filter_types_test --test amp_page_test`
  Expected: FAIL.
- [ ] **Step 3: Implement.** Re-record the screen goldens that change, and only those. List each in the comment block above the table.
- [ ] **Step 4: Run the tests to make sure they pass.**
  Run: `nice -n 15 cargo test -p chimera-core --features orbit --test fx_page_test --test filter_types_test --test amp_page_test --test ui_routing_test --test screen_golden_test`
  Expected: PASS.
- [ ] **Step 5: Commit:** `git commit -m "FLT to layout A: KIND, MODE, FX, PLACE up front; FLT › FX; DRV and AMP's fold gone"` (stage the files listed; not `docs/chimera-ui-ux-spec.md`).

### Task 8: PLACE's glyph and FLT › FX's strip and curve

**Files:**
- Modify: `chimera-engine/src/glyph.rs` (`FocusGlyph::Place`) and `chimera-engine/src/dsp/voice_fx/params.rs` (PLACE `.glyph(FocusGlyph::Place)`).
- Modify: `chimera-core/src/ui/glyph.rs`: `Gauge::Place { post: bool }`, drawn large. It is two boxes, FX and FLT, joined by an arrow; the lit box (ACCENT) is the effect: left for PRE, right for POST.
- Modify: `chimera-core/src/ui/components.rs`: `Cell.mini: Option<MiniGlyph>` and `enum MiniGlyph { Place(Place) }`, drawn small in place of the value text. The region key includes it.
- Modify: `chimera-core/src/ui/block_def.rs` (`VizType::FxCurve`), `viz.rs` (`fx_strip`, `fx_curve`), `renderer.rs` (the `draw_band_viz` arm) and `region.rs` (its key: `quantize_values` and TYPE/PLACE).
- Test: `chimera-core/tests/fx_page_test.rs` (append), `focus_glyph_test.rs`, `screen_golden_test.rs`.

**Interfaces:**
- Consumes: `soft::curve`, `fold::curve` (Task 4); `fuzz::curve` from Task 11 joins in that task.
- Produces:
  - `viz::fx_strip<D>(d, source: &str, fx: &str, post: bool)`: the nodes `source › FX › FLT` or `source › FLT › FX`, with the FX node lit, in `effects_flow`'s style. `source` is the short of the chain node before FLT: `VA`, `OSC`, `DECK`, or MODAL's node.
  - `viz::fx_curve<D>(d, f: impl Fn(f32) -> f32)`: one sine period drawn faint (FAINT) and `f` of it drawn in INK, across `VIZ_LEFT..=VIZ_RIGHT`.
  - `voice_fx::curve(t: FxType, x: f32, pots: [f32; 3]) -> f32`: dispatches by type; OFF is the identity.
  - The curve draws the set values (ADR 0152), never the modulated ones.

- [ ] **Step 1: Write the failing tests:**
  - `place_glyph_lights_the_effect_box`: render `Gauge::Place { post: false }` and `{ post: true }` into a test framebuffer. The ACCENT pixel centroid is left of centre for PRE and right for POST.
  - `the_place_cell_draws_its_mini_glyph`.
  - `fx_band_redraws_when_type_or_place_changes`: the region key differs.
  - New screen goldens: `flt_fx_soft_pre`, `flt_fx_fold_post`, `flt_front_place_focused`.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --features orbit --test fx_page_test --test focus_glyph_test --test screen_golden_test`
  Expected: FAIL.
- [ ] **Step 3: Implement.** Draw the curve from `renderer.set` values (the set target), as FLT's viz does.
- [ ] **Step 4: Run the tests to make sure they pass.** Same command. Expected: PASS.
- [ ] **Step 5: Report the flash in one line, and commit:** `git commit -m "PLACE's glyph, and FLT › FX's strip and curve"`.

### Task 9: The MOD matrix and the MIDI map name FX cells by their type

**Files:**
- Modify: `chimera-core/src/ui/mod_grid.rs`:
  - `struct DestCtx { pub tape: TapeModel, pub fx: FxType }` replaces the `tape: TapeModel` argument of `dest_name`, `header` and `fmt_route_dest`, and their callers.
  - `fx_cell(a, fx)` sits beside `machine_cell`. Under SOFT a P1 column's header is SOFT's `short`, and the readout is `FX DRIVE`.
- Modify: the MIDI MAP page's name lookup (it calls `dest_name`) and `chimera-core/src/ui/learn.rs` if it formats destinations.
- Test: `chimera-core/tests/matrix_view_test.rs`, `mod_registry_test.rs`.

**Interfaces:**
- Consumes: `fx_panel::cell_of` (Task 7).
- Produces: `pub fn fx_cell(a: ParamAddr, fx: FxType) -> Option<&'static FxCell>`. It is `None` off `BlockRef::VoiceFx`, and `None` for TYPE, PLACE and MIX, which keep their spec labels.

- [ ] **Step 1: Write the failing tests:**
  - `a_routed_fx_pot_is_named_by_the_type`: route LFO 1 to `(VoiceFx, P2)`. Under SOFT the header is TONE's short and the band reads `FX TONE`; under FOLD it reads `FX SYMMETRY` or `FX SYM`.
  - `the_fx_column_is_the_fixed_fourth`: `FixedCol::Fx.tag() == "FX"`, and its address is P1.
  - `fx_pots_are_mappable`: `Mappable::new((VoiceFx, P1..P3 | MIX))` is `Some`; TYPE and PLACE are `None`.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test matrix_view_test --test mod_registry_test --test mappable_test`
  Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests to make sure they pass.** Same command, plus `--features orbit --test screen_golden_test` for the matrix screens. Expected: PASS.
- [ ] **Step 5: Commit:** `git commit -m "The matrix and MIDI map name FX cells by their type"`.

### Task 10: Retire DRV and FOLDER; old Sounds load into the slot; LPG's code reserved

**Files:**
- Create: `chimera-engine/src/dsp/voice_fx/legacy.rs`
- Modify: `chimera-engine/src/storage/codes.rs`:
  - `RETIRED` gains `(9,0)`, `(9,1)`, `(9,2)`, `(11,0)`, `(11,1)`, `(11,2)`, and `RETIRED_BLOCKS = &[9, 11]`;
  - `READDRESS` gains `(9,0)→P1`, `(9,1)→P2`, `(9,2)→MIX`, `(11,0)→P1`, `(11,1)→P2`, `(11,2)→MIX`, all on `BlockRef::VoiceFx`;
  - the `disk_code` and `disk_ident` arms of `Drive` and `Folder` are deleted.
- Modify: `chimera-engine/src/storage/sound.rs`:
  - `SoundCheck` gains `legacy: Legacy`, filled in `step` from a `Block` record whose code is 9 or 11 (entries parsed as `(id, f32 LE)`) and reset in `Header`;
  - `finish` applies `legacy_fx` after the routes and taps are in, before `infer_jacks`.
- Modify:
  - `chimera-engine/src/addr.rs` and `params.rs`: delete `BlockRef::{Drive, Folder}`, `DriveParams`, `FolderParams`, `DRIVE_SPECS`, `FOLDER_SPECS` and the `ParamSnapshot` fields, plus every arm the compiler lists (`knobs.rs`, `parts.rs`, `mod_grid.rs`, `block_registry.rs`, `factory_sounds`, `bench.rs`).
- Modify: `chimera-engine/src/dsp/filter/mod.rs`: a doc line on `FilterKind`, "code 7 is reserved for LPG (FX spec § LPG), never another kind".
- Modify: `tools/factory-sounds/va.toml` and `ports.toml`:
  - `FOLDER.FOLD = "0"` lines are deleted;
  - FOLD BASS and the others become `VOICEFX.TYPE = "FOLD"`, `VOICEFX.PLACE = "POST"`, `VOICEFX.P1 = …`;
  - routes become `VOICEFX.P1 ±n`;
  - taps `>FOLD` become `>FX`.
- Modify: `chimera-desktop/src/factory_sounds/build.rs`: `Edit::Fold` becomes `Edit::Fx`, owning `VOICEFX`, and `"DRIVE"` leaves `Effects`. Update the `source.rs` messages to match.
- Modify: `chimera-core/tests/fixtures/disk_codes_v1.txt`: the block 9 and 11 lines become the retired form that `disk_codes_test::retired` expects.
- Test: create `chimera-core/tests/voice_fx_legacy_test.rs`; modify `disk_codes_test.rs`, `filter_types_test.rs` and `library_load_test.rs`.

**Interfaces:**
- Consumes: `VoiceFxParams::set_type`.
- Produces:
  - `pub struct Legacy { pub drive: [Option<f32>; 3], pub fold: [Option<f32>; 3] }`: the raw values by old id; absent means the file left the neutral value out.
  - `pub fn legacy_fx(l: &Legacy, drive_routed: bool, fold_moved: bool) -> Option<VoiceFxParams>`. `None` means neither stage ran, so the Sound keeps OFF.
    - Absent values are the old defaults: DRV `(0.0, 0.5, 1.0)` and fold `(0.0, 0.5, 0.5)`, kept as consts in `legacy.rs`.
    - DRV ran when `drive >= 0.001 || drive_routed`. It gives SOFT, PRE, `pots [drive, tone, 0.5]`, `mix`.
    - The fold ran when `fold >= 0.001 || fold_moved`. It gives FOLD, POST, `[fold, sym, 0.5]`, `mix`.
    - If both ran, DRV wins.
    - `drive_routed` and `fold_moved` come from the raw `Routes` bytes for `(9,0)` and `(11,0)` with a nonzero amount, and for the fold also from an ALGO FX tap (`taps.moving(FixedCol::Fx)`).

- [ ] **Step 1: Write the failing tests** (`voice_fx_legacy_test.rs`). Each encodes a Sound byte-for-byte the way the old firmware did (`common::codec_util`), with block 9 and/or 11 records, and loads it through `SoundCheck`:
  - `old_drv_loads_as_soft_pre`: DRIVE 0.4, TONE 0.7, MIX 0.8 gives SOFT, PRE, `[0.4, 0.7, 0.5]`, 0.8; `skips() == 0`.
  - `old_fold_loads_as_fold_post`: FOLD 0.3, SYM 0.2 (MIX absent) gives FOLD, POST, `[0.3, 0.2, 0.5]`, 0.5.
  - `drv_wins_when_both_ran`.
  - `a_routed_drive_at_zero_ran`: DRIVE 0 with an LFO 1 route on `(9,0)` gives SOFT, and the route is on `FixedCol::Fx` with its amount.
  - `neither_ran_stays_off`.
  - **(RF 1)** `old_routes_and_registry_collapse_onto_the_slot`: DRIVE 0.5, a TONE route, a fold SYM registry column with a route, and a FOLD route. The result is one P2 registry column holding both P2 routes' sources, the FOLD route on `FixedCol::Fx`, and `skips() == 0`.
  - **(RF 5)** `an_old_midi_map_cc_on_drive_drives_p1`: a MIDI map entry at `(9,0)` loads at `(VoiceFx, P1)`. A CC ramp 0 → 127 over 1 s under SOFT renders with `common::clicks` empty and no block step > 1/64 of P1's span.
  - `an_old_project_part_loads_its_drv`: through `project::codec`, the same as `old_drv_loads_as_soft_pre`.
  - `lpg_code_is_reserved`: `FilterKind::from_disk_code(7) == None`. A Sound whose KIND code is 7 loads with KIND at the engine's default and counts one skip; nothing panics.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_legacy_test`
  Expected: FAIL.
- [ ] **Step 3: Implement.** Then rebuild the factory library and listen-check FOLD BASS, which should be unchanged: FOLD POST at LEVEL ½ is bit-exact to the old fold. Run `nice -n 15 just factory-sounds` and compare its render hash with `origin/main`'s.
- [ ] **Step 4: Run the tests to make sure they pass.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_legacy_test --test disk_codes_test --test library_load_test --test preset_test --test load_protocol_test && nice -n 15 cargo test -p chimera-desktop --lib --tests`
  Expected: PASS.
- [ ] **Step 5: Report the flash in one line, and commit:** `git commit -m "DRV and FOLDER retired: old Sounds load into the FX slot, DRV first"`.

### Task 11: FUZZ, to the research

**Files:**
- Create: `chimera-engine/src/dsp/voice_fx/fuzz.rs`
- Modify:
  - `params.rs`: `FxType::Fuzz = 2`, ident `FUZZ`; `LIST` and `NAMES` read OFF, SOFT, FUZZ, FOLD; `default_place` PRE; `runs` is always true; `cost` and `ramp_cost` are placeholders from Task 2's estimate until Task 13.
  - `voice_fx/mod.rs`: `VoiceFx` holds `fuzz: Fuzz`, and `hold` resets it.
  - `fx_panel.rs`: FUZZ's cells are SUSTAIN (Uni), TONE (Bi), VOLUME (Uni).
  - `voice_fx::curve`.
- Modify: `chimera-core/tests/fixtures/disk_codes_v1.txt` (append the FUZZ enum line).
- Create: `docs/adr/0156-fuzz-is-a-big-muff-modelled-from-its-circuit.md`; modify `docs/adr/README.md`.
- Test: `chimera-core/tests/voice_fx_fuzz_test.rs`; `voice_fx_golden_test.rs` (append the FUZZ PRE and POST rows).

**Interfaces:**
- Consumes: `Ends`, `dsp::halfband` (or what Task 2 chose), `FUZZ_CEILING` (Task 1).
- Produces:
  - `pub struct Fuzz`: fixed-size state, no heap.
    - `fn new() -> Self`;
    - `fn run(&mut self, buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool)`: P1 is SUSTAIN, P2 is the Muff's tone stack (an LPF/HPF blend, with the mid scoop at noon), P3 is VOLUME through `pot_level`.
  - `fuzz::curve(x: f32, pots: [f32; 3]) -> f32`: the memoryless transfer of the clipping stages at the pots' gains, for the viz.
  - `#[cfg(test)] enum Aa { On, Off }`, a test-only switch for the anti-aliasing.

- [ ] **Step 1: Write the failing tests:**
  - `fuzz_aliasing_floor_at_high_notes`:
    - A sine at 1760 Hz and at 2093 Hz, SUSTAIN 0.5, TONE 0.5, VOLUME 0.5, 2¹⁵ samples after a 2¹² settle.
    - `common::spectrum::alias_db(x, 48000.0, &[f0]) <= -50.0`.
    - It must also be at least 12 dB below the same render with `Aa::Off`.
    - The floor may move with the owner's ear at UAT, never above −45 dB (VA's gate).
  - `fuzz_tone_scoops_the_mids_at_noon`: with pink noise in at TONE 0.5, the 1 kHz band is at least 6 dB below the mean of the 200 Hz and 4 kHz bands, measured with `goertzel` over noise.
  - `fuzz_sustain_ramps_click_free`: SUSTAIN 0 → 1 over one block, glide on and off, gives `clicks` empty.
  - **(RF 4)** `fuzz_into_a_hot_2040_stays_bounded`: VA INIT on the 2040, INPUT LEVEL 1, FUZZ PRE at SUSTAIN, TONE, VOLUME 1, a held chord for 2 s. `common::assert_stable(out, 4.0, 0.0, ..)`, and the FX output's block peak is ≤ 2 × `fx_scale⁻¹`.
  - `fuzz_mix_zero_is_dry`: covered by `mix_zero_is_dry_on_every_engine` once FUZZ is in `FxType::LIST`.
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_fuzz_test`
  Expected: compile errors.
- [ ] **Step 3: Implement `Fuzz`** to the research ledger's topology, values and anti-aliasing. Every constant cites its source in a comment. No table larger than 2 KB. The spike is the A/B baseline, not the code: write it fresh.
- [ ] **Step 4: Run the tests to make sure they pass.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_fuzz_test --test voice_fx_slot_test --test voice_fx_golden_test --test disk_codes_test`
  Expected: PASS.
- [ ] **Step 5: Write ADR 0156** (Proposed). Record the topology, the diode law, the anti-aliasing and the stage gains, each with its paper or schematic. State that no GPL code was read (BYOD and Surge never). Note the spike's role as the A/B baseline. Add the README row.
- [ ] **Step 6: Commit:** `git commit -m "FUZZ: a Big Muff from its circuit, anti-aliased"`.

### Task 12: FUZZ behind the bank 2 door

**Files:**
- Modify: `chimera-engine/src/dsp/voice_fx/fuzz.rs` (`FuzzDoor`, `Fuzz::play`), `chimera-engine/src/bank2.rs` (`Bank2Door.fuzz`, `DOOR`) and `chimera-stm32/build.rs` (`bank2.x`: `voice_fx::fuzz`'s text and rodata go into `.bank2`, as `rev::*` does).
- Modify: `chimera-core/src/ui/view.rs` and `chimera-core/src/bank2.rs`: while `closed_text` says so, the FX TYPE cell reads `UPDATE` and its focus band `NEEDS UPDATE` under FUZZ, as REVERB TYPE does.
- Create: `docs/adr/0157-fuzz-lives-in-bank-2.md`; modify `docs/adr/README.md`.
- Test: create `chimera-engine/tests/fuzz_proxy_test.rs`, following `reverb_proxy_test.rs`.

**Interfaces:**
- Produces:
  - `pub struct FuzzDoor { pub run: fn(&mut Fuzz, &mut [f32; BLOCK_SIZE], &Ends, bool) }`.
  - `Fuzz::play(&mut self, buf, e, glide)` is bank 1's side. Inside `DOOR_CELL.with` it calls the door's `run`. Closed, it leaves `buf` dry, so the FUZZ side of the slot passes dry and its state waits as it was. No heap, no blocking, no panic.

- [ ] **Step 1: Write the failing tests:**
  - `fuzz_through_the_door_equals_fuzz_direct` (host, door opened as `reverb_proxy_test` does).
  - `fuzz_with_the_door_closed_is_dry`.
  - `fuzz_type_reads_needs_update_while_closed` (a view test with a `Bank2Status` other than `Open`).
- [ ] **Step 2: Run them to make sure they fail.**
  Run: `nice -n 15 cargo test -p chimera-engine --features bank2 --test fuzz_proxy_test`
  Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests to make sure they pass,** then check the build:
  - `nice -n 15 cargo test -p chimera-engine --features bank2 --test fuzz_proxy_test --test reverb_proxy_test`
  - `nice -n 15 just firmware-orbit`, then `just check-bin-size target/chimera-orbit.bin` and the `door-check` that `just check` runs.
  - Expected: PASS, and no bank 1 relocation into bank 2.
- [ ] **Step 5: Write ADR 0157** (Proposed): ADR 0142's pattern for FUZZ, the closed behaviour, and the flash numbers (bank 1 and `.bk2` before and after). Add the README row.
- [ ] **Step 6: Report the flash in one line, and commit:** `git commit -m "FUZZ plays from bank 2; closed, it passes dry and says NEEDS UPDATE"`.

### Task 13: FUZZ's bill, measured, under the ceiling

**Files:**
- Modify: `chimera-engine/src/dsp/voice_fx/params.rs` (`FxType::Fuzz.cost()` and `ramp_cost()`, with their doc comments citing the count and the bench rows; FOLD's re-measured).
- Modify: `chimera-stm32/src/bench.rs` (rows `FUZZ`, `FUZZ MOVE` with SUSTAIN routed from LFO 1, `FUZZ 8V` with eight voices, and `FOLD`, all on `p.fx`).
- Modify: `chimera-core/tests/cost_test.rs`.
- Modify: (private notes).

**Interfaces:**
- Produces: `FxType::Fuzz.cost() <= FUZZ_CEILING − OLD_LP24`, so that FUZZ plus the bare chain is no costlier than a filter.

- [ ] **Step 1: Write the failing tests:**
  - `fuzz_is_billed_its_measured_cost`: `Voice::cost(&fuzz, &none) == bare + FxType::Fuzz.cost()`.
  - `fuzz_routed_adds_its_ramp`.
  - `fuzz_fits_the_ceiling`: `FxType::Fuzz.cost().0 + 95 <= FUZZ_CEILING`, with the constant copied from the ledger.
  - `a_fuzz_part_sheds_only_near_the_limit`: eight FUZZ voices of VA INIT on the 2040 admit at ≤ 75 % of the budget alone (`AUDIO_BUDGET_PERCENT`).
- [ ] **Step 2: Count on the release ELF:** `python3 tools/loop_count.py` on `Fuzz::run`'s steady and moving loops, by ADR 0056's method. Put the numbers in the constants and run `nice -n 15 cargo test -p chimera-core --test cost_test`.
  Expected: PASS.
- [ ] **Step 3: Confirm on the unit:**
  - `nice -n 15 just flash-bench algo bank2` (the `.bk2` staged first, per the recipe), then `just usb bench`.
  - Record `FUZZ`, `FUZZ MOVE`, `FUZZ 8V` and `FOLD` against `1 OP` in the ledger.
  - **If the unit's FUZZ row exceeds the constant,** raise the constant.
  - **If FUZZ exceeds the ceiling,** stop and bring the numbers to the owner (spec § Cost).
- [ ] **Step 4: Re-flash the shipping build:** `just flash`.
- [ ] **Step 5: Commit:** `git commit -m "FUZZ billed as measured on the unit"`.

### Task 14: Desktop QA — sweeps, levels against INPUT LEVEL, A/B against the spike

**Files:**
- Modify: `chimera-utils/src/devices.rs`: `Device::Fx(FxType)` as `fx-soft`, `fx-fuzz` and `fx-fold`, with knobs `p1`, `p2`, `p3`, `mix`, `place` (PRE/POST) and `input` (INPUT LEVEL). It renders through a VA voice's filter chain: an effect, so the input is the stimulus.
- Modify: `chimera-utils/src/main.rs` (`USAGE`).
- Test: `chimera-core/tests/voice_fx_qa_test.rs`; `param_sweep_test` (the thorough sweep).
- Modify: (private notes).

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the failing tests** (`voice_fx_qa_test.rs`):
  - `each_type_at_defaults_matches_off_within_1_5_db`: for each TYPE with P1 at its "normal" point, which the ledger records (SOFT 0.5, FUZZ 0.5, FOLD 0.4), and P3 at ½, render VA INIT C3 for 2 s. Its RMS is within ±1.5 dB of OFF at the same INPUT LEVEL ½. If one misses, tune that type's internal make-up gain, not P3's default.
  - `input_level_sweeps_smoothly_under_every_type`: INPUT 0 → 1 over 2 s under each TYPE × PLACE gives `clicks` empty and no 64-sample block RMS step > 0.5 dB.
  - `every_fx_pot_sweeps_smoothly`: P1, P2, P3 and MIX each 0 → 1 → 0 over 2 s, under each TYPE × PLACE, on VA and MODAL. `clicks` empty (the filter-never-steppy rule).
- [ ] **Step 2: Run them; they fail until the make-ups are tuned.**
  Run: `nice -n 15 cargo test -p chimera-core --test voice_fx_qa_test`
- [ ] **Step 3: Tune the make-up constants** in `soft.rs`, `fuzz.rs` and `fold.rs` (`MAKE_UP`, with doc comments), and run the tests until they pass.
  - If SOFT or FOLD needs a make-up other than 1, it is an intended change (no legacy compat): re-record the Task 4 and Task 6 hashes in this commit, and name the cause in their comment.
- [ ] **Step 4: Run the thorough sweep:**
  `PARAM_SWEEP_REPORT=<dir> nice -n 15 cargo test -p chimera-core --test param_sweep_test -- --ignored sweep_thorough`
  Expected: 0 findings.
- [ ] **Step 5: A/B the shipped models against the spike** on the listening stimuli:
  - `just ab <ref> fx-soft p1=0.5` and `just ab <ref> fx-fuzz p1=0.7 p2=0.5`, at PRE and POST.
  - The reference is a contract wrapper around the spike's SOFT and FUZZ models, built from the `drive-spike` worktree into `$CHIMERA_REFS`, outside this repo (ADR 0098).
  - The stimuli are `bass`, `stab`, `sine_short` and `sine_long` (from `just` → `chimera-utils stim`).
  - Loudness-match with `just voice-stats` (BS.1770). Re-render the spike's acid and willow clips with the shipped models, using the spike binary's `render_all` pointed at the new kernels.
  - Write each pair's LUFS, THD, centroid and alias figures to `qa.md`, and put the WAVs under (private notes).
- [ ] **Step 6: Report the flash for the branch in one line:** ORBIT bank 1 and `.bk2`, against Task 1's baseline.
- [ ] **Step 7: Commit:** `git add chimera-utils/src/devices.rs chimera-utils/src/main.rs chimera-core/tests/voice_fx_qa_test.rs chimera-engine/src/dsp/voice_fx && git commit -m "FX QA: level-matched types, smooth sweeps, fx devices for A/B"`.

### Task 15: Owner UAT on the unit

**Files:** none (a ledger note only).

- [ ] **Step 1: Flash the branch:** `nice -n 15 just flash` (the `.bin` and `.bk2`), then `just status` must show `bank2 open`.
- [ ] **Step 2: Hand the owner the A/B set** from Task 14 Step 5 (the spike against the shipped models, loudness-matched), and the list to play on the unit:
  - VA bass into FUZZ PRE, then POST;
  - SOFT on ALGO and on MODAL STRING (the same bite as VA);
  - FOLD BASS (unchanged);
  - TYPE and PLACE turned while holding a chord;
  - the 2040 at INPUT LEVEL 1 with FUZZ;
  - FLT's three pages, and the AMP page without FOLD.
- [ ] **Step 3: Record the owner's verdict** in the ledger. File every finding as a joegiralt/chimera issue linked from #614. Merge only after the review passes and checks are green (merge-after-review).
