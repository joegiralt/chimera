# 0065. A Part's own steal cuts, or glides the ring to the new note

- **Status:** Proposed
- **Deciders:** project owner (#254, "instead of the note being stolen, it
  glides up to the new note, like the Prophet 5/6 glide"); firmware

## Context
ADR 0062 lets a Modal note ring free after note-off, so a full pool is
mostly ringing tails, and a new note steals the oldest. A steal of the
same Part and model already triggers in place: the new note strikes the
voice at its own pitch (a new pluck on a cleared line, a new burst on the
bank's modes). The owner hears a stolen string as one of six guitar
strings: re-plucked, it could slide to the new note instead, as a
Prophet 5 or 6 glides a voice from its last note.

A steal of another Part, or of another model, fades the voice out
(`Voice::FADE`) and starts it clean: its sound and settings are not the
new note's.

## Decision
- **Two per-Sound params on PIT (the voice's pitch page):** STEAL, CUT
  (default) or GLIDE, on C; GLIDE TIME on D, 1 ms to 2 s on an
  exponential slider (`law::GLIDE_TIME`), 150 ms at INIT. Neither is a
  modulation destination: both are read at the steal. New card keys
  (`PITCH` block, ids 2 and 3); a file without them decodes to CUT at
  150 ms, so old patches play as before.
- **GLIDE applies where a Part's note takes a sounding voice of its own
  Part and model in place**: an in-place note-on on an active engine at
  another pitch (`EngineSlot::note_on`). That is a steal of the Part's
  own voice, and a Mono Part's retrigger, which so becomes a legato
  portamento. Another Part's steal, another model, a fading or idle
  voice, and a re-struck key (ADR 0062) are as before.
- **The glide is a one-pole in log pitch** (`dsp::Glide`, ADR 0062's
  halo glide, moved to `dsp` and generalised to any positive value),
  stepped once a block: a ratio of the pitch the voice sounds over the new
  note's, from where it is (a steal mid-glide starts from the pitch
  heard) to 1. Its time constant is GLIDE TIME / 3, so GLIDE TIME is 95 %
  of the way; it lands within 0.12 cent and then is exactly 1.
- **Modal:** the ratio multiplies the voice's PITCH ratio, so every model
  follows it as it follows a PITCH route: STRING's and SYMP's main line
  and dispersion, SYMP's halo (each string from where it is, ADR 0062),
  BANK's modes, recomputed each block anyway, and the bow's loop, whose
  lock correction (`grip`, ADR 0064) is re-taken as it moves. The new
  strike adds to what rings, as a re-strike does: STRING and SYMP add a
  pluck to the line, the target's length, the bow is set back on its
  string, BANK adds a burst; nothing is cleared, the macros ease on.
- **At the glide's start every ring is sized for the glide's longest
  period** (`KsString::fit`): a string's and SYMP's main string for the
  longer of the two, a bow's half loop for half of it, and each halo
  string for the longest its fold may give it under any chord on the way
  (`halo_reach`: its folded period at the longer main period, or `FITS`,
  the longest line, where the glide crosses a fold). A ring grown a block
  at a time by two or more samples reads its new gap before it has
  written it (measured: a C5 to C2 STRING glide dipped 6.8 dB; sized
  first, 2.0 dB, the fresh pluck's own decay).
- **A glide or a re-strike is billed against the block's clear budget
  (spec § 4.8) for what it writes**: a re-strike nothing, a glide its
  rings' growth (`ModalEngine::strike_clear`), not a fresh note-on's
  clear. Two SYMP glide steals of low notes fit one block.
- **A grown ring's gap holds the loop's continuation**, each sample the
  one a pass younger (`KsString::grow`, the pass as last set, `cycle`),
  negated on the bow's half loop, which inverts each pass
  (`set_period_inverting`): unnegated, every Bowed glide steal's output
  moved, a C5 to C2 at 1 ms dipping 2.2 dB a block against 1.2. A loop
  that lengthens faster than it writes, a fast glide down, reads the old
  cycle again rather than silence: short GLIDE TIMEs stay snappy, with no
  slew limit. Zero-filled, a C5 to C2 glide at 1 or 10 ms carried one
  burst and some 640 samples of silence round the loop, a C2 pulse train
  dipping 30 to 43 dB. On a cleared line the continuation is silence, so
  a note-on is unchanged.
- **A glide's pluck spans the target's line**: the strike is the new
  note's. The old line's length, a C5's on a C2 glide, plucked a quarter
  of the line, 8 dB under a CUT's strike over the first C2 period. Over
  each C2 period's window of the first 40 blocks, a fast glide down is now
  no more than 3 dB under the same steal at CUT, and louder only by what
  still rings (SYMP's halo, a bow's ringing loop).
- **ALGO:** the ratio multiplies every operator's frequency, the classic
  portamento; the envelopes strike as at CUT.
- **Bill:** at STEAL GLIDE a Modal voice may retune every block, as with a
  PITCH route: billed `ModalEngine::PITCH` (30), once beside such a route.
  ALGO's frequency is set each block anyway: nothing more.

## Alternatives considered
- **Glide by a fixed time, linear in period** (the halo's old glide): the
  owner heard it snap (ADR 0062). A one-pole in log pitch is the analogue
  portamento, and equal intervals take equal times.
- **GLIDE TIME as the time constant:** after 3 × TIME a fifth is still
  35 cents off. As 95 % of the way it reads as the time the glide takes.
- **A separate glide per model** (the line's period, the bank's
  frequencies): the voice's PITCH ratio already reaches every model and
  its retune paths, and is billed; one ratio keeps them in step.
- **A steal of another Part gliding too:** its Sound, envelopes and bus
  are not the new note's; it would glide one Sound's ring into another's.
- **Growing rings as the glide needs:** reads silence (above).

## Consequences
- A fast line on few voices plays as a guitarist's slides; at CUT nothing
  changes.
- GLIDE TIME's floor is 1 ms, not 0: a glide there lands in three blocks,
  and a fast glide down replays the old cycle while its loop lengthens.
- A SYMP glide that crosses a fold grows that halo string's ring to the
  whole line; its next fresh note-on clears it whole.
- A SYMP steal past the block's clear budget still waits, as any SYMP
  note does (spec § 4.8): the voice fades and its note starts clean, a
  CUT.
- A Mono Part at GLIDE glides every retrigger.
- STEAL does not touch a note taking a free voice: with voices to spare,
  nothing glides.

## Sources
- Issue #254; owner's UAT, 2026-09-30.
- ADR 0062 (re-strike, halo glide), 0064 (the bow's `grip`), 0042 (voice
  pitch), 0051 (a voice holds one engine), 0027 (fades).
- `chimera-core/src/dsp/glide.rs`, `dsp/engines.rs` (`EngineSlot::note_on`),
  `dsp/modal/mod.rs` (`ModalEngine::glide_on`, `fit`), `dsp/algo/engine.rs`
  (`AlgoEngine::glide_on`), `params.rs` (`Steal`, `PitchParams`).
- Tests: `steal_glide_test.rs`, `modal::tests::a_glide_grows_no_ring_on_its_way`,
  `cost_test::a_glide_steal_bills_the_retune`, `pitch_page_test.rs`,
  `sound_codec_test.rs`, `codec_compat_test::v1_fixtures_steal_cut`.
