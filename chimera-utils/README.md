# chimera-utils

A host-only tool that A/Bs Chimera's DSP against external reference programs
(ADR 0098). It renders Chimera in-process through `chimera-core` (the real
`FxBus`, the real SVF, the real ALGO kernel). It runs each reference as a
subprocess. It never enters the firmware build.

## Commands

| `just …` | What |
|---|---|
| `ab REF DEVICE [k=v …]` | Renders both sides. Prints the null residual, the response error per octave, THD and the centroid. Writes `ref.wav` and `chimera.wav` (RMS-matched) and `report.json`. An effect also gets `<source>_ref.wav` and `<source>_chimera.wav` on the listening set. |
| `mv-refs [PROG …]` | MidiVerb II references: each program (default 1–49) on the click, 16 noise bursts and the listening set. |
| `mv-fit TYPE PROG …` | A Latin-hypercube sweep of the TYPE's knobs, then a pattern search per program. Writes `fits/<type>.json`. With `--start FITS` it searches from those fits instead, in finer steps with SMEAR held. `--seeds N` sets the noise seeds per search step: 8 by default, 16 to search on what verify judges. |
| `mv-verify TYPE` | Re-measures the fitted knobs (16 seeds). Prints residuals and PASS/FAIL per preset. Exits 1 on any FAIL. With `--robust` it also nudges every fitted knob ±0.001 and names each nudge that fails (a report: the exit code ignores it). |
| `mv-start [TYPE …]` | Each TYPE's START against its source program (ROOM #5, BLOOM #45), judged as `mv-verify` judges a fit. Exits 1 on any FAIL. |
| `mv-ab TYPE` | Reference and TYPE at its fitted knobs on the listening set, RMS-matched. |
| `mv-structure PROG …` | A program's injections, allpasses and taps, read from the decompiled listing. |
| `stim DIR` | The listening set as mono 24-bit WAVs, `DIR/<name>.wav`: what the factory effects demos play through the FX bus. |
| `voice-stats WAV …` | Measures any 48 kHz WAVs, ours or a reference's, the same way: loudness, vibrato, drift, breath share, formants, vowel motion and brightness, one row each. `--json PATH` writes the rows as JSON too. |

`chimera-utils devices` lists the devices and their knobs.

Flags go anywhere on the line. `--jobs N` sets the worker threads (default 8).
`chimera-utils` with no command prints every flag.

## Where things live

Nothing a reference produces goes into the repo. The paths come from the
environment, or from the flag of the same name. Export them in the shell
that runs `just`:

    export MIDIVERB_EMU=~/src/midiverb_emulator/midiverb
    export CHIMERA_REFS=~/src/refs-cpp

| Variable | What |
|---|---|
| `MV_OUT` | Outputs (default `target/mv`): `refs/`, `fits/`, `ab/` |
| `MIDIVERB_EMU` | The MidiVerb emulator binary. Its names and decompiled listing are read from beside it. |
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
for each judged metric past 0.8 of its bar. `--size-worst` scores each
setting by the worst of it and SIZE ±0.001 (SIZE's lengths are whole
samples); it is off by default, since it triples a fit and made ROOM's
refits worse. `--steps A,B,…` sets `--start`'s pattern steps.

`--robust` is a report, not a gate: a one-sample length change reshuffles
the echoes below hearing, and moves NED and a short tail's late HF share
past their bars. ROOM's nine presets pass at their fitted knobs; two hold
through every ±0.001 nudge (ADR 0095).

**Generic** (`generic.rs`):
- **null residual:** dB, after the best lag (±100 ms) and gain;
- **response error:** the largest |Δ| in dB per octave. For an effect it is |Pxy|/Pxx on noise. For a source it is the output spectrum. Bins 60 dB under the reference's peak are skipped;
- **THD:** harmonics 2–10, on a 1 kHz sine for an effect, on the output for a source;
- **spectral centroid.**

**Voice** (`voice.rs`, `voice-stats`), on an 85 ms Hann STFT with a
5.3 ms hop:
- **LUFS:** BS.1770-4 gated integrated loudness, over every channel;
- **vibrato and drift:** tonal peaks (20 dB over the 15th percentile of
  the ±280 Hz round them) tracked frame to frame by their phase-vocoder
  frequency. Each track of 0.4 s or more is split at a 0.4 s triangular
  average (about 1 Hz). The fast part's RMS is the vibrato in cents, at the
  rate where its spectrum peaks; the slow part's RMS is the drift.
  Detuned partials beating inside one bin read as movement, as they are
  heard;
- **breath share:** the energy off the steady tonal peaks, 80 Hz–12 kHz.
  A noise band 200 Hz wide reads 0.9; a band 50 Hz wide is nearly a tone;
- **formants:** every 200 ms window's spectral envelope (its tonal peaks
  joined, smoothed as wide as the harmonics are apart), and its peaks over
  150 Hz–5 kHz standing 3 dB over their valleys, within 35 dB of the top:
  the median count, the first three's Hz and −3 dB widths;
- **vowel motion:** the 5–95 % range of the envelope's centroid and of its
  800 Hz–3 kHz peak, and the envelope's shape change every 250 ms (dB RMS,
  level removed);
- **level movement:** 50 ms RMS against its 0.5 s average, dB;
- **brightness:** the long-term spectrum's 2–5 kHz over its 150 Hz–1 kHz.

## The listening set

`mv-ab`, `ab` and `mv-refs` play seven short sounds, synthesised in
`src/stim.rs` with fixed seeds: no samples. Each is mono at 48 kHz, peaked
at −12 dBFS and followed by 12 s of silence for the tail. `stim.rs` gives
each one's exact recipe.

| Source | What | It reveals |
|---|---|---|
| `snare` | a 190 → 160 Hz body and a noise rattle, 300 ms | the early reflections |
| `bass` | A1, a sine and a little saw, a fast pitch drop, ~300 ms | the low end and the transient |
| `stab` | an A minor 7 saw chord, 150 ms | the transient on a full spectrum |
| `sine_short` | 440 Hz, 80 ms | the tail's colour |
| `sine_long` | 220 Hz, 2 s | the build-up and the tail's colour |
| `texture` | 300 ms of band-passed noise grains | the grain |
| `hats` | six closed hats and an open one, high-passed noise, ~1 s | the grain and the top |

The fit and the verdict never hear these: they measure on the click and
the noise bursts.

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
