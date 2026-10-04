# 0091. While ORBIT plays, reserve its own block and a note-on burst; cost the Parts once a block; bill a stolen voice until its fade ends

- **Status:** Proposed (2026-10-04)
- **Deciders:** owner (A, B and the rebill, 2026-10-04; the burst only
  while ORBIT plays, 2026-10-05), firmware
- Supersedes in part [0040](0040-eight-voices-modal-strings-to-g1.md)
  ("admission is unchanged" and its voice counts, while ORBIT plays);
  amends [0027](0027-shedding-fades-tails-first.md) (a steal from another
  Part).

## Context
private issue #355: with ORBIT
playing, the bench's STRESS run (six A16 ∪ A17 keys on Part 6, every
effect at its worst, NEW's groove on Parts 1–5) peaked at 536,475 cycles
a block against the 448,000 budget (70 % of 640,000), seven voices
sounding.

Reproduced on the host (`cost_test.rs`), two causes:

- **Nothing billed ORBIT's block.** `OrbitRunner::block` runs in the audio
  callback before the render (ADR 0071). Admission reserved only
  `FxBus::COST`.
- **A cross-Part steal was billed at the newcomer at once.** The seven
  voices fitted their bill (ORBIT had stolen four keys for cheaper NEW
  Sounds: 5,784 + 1,180 ≤ 7,000); no path admitted past it. But a voice
  taken for another Part's note fades out on its old Sound for
  `Voice::FADE` (two blocks) before the note starts (ADR 0027), and was
  booked at the new note's cost from the start, so the room it "freed"
  could take another note while it still rendered (host: 7,121 of 7,000).

The bench's ORBIT PHASES page (06dd8707) then split ORBIT's worst block,
about 92k: some 81k was its five note-ons, 16–19k each, and only about
10k ORBIT's own work (clock, rings, MUTATE, harmony, re-snap, view). Of a
note-on, `recost` costed all six Parts (6 × `Voice::cost`, about 5k) on
every note-on, refused or not, and an ALGO voice start worked out six
operators' envelope coefficients with `exp2` and divisions.

## Decision
- **The Parts are costed once a block.** `recost` keeps each Part's
  `voice_cost` and the address of the snapshot it read; a later note-on,
  or the render, on the same snapshot reuses them, and the render ends
  the block. A burst of note-ons, admitted or refused, costs the Parts
  once (`a_refused_note_on_does_not_recost`). (Caching at the UI's publish
  was the alternative: every in-place write to a snapshot, the bench's and
  the tests', would have had to refresh it.)
- **Envelope coefficients come from tables.** `attack_add` and
  `decay_log2` depend only on the effective rate (0–63) and the sample
  rate, so they are two 64-entry tables computed at compile time by the
  same `const fn`s for `SAMPLE_RATE`: bit for bit (`env_table_test`), 512
  B of flash, nothing to invalidate. Any other rate computes as before.
  This serves every Sound and note, where a per-Part cache keyed by Sound
  would miss across key-scaling codes (up to 32 per operator).
- **`Instrument::BURST` = 857 cycles/sample is reserved while ORBIT
  plays**: five voice starts in one block, the most ORBIT sends (one step
  per ring per block, five rings), at their measured worst, 54,810
  (ORBIT's first block, every voice cold; the worst of five runs) / 64.
  It covers five starts a block and no more: layered Parts on one
  channel (a note starts a voice on each), or a live chord landing in the
  same block as ORBIT's five, start more, and those run into the 30 %
  headroom, about 6–9k a start. With ORBIT stopped nothing is reserved
  for bursts: a DIN chord arrives a note about every millisecond (a
  note-on is 0.64–0.96 ms at 31,250 baud), so it spreads across blocks of
  1.33 ms, and STRESS without ORBIT passed with no reserve.
- **`OrbitRunner::COST` = 171 cycles/sample is reserved while ORBIT
  plays**: its own worst block without the notes it hands on (`BURST`
  bills those, as anyone's), 10,913 / 64, the worst over five runs. The
  pair covers the costliest whole ORBIT block measured: 171 + 857 = 1,028
  a sample, 65,792 a block, against 65,387 (its own work 10,577 there).
  ORBIT plays exactly while it hands the `Instrument` a harmony, and it
  hands it before its notes.
- **A patch edit sheds down to the bus alone, as before;** ORBIT's
  reserve and the burst refuse new voices and never shed sounding ones.
- **A stolen voice bills the costlier of its two Sounds until its note
  starts** (`VoiceSlot::cost()` = `max(cost, fading)`, `fade_from`). A
  shed voice's fade stays unbilled against a note-on, as ADR 0027 has it.

Voices admitted on rev V, no tape (with the master tape):

| Sound | ORBIT stopped (as before) | ORBIT playing |
|---|---|---|
| TX Sounds (703–712) | 8 (7) | 6 (6) |
| SAW LEAD, SQR BASS (572–575) | 8 (8) | 8 (7) |
| MORPH PAD (853), MORPH KEYS (864) | 6 (6) | 5 (5) |
| A16 ∪ A17 (909) | 6 (6) | 5 (4) |

## Alternatives considered
- **Bill ORBIT's whole worst block (97k, 1,519/sample), no burst** (this
  ADR's first draft). Four A16 ∪ A17 voices with ORBIT playing, and a
  live chord stays unbilled.
- **The burst reserved always** (this ADR's second draft). Covers a USB
  chord with ORBIT stopped, but costs every Sound heavier than about 620
  a voice (TX 8 → 7, MORPH and A16 ∪ A17 6 → 5) for bursts DIN spreads
  over blocks anyway.
- **A burst of two**, DIN's line rate (a note-on is 0.64–0.96 ms of a
  1.33 ms block). Keeps six A16 ∪ A17 voices with ORBIT stopped, but
  ORBIT's five-note blocks and a USB chord spill into the headroom. Any
  burst over 124/sample costs the TX Sounds their eighth voice (TX BRASS,
  712: 5,820 − 8 × 712).
- **Bill ORBIT's mean (141/sample).** Its burst lands on a full budget.
- **Shed to make room when ORBIT starts.** Fades held notes out under the
  player's hands; admission refuses, it doesn't cut.

## Consequences
- With ORBIT stopped the voice counts are as ADR 0031 and 0040 have
  them. While it plays, most Sounds lose one or two: the spec's "six on
  rev V" is five for MORPH PAD, MORPH KEYS and A16 ∪ A17. A cheaper voice
  start, or a burst re-measured lower, gives them back; `cost_test` pins
  both.
- A USB chord of more than a few notes landing in one block with ORBIT
  stopped is unbilled; it runs into the headroom.
- Keys held before ORBIT starts sound past its reserve until let go: six
  A16 ∪ A17 keys with ORBIT then started bill 6 × 909 + 1,180 + 171 +
  857 = 7,662 against 7,000. It still fits because every bill is a
  rounded-up bound: a voice's cost is a bench mean rounded up, and
  `BURST` and `OrbitRunner::COST` are each a worst block, which a full
  pool doesn't meet, since ORBIT's notes are refused (its block in that
  run was 23,191 at worst, against 65,792 reserved). Measured, that case
  peaks at 433,362 a block, 6,771 a sample.
- On the unit (bench, rev V at 480 MHz, 2026-10-05), against 448,000 a
  block and the 640,000 deadline:

  | STRESS | keys first | keys a block after ORBIT starts |
  |---|---|---|
  | before #355 | 534,142, 7 voices | 532,820, 7 voices |
  | ORBIT playing | 433,362, 6 voices | 393,718, 5 voices |
  | the block ORBIT starts in (T0) | 426,196 | 345,842 |
  | ORBIT stopped (`bench`) | 419,609, 6 voices | 419,817, 6 voices |

  ORBIT's block: 6,056 mean, 65,387 worst (its first, cold). The phases
  before A and B: worst 91,648 (81,577 after the first block); after,
  65,387 (47,883); recost in a five-note block 28,434 → 7,067; voice
  starts 36,158 → 30,318.

## Sources
- `chimera-core/tests/cost_test.rs` (`a_stolen_voice_is_billed_until_its_fade_ends`,
  `orbit_and_a_burst_fit_beside_what_renders`, `a_refused_note_on_does_not_recost`,
  the factory table), `env_table_test.rs`.
- Bench (`just flash-bench`, `--features bench,orbit`), ORBIT PHASES and
  STRESS (T0 at 785a028a), rev V at 480 MHz, 2026-10-04 and 2026-10-05.
- ADR 0027, 0031, 0040, 0071.
