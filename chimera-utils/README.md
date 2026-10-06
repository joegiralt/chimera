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
| `mv-verify TYPE` | Re-measures the fitted knobs (16 seeds). Prints residuals and PASS/FAIL per preset. Exits 1 on any FAIL. With `--robust` it also nudges every fitted knob ±0.001 and names each nudge that fails. |
| `mv-start [TYPE …]` | Each TYPE's START against its source program (ROOM #5, BLOOM #45), judged as `mv-verify` judges a fit. Exits 1 on any FAIL. |
| `mv-ab TYPE` | Reference and TYPE at its fitted knobs on idm, snare and strings, RMS-matched. |
| `mv-structure PROG …` | A program's injections, allpasses and taps, read from the decompiled listing. |

`chimera-utils devices` lists the devices and their knobs.

Flags go anywhere on the line. `--jobs N` sets the worker threads (default 8).
`chimera-utils` with no command prints every flag.

## Where things live

Nothing a reference produces goes into the repo. The paths come from the
environment, or from the flag of the same name. Export them in the shell
that runs `just`:

    export MIDIVERB_EMU=~/src/midiverb_emulator/midiverb
    export MV_STIM=~/audio/mv-stim
    export CHIMERA_REFS=~/src/refs-cpp

| Variable | What |
|---|---|
| `MV_OUT` | Outputs (default `target/mv`): `refs/`, `fits/`, `ab/` |
| `MIDIVERB_EMU` | The MidiVerb emulator binary. Its names and decompiled listing are read from beside it. |
| `MV_STIM` | Dry stimuli: `snare`, `kick`, `strings`, `plucked`, `idm` `.wav`, 48 kHz |
| `MV_LISTING` | `decompiled-midiverb2.h`, if it is not beside the emulator |
| `CHIMERA_REFS` | The directory `ab` looks in for a bare REF name. Unset, a bare name means `./name`, never a program on `PATH`. |

## The reference contract

A reference is an executable built outside the repo, whatever its license.
The tool runs it as:

    REF out=OUT.wav sr=RATE [in=IN.wav] [frames=N] [key=value …]

- **`in`** is a mono 32-bit float WAV at `sr`, for an effect.
- **`frames`** replaces `in` for a source (an oscillator): the number of samples to render.
- **`key=value` pairs** are the device's knobs as given on the `ab` line. `ref.key=value` goes to the reference alone.
- **`out`** must be written at `sr`, as PCM or float, mono or stereo. Stereo is averaged.
- **It must hold at least** the input's length (or `frames`) in samples. Extra samples are dropped, and fewer is an error.
- **A non-zero exit** fails the run. Its stderr is shown.

The tool owns the stimuli, the alignment and every metric. A reference only
renders. Latency and inverted polarity are measured, not penalised. The null
finds the lag by the largest |correlation| and reports a negative gain as
`inverted`. The response error is read after the reference is moved by that
lag.

**The MidiVerb II adapter.** thement/midiverb_emulator (GPL-3.0) predates the
contract. The `midiverb` REF adapts it. It takes the program as
`ref.prog=N`, as in `just ab midiverb room ref.prog=5`, and `ref.normalise=0`
drives the unit as is. It resamples 48 kHz to 23,400 Hz with the
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

**GRAIN and HF share** (`grain.rs`, ADR 0095's GRAIN). Judged for ROOM,
reported for the other TYPEs:
- **GRAIN:** a noise-power ratio. Four notched noise bursts (2.5–3.5 and
  8.25–9.75 kHz notches, low-passed at 10.8 kHz) drive both sides, the unit
  at the A/B's −1 dBFS peak. Each notch core's power density over its
  flanks', in the burst's last 500 ms and 50–200 and 200–400 ms after it:
  six values. An LTI reverb leaves the notches empty; arithmetic grain
  fills them. Match: rms Δ ≤ 3.5 dB, no |Δ| over 6.5 dB.
- **HF share:** the noise bursts' tail, P(4–11.7 kHz) / P(0.5–2 kHz), at
  0–50, 50–200 and 200–600 ms after the burst. Match: each ±1 dB.

A `metrics.json` from before GRAIN lacks both, and a judged TYPE then
fails: re-run `mv-refs` for its programs.

A TYPE that doesn't fit SMEAR holds it at 0, and its fits carry it.

**The fit's score** (every TYPE, ADR 0095): the weighted distance, plus 4
for each judged metric past 0.8 of its bar, taken at the worst of the
setting and SIZE ±0.001 (SIZE's lengths are whole samples). `--steps
A,B,…` sets `--start`'s pattern steps.

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

## Builds

The `just` recipes build in the `utils` profile. It is release without the
firmware's fat LTO, and its panics unwind so scratch directories are removed.
`--redo` on `mv-fit` re-measures the knob sweep, which is cached per TYPE in
`fits/<type>_sweep.json`. Use it after a DSP change.

## Third-party notices

The tool ports, and does not link, these. Each notice also sits in the code:

- **NumPy** (`src/stim.rs`): `SeedSequence`, `random_standard_normal`,
  `next_double`, `random_interval` and `Generator.shuffle`. BSD-3-Clause.
  Copyright (c) 2005-2024, NumPy Developers; Copyright (c) 2019 Kevin Sheppard.
  The full license text is in `src/stim.rs`.
- **PCG64** (`src/stim.rs`): Melissa O'Neill's PCG XSL-RR 128/64 (pcg-random.org).
  Apache-2.0 or MIT. Copyright 2014 Melissa O'Neill.
- **SciPy** (`src/sig.rs`): `resample_poly`'s filter design and padding,
  `butter`'s pole placement, `sosfilt` and `welch`. BSD-3-Clause. Copyright (c)
  2001-2002 Enthought, Inc. 2003, SciPy Developers.

References run through the contract (the MidiVerb emulator, GPL-3.0, and any
wrapper) stay outside the repo, whatever their license.
