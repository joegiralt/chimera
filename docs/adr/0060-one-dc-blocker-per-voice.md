# 0060. Block DC once per voice, after its last nonlinear stage

- **Status:** Proposed
- **Deciders:** owner; firmware (Modal 2, task 17)
- **Supersedes in part:** ADR 0022 (its "DC blocking — not added" rule;
  the waves keep their DC inside the engine as before); ADR 0056's voice
  counts (below)
- **Relates to:** ADR 0023 (W3, W4, W7 and W8 keep their DC), ADR 0056
  (the string models' 10 Hz output blocker)

## Context
The parameter sweep (`chimera-core/tests/param_sweep_test.rs`) found DC on
the DAC from three places:
- **The voice's nonlinear stages.** Nothing after the drive, the filter's
  saturating SVF or the folder removed the DC they make from asymmetric
  waves. The folder's SYM adds a bias before its fold, so at SYM 0 a note
  put 0.032 FS on P1, −0.050 through its silent release, and P1 stepped
  when the voice freed. A bright or fast bow drifts below the engine's
  10 Hz blocker, and the voice passed that drift on (0.0083 FS at BRIGHT
  1, 0.0142 at SPEED 1).
- **The Modal engines.** BANK's output had no blocker (0.0091 FS at
  STRUCTURE 1). SYMP drifted below 10 Hz (0.0171 FS at PITCH +24): a
  pluck's mean rings as the string loop's 0 Hz mode for the whole T60,
  and each halo string's comb gains the main string's by 1 / (1 − g).
- **Algo.** ADR 0022 declined a DC blocker, since only W3, W4, W7 and W8
  carry DC, as on the TX81Z. On INIT's carrier W3 put 0.23 FS on P1. The
  TX81Z's own output is AC-coupled, so its DAC never carried that DC.

## Decision
- **One blocker per voice:** a one-pole high-pass
  (`dsp::dc_blocker::DcBlocker`, gain ≤ 1 at every frequency) after the
  folder, before the VCA. Every stage that makes DC (the engine, the drive,
  the filter, the fold) is before it, so none reaches the VCA, the mix or
  the DAC. The VCA scales a signal already free of DC, so its envelope
  can't turn DC into thumps. A fresh note starts it at rest.
- **Its corner is 5 Hz** (`voice::DC_HZ`), so it doesn't colour the
  bass: −0.26 dB and 14° at 20 Hz, −0.06 dB at E1 (41 Hz). It settles in
  3τ = 95 ms. A 10 Hz corner would take −0.97 dB at 20 Hz; a lower one
  settles slower and passes more sub-audio drift.
- **The drive, the filter and the folder see the engine's DC as before.**
  Their asymmetric character, and the TX81Z waves' DC inside the operators
  (ADR 0023), stay; only the output loses the DC.
- **The folder's SYM shapes the fold, not the level:** it folds
  `(x + bias)·gain` less the fold of silence, `fold((bias)·gain)`. Silence
  folds to silence, so no offset steps in at a note-on or out as the
  voice frees.
- **The Modal engines keep their own 10 Hz output blocker (ADR 0056), now
  on all four models:** BANK gets it after its tanh. SYMP's drift is
  fixed where it starts: a pluck is zero-mean (`KsString::shape`), so
  the loop's 0 Hz mode is never struck. The blocker keeps DC out of the
  voice's drive, filter and fold; the voice's blocker is what the DAC
  relies on.

## Alternatives considered
- **A blocker after each nonlinear stage:** three per voice, three times
  the cost, and the stages' own DC would still meet the VCA.
- **After the VCA, or on the mix:** a VCA or a fade moving over DC makes
  steps the high-pass passes as thumps. On the mix, one Part's DC would
  still pass through its level, pan and sends.
- **Leave Algo's DC (ADR 0022):** 0.23 FS on P1 costs headroom and
  thumps as a note ends. The TX81Z's jack never carried it.
- **Fix every source instead of a blocker:** the drive, the filter and
  the fold make DC from any asymmetric wave, by design. BOWED's
  sub-10 Hz drift, which BRIGHT and SPEED raise, is left to the voice's
  blocker. Its source is the bow's stick–slip, and changing it is the
  bow's own work (#240).

## Consequences
- **Cost:** 6.5 instructions a sample (24 per 4 samples, unrolled), 10
  cycles by ADR 0056's host method. With ADR 0061's eases, `CHAIN_COST`
  goes from 10 to 30. The folder's offset adds 1 instruction a sample.
- **Voices,** beside the FX bus at 1,180 (ADR 0061), rev V then rev Y:
  - MORPH PAD 7 → 6 on rev V; SQR BASS 8 → 7 on rev Y.
  - SYMP with BODY and the ensemble: 7 → 6 on rev V.
  - The costliest patch keeps 6 on rev V and 5 on rev Y.
  - With the master tape, ALGO INIT plays 7 on rev V, not 8. It had 2
    cycles to spare: 8 × 691 + 1,470 = 6,998.
  - Every factory Sound keeps at least 6 voices on rev V and 5 on rev Y,
    in both builds.
- **Every audio golden was re-recorded.** With the blocker, the folder's
  offset, BANK's blocker and the zero-mean pluck switched off, each one
  matched its old value bit for bit (`golden_test`, `instrument_test`,
  `codec_compat_test`, `factory_level_test`).
- **MORPH PAD** plays within +1.42 dB of 937b89f over its LFO sweep, up
  from +1.35 dB (#192).
- **Sweep:** DC is judged 95 ms after a note-on, once the blocker has
  settled. `sweep_fast`'s hold is 225 blocks, so each half of the window
  is still 100 ms. FM sidebands and the bow's drift below 5 Hz still
  read as DC, and are allowed with that reason.

## Sources
`chimera-core/src/dsp/{voice,dc_blocker,wavefolder}.rs`;
`chimera-core/src/dsp/modal/{mod,string}.rs`; the parameter sweep's
report (Task 16, t16-sweep-r1) and Task 17's; the pins
`the_folder_sym_puts_no_dc_on_the_dac`, `a_bright_bow_puts_no_dc_on_the_dac`
and `the_modal_engines_put_no_dc_on_the_dac`; J. O. Smith, *Introduction
to Digital Filters*, "DC Blocker".
