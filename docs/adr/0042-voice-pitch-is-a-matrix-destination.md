# 0042. Voice pitch is a matrix destination on each engine's PITCH page

- **Status:** Proposed
- **Deciders:** owner (#162, 2026-09-28), firmware

## Context
No matrix route could reach a voice's pitch, so vibrato and pitch-envelope
patches were impossible. Algo had a TRANSPOSE of its own; Modal had none. A
pitch that belongs to one engine would lose its routes on an engine switch.

## Decision
- **One block for the voice.** `BlockRef::Pitch` (`PitchParams`) lives in
  the Sound's `ParamSnapshot`, beside the engine blocks, not in any of them.
  The same route reaches it from every engine and survives an engine switch.
- **Two cells.** PITCH is a transpose, −24..=+24 semitones in steps of one.
  FINE is −100..=+100 cents. Both are Stepped: turning the encoder moves them
  a whole step, while a modulated copy stays fractional. Both are
  modulatable, and MIX+PLUS on the cell primes it.
- **The offset law.** An amount of ±127 moves PITCH ±24 semitones and FINE
  ±100 cents, linear in the unit (`OffsetLaw::Semitones(24)`,
  `OffsetLaw::Cents(100)`), clamped to the stored range. The MTX readout
  states the effect in the unit (`+32 = +6.0 st`, `+32 = +25 ct`). The block's
  full name there is `VOICE`, so the line reads `VOICE PITCH`, not
  `PITCH PITCH`; its tag is `PIT`.
- **The DSP.** After the matrix pass, each block, the voice's offset is
  `pitch + fine / 100` semitones. Algo adds it to the note's exponent where
  the ratio-1 phase increment is computed, so every operator scales by
  2^(st/12), stepped per block as the note already is. Modal scales its
  resonator bank's frequency every block, and retunes its strings (main and
  sympathetic) when the ratio changes. At exactly 0 no extra maths runs, so
  every audio golden stays bit-identical.
- **The page.** PIT (`PITCH`) is a sub-page of each engine's home node: ALG
  on Algo, MDL on Modal (after MDL2), and VA's when that engine lands. A is
  PITCH, B is FINE. C–F stay empty for GLIDE (portamento rate) and pitch
  SLEW, which are out of scope here.
- **Cost.** It is billed as any destination, through the DEST terms. The
  work is per block only: Algo's increments are already computed per block,
  and Modal's filters already recompute per block. A string retune is one
  division per string, and only when the ratio changes.

## Alternatives considered
- Route Algo's TRANSPOSE: it is Algo's alone, so a route to it would not
  reach Modal or survive an engine switch. Its range is also 48 semitones,
  which the linear law would give to ±127.
- One cent-resolution PITCH cell: coarse transposes would take 2400 ticks.
- A per-sample pitch ramp: smoother vibrato, but it costs a term per sample,
  and the note itself steps per block today.

## Consequences
Vibrato and pitch envelopes are one route away on every engine. Modal's
strings have integer delay lengths, so on them a routed pitch moves in
one-sample steps of the period. Vibrato there is coarse at high notes, and
fixing that needs fractional delay reads. The PIT page has four free cells
for GLIDE and SLEW.

## Sources
- Issue #162 and the owner's comment on it.
- `chimera-core/src/params.rs` (`PitchParams`, `PITCH_SPECS`),
  `chimera-core/src/block.rs` (`OffsetLaw`),
  `chimera-core/src/dsp/algo/engine.rs` (`cycles`),
  `chimera-core/src/dsp/modal/mod.rs` (`set_pitch`, `retune`).
- Tests: `chimera-core/tests/pitch_test.rs`, `pitch_page_test.rs`.
