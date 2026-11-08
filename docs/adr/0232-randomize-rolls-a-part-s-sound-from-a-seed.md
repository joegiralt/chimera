# 0232. RANDOMIZE rolls a Part's Sound from a seed, inside musical ranges

- **Status:** Proposed
- **Deciders:** firmware (owner's direction in #809: "musical by construction", stock parity, done better)

## Context
- Stock PreenFM3 has a patch randomizer that sounds musical. Its facts, read from Ixox/preenfm3 `SynthState.cpp` (GPL; facts only, no code copied), are in #809:
  - four amounts, each OFF/1/2/3: OPER, ENV TIME, IM (mod index), MODL (matrix);
  - ratios from small harmonic sets that widen per level, fine tune mostly 0;
  - carriers loud (0.6–1), envelopes by family (1 percussive, 2 slow pad, 3 anything);
  - mod index bounded per level; the matrix from a few fixed, sensible sources.
- Chimera has four engines (ALGO, VA, MODAL, TAPE), a Part's Sound is an edit like any other (Edited mark, RELOAD FROM PROJ), and bank 1 is frozen (ADR 0198).

## Decision
1. **Where:** SETTINGS › PART › RANDOMIZE, beside the Part's other actions: a leaf page of five cells, OSC, ENV, TONE, MOD and SEED.
   - The amounts are stock's four, named for what they do: OSC `OFF PURE RICH WILD` (OPER), ENV `OFF PLUCK PAD ANY` (ENV TIME's families), TONE `OFF SOFT BRIGHT WILD` (IM), MOD `OFF SUBTLE MOVING WILD` (MODL).
   - SEED is 0–255. Defaults: RICH, ANY, BRIGHT, SUBTLE, seed 0.
   - The values are the UI's (`BlockRef::Random`, `randomize::Randomize`), never stored.
2. **The gesture is the turn.** Any turn of any of the five cells rolls the active Part's Sound at once. Turning SEED is the re-roll; turning SEED back lands on the same Sound.
   - Each cell's focus band says so: `TURN  ROLLS THE SOUND`.
3. **A roll is an edit.** It writes the Part's Sound through the project, so the knobs glide to the new values (ADR 0115) and the page lerps. Nothing loads, the Part reads `* EDITED`, and RELOAD FROM PROJ undoes it. Nothing is saved until the Part is.
4. **A pure function:** `randomize::kern::roll(&mut Sound, &Randomize)`.
   - Deterministic: the same seed, amounts and Sound give the same bits.
   - Each dimension draws from its own seeded stream (xorshift32 behind a 32-bit finaliser), a fixed number of draws per operator whatever the others rolled, so changing one amount never shifts another dimension's draws.
   - On ALGO, ENV and TONE apply their draws by the roles OSC's algorithm gives the operators (a modulator decays quicker and takes TONE's depth), so an operator whose role changes takes other values. On every other engine, and for every operator whose role stays, one amount never moves another's values.
   - OFF leaves that dimension's params as they are.
   - Never touched: OUT LEVEL, PITCH, the sends, LFO 1 (the wheel's vibrato), ENV 1 → VCA, the filter's TYPE and MODE, TAPE's source, machine and place.
5. **The ranges, per engine:**
   - **ALGO**
     - OSC: the algorithm from the TX81Z's eight (PURE), plus the first A rows (RICH), or any of the 32 with a MORPH to a second (WILD).
     - Carriers sound in octaves (0.5, 1, 2), so the note keeps its pitch class. Their LEVEL starts at 99 and drops 1.5 dB for each carrier past the first, never under 80, so a stack sums about where one carrier does. Their waves are W1–W4.
     - Modulators take ratios from {0.5 1 2 4}, then {… 3 5}, then any up to 8 with a rare FINE. Their waves are W1–W8. Operators the algorithm leaves unused go to LEVEL 0. Every modulator takes SOFT's depth (TONE's step at amount 1), so a former carrier never modulates at its old level; TONE replaces it when it rolls.
     - ENV: every operator on one family; modulators decay a little quicker.
     - TONE: the modulators' LEVEL per amount (45–70, 55–82, 58–92), less 2.25 dB for each modulator past the first. FEEDBACK and VELOCITY go on modulators only.
   - **VA**
     - OSC: MODE (MIX; then SYNC and SOFT SYNC; then any), the WAVEs, and OSC 2 at an octave, then a fifth, then a fourth. FINE 2 is a few cents of detune; SUB, SHAPE and NOISE grow with the amount.
   - **MODAL**
     - OSC: STRUCT (≤ 0.3, 0.5, 0.6) and POS (≤ 0.3, 0.4, 0.5: struck past the middle a string loses its fundamental); the MODEL from RICH up. The exciter always strikes (EXCITE ≥ 0.6).
     - ENV: DAMP by family (0.25–0.5, 0.6–0.9, 0.25–0.9).
     - TONE: BRIGHT, BODY, COLOR and BURST.
     - MOD: the ensemble, its MIX at most 0.1 per amount (its detuned copies cancel the strike).
   - **TAPE:** no OSC (a sample's pitch and place are its own).
   - **Every engine**
     - ENV: ENV 1 by family on VA and TAPE, where it is the amp (A ≤ 0.7, R ≤ 0.8, a pad holding).
     - TONE: CUTOFF on a log scale (3–14 kHz, 1.5–16 kHz, 0.9–18 kHz; MODAL's WILD no darker than BRIGHT) and RESO (≤ 0.2, 0.4, 0.65).
     - MOD: LFO 2 and 3's rate and shape, ENV 2, and ENV 2 → FREQ, LFO 2 → FREQ and LFO 3 → Q in the fixed columns, each amount bounded. ENV 2 only ever opens the filter: closing a dark CUTOFF is silence.
6. **Bank 2.** `randomize::kern` is linked into bank 2 (`bank2.x`) and reached through the UI's door (`UiDoor::roll`, magic `0xB2C0_0008`). Closed, the UI lends no `Random` block, so a turn reaches no cell and writes nothing (not even the project's revision), and the five cells read `NEEDS UPDATE`. Bank 1 keeps the specs, the leaf, the dispatch and the hint, and the `Block` impl: its vtable is bank 1's rodata, which may not point into bank 2 (`objcopy -R .bank2` refuses the relocation), and shims would cost what the ~450 B impl does.
7. **PART's list scrolls.** RANDOMIZE makes PART eight rows with the MIDI map, one more than fit under the strip. The list now scrolls like every other SETTINGS list (`PART_ROWS`, 7).

## Alternatives considered
- **A hold or MIX combo on the engine page.** That would be one more hidden gesture, with nowhere to show the amounts or the seed. Stock's randomizer is a page with its amounts; so is this one.
- **A GO row instead of rolling on every turn.** That is an extra step for every re-roll. Turning SEED is the re-roll, and RELOAD is the undo.
- **Uniform random over each param's spec.** It mostly gives silence, noise or clipping, which is what #809 rules out.
- **A seed of 0–999.** `ValFmt::Int` is a byte. 256 seeds × 81 amount settings × each engine is plenty to browse.
- **A base Sound kept at the first roll, so OFF restores it.** It costs a whole Sound of RAM. RELOAD FROM PROJ already restores the saved Sound.

## Consequences
- Every engine rolls inside measured bounds. Through the whole engine, each roll's loudest tenth of a second sits −22.3..+10.0 dB from its engine's INIT, over all 256 amount settings × ALGO, VA and MODAL, from INIT and over an already-rolled Sound (`randomize_test`, gated at −24..+12). MODAL's top is BOWED, which sustains where INIT's STRING decays (−6 dBFS).
- Bank 1 grows by the leaf, the block's dispatch and the scroll (see the PR); the roll itself is bank 2's.
- A new engine needs its own rules in `kern::roll`. Until it has them, its OSC does nothing and the shared ENV/TONE/MOD rules apply.

## Sources
- #809 (owner's direction and stock's facts); Ixox/preenfm3 `SynthState.cpp` (GPL-3.0, read for facts only).
- ADR 0086 (gesture hints), 0115 (knob deltas), 0175 (the UI's door), 0198 (bank 1 frozen).
