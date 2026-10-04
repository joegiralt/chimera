# 0091. Reserve a note-on burst and ORBIT's own block; cost the Parts once a block; bill a stolen voice until its fade ends

- **Status:** Proposed (2026-10-04)
- **Deciders:** owner (A, B and the rebill, 2026-10-04), firmware
- Supersedes in part [0031](0031-six-voices-on-rev-v.md) (six voices for
  every patch on rev V) and [0040](0040-eight-voices-modal-strings-to-g1.md)
  ("admission is unchanged" and its voice counts); amends
  [0027](0027-shedding-fades-tails-first.md) (a steal from another Part,
  and what a shed leaves room for).

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
- **`Instrument::BURST` = 836 cycles/sample is reserved always**: five
  note-ons in one block, the most ORBIT sends (one step per ring per
  block, five rings) and a hand's chord, at their measured worst, 53,468
  (ORBIT's first block, every voice cold; the worst of four runs) / 64.
  A bigger burst, a USB chord of eight, runs into the 30 % headroom, about
  6–9k a note.
- **`OrbitRunner::COST` = 171 cycles/sample is reserved while ORBIT
  plays**: its own worst block without the notes it hands on (`BURST`
  bills those, as anyone's), 10,913 / 64. ORBIT plays exactly while it
  hands the `Instrument` a harmony, and it hands it before its notes.
- **`Instrument::RESERVED` (bus + burst) is what a patch edit sheds down
  to;** ORBIT's reserve refuses new voices and never sheds sounding ones.
- **A stolen voice bills the costlier of its two Sounds until its note
  starts** (`VoiceSlot::cost()` = `max(cost, fading)`, `fade_from`). A
  shed voice's fade stays unbilled against a note-on, as ADR 0027 has it.

Voices admitted on rev V, no tape (with the master tape):

| Sound | before | ORBIT stopped | ORBIT playing |
|---|---|---|---|
| TX Sounds (703–712), ALGO INIT (711) | 8 (7) | 7 (6) | 6 (6) |
| SAW LEAD, SQR BASS (572–575) | 8 (8) | 8 (8) | 8 (7) |
| MORPH PAD (853), MORPH KEYS (864) | 6 (6) | 5 (5) | 5 (5) |
| A16 ∪ A17 (909) | 6 (6) | 5 (5) | 5 (4) |

## Alternatives considered
- **Bill ORBIT's whole worst block (97k, 1,519/sample), no burst** (this
  ADR's first draft). Four A16 ∪ A17 voices with ORBIT playing, and a
  live chord stays unbilled.
- **A burst of two**, DIN's line rate (a note-on is 0.64–0.96 ms of a
  1.33 ms block). Keeps six A16 ∪ A17 voices with ORBIT stopped, but
  ORBIT's five-note blocks and a USB chord spill into the headroom. Any
  burst over 188/sample costs the TX Sounds their eighth voice.
- **Bill ORBIT's mean (141/sample).** Its burst lands on a full budget.
- **Shed to make room when ORBIT starts.** Fades held notes out under the
  player's hands; admission refuses, it doesn't cut.

## Consequences
- Every Sound heavier than about 620 loses a voice on rev V, ORBIT or
  not: the spec's "six on rev V" is five for MORPH PAD, MORPH KEYS and
  A16 ∪ A17. A cheaper voice start, or a burst re-measured lower, gives
  them back; `cost_test` pins the counts.
- Keys held before ORBIT starts can sound past its reserve until let go.
- On the unit (bench, rev V at 480 MHz), ORBIT's worst block 91,648 →
  64,188 (first block, cold) and 81,577 → 47,883 after it; recost in a
  five-note block 28,434 → 7,067; voice starts 36,158 → 30,318. STRESS
  (keys first / a block after ORBIT): 534,142 / 532,820, seven voices,
  before #355; 393,780 / 394,058, five voices, now.

## Sources
- `chimera-core/tests/cost_test.rs` (`a_stolen_voice_is_billed_until_its_fade_ends`,
  `orbit_and_a_burst_fit_beside_what_renders`, `a_refused_note_on_does_not_recost`,
  the factory table), `env_table_test.rs`.
- Bench (`just flash-bench`, `--features bench,orbit`), ORBIT PHASES and
  STRESS, rev V at 480 MHz, 2026-10-04.
- ADR 0027, 0031, 0040, 0071.
