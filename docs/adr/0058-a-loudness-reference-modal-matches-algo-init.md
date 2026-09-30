# 0058. A loudness reference: every Modal model's INIT plays as loud as ALGO INIT

- **Status:** Superseded by [0063](0063-levels-at-the-factory-median.md)
- **Deciders:** owner; firmware (Modal 2, task 16)
- **Relates to:** ADR 0050 (the output trim and the −1 dBFS limiter),
  ADR 0056 (Modal's four models, `BOW_OUT`, #231)

## Context
On hardware the owner hears the physical models as much quieter than FM.
The parameter sweep (`chimera-core/tests/param_sweep_test.rs`, 83e40a3)
confirms it. It measures loudness through the real `Instrument` on P1,
after the output stage. Against ALGO INIT, averaged over C3, C4 and a
chord at velocities 64 and 127, the models measured:

| Model | Δ vs ALGO INIT |
|---|---|
| STRING | −20.2 dB |
| SYMP | −14.7 dB |
| BANK | −10.6 dB |
| BOWED | −4.0 dB |

Nothing tied one engine's level to another's:
- ADR 0050 sets one system trim, 1/√8, and a ceiling. It sets no level
  per engine.
- ADR 0049 makes Algo equal-power across its algorithms.
- `BOW_OUT` (1.16) matches the new bow to the v1 bow, not to Algo.
- The Modal paths have no output gain. A pluck is `vel·EXCITE` noise
  through the loop and BODY, and BANK's level is its tanh's ceiling.

## Decision
- **The reference is ALGO INIT's loudness on P1.** The measurement:
  - one note, C4, at velocity 100, held 1 s;
  - through the `Instrument`, with the default Part and FX off;
  - ungated BS.1770 (K-weighted) loudness over the hold, after the
    output stage (`common::sweep::level`).

  It reads −15.0 LUFS, pinned as `REFERENCE_LUFS` (ALGO INIT within
  ±0.1 dB of it). ALGO INIT stays where it is, and so does every Algo
  patch.
- **Every engine's and Modal model's INIT lands within ±1 dB of it.**
  Algo is the reference. Each Modal model gets one named output gain,
  `modal::out_gain`:

  | Model | Gain | dB | Before, at the reference note |
  |---|---|---|---|
  | STRING | 10.96 | +20.8 | −20.8 dB |
  | BANK | 2.10 | +6.4 | −6.4 dB |
  | BOWED | 1.62 | +4.2 | −4.2 dB |
  | SYMP | 4.24 | +12.6 | −12.6 dB |

- **Applied at the voice's VCA.** `EngineSlot::out_gain` returns 1 for
  Algo and the model's gain for Modal. `Voice::render` multiplies it into
  OUT LEVEL once a block, so it costs nothing per sample and no Algo bit
  moves. A voice's model is fixed from note-on to note-on, so the gain
  never steps under a sounding note.
  - It comes after BANK's `tanh·2`, SYMP's tanh and the voice's drive,
    filter and folder. None of them is driven harder than before. In
    particular the bank's saturation (#231) is unchanged.
  - The engine still judges its own silence before the gain, so voice
    lifetimes don't change.
- **`BOW_OUT` stays in the bow.** It sets the bow's level into the voice's
  drive and filter, as the v1 bow's did.
  `a_v1_bowed_patch_bows_in_tune` pins it at the engine. Folding it into
  `out_gain` would move every Bowed patch's drive and filter operating
  point by 1.3 dB. BOWED's 1.62 is on top of it: 1.88 from the bow's line
  to the VCA.
- **Pinned by `modal_models_match_the_loudness_reference`.** It checks
  each model's INIT against the reference within ±1 dB.
- **The limiter stays a safety ceiling (ADR 0050).** No INIT's chord
  (C3 E3 G3 C4) at velocity 127, the hardest strike, may lose more than
  3 dB of its loudness to it: the limiter may duck a strike, not hold the
  note down. The same test checks this.

## Alternatives considered
- **The gain at the end of `ModalEngine::render`.** This was tried
  first.
  - The voice's filter saturates its integrator states (`filter.rs`
    `saturate`: soft over 1, flat at 1.5), and a pluck as loud as ALGO
    INIT peaks at about 3. The filter clipped each strike: STRING's
    velocity response from 50 to 100 fell from 6.0 dB to 4.1 dB, and the
    gain had to be 14.5 rather than 10.96 to reach the reference.
  - The clipping put DC on the bow's and the bank's notes. The engine
    also judged its silence after the gain, 10 to 23 dB further down the
    ring, so voices lived longer.
  - At the VCA the gain is exact, linear and bit-for-bit elsewhere.
- **The factory median (about −22 LUFS) as the reference, with ALGO INIT
  brought down to it.** The sweep recommended this. It was rejected
  because the complaint is that Modal is quiet, and existing FM patches
  must not change.
- **A per-Sound level.** It would not fix INIT or old patches, and it
  leaves the engines unrelated.

## Consequences
- **Modal patches get louder by their model's gain.** Old patches and
  cards included. That is the fix. No Algo golden moves. The Modal
  goldens were re-recorded (`golden_test`, `instrument_test`
  `two_parts_two_pairs`, `codec_compat_test` `init_modal.snd`). The
  v1 Bowed pins are engine-level and unchanged.
- **STRING's chord is limited.** A pluck's peak sits about 11 dB higher
  for its loudness than ALGO INIT's sustained tone. At the reference, the
  limiter takes this much from each INIT's chord:

  | Model | Velocity 100 | Velocity 127 |
  |---|---|---|
  | STRING | 8.4 dB peak, 1.78 dB loudness | 10.2 dB peak, 2.79 dB loudness |
  | SYMP | 0 | 1.7 dB peak, 0.17 dB loudness |
  | BOWED | 0.3 dB peak, 0.02 dB loudness | 0.4 dB peak, 0.03 dB loudness |
  | BANK | 0 | 0 |

  STRING holds the 3 dB rule by 0.19 dB. ADR 0050's limiter holds the
  ceiling, but this is the first INIT it works on. A lower reference, such
  as the factory median (about −22 LUFS), would give it back its
  headroom. That is the owner's call, and would be a superseding ADR.
- **A Modal voice now peaks over 1.** STRING INIT peaks at about 3.0 at
  the VCA. The sanity gate's ±1 bound is scaled by the model's gain for a
  Modal case. The Modal tests' absolute click and bound checks read a
  voice at its model's level (`common::at_model_level`).
- **Some DC now shows.** Each model's DC keeps its ratio to the note, but
  its absolute level rises with the gain. At the DAC it now crosses the
  sweep's −46 dBFS in a few cases:
  - BOWED BRIGHT high: the bow's own drift below 10 Hz (#248);
  - SYMP at COUPLE or HALO 1, or PITCH +12: the halo drifts below the
    blocker's 10 Hz;
  - BANK at STRUCTURE 1: the bank's output has no blocker.

  ADR 0060's per-voice blocker removes all three. Live tests pin it:
  `a_bright_bow_puts_no_dc_on_the_dac` for BOWED and
  `the_modal_engines_put_no_dc_on_the_dac` for SYMP and BANK.
- **Velocity response is still per path:**
  - ALGO INIT and BOWED: 0 dB;
  - STRING and SYMP: +6 dB from velocity 64 to 127;
  - BANK: +3.5 dB.

  So at velocity 64 STRING sits 4 dB under the reference, and at 127
  it sits 2 dB over. A velocity policy is left to its own ADR.
- **When #231 re-stages the bank's tanh, BANK's loudness will move.**
  Its gain must then be re-measured. This test fails until it is.
- The reference sits at ALGO INIT, 7 dB over the factory median. The
  Modal INITs now read HOT against the median too.

## Sources
- `chimera-core/tests/param_sweep_test.rs` and `tests/common/sweep.rs`
  (the level measure, `level`; the loudness test; the report's tables).
- ADR 0049 (Algo INIT and carrier power), ADR 0050 (trim and limiter),
  ADR 0056 (`BOW_OUT`, the bank's tanh, #231).
- `chimera-core/src/dsp/filter.rs` (`saturate`), `dsp/voice.rs` (the
  VCA), `dsp/modal/mod.rs` (`out_gain`).
