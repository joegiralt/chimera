# ssm2040_prophet: the Prophet-5 rev 1/2 filter

An SSM 2040 wired as the Prophet's Voice schematic SD411, with half an SSM
2020 as the resonance VCA. The lab is #711 (circuit 1), branch
`circuit-2040`, commits fdbd0b53, f5646597 and df667280, 2026-10-20.

## Models (`chimera-circuits/src/ssm2040_prophet/`)

| Model | What | Render name |
|---|---|---|
| `reference::spice` | ngspice-45 on our own netlist (`netlist`): a transistor pair per cell, the Prophet's resistor network, U479 ×−10, the 2020 as a current. This was the lab's reference. | (needs ngspice; not a `Voice`) |
| `reference::Model` | The same node equations, solved by TR-BDF2 with Newton on the four caps, at 2× (f64) | `ssm2040_prophet/reference` |
| `fw::Fast` 2×, Padé | Candidate `d`: REV2's TPT structure on the circuit's numbers (the knee at the attenuator's scale, a soft swing limit, the offsets), in f32 | `ssm2040_prophet/d` |
| `fw::Fast` 1×, quintic | Candidate `e`: the same at 1× with the divide-free tanh. The only one near the unit's budget (+~110 cycles). | `ssm2040_prophet/e` |

**What shipped:** none of these.
- **Kept on the unit:** today's REV2, because at the Prophet's own level the circuit measures as the same clean four-pole (`ab-notes.md`).
- **Found by the lab and shipped:** the per-voice spread, as STABILITY (ADR 0168; `stability-notes.md`).
- **Not ported:** the lab's chord renderer (`stability.rs`) is a level-matched listening page, not a model. It stays on the branch.

## Validation (`tests.rs`)

| Test | Checks | Run |
|---|---|---|
| `the_candidates_are_the_circuits_linear_filter` | `d` and `e`: passband 0 ± 0.1 dB, −12 ± 0.3 dB at FREQ, bass loss 1/(1 + k) ± 0.2 dB at RES 0.85 (−13.2; ledger −13.1) | fast |
| `the_candidates_compress_as_the_circuit` | at +6 dB −1.0…−0.3 dB, at +12 −6.0…−5.0, H2 at +6 −42…−34 dBc (circuit −0.75, −5.5, −38) | fast |
| `todays_rev2_is_the_linear_filter_without_the_push` | the shipped REV2: the same small signal, no compression at +6, −5.0…−3.5 at +12 (ledger −4.5), no H2 | fast |
| `the_model_is_the_circuits_linear_filter` | as above, on the Newton model | slow |
| `the_model_compresses_early_and_lopsided_as_the_circuit` | the model's drive figures as the circuit's (measured −0.51 / −5.28 dB, H2 −38.3 / −44.8 dBc), and no H2 without the offsets | slow |
| `the_candidates_track_the_model_on_sweeps` | LSD < 0.3 dB on the cutoff sweeps (RES 0, ½, 0.85) and the RES sweep; `e` beats REV2 at RES 0.85 (0.18 against 0.43) | slow |
| `the_model_tracks_ngspice` | LSD < 0.3 dB; null < −40 dB on sines, < −15 dB at RES 0.85 (measured 0.01 / 0.03 / 0.11 dB; −46 / −45 / −19 dB) | needs ngspice |

```
cargo test -p chimera-circuits ssm2040_prophet
cargo test -p chimera-circuits --release -- --ignored ssm2040_prophet
NGSPICE=/path/to/ngspice cargo test -p chimera-circuits --release -- --ignored ssm2040_prophet
```

The ngspice test writes its netlists and data under
`target/test-out/c2040-spice/` and reruns only when they change.

## Docs here

- **`research.md`:** sources and licences, the chip, the 2020, SD411's values, the equations and the assumptions (the swing limit's shape, the offsets).
- **`ab-notes.md`, `ab-table.md`, `drive.md`:** the A/B against ngspice, the drive probes, the cost and the recommendation.
- **`stability-notes.md`, `stability-levels.md`:** the per-voice spread that became STABILITY.

## Open questions

The two things that make the circuit differ when pushed are not pinned down by the sources:
- the soft ±1 V swing limit;
- the cells' offsets.

A recording of a real rev 2 would settle them: a RES sweep, and drive at three mixer levels. See `ab-notes.md`, "Worth it?".

## Sources

- SSM 2040 and SSM 2020 data sheets.
- Sequential Circuits' Prophet-5 Technical Manual (Jan 1980, SD411).
- Zavalishin, *The Art of VA Filter Design*.

They are cited in `research.md` and read only; none is copied. Schmitz's
RS2040 page is "fair use only" and was read only for the cell's topology.
