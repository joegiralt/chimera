# Modal 2, step A: the resonators

Sub-project 2 of Modal 2 (#41). Builds on exclusive state (sub-project 1, PR #213). Step B is the blended bow / blow / strike exciter; step C is the RES / EXC / model pages' visuals and animation.

(amended 2026-09-30, owner) Step A now also gives the exciters their own node, EXC, first in the chain. It shows the exciter each model already has; step B grows it into the blended mixer. See the amended § 1, § 2 BOWED and § 3.

(amended 2026-09-30, owner: #240) Step A also rebuilds Bowed as a real bowed string: two delay lines meeting at the bow, after Smith's digital-waveguide model. It plays at its note, POS is the bow's position and BRIGHT is heard. See the amended § 1 Bowed row, § 2 BOWED, § 3 and the tests.

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
- EXC's controls are not modulatable, like the model page. EXCITE, COLOR and BURST are read at note-on. (amended 2026-09-30, controller) FORCE and SPEED are read every block and eased, so turning them moves a held bow. Step B's mixer decides what becomes modulatable.

### Home page (RES)

`MODEL · STRUCTURE · BRIGHT · DAMP · POS · SPACE`

| | STRUCTURE | BRIGHT | DAMP | POS |
|---|---|---|---|---|
| BANK | Mode stretch, harmonic to bell (today's INHARM) | High-mode damping (today's BRIGHT) | Ring time, thud to about 20 s (today's DECAY) | Strike point; nulls the modes it lands on |
| STRING | Stiffness: real dispersion, nylon to piano wire | Loop low-pass, gut to steel | Short pluck to near-endless sustain | Pluck point |
| SYMP | Steps a chord table for the 7 halo strings | Main string and halo | Main string and halo | Main string's pluck point |
| BOWED | dimmed | The bridge's reflection low-pass, clearly heard (amended 2026-09-30, owner: #240) | The ring after the bow lifts (amended 2026-09-30, owner) | Bow position: where the bow splits the string, bridge to middle; the pitch holds (amended 2026-09-30, owner: #240) |

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

- It gets the DC blocker on its output, and a low note sounds from its first block and is not freed while bowed (#206).
- ~~Unchanged sound.~~ ~~A one-loop stick-slip with POS as an output comb.~~ (amended 2026-09-30, owner: #240) The one-loop bow inverted its wave on every pass, so it played an octave low (C3 at 65 Hz), BRIGHT moved it under 0.5 dB, and reading a second tap moved its pitch. Bowed becomes a real bowed string:
  - **The model.** The bow splits the string into two delay lines, bow to bridge and bow to nut, each a round trip. They meet at the bow junction, where a friction curve (the bow table) sets how much of the bow's velocity the string takes. Each end reflects inverted, so a wave comes back upright once a period. This is J. O. Smith's digital-waveguide bowed string (CCRMA, *Physical Audio Signal Processing*); the implementation is Chimera's own, and no STK code, constant or table is used.
  - **Pitch.** The two lines, the bridge filter's one sample and the tuning allpass's fraction make one period, so C3 plays 130.8 Hz. The fraction goes through the existing allpass and `split`. G1 to C7 plays within ±2 cents.
  - **Memory.** The two lines share the string's existing ring: one ring holds the bridge line's cells and then the nut line's, spliced at the bow. D2 does not grow.
  - **POS** is the bow's position β, from near the bridge (0.06 of the string) to the middle (0.5). It splits the loop's length between the two lines; their sum is the period, so POS moves the tone, never the pitch. It is eased like the other macros, and the split glides a whole sample at most every 32 samples, as the STRUCTURE glide does, so a sweep does not click.
  - **BRIGHT** is the bridge's reflection low-pass: the linear-phase three-tap `c/2·(x[n] + x[n−2]) + (1 − c)·x[n−1]` at `c = 0.5·(1 − BRIGHT)`. Its delay is one sample at every frequency, taken off the line, so the pitch holds. From BRIGHT 1 to 0 the upper harmonics fall by 3 dB or more.
  - **DAMP** is the ring after the bow lifts, as before: at note-off the loop's gain (a `LoopGain`, at the bridge) ramps from its bowed gain to DAMP's T60 and never gives the gain back. While the bow is on, the bow sustains the string.
  - **FORCE** is the bow's pressure: it sets the friction curve's width, so more force holds the string longer before it slips. **SPEED** is the bow's velocity. Both are read every block and eased while the bow is on (amended 2026-09-30, controller), and the force is still FORCE × (0.5 + 0.5 × velocity), so a soft key sounds. At note-off the bow lifts.
  - **Stability by construction.** The loop's gain is a `LoopGain` below 1 at the bridge, the nut reflects at −1, the bridge filter's gain is at most 1, and the friction's push is bounded, so the string stays bounded. The output keeps its DC blocker.
  - **Cost.** No more than the one-loop bow's bill (`COST_BOWED` 860), block at a time. The audio thread does not allocate, block or panic.
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
  - ~~Old Bowed patches stored BRIGHT, DAMP and POS but never read them. They load as the old sound: BRIGHT 1, POS 0, and DAMP at the old lifted-bow ring of 0.12 s.~~ (amended 2026-09-30, owner: #240) The old Bowed sound was the bug, so old Bowed patches load as an in-tune bowed string instead: POS 0.15 (the bow about an eighth of the string from the bridge), BRIGHT 0.5 (a moderately lossy bridge), DAMP at a 0.5 s ring after the lift, and FORCE and SPEED at their defaults of 0.5.
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
| ~~`bowed_pos_and_bright_keep_pitch`~~ (amended 2026-09-30, owner: #240) | Replaced by `bowed_pos_moves_the_tone_not_the_pitch` and `bowed_bright_is_heard`. |
| ~~`a_v1_bowed_patch_bows_as_before`~~ `a_v1_bowed_patch_bows_in_tune` (amended 2026-09-30, owner: #240) | A v1 Bowed patch plays at its note, at about its old level, and its held note and release are pinned bit for bit, re-recorded deliberately: the old sound was the bug. |
| `bowed_is_in_tune` (amended 2026-09-30, owner: #240) | Bowed from G1 to C7, within ±2 cents, with the fundamental at the note and not an octave low. Today's bow fails it. |
| `bowed_pos_moves_the_tone_not_the_pitch` (amended 2026-09-30, owner: #240) | POS from 0 to 1 keeps the fundamental within 2 cents of the note, and moves the harmonics the bow point nulls. |
| `bowed_bright_is_heard` (amended 2026-09-30, owner: #240) | BRIGHT 0 against 1 moves the upper harmonics by 3 dB or more. Today's bow moves them under 0.5 dB. |
| `a_bowed_pos_sweep_does_not_click` (amended 2026-09-30, owner: #240) | A square LFO sweeping POS end to end steps no further than the bow's own corner, and the pitch holds. |
| `bowed_is_stable_and_in_tune_at_every_corner` (amended 2026-09-30, owner: #240) | FORCE, SPEED, BRIGHT and POS at their ends, G1 to the top note: bounded, no growth, no DC, and each sounding corner at its note. |
| `old_modal_patches_translate` | `init_modal.snd` and a v1 patch per model decode to the mapped values. (amended 2026-09-30, owner) This includes EXC's defaults, BURST from EXCITE on BANK, and Bowed's neutral macros. |
| cost and memory tests | New `COST_*` values; `Instrument` still fits D2. |

- The Modal goldens (`modal_init`, `modal_lfo_cutoff`, `modal_sympathetic`, `algo_to_modal_switch`) and the `init_modal.snd` render hash are re-recorded once, deliberately. Their `KNOWN_BROKEN` #10 entries go.
- **Listening:** the reel's Modal clips are re-rendered on the desktop as each model lands, so the owner hears each change without a flash.
- (amended 2026-09-30, owner) The EXC node moves no audio golden: the defaults reproduce today's sound. Only the Modal screen goldens are re-recorded.
- (amended 2026-09-30, owner: #240) The two-delay bow re-records the v1 Bowed pins and the Bowed demo clips, deliberately. No other audio golden moves.
- **Ship:** one hardware flash with a bench row per model.

## Provenance

- **From Rings, under the MIT notice (ADR 0032):** the SYMP chord table and the dispersion approach. The SVF bank's filter and mode-tuning math was already Rings-derived and stays so.
- **Ours:** `LoopGain`, the DC blocker placement, the fractional tuning, the body filter, the ensemble, the release ramp and the macro mapping. (amended 2026-09-30, owner) Also ours: the bow's velocity scaling. ~~Its position comb and its loop low-pass.~~ (amended 2026-09-30, owner: #240) Also ours: the bow table's curve, the one-ring splice that holds both lines, the split's glide and the bridge filter's mapping.
- (amended 2026-09-30, owner: #240) **From the published literature:** the two-delay bowed string, after J. O. Smith, *Physical Audio Signal Processing* (CCRMA, Stanford), "Bowed Strings", and McIntyre, Schumacher and Woodhouse (JASA, 1983). No STK code, constant or table is used, so THIRD_PARTY.md does not change.
- A new ADR records the macros, the FDBK removal, the provenance split and the Modal macros becoming modulatable (a partial supersession of ADR 0010's "Modal settings are read at note-on").

## Closes

#191, #10, #50, #51, #163, #206, and #240 (amended 2026-09-30, owner).

## Out of scope

- The bow / blow / strike exciter and Bowed's move into it (step B). (amended 2026-09-30, owner) Step B grows EXC into the blended bow / blow / strike mixer.
- The RES / EXC / model pages' visuals and animation (step C).
- POLY / VOICES (#207); the SVF bank's internals; the Sympathetic pool.
