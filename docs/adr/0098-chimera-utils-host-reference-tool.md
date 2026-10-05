# 0098. A/B Chimera's DSP against external references with `chimera-utils`, a host-only Rust tool

- **Status:** Accepted (2026-10-05)
- **Deciders:** project owner (approved the design, and widened it from MidiVerb reverbs to any C/C++ reference)

## Context
ADR 0095 makes the MidiVerb II the reference for every reverb TYPE. The
harness that measured it was Python in the session scratchpad. It ran each
Chimera render through a separate test binary (`zz_mvref`), passing every
signal through a WAV file, and a fit runs thousands of renders. The harness
was not in the repo for anyone to rerun.

The owner wants the same kind of A/B for anything Chimera models. That
includes filters, oscillators and effects, with the reference being C or C++
code built elsewhere: Mutable Instruments' stmlib, the PreenFM3's stock
firmware, the MidiVerb emulator. Some of these are GPL-3.0. Chimera's own
code was then MIT (GPL-3.0-or-later since ADR 0330).

## Decision
- **One Rust crate, `chimera-utils`**, a workspace member that uses std and
  depends on `chimera-core`. Nothing depends on it: `cargo tree -p
  chimera-stm32` does not list it, and the firmware recipes never build it.
  Its dependencies are host-only: hound, rustfft, rayon and serde_json.
- **Rust, not Python.** The tool links the DSP and so renders it
  in-process. No subprocess or WAV file stands between a render and its
  measurement. The measurement code is ported call for call from the Python
  harness, including its scipy calls (`resample_poly`, `butter`, `sosfilt`,
  `welch`). On the same signals it gives the same numbers, to 1e-7. New
  reverb structures are prototyped in its `models` module, in Rust.
- **The reference contract.** A reference is an executable built outside
  the repo, run as `REF out=OUT.wav sr=RATE [in=IN.wav] [frames=N]
  [key=value …]`. `in` is a mono float WAV at `sr`, for an effect. A source
  gets `frames` instead. The reference writes `out` at `sr`. The tool owns
  the stimuli, the alignment and the metrics; a reference only renders.
  The contract is documented in `chimera-utils/README.md`.
- **The MidiVerb emulator is one adapter on the contract.** The tool finds it
  through `MIDIVERB_EMU` and runs it as a subprocess. The adapter resamples
  48 kHz ↔ 23,400 Hz with scipy's 39/80 Kaiser filter (β 10, linear phase)
  and does the unit's 16-bit stereo I/O.
- **The GPL boundary.** No reference code is linked, vendored or copied in:
  not the emulator, not its decompiled listings, not stock PreenFM3 code, not
  a reference's wrapper, and not an MIT one either. The repo holds only the
  tool and the contract. `mv-structure` reads the decompiled listing from the
  emulator's tree at run time and prints counts and lengths.
- **Outputs stay out of the repo.** References, fits, A/B WAVs and scratch
  files go to `MV_OUT`, by default the git-ignored `target/mv/`. Local paths
  live in a git-ignored `.env`, which `just` loads.
- **A thread cap.** Renders run on a rayon pool of `--jobs` threads, by
  default 8, so a fit shares the machine.
- **Devices are registered as they are used.** Each device has a name, a knob
  list and a render through the real DSP. The first are each reverb TYPE,
  the SVF (CLEAN), and an ALGO operator. The `just ab` metrics are the null
  residual after the best lag and gain, the response error per octave, THD
  and the spectral centroid. The reverb metrics sit beside them, and
  `just mv-*` is built on the same core.

## Alternatives considered
- **Keep the Python harness.** It reached Chimera only through a test binary
  and WAV files. It lived outside the repo. It could not prototype in the
  DSP's own language.
- **Link the references (FFI or a vendored C build).** That is faster still,
  but it puts GPL code into the build and makes Chimera's license depend on
  which references are compiled in.
- **A crate inside `chimera-desktop`.** The simulator is a product. A
  measuring tool and its dependencies do not belong in it.

## Consequences
- Fits and verifications are reproducible from the repo by anyone holding
  the emulator.
- The stimuli draw from a port of numpy's `default_rng` (SeedSequence,
  PCG64, the ziggurat normal). The noise and the knob sweep are the Python
  harness's, seed for seed, so its fits and references stay comparable. A
  short program's band RT60 moves several percent between noise
  realisations, so a different generator would have moved verdicts.
- A reference needs a small wrapper outside the repo that speaks the
  contract.

## Sources
- ADR 0095. The Python harness (session scratchpad `midiverb-ref/`:
  `mvlib.py`, `refs.py`, `fit.py`, `verify.py`, `ab.py`, `structure.py`,
  `zz_mvref.rs`), now replaced.
- thement/midiverb_emulator (GPL-3.0): run as an external program only.
- pichenettes/stmlib `dsp/filter.h` (MIT): the first contract reference,
  wrapped outside the repo.
- SciPy `signal.resample_poly`, `firwin`, `butter`, `sosfilt` and `welch`
  (BSD-3-Clause): the behaviour ported, not the code.
