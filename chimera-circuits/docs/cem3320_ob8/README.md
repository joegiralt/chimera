# CEM3320, as wired in the OB-8 4-pole

Code: `chimera-circuits/src/cem3320_ob8/`. This is the CEM3320 built from its own circuit: Curtis's translinear log/antilog current cell (US 4,004,141 fig. 3) and a degenerated-pair resonance OTA, wired as the Oberheim OB-8 voice board's 4-pole. It is not built from the 2040.

## Models

| Name | What it is |
|---|---|
| `cem3320_ob8/reference` | `reference::render_ref`. Newton on 13 nodes per sample in f64 at 8× (384 kHz). Shockley/Ebers–Moll devices with Early effect, trapezoidal caps. It costs about 16.6 µs per 48 kHz sample (release, one core). |
| `cem3320_ob8/fw` | `fw::Fw` at 1×, the firmware candidate. It has closed-form implicit stages with a hard diode corner, per-stage rails, a polynomial one-step Newton OTA, pin 8's CX16 as a TPT one-pole, and CX20 AC coupling. It runs f32 inside. |
| `cem3320_ob8/fw2x` | The same at 2×. The lab used it only to check that 1× is enough: it came within 0.1 dB on every probe. |

Each model renders as the lab heard it: `Chip::OB8`, padded at rest with `util::harness::pad_run`, and passed through the house 15.9 Hz blocker `ac`. One digital unit is one OB-8 CEM3340 saw at the bus, 25.3 µA peak into pin 1. RES maps to loop gain by the house law `k = 4·RES/0.95` (`k_of`).

`chip::Chip::DATASHEET` is the data sheet's own circuit (R_I 91 K, R_RI 51 K, no pin-8 cap). It is used for validation.

## What shipped

Nothing shipped. At playing levels and RES ≤ 0.5 the chip nulls with today's 2040 REV2 to −21 … −43 dB. The differences that remain are:

- a louder, asymmetric ring
- the OB-8's CX16 squeal at high cutoff
- a lopsided clip at +9 dB and up

The lab left adopting them to the owner, as a product call. The estimated cost was ~230–250 M7 cycles a sample against a ~200 target, which would cost one voice on rev V. See `notes.md` ("The bottom line", "Cost", "Open questions").

## Lab history

- **`cem3320-lab`, 19c87c8e**: this model, ported from `chimera-utils/src/cem3320.rs`. The ledger is (private notes).
- **`curtis-lab`, cec352e9**: the earlier CURTIS lab (#720). It was the 2040 REV2 structure with a resonance limiter, and the owner rejected it. That model is not in the crate. Only its stimuli came over (`stimuli::{bassline, stab, pad}`). Its research and notes are kept as history: `curtis-history-research.md`, `curtis-history-notes.md`.

## Docs here

- `research.md`: sources, the cell, the OB-8 wiring, the reference, and how `Fw` was derived from it.
- `notes.md`: lab notes covering validation, fw vs reference, the A/B against REV2 and LADDER, cost, and open questions.
- `validation.md`: the reference against the data sheet (`cem3320-validate`).
- `probes.md`: drive probes, onset, cost, the A/B tables (with and without CX16), and the fw variants tried.

## Tests

`src/cem3320_ob8/tests.rs` holds the lab's `validate`, probes, onset and A/B as assertions. The thresholds are the ledger's numbers with a margin, alongside the data sheet's bound wherever the lab compared against one.

- **Fast (debug):** chip formulas, fig. 6 gm and knee, the `icr_of_k`/`gm` inverse, `ota`, `antilog`, `g_set`, quiescent points, fw DC unity and pole, and fw probe and ring numbers. They also check that the fw onset brackets the ledger's.
- **Slow (ignored in debug):** everything that runs the 8× reference. Run them with:

  ```
  cargo test -p chimera-circuits --release -- --ignored cem3320
  ```

  They cover:
  - the pole vs V_C slope
  - swing and THD
  - the resonance onset, ring and peak
  - noise
  - the OB-8 pole calibration
  - fw vs reference on the probes
  - the onset (with and without CX16)
  - the bassline null and LSD against the reference and REV2

## Left out, and why

- **Lab drivers.** `validate()`, `probe()`, `ab()` and `onset()` were printers that wrote markdown and WAVs, and `cost()` was a host timer. They are replaced by the tests and by the tables in `probes.md`. `ab_jobs`/`AB_*` (the WAV sets) and `out_dir` were dropped too.
- **The disk cache (`cached_ref`, `sha2`) and the parallel renders (`rayon`).** These were lab conveniences. No new dependencies were added.
- **`Which`.** It was only a renderer selector. `render_ref`, `Fw::render` and the `models()` entries replace it. The REV2 and LADDER renders went through the shipped `Filter`; that is test-only (`tests::rev2`).
- **Env knobs.** These are now plain fields or arguments with the lab's defaults:
  - `CEM_C8` → `Chip::c8`
  - `CEM_RE` → `Chip::re`
  - `CEM_W` → `Fw::w`
  - `CEM_NEWTON` → `Fw::newton`
  - `CEM_TRIM` → `render_ref`'s `fc_trim`
  - `CEM_OUT` / `CEM_NOCACHE` / `CEM_GROWTH_FC` are gone.
- **CURTIS's model** (`curtis::Curtis`, `limit_of`, `floor_neg` and its constants) was rejected.
- **No WAVs, images, service-manual dumps or the data sheet.** The ledger keeps those.

## Sources (cited, nothing copied)

- Curtis Electromusic Specialties, *CEM 3320 Voltage Controlled Filter* data sheet (1980). The reissue is at curtiselectromusic.com.
- US 4,004,141, D. R. Curtis, "Linear/logarithmic analog multiplier" (1977, expired): https://patents.google.com/patent/US4004141A
- Oberheim, *OB-8 Service Manual* (1983), voice board sheet 3 of 3 (1682B): archive.org `JL11369`.
- Oberheim, *OB-Xa Service Manual* (1982): archive.org `JL11372`.
- V. Zavalishin, *The Art of VA Filter Design* (TPT integrators; method only).

All code here is our own, written from those facts and from textbook device equations. There is no GPL or SuperCollider code. The only borrowed code these models run is the crate's shared `util::sig::resample_poly`, which follows SciPy (BSD-3) and carries SciPy's notice in its own header.
