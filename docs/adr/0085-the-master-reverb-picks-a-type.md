# 0085. The master reverb picks a TYPE: QUAD plus ROOM, HALL, GATE, REVERSE, BLOOM and ECHO

- **Status:** Proposed; supersedes in part [0028](0028-one-alesis-style-reverb.md) (its "one reverb" and "TYPE retires"); amends [0067](0067-focus-glyphs.md) (the reverb composite)
- **Deciders:** project owner (2026-10-03 rulings on #343; 2026-10-04: QUAD stays exactly as it is); firmware

## Context
ADR 0028 cut the FX bus to one reverb, the Alesis-style ring, and retired
TYPE. The cut was for cost: three reverbs by TYPE, each one costly. The ring
reads 485 cycles per 48 kHz output sample at its worst, on the bench's
REVERB row (ADR 0031).

The owner now wants more spaces, though not at that price. The rulings of
2026-10-03:
- a TYPE setting, with only the selected model running;
- TYPEs that are families, not presets;
- no TYPE costing more CPU than the ring's worst case;
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

  The screen lists them QUAD, ROOM, HALL, BLOOM, GATE, REVERSE, ECHO.
  ParamId 0 stays retired (ADR 0009).
- **One engine runs at a time.** All TYPEs share the 23,220-sample i16
  arena (46,440 B), lifted out of `Ring` unchanged. Each TYPE's line layout
  is a `const` asserted to fit. All TYPEs run at half rate behind the
  existing half-bands, with GRIT's grid and ADR 0028's Q3 quantiser.
- **PLATE is not a TYPE.** It is HALL's corner: the same graph and only
  other constants, so by the owner's own rule it is a preset. HALL's SIZE
  reaches down to a 120 ms loop. The corner is a tested golden, and a
  focus-band hint names it.
- **QUAD stays exactly as it is** (owner, 2026-10-04): the same sound, the
  same cost, and its double-read SIZE crossfade, which is clean and
  pitch-free. Only the new TYPEs glide SIZE (and every other moving length)
  through linearly interpolated read pointers. The glide is a 100 ms
  one-pole, slew-limited to ⅛ sample per sample. Its slight pitch bend is
  part of their vintage character. The new engines always read
  interpolated, so their steady cost equals their worst.
- **One block, semantic slots.**
  - The Reverb block stays one static table, of 14 ids out of the codec's 32.
  - Ids 1–5 keep their idents. Ids 7–14 are TONE, DIFFUSE, PREDELAY, SHAPE,
    WOBBLE, RATE, SYNC and PATTERN.
  - A per-TYPE `const` panel gives each slot its label, range and unit,
    or marks it absent (ADR 0037's pattern).
  - A UI edit of TYPE loads that TYPE's starting knobs and keeps MIX, GRIT
    and SYNC. Decode and CC writes store TYPE alone.
- **The switch.** The old engine's return ramps linearly to 0 over 20 ms.
  No engine runs while the arena clears a quarter per block, over 4 blocks,
  with the engine state and the interpolators' histories cleared too. The
  new engine starts fresh and ramps in over 20 ms. A test holds it bit-equal
  to a fresh engine from its first block.
- **The extensions.**
  - A wobble LFO on every new TYPE.
  - Four modulation inputs per new TYPE: size, decay, tone and wobble depth.
    Their source is an owner question in the spec, and its wiring gets its
    own ADR amending ADR 0010.
  - SYNC (OFF, TIME, ALL), on ORBIT's clock: PRE-DLY, LENGTH and ECHO TIME
    in note values, and DECAY in beats under ALL. A synced time halves or
    doubles to fit its TYPE's range, so the arena never grows. QUAD takes
    none of these.
- **The glyph.** `CompositeId::ReverbCube` becomes `CompositeId::Reverb`.
  It draws the cube under QUAD and a TAIL envelope under the other TYPEs,
  from set values only. This amends ADR 0067's "the reverb shows the cube".
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
- **Clearing the arena in one block:** about 12 k cycles in one block,
  over the reverb's per-block share. Four quarters stay inside it.
- **Growing the arena by about 800 samples, so ECHO fits a quarter note at
  60 BPM:** the owner ruled that nothing grows. Halving to fit stays on the
  grid.

## Consequences
- **CPU.** Estimated worst cases in cycles per output sample, against
  QUAD's measured 485:
  - ROOM and HALL ≈ 440;
  - GATE and REVERSE ≈ 460;
  - BLOOM ≈ 420;
  - ECHO ≈ 235.

  The gate is a bench row per TYPE (REV QUAD … REV ECHO, plus REV SWITCH),
  each ≤ REV QUAD on the same build. The spec orders fallbacks (fewer taps)
  if one reads over. `FxBus::COST` is unchanged while every TYPE ≤ QUAD. The
  BUS row runs the costliest TYPE.
- **Memory.** The arena does not grow. `FxBus` gains one engine-state enum
  (≤ 160 B), and `FxParams` about 28 B.
- **Flash.** Estimated 11–14 KB in all: about 3–4 KB of shared scaffold,
  then 1–2.5 KB per TYPE. Each step's fat-LTO delta is recorded, and a TYPE
  over 3 KB is cut back before the next one starts.
- **Interactions.** A TYPE switch drops about 5 ms of send and dips the
  return for 45 ms. Sweeps list this as by design. #142's quiet-send tail
  loss applies to every TYPE alike.
- **Order.** Tempo sync waits for ORBIT on `main`, and modulation waits for
  the owner's answer on its source. ADR 0085 is accepted after the A/B and
  UAT pass, with measured CPU and flash recorded here.
- **What it rules out.** FREEZE and per-TYPE knob memory, unless a later ADR
  adds them.

## Sources
- `docs/superpowers/specs/2026-10-04-reverb-types-design.md`.
- private issue #343: the owner's rulings, 2026-10-03, and the naming ruling.
- The #343 research spike, kept outside the repository; its prose and measurements are summarised in the spec.
- github.com/thement/midiverb_emulator (GPL-3.0), read as documentation only.
- M. R. Schroeder, "Natural sounding artificial reverberation", JAES, 1962.
- W. G. Gardner, "A realtime multichannel room simulator", 1992.
- J. Dattorro, "Effect Design, Part 1: Reverberator and Other Filters", JAES, 1997.
- ADRs 0009, 0010, 0011, 0028, 0031, 0037, 0045, 0061, 0067; ORBIT's 0071, 0072, 0080 (orbit branch).
- `chimera-core/src/dsp/ring.rs`, `reverb.rs`, `fx_bus.rs`; `chimera-stm32/src/bench.rs`.
