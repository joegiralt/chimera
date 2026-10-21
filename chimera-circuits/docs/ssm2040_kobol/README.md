# ssm2040_kobol: the RSF Kobol Expander's filter

The Kobol Expander's VCF, rebuilt from RSF's schematic: an SSM 2040 in the
data sheet's standard connection (10 K chain, 220 Ω legs, 1 nF caps, R24
100 K in), its resonance through a CA3080 (CI5) fed from OUT 4 by C6 4.7 µF
and a 4.7 K/220 Ω divider, its output through a second CA3080, the VCA (CI3).
Nothing compensates the bass. Code: `chimera-circuits/src/ssm2040_kobol/`.

## Models

| Name | What | Renders as a `Voice`? |
|---|---|---|
| `circuit::Kobol` + `circuit::spice` | The transistor-level netlist (2040 cells after the patent, RCA's CA3080 topology, Gummel-Poon BJTs), Gear-2 at 192 kHz in ngspice: the lab's reference. `Kobol::SCHEMATIC` (typical offsets), `Kobol::UNIT` (CI5 at 1.5 mV, the owner's unit). `circuit::dc_cell` is one cell's DC curve, which EXP-C's swing is fitted to. | No: it needs ngspice, an external tool, so it is not registered. |
| `ssm2040_kobol/exp_c` | `fw::Exp2`, EXP-C: the firmware's 2040 structure (four TPT OTA stages, the loop solved linearly in one step, f32, 1×) with every constant taken from the circuit: the cells' knees and offsets, stage 1's own pole, a two-sided swing fitted to the cell's DC curve, C6's 10.2 Hz coupling in the loop, CI5 as a tanh on OUT 4 (degree-7 `fw::tanh_q`) with its bias-current offset, CI3 as a cubic. RES → k = 5.59·RES (the 3080 linear in its CV, the Kobol's top). `Offsets::UNIT`. | Yes |
| `ssm2040_kobol/exp_c_typical` | The same on `Offsets::REFERENCE` (the data sheets' typical offsets, the netlist's chip). | Yes |

`takes.rs` holds what the lab read the owner's recorded Kobol with: harmonic
analysis (`f0_near`, `f_peak`, `harmonics`, `amp_at`), the Kobol's VCO as a
wave table (`Wave::measured`), response tables and shape fits
(`response_table`, `shape_err`, `best_fc`, `shape_of`, `masked`), the
linear 4-pole bass fit (`lin4`, `fit_lin4`), the drive steps (`steps`), the
self-oscillation note finder (`osc_notes`), the squelch measure
(`ring_over`), the phrases' sequences, envelopes, stimuli and patch fit
(`sequence`, `events`, `ads`, `phrase`, `fit_phrase`, `phrase_patch`), and
the A/B's jobs (`drive_job`, `sweep_job`, `phrase_job`, `selfosc_job`,
`tune_to`). Renderers are closures, so the circuit, EXP-C and the firmware's
2040 are read the same way.

## What shipped, and what is kept here

- **Shipped:** VOICE EXP on the 2040 (`chimera-engine/src/dsp/filter/k2040.rs`,
  label EXPNDR), ADR 0105, then ADR 0191 "EXP keeps the Kobol's bass and
  rings over its input": the firmware's own EXP structure (6 dB more drive
  than REV2, a cubic 3080 limiter at L = ½ on the summed loop, a 3080 output
  stage, the circuit-scale stage bend) plus a make-up gain `1 + k·(0.6 −
  0.0752·k)` fitted to the Kobol's sweep takes, and a +36-cent trim.
- **Not shipped: EXP-C.** It is not in the firmware. The lab (this module)
  found afterwards that the sweep takes ADR 0191 was fitted to were
  contaminated (VCO1 patched into the VCF's FREQ input, and a 5 dB gain
  change between takes), that the circuit itself loses the plain 1/(1+k),
  and that EXP-C would cost a voice (≈335–361 cycles against the 285–300
  target: 6 voices on rev V, against today's EXP's 7). The owner's open
  questions (keep or drop 0191's make-up, the RES law's onset at 0.72, the
  cost, per-voice H2 through STABILITY) are in `notes.md`.
- **Kept:** the netlist, EXP-C, the analysis code, and the lab's numbers as
  tests (`tests.rs`), so the model can go to a bigger processor or a plugin
  as it is.

## Lab history

- `fa996658` "Kobol lab: takes vs the 2040's voices, fits, A/B renders"
  (`kobol.rs`; the comparison of 2026-10-20, `compare-*.md`).
- `f17f244b` "Kobol EXP lab: transistor-level netlist, EXP-C, probes, M7
  count" and `bdfa3045` "Kobol EXP lab: A/B of five sources, drive
  linearity, degree-7 tanh, lean M7 loop" (`kcircuit.rs`, `exp2.rs`,
  `kab.rs`, `kexp.rs`; `research.md`, `notes.md`, `ab.md`).
- Branch `exp-circuit-lab` (pushed; never merged). The 2040 commits under it
  are the Prophet lab, ported as `ssm2040_prophet`.

## Docs here

- `research.md`: sources, the circuit read off the schematic, the derived
  loop, the netlist's assumptions, EXP-C's derivation.
- `notes.md`: validation against the Kobol, EXP-C against the circuit and
  the firmware's EXP, cost, open questions.
- `ab.md`: the A/B's measures (five sources, eleven takes).
- `kobol-research.md`: the first read of the Kobol against EXP (2026-10-07).
- `compare-notes.md`, `compare-drive.md`, `compare-selfosc.md`,
  `compare-phrases.md`, `compare-sweeps.md`: the 2026-10-20 comparison of
  the takes with REV2 and EXP. `compare-sweeps.md` (the inferred FREQ
  tracks) and `compare-phrases.md` (the fitted patches) are data the A/B's
  jobs read (`takes::SWEEPS_MD`, `takes::PHRASES_MD`).
- `2040-bass.md`: the A/B of ADR 0191's make-up (what shipped).

Sources are cited, never copied: the RSF *Expander Kobol* service manual
and schematic (yusynth.net `gear/RSF/schema-RSF-KOBOL.pdf`), the SSM 2040
data sheet (SSMT 1981), the CA3080 data sheet and RCA/Intersil AN6668,
US 3,969,682. No GPL code was read or used. The CA3080 here is a
transistor-level topology with generic parameters, not Köper et al.'s
fitted model.

## Left out, and why

- **The takes** (private notes, 20-channel
  MOTU recordings): the owner's recordings never go in the repo. Code reads
  them from `KOBOL_DIR`.
- **ngspice outputs and netlists** (the lab's `spice/`, 7 GB): regenerated
  by `circuit::spice` and `circuit::dc_cell`, cached under
  `target/spice/ssm2040_kobol/`.
- **WAV sets and the A/B's renders** (`wav/`, `compare/*.wav`):
  `chimera-utils circuit render` renders any registered model.
- **The printers** (`kobol explore|profile|sweeps|drive|selfosc|phrases|dbg|
  law|comp|frame|squelch|track|peaks|lab`, `kexp probe|ratio|floor|csine|
  e2sine|ab`, `write16`, `write_set`): they wrote markdown and WAVs. Their
  measures, fits and stimuli are functions in `takes.rs`; their numbers
  are tests.
- **`lab/m7probe` and `lab/probes.sh`** (on `exp-circuit-lab`): the M7
  static cycle count of EXP-C's lean inner loop (a `no_std` crate built for
  thumbv7em) and the shell script that ran the ngspice/EXP-C probe set.
  Tooling, not a model; they stay on the branch.
- **rayon:** the lab parallelised renders with it; here everything runs
  serially (no new dependency).
- `Kobol::from_env` (`KEXP_VAR` overrides) and `KEXP_UNIT`: plain struct
  values now (`Kobol { vos5: 2e-3, ..Kobol::SCHEMATIC }`, `Offsets::UNIT`).

## Tests

`cargo test -p chimera-circuits` runs the fast set (about 1 s in debug):
EXP-C's unity, pole, bass loss, resonance onset, ring (H2, H3, level,
pitch, tracking), drive linearity, hard drive, `tanh_q`'s accuracy and the
swing's limits, and the firmware's EXP against EXP-C on ADR 0191's bass
numbers.

The rest are `#[ignore]`d:

```
# slow (the firmware's EXP on 8 s drive renders)
cargo test -p chimera-circuits --release -- --ignored
# the circuit: needs ngspice (each run 20–50 s; cached)
NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored --test-threads=2
# the owner's takes: point KOBOL_DIR at the session directory
KOBOL_DIR=<dir> cargo test -p chimera-circuits --release -- --ignored
```

A takes test with `KOBOL_DIR` unset passes vacuously and says so on stderr.

With both set, the ignored set takes about 2 minutes from a cold ngspice
cache (release, two threads) and 3 s once cached. What did not reproduce
exactly, and how the tests say so: the swing fit's "9–11 mV rms" holds
over ±1.5 V with the cell's −20 mV resting offset taken out (10.0 / 10.0 /
11.1 mV); `sweep_res10`'s flat step reads −6.95 dB on its last 2 s
against the notes' −6.81 (window not recorded); `tanh_q` peaks at 3.8 %
between |u| = 1 and 1.5; the circuit's CI2A output reads 0.5 dB over
`out_gain()`, so `exp_c_scale` is nominal (OUT 4 agrees to 0.13 dB).
`fit_lin4` (the `comp` fit) is kept untested: its frame window was not
recorded.
