# Modal 2 Resonators (Step A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modal's four models are rebuilt in place so every string loop is stable by construction and in tune, every model reads the same four modulatable macros (STRUCTURE, BRIGHT, DAMP, POS) in its own way, and old patches load translated.

**Architecture:** The loop parts are small pure types in `dsp/modal/loop_parts.rs` (ours: `LoopGain`, `DcBlocker`, the fractional `Allpass1` and its exact phase-delay maths, the release ramp) and `dsp/modal/dispersion.rs` and `dsp/modal/chords.rs` (Rings-derived, MIT notice). `KsString` becomes a ring whose read tap sits a fractional delay behind the write, with a symmetric 3-tap loop low-pass, a DC blocker and a fractional allpass in the loop; every other in-loop delay is computed exactly at f0 and taken off the line. `ModalEngine` eases the four macros once per block from the voice's modulated `ModalParams`; the model-page settings are read at note-on. One pure table, `modal::reads(mode, id)`, decides what each model reads; the page cells, `view::dimmed` and the audio test all use it. Old files translate once, in `decode_block`, through a new `Translation` hook beside `Migration`.

**Tech Stack:** Rust 2024, `no_std` `chimera-core` (f32 DSP, `libm`), `thumbv7em-none-eabihf` firmware, `just`.

**Spec:** `docs/superpowers/specs/2026-09-29-modal-2-resonators-design.md` (owner-approved, binding). Read it with this plan; § numbers below are the spec's.

**Where it runs:** worktree `$SP/wt-m2`, branch `modal2-resonators`, based on `exclusive-state` (PR #213), where `SP=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad`. Every command runs from the worktree root.

## Global Constraints

- **Stability (§ 2):** `LoopGain::MAX = 0.9995`; its only constructors clamp to `[0, MAX]` (NaN → 0). No string loop multiplies by anything but a `LoopGain` and filters whose gain is ≤ 1 at every frequency. The DC blocker's corner is `DC_HZ = 10.0`, in every string loop: STRING, the SYMP main string, each halo string and BOWED. FDBK and the `±1.5` clamp are deleted.
- **Timing constants:** release ramp `RELEASE_SAMPLES = 240` (5 ms), chord glide `CHORD_GLIDE_SAMPLES = 960` (20 ms), macro easing `EASE = 0.3` per block, ensemble LFO 0.1–6 Hz, detune up to ±15 cents, `ENS_HEADS = 3`.
- **Defaults carry today's sound:** COUPLE 0.25 maps to today's 0.025 (`coupling = 0.1 · couple`); HALO 0.25 maps to today's 0.15 (`level = 0.6 · halo`); MODES defaults to 32. `ModalParams::default()` is exactly what the v1 translation makes of today's default (Task 4 pins it bit for bit).
- **Disk (ADR 0045):** kept ids and idents: MODE 0 `MODE`, EXCITE 1, BRIGHT 3, POS 4, BODY 6, E.DPT 9, E.RAT 10, E.MIX 11. Retired: `(1, 2)` DECAY, `(1, 5)` INHARM, `(1, 7)` STIFF, `(1, 8)` FDBK. New: DAMP 12 `DAMP`, STRUCTURE 13 `STRUCTURE`, COUPLE 14 `COUPLE`, HALO 15 `HALO`, MODES 16 `MODES` (Enum, codes 0–3, idents `M16 M24 M32 M48`). Task 13 adds FORCE 17 `FORCE`, SPEED 18 `SPEED`, COLOR 19 `COLOR` and BURST 20 `BURST`. `ResonatorMode`'s codes and idents are untouched. The fixture is only appended to.
- **Modulation (ADR 0010, superseded in part by ADR 0056):** exactly STRUCTURE, BRIGHT, DAMP and POS are `modulatable`. Model-page settings, EXC's settings (Task 13) and MODEL are not.
- **Memory:** no new buffers. `MAX_STRING_DELAY` goes from 984 to 1016 (Task 2, the one growth, about 4.6 KB of D2, recorded in ADR 0056). `size_of::<Instrument>() <= VOICE_RAM_BUDGET` stays asserted.
- **Audio thread:** no heap, no blocking. Transcendentals (`atan2f`, `sinf`, `expf`, `powf`) run per note-on or per block, never per sample. `just stack-check` stays green.
- **Goldens (ADR 0011):** Task 1 moves the four Modal rows (`modal_init`, `modal_lfo_cutoff`, `modal_sympathetic`, `algo_to_modal_switch`) and the `init_modal.snd` render row into a `PENDING` list that the checks skip. Task 11 re-records them once and empties `PENDING` and `KNOWN_BROKEN`. Every other golden stays bit-identical in every task.
- **ADR:** `docs/adr/0056-modal-resonators-share-four-macros.md` (the next free number), `Status: Proposed`, from `0000-template.md`, with a row in `docs/adr/README.md`. Task 1 creates it; later tasks amend it while it is Proposed. Never edit an accepted ADR.
- **Provenance (ADR 0032):** `chords.rs` and `dispersion.rs` carry Mutable Instruments' MIT notice (Copyright 2015 Emilie Gillet); `loop_parts.rs`, the body filter, the ensemble and the macro mapping are ours and say so.
- **Green gate per task:** `just check` exits 0 (it runs the tests, the firmware builds, `just clippy`, `cargo fmt --check` and `just stack-check`). If ALSA's pkg-config is missing, set `PKG_CONFIG_PATH` as the Justfile says.
- **Commits:** a terse plain sentence, no type prefix, never a Co-Authored-By or other attribution line. Stage named paths only. Never stage `docs/chimera-ui-ux-spec.md` or `chimera.bin`.
- **Hardware:** no flash until Task 12, which runs last, after Tasks 13 and 14. Between tasks, the owner listens on the desktop (the demo renderer steps below, or `just desktop`).
- **Task order (owner, 2026-09-30):** Tasks 1–11, then Task 13 (the EXC node), then Task 14 (the two-delay bow, #240), then Task 12 (the ship flash).

## Review Focus

1. **DAMP modulated to its top during a release.** An LFO or ENV sweeps DAMP to 1 while a released STRING note dies. It should still die: the release gain wins over the macro. Test: `a_released_note_ends_with_damp_at_its_top` (Task 7).
2. **The widest chord at the lowest note.** SYMP at STRUCTURE 1 (and every other chord) on G1: Rings' −12 interval asks for a period twice the line. Each halo string should fold up an octave until it fits, never index past `MAX_STRING_DELAY`, and play its folded pitch. Test: `every_chord_fits_the_line_at_g1` (Task 10).
3. **MODES changed while bank notes sound.** A ringing 48-mode note should keep 48 modes (no click from stale filters) and be billed at 48 until it ends. Test: `modes_change_keeps_sounding_notes_and_their_bill` (Task 5).
4. **An old patch with FDBK at 1.** A v1 STRING patch with FDBK 1 and DECAY 0 (today's runaway) should load and play bounded, with no DC. Test: `an_old_fdbk_1_patch_loads_stable` (Task 4).
5. **The ensemble at full depth on G1.** DEPTH 1 at 0.1 Hz and at 6 Hz on the longest loop: the heads must stay inside the loop, with no clicks and a bounded output. Test: `ensemble_at_full_depth_on_g1_stays_in_the_line` (Task 9).
6. **A soft key on Bowed (Task 13).** A velocity-20 note on the default Bowed should bow, and a routed DAMP pushed to its top after the lift should not hold the ring. Tests: `a_soft_bowed_note_sounds`, and `macros_are_routable` with BOWED's DAMP through a release (Task 13).
7. **The bow's splice under a sweep at the extremes (Task 14).** POS swept end to end by a square LFO at G1 (the longest line) and at C7 (a bridge line of a few samples), at FORCE 1 and SPEED 1: the split must stay within `[1, d − 1]`, the sum must stay the line, and nothing may click or run away. Tests: `a_bowed_pos_sweep_does_not_click`, `bowed_is_stable_and_in_tune_at_every_corner` and the unit test `a_splice_step_keeps_the_loop_length` (Task 14).

## Spec ambiguities ruled here

- **DECAY → DAMP direction.** DAMP runs short → long on every model (§ 1). Today's bank DECAY already did, and today's string DECAY ran the other way. So the translation is DAMP = DECAY on BANK, and DAMP = 1 − DECAY on STRING, SYMP and BOWED. Today's string BRIGHT ran dark at the top. The knob now runs dark → bright on every model, and an old patch must keep its tone (owner, spec § 3: old patches load and keep their sound where the fix allows), so BRIGHT = 1 − BRIGHT on STRING, SYMP and BOWED and as-is on BANK. INIT's BRIGHT becomes 0.3, today's INIT tone in the new direction.
- **The 10 Hz DC blocker vs G1.** In the loop, at 49 Hz, the blocker is a 31-sample phase advance. Keeping G1 in tune (±2 cents) needs a 1,011-sample line. The line grows to 1,016 samples, not the corner down. That costs about 4.6 KB of the ~126 KB D2 headroom. It is the plan's one departure from "no D2 growth", and it partly supersedes ADR 0040 (in ADR 0056).
- **"No duplicate intervals".** The halo table is Rings' single-voice chords with the 0.0 dropped, since the main string plays that note. That leaves 7 distinct intervals per chord. Rings' 0.01-apart pairs (3.0 / 3.01) are its detuned chorus, and they stay.
- **MODEL order.** The encoder keeps today's value order (STRING, BANK, BOWED, SYMP). Only the shown names change. § 1's list is names, not an order.
- **SYMP ensemble rate.** ENS RATE isn't on SYMP's page, so SYMP runs a fixed `SYMP_ENS_RATE = 0.3` (normalized, ≈ 0.34 Hz). The stored ENS RATE changes nothing there.
- **BANK note-off.** § 2 keeps the bank unchanged, so it rings out on DAMP after note-off. The release ramp applies to string loops. BOWED's bow force ramps instead.
- **SYMP chord reads un-eased STRUCTURE.** Chords are discrete, and the 20 ms glide is their easing. Easing the index too would miss the 25 ms bound.
- **Sympathetic's size.** SYMP's main string is STRING's full string (body, ensemble, dispersion), so `SympatheticVoice` is at most `StringVoice` plus one align, not Bowed's size. That replaces ADR 0054's const assert, and ADR 0056 records the change.
- **Task 13 (owner, 2026-09-30).** The amended spec leaves these open; they are ruled here:
  - **BURST** is the strike's burst length, named and ranged as the old law: `2 + 4·BURST` ms. Its default is 0.8, EXCITE's. v1 BANK patches take BURST = EXCITE, so an old strike keeps its length.
  - **COLOR** keeps the old `ks_color` law: `⌊(1 − COLOR)·7⌋` smoothing passes, 8 steps. The old hidden 0.8 is one pass, today's. A continuous one-pole would not keep INIT bit for bit.
  - **STRUCTURE stays dimmed on BOWED.** The owner named DAMP, BRIGHT and POS.
  - **DAMP on BOWED** sets the lifted bow's release target; the bowed loop keeps `BOW_GAIN` while the bow is on, as before.
  - ~~**POS on BOWED** is a second, interpolated tap feeding only the friction, so the loop's period, and the pitch, are untouched. It uses the pluck's comb law and 0.03 threshold.~~ Superseded by Task 14 (POS became an output comb in Task 13, then the bow position in Task 14).
  - ~~**BRIGHT on BOWED** is the strings' linear-phase 3-tap low-pass at `0.25·(1 − BRIGHT)`, so it adds no delay. At BRIGHT 1 the tap is read alone.~~ Superseded by Task 14.
  - ~~**Old Bowed patches' macros:** v1 Bowed stored BRIGHT, DAMP and POS but never read them. They translate to the values that reproduce today's Bowed (BRIGHT 1, POS 0, DAMP = `damp_for(RELEASE_T60)`), extending "FORCE and SPEED reproduce today's sound" to the three newly live macros. The held note is pinned bit for bit. The release matches to within the `powf`/`logf` round trip.~~ Superseded by Task 14: the old sound was the bug (#240).
  - **The bow's lift** sheds the note's force over `RELEASE_SAMPLES` at any FORCE (`lift = force / RELEASE_SAMPLES`), which equals today's `BOW_LIFT` at full force.
  - **EXC's settings** are note-on and unmodulatable, like the model page. Step B's mixer decides what becomes live.
  - **EXC's header** reads the exciter's name (PLUCK, STRIKE, BOW), and the map node reads EXC. The Part opens on EXC, the chain's first node.
- **Task 14 (owner, 2026-09-30, #240).** The amended spec's § 2 BOWED leaves these open; they are ruled here:
  - **One ring, two lines, spliced at the bow.** The ring holds the bridge line's cells and then the nut line's. Each sample, at the write the nut line's oldest cell is read (the nut's return) and the bridge line's input is written; `Lb` cells behind it the bridge line's cell is read (the bridge's return) and the nut line's input is written in its place. The ring's loop length `d` is `Lb + Ln`, so the two lines cost no new memory (D2 +0 B; `BowedString` grows by a few registers, inside `ModelSlot`'s 4,160 B). Two separate rings would need about 1.5× the line (+1.8 KB a voice, past `ModelSlot`).
  - **Pitch.** The loop is `Lb + Ln` whole samples, the bridge filter's one sample (`BRIDGE_DELAY`) and the tuning allpass's fraction on the bridge line's input: `KsString::set_period(period, BRIDGE_DELAY, w)`, the existing `split` and `eta_for`. Each end reflects inverted (nut −1, bridge −g·H), so the wave comes back upright once a period: C3 is 130.8 Hz.
  - **The split is whole samples.** `Lb = round(β·d)`, clamped to `[1, d − 1]`, with β = `BETA_MIN + (BETA_MAX − BETA_MIN)·POS`, `BETA_MIN = 0.06` and `BETA_MAX = 0.5` (near the bridge to the middle). The sum stays `d`, so the pitch is exact at every split. A fractional split needs two interpolated reads, whose loss varies with the fraction and would move the tone as much as BRIGHT does; two allpasses cost more and keep the whole-sample crossing. A whole sample of bow position is 1/d of the string: 0.3 % at C3, 4 % at C7, where few harmonics sound.
  - **The split glides.** One whole-sample step at most every `BOW_SLEW = 32` samples, at the block's samples 0 and 32: two a block, `DISP_SLEW`'s rate. A step of +1 re-reads the last bridge return (held in the bridge filter's register) and drops one nut-line sample; a step of −1 writes the nut line's input into both cells, repeating one. Either way the sum is unchanged and nothing is read from the wrong line.
  - **The bow table (ours).** `ρ(Δv) = w⁴ / (w⁴ + Δv⁴ + 1e-20)` with `w = BOW_WIDTH·force`, `BOW_WIDTH = 0.3`: 1 at rest (the string sticks), ½ at `|Δv| = w`, falling as `Δv⁻⁴` (it slips). The push `Δv·ρ` is at most `0.57·w`. FORCE widens the stick region, which is its pressure; the curve has no offset, since an offset puts DC into the string. Force 0 gives `ρ = 0`, exactly, so a lifted bow lets the string ring free. One `vdiv.f32` a sample, no `tanhf`, no `powf`. STK's table (`|x·slope + offset| + 0.75` to the −4th, slope `5 − 4·pressure`) is not used.
  - **The output** is the wave the bow sends toward the bridge, × `BOW_OUT`: what the bridge hears `Lb` samples later, with the same spectrum, and non-zero from the first sample, so a low note sounds in its first block (#206). `BOW_OUT` is set so the v1 Bowed patch's held C3 is within ±1 dB of the old bow's RMS (Step 1 records it).
  - **BRIGHT** is the bridge filter, `c/2·(a[n] + a[n−2]) + (1 − c)·a[n−1]` on the bridge's returns, `c = BOW_BRIGHT·(1 − BRIGHT)`, `BOW_BRIGHT = 0.5`: linear phase, one sample's delay at every frequency, `|H| ≤ 1`. It is in the loop once a period, so it sets how fast the upper harmonics lose energy, and the bow no longer re-saturates them. If `bowed_bright_is_heard` reads under 3 dB at `BOW_BRIGHT = 0.5`, square the filter (five taps, two samples' delay, `BRIDGE_DELAY = 2.0`), still linear phase; don't raise `c` past 0.5, which would break `|H| ≤ 1`.
  - **DAMP** keeps Task 13's law: the bridge's `LoopGain` is `BOW_GAIN` while bowed, and ramps at note-off to `LoopGain::from_t60(t60(damp), f0)` through `Release`. That is exact per period, since the gain is met once a period.
  - **FORCE and SPEED** keep Task 13's easing (`BOW_EASE` a sample, from the block's values while bowing), the lift, `bow_force`, `BOW_SPEED = 0.3` and `vel_scale`. The force now sets `w`; `w⁴` is two multiplies a sample from the eased force.
  - **v1 Bowed patches** translate to FORCE 0.5, SPEED 0.5 (the defaults), POS 0.15 (β ≈ 0.126, about an eighth of the string from the bridge, a normal contact point), BRIGHT 0.5 (`c = 0.25`, a moderately lossy bridge) and DAMP `damp_for(0.5)` (a 0.5 s ring after the lift, where the old bow choked in 0.12 s). INIT is untouched: it is STRING's, and at MODEL → BOWED it bows at POS 0 (β 0.06, near the bridge), BRIGHT 0.3 and INIT's long ring.
  - **Stability.** The loop's linear gain is `g·|H| < 1` per period (nut −1, bridge `−g·H`, `g` a `LoopGain`), and the junction adds at most `0.57·w` to each line a sample, so every wave is bounded by `2·0.57·BOW_WIDTH / (1 − LoopGain::MAX)`. In practice the string moves at about the bow's speed; the tests hold the output to 4.0. The output keeps its blocker.
  - **Supersedes** Task 13's three parked Bowed DSP findings (the POS crossfade at 0.03, BRIGHT through both taps, hoisting the per-sample work): the code they were about is deleted.
- **Cost.** SYMP's host estimate (950) is above the +30–60 the spec expected: the halo's seven fractional allpasses and blockers add about 80. Unless the ship bench reads ≤ 883, SYMP gets 5 voices on rev V instead of 6. The bench decides (Task 12).

## Files

| File | Responsibility | Tasks |
|---|---|---|
| `chimera-core/src/dsp/modal/loop_parts.rs` (new, ours) | `LoopGain`, `DcBlocker`, `Allpass1`, `dc_phase_delay`, `allpass_phase_delay`, `eta_for`, `split`, `Release` | 1, 2, 7 |
| `chimera-core/src/dsp/modal/string.rs` | `KsString`: ring + fractional tap, 3-tap low-pass, DC blocker; `StringVoice` (body, ensemble, dispersion); COLOR's passes, Bowed's taps (Task 13, deleted by 14), `KsString::guide` (14) | 1, 2, 8, 9, 13, 14 |
| `chimera-core/src/dsp/modal/bow.rs` (new, ours, after Smith) | `BowedString`, the bow table, the split and its glide, the bridge filter, `render` in spans and its `tick` reference | 14 |
| `chimera-core/src/dsp/modal/dispersion.rs` (new, MIT) | `Dispersion`: 4 first-order allpasses, Rings' `ap_gain` law | 8 |
| `chimera-core/src/dsp/modal/body.rs` (new, ours) | `Body`: three fixed SVF resonances on the output | 8 |
| `chimera-core/src/dsp/modal/ensemble.rs` (new, ours) | `Ensemble`: quadrature LFO, 3 interpolated heads | 9 |
| `chimera-core/src/dsp/modal/chords.rs` (new, MIT) | `CHORDS: [[f32; 7]; 11]`, `chord_of`, `fold` | 10 |
| `chimera-core/src/dsp/modal/params.rs` | the new `ModalParams`, `BankModes`, `MODAL_SPECS`, `reads`, `page_cells`, `translate_v1`; EXC's fields, `ModalPage`, `bow_force`; v1 Bowed's new defaults | 3, 4, 5, 13, 14 |
| `chimera-core/src/dsp/modal/mod.rs` | `ModalEngine` (eased macros, deferred pluck, release, playing cost), models' render; the playable Bow and the strike's BURST; Bowed wired to `bow.rs`, `COST_BOWED` | 1–10, 13, 14 |
| `chimera-core/src/storage/{codes,block_codec,sound,system}.rs` | `RETIRED`, `Translation`, `Retired`, `TRANSLATIONS` | 3, 4 |
| `chimera-core/src/dsp/voice.rs:162-170` | `held_model_extra` through `playing_cost` | 5 |
| `chimera-core/src/ui/{block_def,view,block_registry}.rs` | `SlotBinding::ModalPanel`, `SlotCtx::model`, MDL/MDL2 pages, SPACE, dimming; the EXC node | 6, 13 |
| `chimera-core/tests/modal_resonator_test.rs` (new) | the spec's audio tests, plus Review Focus 1, 2, 3, 5, 7 | 1–10, 13, 14 |
| `chimera-core/tests/common/mod.rs` | `clicks`, `modal_engine`, `play_modal` | 1 |
| `chimera-core/tests/{golden,codec_compat,modulatable,click_free,sanity,memory_budget,cost,disk_codes,part_page,mod_registry,modal,modal_integration,exclusive_state,screen_golden}_test.rs`, `tests/screen/mod.rs`, `tests/fixtures/disk_codes_v1.txt` | pins moved, rows appended, goldens re-recorded | 1–11, 13 |
| `chimera-stm32/src/bench.rs:231-262` | new MDL rows; MDL BOW+, PLUCK DARK; BOW+'s doc (14) | 11, 13, 14 |
| `chimera-core/src/ui/{components,renderer}.rs` | EXC's header named for the exciter | 13 |
| `chimera-core/tests/{block_def_tests,header_map_test,binding_test}.rs` | the EXC node and its header | 13 |
| `docs/adr/0056-modal-resonators-share-four-macros.md`, `docs/adr/README.md`, `THIRD_PARTY.md` | the ADR, provenance | 1, 2, 5, 8, 10, 11, 12, 13, 14 |

---

### Task 1: Stable loops: `LoopGain`, the DC blocker, FDBK out of the DSP

**Files:**
- Create: `chimera-core/src/dsp/modal/loop_parts.rs`, `chimera-core/tests/modal_resonator_test.rs`, `docs/adr/0056-modal-resonators-share-four-macros.md`
- Modify: `chimera-core/src/dsp/modal/string.rs:216-234` (`KsRenderParams`), `:405-529` (`damp`, `tick_full`, `tick_coupled`, `lowpass`); `chimera-core/src/dsp/modal/mod.rs:814-957` (the three string renders), `:87-93` (`BowedString`); `chimera-core/tests/common/mod.rs`; `chimera-core/tests/click_free_test.rs:21-84`; `chimera-core/tests/golden_test.rs:263-312`; `chimera-core/tests/codec_compat_test.rs:140-170`; `docs/adr/README.md`

**Interfaces:**
- Produces, in `modal::loop_parts` (`pub(super)`):

```rust
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LoopGain(f32);
impl LoopGain {
    pub const MAX: f32 = 0.9995;
    pub fn new(g: f32) -> Self;                      // clamps to [0, MAX]; NaN → 0
    pub fn from_t60(t60_s: f32, freq_hz: f32) -> Self; // new(0.001^(1 / (t60_s · freq_hz)))
    pub fn get(self) -> f32;
    pub fn min(self, o: Self) -> Self;
}
pub const DC_HZ: f32 = 10.0;
pub struct DcBlocker { r: f32, g: f32, x1: f32, y1: f32 }
impl DcBlocker {
    pub fn new(sample_rate: u32) -> Self;            // state 0
    pub fn process(&mut self, x: f32) -> f32;
    pub fn r(&self) -> f32;
    pub fn reset(&mut self);
}
```

  The blocker is normalized so its gain is at most 1 at every frequency. The implementer can't derive this from the tests:

```rust
// y = g·(x − x1) + r·y1,  r = e^(−2π·DC_HZ/fs),  g = (1 + r) / 2   (|H| = 1 at Nyquist, < 1 elsewhere)
```

- `KsRenderParams` loses `feedback`. It gains `gain: LoopGain`, which replaces the in-loop `0.999 - decay * 0.009`, now `LoopGain::new(0.999 - decay * 0.009)` at each call site. `decay` stays until Task 3.
- `KsString` gains `dc: DcBlocker`, applied to the filtered sample before it is written back (`tick_full`, `tick_coupled`). `BowedString` gains `dc: DcBlocker` on the pushed sample.
- Test support in `tests/common/mod.rs`:
  - `pub fn clicks(out: &[f32]) -> Vec<(usize, f32)>`: `click_free_test`'s detector, moved here unchanged. It flags each `i` where `|tanh(0.4·out[i]) − tanh(0.4·out[i−1])| > 0.15`.
  - `pub fn play_modal(p: &ModalParams, note: u8, on_blocks: usize, off_blocks: usize) -> Vec<f32>`: a boxed `SymPool` and `ModalEngine::new_in`, rendered block by block with `note_off` after `on_blocks`.

- [ ] **Step 1: Write the failing test** `every_model_is_stable_at_every_extreme` in `modal_resonator_test.rs`.
  - For each of the four modes and each spec in `MODAL_SPECS` other than MODE, render at `spec.min` and at `spec.max`, the others at their defaults. Hold note 36 (C2) for 30 s (`30 * SR / BLOCK_SIZE` blocks).
  - It iterates the spec table, so it follows Tasks 3–10's params unchanged.
  - Assertions, with `last` the final second and `second` the samples of seconds 1–2:

```rust
assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 4.0), "{mode:?} {} = {v}: bounded", s.label);
assert!(rms(last) <= rms(second) * 1.001 + 1e-6, "{mode:?} {} = {v}: grows", s.label);
assert!((last.iter().sum::<f32>() / last.len() as f32).abs() < 1e-3, "{mode:?} {} = {v}: DC", s.label);
```

- [ ] **Step 2: Run it to verify it fails.** Run `cargo test -p chimera-core --test modal_resonator_test every_model_is_stable_at_every_extreme`. Expected: FAIL on STRING, FDBK = 1 ("grows" or "DC").
- [ ] **Step 3: Implement.**
  - Write `loop_parts.rs` as in Interfaces, with unit tests:
    - `loop_gain_never_reaches_one`: `new(1.0)`, `new(2.0)`, `from_t60(1e9, 49.0)` and `new(f32::NAN)` are all `<= MAX`, and NaN gives 0.
    - `dc_blocker_gain_is_at_most_one`: `|H(e^{jω})| <= 1 + 1e-6` over 512 ω from 0 to π.
  - Delete the FDBK branch and the clamp from `tick_full`; `KsRenderParams.feedback` goes.
  - Run every string loop through its `dc` and its `LoopGain`.
  - In `render_bowed`, push `b.dc.process(clamped)`.
  - `ModalParams::ks_feedback` stays, unread, until Task 3.
- [ ] **Step 4: Move today's Modal pins aside.**
  - In `golden_test.rs`, add `const PENDING: &[&str] = &["modal_init", "modal_lfo_cutoff", "modal_sympathetic", "algo_to_modal_switch"];`, with a comment: "re-recorded once, in Modal 2 step A's last task". `goldens_match` and `goldens_match_through_the_instrument` drop those names from both the rows and `got` before comparing.
  - In `codec_compat_test.rs`, `v1_fixtures_render_identically` skips `init_modal.snd` for the same reason.
  - Move `click_free_test`'s detector to `common::clicks` and call it from there.
- [ ] **Step 5: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test modal_resonator_test --test golden_test --test codec_compat_test --test click_free_test --test sanity_test --test modal_test --lib modal`. Expected: PASS. Only Modal output moved. If a non-Modal golden fails, stop and investigate.
- [ ] **Step 6: Write ADR 0056** (Proposed, Deciders: project owner).
  - **Context:** #191's survey, where FDBK above ~0.012 made the loop gain >1, a clamp latched DC, and no DC blocker existed.
  - **Decision (so far):** `LoopGain` capped at 0.9995 by its constructor; a 10 Hz normalized DC blocker in every string loop; FDBK and its clamp deleted.
  - **Sources:** the spec and this plan.
  - In the README, add row 0056: "Modal's resonators share four modulatable macros; loops are stable by construction (supersedes in part 0010, 0040, 0054)" | Proposed.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/modal/loop_parts.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/modal_resonator_test.rs chimera-core/tests/common/mod.rs chimera-core/tests/click_free_test.rs chimera-core/tests/golden_test.rs chimera-core/tests/codec_compat_test.rs docs/adr/0056-modal-resonators-share-four-macros.md docs/adr/README.md
git commit -m "Modal's string loops cannot run away: LoopGain, a DC blocker, no FDBK"
```

---

### Task 2: Fractional tuning (#163)

**Files:**
- Modify: `chimera-core/src/dsp/modal/string.rs:211-403` (`MAX_STRING_DELAY`, the ring, `set_freq`, `trigger`, `clear`, `ring_tap`), `:415-529` (the ticks, the low-pass); `chimera-core/src/dsp/modal/loop_parts.rs`; `chimera-core/src/dsp/modal/mod.rs:465-484` (`retune`), `:772-777` (`SympatheticSet::tune`); `chimera-core/tests/memory_budget_test.rs:69-79`; `docs/adr/0056-*.md`
- Test: `chimera-core/tests/modal_resonator_test.rs`, `loop_parts.rs` unit tests

**Interfaces:**
- Consumes: `DcBlocker::r` (Task 1).
- Produces, in `loop_parts`:

```rust
pub struct Allpass1 { eta: f32, x1: f32, y1: f32 }      // (η + z⁻¹)/(1 + η z⁻¹)
impl Allpass1 { pub fn set(&mut self, eta: f32); pub fn process(&mut self, x: f32) -> f32; pub fn reset(&mut self); }
pub fn allpass_phase_delay(eta: f32, w: f32) -> f32;    // samples, at ω rad/sample
pub fn dc_phase_delay(r: f32, w: f32) -> f32;           // samples; negative (an advance)
pub fn eta_for(frac: f32, w: f32) -> f32;               // exact: the η whose phase delay at ω is `frac`
pub fn split(period: f32, other: f32, w: f32) -> (usize, f32); // (line delay, η)
pub const MIN_LINE: usize = 2;
```

  These formulas are not determined by the signatures; use them exactly:

```rust
// allpass_phase_delay: 1 − 2·atan2(η·sin ω, 1 + η·cos ω) / ω
// dc_phase_delay:      −((π − ω)/2 − atan2(r·sin ω, 1 − r·cos ω)) / ω
// eta_for:             θ = ω·(1 − frac)/2;  η = sin θ / sin(ω − θ)          (frac = 1 → η = 0)
// split:               d = period − other;  n = floor(d − 0.5).max(MIN_LINE);  (n, eta_for(d − n, ω))
//                      so frac ∈ [0.5, 1.5) wherever n isn't clamped
```

- `pub const MAX_STRING_DELAY: usize = 1016`. G1's 979.6-sample period plus the blocker's 31.4-sample advance needs a 1,011-sample line.
- `KsString` fields:
  - `buffer`, `write_pos`, `dirty`, `noise_state` stay.
  - `ring_len: usize` (the wrap), `delay: usize` (the line delay) and `frac: Allpass1` replace `delay_len`.
  - `dc` is from Task 1.
  - `ens_lfo_phase` goes to Task 9's `Ensemble`.
  - Invariants: `MIN_LINE <= delay`, `delay + 2 <= ring_len <= MAX_STRING_DELAY`, `ring_len <= dirty` and `write_pos < dirty`.
- `KsString::set_period(&mut self, period: f32, other: f32, w: f32)` replaces `set_freq`. It sets `delay` and η through `split`, clamped so `delay + 2 <= MAX_STRING_DELAY`. It grows `ring_len` to `delay + 2` if needed (never shrinks it mid-note) and raises `dirty` to `ring_len`. Its callers compute `other = dc_phase_delay(self.dc.r(), w)`; Task 8 adds the dispersion term.
- The loop low-pass becomes the linear-phase 3-tap `y = c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]`, centred on the line delay, so it adds no delay. `c = lp_coeff(bright) = 0.02 + 0.48·(1 − bright)`. Until Task 3, `bright` is today's `1 − brightness`, which keeps today's tone direction.
- `ring_tap` (Bowed) reads `delay` behind the write over `ring_len`.

- [ ] **Step 1: Write the failing tests.**
  - In `loop_parts.rs`:
    - `eta_for_inverts_the_phase_delay`: for ω in {2π·49/48000, 2π·2093/48000} and frac in {0.5, 0.75, 1.0, 1.49}, `(allpass_phase_delay(eta_for(frac, w), w) - frac).abs() < 1e-4`.
    - `split_keeps_the_fraction_in_range`: `split(979.59, dc_phase_delay(r, w), w).0 == 1010` at G1, and `frac ∈ [0.5, 1.5)` for periods 22.9 to 979.6.
  - In `modal_resonator_test.rs`:
    - `strings_are_in_tune`: for STRING and for SYMP (the main string, halo bare), every note 31..=96 (G1..C7) at velocity 100, DAMP longest, BRIGHT brightest (today's `decay = 0.0`, `brightness = 0.0`). Render 3 s. `common::period_hz(&out[SR as usize / 4..])` must be within ±2 cents of `note_to_freq(n)`:

```rust
let cents = 1200.0 * (period_hz(&out[SR as usize / 4..]) / note_to_freq(n) as f64).log2();
assert!(cents.abs() < 2.0, "{mode:?} note {n}: {cents:+.2} cents");
```

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --lib modal::loop_parts && cargo test -p chimera-core --test modal_resonator_test strings_are_in_tune`. Expected: compile errors, then after stubs, whole-sample tuning fails high notes by more than 2 cents.
- [ ] **Step 3: Implement** the Interfaces.
  - `trigger` fills `delay` samples.
  - `clear` keeps the dirty-extent contract (ADR 0054): every sample at or past `dirty` reads 0.0.
  - `retune` and `SympatheticSet::tune` call `set_period`.
  - Keep the exclusive-state tests in `modal/mod.rs` green (`dirty_clear_is_bit_identical_to_the_full_clear`, `note_on_clear_is_what_the_note_on_clears`, …). They compare paths, not values.
- [ ] **Step 4: Update `modal_strings_cover_g1_and_no_lower`.** G1's period plus the blocker's advance fits: `979.59 - dc_phase_delay(r, w_g1) + 2.0 <= MAX_STRING_DELAY as f32`. F♯1 (MIDI 30) does not. Print `Instrument`'s size and what is left of D2.
- [ ] **Step 5: Run the tests to verify they pass.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test modal_resonator_test --test memory_budget_test --test sym_pool_test --test exclusive_state_test --test click_free_test`. Expected: PASS.
- [ ] **Step 6: Amend ADR 0056.** Add fractional tuning by an exact first-order allpass and exact compensation of every in-loop phase delay at f0. Add the 1,016-sample line, which supersedes in part ADR 0040's 984, with its D2 cost and the reason (the blocker's 31-sample advance at G1).
- [ ] **Step 7: Listen on the desktop.**
  - Once: `cp -r $SP/demo $SP/demo-m2`. In `$SP/demo-m2/Cargo.toml`, point `chimera-core`'s path at `$SP/wt-m2/chimera-core`. Leave `$SP/demo` on `wt-demo`.
  - Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-string && cargo run -q --release --bin demo -- modal-sympathetic`. Expected: WAVs in `$SP/demo-m2/out`. Tell the owner they are there.
- [ ] **Step 8: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 9: Commit.**

```bash
git add chimera-core/src/dsp/modal/loop_parts.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/modal_resonator_test.rs chimera-core/tests/memory_budget_test.rs docs/adr/0056-modal-resonators-share-four-macros.md
git commit -m "Strings tune to a fraction of a sample, compensated at f0"
```

---

### Task 3: The new `ModalParams`, its disk codes, and what each model reads

**Files:**
- Modify: `chimera-core/src/dsp/modal/params.rs` (whole file); `chimera-core/src/dsp/modal/mod.rs` (every `params.*` read: `:305-314`, `:394-463`, `:600-650`, `:814-971`); `chimera-core/src/storage/codes.rs:125`; `chimera-core/tests/fixtures/disk_codes_v1.txt` (append); `chimera-core/src/ui/block_registry.rs:25-54` (field renames only; Task 6 lays the pages out); `chimera-stm32/src/bench.rs`, `chimera-core/tests/{common/mod,modal,modal_integration,exclusive_state,part_page,block,sanity,click_free}_test.rs` (field renames)
- Test: `chimera-core/tests/modal_resonator_test.rs`, `params.rs` unit tests

**Interfaces:**
- Produces, in `dsp::modal`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankModes { M16, M24, M32, M48 }          // DiskCode: codes 0..=3, idents "M16".."M48"
impl BankModes { pub const fn count(self) -> usize; pub fn from_index(v: u8) -> Self; }

pub struct ModalParams {
    pub mode: ResonatorMode,
    pub structure: f32, pub bright: f32, pub damp: f32, pub pos: f32,   // home
    pub excite: f32, pub body: f32, pub ens_depth: f32, pub ens_rate: f32, pub ens_mix: f32,
    pub couple: f32, pub halo: f32, pub modes: BankModes,               // model page
}
impl ModalParams {
    pub const MODE: ParamId = ParamId(0);   pub const EXCITE: ParamId = ParamId(1);
    pub const BRIGHT: ParamId = ParamId(3); pub const POS: ParamId = ParamId(4);
    pub const BODY: ParamId = ParamId(6);   pub const ENS_DEPTH: ParamId = ParamId(9);
    pub const ENS_RATE: ParamId = ParamId(10); pub const ENS_MIX: ParamId = ParamId(11);
    pub const DAMP: ParamId = ParamId(12);  pub const STRUCTURE: ParamId = ParamId(13);
    pub const COUPLE: ParamId = ParamId(14); pub const HALO: ParamId = ParamId(15);
    pub const MODES: ParamId = ParamId(16);
}
pub static MODAL_SPECS: [ParamSpec; 13];   // MODE, STRUCTURE, BRIGHT, DAMP, POS, EXCITE, BODY,
                                           // ENS_DEPTH, ENS_RATE, ENS_MIX, COUPLE, HALO, MODES
pub const MODEL_NAMES: [&str; 4] = ["STRING", "BANK", "BOWED", "SYMP"]; // by value (ResonatorMode as u8)
/// Whether `mode` reads `id`: the one table for page cells, dimming and the audio test.
pub fn reads(mode: ResonatorMode, id: ParamId) -> bool;
/// MDL2's six cells for `mode` (§ 1's model-page table).
pub fn page_cells(mode: ResonatorMode) -> [Option<ParamId>; 6];
```

- The specs:
  - MODE: `choice(0, "MODEL", ValFmt::Names(&MODEL_NAMES), 3.0, 0.0).ident("MODE")`.
  - STRUCTURE, BRIGHT, DAMP, POS: continuous 0..1, step 1/128, labels `STRUCT BRIGHT DAMP POS`, shorts `STR BRT DMP POS`, `modulatable: false` until Task 5.
  - EXCITE, BODY: continuous. ENS_DEPTH, ENS_RATE, ENS_MIX: labels `ENS.D ENS.R ENS.M`, idents unchanged (`E.DPT E.RAT E.MIX`). COUPLE, HALO: continuous.
  - MODES: `choice(16, "MODES", ValFmt::Names(&["16", "24", "32", "48"]), 3.0, 2.0).ident("MODES")`.
- The defaults. They are today's default translated (Task 4 proves it bit for bit):
  - MODE String, STRUCTURE 0.0, BRIGHT `1.0 - 0.7` (write it as that expression: today's INIT tone, new direction), DAMP `1.0 - 0.3` (likewise), POS 0.0.
  - EXCITE 0.8, BODY 0.3, ENS_DEPTH 0.0, ENS_RATE 0.3, ENS_MIX 0.0.
  - COUPLE 0.25, HALO 0.25, MODES `M32`.
- `reads` (MODE is read by every model):

| id | BANK | STRING | SYMP | BOWED |
|---|---|---|---|---|
| STRUCTURE, BRIGHT, DAMP, POS, EXCITE | ✓ | ✓ | ✓ | |
| BODY, ENS_DEPTH, ENS_MIX | | ✓ | ✓ | |
| ENS_RATE | | ✓ | | |
| COUPLE, HALO | | | ✓ | |
| MODES | ✓ | | | |

- `page_cells`, in order, with the rest `None`: STRING `EXCITE BODY ENS_DEPTH ENS_RATE ENS_MIX`; SYMP `EXCITE COUPLE HALO BODY ENS_DEPTH ENS_MIX`; BANK `EXCITE MODES`; BOWED all `None`.
- The DSP reads the new fields.
  - **BANK:**
    - `structure` where it read `inharm`, `bright` where it read `brightness`, `damp` where it read `decay`. Bank DAMP keeps DECAY's direction.
    - `pos` and `excite`.
    - `modes.count()` replaces `num_modes`. Resolution is latched at note-on into `ModalBank::resolution`, and `compute_filters` uses `self.resolution`.
  - **STRING and the SYMP main string:**
    - `LoopGain::from_t60(t60(damp), f0)`, with `pub(super) fn t60(damp: f32) -> f32 { 0.05 * libm::powf(400.0, damp) }` (0.05 s to 20 s).
    - The low-pass is `lp_coeff(bright)`: 1 is bright, the new direction.
    - `structure` drives today's STIFF two-sample mix until Task 8.
    - `body` drives today's comb until Task 8. `ens_*` drive today's ensemble until Task 9, with SYMP's rate fixed at `SYMP_ENS_RATE = 0.3`.
  - **SYMP halo:**
    - `LoopGain::from_t60(2.0 * t60(damp), f)` and `lp_coeff(bright * 0.7)`.
    - Ratios from `structure` through today's `sympathetic_ratios` until Task 10.
    - Coupling `0.1 * couple` and level `0.6 * halo`.
  - **BOWED:** reads no `ModalParams` field. Its excitation uses the constants `BOW_VELOCITY = 0.5` and `BOW_FORCE = 0.5`, today's hidden defaults.
- `ks_excitation`, `ks_color`, `bow_velocity`, `bow_force`, `note`, `num_modes`, `inharm`, `decay`, `ks_stiffness` and `ks_feedback` leave the struct. The pluck is today's noise excitation (`excitation 0`) at colour 0.8.
- `storage::RETIRED` becomes `&[(10, 3), (10, 4), (10, 5), (1, 2), (1, 5), (1, 7), (1, 8)]`.

- [ ] **Step 1: Write the failing tests.**
  - In `params.rs`:
    - `page_cells_are_what_the_model_reads`: for each mode, every `Some(id)` in `page_cells(mode)` has `reads(mode, id)`. Every non-macro id the mode reads appears in its cells.
    - `macros_are_dimmed_only_on_bowed`: the four macros are read by BANK, STRING and SYMP, and not by BOWED.
  - In `modal_resonator_test.rs`, `live_knobs_move_dimmed_knobs_do_not`. For each mode:
    - The base is every continuous param at 0.5 and MODES at `M32`. Hold note 48 for 1 s.
    - For each spec other than MODE, render with that param at min and at max.
    - A param the mode reads changes the output measurably. A param it doesn't read changes nothing, bit for bit:

```rust
if reads(mode, s.id) {
    assert!(rms_diff(&lo, &hi) > 1e-3, "{mode:?} {}: live but inaudible", s.label);
} else {
    assert!(lo.iter().zip(&hi).all(|(a, b)| a.to_bits() == b.to_bits()), "{mode:?} {}: dimmed but heard", s.label);
}
```

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --lib modal::params && cargo test -p chimera-core --test modal_resonator_test live_knobs_move_dimmed_knobs_do_not`. Expected: compile errors (`reads`, `ModalParams::DAMP` not found).
- [ ] **Step 3: Implement** the Interfaces and rename every field use.
- [ ] **Step 4: Append the fixture lines.** Run `cargo test -p chimera-core --test disk_codes_test`. It prints each missing line ("append this line to the fixture"). Append exactly those lines: B 1 12..16, and E 1 16 0..3 `M16..M48`. The readable column of the kept B lines may change (labels); keys don't.
- [ ] **Step 5: Run the tests to verify they pass.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test modal_resonator_test --test disk_codes_test --test codec_compat_test --test block_test --test part_page_test --test modal_test --test modal_integration_test --test exclusive_state_test --test sanity_test`.
  - Expected: PASS. `v1_fixtures_equal_factory` holds: the retired ids are skipped and the defaults are their translation.
  - `modal_test`'s `test_modal_inharm_*` become `test_modal_structure_*` (the same assertions on `structure`). `part_page_test::modal_pages` reads the new cells (Task 6 rewrites it; here, update its field names only).
- [ ] **Step 6: Listen on the desktop.**
  - In `$SP/demo-m2/src/clips.rs`, rename the Modal fields to the new struct: `decay → damp` (`1 − old` on string models, as-is on the bank), `brightness → bright`, `position → pos`, `inharm → structure` (bank and SYMP), `ks_stiffness → structure` (string), `ks_body → body`, `ks_ens_* → ens_*`, `num_modes = 16 → modes = BankModes::M16`. Delete `ks_feedback`, `ks_excitation`, `ks_color` and `bow_*`.
  - Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal`. Tell the owner which files changed.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/modal/params.rs chimera-core/src/dsp/modal/mod.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/storage/codes.rs chimera-core/src/ui/block_registry.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/tests/modal_resonator_test.rs chimera-core/tests/common/mod.rs chimera-core/tests/modal_test.rs chimera-core/tests/modal_integration_test.rs chimera-core/tests/exclusive_state_test.rs chimera-core/tests/part_page_test.rs chimera-core/tests/block_test.rs chimera-core/tests/sanity_test.rs chimera-core/tests/click_free_test.rs chimera-stm32/src/bench.rs
git commit -m "Modal's shared macros: STRUCTURE, BRIGHT, DAMP, POS, and what each model reads"
```

---

### Task 4: Old patches translate at decode

**Files:**
- Modify: `chimera-core/src/storage/codes.rs:136-145` (beside `Migration`), `chimera-core/src/storage/block_codec.rs:41-107`, `chimera-core/src/storage/sound.rs:232-237`, `chimera-core/src/storage/system.rs:122`, `chimera-core/src/storage/mod.rs:15-17`, `chimera-core/src/dsp/modal/params.rs`; `chimera-core/tests/sound_codec_test.rs:267-293` (new argument)
- Test: `chimera-core/tests/codec_compat_test.rs`

**Interfaces:**
- Consumes: `RETIRED`, `ModalParams` (Task 3).
- Produces, in `storage`:

```rust
/// A block's retired values from one file, by old `ParamId`.
pub struct Retired([Option<f32>; MAX_BLOCK_PARAMS]);
impl Retired { pub fn get(&self, id: ParamId) -> Option<f32>; pub fn any(&self) -> bool; }
/// Live params several retired ones derive from together (spec § 3): run once
/// the file's live values are written, only when the file held a retired id of `block`.
pub struct Translation { pub block: u8, pub apply: fn(&Retired, &mut dyn Block) }
pub const TRANSLATIONS: &[Translation] = &[Translation { block: 1, apply: crate::dsp::modal::translate_v1 }];
pub fn decode_block(payload: &[u8], migrations: &[Migration], translations: &[Translation],
                    target: Option<&mut dyn Blocks>) -> Result<(), FileError>;
```

- `decode_block` collects a finite entry whose `(code, id)` is in `RETIRED` into `Retired` rather than skipping it. After the spec-order writes, it runs the matching `Translation` if `retired.any()`.
- `pub fn translate_v1(old: &Retired, blk: &mut dyn Block)` in `modal/params.rs` reads MODE from `blk` (already written). Each rule applies only when its source is present, through `blk.set`:
  - DAMP: `decay` on BANK (`ResonatorMode::Modal`); `1.0 - decay` on STRING, SYMP and BOWED.
  - STRUCTURE: `stiff` on STRING and BOWED; `inharm` on BANK and SYMP.
  - BRIGHT (a kept id, already written live): `1.0 - bright` on STRING, SYMP and BOWED; untouched on BANK. It runs only inside the translation, so a new-format file is never inverted.
  - FDBK is ignored.

- [ ] **Step 1: Write the failing tests** in `codec_compat_test.rs`.
  - `old_modal_patches_translate`:
    - (a) `decode(&fixture("init_modal.snd"))` is `bits_eq` to `Sound::init(EngineType::Modal)`.
    - (b) For each mode, decode a hand-built v1 Block payload: code 1, then `(0, mode code)`, `(1, 0.6)`, `(2, 0.2)`, `(3, 0.9)`, `(4, 0.4)`, `(5, 0.7)`, `(6, 0.5)`, `(7, 0.35)`, `(8, 1.0)`, `(9, 0.1)`, `(10, 0.2)`, `(11, 0.3)` as `(id u8, f32 LE)`. Decode it with `decode_block(&payload, MIGRATIONS, TRANSLATIONS, Some(&mut snap))` into `ParamSnapshot::for_engine(Modal)`. Assert:

```rust
let m = &snap.modal;
let (damp, structure) = match mode { Modal => (0.2, 0.7), String | Bowed => (1.0 - 0.2, 0.35), Sympathetic => (1.0 - 0.2, 0.7) };
assert_eq!((m.damp, m.structure), (damp, structure), "{mode:?}");
let bright = if mode == Modal { 0.9 } else { 1.0 - 0.9 };
assert_eq!((m.excite, m.bright, m.pos, m.body), (0.6, bright, 0.4, 0.5), "{mode:?}");
assert_eq!((m.ens_depth, m.ens_rate, m.ens_mix), (0.1, 0.2, 0.3));
assert_eq!((m.couple, m.halo, m.modes), (0.25, 0.25, BankModes::M32));
```

  - Review Focus 4, `an_old_fdbk_1_patch_loads_stable`: the STRING payload above with `(2, 0.0)` (DECAY 0, today's longest) and FDBK 1. Decode it, then `common::play_modal` note 36 for 30 s. It must be finite, `peak <= 1.0`, with |mean of the last second| < 1e-3 and the last second's RMS ≤ the second second's.
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --test codec_compat_test old_modal_patches_translate an_old_fdbk_1_patch_loads_stable`. Expected: compile error (`TRANSLATIONS`), then (b) fails with the defaults.
- [ ] **Step 3: Implement** the Interfaces. Add `translations_target_live_blocks` to `block_codec`'s unit tests: every `Translation::block` is a live block code, and has at least one `RETIRED` id.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test codec_compat_test --test sound_codec_test --test disk_codes_test --lib storage`. Expected: PASS.
- [ ] **Step 5: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/storage/codes.rs chimera-core/src/storage/block_codec.rs chimera-core/src/storage/sound.rs chimera-core/src/storage/system.rs chimera-core/src/storage/mod.rs chimera-core/src/dsp/modal/params.rs chimera-core/tests/codec_compat_test.rs chimera-core/tests/sound_codec_test.rs
git commit -m "Old Modal patches translate once, at decode"
```

---

### Task 5: The macros are modulatable, read every block and eased

**Files:**
- Modify: `chimera-core/src/dsp/modal/params.rs` (the four specs' `modulatable`); `chimera-core/src/dsp/modal/mod.rs:236-278` (fields, `models_are_exclusive`), `:394-463` (`note_on`), `:518-577` (`render`); `chimera-core/src/dsp/modal/string.rs:291-355` (`trigger` split); `chimera-core/src/dsp/engines.rs:243-249`; `chimera-core/src/dsp/voice.rs:162-170`; `chimera-core/tests/modulatable_test.rs:71-110`; `docs/adr/0056-*.md`
- Test: `chimera-core/tests/modal_resonator_test.rs`

**Interfaces:**
- Consumes: `ModalParams`, `reads` (Task 3).
- Produces:

```rust
/// The four macros as the loops play them: eased toward the block's
/// modulated values by EASE a block, snapped at note-on.
#[derive(Clone, Copy)]
pub(super) struct Macros { pub structure: f32, pub bright: f32, pub damp: f32, pub pos: f32 }
impl Macros { pub fn of(p: &ModalParams) -> Self; pub fn ease(&mut self, to: &Self); }
pub(super) const EASE: f32 = 0.3;
impl KsString {
    pub(super) fn excite(&mut self, (period, other, w): (f32, f32, f32), amplitude: f32); // clear, set_period, noise fill
    pub(super) fn shape(&mut self, position: f32);  // today's pluck comb and colour passes, in place
}
impl ModalEngine { pub fn playing_cost(&self) -> Option<Cost>; }   // the sounding note's model and latched MODES
impl EngineSlot { pub fn modal_playing_cost(&self) -> Option<Cost>; }
```

- `ModalEngine` gains `macros: Macros` and `shape_pending: bool`.
  - `note_on` snaps `macros` to `Macros::of(params)`, excites the string(s) and sets `shape_pending`.
  - `render` eases `macros` toward `Macros::of(params)` (the voice's modulated copy) first. If `shape_pending`, it runs `shape(macros.pos)` on STRING's or SYMP's main string before the first tick, and clears the flag.
  - So POS takes effect at the pluck, from the first block's modulated POS (VEL and NOTE routes included). With no route, the result is bit-identical to shaping at note-on.
  - The loops read `macros`, never `params`, for the four.
- `voice.rs::held_model_extra` becomes `self.slot.modal_playing_cost().map_or(Cost::ZERO, |held| Cost(held.0.saturating_sub(ModalEngine::cost(&p.modal).0)))`. That covers a MODE switch's fade, as before, and a MODES change under a sounding bank note.

- [ ] **Step 1: Write the failing tests.**
  - In `modal_resonator_test.rs`, `macros_are_routable`. For BANK, STRING and SYMP, and each macro:
    - Route `LFO1 → (Modal, macro)` at 127 through `ModDestRegistry` (as `modulatable_test::routes`), with `lfos[0].rate = 5.0`.
    - Play note 48 through `common::Rig` for 2 s. Re-pluck at 1 s, so POS is heard at a pluck.
    - Assert `rms_diff(dry, wet) > 1e-3` against amount 0, and `common::clicks(&wet).is_empty()`.
  - Review Focus 3, `modes_change_keeps_sounding_notes_and_their_bill`:
    - BANK at `M48`, note 60 held. At block 50, set `modes = M16` on the params the voice renders.
    - The next 50 blocks are bit-identical to a run with no change.
    - `voice.held_model_extra(&p16) == Cost(ModalEngine::COST_MODE.0 * 32)` while the note sounds.
    - The next note-on plays 16 modes, and `held_model_extra` is `Cost::ZERO` after it.
  - In `modulatable_test.rs`:
    - The count becomes `17 + 3 * 5 + 1 + 2 + 4` (+ the four Modal macros).
    - `render` re-plucks at block 50 when `addr.block == BlockRef::Modal`, so POS is heard at its next pluck.
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --test modal_resonator_test macros_are_routable modes_change_keeps_sounding_notes_and_their_bill && cargo test -p chimera-core --test modulatable_test`. Expected: FAIL. The registry refuses the macros, so there is no route.
- [ ] **Step 3: Implement** the Interfaces. Set `modulatable: true` on STRUCTURE, BRIGHT, DAMP and POS only. `mod_registry_test::registry_refuses_non_modulatable` keeps EXCITE refused.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test modal_resonator_test --test modulatable_test --test mod_registry_test --test cost_test --test instrument_test --test exclusive_state_test --lib modal`. Expected: PASS.
- [ ] **Step 5: Amend ADR 0056.**
  - The four macros are read every block from the voice's modulated params and eased (`EASE` 0.3 a block).
  - This partially supersedes ADR 0010's "Modal settings are read only at note-on" for these four. The model page stays note-on.
  - POS shapes the pluck from the first block's modulated value.
  - MODES latches at note-on and is billed while it sounds.
  - Mark 0010's README row "Superseded in part by 0056". Don't edit 0010's file.
- [ ] **Step 6: Listen on the desktop.** Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-bank`. Tell the owner: the bank now answers its knobs live.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/modal/params.rs chimera-core/src/dsp/modal/mod.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/engines.rs chimera-core/src/dsp/voice.rs chimera-core/tests/modal_resonator_test.rs chimera-core/tests/modulatable_test.rs docs/adr/0056-modal-resonators-share-four-macros.md docs/adr/README.md
git commit -m "Modal's four macros are mod destinations, read every block"
```

---

### Task 6: The pages: MODEL by name, MDL2 follows MODEL, SPACE, dimming

**Files:**
- Modify: `chimera-core/src/ui/block_def.rs:38-58,131-174` (`SlotBinding`, `spec`, `label`, `format`), `chimera-core/src/ui/view.rs:46-81` (`SlotCtx`), `:146-193` (`view`), `:211-223` (`dimmed`), `chimera-core/src/ui/block_registry.rs:25-54`
- Test: `chimera-core/tests/part_page_test.rs:31-52`, `chimera-core/tests/screen/mod.rs`, `chimera-core/tests/screen_golden_test.rs`, `chimera-core/src/ui/mod_grid.rs` (tests)

**Interfaces:**
- Consumes: `reads`, `page_cells`, `MODEL_NAMES` (Task 3).
- Produces:
  - `SlotBinding::ModalPanel(u8)` and `ParamSlot::modal_panel(k: u8)`.
  - `SlotCtx` gains `pub model: ResonatorMode`, read from `(Modal, MODE)`.
  - `view` resolves `ModalPanel(k)` to `page_cells(ctx.model)[k]`: a `Param` with its spec's label and format, or `View::Empty`.
  - `dimmed` gains `(BlockRef::Modal, id) => !reads(sound.params.modal.mode, id)`.
- The pages:
  - `MODAL_1` (MDL): `MODE, STRUCTURE, BRIGHT, DAMP, POS`, then `ParamSlot::param(BlockRef::Part, PartParams::SEND_REVERB).with_label("SPACE")`.
  - `MODAL_2` (MDL2): `modal_panel(0..=5)`.
  - SPACE resolves through `UiBlocks`/`part_block` (`ui/mod.rs:1042-1057`), which already serves `BlockRef::Part` on any page. So it is the Part's REV send, not a copy, and no new plumbing is needed.

- [ ] **Step 1: Write the failing tests.**
  - Rewrite `part_page_test::modal_pages`.
    - A SYMP `ParamSnapshot`: `read(&MODAL_2)` is `[excite, couple, halo, body, ens_depth, ens_mix]`. Turning slot 1 moves `couple` by 1/128.
    - On BANK: slot 1 is MODES, and slot 2 is `View::Empty` (its turn changes nothing).
    - On BOWED: every MDL2 slot is `View::Empty`.
    - MODE still steps 0..3 and clamps at SYMP.
  - `space_is_the_parts_reverb_send`, through the screen harness's `Ui`:
    - Load init Modal and turn MDL's slot F by +10. `performance.parts[0].mix.sends[2]` rises by 10/128.
    - The SENDS page's REV slot reads the same normalized value.
  - `bowed_dims_the_macros_and_their_columns`, in `mod_grid`'s tests:
    - On BOWED, `view::dimmed` is true for the four macros and false for MODE and SPACE.
    - `inert_dests` sets the bit of a `(Modal, DAMP)` column. On STRING it doesn't.
  - Add screen cases to `tests/screen/mod.rs`: `modal_home` (init Modal, MDL), `modal_mdl2_symp` (MODEL → SYMP, then MDL2) and `modal_home_bowed` (MODEL → BOWED: four dimmed cells).
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --test part_page_test --test screen_golden_test --lib ui::mod_grid`. Expected: FAIL (`modal_panel` not found; new screens have no row).
- [ ] **Step 3: Implement** the Interfaces. Any `match` on `SlotBinding` gets the new arm; there is no wildcard.
- [ ] **Step 4: Record the screen goldens.** Run `GOLDEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test`. Paste the rows for the three new cases, and for `modal_pitch` and `modal_amp` only if they changed. Open them with `SCREEN_DUMP=$SP/screens cargo test -p chimera-core --test screen_golden_test` and check each by eye: MODEL shows `STRING`/`SYMP`/`BOWED`, SPACE sits in slot F, and BOWED's four macros are dimmed.
- [ ] **Step 5: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test part_page_test --test screen_golden_test --test all_pages_walk_test --test binding_test --lib ui`. Expected: PASS.
- [ ] **Step 6: Run the green gate.** Run `just check`. Expected: exit 0. Tell the owner the pages can be tried with `just desktop`.
- [ ] **Step 7: Commit.**

```bash
git add chimera-core/src/ui/block_def.rs chimera-core/src/ui/view.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/mod_grid.rs chimera-core/tests/part_page_test.rs chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs
git commit -m "MDL shows the macros and SPACE; MDL2 follows the model"
```

---

### Task 7: The release ramp (#51) and Bowed's low notes (#206)

**Files:**
- Modify: `chimera-core/src/dsp/modal/loop_parts.rs`, `chimera-core/src/dsp/modal/mod.rs:486-512` (`note_off`), `:539-567` (`exciting`), `:847-883` (`render_bowed`), `:87-93` (`BowedString`); `chimera-core/src/dsp/modal/string.rs:405-413` (`damp` deleted)
- Test: `chimera-core/tests/modal_resonator_test.rs`

**Interfaces:**
- Produces, in `loop_parts`:

```rust
pub const RELEASE_SAMPLES: u32 = 240;
pub const RELEASE_T60: f32 = 0.12;           // seconds, a released string's ring
/// A note-off's ramp from the held gain to the released one, a sample at a time.
pub struct Release { left: u32, from: f32, to: f32 }
impl Release {
    pub fn start(&mut self, from: LoopGain, to: LoopGain);
    pub fn gain(&mut self, held: LoopGain) -> LoopGain;  // per sample
    pub fn idle(&self) -> bool;
}
```

  `gain`'s rule is not determined by the signature: `from + (to − from)·(1 − left/RELEASE_SAMPLES)` while ramping, then `held.min(to)`. The release never gives the gain back, so DAMP modulated upward can't hold a released note.
- STRING and the SYMP main string: at note-off, `Release::start(current, LoopGain::from_t60(RELEASE_T60, f0))`. Each halo string starts one with `2.0 * RELEASE_T60`. `KsString::damp` and every buffer scaling go.
- BOWED: `BowedString` gains `force_to: f32` and `written: u32`.
  - At note-off, the force ramps linearly to 0 over `RELEASE_SAMPLES`. Today's `release_decay` 0.995 applies once it reaches 0.
  - The #206 fix: `ring_tap` reads `min(delay, written.max(1))` behind the write. So a note sounds from its first samples, and the tap reaches its full period after one period.
  - BOWED's `exciting` is `force > 0.0`, so a bowed note is never freed for silence while bowing.

- [ ] **Step 1: Write the failing tests** in `modal_resonator_test.rs`.
  - `release_does_not_click`: for each mode, note 60 at velocity 127, 0.5 s held and 0.5 s released through `common::Rig`, with the default Modal Sound (no VCA route). `common::clicks(&out).is_empty()`.
  - `bowed_low_notes_sound`: BOWED, note 31 (G1), held 2 s through `Rig`. The first block's peak is > 1e-3, `rig.is_active()` after every block, and the last block's peak is > 1e-3.
  - Review Focus 1, `a_released_note_ends_with_damp_at_its_top`: STRING note 48, held 0.5 s, then released. From note-off on, render with `damp = 1.0` on the params the voice renders: the value a route at its top produces. The voice goes inactive within 2 s of note-off, and the last 0.1 s before that has peak < 1e-3.
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --test modal_resonator_test release_does_not_click bowed_low_notes_sound a_released_note_ends_with_damp_at_its_top`. Expected: STRING and BOWED click, and Bowed G1 is silent in its first block.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test modal_resonator_test --test click_free_test --test sanity_test --test modal_integration_test --test exclusive_state_test --test sym_pool_test --lib modal`. Expected: PASS.
- [ ] **Step 5: Listen on the desktop.** Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-bowed && cargo run -q --release --bin demo -- modal-string`.
- [ ] **Step 6: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 7: Commit.**

```bash
git add chimera-core/src/dsp/modal/loop_parts.rs chimera-core/src/dsp/modal/mod.rs chimera-core/src/dsp/modal/string.rs chimera-core/tests/modal_resonator_test.rs
git commit -m "Note-off ramps the loop over 5 ms; low bowed notes sound at once"
```

---

### Task 8: Dispersion from STRUCTURE, and BODY as an output body (#10)

**Files:**
- Create: `chimera-core/src/dsp/modal/dispersion.rs` (Mutable's MIT notice, as `rings.rs`), `chimera-core/src/dsp/modal/body.rs` (ours)
- Modify: `chimera-core/src/dsp/modal/string.rs` (`StringVoice`; the STIFF mix and the body comb in `tick_full` go), `chimera-core/src/dsp/modal/mod.rs` (`ModelSlot::String(StringVoice)`, `SympatheticVoice { main: StringVoice, halo }`, the size assert `:141-147`, `layout`), `chimera-core/tests/sanity_test.rs:97-101`, `chimera-core/tests/memory_budget_test.rs:175-179`, `THIRD_PARTY.md:10`, `docs/adr/0056-*.md`
- Test: `chimera-core/tests/modal_resonator_test.rs`, unit tests in both new files

**Interfaces:**
- Consumes: `Allpass1`, `allpass_phase_delay`, `split` (Task 2); `Macros` (Task 5).
- Produces:

```rust
// dispersion.rs
pub const DISPERSION_STAGES: usize = 4;
pub struct Dispersion { stages: [Allpass1; DISPERSION_STAGES] }
impl Dispersion {
    /// Rings' `ap_gain` law (string.cc), limited so the chain's DC delay is at most half the period.
    pub fn coeff(structure: f32, period: f32) -> f32;
    pub fn set(&mut self, a: f32);
    pub fn process(&mut self, x: f32) -> f32;
    pub fn phase_delay(a: f32, w: f32) -> f32;       // DISPERSION_STAGES · allpass_phase_delay(a, w)
}
// body.rs
pub const BODY_MODES: [(f32, f32, f32); 3] = [(102.0, 3.0, 1.0), (236.0, 4.0, 0.7), (517.0, 3.0, 0.5)]; // Hz, Q, gain
pub struct Body { modes: [Svf; 3] }
impl Body { pub fn tune(&mut self, sample_rate: u32); pub fn process(&mut self, x: f32, amount: f32) -> f32; }
// string.rs
pub(super) struct StringVoice { string: KsString, disp: Dispersion, body: Body, ens: Ensemble /* Task 9 */, release: Release }
```

  The coefficient law and the output mix, which the signatures leave open:

```rust
// coeff:  a = −0.618·s / (0.15 + s);  D = period / (2·STAGES);  a.max((1 − D) / (1 + D))
// Body::process:  (x + amount · Σ gᵢ · bpᵢ(x)) / (1 + 0.5 · amount)      // outside the loop
```

- The loop runs the dispersion chain on the filtered sample, before the DC blocker. Each block, when the eased STRUCTURE moved (`!= last`), the string re-splits with `other = dc_phase_delay(r, w) + Dispersion::phase_delay(a, w)`, so the fundamental stays put.
- `Svf` is `rings::Svf`; `Body::tune` calls `set(f / sr, q)` once at note-on.
- The size rule: `const _: () = assert!(size_of::<SympatheticVoice>() <= size_of::<StringVoice>() + align_of::<StringVoice>());`, replacing ADR 0054's Bowed bound. `memory_budget_test::sympathetic_pool_fits_d2` asserts `MODEL_SLOT <= max(SYMPATHETIC_VOICE, BOWED).next_multiple_of(align) + align`.

- [ ] **Step 1: Write the failing tests.**
  - `dispersion.rs`: `coeff_is_rings_law_within_the_period_limit`. `coeff(0.0, 979.0) == 0.0`. `coeff(1.0, 979.0)` equals `-0.618 / 1.15` to 1e-6. For each period in {6.0, 22.9, 979.0}, `phase_delay(coeff(1.0, p), 1e-4) <= p / 2 + 1e-3`.
  - `body.rs`: `body_gain_is_bounded`. The impulse response's peak magnitude response at amount 1 is ≤ 2.0 over 20 Hz–20 kHz.
  - `modal_resonator_test.rs`:
    - `dispersion_keeps_pitch`: STRING at notes 36, 60 and 84, BRIGHT 0, DAMP 1. `period_hz` at STRUCTURE 0 and at 1 differ by < 2 cents.
    - `body_does_not_transpose`: STRING note 48, BODY 0 vs 1. `period_hz` within 2 cents, and within 2 cents of `note_to_freq(48)`.
    - `dispersion_stretches_the_partials`: STRING note 48 at STRUCTURE 1. The 8th partial's peak (Goertzel search ±3 % around 8·f0) sits above 8·f0 by > 5 cents, and is within 1 cent of it at STRUCTURE 0.
  - In `sanity_test.rs`, remove `#[ignore]` from `modal_is_pitched` (#10).
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --lib modal::dispersion modal::body && cargo test -p chimera-core --test modal_resonator_test dispersion_keeps_pitch body_does_not_transpose dispersion_stretches_the_partials && cargo test -p chimera-core --test sanity_test modal_is_pitched`. Expected: compile errors, then the comb's octave fails `body_does_not_transpose` and `modal_is_pitched`.
- [ ] **Step 3: Implement** the Interfaces. `live_knobs_move_dimmed_knobs_do_not` must stay green: STRUCTURE and BODY stay live on STRING and SYMP.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test modal_resonator_test --test sanity_test --test memory_budget_test --test sym_pool_test`. Expected: PASS.
- [ ] **Step 5: Provenance and ADR.**
  - In `THIRD_PARTY.md`'s Modal line, list `dispersion.rs`.
  - Amend ADR 0056:
    - STRUCTURE's dispersion is a 4-stage first-order allpass chain using Rings' `ap_gain` law (MIT). Its phase delay at f0 is taken off the line, so pitch holds. There is no new buffer.
    - BODY is an output body of three fixed resonances.
    - Sympathetic is sized within one align of String, superseding in part ADR 0054's const assert.
- [ ] **Step 6: Listen on the desktop.** Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-string`.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/modal/dispersion.rs chimera-core/src/dsp/modal/body.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/modal_resonator_test.rs chimera-core/tests/sanity_test.rs chimera-core/tests/memory_budget_test.rs THIRD_PARTY.md docs/adr/0056-modal-resonators-share-four-macros.md
git commit -m "STRUCTURE stiffens the string in tune; BODY colours without transposing"
```

---

### Task 9: The ensemble, rebuilt (#50)

**Files:**
- Create: `chimera-core/src/dsp/modal/ensemble.rs` (ours)
- Modify: `chimera-core/src/dsp/modal/string.rs` (today's heads and `ENS_SPREAD` go; `StringVoice.ens`), `chimera-core/src/dsp/modal/mod.rs` (render reads it)
- Test: `chimera-core/tests/modal_resonator_test.rs`, `ensemble.rs` unit tests

**Interfaces:**
- Consumes: `StringVoice`, `KsString`'s ring (Task 2).
- Produces:

```rust
pub const ENS_HEADS: usize = 3;
pub const ENS_MAX_CENTS: f32 = 15.0;
pub fn rate_hz(ens_rate: f32) -> f32;                  // 0.1 · 60^ens_rate  (0.1–6 Hz)
pub struct Ensemble { cos: f32, sin: f32, rot_c: f32, rot_s: f32, amp: f32 }
impl Ensemble {
    pub fn set(&mut self, depth: f32, rate: f32, delay: usize, sample_rate: u32); // per block
    /// Head k's delay behind the write, this sample: within [2, delay − 2].
    pub fn head_delays(&self, delay: usize) -> [f32; ENS_HEADS];
    pub fn advance(&mut self);                          // per sample
}
impl KsString { pub(super) fn read_frac(&self, delay: f32) -> f32; } // linear interpolation
```

  The head law, which the signatures leave open:

```rust
// A quadrature oscillator: (c, s) rotated by (rot_c, rot_s) = (cos, sin)(2π·rate/fs) each sample,
// renormalised once per block. Head k's phase is k·120°: sₖ = s·cos(2πk/3) + c·sin(2πk/3).
// oₖ = delay/2 + A·sₖ,  A = min((2^(ENS_MAX_CENTS·depth/1200) − 1) · fs / (2π·rate), delay/2 − 2)
// out = dry·(1 − mix) + mix · (Σ read_frac(oₖ)) / ENS_HEADS          // on the output, not in the loop
```

  The Doppler of `oₖ` is the detune: ±15 cents at DEPTH 1 wherever `A` isn't capped by the loop. Capped at slow rates on short loops, it is less. There are no new buffers.
- SYMP's main string uses `rate_hz(SYMP_ENS_RATE)`.

- [ ] **Step 1: Write the failing tests.**
  - `ensemble.rs`, `heads_stay_inside_the_loop`: for delay ∈ {3, 23, 1010}, depth ∈ {0, 1} and rate ∈ {0, 1}, over 10⁵ `advance`s every head delay is in `[2.0, delay as f32 - 2.0]` (for delay 3: exactly 1.5, and depth has no effect).
  - `modal_resonator_test.rs`:
    - `ensemble_is_audible`: STRING note 48, DAMP 1, BRIGHT 0.7, ENS RATE 0.5 (≈ 0.77 Hz), 4 s. ENS on is DEPTH 1 and MIX 0.5; off is MIX 0.
      - Spectral spread: `sideband(on) - sideband(off) > 6.0` dB, where `sideband` is `20·log10` of the mean of the Goertzel magnitudes at `f0·2^(±10/1200)` over the fundamental's.
      - Amplitude movement: the 10 ms-window RMS envelope over seconds 1–4, detrended by a straight line in dB, has a standard deviation > 3× off's.
    - Review Focus 5, `ensemble_at_full_depth_on_g1_stays_in_the_line`: STRING note 31, DAMP 1, ENS DEPTH 1, MIX 1, at ENS RATE 0 and at 1, for 30 s. Finite, peak ≤ 1.5, `common::clicks(&out).is_empty()`.
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --lib modal::ensemble && cargo test -p chimera-core --test modal_resonator_test ensemble_is_audible ensemble_at_full_depth_on_g1_stays_in_the_line`. Expected: compile error, then today's frozen LFO fails `ensemble_is_audible`.
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test modal_resonator_test`. Expected: PASS, `live_knobs_move_dimmed_knobs_do_not` included (ENS RATE live on STRING only).
- [ ] **Step 5: Listen on the desktop.** Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-string`.
- [ ] **Step 6: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 7: Commit.**

```bash
git add chimera-core/src/dsp/modal/ensemble.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/modal_resonator_test.rs
git commit -m "A real ensemble: three heads, 0.1 to 6 Hz, up to 15 cents"
```

---

### Task 10: SYMP's chords, their glide, COUPLE and HALO

**Files:**
- Create: `chimera-core/src/dsp/modal/chords.rs` (Mutable's MIT notice, citing `rings/dsp/part.cc`, Copyright 2015 Emilie Gillet)
- Modify: `chimera-core/src/dsp/modal/mod.rs:95-107` (`SympatheticSet`), `:436-456` (SYMP note-on), `:885-957` (`render_sympathetic`), `:966-971` (`sympathetic_ratios` deleted); `THIRD_PARTY.md:10`; `docs/adr/0056-*.md`
- Test: `chimera-core/tests/modal_resonator_test.rs`, `chords.rs` unit tests

**Interfaces:**
- Consumes: `KsString::set_period`, `split`, `dc_phase_delay` (Task 2); `Release` (Task 7).
- Produces:

```rust
pub const CHORD_COUNT: usize = 11;
pub const CHORD_GLIDE_SAMPLES: u32 = 960;
/// Rings' single-voice chords (part.cc, `chords[0]`), the 0.0 the main string plays dropped.
pub static CHORDS: [[f32; 7]; CHORD_COUNT] = [
    [-12.0, 0.01, 0.02, 0.03, 11.98, 11.99, 12.0],
    [-12.0, 3.0, 3.01, 7.0, 9.99, 10.0, 19.0],
    [-12.0, 3.0, 3.01, 7.0, 11.99, 12.0, 19.0],
    [-12.0, 3.0, 3.01, 7.0, 13.99, 14.0, 19.0],
    [-12.0, 3.0, 3.01, 7.0, 16.99, 17.0, 19.0],
    [-12.0, 6.98, 6.99, 7.0, 12.0, 18.99, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 16.99, 17.0, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 13.99, 14.0, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 11.99, 12.0, 19.0],
    [-12.0, 3.99, 4.0, 7.0, 10.99, 11.0, 19.0],
    [-12.0, 4.99, 5.0, 7.0, 11.99, 12.0, 17.0],
];
pub fn chord_of(structure: f32) -> usize;               // min((structure · 11) as usize, 10)
/// `period` raised by octaves (halved) until its line fits: period − dc_phase_delay + 2 ≤ MAX_STRING_DELAY.
pub fn fold(period: f32, r: f32, sample_rate: u32) -> f32;
```

- `SympatheticSet` gains `chord: u8`, `from: [f32; 7]`, `to: [f32; 7]` and `glide: u32` (samples left). The halo's glide state lives with its strings, in the pool.
- Note-on:
  - Each halo string gets `ring_len` for the longest folded period any chord gives it at this note. That keeps a glide from ever growing a ring mid-note.
  - It tunes to `chord_of(params.structure)` at once.
- Each block:
  - `chord_of` reads the block's modulated STRUCTURE un-eased (the glide is the easing).
  - On a change, `from` = the current periods, `to` = the new folded ones, and `glide = CHORD_GLIDE_SAMPLES`.
  - While gliding, each string re-splits every block at `from + (to − from)·(1 − glide/CHORD_GLIDE_SAMPLES)`.
- The coupling is `0.1 * m.couple` and the halo level is `0.6 * m.halo`, read at note-on.

- [ ] **Step 1: Write the failing tests.**
  - `chords.rs`: `every_chord_has_seven_distinct_intervals`, where no row repeats a value.
  - `modal_resonator_test.rs`:
    - `chord_change_glides`. SYMP note 48, STRUCTURE 0.1 (chord 1), held.
      - At block 100, set STRUCTURE 0.3 (chord 3) on the rendered params.
      - `common::clicks(&out).is_empty()`.
      - From block 100 + 19 (25 ms, 1,216 samples) on, the test-support accessor `ModalEngine::halo_periods(&self, &SymPool) -> Option<[f32; 7]>` equals the folded chord-3 periods to 1e-3 samples.
    - Review Focus 2, `every_chord_fits_the_line_at_g1`: SYMP note 31, each STRUCTURE `(k as f32 + 0.5) / 11.0` for k in 0..11. For every halo string:
      - `ring_len <= MAX_STRING_DELAY` and `delay + 2 <= ring_len`, through the accessor `ModalEngine::halo_lines(&self, &SymPool) -> Option<[(usize, usize); 7]>` (delay, ring_len).
      - Its period is `note_period · 2^(−interval/12) / 2^k` for the least k that fits (−12 folds to unison at G1).
      - 2 s of audio are finite and bounded.
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --lib modal::chords && cargo test -p chimera-core --test modal_resonator_test chord_change_glides every_chord_fits_the_line_at_g1`. Expected: compile errors (`CHORDS`, `halo_periods`).
- [ ] **Step 3: Implement** the Interfaces. Exclusive state's pool, leases and no-steal rule stay untouched (ADR 0054). `SymPool::note_on_clear` stays exact, and `note_on_clear_is_what_the_note_on_clears` stays green.
- [ ] **Step 4: Run the tests to verify they pass.** Run `cargo test -p chimera-core --lib modal && cargo test -p chimera-core --test modal_resonator_test --test sym_pool_test --test exclusive_state_test --test instrument_test`. Expected: PASS.
- [ ] **Step 5: Provenance and ADR.**
  - In `THIRD_PARTY.md`, list `chords.rs`.
  - Amend ADR 0056:
    - SYMP steps Rings' chord table (MIT) adapted to 7 strings, glides 20 ms, and folds low strings by octaves to fit the line.
    - COUPLE and HALO replace 0.025 and 0.15.
    - The provenance split: from Rings, the chords and the `ap_gain` law; ours, `LoopGain`, the blocker's placement, the fractional tuning, the body, the ensemble, the release and the macro mapping.
- [ ] **Step 6: Listen on the desktop.** Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-sympathetic`.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/modal/chords.rs chimera-core/src/dsp/modal/mod.rs chimera-core/tests/modal_resonator_test.rs THIRD_PARTY.md docs/adr/0056-modal-resonators-share-four-macros.md
git commit -m "Sympathetic steps Rings' chords and glides between them"
```

---

### Task 11: Costs, bench rows, memory, and the goldens re-recorded once

**Files:**
- Modify: `chimera-core/src/dsp/modal/mod.rs:280-320` (`COST_*`, doc comments), `chimera-stm32/src/bench.rs:231-262`, `chimera-core/tests/cost_test.rs:36-54,743-765`, `chimera-core/tests/memory_budget_test.rs`, `chimera-core/tests/golden_test.rs` (rows, `PENDING`, `KNOWN_BROKEN`), `chimera-core/tests/codec_compat_test.rs:140-170`, `docs/adr/0056-*.md`

**Interfaces:**
- The provisional costs, estimated on the host until Task 12's bench. Each estimate is the count of f32 operations the new per-sample code adds, × 1.5 cycles, + 10 %:
  - `COST_STRING` 460: 390, plus 4 allpasses, the fractional allpass, the blocker, 3 body SVFs and 3 interpolated heads.
  - `COST_BOWED` 640: 620, plus the blocker and the ramp.
  - `COST_SYMPATHETIC` 950: 809, plus the main string's additions and 7 × (fractional allpass + blocker).
  - `COST_BANK` 480: 460, plus the easing. `COST_MODE` stays 45.
- New bench rows in `ROUTING`:
  - `("MDL STR+", |p| modal_full(p, ResonatorMode::String), STILL)`: STRUCTURE 1, BODY 1, ENS DEPTH 1, MIX 0.5, LFO1 → each macro at 64.
  - `("MDL SYM+", |p| modal_full(p, ResonatorMode::Sympathetic), chord_storm)`: STRUCTURE stepped across a chord every 8 blocks.
  - `("MDL RES48", |p| { modal(p, ResonatorMode::Modal); p.params.modal.modes = BankModes::M48 }, STILL)`.

- [ ] **Step 1: Write the failing tests.**
  - In `cost_test.rs`:
    - `voice_costs_are_the_bench_measurements` expects `Cost(ModalEngine::COST_STRING.0 + 10)`.
    - New `modal_costs_are_the_host_estimates`: `assert_eq!` the four values above, and that `cost` of BANK at `M48` is `480 + 45·48`.
  - In `golden_test.rs`: `PENDING` is empty. `KNOWN_BROKEN` drops its four #10 entries, and its doc comment says why (Modal 2 step A closes #10). `known_broken` accepts an empty list.
- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p chimera-core --test cost_test --test golden_test`. Expected: the cost asserts fail, and the four Modal rows mismatch.
- [ ] **Step 3: Implement.**
  - Set the `COST_*`. Each doc comment says "host estimate, provisional until the bench row (Task 12)", with its arithmetic.
  - Add the bench rows and their builders `modal_full` and `chord_storm`.
- [ ] **Step 4: Re-record the Modal goldens, once.**
  - Run `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test goldens_match -- --nocapture`. Paste only the four Modal rows, each with the comment "Re-recorded: Modal 2 step A's resonators (spec § Tests)".
  - Compute `init_modal.snd`'s render hash with `fnv1a(render_sound(...))`, printed by a temporary `eprintln!`. Paste it into `FIXTURE_RENDERS` with the same comment, and remove the skip.
  - No non-Modal row may change.
- [ ] **Step 5: Run the tests to verify they pass.** Run `cargo test -p chimera-core --test golden_test --test codec_compat_test --test cost_test --test memory_budget_test -- --nocapture`. Expected: PASS. `instrument_fits_d2` and `sympathetic_pool_fits_d2` print the sizes; put them in the ADR.
- [ ] **Step 6: Amend ADR 0056** with the costs and host sizes (`Voice`, `ModelSlot`, `SymPool`, `Instrument`, D2 left). Note that SYMP bills 5 voices on rev V at 950. Until the bench, name 883 as the most that keeps 6.
- [ ] **Step 7: Run the green gate.** Run `just check`. Expected: exit 0.
- [ ] **Step 8: Commit.**

```bash
git add chimera-core/src/dsp/modal/mod.rs chimera-stm32/src/bench.rs chimera-core/tests/cost_test.rs chimera-core/tests/memory_budget_test.rs chimera-core/tests/golden_test.rs chimera-core/tests/codec_compat_test.rs docs/adr/0056-modal-resonators-share-four-macros.md
git commit -m "Modal's costs re-estimated, bench rows added, goldens re-recorded once"
```

---

### Task 13: The EXC node: each model's exciter and a playable Bow

The owner's decision of 2026-09-30 (spec § 1, § 2 BOWED and § 3, each marked "amended 2026-09-30, owner"). On the bench, Bowed's values didn't move and soft keys made no sound on Bowed. So the exciters get their own node, first in the chain, and Bowed's macros go live. It runs after Task 11 and before Task 12. Task 12's ship flash covers it.

**Files:**
- Modify:
  - `chimera-core/src/dsp/modal/params.rs`: the fields, specs, `reads`, `page_cells`, `ModalPage`, `EXCITER_NAMES`, `bow_force`, `damp_for` made public, and `translate_v1`.
  - `chimera-core/src/dsp/modal/mod.rs`: `BowedString`, the bank's note-on, Bowed's note-on and note-off, `render_bowed`, `COST_BOWED`, `pub use loop_parts::RELEASE_T60`, and the unit tests.
  - `chimera-core/src/dsp/modal/string.rs`: `color_passes`, `KsString::shape`, `StringVoice::pluck` and its `color`, `ring_tap_at`, `ring_tap_lp`.
  - `chimera-core/src/ui/block_def.rs` (`SlotBinding::ModalPanel`, `ParamSlot::modal_panel`), `chimera-core/src/ui/view.rs` (`view`), `chimera-core/src/ui/block_registry.rs` (`MODAL_EXC`, `MODAL_2`, `MODAL_PLUCK_BLOCKS`), `chimera-core/src/ui/components.rs` (`header_text`), `chimera-core/src/ui/renderer.rs:421`, `chimera-core/src/ui/mod_grid.rs` (tests).
  - `chimera-core/tests/fixtures/disk_codes_v1.txt` (append only).
  - `chimera-stm32/src/bench.rs`: `MDL BOW+` and the MEMORY screen's `PLUCK DARK`.
  - `docs/adr/0056-modal-resonators-share-four-macros.md`.
- Test: `chimera-core/tests/{modal_resonator_test,codec_compat_test,part_page_test,block_def_tests,header_map_test,binding_test,cost_test,screen_golden_test}.rs`, `chimera-core/tests/screen/mod.rs`, and the unit tests in `params.rs`, `string.rs`, `mod.rs` and `mod_grid.rs`.

**Interfaces:**
- Consumes: `ModalParams`, `reads`, `translate_v1` (Tasks 3 and 4); `Macros` (Task 5); `SlotBinding::ModalPanel` (Task 6); `Release`, `RELEASE_SAMPLES`, `RELEASE_T60` and `KsString::ring_tap` (Task 7); the cost method in ADR 0056's Costs section (Task 11).
- Produces, in `dsp::modal` (`params.rs`):

```rust
pub struct ModalParams {
    // … Task 3's fields, then EXC's (note-on, not modulatable):
    pub color: f32,   // PLUCK: the noise's smoothing, 1 brightest
    pub burst: f32,   // STRIKE: the burst's length
    pub force: f32,   // BOW: pressure
    pub speed: f32,   // BOW: velocity
}
impl ModalParams {
    pub const FORCE: ParamId = ParamId(17); pub const SPEED: ParamId = ParamId(18);
    pub const COLOR: ParamId = ParamId(19); pub const BURST: ParamId = ParamId(20);
}
pub static MODAL_SPECS: [ParamSpec; 17];   // Task 3's 13, then COLOR, BURST, FORCE, SPEED, appended
/// EXC's header by MODEL, by `ResonatorMode as u8`.
pub const EXCITER_NAMES: [&str; 4] = ["PLUCK", "STRIKE", "BOW", "PLUCK"];
/// Which of MODEL's two pages a cell list is for: EXC or the model page (MDL2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalPage { Exciter, Model }
/// `page`'s six cells for `mode`. Replaces `page_cells(mode)`.
pub fn page_cells(page: ModalPage, mode: ResonatorMode) -> [Option<ParamId>; 6];
/// The bow's force at a note-on: FORCE × (0.5 + 0.5 × velocity), velocity in 0..=1.
pub fn bow_force(force: f32, vel: f32) -> f32;
pub fn damp_for(t60_s: f32) -> f32;        // Task 3's, now public: the tests name Bowed's v1 DAMP with it
```

- The specs, appended after MODES, so every earlier spec keeps its place:
  - `unit(19, "COLOR", 0.8).ident("COLOR")`
  - `unit(20, "BURST", 0.8).ident("BURST")`
  - `unit(17, "FORCE", 0.5).ident("FORCE")`
  - `unit(18, "SPEED", 0.5).ident("SPEED")`
  - Defaults: COLOR 0.8 and BURST 0.8 (today's EXCITE default, so INIT's strike keeps its length); FORCE 0.5 and SPEED 0.5, the old hidden `BOW_FORCE` and `BOW_VELOCITY`. None is `modulatable`.
- `reads` (MODE is read by every model):

| id | BANK | STRING | SYMP | BOWED |
|---|---|---|---|---|
| STRUCTURE | ✓ | ✓ | ✓ | |
| BRIGHT, DAMP, POS | ✓ | ✓ | ✓ | ✓ |
| EXCITE | ✓ | ✓ | ✓ | |
| COLOR | | ✓ | ✓ | |
| BURST | ✓ | | | |
| FORCE, SPEED | | | | ✓ |
| BODY, ENS_DEPTH, ENS_MIX, ENS_RATE, COUPLE, HALO, MODES | as Task 3 | | | |

- `page_cells`, in order, with the rest `None`:
  - `Exciter`: STRING and SYMP `EXCITE COLOR`; BANK `EXCITE BURST`; BOWED `FORCE SPEED`.
  - `Model`: STRING `BODY ENS_DEPTH ENS_RATE ENS_MIX`; SYMP `COUPLE HALO BODY ENS_DEPTH ENS_MIX`; BANK `MODES`; BOWED all `None`.
- `translate_v1` gains two rules, each applied inside the translation only:
  - BANK: `BURST = EXCITE`, the file's value, already written. The old burst was `2 + 4·EXCITE` ms.
  - BOWED: `DAMP = damp_for(RELEASE_T60)`, `BRIGHT = 1.0` and `POS = 0.0`. These run after the DECAY and BRIGHT rules and override them, because v1 Bowed never read them.
- In `string.rs`:

```rust
/// The pluck's smoothing passes at COLOR: the old `ks_color` law.
pub(super) fn color_passes(color: f32) -> usize;       // ((1.0 - color) * 7.0) as usize, at most 7
impl KsString {
    pub(super) fn shape(&mut self, position: f32, period: f32, passes: usize); // was one pass, fixed
    /// Bowed's bow point: `back` samples behind the write, in `ring_tap`'s
    /// measure, linearly interpolated, clamped to `[1, delay]` and held
    /// within what `written` has reached (#206's rule).
    /// `ring_tap_at(w, delay as f32) == ring_tap(w)`, bit for bit.
    pub(super) fn ring_tap_at(&self, written: u32, back: f32) -> f32;
    /// Bowed's loop tap through the linear-phase 3-tap low-pass centred on it:
    /// c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]. At c = 0, or before `written`
    /// passes `delay + 1`, it is `ring_tap(written)` exactly.
    pub(super) fn ring_tap_lp(&self, written: u32, c: f32) -> f32;
}
// StringVoice gains `passes: u8`, latched by `pluck` from `color_passes(params.color)`;
// `StringVoice::shape(position)` passes it on. `pluck` takes it as a new last argument.
```

- In `mod.rs`:

```rust
struct BowedString {
    string: KsString, force: f32, force_to: f32,
    lift: f32,        // force shed a sample at note-off: force / RELEASE_SAMPLES
    bow_vel: f32,     // SPEED × BOW_SPEED, latched at note-on
    written: u32, release: Release,
}
/// SPEED 1's bow velocity; SPEED 0.5 is the old `BOW_VELOCITY · 0.3`.
const BOW_SPEED: f32 = 0.3;
/// BRIGHT 0's low-pass side taps on the bowed loop: gentle, |H| ≤ 1.
const BOW_LP: f32 = 0.25;
fn render_bowed(b: &mut BowedString, output: &mut [f32; BLOCK_SIZE], m: &Macros, f0: f32);
```

  - `BOW_VELOCITY`, `BOW_FORCE` and `BOW_LIFT` go. `field_list!` for `BowedString` and `StringVoice` gains the new fields.
  - Note-on: `b.force = bow_force(params.force, vel)` and `b.bow_vel = params.speed * BOW_SPEED`. At FORCE 0.5, SPEED 0.5 and velocity 127 both equal the old values bit for bit: `0.5 * (0.5 + 0.5 * 1.0) == 1.0 * 0.5`, and `0.5 * 0.3` is the old product.
  - Note-off: `b.lift = b.force / RELEASE_SAMPLES as f32`, `b.force_to = 0.0`, and `b.release.start(BOW_GAIN, LoopGain::from_t60(t60(self.macros.damp), f0))`.
  - `render_bowed`, per sample:
    - The bow lifts by `b.lift` toward `force_to`.
    - The loop reads `x = b.string.ring_tap_lp(b.written, BOW_LP * (1.0 - m.bright))`.
    - The bow reads `v = x` when `m.pos <= 0.03`. Otherwise it reads `v = 0.5 * (x + b.string.ring_tap_at(b.written, d - m.pos * d))`, with `d` the line delay: the pluck's comb law and threshold (`KsString::shape`).
    - `friction = 4·force · tanh(8·(bow_vel − v))`.
    - The loop's gain is `b.release.gain(LoopGain::from_t60(t60(m.damp), f0))` after note-off and `BOW_GAIN` before it. A DAMP routed upward never lengthens a lifted bow's ring (`Release::gain` takes the minimum).
    - The output is `x`, as today. The `from_t60` runs once a block, not per sample.
  - Stability holds by construction: the loop multiplies `x` by a `LoopGain` below 1; the low-pass's `|H| = (1 − c) + c·cos ω ≤ 1` for `c ≤ 0.5`; the friction is bounded; `tanh` bounds the push; the output blocker stays.
  - BANK's note-on: `burst_ms = 2.0 + params.burst * 4.0`; `burst_amp = vel * params.excite` is unchanged.
- In the UI:
  - `SlotBinding::ModalPanel(ModalPage, u8)` and `ParamSlot::modal_panel(page: ModalPage, k: u8)`. `view` resolves it to `page_cells(page, ctx.model)[k]`.
  - The page and the chain:

```rust
/// EXC: the model's exciter; its cells follow MODEL.
pub static MODAL_EXC: BlockDef = BlockDef { id: 67, name: "Exciter", short: "EXC",
    layout: PageLayout::CellGrid, viz: VizType::None,
    params: [ParamSlot::modal_panel(ModalPage::Exciter, 0), /* … 1..=5 */] };
static MODAL_PLUCK_BLOCKS: [ChainBlock; 5] = [
    ChainBlock::page(&MODAL_EXC),
    ChainBlock::with_subs(&MODAL_1, &MODAL_SUB_PAGES),
    ChainBlock::with_subs(&FILTER, &FILTER_SUB_PAGES),
    ChainBlock::page(&FOLDER),
    ChainBlock { def: &MOD_MATRIX, sub_pages: &MOD_SUB_PAGES, map: Some("MOD") },
];
```

  - `MODAL_2` is `modal_panel(ModalPage::Model, 0..=5)`. The map reads `EXC · RES · FLT · AMP · MOD`, and a Part's Modal chain opens on EXC.
  - `components::header_text(nav, def, model: ResonatorMode)` names `MODAL_EXC` `EXCITER_NAMES[model as usize]` and every other page as before. `renderer.rs:421` passes `f.ctx.model`.

- [ ] **Step 1: Pin today's Bowed.** Before any other change, add `a_v1_bowed_patch_bows_as_before` to `codec_compat_test.rs`:
  - Decode `v1_modal(ResonatorMode::Bowed, 0.2)`, then `play_modal_at(&snap.modal, 48, 127, SR as usize / BLOCK_SIZE, 0)`: one second held, no release.
  - Assert `fnv1a(&out) == BOWED_V1_HELD`, a `const` recorded on today's code.
  - Run `cargo test -p chimera-core --test codec_compat_test -- a_v1_bowed_patch_bows_as_before --nocapture` with a placeholder and a temporary `eprintln!` of the hash. Paste the hash and remove the print.
  - Expected: PASS on today's code. It must still pass after Step 4: it is the compatibility pin.
- [ ] **Step 2: Write the failing tests.**
  - `params.rs`:
    - `page_cells_are_what_the_model_reads` (rewrite): for each mode and both pages, every `Some(id)` is read by the mode. Every non-home id the mode reads appears on exactly one of the two pages.
    - `the_exciter_page_holds_each_models_exciter`: the `Exciter` cells are exactly the lists in Interfaces, and no `Model` cell is EXCITE, COLOR, BURST, FORCE or SPEED.
    - `bowed_reads_three_macros` replaces `macros_are_dimmed_only_on_bowed`. BRIGHT, DAMP and POS are read by all four models; STRUCTURE by all but BOWED.
    - `bow_force_scales_with_velocity`: `bow_force(0.5, 1.0).to_bits() == 0.5f32.to_bits()`, `bow_force(0.5, 0.0) == 0.25`, `bow_force(0.0, 1.0) == 0.0`, and `bow_force(1.0, 20.0 / 127.0) > 0.5`.
  - `string.rs`:
    - `color_passes_are_the_old_law`: `color_passes(0.8) == 1` (the old hidden value), `color_passes(0.0) == 7` and `color_passes(1.0) == 0`. It does not increase from 0 to 1 in steps of 1/128.
    - `ring_tap_at_the_delay_is_ring_tap`: on a bowed ring after 2,000 pushes and after 3, `ring_tap_at(w, delay as f32)` and `ring_tap_lp(w, 0.0)` equal `ring_tap(w)` bit for bit, and `ring_tap_at(w, 0.0)` reads at 1.
  - `mod.rs`: `bank_burst_is_2_to_6_ms`. A BANK note-on at BURST 0 leaves `burst_remaining == 96`, and at BURST 1 `288`, at EXCITE 0.2 and at 1.0 alike.
  - `modal_resonator_test.rs`:
    - `a_soft_bowed_note_sounds`: BOWED at the defaults, note 48 at velocity 20, `play_modal_at` for 2 s held. Seconds 1–2 have `rms > 1e-2`, and the last block has `peak > 1e-3`. If this passes on today's code, the test doesn't reproduce the bench: stop and report.
    - `bowed_damp_is_the_ring_after_the_lift`: BOWED note 48, velocity 100, 1 s held and 2 s released, at DAMP 0.3 (T60 0.30 s) and 0.6 (1.82 s).
      - The held second is bit-identical between the two.
      - The fall from `db_at(out, 1.05)` to `db_at(out, 1.05 + t60(d) / 2)` is 30 ± 6 dB.
    - `bowed_pos_and_bright_keep_pitch`: BOWED note 48 at POS {0, 0.3, 0.7} × BRIGHT {0, 1}. `fundamental_hz` over seconds 0.5–1.5 is within 2 cents of the POS 0, BRIGHT 1 render's. Each render differs from that one by `rms_diff > 1e-3`, except POS 0 at BRIGHT 1 itself.
    - `live_knobs_move_dimmed_knobs_do_not`: play `play_modal(&p, 48, blocks, blocks / 2)`, 1 s held and 0.5 s released, so a lifted bow's DAMP is heard. It already iterates `MODAL_SPECS`, so the four new specs join without further edits.
    - `macros_are_routable`: BOWED joins for BRIGHT and POS, as the others do. DAMP on BOWED is routed through a release: 1 s held, then 1 s after note-off. Assert `rms_diff > 1e-3` and `clicks(&wet).is_empty()`.
    - `a_released_bowed_c2_is_silent_within_half_a_second`: set `damp: damp_for(RELEASE_T60)`, the v1 Bowed value. At INIT's DAMP a lifted bow now rings about 14 s.
  - `codec_compat_test.rs`, `old_modal_patches_translate`:
    - For every mode, `(m.color, m.force, m.speed) == (0.8, 0.5, 0.5)`.
    - `m.burst` is 0.6 (the file's EXCITE) on BANK and 0.8 elsewhere.
    - On BOWED, `(m.damp, m.bright, m.pos) == (damp_for(RELEASE_T60), 1.0, 0.0)`. The other modes keep Task 4's expectations.
  - UI:
    - `block_def_tests::modal_pluck_chain`: `blocks[0]` is `"Exciter"` with `sub_page_count() == 0`, and `blocks[1]` is `"Modal"` with 3. The map labels, `block.map.unwrap_or(block.def.short)`, are `["EXC", "RES", "FLT", "AMP", "MOD"]`.
    - `part_page_test::modal_pages` (rewrite): on SYMP, `read(&reg::MODAL_2)` is `[couple, halo, body, ens_depth, ens_mix, 0.0]`, and turning slot 0 moves `couple` by 1/128. On BANK, slot 0 is MODES and slot 1 is `View::Empty`. The MODE assertions stay on `MODAL_1`.
    - `part_page_test::exciter_page_follows_the_model`:
      - STRING reads `[excite, color, 0.0, 0.0, 0.0, 0.0]` on `MODAL_EXC`.
      - On BANK, slot 1 is BURST.
      - On BOWED, slots 0 and 1 are FORCE and SPEED; turning slot 0 by +1 moves `force` by 1/128, and slot 2 is `View::Empty`.
    - `part_page_test::space_is_the_parts_reverb_send`: after `load_init` the page is `MODAL_EXC`; press Plus once, assert `MODAL_1`, then turn F as before.
    - `mod_grid::bowed_dims_structure_and_its_column` replaces `bowed_dims_the_macros_and_their_columns`:
      - On BOWED, `dimmed` is true for STRUCTURE and false for BRIGHT, DAMP, POS, MODE and SPACE.
      - With `(Modal, STRUCTURE)` and `(Modal, DAMP)` registered after CUTOFF, `inert_dests` is `0b10` on BOWED and 0 on STRING.
    - `header_map_test::the_exciter_page_is_named_after_the_exciter`: for each mode, `header_text`'s name for `MODAL_EXC` is `EXCITER_NAMES[mode as usize]`, and the map node's label is `EXC`. `a_model_change_redraws_the_map` presses Plus to RES before turning MODEL.
    - `binding_test`: `MODAL_EXC` joins the panel pages next to `MODAL_2`.
    - `tests/screen/mod.rs`:
      - Every Modal case reaches its page from EXC: `modal_home`, `modal_mdl2_symp` and `modal_home_bowed` call `plus(ui, 1)` first, and `modal_amp` is `plus(ui, 3)`.
      - `to_pitch` finds the node whose `sub_pages` hold `PITCH`, not `blocks[0]`.
      - New cases: `modal_exc` (init: PLUCK, EXCITE and COLOR), `modal_exc_bank` (RES, MODEL → BANK, Minus: STRIKE, EXCITE and BURST) and `modal_exc_bowed` (MODEL → BOWED: BOW, FORCE and SPEED).
      - `modal_home_bowed` now shows only STRUCT dimmed.
- [ ] **Step 3: Run them to verify they fail.**
  - `cargo test -p chimera-core --lib -- modal::params modal::string modal::tests::bank_burst_is_2_to_6_ms ui::mod_grid`
  - `cargo test -p chimera-core --test modal_resonator_test -- a_soft_bowed_note_sounds bowed_damp_is_the_ring_after_the_lift bowed_pos_and_bright_keep_pitch live_knobs_move_dimmed_knobs_do_not macros_are_routable`
  - `cargo test -p chimera-core --test codec_compat_test --test part_page_test --test block_def_tests --test header_map_test --test screen_golden_test`
  - Expected: compile errors first (`ModalPage`, `MODAL_EXC`, `ModalParams::FORCE`). With stubs:
    - `a_soft_bowed_note_sounds` fails: today's velocity-20 bow is silent.
    - Bowed's DAMP, BRIGHT and POS fail as "live but inaudible".
    - The chain and page tests fail on the old layout.
    - `a_v1_bowed_patch_bows_as_before` passes.
- [ ] **Step 4: Implement** the Interfaces. Any `match` on `SlotBinding` gets the new arm shape; there is no wildcard. Exclusive state is untouched: Bowed's ring, its dirty extent and `bowed_clears_the_ring_it_wrote` stay as they are.
- [ ] **Step 5: Append the fixture lines.** Run `cargo test -p chimera-core --test disk_codes_test`. It prints each missing line. Append exactly `B 1 17 FORCE`, `B 1 18 SPEED`, `B 1 19 COLOR` and `B 1 20 BURST` with their readable columns. No earlier line changes.
- [ ] **Step 6: Run the tests to verify they pass.**
  - `cargo test -p chimera-core --lib -- modal ui`
  - `cargo test -p chimera-core --test modal_resonator_test --test codec_compat_test --test disk_codes_test --test golden_test --test modulatable_test --test mod_registry_test --test part_page_test --test block_def_tests --test header_map_test --test binding_test --test all_pages_walk_test --test exclusive_state_test --test sanity_test`
  - Expected: PASS. No audio golden moves:
    - STRING, SYMP and BANK at INIT pluck and strike as before (COLOR 0.8 is one pass; BURST 0.8 is EXCITE 0.8's length).
    - `init_modal.snd` renders the same, and `a_v1_bowed_patch_bows_as_before` holds.
    - If any `golden_test` or `FIXTURE_RENDERS` row fails, stop and investigate. Don't re-record it.
- [ ] **Step 7: Re-record the screen goldens, deliberately.**
  - Run `GOLDEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test`. Paste the rows for the Modal cases only: every existing `modal_*` case (the map gains its EXC node) and the three new ones. Comment each "Re-recorded: the EXC node (plan Task 13)".
  - No Algo, Mixer or System row may change.
  - Open them with `SCREEN_DUMP=$SP/screens cargo test -p chimera-core --test screen_golden_test`, and check each by eye:
    - The map reads EXC · RES · FLT · AMP · MOD.
    - EXC's header reads PLUCK, STRIKE or BOW, with its two cells.
    - MDL2 has no EXCITE.
    - BOWED's RES dims STRUCT alone.
  - Then run `cargo test -p chimera-core --test screen_golden_test`. Expected: PASS.
- [ ] **Step 8: Re-bill Bowed and add the bench rows.**
  - Re-count `ModalEngine::render`'s bowed loop by ADR 0056's method (`llvm-objdump -d --mcpu=cortex-m7` of the `just firmware` build, instructions a sample × 1.46 + 10 %, rounded up to 10). The expectation is about +25 cycles: the second tap and its lerp, the low-pass's two taps, and a `powf` a block.
  - Set `COST_BOWED` with its arithmetic in the doc comment.
  - Update `cost_test::modal_bills_each_model`'s BOWED row (its bill and its voice counts), deliberately. Then run `cargo test -p chimera-core --test cost_test`. Expected: PASS.
  - In `bench.rs`:
    - Add `("MDL BOW+", bow_full, STILL)`. `bow_full` is BOWED at FORCE 1, SPEED 1, POS 0.5 and BRIGHT 0, with LFO 1 (10 Hz) into BRIGHT, DAMP and POS at 64.
    - Add a MEMORY-screen line `PLUCK DARK {n} CYC`, measured as `SYM NOTE-ON LOW` is: a STRING note-on at G1 and COLOR 0, and its first block's seven smoothing passes over the line.
  - Run `just firmware`. Expected: exit 0.
- [ ] **Step 9: Amend ADR 0056** (Proposed, so it may be edited):
  - **Macros:**
    - The Modal chain is EXC · RES · FLT · AMP · MOD, and a Part opens on EXC.
    - EXC's cells follow MODEL through `page_cells(ModalPage::Exciter, _)`, and its header is named for the exciter (PLUCK, STRIKE, BOW).
    - EXCITE leaves the model page.
    - Replace "BOWED dims all four macros" with "BOWED dims STRUCTURE".
    - COLOR, BURST, FORCE and SPEED are read at note-on and are not modulatable.
  - **A new "Bowed" section:**
    - The force is FORCE × (0.5 + 0.5 × velocity); SPEED × 0.3 is the bow's velocity.
    - DAMP is the ring after the lift, which never gives the gain back.
    - BRIGHT is the 3-tap loop low-pass at `0.25·(1 − BRIGHT)`, with no delay.
    - POS is the bow's two-tap comb read, with the loop and output on the one tap.
    - Stability holds by construction.
  - **Release:** a lifted bow ramps to DAMP's T60, not `RELEASE_T60`.
  - **Old patches:** BURST = EXCITE on BANK. On Bowed, DAMP = `damp_for(RELEASE_T60)`, BRIGHT 1 and POS 0. COLOR, FORCE and SPEED take their defaults, the old hidden values.
  - **Costs:** the new `COST_BOWED` row and its count. **Memory:** the new host sizes, since `StringVoice` and `BowedString` grew.
  - **Bench rows:** add MDL BOW+ and PLUCK DARK.
  - **Sources:** add "Task 13" to the plan's range, and the owner's decision of 2026-09-30.
- [ ] **Step 10: Listen on the desktop**, in `$SP/demo-m2` only. Leave `$SP/demo` alone.
  - In `$SP/demo-m2/src/clips.rs`, add `modal_bowed_soft()` to the clip list: file `modal-bowed-soft`. It plays one D3 line at velocities 20, 40, 60, 90 and 127, then again at POS 0.3, BRIGHT 0.2 and DAMP 0.6, so the soft bow, the position, the tone and the ring after the lift are each heard.
  - `modal_bowed()` keeps its settings. Its BRIGHT 0.4 is now heard, deliberately.
  - Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-bowed && cargo run -q --release --bin demo -- modal-string && cargo run -q --release --bin demo -- modal-bank`. Expected: WAVs in `$SP/demo-m2/out`. The string and bank clips sound as before, since their defaults are today's.
  - Tell the owner which files changed, and that `just desktop` shows the EXC node.
- [ ] **Step 11: Run the green gate.** Run `just test`, `just clippy` and `just check`. Expected: each exits 0.
- [ ] **Step 12: Commit.**

```bash
git add chimera-core/src/dsp/modal/params.rs chimera-core/src/dsp/modal/mod.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/ui/block_def.rs chimera-core/src/ui/view.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/components.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/mod_grid.rs chimera-core/tests/fixtures/disk_codes_v1.txt chimera-core/tests/modal_resonator_test.rs chimera-core/tests/codec_compat_test.rs chimera-core/tests/part_page_test.rs chimera-core/tests/block_def_tests.rs chimera-core/tests/header_map_test.rs chimera-core/tests/binding_test.rs chimera-core/tests/cost_test.rs chimera-core/tests/screen_golden_test.rs chimera-core/tests/screen/mod.rs chimera-stm32/src/bench.rs docs/adr/0056-modal-resonators-share-four-macros.md
git commit -m "Exciters get their own node, and soft keys bow"
```

---

### Task 14: Bowed as a two-delay bowed string (#240)

The owner's decision of 2026-09-30 (spec § 1 Bowed row, § 2 BOWED, § 3 and the tests, each marked "amended 2026-09-30, owner: #240"). The one-loop bow inverts its wave every pass, so it plays an octave low (C3 at 65 Hz); BRIGHT moves it under 0.5 dB; and POS became an output comb because a second tap moved the pitch. Bowed becomes Smith's two-delay bowed string. It runs after Task 13 and before Task 12, whose ship flash covers it. The rulings are under "Spec ambiguities ruled here", Task 14. It supersedes Task 13's three parked Bowed DSP findings (`.superpowers/sdd/2026-09-29-modal-2-resonators/task-13-review.md`, Important 1–3).

**Files:**
- Create: `chimera-core/src/dsp/modal/bow.rs` (ours, citing Smith).
- Modify:
  - `chimera-core/src/dsp/modal/mod.rs`: `mod bow`, `BowedString` and its `init_in_place` and `field_list!` moved to `bow.rs`, Bowed's note-on, note-off, the render arm and `tune` arm, `COST_BOWED` and its doc comment; `render_bowed`, `BOW_LP`, `BOW_POS_MIN` deleted.
  - `chimera-core/src/dsp/modal/string.rs`: `KsString::guide` added; `ring_tap`, `ring_tap_at`, `ring_tap_lp`, `ring_push` and the unit test `ring_tap_at_the_delay_is_ring_tap` deleted.
  - `chimera-core/src/dsp/modal/params.rs`: `translate_v1`'s BOWED rule, the `reads`/`MODAL_SPECS` doc comments that describe Bowed's POS and BRIGHT.
  - `chimera-stm32/src/bench.rs`: `bow_full`'s doc comment.
  - `docs/adr/0056-modal-resonators-share-four-macros.md`.
- Test: `chimera-core/tests/{modal_resonator_test,codec_compat_test,cost_test}.rs`, `chimera-core/tests/common/mod.rs` (`step`, moved from `force_and_speed_move_a_held_bow`), and the unit tests in `bow.rs`.
- Unchanged, and run: `pitch_test`, `exclusive_state_test`, `sym_pool_test`, `in_place_test`, `memory_budget_test`, `modal_integration_test`, `golden_test`.

**What Task 13's output-comb POS leaves behind, deleted:**
- Code: `render_bowed`'s comb (`0.5·(x + ring_tap_at(…))`), `BOW_POS_MIN`, `BOW_LP` and its "tuned to a test floor" doc, `KsString::ring_tap_at`, `ring_tap_lp`, `ring_tap` and `ring_push` (Bowed's only callers), `BowedString::written` (the ring-tap reach rule; the new output sounds from the first sample), and both `tanhf`s (the friction's and the push's).
- Tests: `string::tests::ring_tap_at_the_delay_is_ring_tap`; `bowed_pos_and_bright_keep_pitch` (its octave-low search centre, `note_to_freq(48) / 2`, goes with it), replaced by `bowed_pos_moves_the_tone_not_the_pitch` and `bowed_bright_is_heard`; `a_v1_bowed_patch_bows_as_before`, renamed and re-recorded as `a_v1_bowed_patch_bows_in_tune`.
- Docs: ADR 0056's Bowed bullets on the comb and the three-tap loop low-pass, and its "Open for the owner" bullet on the octave (both rewritten in Step 10).

**Interfaces:**
- Consumes: `KsString`, `set_period`, `split`, `eta_for`, `Allpass1`, `MIN_LINE` (Tasks 1–2); `LoopGain`, `Release`, `RELEASE_SAMPLES` (Tasks 1, 7); `Macros`, `EASE` (Task 5); `bow_force`, `BOW_SPEED`, `BOW_EASE`, `BOW_GAIN`, `damp_for`, `t60` (Task 13); the spans pattern of `StringVoice::run` (Task 11b); ADR 0056's cost method.
- Produces, in `dsp::modal::bow` (all `pub(super)`):

```rust
//! Bowed: two delay lines, bow to bridge and bow to nut, meeting at the
//! bow. After J. O. Smith's digital-waveguide bowed string (Physical Audio
//! Signal Processing, CCRMA, "Bowed Strings"). Chimera's own code: no STK
//! code, constant or table.

/// The bridge filter's delay, samples: its centre tap. Off the line.
pub const BRIDGE_DELAY: f32 = 1.0;
/// POS 0's and POS 1's bow position, as a fraction of the string from the bridge.
pub const BETA_MIN: f32 = 0.06;
pub const BETA_MAX: f32 = 0.5;
/// The friction curve's half-width at force 1 (`width4`).
pub const BOW_WIDTH: f32 = 0.3;
/// BRIGHT 0's bridge-filter side taps: the most `|H| ≤ 1` allows.
pub const BOW_BRIGHT: f32 = 0.5;
/// Samples between whole-sample steps of the split: 2 a block, as `DISP_SLEW`.
pub const BOW_SLEW: usize = 32;
/// The output's level: the v1 Bowed patch's C3 within ±1 dB of the old bow's.
pub const BOW_OUT: f32 = /* Step 6 */;

/// POS's bow position β.
pub fn beta(pos: f32) -> f32;                 // BETA_MIN + (BETA_MAX − BETA_MIN)·pos, pos clamped to [0, 1], NaN → 0
/// The bridge line's whole samples for a loop line of `d`: round(β·d), in [1, d − 1].
pub fn bridge_len(pos: f32, d: usize) -> usize;
/// `split` one whole sample towards `to`: the glide's step.
pub fn glide(split: usize, to: usize) -> usize;
/// (BOW_WIDTH·force)⁴: the friction curve's width, to the fourth.
pub fn width4(force: f32) -> f32;
/// The bow's push on the string at a velocity difference `dv`:
/// dv·ρ(dv), ρ = w4 / (w4 + dv⁴ + 1e-20). At most 0.57·w; 0 at w4 = 0.
pub fn push(dv: f32, w4: f32) -> f32;

/// The bridge filter's state: the bridge's last two returns.
#[derive(Clone, Copy, Default)]
pub struct Bridge { a1: f32, a2: f32 }
impl Bridge {
    /// c/2·(a + a2) + (1 − c)·a1, then shifts: linear phase, one sample, |H| ≤ 1 for c ≤ 0.5.
    pub fn reflect(&mut self, a: f32, c: f32) -> f32;
}

pub struct BowedString {
    pub string: KsString,
    pub force: f32, pub force_to: f32, pub lift: f32,
    pub bow_vel: f32, pub vel_scale: f32, pub bowing: bool,
    pub release: Release,
    bridge: Bridge,
    /// The bridge line's whole samples now; the nut line is `delay − split`.
    split: usize,
}
impl BowedString {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self;
    /// Note-on: the line cleared and tuned (`BRIDGE_DELAY` off the period),
    /// the split snapped to POS, the bridge filter zeroed.
    pub fn start(&mut self, freq: f32, sample_rate: u32, pos: f32);
    /// A pitch move: `set_period(period, BRIDGE_DELAY, w)`, the split clamped to the new line.
    pub fn tune(&mut self, freq: f32, sample_rate: u32);
    /// A block: in spans where neither the write nor the splice wraps; the
    /// split steps at samples 0 and `BOW_SLEW`; the block's gain, `c`, and
    /// FORCE's and SPEED's targets computed once.
    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], m: &Macros, (f0, vel_to): (f32, f32));
    /// One sample of `render`, for the tests: bit for bit the same, at
    /// the block's held gain, bridge filter `c` and bow velocity target.
    #[cfg(test)]
    pub fn tick(&mut self, held: LoopGain, c: f32, vel_to: f32) -> f32;
}
```

- In `string.rs`:

```rust
impl KsString {
    /// Bowed's waveguide on the ring (`bow.rs`): the ring in use
    /// (`ring_len` cells), the last write, the loop's line and its tuning allpass.
    pub(super) fn guide(&mut self) -> (&mut [f32], &mut usize, usize, &mut Allpass1);
}
```

- A sample, in `tick`'s order (the spans do the same arithmetic):
  - The bow eases or lifts as in Task 13; `w4 = width4(force)`; `bow_vel` is 0 once `force <= 0.001`.
  - `n = ` the nut line's return: the cell `delay` pushes back, the oldest in the loop. `a = ` the bridge's return: the cell `split` pushes back.
  - `bridge = −gain·bridge_filter.reflect(a, c)`, `nut = −n`. `gain` is `release.gain(held)`, `held` as Task 13.
  - `dv = bow_vel − (bridge + nut)`, `v = push(dv, w4)`.
  - The nut line's input `bridge + v` goes into the cell `a` came from; the bridge line's input `nut + v` is pushed through the tuning allpass at the next write.
  - The output is `BOW_OUT·(nut + v)`, the wave toward the bridge, before the allpass.
- A split step (`glide`), between spans only:
  - `+1`: the next sample's `a` is the filter's `a1` (the last return re-read), and the nut line's input overwrites the cell one further back.
  - `−1`: the nut line's input is written into both the cell `a` came from (the new split) and the cell one further back (the old split's), so no bridge-line sample is read as the nut's.
- Note-on: `force`, `force_to`, `lift`, `bow_vel`, `vel_scale`, `bowing` and `release` as Task 13; `start(freq, sr, m.pos)`. Note-off as Task 13.
- `render`'s return is unchanged: the voice is held while `force > 0` (#206).
- `translate_v1`, BOWED: `DAMP = damp_for(0.5)`, `BRIGHT = 0.5`, `POS = 0.15`, after the DECAY and BRIGHT rules as before. FORCE and SPEED stay at their defaults, 0.5.
- `BowedString` grows by the filter's two floats and the split, less `written`: it stays under `ModelSlot`'s 4,160 B, so `Voice` and `Instrument` do not grow. D2 +0 B.

- [ ] **Step 1: Record the old bow's level, then keep today's tests green.** On HEAD, before any other change:
  - Add to `codec_compat_test.rs` a temporary `eprintln!` of `rms(&held[SR/2..SR])` in `a_v1_bowed_patch_bows_as_before`, and run `cargo test -p chimera-core --test codec_compat_test -- a_v1_bowed_patch_bows_as_before --nocapture`. Paste the value as `const BOWED_V1_RMS: f32` with the comment "the one-loop bow's held C3, second half of its first second, recorded at 330298c". Remove the print.
  - Run `cargo test -p chimera-core --test modal_resonator_test -- bowed a_soft a_released_bowed force_and_speed`. Expected: PASS.
- [ ] **Step 2: Write the failing unit tests** in `bow.rs` (the module compiles with `todo!()` bodies and `BOW_OUT = 1.0` until Step 5):
  - `the_bow_table_sticks_at_rest_and_slips_away`: at `w4 = width4(0.5)`, `push(dv, w4) / dv` is within 1e-6 of 1 at `dv = 1e-4`, 0.5 at `dv = ±0.15` (`w`), and below 0.01 at `dv = ±0.6`.
  - `the_push_is_bounded_and_finite`: over `dv` in ±10 in 1e-3 steps and force in {0, 0.001, 0.5, 1}, `|push| <= 0.5700·BOW_WIDTH·force` and it is finite; `push(0.0, 0.0) == 0.0` and `push(x, 0.0) == 0.0`.
  - `the_bridge_filter_is_linear_phase_and_lossless_at_dc`: an impulse through `reflect` at `c` 0.5 gives `[0.25, 0.5, 0.25]`; at `c` 0, `[0, 1, 0]`; a constant input settles to itself.
  - `the_split_fits_the_line`: for `d` in `MIN_LINE..=979` and POS in 0..=1 by 1/64 (and NaN), `1 <= bridge_len(pos, d) <= d − 1`, and it rises with POS.
  - `glide_steps_one_sample`: `glide(10, 14) == 11`, `glide(10, 7) == 9`, `glide(10, 10) == 10`.
  - `an_impulse_comes_back_upright_once_a_period`: force 0 (the string free), BRIGHT 1, the held gain `LoopGain::TOP`, a period of `d + 2` whole samples (`frac` 1, so the allpass is a pure unit delay): one impulse on the bridge line returns to the output exactly `d + 2` samples later, positive, `0.9995` of it to 1e-6, and nothing between, at every POS in {0, 0.15, 0.5, 1}. This is the pitch law: two inversions a period, so one period, not two.
  - `a_splice_step_keeps_the_loop_length`: as above, with the split stepped +1 and then −1 mid-period: the impulse still returns once a period, positive, and no second impulse appears.
  - `render_is_tick_bit_for_bit`: 40 blocks at C3 and G1, POS gliding (0.2 → 0.9 at block 10), BRIGHT 0.3, FORCE easing, note-off at block 30: `render` and 64 × `tick` agree bit for bit.
- [ ] **Step 3: Write the failing integration tests.**
  - `modal_resonator_test.rs` (helpers: `v1_bowed()` = `ModalParams { mode: Bowed, force: 0.5, speed: 0.5, pos: 0.15, bright: 0.5, damp: damp_for(0.5), ..Default::default() }`; `octave_clear(s, f0)` = `goertzel(s, f0) > 10·goertzel(s, f0/2)`):
    - `bowed_is_in_tune`: `v1_bowed()`, notes 31 to 96, velocity 100, 2 s held; over seconds 0.5–1.5, `fundamental_hz(s, f0)` within ±2 cents and `octave_clear(s, f0)`. At 31 and 48, partials 2–4 within ±5 cents of multiples of it. Fails today: C3 is 65 Hz.
    - `bowed_pos_moves_the_tone_not_the_pitch`: notes 31, 48 and 84; POS {0, 0.15, 0.5, 1} × BRIGHT {0, 1}: each within 2 cents of the note and `octave_clear`. At note 48, POS 1 (β 0.5) puts the 2nd harmonic at least 10 dB below POS 0.15's (`goertzel` at 2·f0): the bow at the middle nulls the even harmonics. Fails today on the pitch.
    - `bowed_bright_is_heard`: `v1_bowed()` at note 48, BRIGHT 0 against 1: the summed `goertzel` power of harmonics 8 to 24 falls by at least 3 dB, and the fundamental moves under 2 cents. Fails today (under 0.5 dB).
    - `a_soft_bowed_note_sounds` gains a second case: velocity 20 on `v1_bowed()` sounds as the defaults' does, and over seconds 1–2 its `fundamental_hz` is within ±2 cents of note 48 and `octave_clear`. Fails today on the pitch. The defaults' case (INIT's POS 0, the bow at the bridge) keeps only its level clause: a light bow that near the bridge may play a surface sound, as a real one does.
    - `a_bowed_pos_sweep_does_not_click`: `ParamSnapshot` Bowed at `v1_bowed()`'s values, LFO 1 square at 2 Hz into POS at 127 (as `a_structure_step_at_g1_does_not_click`), notes 31 and 96, 2 s, at FORCE {0.5, 1}. The route changes the sound (`rms_diff > 1e-3`); `step` (Task 13's `tanh(0.4x)` measure, moved to `common`) over the routed render is at most 1.05 × the larger of the held renders' at POS 0 and 1; and `fundamental_hz` over seconds 0.5–1.5 is within ±2 cents of the note. Fails today on the pitch; the click clause is proved in Step 5.
    - `bowed_is_stable_and_in_tune_at_every_corner`: notes 31, 96 and 127, a thread each; FORCE, SPEED, BRIGHT and POS each at 0 and 1 (16 corners), 12 s held: `assert_stable(&out, 4.0, BOW_MARGIN, …)` (bounded, no growth, DC over 10 s). Each corner with FORCE and SPEED at 1, then released 1 s at DAMP 0 and 1: finite and within 4.0. Every corner with FORCE > 0 and SPEED > 0 at notes 31 and 96 sounds (`rms > 1e-3` over the last second) and is `octave_clear`. Fails today on the octave.
    - `every_model_is_stable_at_every_extreme` is unchanged; it must still pass.
    - `bowed_low_notes_sound`, `bowed_damp_is_the_ring_after_the_lift`, `a_released_bowed_c2_is_silent_within_half_a_second`, `force_and_speed_move_a_held_bow`, `macros_are_routable`, `live_knobs_move_dimmed_knobs_do_not` and `release_does_not_click` are unchanged; they must still pass.
    - Delete `bowed_pos_and_bright_keep_pitch`.
  - `codec_compat_test.rs`:
    - `old_modal_patches_translate`: on BOWED, `(m.damp, m.bright, m.pos) == (damp_for(0.5), 0.5, 0.15)`, and `(m.force, m.speed) == (0.5, 0.5)`. Fails today.
    - Rename `a_v1_bowed_patch_bows_as_before` to `a_v1_bowed_patch_bows_in_tune`: the held second is within ±2 cents of note 48 and `octave_clear`; its RMS over the second half is within ±1 dB of `BOWED_V1_RMS`; and the held and released renders' `fnv1a` equal `BOWED_V1_HELD` and `BOWED_V1_RELEASED`. Leave the two hashes as they are until Step 7. Fails today on the pitch.
  - `cost_test.rs`: `modal_bills_each_model`'s BOWED row is written in Step 9, with the counted bill and voice counts, before `COST_BOWED` changes, so it fails first.
- [ ] **Step 4: Run them to verify they fail.**
  - `cargo test -p chimera-core --lib -- modal::bow modal::string`
  - `cargo test -p chimera-core --test modal_resonator_test -- bowed_is_in_tune bowed_pos_moves_the_tone_not_the_pitch bowed_bright_is_heard a_soft_bowed_note_sounds a_bowed_pos_sweep_does_not_click bowed_is_stable_and_in_tune_at_every_corner`
  - `cargo test -p chimera-core --test codec_compat_test -- old_modal_patches_translate a_v1_bowed_patch_bows_in_tune`
  - Expected: the unit tests panic on `todo!()`. The integration tests fail on the pitch (C3 near 65 Hz, `octave_clear` false), BRIGHT under 3 dB, and the v1 defaults. If `bowed_is_in_tune` passes on today's code, it does not measure the octave: stop and report.
- [ ] **Step 5: Implement** the Interfaces, `tick` first, then `render` in spans (Task 11b's pattern: `m = min(left, ring_len − write, ring_len − splice, next step)`; the filter's taps, the allpass, the force and the bow's velocity in registers; one `vdiv.f32` a sample, no transcendental). Delete what "What Task 13's output-comb POS leaves behind" lists. Any `match` on `ModelSlot` keeps its arms; exclusive state is untouched (`bowed_clears_the_ring_it_wrote` stays).
  - Prove the click clause discriminates: temporarily make `glide` jump straight to its target, run `cargo test -p chimera-core --test modal_resonator_test -- a_bowed_pos_sweep_does_not_click`, and see it fail on the step measure. Restore `glide`. If it passes with the jump, the measure is blind to the splice: tighten it to 1.0 × the corner, or measure the second difference as `kink` does, and say which in the report.
  - If `bowed_bright_is_heard` reads under 3 dB, apply the ruled fallback (the squared five-tap filter, `BRIDGE_DELAY = 2.0`) and record the measured dB either way.
- [ ] **Step 6: Set `BOW_OUT`.** Run `cargo test -p chimera-core --test codec_compat_test -- a_v1_bowed_patch_bows_in_tune --nocapture` with a temporary `eprintln!` of the held RMS at `BOW_OUT = 1.0`. Set `BOW_OUT = BOWED_V1_RMS / that`, rounded to 3 significant figures, with its measurement in the doc comment. Remove the print.
- [ ] **Step 7: Re-record the v1 pins, deliberately.** With a temporary `eprintln!` of both hashes, run `cargo test -p chimera-core --test codec_compat_test -- a_v1_bowed_patch_bows_in_tune --nocapture`. Paste them into `BOWED_V1_HELD` and `BOWED_V1_RELEASED`, and replace their doc comment with "A v1 Bowed patch on the two-delay bow (plan Task 14): re-recorded deliberately, since the one-loop bow played an octave low (#240)". Remove the print. Only after the pitch, level and translation clauses pass.
- [ ] **Step 8: Run the tests to verify they pass.**
  - `cargo test -p chimera-core --lib -- modal`
  - `cargo test -p chimera-core --test modal_resonator_test --test codec_compat_test --test golden_test --test pitch_test --test exclusive_state_test --test sym_pool_test --test in_place_test --test memory_budget_test --test modal_integration_test --test modulatable_test --test sanity_test -- --nocapture`
  - Expected: PASS. No `golden_test` row or `FIXTURE_RENDERS` hash moves (no golden plays Bowed); if one does, stop and investigate. `instrument_fits_d2` prints the same `Instrument` size as Task 13's, and `BowedString` stays under `ModelSlot`.
- [ ] **Step 9: Re-bill Bowed and the bench rows.**
  - Build with `just firmware` and count `BowedString::render`'s fast span by ADR 0056's method: `llvm-objdump -d --mcpu=cortex-m7 target/thumbv7em-none-eabihf/release/chimera-stm32`, instructions a sample, spans at the bench's notes (2.2 a block, plus the split's two step points), the per-block setup spread over 64, the output blocker's 7.8.
  - The bill: `620 (benched) + (N − 143) × 1.46 × 0.9 − T × 1.46 × 0.9 + (14 − 1.46) × 1.1`, rounded up to 10, where `N` is the new count, 143 the one-loop bow's count at the bench, `T` the two `tanhf` bodies' instructions on their taken path (counted from `libm::tanhf`'s disassembly; they were in the bench's 620), and the last term the `vdiv.f32`'s 14 cycles against its 1.46. Expected: `N` about 50, a bill about 450–550. The gate is 860 (today's `COST_BOWED`); over it, stop and report.
  - Set `COST_BOWED` with this arithmetic in its doc comment, replacing Task 13's. Update `modal_bills_each_model`'s BOWED row (its bill and its voice counts, with and without the tape), deliberately; run `cargo test -p chimera-core --test cost_test` and `cargo test -p chimera-core --test cost_test --features master-tape`. Expected: PASS.
  - `bench.rs`: no new row. `MDL BOW` (the default Bowed Sound) and `MDL BOW+` keep their builders; `bow_full`'s doc comment says that its 10 Hz POS route steps the split twice a block, the bow's worst case. Run `just firmware`. Expected: exit 0.
- [ ] **Step 10: Amend ADR 0056** (Proposed, so it may be edited):
  - **Bowed**, rewritten: the two lines on one ring and the splice; the pitch law (`BRIDGE_DELAY` off the period, upright once a period); POS as β from 0.06 to 0.5 and the split's whole-sample glide (`BOW_SLEW`); BRIGHT as the bridge filter at `0.5·(1 − BRIGHT)`, with the measured harmonic change; DAMP, FORCE, SPEED and the velocity scaling as before; the bow table `w⁴/(w⁴ + Δv⁴)` and its bound; the output toward the bridge and `BOW_OUT`; stability by construction.
  - **Tuning:** "BOWED's ring runs through the same allpass" becomes the bridge line's input.
  - **Old patches:** v1 Bowed loads POS 0.15, BRIGHT 0.5, DAMP `damp_for(0.5)`, FORCE and SPEED 0.5, and why (the old sound was the bug).
  - **Memory:** `BowedString`'s new size; D2 unchanged.
  - **Costs:** the BOWED rows of both tables (the count, `T`, the `vdiv`), the voice counts, and the bench rows' note on BOW+.
  - **Open for the owner:** drop the octave bullet.
  - **Alternatives considered:** Task 13's output comb (pitch held, but not a bow position, and BRIGHT inaudible); two separate rings (+1.8 KB a voice); a fractional split by interpolation (its loss moves the tone with the fraction) or by two allpasses (more cost, the same whole-sample crossing); STK's bow table (a licensed constant set; ours is a rational curve with no `powf`).
  - **Sources:** J. O. Smith, *Physical Audio Signal Processing*, CCRMA, "Bowed Strings" and "Digital Waveguide Bowed-String"; M. E. McIntyre, R. T. Schumacher and J. Woodhouse, "On the oscillations of musical instruments", JASA 74(5), 1983; STK's `Bowed` named as a known implementation, not a source: no STK code, constant or table is used, so THIRD_PARTY.md does not change. Add "Task 14" to the plan's range, and the owner's decision of 2026-09-30 on #240.
  - Check provenance: `git diff 330298c -- chimera-core | grep -n -i -E "stk|0\.75|5\.0 - 4\.0|bowTable"` prints nothing.
- [ ] **Step 11: Listen on the desktop**, in `$SP/demo-m2` only. Leave `$SP/demo` alone.
  - `modal_bowed()` and `modal_bowed_soft()` keep their settings; their descriptions in `src/clips.rs` say the bow now plays at its note, POS moves the bow and BRIGHT darkens it.
  - Run `cd $SP/demo-m2 && cargo run -q --release --bin demo -- modal-bowed-drone && cargo run -q --release --bin demo -- modal-bowed-soft`. Expected: `out/modal-bowed-drone.wav` and `out/modal-bowed-soft.wav` re-rendered, the drone's D an octave above the old clip's.
  - Tell the owner which files changed, the v1 defaults, and that INIT → MODEL BOWED now bows at POS 0 (near the bridge), BRIGHT 0.3 and INIT's long ring.
- [ ] **Step 12: Run the green gate.** Run `just test`, `just clippy`, `just firmware` and `just check`. Expected: each exits 0.
- [ ] **Step 13: Commit.**

```bash
git add chimera-core/src/dsp/modal/bow.rs chimera-core/src/dsp/modal/mod.rs chimera-core/src/dsp/modal/string.rs chimera-core/src/dsp/modal/params.rs chimera-core/tests/modal_resonator_test.rs chimera-core/tests/codec_compat_test.rs chimera-core/tests/cost_test.rs chimera-core/tests/common/mod.rs chimera-stm32/src/bench.rs docs/adr/0056-modal-resonators-share-four-macros.md
git commit -m "Bowed is a two-delay bowed string: in tune, POS is the bow, BRIGHT is heard"
```

---

### Task 12: Ship: one flash, the bench and the ears

Task 12 runs after Task 14, which follows Task 13 (the owner's decisions of 2026-09-30): the one flash ships the EXC node and the two-delay bow (#240) with the resonators.

**Files:**
- Modify: `chimera-core/src/dsp/modal/mod.rs` (`COST_*` to the bench figures), `chimera-core/tests/cost_test.rs` (a measured row per MDL bench row), `docs/adr/0056-*.md` (Consequences: chip figures)

This is the only task that touches hardware. Run it with the owner, on one combined flash of the branch.

- [ ] **Step 1: Build and flash.** Run `just flash-bench` for Steps 2 and 3, then `just flash` for the play test in Step 3 (l).
- [ ] **Step 2: Read the bench rows.** Record each `/VOICE` and voice count on rev V at 480 MHz:
  - `MDL STR`, `MDL STR+`, `MDL BOW`, `MDL SYM` (1–4 notes and flat past 4), `MDL SYM+`, `MDL RES`, `MDL RES48`, `SWITCH`.
  - Task 14's bow: `MDL BOW` (the two-delay bow at the default Bowed Sound) and `MDL BOW+` (FORCE 1, SPEED 1, LFO 1 on BRIGHT, DAMP and POS, so the split steps twice a block). Compare each with Task 14's host bill; if `MDL BOW` bills above 860, file an issue.
  - `SYM NOTE-ON` and `SYM NOTE-ON LOW`, which grew with the 1,016-sample line.
  - `DARK NOTE+BLOCK` (Task 13, was `PLUCK DARK`): a G1 STRING note-on at COLOR 0 and its first block. If it overruns a block beside eight voices, file an issue. `DARK +6 PASSES` is the passes alone.
  - The MEMORY screen's `Voice`, `MODAL` and `SYM POOL`, and D2 left.
- [ ] **Step 3: Listen, by ear, with the owner.** Check:
  - (a) Each model at G1, C4 and C6: in tune, and STRING distinct from Bowed. Bowed plays at its note, not an octave below (C3 at 130.8 Hz on a tuner).
  - (b) DAMP swept 0 → 1 on STRING: a short pluck to near-endless, never a runaway or a DC thump.
  - (c) STRUCTURE on STRING: nylon to wire, the pitch unmoved. On BANK: harmonic to bell. On SYMP: the chords step and glide.
  - (d) BRIGHT and POS on each model.
  - (e) Releases: no click on any model.
  - (f) Bowed below C2: sounds at once and holds.
  - (g) The ensemble at DEPTH 1 on low and high notes.
  - (h) BODY: colour, with no octave jump.
  - (i) An LFO on each macro.
  - (j) SPACE and the Part's REV send move together.
  - (k) An old v1 Modal patch with FDBK 1 loads and plays calmly.
  - (k2) Task 13's EXC node: the map reads EXC · RES · FLT · AMP · MOD, and a Part opens on EXC. The header reads PLUCK, STRIKE or BOW as MODEL changes.
  - (k3) PLUCK: EXCITE and COLOR on STRING and SYMP, dark to bright. STRIKE: EXCITE and BURST on BANK, a click to a thud.
  - (k4) BOW (Task 14): a velocity-20 key bows, in tune. FORCE and SPEED move the tone. DAMP is the ring after the bow lifts. BRIGHT clearly darkens. POS moves the bow from near the bridge (glassy) to the middle (hollow, the even harmonics gone), swept by hand and by an LFO without a click, the pitch unmoved. No runaway at FORCE 1 and SPEED 1, at G1 or C7.
  - (k5) An old v1 BANK patch sounds as before. An old v1 Bowed patch plays in tune, at about its old level, at the Task 14 defaults (POS 0.15, BRIGHT 0.5, a 0.5 s ring after the lift): the owner confirms by ear that they sound like a reasonable bowed string, or names new ones.
  - (l) An eight-note chord on STRING and on SYMP, with reverb, delay and every page edited: record LOAD, OVER and DROPS.
- [ ] **Step 4: Bill the measurements.**
  - Set each `COST_*` to its bench slope less the Modal Sound's chain (57), as ADR 0054 did. `COST_BOWED` takes Task 14's `MDL BOW`.
  - In `cost_test.rs`, add `the_model_bills_every_modal_row_high` with the measured rows, `MDL BOW+` among them. If `MDL SYM` bills 883 or less, SYMP keeps 6 voices on rev V: note it.
  - Run `just check`. Expected: exit 0.
- [ ] **Step 5: Finish ADR 0056.** Add the chip figures and the owner's by-ear verdict to Consequences. It stays Proposed until the owner accepts it; only then does the status become `Accepted (date)`, in the file and the README.
- [ ] **Step 6: Commit.**

```bash
git add chimera-core/src/dsp/modal/mod.rs chimera-core/tests/cost_test.rs docs/adr/0056-modal-resonators-share-four-macros.md docs/adr/README.md
git commit -m "Modal 2 resonators billed at the ship bench's figures"
```

- [ ] **Step 7: Close the issues.** In the PR description, reference #191, #10, #50, #51, #163, #206 and #240 as closed. File any by-ear finding as a new GitHub issue; don't write it into the repo.
