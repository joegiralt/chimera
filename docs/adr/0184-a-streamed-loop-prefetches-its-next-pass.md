# 0184. A streamed LOOP prefetches its next pass's start a pass ahead

- **Status:** Proposed
- **Deciders:** owner (#698), firmware
- **Amends:** slice 1 § 5.7 (drop-outs fade), spec § 5.6 (REEL's "known a step ahead")

## Context
Musical finding 2. Each wrap of a streamed LOOP whose start wasn't on the staged loop head faded out over 3 ms, started cold and faded back in when its chunk landed: every wrap under WALK, an LFO into POSITION or LENGTH, and every wrap of a held note after a POSITION or LENGTH turn. `moved_loop_fades_at_the_wrap` pinned the drop-out as intended. The cursor meanwhile asked chunks past the loop's end that the head never read.

## Decision
- The deck hands the cursor the next wrap (`cursor::Wrap`) whenever it doesn't land on a RAM head: this pass's last read and fade, and what the next pass reads (`next_reads`: the fade's second head from where `heads::plan` starts it, through the landing and a block's travel).
- The cursor wants no chunk past the pass's end, and the next pass's start chunks in the slots that frees (`Cursor::wanted`), due by the fade's start. A jump that lands on a Ready slot keeps its slots and its level (`Cursor::jump`).
- The fade's second head reads the prefetched slots, up to two, handing over between them (`Reads::second_at`); a fade that starts before they land fades to silence whole (`mute`), and a second slot late for the head ducks the voice first, so nothing steps.
- Only a pass shorter than the card's read, or a burst, still drops out, through the same fades.

## Alternatives considered
- **A fourth slot a voice for the wrap.** 8 × 4 KB more SRAM4, which is full; the slots past the loop's end were wasted anyway.
- **Re-stage the loop head on every move.** It never reaches a held note's motion (WALK, routes).

## Consequences
- `moved_loop_lands_warm_at_the_wrap` replaces the drop-out pin: no cold wrap, and at XFADE 0 the streamed render equals the whole row's sample for sample. `walking_loop_lands_warm` covers WALK.
- No more card reads than before: the prefetch takes the reads the pass's end used to waste.

## Sources
- Musical finding 2; #698. Spec § 5.6 (REEL).
- `chimera-engine/src/stream/cursor.rs`, `dsp/tape_engine/{mod,span}.rs`, `chimera-core/tests/tape_stream_test.rs`.
