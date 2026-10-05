# chimera-utils

A host-only tool that A/Bs Chimera's DSP against external reference programs
(ADR 0098). It renders Chimera in-process through `chimera-core` (the real
`FxBus`, the real SVF, the real ALGO kernel). It runs each reference as a
subprocess. It never enters the firmware build.

## Commands

| `just …` | What |
|---|---|
| `ab REF DEVICE [k=v …]` | Renders both sides. Prints the null residual, the response error per octave, THD and the centroid. Writes `ref.wav` and `chimera.wav` (RMS-matched) and `report.json`. |
| `mv-refs [PROG …]` | MidiVerb II references: each program (default 1–49) on the click, 16 noise bursts and the dry stimuli. |
| `mv-fit TYPE PROG …` | A Latin-hypercube sweep of the TYPE's knobs, then a pattern search per program. Writes `fits/<type>.json`. With `--start FITS` it searches from those fits instead, in finer steps with SMEAR held. `--seeds N` sets the noise seeds per search step: 8 by default, 16 to search on what verify judges. |
| `mv-verify TYPE` | Re-measures the fitted knobs (16 seeds). Prints residuals and PASS/FAIL per preset. Exits 1 on any FAIL. |
| `mv-ab TYPE` | Reference and TYPE at its fitted knobs on idm, snare and strings, RMS-matched. |
| `mv-structure PROG …` | A program's injections, allpasses and taps, read from the decompiled listing. |

`chimera-utils devices` lists the devices and their knobs.

Flags go anywhere on the line. `--jobs N` sets the worker threads (default 8).
`chimera-utils` with no command prints every flag.

## Where things live

Nothing a reference produces goes into the repo. The paths come from the
environment. `just` reads a git-ignored `.env` at the repo root, so local
paths can live there.

| Variable | What |
|---|---|
| `MV_OUT` | Outputs (default `target/mv`): `refs/`, `fits/`, `ab/` |
| `MIDIVERB_EMU` | The MidiVerb emulator binary. Its names and decompiled listing are read from beside it. |
| `MV_STIM` | Dry stimuli: `snare`, `kick`, `strings`, `plucked`, `idm` `.wav`, 48 kHz |
| `MV_LISTING` | `decompiled-midiverb2.h`, if it is not beside the emulator |
| `CHIMERA_REFS` | The directory `ab` looks in for a REF by name |

## The reference contract

A reference is an executable built outside the repo, whatever its license.
The tool runs it as:

    REF out=OUT.wav sr=RATE [in=IN.wav] [frames=N] [key=value …]

- **`in`** is a mono 32-bit float WAV at `sr`, for an effect.
- **`frames`** replaces `in` for a source (an oscillator): the number of samples to render.
- **`key=value` pairs** are the device's knobs as given on the `ab` line. `ref.key=value` goes to the reference alone.
- **`out`** must be written at `sr`, as PCM or float, mono or stereo. Stereo is averaged.
- **A non-zero exit** fails the run. Its stderr is shown.

The tool owns the stimuli, the alignment and every metric. A reference only
renders.

**The MidiVerb II adapter.** thement/midiverb_emulator (GPL-3.0) predates the
contract. The `midiverb` REF adapts it. It takes `prog=N`, and
`normalise=0` drives the unit as is. It resamples 48 kHz to 23,400 Hz with the
`resample_poly` 39/80 filter (Kaiser β 10, linear phase). It writes the
unit's 16-bit stereo, runs `midiverb N in out`, resamples back, and returns
(L+R)/2.

## Metrics

**Reverb** (`metrics.rs`, ported call for call from the Python harness):
- band RT60 from interrupted noise: T20, floor subtracted, 125 Hz–8 kHz octaves;
- pre-delay: −20 dB re the peak;
- C50;
- Abel–Huang NED at 20/50/100 ms (each the mean over ±5 ms);
- tail centroid and tilt;
- the burst envelope.

Match: band RT60 ±5 % (judged 250 Hz–4 kHz), pre-delay ±1 ms, C50 ±1 dB, NED
±10 %, centroid ±5 %.

**Generic** (`generic.rs`):
- **null residual:** dB, after the best lag (±100 ms) and gain;
- **response error:** the largest |Δ| in dB per octave. For an effect it is |Pxy|/Pxx on noise. For a source it is the output spectrum. Bins 60 dB under the reference's peak are skipped;
- **THD:** harmonics 2–10, on a 1 kHz sine for an effect, on the output for a source;
- **spectral centroid.**

## Noise and seeds

Stimuli draw from a port of numpy's `default_rng(seed)`: SeedSequence,
PCG64, the ziggurat `standard_normal`, `random` and `permutation`. Seed for
seed, the noise bursts and the Latin hypercube are the Python harness's. Its
ziggurat tables come from the recurrence and are within about 1e-14 of
numpy's. Written as float32 WAVs, they are bit-identical.

## Prototyping

New reverb structures, and any other DSP idea measured against a reference,
are prototyped in `src/models.rs`, in Rust.
