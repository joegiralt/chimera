# 0100. chimera-utils takes its paths from the environment; ported third-party code credited (supersedes in part 0098)

- **Status:** Accepted (2026-10-05); supersedes in part [0098](0098-chimera-utils-host-reference-tool.md) (its `.env` clause)
- **Deciders:** project owner, from the chimera-utils review (fix round 1)

## Context
ADR 0098 says local paths live in a git-ignored `.env` that `just` loads.
The review found that `set dotenv-load` applies to the whole workspace. It
also searches parent directories, so any `.env` up the tree reaches every
recipe, `firmware` and `flash` included. The tool already reads every path
from an environment variable or a flag, so it never needed the file.

The same review found ported code without its notices:
- NumPy's `SeedSequence`, ziggurat normal, `random_interval` and `shuffle` (BSD-3-Clause);
- O'Neill's PCG64 (Apache-2.0 or MIT);
- SciPy's `resample_poly` padding and `butter` design (BSD-3-Clause).

ADR 0098's Sources credited only SciPy, as behaviour.

## Decision
- **No `.env` loading.** The Justfile has no `set dotenv-load`. Paths come
  from exported variables (`MIDIVERB_EMU`, `MV_STIM`, `MV_LISTING`,
  `CHIMERA_REFS`, `MV_OUT`) or the flags of the same name. The crate README
  shows the `export` lines. This replaces ADR 0098's ".env, which `just`
  loads" sentence. The rest of 0098 stands.
- **Ported code is credited where it lives.**
  - `chimera-utils/src/stim.rs` carries NumPy's BSD-3 notice in full (NumPy Developers; Kevin Sheppard, randomgen). It also credits PCG64 to Melissa O'Neill (Apache-2.0 or MIT) and the ziggurat tables to Marsaglia and Tsang.
  - `chimera-utils/src/sig.rs` notes that the resampler's padding and the Butterworth design follow SciPy (BSD-3).
  - The crate README has a "Third-party notices" section.
- **References stay out of the repo.** Code run through the contract (the
  MidiVerb emulator, wrappers) stays out of the repo, as 0098 says.

## Alternatives considered
- **Load a `.env` only in the utils recipes.** That is still a file to keep
  and to leak, and it buys nothing over `export`.
- **Credit the ports only in an ADR.** BSD-3 asks for the notice to travel
  with the source, so it goes in the code. An ADR alone is not enough.

## Consequences
- **No `.env` to keep.** Each shell exports its own paths, and no recipe can
  pick up a stray `.env` from up the tree.
- **The notices travel with the code.** A port of more NumPy or SciPy code
  adds its notice in the same file and the README.

## Sources
- ADR 0098; the chimera-utils review (fix round 1).
- NumPy `numpy/random` (BSD-3-Clause), PCG (pcg-random.org, Apache-2.0 or
  MIT), SciPy `scipy.signal` (BSD-3-Clause).
