# 0335. TRON's run-out eases in over its block

- **Status:** Accepted (2026-11-23)
- **Deciders:** firmware (bug batch G, #1126; the owner's rule: no known bugs at release)

## Context
`factory-tapes qa` failed on 0.4: `walk-epiano-c4-tron` clicked at 7.999 s (4.47×) and `walk-tx-bass-c2-tron` at 7.999 s (4.18×), where the 8 s tape runs out. ADR 0203 has the run-out fall in 2 ms. The deck did that with a one-pole (`RUN_OUT_S`, τ 2 ms) that started on the first sample of the block the tape ends in. A one-pole starts at its full slope (1/96 a sample at 48 kHz), so the gain had a corner there. On a loud, low or slow wave that corner bends the wave further than its own curvature: a click. Quieter TRON renders passed, and that is why only two of the 23 failed.

## Decision
1. **The run-out block eases into the fall.** In the block the tape runs out in, the gain is `g + (h − g) · w`, where `g` is the run-out's one-pole, `h` the fall the gain was on (1 held, FADE's one-pole after key up), both from the block's first gain, and `w = 1 − smoothstep(i / 64)`. The gain leaves the old path with its slope and lands on the one-pole with its own, so neither end has a corner (`tape_engine::run_out::onset`).
2. **Every other sample plays as before, bit for bit.** The one-pole's gain is stepped through the block exactly as the release loop steps it, and handed on, so the blocks after the run-out fall as they did. Only the samples of the run-out block change. Through the voice, the filter carries that block's change under −80 dB until the note ends.
3. **The fall stays ADR 0203's**: a 2 ms one-pole after its first block, ending at −80 dB (`TRON_FLOOR`).
4. **The onset is out of line, in an S4 group.** TAPE lies in S0-S3, which ORBIT had full: inlined, the onset overflowed S0-S3 into MIXER's slot by 416 B. `run_out` is its own module, which TAPE's S0-S3 globs (`*11tape_engine4tron*` and the rest) don't name, and `build.rs` places `*11tape_engine7run_out*` in the S4 groups.

## Alternatives considered
- **A 2 ms smoothstep in place of the one-pole.** Click-free too, but it cut the old tail (about 17 ms from −6 dB to −80 dB) to silence, a change past the tape's last block that would go to the owner's ear.
- **Raising `factory-tapes qa`'s click bar.** It hides the click instead of removing it.

## Consequences
- Sound: only the run-out block of a TRON note changes. 23 of `factory-tapes qa`'s 257 renders move, at about 63 samples a run-out. The other 234 are bit-identical. `golden_test`'s `tape_tron_held` and `tape_eff_hash_test`'s 108 TRON FORWARD and REVERSE rows (each runs its tape out) are re-taken for it. No other golden moves, TINZ's and WAVE MORPH's included.
- CPU: one 64-sample loop once a note, at the run-out.
- Flash: bank 1 +0 B on every set. Bank 2, measured on 0.4 9898eef5d: default +576 B, orbit +576 B, bank2-gate +608 B, all in S4.

## Sources
- #1126; `factory-tapes qa` (`chimera-desktop/src/bin/factory-tapes.rs`, `listen::qa_takes`).
- Code: `chimera-engine/src/dsp/tape_engine/run_out.rs`, `mod.rs` (`render_reads`'s run-out), `tron.rs` (`RUN_OUT_S`), `chimera-stm32/build.rs`.
- Test: `chimera-desktop/tests/factory_tapes_test.rs` `tron_runs_out_click_free`.
