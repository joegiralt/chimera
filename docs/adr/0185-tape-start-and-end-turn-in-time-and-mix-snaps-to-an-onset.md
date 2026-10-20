# 0185. TAPE's START and END turn in time, and MIX + turn snaps to an onset

- **Status:** Proposed
- **Deciders:** owner (#698, "fix it all"), firmware

## Context
START and END were fractions stepped 1/128 of the source a detent: 33 ms on a 4.3 s RAM source, 195 ms on a 25 s streamed bed (adversarial review, musical finding 6). A few ms of pre-roll couldn't be trimmed and a phrase couldn't start on its consonant. The owner's unit has no push encoders, so the push-fine path (ADR 0140) is no answer. PREP (ADR 0171), which will own sample-accurate marks, isn't built.

## Decision
- **A turn moves START or END in time** (`ui::onset::stepped`): the encoder's accelerated ticks squared, in ms (1, 4, 16, 25, 100 ms a detent at `encoder_accel`'s 1, 2, 4, 5, 10), and at full speed at least a hundredth of the source, so a long bed crosses in a few turns. The stored value stays the fraction; only the step changes. Their cells and band read time: `137 MS` under a second, else `12.345 S` (`onset::time_text`).
- **MIX + turn snaps to the next onset the turn's way** (`onset::onset`): a cheap energy rise, 2 ms blocks of |x|, a block over 3× the two before it and over −48 dBFS, pulled back to the zero crossing before its first loud sample. More than a block from the mark, so a mark on an onset moves on to the next. With none, the mark stays. It reads what RAM holds: the whole row, or a streamed source's RAM heads (the rest is on the card, so a streamed bed snaps only inside its heads until PREP).
- **The hint says it:** START's and END's focus band reads `MIX + TURN  NEXT ONSET` (ADR 0086).

## Alternatives considered
- **A finer fixed step (1/1024).** Still source-relative: 24 ms on a 25 s bed, and a slow crawl across a short one.
- **Push for fine.** Not on the owner's unit (no-push-encoders rule).
- **Snap to zero crossings only** (PREP's SNAP, ADR 0171). Right for loop seams, not for finding an attack; PREP keeps it for its FOCUS.
- **Full transient detection (spectral flux).** More code in bank 1 for a gesture PREP will refine; the energy rise finds drum and consonant attacks, which is what a trim wants.

## Consequences
- Precedent: the MPC's sample edit "snap to transient/zero" and the Octatrack's slice editor both trim to attacks; Elektron's START is time-scaled in its editor.
- Bank 1 grows by the detector and the time format (see the PR's flash line).
- Goldens re-recorded: `tape_strip_oneshot*`, `tape_strip_loop_reversed`, `tape_src_pool`, `tape_src_loaded` (START and END read time).

## Sources
- #698; (private notes) finding 6.
- `chimera-core/src/ui/onset.rs`, `chimera-core/tests/tape_musical_ui_test.rs`.
- ADR 0140 (push encoders), 0171 (PREP), 0086 (hints).
