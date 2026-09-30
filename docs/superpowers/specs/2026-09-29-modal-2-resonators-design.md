# Modal 2, step A: the resonators

Sub-project 2 of Modal 2 (#41). Builds on exclusive state (sub-project 1, PR #213). Step B is the blended bow / blow / strike exciter; step C is the RES / EXC / model pages' visuals and animation.

(amended 2026-09-30, owner) Step A now also gives the exciters their own node, EXC, first in the chain. It shows the exciter each model already has; step B grows it into the blended mixer. See the amended § 1, § 2 BOWED and § 3.

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
- ~~**Bowed stays** as a fourth model with today's sound until step B folds it into the exciter.~~ Superseded below. It gets the stability fixes.
- **(amended 2026-09-30, owner) Exciters get their own node; Bowed is playable.** On the bench, Bowed's values didn't move and soft keys made no sound on Bowed. So the exciters move to a node of their own, EXC, first in the chain, and Bowed's macros go live (§ 1, § 2 BOWED). Bowed stays a fourth model until step B.
- **FDBK is folded into DAMP.** DAMP's top reaches near-endless sustain; nothing can run away.
- **The ensemble is rebuilt**, not dropped.
- **The four macros are mod destinations**, read every block.
- **Replace, not add.** The four models are rebuilt in place; no "classic" model keeps today's bugs.

## 1. Controls

### Chain map (amended 2026-09-30, owner)

`EXC · RES · FLT · AMP · MOD`. The filter stays. A Part's Modal chain opens on EXC.

### Exciter page (EXC) (amended 2026-09-30, owner)

EXC shows the exciter each model already has. Its header is named after the exciter; its map node reads EXC.

| MODEL | Exciter | Cells |
|---|---|---|
| STRING, SYMP | PLUCK | EXCITE (level), COLOR (the pluck noise, dark to bright) |
| BANK | STRIKE | EXCITE (level), BURST (the strike's length) |
| BOWED | BOW | FORCE, SPEED |

- **EXCITE:** the pluck's or the strike's level, as before. It moves here from the model page; its value and disk code are unchanged.
- **COLOR:** the old hidden `ks_color`, the pluck's smoothing passes. Its default is the old hidden value.
- **BURST:** the strike's noise burst, 2 to 6 ms. Until now EXCITE set it too.
- **FORCE:** the bow's pressure. A note bows at FORCE × (0.5 + 0.5 × velocity), so a soft key still bows.
- **SPEED:** the bow's velocity.
- EXC's controls are read at note-on and are not modulatable, like the model page. Step B's mixer decides what becomes live.

### Home page (RES)

`MODEL · STRUCTURE · BRIGHT · DAMP · POS · SPACE`

| | STRUCTURE | BRIGHT | DAMP | POS |
|---|---|---|---|---|
| BANK | Mode stretch, harmonic to bell (today's INHARM) | High-mode damping (today's BRIGHT) | Ring time, thud to about 20 s (today's DECAY) | Strike point; nulls the modes it lands on |
| STRING | Stiffness: real dispersion, nylon to piano wire | Loop low-pass, gut to steel | Short pluck to near-endless sustain | Pluck point |
| SYMP | Steps a chord table for the 7 halo strings | Main string and halo | Main string and halo | Main string's pluck point |
| BOWED | dimmed | A gentle low-pass in the loop (amended 2026-09-30, owner) | The ring after the bow lifts (amended 2026-09-30, owner) | Bow position (amended 2026-09-30, owner) |

- **MODEL** is a named enum: BANK, STRING, SYMP, BOWED. A change is a switch (exclusive-state spec § 3): the Part's voices fade and rebuild, and the new model plays from the next note.
- **STRUCTURE, BRIGHT, DAMP, POS** are modulatable. They are read every block from the modulated values and eased between blocks; none snaps. POS on STRING and SYMP takes effect at the next pluck, because the pluck comb shapes the excitation.
- **SPACE** addresses the Part's existing reverb send. It is the same value as the send elsewhere, not a copy.
- (amended 2026-09-30, owner) RES is purely the resonator: MODEL, STRUCTURE, BRIGHT, DAMP, POS and SPACE. BOWED reads BRIGHT, DAMP and POS; only STRUCTURE stays dimmed on it until step B.

### Model page (MDL2), by model

(amended 2026-09-30, owner: EXCITE leaves this page for EXC.)

| | Cells |
|---|---|
| STRING | BODY, ENS DEPTH, ENS RATE, ENS MIX |
| SYMP | COUPLE, HALO, BODY, ENS DEPTH, ENS MIX |
| BANK | MODES (16 / 24 / 32 / 48) |
| BOWED | none live |

- **COUPLE:** how hard the main string drives the halo (fixed at 0.025 today).
- **HALO:** the halo's level in the mix (fixed at 0.15 today).
- **MODES:** the bank's size, which also sets its billed cost.
- Model-page settings are read at note-on and are not modulatable. The ensemble LFO runs every sample.

### Dimming

A control the current model ignores is dimmed through `view::dimmed` (`ui/view.rs`). That dims the page cell and its matrix column together (`mod_grid::inert_dests`). Every Modal control has one rule, and a test holds each rule to the audio: a dimmed control changes nothing, and a live one changes something.

## 2. DSP

### Stability by construction

- **`LoopGain`**, a newtype whose constructor caps it at 0.9995. DAMP maps into it; no code path makes a string loop gain of 1 or more.
- **A DC blocker**, a one-pole high-pass at about 10 Hz, on each string model's output, one per voice: STRING, SYMP after the main-and-halo mix, and BOWED after its ring. It stays out of the loops because there its phase detunes the upper partials, and `LoopGain` < 1 already keeps DC from growing inside them.
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
- (amended 2026-09-30, owner) The strike's burst lasts 2 + 4 × BURST ms. It used to be 2 + 4 × EXCITE. EXCITE is the strike's level alone.

### BOWED

- It gets the DC blocker. The fix for #206 is that the read tap starts where the note has written, so a low note is not silent for its first period.
- ~~Unchanged sound.~~ (amended 2026-09-30, owner) Bowed is playable in step A:
  - **Velocity:** the bow's force is FORCE × (0.5 + 0.5 × velocity). At FORCE 0.5 and full velocity that is the old bow, so a soft key still bows.
  - **SPEED** is the bow's velocity. At 0.5 it is the old one.
  - **DAMP** is the ring after the bow lifts: at note-off the loop ramps from its bowed gain to DAMP's T60, and never gives the gain back. While the bow is on, the bow sustains the string, as before.
  - **BRIGHT** is a gentle low-pass in the loop. It is linear-phase, centred on the tap, so it adds no delay and the pitch holds. At BRIGHT 1 the tap is read alone, as before.
  - **POS** is where the bow meets the string. The friction reads the string through a two-tap comb on the ring, the pluck's comb law. The loop and the output keep reading the one tap, so the pitch holds. At POS 0 the bow reads the one tap, as before.
  - Stability by construction still holds: the loop's gain is a `LoopGain` below 1, the low-pass's gain is at most 1, the friction is bounded, and the output has its DC blocker.
- (amended 2026-09-30, owner) **STRING and SYMP's pluck:** COLOR sets the pluck noise's smoothing passes, `⌊(1 − COLOR) × 7⌋`. The old hidden 0.8 is one pass, as before. At COLOR 0 the first block of a low note does seven passes over the line, and the ship bench measures that.

### Cost and memory

- Each model's `COST_*` is re-estimated on the host and gets a bench row. The expectation is about +30 to +60 cycles a voice, from the allpasses, the DC blocker and the easing.
- No new buffers. The ~126 KB of D2 that exclusive state freed stays free for step B.

## 3. Data, pages, compatibility

- **`ModalParams`:**
  - home: MODEL, STRUCTURE, BRIGHT, DAMP, POS;
  - model page: BODY, ENS DEPTH, ENS RATE, ENS MIX, COUPLE, HALO, MODES;
  - (amended 2026-09-30, owner) exciter page: EXCITE, COLOR, BURST, FORCE, SPEED.
  - Each gets a frozen disk code and ident (the storage rules, ADR 0045). EXCITE keeps its code. FDBK, STIFF, INHARM, DECAY, E.*, `ks_excitation`, `ks_color`, `bow_velocity`, `bow_force` and the unused `note` leave the struct.
- **Old patches load.** The translation happens once, at decode:
  - DECAY becomes DAMP;
  - BRIGHT and POS carry over;
  - STRUCTURE comes from STIFF on STRING, and from INHARM on BANK and SYMP;
  - E.DPT, E.RAT and E.MIX become the ENS controls;
  - FDBK and the hidden fields are dropped.
  - The v1 fixture `init_modal.snd` pins the translation.
- **(amended 2026-09-30, owner) Old patches and the exciters:**
  - An old patch's EXCITE keeps its value; it now shows on EXC.
  - COLOR, FORCE and SPEED load at their defaults: 0.8, 0.5 and 0.5, the old hidden values. So an old Bowed patch bows as before.
  - On BANK, BURST is set to the patch's EXCITE, so an old strike keeps its length.
  - Old Bowed patches stored BRIGHT, DAMP and POS but never read them. They load as the old sound: BRIGHT 1, POS 0, and DAMP at the old lifted-bow ring of 0.12 s.
- **Pages:**
  - (amended 2026-09-30, owner) The Modal chain is `EXC · RES · FLT · AMP · MOD`, and EXC's cells follow MODEL.
  - MODEL shows names.
  - MDL2's cells follow MODEL.
  - SPACE sits on the home page.
  - The visuals are unchanged, since step C owns them. The screen goldens for the Modal pages are re-recorded, and re-recorded again for EXC (amended 2026-09-30, owner).

## Tests

Each is written to fail on today's code where today's code is wrong.

| Test | Proves |
|---|---|
| `every_model_is_stable_at_every_extreme` | Each model, each macro and model setting at 0 and 1, 30 s held: bounded, no growth, \|DC\| < 0.001. Today's FDBK fails it. |
| `strings_are_in_tune` | STRING and the SYMP main from G1 to C7, within ±2 cents. Whole-sample tuning fails it. |
| `dispersion_keeps_pitch` | STRUCTURE 0 to 1 on STRING moves the pitch by under 2 cents. |
| `live_knobs_move_dimmed_knobs_do_not` | Per model: every live control changes the output measurably; every dimmed one changes nothing, bit for bit. (amended 2026-09-30, owner) The note is released too, so Bowed's DAMP is heard, and the EXC controls are included. |
| `macros_are_routable` | An LFO on each macro moves the sound, with no clicks. |
| `release_does_not_click` | Note-off on every model passes the click detector. |
| `chord_change_glides` | STRUCTURE across a chord boundary on SYMP: no click, and the halo reaches the new tuning within 25 ms. |
| `ensemble_is_audible` | ENS on vs off: measurable spectral spread and amplitude movement at the LFO rate. |
| `bowed_low_notes_sound` | Bowed G1: audible within the first block and not freed (#206). |
| `a_soft_bowed_note_sounds` (amended 2026-09-30, owner) | A velocity-20 Bowed note sounds. Today's bow fails it. |
| `bowed_damp_is_the_ring_after_the_lift` (amended 2026-09-30, owner) | After note-off Bowed decays at DAMP's T60; while bowed, DAMP changes nothing. |
| `bowed_pos_and_bright_keep_pitch` (amended 2026-09-30, owner) | POS and BRIGHT on Bowed move its fundamental by under 2 cents. |
| `a_v1_bowed_patch_bows_as_before` (amended 2026-09-30, owner) | A v1 Bowed patch's held note renders bit for bit as before the EXC node. |
| `old_modal_patches_translate` | `init_modal.snd` and a v1 patch per model decode to the mapped values. (amended 2026-09-30, owner) This includes EXC's defaults, BURST from EXCITE on BANK, and Bowed's neutral macros. |
| cost and memory tests | New `COST_*` values; `Instrument` still fits D2. |

- The Modal goldens (`modal_init`, `modal_lfo_cutoff`, `modal_sympathetic`, `algo_to_modal_switch`) and the `init_modal.snd` render hash are re-recorded once, deliberately. Their `KNOWN_BROKEN` #10 entries go.
- **Listening:** the reel's Modal clips are re-rendered on the desktop as each model lands, so the owner hears each change without a flash.
- (amended 2026-09-30, owner) The EXC node moves no audio golden: the defaults reproduce today's sound. Only the Modal screen goldens are re-recorded.
- **Ship:** one hardware flash with a bench row per model.

## Provenance

- **From Rings, under the MIT notice (ADR 0032):** the SYMP chord table and the dispersion approach. The SVF bank's filter and mode-tuning math was already Rings-derived and stays so.
- **Ours:** `LoopGain`, the DC blocker placement, the fractional tuning, the body filter, the ensemble, the release ramp and the macro mapping. (amended 2026-09-30, owner) Also ours: the bow's velocity scaling, its position comb and its loop low-pass.
- A new ADR records the macros, the FDBK removal, the provenance split and the Modal macros becoming modulatable (a partial supersession of ADR 0010's "Modal settings are read at note-on").

## Closes

#191, #10, #50, #51, #163, #206.

## Out of scope

- The bow / blow / strike exciter and Bowed's move into it (step B). (amended 2026-09-30, owner) Step B grows EXC into the blended bow / blow / strike mixer.
- The RES / EXC / model pages' visuals and animation (step C).
- POLY / VOICES (#207); the SVF bank's internals; the Sympathetic pool.
