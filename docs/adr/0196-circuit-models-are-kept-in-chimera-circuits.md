# 0196. Circuit models are kept in chimera-circuits

- **Status:** Accepted (2026-10-21)
- **Deciders:** project owner (the rule: none of the filter and circuit modelling work is thrown away, shipped or not)

## Context
The filter epics (#711, #720) built circuit-level models of every filter the owner asked for. Each one came as an f64 reference (a nodal Newton solver, or ngspice on a netlist we wrote), a firmware-shaped model derived from it, and validation against data sheets, schematics and recordings. A few shipped as firmware KINDs or voicings: SEM (ADR 0192), EXP's voicing (ADR 0105, 0191), and STABILITY (ADR 0168). The rest did not, mostly for cost on the M7. They are the Prophet circuit and its candidates, the CEM3320 as the OB-8 runs it, the Kobol's EXP-C, the Buchla 292B with its vactrol, and the Plaits LPG port.

All of that lived only on unmerged lab branches, as `chimera-utils` modules organised by experiment. Each branch forked `chimera-utils` and carried its own copy of the 2040 lab's harness. The owner's rule is that none of it is thrown away. The models are wanted for a future synth on a bigger processor and for the planned plugins, so they have to stay buildable, and closed and commercially clean (ADR 0002, the DSP company plan).

## Decision
- **One host-only crate, `chimera-circuits`**, a workspace member. No firmware crate depends on it, and it is never in the STM32 build. The library depends only on `rustfft`. `chimera-core` and `chimera-hal` are dev-dependencies, so only the tests that compare a model with a shipped KIND can reach the engine. A model can move to another processor or a plugin without Chimera.
- **One module a circuit, not an experiment:** `ssm2040_prophet`, `ssm2040_kobol`, `cem3320_ob8`, `sem1a`, `buchla292b`, `vtl5c3`, `plaits_lpg`. Each module holds:
  - the f64 circuit or reference solver;
  - the firmware-shaped model;
  - its validation, as `#[test]`s with thresholds from the lab's recorded numbers.

  `util` holds what the labs shared: the four-pole harness, the measures, SciPy's `resample_poly` and the ngspice runner.
- **The research stays with the code.** Each lab's research, notes and validation tables go in `chimera-circuits/docs/<circuit>/`. Each copy names its ledger file, and a README in each folder lists the lab branches and commits it came from.
- **What is left out:**
  - Rejected tweak-models: `curtis-lab`'s REV2-tweak CURTIS, and `lpg-lab`'s invented parts, except where a kept model reuses a stimulus from them.
  - Drivers that only wrote WAV pages or caches.
  - M7 cycle-count probes and render outputs.

  The lab branches stay on origin as history.
- **One render path.** Every model renders as a `chimera_circuits::Voice`: offline, the whole input with per-sample controls, at its own level, never normalised.
  - `chimera-utils circuit list | render MODEL OUTDIR` plays a model on unitab's four scripts at RES 19, 64, 95 and 114, through unitab's lab source. It writes raw 32-bit float files.
  - Before rendering, it runs the A/B method's known-pair check (2040 REV2 against EXPNDR) on the same source and writer.
  - `unitab` (the full-engine A/B, from `ab-debug`) moves to main with it, with its scripts and the reference Sound bundled.
  - The kept models have no place in the engine yet, so their renders are filter-alone research. Playing them as desktop KINDs, without a codec change, is #731.
- **`just check` runs the fast tests** in debug, and clippy covers the crate. The slow references (8× Newton, long sweeps) and the tests that need ngspice or the owner's recordings are `#[ignore]`d with the command that runs them.
- **The licence rules:**
  - The crate holds our own code, plus permissive code with its notice kept in the file and an entry in `THIRD_PARTY.md`: the Plaits LPG port (MIT) and SciPy's `resample_poly` (BSD-3).
  - CC BY papers are credited in `THIRD_PARTY.md` and the circuit's docs: Köper et al. DAFx-22, and Najnudel et al. DAFx-23.
  - No GPL code, and no SuperCollider tool, script or render, goes in the repo.
  - No data sheets, schematics, scans or WAVs. The docs cite and link them; Schmitz's RS2040 page is "fair use only" and is not copied.
  - ngspice is an outside lab tool, found through `NGSPICE`. It is never linked or shipped.

## Alternatives considered
- **Merging the lab branches as they are.** Each one forks `chimera-utils` differently and is organised by experiment. Their drivers write into ledger paths, and several duplicate the same harness.
- **Leaving them on branches.** That breaks the owner's rule in practice: branches rot as main moves, and nothing builds or tests them.
- **Putting the models in `chimera-utils`.** It is an A/B tool, and it depends on the engine. The models have to stand alone for a plugin or another processor, so the tool depends on them, not the other way round.
- **Rendering the kept models through the engine now.** That needs an engine hook. Done host-only it costs the firmware nothing, but it is its own decision (#731).

## Consequences
- Every kept model builds and is tested on every `just check`, at a small added cost (the fast set runs in seconds).
- A model that later ships, as a KIND or in a plugin, starts from tested code and its research.
- A new circuit lab lands here as a module from the start, using `util`, not as a fork of `chimera-utils`.

## Sources
- Lab branches `circuit-2040`, `curtis-lab`, `cem3320-lab`, `sem-lab`, `sem-circuit-lab`, `lpg-lab`, `lpg-circuit-lab`, `exp-circuit-lab` and `ab-debug`.
- Ledgers (private notes) (with `ab-method.md`) and (private notes).
- #711 and #720 (the epics), #731 (desktop KINDs).
- ADR 0002, 0098, 0100, 0105, 0168, 0191 and 0192.
