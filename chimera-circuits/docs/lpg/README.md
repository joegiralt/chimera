# Low-pass gates: Buchla 292B, VTL5C3, Plaits

One docs directory for the three LPG modules, because one lab built them
together: `src/buchla292b/` (the circuit), `src/vtl5c3/` (its vactrol) and
`src/plaits_lpg/` (the behaviour reference it was checked against).

## Models

| `chimera-utils circuit render` name | what |
|---|---|
| `buchla292b/reference` | the 292B as drawn (B21208, rev. 7-76): f64, 8× (384 kHz), nodal Newton; LED driver; VTL5C3 vactrol. FILT (LOWPASS), the 292B's own mode and the lab's default |
| `buchla292b/reference-gate` | the same in GATE (15 kΩ shunt) |
| `buchla292b/reference-combo` | the same in the 292C's centre position, COMBO (leaky: −20 dB at DC shut) |
| `buchla292b/fw`, `fw-gate`, `fw-combo` | the firmware shape: f32 at 48 kHz, P&D's TPT core, driver and vactrol once every 16 samples |
| `plaits_lpg/port` | Plaits' LPG (MIT), DECAY and COLOUR ½ |

As voices: a gate rise strikes (the gate checks' ping, 1 ms up / 10 ms
down, × velocity; × 15 V into the 292B's CV, into Plaits' LEVEL); CUTOFF is
the resting offset (the 292B's OFFSET volts, or Plaits' LEVEL, whose
settled −3 dB is CUTOFF); RES is ignored (the 292B has no resonance; Q1's
fixed gain is its feedback). `vtl5c3` registers no voice: a vactrol has no
audio path. The 292B's cell is a `vtl5c3::Cell`, so `VTL5C3_2` (Najnudel's
identified dual cell) is one argument away.

## Files

| file | what |
|---|---|
| `research.md` | the circuit lab's sources, what was followed and why, what is not modelled |
| `notes.md` | the circuit lab's notes: the gate review, what the first lab got wrong, the firmware model, cost, open questions |
| `validation.md` | the circuit and vactrol checks (VTL5C3/2) |
| `gate.md` | the gate checks (VTL5C3), Plaits and the SuperCollider LPG as references |
| `levels.md` | the A/B against CLEAN LP2 through the engine, ping timelines, brightness |
| `research-first-lab.md` | the first lab's source survey (P&D, Najnudel, sheets, Make Noise, Serge) |

## Labs

- `lpg-lab`, 98c95567: the first LPG, a behavioural guess (an exponential
  LED converter, a hand-fit one-pole vactrol, a "dark floor", a cubic
  "buffer", STRIKE/DECAY/FB/STABILITY). The owner rejected it. Only its
  ADSR stimulus (`buchla292b::stim::Env`) came over; its notes and
  `levels.md` describe that model and are left in the ledger.
- `lpg-circuit-lab`, ecf60930 (the circuit) and db41162b (the gate review:
  the VTL5C3 refit, FILT as default, the gate checks, the Plaits port).

## Left out, and why

- The first lab's model, all of it (above).
- Everything that rendered through Chimera's engine (`Instrument`): the
  takes' VA sources (marimba, bongo, bass, pad, sweep). The convergence
  and velocity tests use a stand-in bar (a sine and its fourth partial),
  so they check the ledger's spans and bounds, not its absolute numbers.
- Everything compared with the shipped CLEAN filter (the CLEAN rows, the
  LPG − CLEAN nulls, the cutoff map that drove it): the record stays in
  `gate.md` and `levels.md`.
- The drivers that only wrote WAVs and markdown: `ab()`, `prep()`,
  `diagnose()`, `gate_debug()`, `open_table()` (its numbers are a test),
  `cost()`, `timelines()`/`brightness()` (their numbers are tests).
- The SuperCollider LPG (GPL-3.0): never read, and nothing of it is here.
  The lab ran it outside the repo as an external listening reference; its
  rows in `gate.md` are cited as such. Its input renderer (`prep`) and WAV
  reader went with it. The ping's shape is the curve SuperCollider calls
  Env.perc, written from its formula.
- The lab's process-global cell switch (`set_cell`): each model now takes
  its `Cell`.

## Tests

Fast (debug, in `just check`): the static and LED laws, the f32 vactrol
against the reference, the VTL5C3 refit's error, the TPT core at 48 kHz,
DC gain / a_max, the network responses, the LED driver, the fw ping
timelines, the fw gate checks, the voices, Plaits' gate checks and trace.

Slow (`#[ignore]`): the reference against eq. 7 at 8×, the follower's
distortion, oversampling convergence, the reference ping timelines, the
reference gate checks (with fw tracking and the old cell), how far a ping
opens the cell, velocity against brightness, the vactrol dynamics, and the
ν± refit:

```
cargo test -p chimera-circuits --release -- --ignored
```

## Licences and citations

- Najnudel, J., Müller, R., Hélie, T. & Roze, D., "Power-balanced dynamic
  modeling of vactrols: application to a VTL5C3/2", Proc. DAFx-23,
  Copenhagen, 2023. CC BY 4.0. Eq. 4, 7–8, 13 and Table 1 used; eq. 12
  replaced (§5.3 method with the PerkinElmer sheet); ν± refit for the
  VTL5C3. https://www.dafx.de/paper-archive/2023/DAFx23_paper_50.pdf
- Parker, J. & D'Angelo, S., "A digital model of the Buchla lowpass-gate",
  Proc. DAFx-13, Maynooth, 2013. Equations and the TPT method; our code.
- Plaits, Copyright 2014–2016 Emilie Gillet, MIT: `plaits/dsp/fx/low_pass_gate.h`,
  `plaits/dsp/envelope.h`, `plaits/dsp/voice.cc`, stmlib `dsp/filter.h`.
  The notice is in `src/plaits_lpg/mod.rs`.
- Buchla Associates drawing B21208 "Mod 292B Quad Lo Pass" (10/12/70, rev.
  7-76) and the 292C schematic excerpts, via
  https://modularsynthesis.com (MEMS and Roman's pages): values only, cited
  by link; no schematic image is copied.
- PerkinElmer, "Photoconductive Cells and Analog Optoisolators" (2001),
  VTL5C3 and VTL5C3/2; the 1N5228B sheet. Values read off, cited.
- SuperCollider LPG UGen (PortedPlugins, GPL-3.0): an external listening
  reference only, not read, not in the repo.

## Reproduction notes (the port against the ledger)

Every ledger number the tests check reproduces. Where the lab's prose and
its own tables disagree, the tests follow the tables:
- fw against ref −3 dB after a 15 V ping: `notes.md` says within 1 % from
  3 ms; `levels.md` has 2.1 % at 3 ms and 2.4 % at 5 ms.
- fw against ref on the gate checks: "≤ 3 ms"; `gate.md` has 4 ms (saw,
  GATE, −40 dB at 179 / 175 ms).
- f32 vactrol against the reference from dark: "up to −6 % R in the first
  ms at 0.6 mA"; measured −1.3 % at 0.6 mA but −16 % in the first block
  and −11 % at 0.67 ms at 3 mA, ≤ 4 % from 2 ms.
- Plaits' cutoff column in `gate.md` came from the state before each
  block's update (the lab's trace; its audio path used the state after).
- ref − fw nulls on the gate checks' bright tones are −18.6…−31.5 dB
  (worst: saw in GATE), against −37.8…−48.7 on the lab's darker
  engine-rendered takes; no ledger number exists for these stimuli.
