# Modal 2, step A: the resonators

Sub-project 2 of Modal 2 (#41). Builds on exclusive state (sub-project 1, PR #213). Step B is the blended bow / blow / strike exciter; step C is the RES / EXC / model pages and their visuals.

## Why

The owner's bench report (#191): only the bank answers its knobs; String and Bowed sound alike and barely respond. The survey behind this spec found why:

- Bowed reads none of its page's parameters; the bank reads 5 of 12; INHARM is dead on String.
- FDBK sets the string's loop gain above 1 for any value over about 0.012. The string grows until a ±1.5 clamp and can latch DC. There is no DC blocker anywhere in the chain.
- BODY is a comb inside the loop that transposes the string an octave up (#10).
- The ensemble's LFO turns once every 90–290 s and reads the neighbouring sample, so it is inaudible (#50).
- Strings tune in whole samples (#163), and note-off scales the buffers at once, which clicks (#51).
- Bowed notes below about C2 free their voice before the first period sounds (#206).
- Every Modal knob is read at note-on only; nothing in the matrix can move one.

## Decisions (owner, 2026-09-29)

- **Resonators first.** Step A reworks the resonators with today's excitation; the exciter is step B.
- **Shared macros, the Rings approach.** Every model reads the same home knobs, each in its own way. Model-only extras live on a second page.
- **Bowed stays** as a fourth model with today's sound until step B folds it into the exciter. It gets the stability fixes.
- **FDBK is folded into DAMP.** DAMP's top reaches near-endless sustain; nothing can run away.
- **The ensemble is rebuilt**, not dropped.
- **The four macros are mod destinations**, read every block.
- **Replace, not add.** The four models are rebuilt in place; no "classic" model keeps today's bugs.

## 1. Controls

### Home page (RES)

`MODEL · STRUCTURE · BRIGHT · DAMP · POS · SPACE`

| | STRUCTURE | BRIGHT | DAMP | POS |
|---|---|---|---|---|
| BANK | Mode stretch, harmonic to bell (today's INHARM) | High-mode damping (today's BRIGHT) | Ring time, thud to about 20 s (today's DECAY) | Strike point; nulls the modes it lands on |
| STRING | Stiffness: real dispersion, nylon to piano wire | Loop low-pass, gut to steel | Short pluck to near-endless sustain | Pluck point |
| SYMP | Steps a chord table for the 7 halo strings | Main string and halo | Main string and halo | Main string's pluck point |
| BOWED | dimmed | dimmed | dimmed | dimmed |

- **MODEL** is a named enum: BANK, STRING, SYMP, BOWED. A change is a switch (exclusive-state spec § 3): the Part's voices fade and rebuild, and the new model plays from the next note.
- **STRUCTURE, BRIGHT, DAMP, POS** are modulatable. They are read every block from the modulated values and eased between blocks; none snaps. POS on STRING and SYMP takes effect at the next pluck, because the pluck comb shapes the excitation.
- **SPACE** addresses the Part's existing reverb send. It is the same value as the send elsewhere, not a copy.

### Model page (MDL2), by model

| | Cells |
|---|---|
| STRING | EXCITE, BODY, ENS DEPTH, ENS RATE, ENS MIX |
| SYMP | EXCITE, COUPLE, HALO, BODY, ENS DEPTH, ENS MIX |
| BANK | EXCITE, MODES (16 / 24 / 32 / 48) |
| BOWED | none live |

- **EXCITE:** the strike level, as today.
- **COUPLE:** how hard the main string drives the halo (fixed at 0.025 today).
- **HALO:** the halo's level in the mix (fixed at 0.15 today).
- **MODES:** the bank's size, which also sets its billed cost.
- Model-page settings are read at note-on and are not modulatable. The ensemble LFO runs every sample.

### Dimming

A control the current model ignores is dimmed through `view::dimmed` (`ui/view.rs`). That dims the page cell and its matrix column together (`mod_grid::inert_dests`). Every Modal control has one rule, and a test holds each rule to the audio: a dimmed control changes nothing, and a live one changes something.

## 2. DSP

### Stability by construction

- **`LoopGain`**, a newtype whose constructor caps it at 0.9995. DAMP maps into it; no code path makes a string loop gain of 1 or more.
- **A DC blocker**, a one-pole high-pass at about 10 Hz, inside every string loop: STRING, the SYMP main string, each halo string and BOWED.
- FDBK and its `±1.5` clamp are deleted.

### String

- **Fractional tuning (#163):** a first-order allpass carries the fraction of a sample; the delay length carries the whole part. The cost is fixed, not per note.
- **Dispersion:** STRUCTURE drives a short allpass chain inside the loop. Its phase delay is taken off the integer delay, so pitch stays put as stiffness rises. It replaces STIFF's two-sample mix.
- **BODY (#10):** a small fixed resonant body filter on the output, outside the loop. It colours without transposing.
- **Release (#51):** note-off ramps the loop's damping over about 5 ms. Nothing scales a buffer at once.
- **Ensemble:** 2 or 3 extra read heads on the same delay line, linearly interpolated, with a real LFO (0.1–6 Hz) and up to about ±15 cents of detune. It mixes on the output. No new buffers.

### SYMP

- STRUCTURE steps through a chord table for the 7 halo strings, following Rings' chords and adapted to 7 strings. The table has no duplicate intervals; today's has 12 and 7.02 twice.
- Crossing into a new chord glides the halo strings' delays over about 20 ms, through the fractional tuning.
- COUPLE and HALO replace the fixed 0.025 and 0.15.
- The pool, leases and the no-steal rule are unchanged (ADR 0054).

### BANK

- The SVF bank is unchanged. It already recomputes its filters every block, so the live macros just read the modulated values.
- MODES exposes the hidden `num_modes`.

### BOWED

- Unchanged sound. It gets the DC blocker. The fix for #206 is that the read tap starts where the note has written, so a low note is not silent for its first period.

### Cost and memory

- Each model's `COST_*` is re-estimated on the host and gets a bench row. The expectation is about +30 to +60 cycles a voice, from the allpasses, the DC blocker and the easing.
- No new buffers. The ~126 KB of D2 that exclusive state freed stays free for step B.

## 3. Data, pages, compatibility

- **`ModalParams`:**
  - home: MODEL, STRUCTURE, BRIGHT, DAMP, POS;
  - model page: EXCITE, BODY, ENS DEPTH, ENS RATE, ENS MIX, COUPLE, HALO, MODES.
  - Each gets a frozen disk code and ident (the storage rules). FDBK, STIFF, INHARM, DECAY, E.*, `ks_excitation`, `ks_color`, `bow_velocity`, `bow_force` and the unused `note` leave the struct.
- **Old patches load.** The translation happens once, at decode:
  - DECAY becomes DAMP;
  - BRIGHT and POS carry over;
  - STRUCTURE comes from STIFF on STRING, and from INHARM on BANK and SYMP;
  - E.DPT, E.RAT and E.MIX become the ENS controls;
  - FDBK and the hidden fields are dropped.
  - The v1 fixture `init_modal.snd` pins the translation.
- **Pages:**
  - MODEL shows names.
  - MDL2's cells follow MODEL.
  - SPACE sits on the home page.
  - The visuals are unchanged, since step C owns them. The screen goldens for the Modal pages are re-recorded.

## Tests

Each is written to fail on today's code where today's code is wrong.

| Test | Proves |
|---|---|
| `every_model_is_stable_at_every_extreme` | Each model, each macro and model setting at 0 and 1, 30 s held: bounded, no growth, \|DC\| < 0.001. Today's FDBK fails it. |
| `strings_are_in_tune` | STRING and the SYMP main from G1 to C7, within ±2 cents. Whole-sample tuning fails it. |
| `dispersion_keeps_pitch` | STRUCTURE 0 to 1 on STRING moves the pitch by under 2 cents. |
| `live_knobs_move_dimmed_knobs_do_not` | Per model: every live control changes the output measurably; every dimmed one changes nothing, bit for bit. |
| `macros_are_routable` | An LFO on each macro moves the sound, with no clicks. |
| `release_does_not_click` | Note-off on every model passes the click detector. |
| `chord_change_glides` | STRUCTURE across a chord boundary on SYMP: no click, and the halo reaches the new tuning within 25 ms. |
| `ensemble_is_audible` | ENS on vs off: measurable spectral spread and amplitude movement at the LFO rate. |
| `bowed_low_notes_sound` | Bowed G1: audible within the first block and not freed (#206). |
| `old_modal_patches_translate` | `init_modal.snd` and a v1 patch per model decode to the mapped values. |
| cost and memory tests | New `COST_*` values; `Instrument` still fits D2. |

- The Modal goldens (`modal_init`, `modal_lfo_cutoff`, `modal_sympathetic`, `algo_to_modal_switch`) and the `init_modal.snd` render hash are re-recorded once, deliberately. Their `KNOWN_BROKEN` #10 entries go.
- **Listening:** the reel's Modal clips are re-rendered on the desktop as each model lands, so the owner hears each change without a flash.
- **Ship:** one hardware flash with a bench row per model.

## Provenance

- **From Rings, under the MIT notice (ADR 0032):** the SYMP chord table and the dispersion approach. The SVF bank's filter and mode-tuning math was already Rings-derived and stays so.
- **Ours:** `LoopGain`, the DC blocker placement, the fractional tuning, the body filter, the ensemble, the release ramp and the macro mapping.
- A new ADR records the macros, the FDBK removal, the provenance split and the Modal macros becoming modulatable (a partial supersession of ADR 0010's "Modal settings are read at note-on").

## Closes

#191, #10, #50, #51, #163, #206.

## Out of scope

- The bow / blow / strike exciter and Bowed's move into it (step B).
- The RES / EXC / model pages' visuals and animation (step C).
- POLY / VOICES (#207); the SVF bank's internals; the Sympathetic pool.
