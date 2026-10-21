# chimera-circuits

Every filter and gate model the circuit labs built, shipped or not, kept
buildable (ADR 0196). The owner's rule is that none of this work is thrown
away. The models are kept for a future synth on a bigger processor and for
the plugins, so this crate stays closed and commercially clean.

It is host only. No firmware crate depends on it, and it isn't in the STM32
build. The models don't need Chimera at all: `chimera-core` is only a
dev-dependency, for the tests that compare a model with a shipped KIND.

## Layout

Each module is one circuit, not one experiment. It holds:
- the f64 circuit or reference solver;
- the firmware-shaped model derived from it;
- its validation, as tests.

`docs/<circuit>/` holds the lab's research, notes and validation tables.

| Module | Circuit | Models (`chimera-utils circuit list`) |
|---|---|---|
| `ssm2040_prophet` | SSM 2040 + SSM 2020 loop, Prophet-5 rev 1/2 (SD411). Not shipped (REV2 kept; STABILITY shipped, ADR 0168). | `reference` (Newton, 2×), `d`, `e`; ngspice netlist |
| `ssm2040_kobol` | SSM 2040 + CA3080 loop, RSF Kobol Expander. EXP-C not shipped (EXP is ADR 0105/0191). | `exp_c`, `exp_c_typical`; ngspice netlist (not a `Voice`); the owner's takes' measures |
| `cem3320_ob8` | CEM3320 from US 4,004,141's cell, wired as the OB-8 (and the data sheet's circuit). Not shipped. | `reference` (13-node Newton, 8×), `fw`, `fw2x` |
| `sem1a` | Oberheim SEM-1A VCF. `fw` shipped as KIND SEM (ADR 0192); tests pin the shipped constants and fixtures to it. | `reference` (15-node Newton, 8×), `fw`, `fw_exact`; the first lab's `ota_svf` (superseded) |
| `buchla292b` | Buchla 292B low-pass gate (B21208), its LED driver and vactrol. Not shipped. | `reference`, `reference-gate`, `reference-combo` (8× Newton), `fw`, `fw-gate`, `fw-combo` |
| `vtl5c3` | VTL5C3 and VTL5C3/2 vactrols (Najnudel et al. DAFx-23), f64 and the f32 firmware cell | none (no audio path; the 292B's cell) |
| `plaits_lpg` | Plaits' LPG (MIT), the 292B's behaviour reference | `port` |

Docs: `docs/ssm2040_prophet/`, `docs/ssm2040_kobol/`, `docs/cem3320_ob8/`, `docs/sem1a/`, and `docs/lpg/` for the three LPG modules.

`util` is what the labs shared:
- the four-pole harness (`Case`, padding, the 15.9 Hz output coupling, saws, sweeps, probes);
- the measures (`rms_db`, `bands`, `lsd`, `null_db`, `tone`);
- SciPy's `resample_poly`, ported under its BSD-3 notice;
- the ngspice runner.

## Auditioning a model

```
chimera-utils circuit list
chimera-utils circuit render cem3320_ob8/fw OUTDIR
```

`render` plays unitab's four scripts (acid, bass, sweep, orbit) at RES 19,
64, 95 and 114 through the model. It uses unitab's lab source: a saw at ½,
gated by note, with CUTOFF and RES from the script's CCs. It writes
`OUTDIR/ckt_<model>_<script>_r<res>.wav` as 32-bit float at the model's own
level. Nothing is normalised.

Before it renders, it runs the A/B method's known-pair check on the same
source and writer (private notes). It
renders 2040 REV2 against EXPNDR and fails unless EXP comes out +5.6 / +6.4
dB (±1) at RES 64 / 114, as on the unit.

- **Kept models:** these have no place in the engine yet, so they render
  through the filter-alone harness. That makes them research, not a page
  source. Playing them in the engine is #731.
- **Shipped KINDs:** use `chimera-utils unitab render SOUND OUTDIR`, which
  renders through the full engine. Compare any two files with
  `chimera-utils unitab cmp A B`.

## Tests

```
cargo test -p chimera-circuits                       # the fast set; `just check` runs it
cargo test -p chimera-circuits --release -- --ignored # the slow references (8× Newton, long sweeps)
NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored
```

The ignored tests are slow references (8× Newton, long sweeps) or need
outside data:
- **ngspice** (a lab tool, never in the repo): `NGSPICE` names the binary.
- **The owner's Kobol takes** (never in the repo): `KOBOL_DIR` names the session folder.

Each ignored test's reason names its command.

## Licence rules

- Our own code, plus permissive third-party code with its notice kept in
  the file. `THIRD_PARTY.md` lists each one: the Plaits LPG port (MIT),
  SciPy's `resample_poly` (BSD-3).
- Papers used under CC BY 4.0 are credited in `THIRD_PARTY.md` and the
  circuit's docs: Köper et al. DAFx-22 (SEM-1A) and Najnudel et al.
  DAFx-23 (VTL5C3).
- No GPL code, and no SuperCollider tool or script. These stay out of the
  repo, as do any references built from them. Where the labs listened to
  one, the docs cite it as an outside reference.
- No data sheets, schematics or scans. Their licences don't allow it
  (Schmitz's RS2040 page is "fair use only"), so the docs cite and link
  them instead.
- No WAVs or render outputs. They stay in the ledgers, and `circuit render`
  makes them again.
