# 0023. The waves come from our own recipes

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
The Algo engine needs band-limited wave tables in flash. The TX81Z's eight
waves are well described by simple piecewise-sine formulas; its ROM is not
ours to copy, and the ported FM code (the formulas and tables we had) is of
unverified licence.

## Decision
- A host-only crate, `chimera-waves`, holds each wave as a formula (or a
  harmonic series) and renders it: 256 `i16` samples per mip, 8 mips of
  constant length, each keeping half the harmonics of the one before
  (127, 64 … 1), one scale per wave so no mip is louder than another.
  `chimera-core`'s build script writes the tables; a build script cannot use
  the crate it builds, hence the separate crate.
- Sub-project 1 has 16 waves: TX81Z W1–W8 from their shapes (sine, the
  sharpened sine W2, and half-wave and doubled forms of the two), and
  triangle, saw, square, 25 % and 12 % pulse, trisaw, rounded square and
  soft saw.
- W3, W4, W7 and W8 keep their DC; every other wave is centred.
- The TX81Z's facts the engine uses (the 64 coarse ratios, the FINE targets,
  the 0.75 dB LEVEL step, D1L's 3 dB step) are re-entered from the owner's
  manual, and no code from the ported FM engine is carried over.
- The tables are 64 KB (plus one guard sample per table), checked against a
  flash budget at compile time.
- The mip is chosen by the ceiling of the bandwidth octave, not the floor:
  the two mips crossfaded both keep the operator's bandwidth
  (`fundamental × (1 + Σ incoming modulator level)`) under Nyquist, so the
  choice is alias-free, at the cost of the top octave of harmonics.
- A new note on a silent voice (an idle operator) sets its mip directly.
  Otherwise the mip position slews at most one mip (one octave) per block,
  so it never steps. When a sounding voice retriggers higher, or LEVEL or
  MORPH sweeps a modulator up, the slew lags the bandwidth and can alias
  briefly. This is a known limit, accepted and parked.

## Alternatives considered
- **Dump the TX81Z ROM:** not ours; no.
- **Keep the ported wave code:** licence unverified, and it computed `sinf`
  per sample.
- **Halving mip lengths:** a quarter of the flash, but the indexing and
  interpolation differ per mip; 64 KB fits.
- **The floor of the bandwidth octave:** keeps one more octave of
  harmonics, but the upper half of each octave folds down.
- **Jump the mip on every retrigger or sweep:** no lag, but a step in the
  sound of a voice that is already sounding.

## Consequences
Below 187.5 Hz, mip 0 cannot hold every harmonic up to Nyquist; a low saw is
duller than an analogue one. Phase-modulation sidebands alias, as on the
TX81Z. A retrigger or a fast upward sweep can alias for a few blocks while
the mip catches up. Sub-project 3 grows the set to 64 waves.

## Sources
`chimera-waves/src/lib.rs`; `chimera-core/build.rs`;
`chimera-core/src/dsp/algo/{waves,engine}.rs`;
`docs/superpowers/specs/2026-09-26-algo-engine-design.md` § Addendum:
kernel bench result and rulings; TX81Z owner's manual (waveform chart,
frequency-ratio chart).
