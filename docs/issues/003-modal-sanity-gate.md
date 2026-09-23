# Issue #003: Modal engine fails the refactor sanity gate

Found by `chimera-core/tests/sanity_test.rs` (engine refactor, sub-project 1),
Modal init patch (`Patch::init(ChainType::Modal)`: mode String/KS+, pluck
position 0.0), note 60 vel 100, 48 kHz, 200 blocks on, 200 blocks off.

## Findings (measured at c1dbd23 + scope compile fix)

1. **Plays an octave high.** Normalized autocorrelation over blocks 50..150
   is 0.998 at a lag of 91 samples (527.5 Hz) and 0.999 at 183 (262 Hz): the
   waveform repeats every half period, so the fundamental is (almost) absent
   and note 60 is heard as note 72 (+12.14 semitones).
2. **Rings after note-off.** Peak per block after note-off: block 200 0.038,
   250 0.0083, 300 0.0071, 350 0.0062, 399 0.0053 — it does not reach
   −80 dBFS (1e-4) within 200 blocks (0.27 s).

## Status

Not fixed in the engine refactor. The goldens `modal_init`,
`modal_lfo_cutoff` and `pizza_to_modal_switch` lock this output bit-for-bit
(`golden_test.rs::KNOWN_BROKEN`). `modal_is_pitched` and
`modal_is_silent_after_note_off` are `#[ignore]`d until this is fixed; the
fix must re-record those goldens deliberately.
