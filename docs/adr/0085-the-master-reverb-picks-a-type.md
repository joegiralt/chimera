# 0085. The master reverb picks a TYPE: QUAD plus ROOM, HALL, GATE, REVERSE, BLOOM and ECHO

- **Status:** Proposed; supersedes in part [0028](0028-one-alesis-style-reverb.md) (its "one reverb" and "TYPE retires"); amends [0067](0067-focus-glyphs.md) (the reverb composite)
- **Deciders:** project owner (2026-10-03 rulings on #343; 2026-10-04: QUAD stays exactly as it is and keeps its name; modulation deferred to #345; "sound first" on CPU); firmware

## Context
ADR 0028 cut the FX bus to one reverb, the Alesis-style ring, and retired
TYPE. The cut was for cost: three reverbs by TYPE, each one costly. The ring
reads 485 cycles per 48 kHz output sample at its worst, on the bench's
REVERB row (ADR 0031).

The owner now wants more spaces, though not at that price. The rulings of
2026-10-03:
- a TYPE setting, with only the selected model running;
- TYPEs that are families, not presets;
- no TYPE costing more CPU than the ring's worst case. This was amended on 2026-10-04 by "sound first": that cost became the target, and the hard limit became the system worst case;
- one shared delay arena that does not grow;
- about 1–3 KB of flash per TYPE;
- a switch with no click and no stale echo;
- sound names on screen;
- the method: study the Midiverb II through github.com/thement/midiverb_emulator
  (GPL-3.0) as documentation only, and write our own code;
- extensions the hardware never had: wobble, modulation and tempo sync.

The #343 spike grouped the Midiverb II's 100 programs by dataflow graph. It
found three reverb engines: diffuse loops, tap bursts and an allpass bloom.
It counted every proposed engine at ≤ 45 line accesses per ring sample,
against the ring's 48 while crossfading.

## Decision
- **The TYPEs.** The master reverb has a TYPE (Reverb ParamId 6, a
  `ReverbType` enum with explicit disk codes). The values, with their codes:
  - QUAD 0: today's ring, unchanged;
  - ROOM 1: the LOOP engine, 2 loops × 2 stages;
  - HALL 2: the LOOP engine, 1 loop × 4 stages;
  - GATE 3: the BURST engine;
  - REVERSE 4: the BURST engine;
  - BLOOM 5: the BLOOM engine;
  - ECHO 6: the ECHO engine.

  QUAD keeps the word QUAD on screen (the owner's name). The screen lists
  them QUAD, ROOM, HALL, BLOOM, GATE, REVERSE, ECHO.
  ParamId 0 stays retired (ADR 0009).
- **One engine runs at a time.** All TYPEs share the 23,220-sample i16
  arena (46,440 B), lifted out of `Ring` unchanged. Each TYPE's line layout
  is a `const` asserted to fit. All TYPEs run at half rate behind the
  existing half-bands, with GRIT's grid and ADR 0028's Q3 quantiser.
- **PLATE is not a TYPE.** It is HALL's corner: the same graph and only
  other constants, so by the owner's own rule it is a preset. HALL's SIZE
  reaches down to a 120 ms loop. The corner is a tested golden.
- **QUAD stays exactly as it is** (owner, 2026-10-04): the same sound, the
  same cost, and its double-read SIZE crossfade, which is clean and
  pitch-free. Only the new TYPEs glide SIZE (and every other moving length)
  through linearly interpolated read pointers. The glide is a 100 ms
  one-pole, slew-limited to ⅛ sample per sample. Its slight pitch bend is
  part of their vintage character.
- **CPU: "sound first"** (owner, 2026-10-04).
  - QUAD's worst case is the **target**: 485 cycles per output sample,
    239 instructions per ring sample.
  - The **hard limit** is the measured system worst case: 8 voices of the
    costliest patch, every FX at its worst, ORBIT running and the UI. It
    must stay inside the audio budget, 70 % of the CPU
    (`AUDIO_BUDGET_PERCENT` in `hw.rs`, ADR 0020), which is 7,000 cycles
    per sample on rev V. That keeps the **30 % reserve** for UI, MIDI and
    interrupts whole.
  - A TYPE may exceed the target when the A/B shows its cheaper design
    sounds wrong and the system worst case still keeps the reserve.
  - Cheaper fallback designs are a last resort: they are used only if the
    reserve would be breached, or the A/B hears no difference.
- **Moving reads are few and costed.** A read whose length changes within a
  block costs about 7 instructions more than two fixed reads, because it
  works out its index per sample instead of using `ring.rs`'s wrap-free
  segments. So each new TYPE keeps 5–6 of them:
  - output taps are fixed, at fractions of each line's maximum length;
  - BURST moves at most 4 of its taps a block, by a scheduler (tap 0 is
    the input register, and tap 11 doubles as REGEN's read). Taps 1–10
    slide at up to ½ sample per sample, and tap 11 at ⅛, so REGEN's tail
    bends no more than the loops' glide does. A tap stops moving only on
    an integer slot;
  - BLOOM's SWELL moves one loop delay, not its ten allpasses. Whether
    that long delay echoes instead of swelling is an A/B item, with a
    fallback that puts an allpass inside it.

  Every read stays ≥ 2 samples behind the write head, and wobble depth is
  capped at half the shortest length it moves.
- **Loops decay by construction.**
  - Any loop without an allpass (ECHO, REVERSE's REGEN) truncates its
    feedback write, so no ±Δ limit cycle survives.
  - ROOM couples its two loops through an energy-preserving rotation.
  - New engines cap a stage gain at 0.985 (QUAD keeps 0.97), so HALL
    reaches 10 s at its smallest SIZE.
- **One block, semantic slots.**
  - The Reverb block stays one static table, of 14 ids out of the codec's 32.
  - Ids 1–5 keep their idents. Ids 7–14 are TONE, DIFFUSE, PREDELAY, SHAPE,
    WOBBLE, RATE, SYNC and PATTERN.
  - A per-TYPE `const` panel gives each slot its label, range, unit and
    encoder step, or marks it absent (ADR 0037's pattern).
  - Slot 3's static step is 1/128. QUAD's panel keeps its 1/31 tick, and
    `size_step` still rounds to 32, so QUAD's goldens hold.
  - A UI edit of TYPE loads that TYPE's starting knobs and keeps MIX, GRIT
    and SYNC. Decode and CC writes store TYPE alone.
- **The switch.**
  1. The old engine runs on knobs latched when the switch starts, since
     the UI has already written the new TYPE's knobs. Its return ramps
     linearly to 0 over 20 ms.
  2. One block clears the whole arena (about 12 k cycles, under the
     reverb's 31 k per-block share) with no engine running. The engine
     state and the interpolators' histories are cleared too.
  3. The new engine starts fresh and ramps in over 20 ms.

  A test holds the new engine bit-equal to a fresh engine from its first
  block, and a host test bounds the peak block's cost.
- **The extensions.**
  - A wobble LFO on every new TYPE.
  - No external mod destinations (owner, 2026-10-04). Every mod source
    lives inside a Part, and nothing exists at the mixer level yet; a
    mixer-level LFO or matrix is
    private issue #345. Matrix routes,
    a send DUCK and MIX+PLUS priming on reverb cells are deferred there.
    Adding destinations later changes no disk code, since routes are stored
    by `ParamAddr`. #345 must still:
    - supersede ADR 0010, which allows only params `Voice` reads per block;
    - decide what a semantic slot means as a destination (SIZE under one
      TYPE, LENGTH under another).
  - SYNC (OFF, TIME, ALL), on ORBIT's clock: PRE-DLY, LENGTH and ECHO TIME
    in note values, and DECAY in beats under ALL. A synced time halves or
    doubles to fit its TYPE's range, so the arena never grows.
    - The fold has 2 % hysteresis.
    - The tempo is quantised to 0.25 BPM.
    - A division is stored as its index over the slot's 0..1, and a UI
      SYNC toggle converts the slot so the sound stays close.
    - ROOM has no ALL.

    QUAD takes none of these.
- **The glyph.** `CompositeId::ReverbCube` becomes `CompositeId::Reverb`.
  It draws the cube under QUAD and a TAIL envelope under the other TYPEs,
  from set values only. This amends ADR 0067's "the reverb shows the cube".
- **No hints.** The reverb adds no gesture, so under ADR 0086 (Proposed,
  on the ORBIT branch) no reverb cell has a hint. A folded synced time is
  marked in its value text.
- **Projects.** Old projects lack id 6 and load as QUAD, sounding as
  before. New projects default to QUAD.
- **Provenance.** No code, instruction listing or ROM coefficient, offset or
  tap table from the emulator or from commit 7119f7ad enters the repository,
  whole or in part. Lengths are our own primes from `const fn`. Gains come
  from RT60 formulas. Tap positions, signs and ranges are chosen in the
  spec, then tuned by ear and by measurement against the Midiverb VST. Only
  prose structure and acoustic measurements carry over. Every engine task's
  review checks this.

## Alternatives considered
- **One "80s" Midiverb model (the issue's first framing):** a single
  graph cannot be a room, a gate and a bloom. The families are what the
  hardware's presets actually were.
- **A faithful MV2 port, reviving 7119f7ad:** GPL-derived code and tables
  in what was then an MIT workspace (GPL-3.0-or-later since ADR 0330). It was also retired once already, for cost.
- **PLATE as its own TYPE:** a preset presented as a family. It could be
  added later as a new code mapped to HALL, for about 200 B.
- **Per-TYPE parameter ids:** about 56 ids against the codec's 32-param
  cap. Shared slots plus panels need 14.
- **Crossfading SIZE in the new TYPEs, as QUAD does:** 60–75 accesses,
  over QUAD's worst. QUAD alone keeps it, by the owner's ruling.
- **Running both engines through a switch crossfade:** twice the cost for
  20 ms, over budget. Fading out, clearing and fading in costs one engine.
- **Gliding every tap and every allpass:** about 15 moving pairs in BURST
  (≈ 330 instructions) and 10 in BLOOM (≈ 282), both against QUAD's 239.
- **SIZE-scaled output taps in ROOM and HALL:** 6 more moving pairs, over
  budget. The ear judges the fixed taps at A/B.
- **Clearing the arena in quarters over 4 blocks:** it was meant to keep
  the per-block cost down, but one block (≈ 12 k cycles) is already under
  the reverb's per-block share, and one block means fewer blocks of
  silence.
- **Growing the arena by about 800 samples, so ECHO fits a quarter note at
  60 BPM:** the owner ruled that nothing grows. Halving to fit stays on the
  grid.

## Consequences
- **CPU.** The estimates count a moving pair at 17–20 instructions
  (index work of +7 to +10). Host instructions per ring sample, against
  QUAD's 239 (ADR 0031):

  | TYPE | Pair at +7 | Pair at +10 |
  |---|---|---|
  | ROOM | 224 | 239 (at the limit) |
  | HALL | 220 | 235 |
  | GATE, REVERSE | 212 | 230 |
  | BLOOM | 220 | 235 |
  | ECHO | 147 | 165 |

  All of them meet the target by estimate. As a last resort, each TYPE has
  a concrete fallback design that saves 12–30 instructions:
  - ROOM and HALL glide SIZE on two delays only;
  - BURST moves 3 taps;
  - BLOOM wobbles 2 allpasses.

  PRE-DLY always glides.

  The target figures date from 2026-09-27, and step 0 re-measures them as
  an absolute baseline: after the refactor, REV QUAD must read ≤ that
  baseline + 1 %. Step 0 also:
  - measures a moving pair's cost;
  - benches the worst-case BURST, ROOM-loop and HALL-loop kernels on the
    chip, before any TYPE work;
  - measures the system worst case on the unit with QUAD, which gives the
    headroom.

  The gates are absolute:
  - the system worst case, re-measured as each TYPE lands, must keep the
    reserve;
  - each TYPE's bench row is recorded against the target.

  `FxBus::COST` is unchanged while every TYPE ≤ QUAD. A dearer TYPE raises
  it to the new BUS reading, and the voice counts are re-checked.
- **Memory.** The arena does not grow. `FxBus` gains one engine-state enum
  (≤ 160 B), and `FxParams` about 28 B.
- **Flash.** Estimated 11–13.5 KB in all: about 3–4 KB of shared scaffold,
  then 1–2.5 KB per TYPE. Each step's fat-LTO delta is recorded, and a TYPE
  over 3 KB is cut back before the next one starts.
- **Interactions.** A TYPE switch drops about 1.3 ms of send and dips the
  return for about 41 ms. Sweeps list this as by design. #142's quiet-send tail
  loss applies to every TYPE alike.
- **Order.** Step 0 (the baseline and the BURST spike) comes first, and
  tempo sync waits for ORBIT on `main`. ADR 0085 is accepted once every
  A/B pair passes the spec's criterion, the system worst case with every TYPE keeps the 30 % reserve, (each time metric within ±30 % of
  the named Midiverb plugin's, or an owner-signed ear exception) and UAT
  passes, with measured CPU and flash recorded here.
- **What it rules out.** FREEZE, per-TYPE knob memory and external
  modulation of the reverb, unless a later ADR adds them (#345 for the
  last).

## Sources
- `docs/superpowers/specs/2026-10-04-reverb-types-design.md`.
- private issue #343: the owner's rulings, 2026-10-03, and the naming ruling.
- The #343 research spike, kept outside the repository; its prose and measurements are summarised in the spec.
- github.com/thement/midiverb_emulator (GPL-3.0), read as documentation only.
- M. R. Schroeder, "Natural sounding artificial reverberation", JAES, 1962.
- W. G. Gardner, "A realtime multichannel room simulator", 1992.
- J. Dattorro, "Effect Design, Part 1: Reverberator and Other Filters", JAES, 1997.
- ADRs 0009, 0010, 0011, 0028, 0031, 0037, 0045, 0061, 0067; ORBIT's 0071, 0072, 0080, 0081, 0083, 0086 (orbit branch).
- `chimera-core/src/dsp/ring.rs`, `reverb.rs`, `fx_bus.rs`; `chimera-stm32/src/bench.rs`.
