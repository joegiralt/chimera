# 0027. Shedding fades, and takes tails before held notes

- **Status:** Proposed (2026-09-27); extends [0015](0015-voice-steal-and-fx-returns.md); supersedes in part [0026](0026-algo-voices-billed-by-patch-shape.md) (its "steals the oldest held voice; it never goes over" clause)
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
  still counts in `sounding_cost`. It is never stolen or shed again.
- **Note-on during a fade:** a dying voice's cost does not count against a
  note-on, and with no voice free a note-on takes a dying slot and waits out
  its fade (at most two blocks, a bounded delay, not a block) before any
  held note is stolen: a late note is less surprising than a cut one
  ([#33](https://github.com/joegiralt/chimera/issues/33) M7).
- **Costs are current first:** `handle` recosts and sheds before it admits a
  note-on, so a note played with a patch edit is judged at the new cost
  (#33 M1).
- **Engine change:** a Sound that changes engine under a sounding voice
  fades the old engine out the same way, resets the voice, then restarts a
  held note on the new engine; a released tail just ends (#33 M6).
- **Overrun:** `shed` stops once the voices that are not dying fit, so the
  pool runs over budget by the dying voices only, for at most two blocks.
  This supersedes ADR 0026's clause that past the budget the allocator
  "steals the oldest held voice; it never goes over": steals take tails
  first (ADR 0015), and a shed may briefly go over.
- **Clean slot:** when the fade ends the voice is reset to the state a new
  voice has, engine and chain, so the next note on the slot starts clean.

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
- A note-on in those two blocks may start up to two blocks late.
- The overrun stays bounded by the physical pool: six voices at the
  costliest patch (842) plus the FX bus is 8,362, under the 10,000-cycle
  deadline.
- `FxBus::COST` is now 3,310, the MV reading of 3,300 rounded up like the
  other terms.
- ADR 0026's floor of four voices holds on rev V at 480 MHz only. A rev Y
  chip, or an unknown revision, runs at 400 MHz: 2,523 cycles beside the FX
  bus, so the costliest patch gets two voices and MORPH KEYS three
  (`cost_test::the_voice_floor_per_revision`).

## Sources
[#31](https://github.com/joegiralt/chimera/issues/31);
`.superpowers/sdd/2026-09-26-algo-engine-core/task-13a-review.md` (recost
policy); `chimera-core/src/voice_alloc.rs`, `chimera-core/src/dsp/voice.rs`,
`chimera-core/src/instrument.rs`.
