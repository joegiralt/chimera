# 0027. Shedding fades, and takes tails before held notes

- **Status:** Accepted (2026-09-27); extends [0015](0015-voice-steal-and-fx-returns.md) and [0026](0026-algo-voices-billed-by-patch-shape.md)
- **Deciders:** project owner

## Context
Since ADR 0026 an Algo voice's cost follows its patch, so a knob turn on a
held chord can push a Part over budget. `recost` then called `shed`, which
hard-cut the newest held voice, with a click, even while released tails
were still ringing ([#31](https://github.com/joegiralt/chimera/issues/31)).

## Decision
- **Order:** `shed` takes non-mono voices before mono ones; among them the
  oldest released tail first (as ADR 0015 steals), then the newest held
  note.
- **Fade, not cut:** a shed voice ramps linearly to 0 over `Voice::FADE`,
  two blocks (128 samples, 2.7 ms), then goes inactive and frees.
- **Dying:** through the fade the voice keeps its slot, is marked dying and
  still counts in `sounding_cost`. It is never stolen, retriggered or shed
  again. A note-on meanwhile steals elsewhere or is refused.
- **Overrun:** `shed` stops once the voices that are not dying fit, so the
  pool runs over budget by the dying voices only, for at most two blocks.

## Alternatives considered
- **Price at note-on only:** the overrun would last as long as the notes
  are held, up to the whole UI reserve.
- **Shed only above a higher ceiling:** spends the UI reserve before we
  have chip numbers.
- **Release the voice instead of fading it:** a long release keeps the
  overrun going for its whole tail.

## Consequences
- A knob turn mostly eats tails, and a shed note fades out without a click.
- For up to two blocks the audio thread may take more than the 70% budget,
  still inside the 30% UI reserve below the deadline; the UI loop, not
  audio, is briefly delayed.
- A note-on in those two blocks may be refused that would fit a moment
  later.

## Sources
[#31](https://github.com/joegiralt/chimera/issues/31);
`.superpowers/sdd/2026-09-26-algo-engine-core/task-13a-review.md` (recost
policy); `chimera-core/src/voice_alloc.rs`, `chimera-core/src/dsp/voice.rs`,
`chimera-core/src/instrument.rs`.
