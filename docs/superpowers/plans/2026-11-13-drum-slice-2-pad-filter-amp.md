# DRUM slice 2: per-pad FILTER, AMP and modulation — Implementation Plan

**Goal:** Each pad of a DRUM Part gets its own FILTER (the FLT front's filter cells), its own AMP (an AHD envelope plus the Part AMP's VEL, LPG, DECAY and STRIKE), one mod envelope, one LFO and four routes. This is the compact Rytm-style pad. The FLT, AMP and pad-MOD pages follow the selected pad as MCH does. Slice-1 projects and every non-DRUM Sound sound exactly as before.

**Architecture:** Task 1 adds a typed `PadSound` to `Pad`. It lives in ITCM with the kit (`project::Kits`) and in the audio's `KIT_VIEW` seqlock. A hit copies it into the voice inside its `DrumHit`, as slice 1 copies the machine. On a DRUM voice, `Voice::render_mono` lays the pad's filter and AMP values over its `played` snapshot right after the Part's knobs (`DrumEngine::overlay`). From there the Sound chain runs unchanged: the Part's routes, the filter, the FX slot, the VCA. The pad's AMP envelope, mod envelope, LFO and routes run in `DrumEngine` and reuse `Envelope`, `Lfo` and `apply_offset`. A pad copied from its Part's Sound overlays exactly what the Sound holds, so a slice-1 kit nulls bit for bit. **Latched or live:** choices (filter TYPE and MODE, LFO WAVE and MODE, a route's SOURCE and DEST, env SPEED) latch at the hit. Continuous values (CUTOFF, RES, env times, VEL, LPG, LFO SPEED and DEPTH, route AMOUNT) follow live edits of the pad on its sounding hits, glided (Task 8). That follows the Rytm, whose FILTER and AMP knobs move a hit that is still ringing. Task 8 can be cut: without it, everything is latched at the hit.

**Tech Stack:** Rust no_std (`chimera-engine`, `chimera-core`), desktop simulator, host goldens and nulls (`chimera-core/tests`, `chimera-utils`).

**Spec:** `docs/superpowers/specs/2026-11-10-drum-engine-design.md`: "The voice", "Chain and pages", "Owner rulings 2026-11-10" item 4 (compact pads), "Owner, 2026-11-13: slice order and kit playing". Prior art: ADR 0265 (kit on the Part, `KIT_VIEW`, `Kit` record 0x0024), ADR 0266 (ITCM), ADR 0267 (size-opt), slice-1 plan `docs/superpowers/plans/2026-11-12-drum-slice-1.md` and its ledger `.superpowers/sdd/2026-11-12-drum-slice-1/progress.md` on `drum-slice-1`.

## Global Constraints

- **Type-driven, mandatory (owner, 2026-11-12).** No bare `u8`/`i8`/`f32` for units, ranges, indices, codes or modes where a newtype or enum fits. Constructors clamp or return `Option`, never panic. Fields are private. Cells bind typed slots, never bare indices. Reuse the engine's types (`FilterKind`, `FilterMode`, `K2040Voice`, `EnvSpeed`, `LfoShape`, `lpg::Knobs`, `Unit`, `Hz`) before adding one.
- **Sound.** Every existing audio golden (`golden_test`, `fx_golden_test`, `voice_fx_golden_test`) stays bit-identical. The drum nulls stay 72/72 at −inf (`cargo test -p chimera-utils drums::`). Task 0's `drum_slice1_kit` golden stays bit-identical in every task.
- **Never snap; filter never steppy.** A value that moves a sounding voice glides. A route moves the filter at audio rate, as a Part route does.
- **RAM.** Pad data lives in ITCM only (`Kits`, `KIT_VIEW`; ~51 KB free there). D2 is full and AXI is 100 B under its margin: neither may grow. `Voice` doesn't grow: `size_of::<DrumEngine>() <= size_of::<ModalEngine>()` holds. `memory_budget_test` and the ITCM link asserts carry the new sizes.
- **Stack.** ORBIT's `just stack-check` had 416 B of headroom at slice 1's end (62,816 of 63,232). `DrumHit` grows ~13 → ~70 B, so it is copied once: `KitView::hit` reads it in `drum_on`, and `Voice::strike`, `trigger` and `DrumEngine::strike` take `&DrumHit`. Per-hit state is built in place, as slice 1's out-param rework did. Run `just stack-check` in Tasks 2, 3, 8 and 9.
- **Flash.** DRUM goes in bank 1 (ADR 0251, 0265). Bank 2 must stay ≥ 8 KB free (8,672 B at slice 1's end): no new code in the bank-2 globs. Keep `dsp::drum` out of them in `chimera-stm32/build.rs`. New non-audio code (codec, cells, records) takes `#[optimize(size)]` under ADR 0267. Audio paths stay at opt-level 2. Raise `tools/bank1-budget.txt` lines as `; ADR 0251 +N DRUM slice 2 (from X)`. Give the bank-1 delta in one line per task.
- **`just check` in every task that adds an enum arm or a static** (slice 1's lesson: a missed check let bank 2 overflow).
- **Builds:** `$B` below means `CARGO_BUILD_JOBS=10 nice -n 15`. Run the targeted tests in each task and the full suite before the PR. No `git stash`.
- **Labels and gestures:** full words where they fit (CUTOFF, ATTACK, DECAY, AMOUNT). MIX + MINUS / PLUS (pad step) shows its focus-band hint on every pad page.
- **ADR:** ADR 0269, or the next free number if TONAL's work took it, supersedes 0265 in part. It replaces 0265's "the Sound's FILTER, AMP, LFOs and MOD are shared by every pad" and, if Task 8 ships, "the voice never reads the kit again". 0265 is not edited.
- **Scope fence.** TONAL, PAD FOLLOW and lit pads are being built on `drum-slice-1` and are not in this plan. Also out: SMP/MIX/combine, FM and BODY machines, a per-pad FX slot, pad OUTPUT, RESERVE, pads modulating pads, per-pad MIDI CC learn (file an issue).
- Commits are terse, with no type prefixes. Bugs and follow-ups go to GitHub issues.

## Review Focus

1. **A pad's CUTOFF turned while its open hat or cymbal rings.** The player expects the ringing hit to sweep smoothly, never in steps, and only that pad's voices to move. A TYPE change waits for the next hit (Rytm precedent). Task 8 `cutoff_turn_glides_on_the_ringing_hit` and `type_change_waits_for_the_next_hit` pin it. If the owner rules pure latch, Task 2's `latched_at_the_hit` pins it instead.
2. **A slice-1 project whose DRUM Part had an edited Sound FILTER or AMP** (LADDER at 420 Hz, LPG on): it loads as Saved, every pad carries that filter and amp, and it renders bit-identical to slice 1. Task 0 golden plus Task 5 `slice1_file_takes_the_sounds_filter`.
3. **A kit whose pads use a costlier filter than the Part's Sound** (a 2040 pad on an SVF Sound, or a pad route moving CUTOFF). The voice bill covers it, so the unit doesn't overrun when eight such hits stack. Task 4 `a_2040_pad_bills_above_svf`.
4. **Switching a Part DRUM → VA → DRUM.** VA plays its own Sound's FILTER and AMP untouched. Back on DRUM, every pad keeps its filter and amp. A Sound load never touches the kit. Task 5 `engine_switch_keeps_pad_sounds`.
5. **All four routes on CUTOFF or RES at ±127, with the pad LFO at full SPEED and DEPTH, on every machine.** Output stays finite with peak ≤ +6 dBFS, and CUTOFF stays inside its spec range: no NaN, no blast. Task 3 `routes_at_extremes_stay_bounded`.

---

### Task 0: Base, scope fence, slice-1 baseline

**Files:**
- Create: `chimera-core/tests/fixtures/drum_slice1.prj` (written by the slice-1 code)
- Modify: `chimera-core/tests/golden_test.rs` (`GOLDENS` + one case), `chimera-core/tests/drum_kit_codec_test.rs` (an `#[ignore]` fixture writer)

**Interfaces:**
- Consumes: slice 1 as merged (`Kit`, `Kits`, `KIT_VIEW`, the project codec's save, the helper `drum_kit_codec_test` already uses).
- Produces: fixture `drum_slice1.prj` and golden `drum_slice1_kit`, which every later task must keep bit-identical.

- [ ] **Step 1:** Create worktree (private notes), branch `drum-slice-2`, from `drum-slice-1`'s head. Do this only after TONAL, PAD FOLLOW and lit pads have landed there (ask the controller), or from `0.4` once slice 1 is merged. Don't touch their code (TONAL routing in `drum_on`, the KIT page's TONAL cell, PERSONALIZE) except through merges.
- [ ] **Step 2:** Add the `#[ignore] fn write_slice1_fixture()` writer. It builds Part 1 on DRUM INIT with Sound FILTER LADDER LP, cutoff 420.0, resonance 0.62, AMP VEL 0.4, gate `[0.5, 0.3, 0.2]`, `Kit::DEFAULT` with pad 9 PAN +20, and Part 2 on VA INIT, and saves to the fixture. Run `$B cargo test -p chimera-core --test drum_kit_codec_test write_slice1_fixture -- --ignored` on the unchanged base.
- [ ] **Step 3:** Add golden `drum_slice1_kit`: load the fixture, strike pads 1, 2, 8, 9 and 10 on Part 1 at velocities 100, 40, 127, 90 and 64, one block apart, and render 2 s through the `Instrument`. Record it with `GOLDEN_RECORD=1 $B cargo test -p chimera-core --test golden_test goldens_match`, then run it without the variable → PASS.
- [ ] **Step 4:** Commit `DRUM slice 2: slice-1 baseline golden`.

### Task 1: ADR 0269 and the pad sound types

**Files:**
- Create: `docs/adr/0269-per-pad-filter-amp-and-modulation.md` (from `0000-template.md`, Proposed), added to `docs/adr/README.md`; `chimera-engine/src/dsp/drum/sound.rs`
- Modify: `chimera-engine/src/dsp/drum/mod.rs`, `chimera-engine/src/dsp/drum/types.rs` (`Pad` gains `sound`)
- Test: `chimera-core/tests/drum_sound_test.rs`

**Interfaces:**
- Consumes: `FilterParams` (`kind()`, `mode()`, `set_kind_raw`, `set_mode`, `voice`, `cutoff`, `resonance`, `morph`), `OutParams` (`vca_vel`, `gate`), `EnvParams`, `LfoParams`, `EnvSpeed`, `LfoShape`, `lpg::Knobs`, `Unit`, drum `Hz`, `U7`, `ParamSnapshot`.
- Produces (in `drum::sound`, all `Copy`, fields private):
  - `EnvPos(U7)` with `fn unit(self) -> Unit`. `EnvDecay { Time(EnvPos), Infinite }` with `From<U7>` (127 → `Infinite`) and `fn code(self) -> U7`.
  - `PadEnv { attack: EnvPos, hold: EnvPos, decay: EnvDecay, speed: EnvSpeed }`, with `PadEnv::OPEN` (attack 0, hold 0, `Infinite`), `fn is_open(&self) -> bool`, and `fn env_params(&self) -> EnvParams`, an AHD one-shot: sustain 0, release = decay, `EnvType::A`, `HoldPos::Ahdsr`.
  - `PadFilter { kind, mode, voice, cutoff: Hz, resonance: Unit, morph: Unit }` with `PadFilter::IDS: [ParamId; 6]`: FilterParams' own ids KIND, MODE, VOICE, CUTOFF, RESONANCE, MORPH. Also `fn of(&FilterParams) -> PadFilter`, `fn apply(&self, &mut FilterParams)`, and `fn set_kind(&mut self, FilterKind)`, which keeps MODE valid by `FilterParams::set_kind`'s rule.
  - `PadAmp { env: PadEnv, vel: Unit, gate: lpg::Knobs }`.
  - `LfoMode { Free, Trig }` (`LfoParams::sync` 0/1). `PadLfo { wave: LfoShape, speed: U7, depth: U7, phase: U7, mode: LfoMode }` with `fn lfo_params(&self) -> LfoParams`. SPEED goes through LFO RATE's `ParamSpec::denormalize`.
  - `RouteSource { ModEnv, Lfo, Vel, Lfo1, Lfo2, Lfo3 }` (the last three are the Part's shared LFOs). `RouteDest { Cutoff, Reso, Volume, LfoSpeed, LfoDepth }`. `Amount` is −127..=127 (`new -> Option`, `clamped`). `RouteId` is 0..=3 (`ALL`, `number`). `PadRoute { source, dest, amount }` has `PadRoute::OFF` (ModEnv → Cutoff, 0). `PadRoutes([PadRoute; 4])` has `get` / `set` by `RouteId` and `fn moves_filter(&self) -> bool`. Each source and dest has a disk code fixed by hand (1-based; 0 is never used).
  - `PadSound { filter, amp, env, lfo, routes }`, with `PAD_SOUND_BYTES = 56` and `PadSoundBytes = [u8; 56]`. The layout: filter 15 (kind, mode, voice, then cutoff, resonance and morph as f32 LE), amp 20 (A, H, D, SPEED, then VEL and the three gate values as f32 LE), env 4, LFO 5, routes 12 (source, dest, amount ×4). It has `fn to_bytes(&self) -> PadSoundBytes` and `fn from_bytes(b: PadSoundBytes, d: &PadSound) -> PadSound`. `from_bytes` is total: an unknown code or a non-finite float takes `d`'s field, and a value past its range is held to it.
  - `PadSound::DEFAULT` (const; equals DRUM INIT's Sound with an open AMP, a default LFO and no routes), `fn from_sound(p: &ParamSnapshot) -> PadSound` (the filter and AMP from `p`, the rest as `DEFAULT`), and `fn overlay(&self, m: &mut ParamSnapshot)` (writes `filter.apply`, `out.vca_vel` and `out.gate`).
  - `Pad::sound(&self) -> &PadSound`, `Pad::sound_mut`, `Pad::with_sound(self, PadSound) -> Pad`. `Pad::new` sets `PadSound::DEFAULT`. `Pad::to_bytes` / `from_bytes` stay 13 B (the `Kit` record is frozen), and `from_bytes` keeps `d`'s sound.

- [ ] **Step 1:** Write the ADR. Pads carry a `PadSound`, overlaid on the voice's `played` snapshot. The Part's MTX, E1–E3 and L1–L3, its FX slot, SENDS and the filter's INPUT, FB, STABILITY and jacks stay shared. The AMP is AHD one-shot (note-off is heard only by the Part's VCA routes, as in slice 1). There are four routes with the source and dest lists above. Latched vs live: the proposal above, with the Rytm as precedent. The saving rule is Task 5's. Give the ITCM figures (`Kits` ~1.0 → ~5.3 KB, `KIT_VIEW` 1.9 → ~10.0 KB). Bank 1 is estimated at +9–14 KB.
- [ ] **Step 2: Write the failing tests:**

```rust
#[test] fn pad_sound_bytes_round_trip() { /* DEFAULT and an edited sound: LADDER 420.0 / 0.62,
    amp A 10 H 0 D 64 FAST, routes [ModEnv→Cutoff +90, Lfo→Reso −40, OFF, OFF] */
    assert_eq!(PadSound::from_bytes(s.to_bytes(), &PadSound::DEFAULT), s);
    assert_eq!(PAD_SOUND_BYTES, 56);
}
#[test] fn any_bytes_are_a_sound() { // [0; 56], [0xFF; 56], NaN floats
    let s = PadSound::from_bytes(b, &PadSound::DEFAULT);
    assert!(s.filter().kind().modes().contains(&s.filter().mode()));
    assert!(s.filter().cutoff().get().is_finite());
}
#[test] fn overlay_of_from_sound_is_identity() { // the null's foundation
    let mut q = p.clone(); PadSound::from_sound(&p).overlay(&mut q);
    assert_eq!(q.filter.cutoff.to_bits(), p.filter.cutoff.to_bits()); // and resonance, morph, kind, mode, voice, vca_vel, gate[0..3]
}
#[test] fn default_is_drum_init() {
    assert_eq!(PadSound::DEFAULT, PadSound::from_sound(&drum_init().params));
    assert!(PadSound::DEFAULT.amp().env().is_open());
    assert!(Kit::DEFAULT.pads().all(|p| *p.sound() == PadSound::DEFAULT));
}
#[test] fn decay_127_is_infinite() { assert_eq!(EnvDecay::from(U7::MAX), EnvDecay::Infinite); }
#[test] fn the_pad_carries_the_flt_front() { // every kind's front cells 0..=2
    for k in FilterKind::ALL { for c in 0..=2 { if let Some(PanelTarget::Filter(id)) = filter_panel::knob(k, c).map(|p| p.target) {
        assert!(PadFilter::IDS.contains(&id)); } } }
}
#[test] fn amounts_clamp() { assert_eq!(Amount::new(-128), None); assert_eq!(Amount::clamped(127).get(), 127); }
```
- [ ] **Step 3:** Run `$B cargo test -p chimera-core --test drum_sound_test` → FAIL (unresolved `drum::sound`).
- [ ] **Step 4:** Implement `sound.rs` and `Pad`'s field. Add `const _: () = assert!(size_of::<PadSound>() <= 60);`.
- [ ] **Step 5:** Run → PASS. Then run `$B cargo test -p chimera-core --test drum_types_test --test drum_kit_codec_test` → PASS. Commit `DRUM: ADR 0269 and the pad sound types`.

### Task 2: The pad's filter and AMP in the voice (latched at the hit)

**Files:**
- Modify: `chimera-engine/src/dsp/drum/view.rs` (slot width), `chimera-engine/src/dsp/drum/engine.rs`, `chimera-engine/src/dsp/voice.rs` (`strike`, `trigger`, `render_mono`), `chimera-engine/src/instrument.rs` (`drum_on` passes `&DrumHit`), `chimera-core/tests/memory_budget_test.rs`
- Test: `chimera-core/tests/drum_voice_test.rs`

**Interfaces:**
- Consumes: Task 1 `PadSound`, `overlay`, `PadEnv::env_params`.
- Produces:
  - `VIEW_BYTES = PAD_BYTES + PAD_SOUND_BYTES` (69) per view slot. `KitView::hit` decodes both. Its size assert becomes the new figure (≈10,008 B).
  - `Voice::strike(&mut self, key, (hit, seed): (&DrumHit, HitSeed), velocity, params, ctx) -> bool`, `DrumEngine::strike(&mut self, hit: &DrumHit, vel, seed, sr)`.
  - `DrumEngine::overlay(&self, m: &mut ParamSnapshot)` (the hit's `PadSound::overlay`). Call it in `render_mono` right after `knobs.apply(m, live)`, only on an active Drum slot.
  - `DrumEngine::amp_block(&mut self, out: &mut [f32; BLOCK_SIZE])`: the AMP envelope (an `Envelope` on the latched `EnvParams`, built in place at the strike) as a per-sample gain after the DC blocker and before the VCA. It does nothing while `amp.env().is_open()`. `is_active()` is also false once the envelope has ended.

- [ ] **Step 1: Write the failing tests:**
  - `pad_filter_reaches_the_voice`: the Sound is SVF at 1000. Pad 1 is LADDER at 200. Strike pad 1 → `played_filter().kind() == Ladder && cutoff == 200.0`. Strike pad 2 (default) → equals the Sound's filter.
  - `latched_at_the_hit`: strike pad 1, then publish pad 1 at cutoff 5000 and TYPE SEM. After 4 blocks the voice's played filter is still LADDER at 200. Task 8 narrows this to TYPE alone.
  - `amp_env_ends_the_hit`: PLAIN SINE with DECAY knob 127. AMP A 0, H 0, D 10, FAST → the voice is inactive within 300 ms and the 200 ms block is under −60 dB of block 0. AMP `OPEN` → still active at 300 ms.
  - `amp_note_off_is_ignored`: a note-off at 10 ms leaves an AHD hit's level curve equal to the one without a note-off (no Part VCA route).
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_voice_test` → FAIL.
- [ ] **Step 3:** Implement. Set the `memory_budget_test` ITCM figures to the measured sizes.
- [ ] **Step 4:** Run → PASS. Then run `$B cargo test -p chimera-core --test golden_test --test fx_golden_test --test voice_fx_golden_test --test drum_hits_test --test drum_engine_test` → PASS, with `drum_slice1_kit` bit-identical. Then `$B cargo test -p chimera-utils drums::` → 72 at −inf.
- [ ] **Step 5:** Run `$B just check` and `$B just stack-check` → green, ORBIT ≤ 63,232. Give bank-1 and ITCM in one line. Commit `DRUM: each pad's filter and amp in its hits`.

### Task 3: The pad's mod envelope, LFO and four routes

**Files:**
- Modify: `chimera-engine/src/dsp/drum/sound.rs` (pure route sum), `chimera-engine/src/dsp/drum/engine.rs`, `chimera-engine/src/dsp/voice.rs` (offsets, `FilterMods.glide`)
- Test: `chimera-core/tests/drum_pad_mods_test.rs`

**Interfaces:**
- Consumes: Task 1 `PadRoutes`, `PadLfo::lfo_params`, `PadEnv::env_params`; the voice's Part LFO values (`mod_values[ModSource::of_lfo(s)]`), `apply_offset` (block.rs:711).
- Produces:
  - `RouteValues([f32; 6])`, indexed by `RouteSource`. `RouteOffsets { cutoff, reso, volume, lfo_speed, lfo_depth }` (f32; `RouteOffsets::ZERO`).
  - `PadRoutes::offsets(&self, v: &RouteValues) -> RouteOffsets`: for each dest, Σ amount / 127 × v[source]. It is pure.
  - `DrumEngine::mods_block(&mut self, part_lfos: [f32; 3], vel: Velocity, sr: u32) -> RouteOffsets`. It runs the mod envelope and the pad LFO (one `Envelope` and one `Lfo`, built in place at the strike). LFO SPEED and DEPTH offsets act on the next block, as ADR 0250's LFO DEPTH does.
  - In `render_mono`, after the Part's routes: CUTOFF and RES go through `apply_offset(&mut m.filter, FilterParams::CUTOFF | RESONANCE, off)`. VOLUME ramps the AMP gain from last block's value to this one's, per sample, as the VCA sources do. `FilterMods.glide = !(mod_state.filter_routed() || pad.routes().moves_filter())`.

- [ ] **Step 1: Write the failing tests:**
  - `offsets_sum_by_dest`: routes [ModEnv→Cutoff 127, Lfo→Cutoff −64, Vel→Reso 32, OFF] with v ModEnv 1.0, Lfo 0.5, Vel 1.0 → `cutoff ≈ 1.0 − 64.0/127.0 × 0.5`, `reso ≈ 32.0/127.0`, `volume == 0.0` (1e-6).
  - `no_routes_no_offsets`: `PadRoutes` of `DEFAULT` → `RouteOffsets::ZERO` for any values.
  - `mod_env_opens_the_filter`: LADDER at 200 with ModEnv→Cutoff +127 and mod env A 0, H 0, D 40 → played cutoff > 800 in block 1 and within 1 % of 200 at 1 s.
  - `part_lfos_reach_pads`: Lfo1→Cutoff +64 with Part LFO 1 at 5 Hz → played cutoff takes ≥ 2 distinct values a second apart by 100 ms.
  - Review Focus 5, `routes_at_extremes_stay_bounded`: all four routes → Cutoff at +127, then → Reso at ±127, with pad LFO SPEED and DEPTH 127, on every `Machine::ALL`, at velocities 1 and 127, for 2 s. Every sample is finite, peak ≤ 2.0, and played cutoff stays inside `FilterParams`' CUTOFF spec range.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_pad_mods_test` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run → PASS. Task 2's goldens command → PASS, with `drum_slice1_kit` identical.
- [ ] **Step 5:** Run `$B just check` and `$B just stack-check` → green. Give bank-1 in one line. Commit `DRUM: a mod envelope, an LFO and four routes per pad`.

### Task 4: The bill follows the pads

**Files:**
- Modify: `chimera-engine/src/dsp/drum/sound.rs`, `chimera-engine/src/dsp/drum/view.rs` (a bill per Part), `chimera-engine/src/dsp/voice.rs`, `chimera-engine/src/instrument.rs` (admission of a DRUM Part)
- Test: `chimera-core/tests/drum_bill_test.rs`

**Interfaces:**
- Consumes: `FilterParams::filter_cost`, `FilterKind::move_cost`, `Voice::cost`.
- Produces:
  - `PadSound::cost(&self) -> Cost`: the filter's steady bill for its kind and mode, plus `move_cost` when `routes.moves_filter()`, plus `PAD_MOD_COST` when a route is non-zero or an envelope isn't open (`// ponytail: placeholder bill until the unit bench`, as slice 1's).
  - `fn kit_bill(kit: &Kit) -> Cost` (the max over pads), published by `KitWriter::publish` into the view (`KitView::bill(part) -> Cost`).
  - `Voice::drum_cost(p: &ParamSnapshot, mods: &ModState, kit: Cost) -> Cost` = `cost(p, mods)` with the Sound's steady filter bill swapped for `kit` (saturating). Admission bills a DRUM Part's hit with it.

- [ ] **Step 1: Write the failing tests:**
  - Review Focus 3, `a_2040_pad_bills_above_svf`: pad 5 on K2040 under an SVF Sound → `drum_cost > cost`, and the difference ≥ K2040's steady bill − SVF's.
  - `default_kit_bills_as_slice1`: `Kit::DEFAULT` → `drum_cost == cost` exactly.
  - `a_routed_pad_bills_its_move`: a ModEnv→Cutoff route on one pad → `kit_bill` includes `move_cost`.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_bill_test` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run → PASS. `$B cargo test -p chimera-core --test cost_test --test drum_hits_test` → PASS. Commit `DRUM: the bill follows the pads`.

### Task 5: Saving the pad sounds (record `KitSound`, 0x0025)

**Files:**
- Modify: `chimera-engine/src/storage/record.rs` (`RecordTag::KitSound` = 0x0025), `chimera-engine/src/project/codec.rs` (`put_kit_sound`, `get_kit_sound`, Part end), `chimera-engine/src/storage/disk_codes_v1.txt` (`C DRUM_ROUTE_SOURCE`, `C DRUM_ROUTE_DEST`, `C DRUM_LFO_MODE`)
- Test: `chimera-core/tests/drum_kit_codec_test.rs`

**Interfaces:**
- Consumes: Task 1 `PadSound::to_bytes` / `from_bytes` / `from_sound` / `DEFAULT`; Task 0 fixture.
- Produces: `KitSound` is one record a Part, after `Kit`, not critical: 56 B a pad, pads 1–12 (672 B). It takes `#[optimize(size)]`. The rules:
  - **Absent, with a `Kit` record, on a Part whose Sound is DRUM** (a slice-1 file): every pad gets `PadSound::from_sound(&part.sound.params)`, so the kit sounds as it did. Settled at the Part's end, once its Sound is read.
  - **Absent otherwise** (0.3 files, non-DRUM Parts): `PadSound::DEFAULT`.
  - Both absent cases still read Saved (not lossy).
  - An unknown code or a non-finite float takes the default's field and makes the load lossy. A wrong length is `Bounds`; a second `KitSound` is `Corrupt`.
  - NEW and a project load reset pad sounds with the kit.

- [ ] **Step 1: Write the failing tests:**
  - `kit_sound_round_trips`: an edited kit saves and loads byte-exact.
  - Review Focus 2, `slice1_file_takes_the_sounds_filter`: load `drum_slice1.prj` → Part 1's pads all `== PadSound::from_sound(&part1.sound.params)` (LADDER 420.0, 0.62, VEL 0.4, gate as written); Part 2's pads `== DEFAULT`; `!lossy`.
  - `unknown_route_code_defaults`: route source byte 99 → that route `== PadRoute::OFF`, lossy.
  - `wrong_length_is_bounds`, `two_kit_sounds_are_corrupt`, `new_resets_pad_sounds`.
  - Review Focus 4, `engine_switch_keeps_pad_sounds`: edit pad 3's filter, switch Part 1 to VA INIT and render (the VA voice's `played_filter` equals VA's Sound), switch back to DRUM INIT → pad 3's filter unchanged.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_kit_codec_test` → FAIL.
- [ ] **Step 3:** Implement. Add the codes to `disk_codes_v1.txt`, frozen.
- [ ] **Step 4:** Run → PASS. Then `$B cargo test -p chimera-core --test golden_test` → `drum_slice1_kit` identical, and the project/sound codec tests → PASS.
- [ ] **Step 5:** Run `$B just check` → green (new `RecordTag` arm). Commit `DRUM: pad sounds saved with the kit`.

### Task 6: Pad cells

**Files:**
- Modify: `chimera-engine/src/addr.rs` (`BlockRef::PadFilter`, `PadAmp`, `PadEnv`, `PadLfo`, `PadRoute`; `specs`; `in_sound` and `voice_reads` → `false`), `chimera-engine/src/dsp/drum/params.rs` (specs), `chimera-engine/src/block.rs` (`ParamSpec::with_id` if missing), `chimera-core/src/ui/block_def.rs` (`SlotBinding::PadFilterPanel(u8)`, `SlotBinding::SelectRoute`; `edits_pad` for every `Pad*` block), `chimera-core/src/ui/view.rs` (resolve them on the selected pad), the UI's write path for `BlockRef::Pad` (extend it to the new blocks)
- Test: `chimera-core/tests/drum_cells_test.rs`

**Interfaces:**
- Consumes: Task 1 types; slice 1's selected pad (`Kits::selected`) and its pad write and publish path.
- Produces:
  - `PAD_FILTER_SPECS` holds FilterParams' own spec entries for `PadFilter::IDS`, with the same ids, so `filter_panel::knob(kind, k)` targets map one to one.
  - `PAD_AMP_SPECS`: ATTACK, HOLD, DECAY (127 reads `INF`) and SPEED (new), then VEL, LPG, DECAY and STRIKE (OutParams' entries with ids replaced by `ParamSpec::with_id`).
  - `PAD_ENV_SPECS`: ATTACK, HOLD, DECAY, SPEED.
  - `PAD_LFO_SPECS`: WAVE, SPEED, DEPTH, PHASE, MODE.
  - `PAD_ROUTE_SPECS`: SOURCE, DEST, AMOUNT (−127..=127), on the selected route.
  - `ParamSlot::pad_filter_panel(k)` resolves on the selected pad's kind. Its Route, Keybd and Jack targets show `View::Empty`. `ParamSlot::select_route()` is ROUTE 1–4. The selected route is `UiState`'s `RouteId` and is not saved.

- [ ] **Step 1: Write the failing tests:**
  - `flt_cutoff_edits_the_selected_pad_only`: with pad 2 selected, a CUTOFF edit changes pad 2's sound and leaves pad 1's and the Sound's filter unchanged.
  - `pad_filter_panel_follows_the_pad`: pad 1 LADDER, pad 2 SEM → cells 0..=2 read `filter_panel::knob(kind, k)`'s labels for each pad's kind.
  - `kind_change_keeps_mode_valid`.
  - `route_cells_edit_the_selected_route`: ROUTE 2 → a SOURCE edit changes route 2 only. AMOUNT runs −127..=127.
  - `amp_decay_reads_inf_at_127`.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_cells_test` → FAIL.
- [ ] **Step 3:** Implement. Learn on a pad cell takes the path slice 1's `BlockRef::Pad` cells take. File an issue for per-pad CC.
- [ ] **Step 4:** Run → PASS. Run `$B just check` → green (five `BlockRef` arms). Commit `DRUM: pad filter, amp and mod cells`.

### Task 7: FLT, AMP and the pad's MOD pages

**Files:**
- Modify: `chimera-core/src/ui/block_registry.rs` (new `BlockDef`s at the next free ids; `DRUM_BLOCKS`; a DRUM MOD sub list), `chimera-core/src/ui/renderer.rs` (the FilterResponse, Adsr and LFO viz drawn from the selected pad on pad pages), `chimera-core/src/ui/components.rs` (header), the focus-band hint
- Test: `chimera-core/tests/drum_nav_test.rs`, `chimera-core/tests/screen/mod.rs`, `chimera-core/tests/screen_golden_test.rs` `GOLDENS`

**Interfaces:**
- Consumes: Task 6 slots.
- Produces: the chain stays `KIT · MCH · FLT · AMP · MOD`.

| Page | Cells (A–F) | Subs |
|---|---|---|
| DRUM FLT | KIND (pad) · FX TYPE (Part) · `pad_filter_panel(2)` · `pad_filter_panel(0)` · FX PLACE (Part) · `pad_filter_panel(1)`; viz FilterResponse of the pad | FMD · FX · JACK (Part's, as today) |
| DRUM AMP | ATTACK · HOLD · DECAY · SPEED · VEL · — ; viz Adsr of the pad's AHD | GATE, SENDS (Part's) |
| GATE | — · LPG · DECAY · STRIKE · — · — (Part AMP's places) | |
| PENV (MOD sub) | ATTACK · HOLD · DECAY · SPEED · — · — ; viz Adsr | |
| PLFO (MOD sub) | WAVE · SPEED · DEPTH · PHASE · MODE · — | |
| ROUTE (MOD sub) | ROUTE · SOURCE · DEST · AMOUNT · — · — | |

  The MOD node keeps the Part's MTX as its home. On DRUM its subs are PENV, PLFO, ROUTE, then MOD_SUB_PAGES' seven. Every page with a pad cell shows the header `PART 1 • 02 SNARE` and steps the pad on MIX + MINUS / PLUS, with the hint `MIX + −/+  PAD` in its focus band while a pad cell is touched. MTX, E1–E3, SPD, L1–L3, FMD, FX, JACK and SENDS are not pad pages.

- [ ] **Step 1: Write the failing tests:**
  - nav: MINUS / PLUS walks KIT → MCH → FLT → AMP → MOD. EDIT on AMP gives GATE then SENDS. MOD's subs start PENV, PLFO, ROUTE.
  - MIX + PLUS on FLT steps pad 1 → 2, and the CUTOFF cell shows pad 2's value.
  - Slice 1's MTX case still holds: MIX + MINUS on MTX deletes the route and leaves the pad.
  - The header on FLT reads `PART 1 • 02 SNARE`.
  - Screen goldens: `drum_flt_ladder`, `drum_flt_sem`, `drum_amp`, `drum_gate`, `drum_penv`, `drum_plfo`, `drum_route`, `drum_flt_hint`.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_nav_test` and `$B cargo test -p chimera-core --features master-tape --test screen_golden_test` → FAIL.
- [ ] **Step 3:** Implement. Record the goldens with `GOLDEN_RECORD=1` and look at every PNG (`just screens`) before committing.
- [ ] **Step 4:** Run → PASS. Run `$B just check` → green. Commit `DRUM: FLT, AMP and MOD follow the pad`.

### Task 8: Live edits reach the ringing hit, glided (cut if the owner rules latch)

**Files:**
- Modify: `chimera-engine/src/dsp/drum/view.rs`, `chimera-engine/src/dsp/drum/engine.rs`, `chimera-engine/src/dsp/drum/sound.rs`, `chimera-core/tests/drum_voice_test.rs` (narrow `latched_at_the_hit` to TYPE)
- Test: `chimera-core/tests/drum_live_test.rs`

**Interfaces:**
- Consumes: Task 2's view and `DrumEngine`. `ctx.part`. `dsp::knobs`' KNOB coefficient.
- Produces:
  - `KitView::sound_if_newer(&self, part: PartId, pad: PadId, seen: &mut u32) -> Option<PadSound>`: one `seq` load, compared with `seen`. It decodes only after a publish, and never waits.
  - `PadSound::live_target(&self, live: &PadSound) -> PadSound`: `self`'s choices (kind, mode, voice, env SPEED, LFO WAVE and MODE, each route's source and dest) with `live`'s continuous values.
  - `DrumEngine::follow(&mut self, part: PartId)`, called once a block before `overlay`. It eases the hit's continuous values toward the target on each value's normalized spec position (`ParamSpec::normalize` / `denormalize`, CUTOFF's log law included) at the KNOB coefficient (ADR 0112/0116). The `EnvParams` and `LfoParams` it runs move with them.

- [ ] **Step 1: Write the failing tests:**
  - Review Focus 1, `cutoff_turn_glides_on_the_ringing_hit`: TONE CYMBAL on LADDER at 300. Strike, then 10 blocks later publish cutoff 6000. Played cutoff rises monotonically. Each block's step matches what `Knobs` does for Filter CUTOFF over the same blocks within 1e-4 normalized. It is within 1 % of 6000 by 300 ms.
  - `type_change_waits_for_the_next_hit`: publish SEM during the ring → LADDER until the next hit, then SEM.
  - `other_pads_hold`: edit pad 10 → a ringing pad 9's played filter doesn't move.
  - `machine_knobs_stay_latched`: a DECAY knob edit doesn't reach the ringing hit.
  - `no_publish_no_read`: `sound_if_newer` → `None` while `seq` is unchanged.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_live_test` → FAIL.
- [ ] **Step 3:** Implement. Amend ADR 0269, still Proposed: the voice re-reads its pad's sound from the view after a publish.
- [ ] **Step 4:** Run → PASS. Task 2's goldens command → PASS. `$B just check` and `$B just stack-check` → green. Commit `DRUM: live pad edits glide into ringing hits`.

### Task 9: Playable on the desktop; flash, RAM and stack

**Files:**
- Modify: `chimera-core/tests/drum_e2e_test.rs`, `tools/bank1-budget.txt`, `docs/adr/0269-…` (measured figures), whatever `just check` names

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the failing e2e test** `pad_filter_from_the_ui`. Through `UiState` + `Instrument`: on DRUM INIT, open FLT, set KIND LADDER and CUTOFF 300 on pad 1, MIX + PLUS, set pad 2's CUTOFF 4000, then play notes 0 and 1. The two voices' played filters are LADDER/300 and SVF/4000, and the Sound's filter is unchanged.
- [ ] **Step 2:** Run `$B cargo test -p chimera-core --test drum_e2e_test` → FAIL until the wiring is closed, then PASS.
- [ ] **Step 3:** In `just desktop`, load DRUM INIT, give the hats a resonant LADDER and the kick a short AMP, sweep CUTOFF on a ringing cymbal, and screenshot FLT, AMP and ROUTE for the owner. If the DRUM guide page exists, add annotated shots (yellow numbered callouts). Otherwise note them on the docs issue.
- [ ] **Step 4:** Run `$B just test` (full suite), `$B just check`, `$B just stack-check` and `$B cargo test -p chimera-utils drums::` → all green, nulls −inf. Raise bank-1 lines only as far as the build needs. Write the measured bank 1, bank 2, ITCM and stack figures into ADR 0269 and report them in one line.
- [ ] **Step 5:** Commit `DRUM: per-pad filter and amp, playable`. Open the PR into `0.4`.

---

## Self-review

- **Spec coverage:** own FILTER (Tasks 1, 2, 6, 7), AMP envelope (1, 2, 7), one mod envelope, LFO and 4 routes (1, 3, 6, 7), the Part's three LFOs shared (sources `Lfo1`–`Lfo3`, Task 3), FLT and AMP following the pad (6, 7), ITCM (2), kit view publish and copy at the hit (2), latched vs live (Architecture, 2, 8, Review Focus 1), saving and old kits (5), types first (1), sound unchanged (0, 2, 3, 5), scope fence (Task 0, Global Constraints).
- **Types:** `PadSound`, `PadFilter`, `PadAmp`, `PadEnv`, `PadLfo`, `PadRoutes`, `RouteSource`, `RouteDest`, `RouteOffsets`, `KitView::hit`/`bill`/`sound_if_newer`, `DrumEngine::overlay`/`amp_block`/`mods_block`/`follow` and `Voice::drum_cost` are named the same everywhere they appear.
- **Proportion:** signatures, values and assertions only. Bodies are left to the implementer except the byte layout and the record rules, which the disk format fixes.
