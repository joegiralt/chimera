# SEM-1A (Oberheim SEM VCF)

The Oberheim SEM-1A's 12 dB OTA state-variable: two CA3080 integrators behind 100 K/220 Ω, 2N4302 source followers, a diode antisaturator in the RES path, and a MODE pot that crossfades LP through a moving notch to HP. Epic #720.

## Models (`src/sem1a/`)

| Name (`chimera-utils circuit render`) | Code | What it is |
|---|---|---|
| `sem1a/reference` | `reference.rs`, `fw::render_ref` | The VCF sheet as built: 15 node unknowns, trapezoidal companions, Newton with an analytic Jacobian, at ×8 (384 kHz). It includes A12 (LM301A: GBW, slew, rails), the CA3080s (Ebers–Moll pair, base currents, saturating mirrors, r_o), the followers, A14, PS16 with R157, D11/D12 with R156, and C15/C18/C22/C23. Slow. |
| `sem1a/fw` | `fw.rs` | The firmware model derived from the reference: a TPT SVF in the circuit's sign and volts, f32 at 48 kHz. It **shipped** as filter KIND SEM (ADR 0192, `chimera-engine/src/dsp/filter/sem.rs`). Rendered with the C15/C23 couplings, as in the lab's A/B. The firmware leaves them to the voice's DC blocker. |
| `sem1a/fw_exact` | `fw.rs` (`Fw::exact`) | `fw` with the followers folded into the solve, a divide a sample. Measured no better, not shipped. |
| `sem1a/ota_svf` | `ota_svf.rs` (`Sem`) | The first lab's candidate. Simplified equations: CLEAN's RES law, an invented Q 25 floor, 5 V a unit, no followers. Superseded by the circuit model but kept: it is cheaper (80–90 cycles a sample on the lab's M7 probe, against 120–130). |
| `sem1a/ota_svf_osc` | `ota_svf.rs` (`K_OSC`) | `ota_svf` on its oscillating floor (k −0.06, held by the diodes): a SEM that self-oscillates, which the circuit cannot do (open question 5 in `notes.md`). |
| `sem1a/ota_svf_ref` | `ota_svf.rs` (`Model`) | The first lab's own reference: the same simplified equations at ×4 by Newton. It is circular, not the circuit. |

Every entry maps CUTOFF to fc in Hz and RES to the model's knob (0..1), at LEVEL 0 dB. MODE is fixed at LP (0); `fw::Case` and `ota_svf::Case` take MODE and BP directly. `export.rs` regenerates the shipped constants (`consts()`, the lab's `semc-consts`) and the firmware's fixture file (`fixtures()`, `semc-fixtures`). `validate.rs` holds the schematic's algebra for `k` and the small-signal measures. `stimuli.rs` holds both labs' listening set and probes.

## Tests

The fast tests run under `just check` (debug). They cover:
- the reference's DC point, the CA3080, the followers and the Jacobian;
- `k` from the parts and the antisaturator;
- the old model's diode fit, ring-out and gain at cutoff;
- the pins on the shipped KIND: the constants equal the kernel's literals, `fw::coefs` equals `sem_g`/`sem_k`, the regenerated fixtures equal `chimera-core/tests/fixtures/sem_fw_v1.bin` byte for byte, and the real `Filter` nulls this `fw` under −90 dB.

The slow ones are ignored. They cover the ×8 small-signal grid, the notch, the fw-against-reference probes, the 75-job A/B sets of both labs, and the diode fit search:

```
cargo test -p chimera-circuits --release -- --ignored sem1a
```

Each assertion's bound is the recorded number from `validation.md`, `probes.md`, `ab-table.md`, `sem-lab-probes.md` or `sem-lab-ab-table.md`, plus a small margin. The test's doc comment names its number.

## What reproduced

Every recorded number the tests read reproduced, from the DC point to the 75-job means; the fixture file regenerates byte for byte. Four of the ledger's *summaries* were looser than its own tables, and the tests follow the tables:
- `notes.md` says k measured is "within 0.002" of the algebra at 100 Hz for RES ≥ ½. The table (`validation.md` § 5) has 0.0128 at ½ and 0.0034 at 0.9; 0.002 holds only from RES 0.98.
- ADR 0192 and `notes.md` put the fw's MODE notch "within 0.4 %" of the circuit's. It is 0.34 / 0.41 / 0.51 / 0.52 / 0.61 % (m 0.1 … 0.9).
- `probes.md`'s "3.32 Newton iterations a step" counts the 50 ms settle; over the driven second alone it is 3.84.
- The shipped kernel's `g` (the engine's `tan_accurate`) reads low against the lab's `tan` above 3 kHz: 5.2e-4 at 10 kHz, 1.1 % at 15 kHz, as `coef.rs` documents for every kind. The fixtures stop below that, so the −90 dB null never saw it.

## Docs here

- `research.md` and `notes.md`: the circuit lab. These hold the sources, the circuit as read, the validation table, how the circuit differs from CLEAN, cost and open questions.
- `validation.md`, `probes.md` and `ab-table.md`: the circuit lab's raw outputs.
- `sem-lab-research.md`, `sem-lab-notes.md`, `sem-lab-probes.md` and `sem-lab-ab-table.md`: the first lab.

## Provenance

- Branch `sem-lab`, def4f753: the first lab (`chimera-utils/src/sem.rs`).
- Branch `sem-circuit-lab`: aa4cb6a2 (the circuit, the fw model, validation and A/B), cdb9bf2a (a field dropped) and 9582667d (`export.rs`).
- Shipped as KIND SEM by ADR 0192 (`docs/adr/0192-sem-filter-kind-from-the-sem-1a-circuit.md`).

Sources (cited, not copied; the lab ledgers keep local copies):
- L. Köper, M. Holters, F. Esqueda, J. D. Parker, "A Virtual Analog Model of the EDP Wasp VCF", *Proc. DAFx20in22*, Vienna 2022, pp. 17–24, licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). We used its method (one nodal system solved by Newton) and its measured CA3080 fit (α 0.8635, β 0.9408, Table 3). No text or code was copied. It is credited in `THIRD_PARTY.md`.
- Oberheim Electronics, *SEM-1A schematic* (a service document): part values and topology only.
- Datasheets, for facts only: Intersil CA3080 FN475, 2N4302 (onsemi PN4302), the 1N4148 SPICE card, and the National LM301A and 741.
- V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.0 (NI 2018), § 4.4, § 6.9 and § 6.11: the frame for `fw` and `ota_svf`.
- Our own Rust throughout. No GPL code was read or used.
