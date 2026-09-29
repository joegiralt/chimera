# 0055. The master tape stage is off the chain

- **Status:** Proposed
- **Deciders:** owner; firmware
- **Supersedes in part:** [0030](0030-master-section.md) (its tape on DAC
  pair 1; REV SEND and the linked compressor stand)

## Context
The FX bus is reserved from the voice budget whether or not an effect is
on (ADR 0031). After the output limiter (ADR 0050) and MECHANICS
(ADR 0053) it read BUS 1,495 cycles per sample, and 1,469 once the limiter
scaled the DAC's own block and the delay kept its loop state in locals.
Six voices of the costliest patch on rev V, and five on rev Y, need about
1,388 or less. The master tape on pair 1 is the bus's second-largest
stage: TAPE 280 cycles per sample on the ADR 0031 bench (rev V at 480 MHz),
about a fifth of the bus. The owner would rather spend the cycles on a
beefier tape delay, where the tape character is heard most.

## Decision
- The master section is the compressor, linked across the three pairs,
  then the output trim and limiter (ADR 0050). The tape no longer runs.
- The code stays, behind the `master-tape` cargo feature, off by default
  (`chimera-core`; `chimera-stm32` and `chimera-desktop` forward it). The
  feature gates the `Tape` DSP (`dsp/tape/stage.rs`), its `FxBus` field
  and its line in the field list, its call in `FxBus::master`, the bench's
  TAPE row and the Mix chain's TAPE page. With it on, the chain, the bench
  and the screens are as before.
- The data model is untouched: `TapeParams`, `TAPE_SPECS`, their idents,
  `BlockRef::Tape` and its disk codes stay compiled in every build, so a
  Performance keeps its TAPE settings, the frozen disk-code golden is
  unchanged and nothing moves to the retired lists. Without the feature
  those settings are inert: pair 1 is bit-identical at any tape MIX.
- Next, by the owner's plan: a beefier tape delay; then, maybe, a small
  warmth stage at the very end of the chain, decided by ear.

## Alternatives considered
- **Delete the tape.** It is tested, tuned and may come back as the
  end-of-chain warmth; the feature keeps it building and tested for the
  cost of a few `cfg`s.
- **Keep it and shed voices.** ADR 0031's six voices on rev V is the
  promise; the tape's character on one pair is worth less than a voice.
- **A cheaper tape (no oversampling, one filter).** A different sound, to
  be judged by ear; the warmth stage later is where that belongs.

## Consequences
- The bus loses the tape's 280 cycles per sample on the bench; `FxBus::COST`
  moves only when a bench reading (`bench-notape`) replaces it.
- AXI: `FxBus` shrinks by the tape's state, 2,536 B (162,216 → 159,680 B);
  `AXI_RESIDENT` follows `size_of::<FxBus>()`.
- Sound: pair 1 loses the tape's saturation, head bump and roll-off. At
  typical levels nothing else changes; every audio golden is unchanged
  (none runs the tape at MIX above 0). SAW LEAD at eight voices and FX
  send 1 now reaches 1.50 of full scale after the trim, where the tape's
  clip held it under 1; the limiter keeps it under the ceiling
  (`no_stage_exceeds_ceiling`).
- UI: the Mix chain is PRT, SND, CHR, DLY, REV, MST; the seven Mix screen
  goldens were re-recorded for the map without TPE, and `mixer_tape` runs
  only with the feature.
- Tape tests (`tape_test.rs`, the clip's corner check) run only with the
  feature; `just check` builds, tests and lints the feature-on build too.

## Sources
ADR 0030 (master section), ADR 0031 (bench: TAPE 280, BUS 1,356), ADR 0050
and ADR 0053 (their bench figures); `chimera-core/src/dsp/tape/stage.rs`,
`chimera-core/src/dsp/fx_bus.rs`, `chimera-stm32/src/bench.rs`.
