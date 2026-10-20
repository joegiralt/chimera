# 0186. TAPE says what it can't play where the player is

- **Status:** Proposed
- **Deciders:** owner (#698), firmware
- **Amends:** [0178](0178-tape-model-lists-one-loop-and-ping-and-deck-holds-the-glyphs-knobs.md) (no TAPE page cell dimmed), [0151](0151-a-refused-kind-or-algorithm-is-shown-dimmed-with-its-reason.md) (held choices)

## Context
The adversarial review (musical findings 1, 3, 5 and 14) found TAPE silent or inert with the reason on another page or nowhere: a streamed chord stole its own notes with no sign of the limit; MODEL offered machines a streamed source can't play, and only SRC said why; WOW did nothing at TAPE 0; with AMP's release the one release a note (finding 3's engine fix), PLAY's FADE no longer ends LOOP or PING notes under a VCA route; and a streamed loop's XFADE silently capped at 100 ms (spec § 2.3).

## Decision
- **SOURCE's band gives a streamed source's polyphony:** `24K · 30.0 S · 7 STREAMS` (named: `CHOIR · 24K · 7 STREAMS`), the notes at ROOT the card's budget fits (`admit::notes_at_root`), wow billed when TAPE runs (stored above 0 or routed).
- **MODEL holds a machine a streamed source refuses** (`needs_ram`), as ADR 0151 holds a formant choice: the turn lands on it, the cell reads it dimmed, the band says `PING NEEDS RAM · STREAMED`, nothing is written; the next turn moves on from it. A RAM source turns freely.
- **WOW dims while TAPE rests** (stored 0, unrouted: `tape_runs`), the band in the hint's place: `TAPE AT 0 · NO WOW TO SHAPE`.
- **LOOP's and PING's FADE dim under a VCA route**: `AMP RELEASE ENDS THE NOTE`. ONE's FADE stays live (its HOLD step is the gate); TRON's FADE is the machine.
- **A streamed LOOP's XFADE past 100 ms of source** keeps its cell live and the band says `STREAMED · XFADE CAPPED AT 100 MS`.

## Alternatives considered
- **Skip refused machines on MODEL.** The list would change length with the source and a player couldn't see why PING vanished; ADR 0151 already set the dimmed-with-reason rule.
- **Write the refused machine and dim MODEL.** A dimmed cell ignores its encoder, so MODEL would stick.
- **Dim ONE's FADE too.** It would lose HOLD, which decides whether a hit plays to END.
- **A polyphony counter on SRC's meter.** The band is where the source is read; the meter is the pool's.

## Consequences
- Precedent: Elektron greys a parameter a machine ignores and says why in the info line; the Octatrack shows a static/flex machine's limits on its own page.
- Goldens re-recorded: every `tape_page_*` (WOW dimmed at INIT, LOOP's and PING's FADE dimmed).
- The band strings fit 216 px (`tape_musical_ui_test`).

## Sources
- #698; (private notes) findings 1, 3, 5, 14.
- `chimera-core/src/ui/{view,held,renderer}.rs`, `chimera-core/tests/tape_musical_ui_test.rs`.
