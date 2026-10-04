# 0091. Reserve ORBIT's worst block while it plays; bill a stolen voice until its fade ends

- **Status:** Proposed (2026-10-04)
- **Deciders:** firmware; owner to accept
- Supersedes in part [0040](0040-eight-voices-modal-strings-to-g1.md)
  ("admission is unchanged" and its voice counts, which hold with ORBIT
  stopped); amends [0027](0027-shedding-fades-tails-first.md) (a steal from
  another Part).

## Context
private issue #355: with ORBIT
playing, the bench's STRESS run (six A16 ∪ A17 keys on Part 6, every
effect at its worst, NEW's groove on Parts 1–5) peaked at 536,475 cycles
a block against the 448,000 budget (70 % of 640,000). Its worst block
held seven voices; ORBIT's runner took about 78k and the render 458k.

Reproduced on the host (`cost_test.rs`), two causes:

- **ORBIT is billed nowhere.** `OrbitRunner::block` runs in the audio
  callback before the render (ADR 0071): its clock, MUTATE and the
  `Instrument`'s note-on work for every note it sends. Admission reserves
  only `FxBus::COST`. The bench's ORBIT row: 8,981 a block typical,
  97,153 worst (88,466 at cf50ee74), the worst when all five rings fire
  together.
- **A cross-Part steal is billed at the newcomer at once.** The seven
  voices were billed correctly: ORBIT's notes had stolen four of the six
  keys (rule 3 takes the oldest held voice), so the seven were mixed NEW
  Sounds, 5,784 + 1,180 ≤ 7,000. No path admitted past the bill. But a
  voice taken for another Part's note fades out on its old Sound for
  `Voice::FADE` (two blocks) before the note starts (ADR 0027), and its
  slot was booked at the new note's cost from the start: four keys at 909
  taken for notes at 704–853 "freed" 523 that was still rendering, and a
  note could be admitted into it (host: 7,121 rendered of 7,000).

## Decision
- **`OrbitRunner::COST` = 1,519 cycles/sample** (97,153 / 64, rounded
  up): ORBIT's worst block, reserved beside `FxBus::COST` while ORBIT
  plays. A worst block, not the mean, because the budget is a per-block
  deadline and ORBIT's cost is bursty (its worst about 11× its mean), where a voice's is
  steady. ORBIT plays exactly while it hands the `Instrument` a harmony,
  so `Instrument::reserved()` is `FxBus::COST`, plus `OrbitRunner::COST`
  while `harmony` is `Some`. The runner hands the harmony before it sends
  the block's notes, so its first block's notes are judged with it.
- **The reserve refuses new voices; it never sheds sounding ones.** `shed`
  still reserves only the bus. Keys held before ORBIT starts keep sounding;
  ORBIT's notes (and any new key) wait for room.
- **A stolen voice bills the costlier of its two Sounds until its note
  starts.** `VoiceSlot::cost()` is `max(cost, fading)`; the `Instrument`
  sets `fading` (`Allocator::fade_from`) when it takes a voice another
  Part still sounds, and clears it when the waiting note starts. A shed
  (dying) voice's fade stays unbilled against a note-on, as ADR 0027 has
  it.

Voices on rev V, no tape (with the master tape):

| Sound | ORBIT stopped | ORBIT playing |
|---|---|---|
| TX Sounds (703–712) | 8 (7) | 6 (5) |
| SAW LEAD, SQR BASS (572–575) | 8 (8) | 7 (6) |
| MORPH PAD (853) | 6 (6) | 5 (4) |
| MORPH KEYS (864) | 6 (6) | 4 (4) |
| A16 ∪ A17 (909) | 6 (6) | 4 (4) |

## Alternatives considered
- **Bill ORBIT's mean (141/sample).** Keeps a voice, but the STRESS block
  stays over: its 78k burst lands on top of a full budget.
- **Bill ORBIT per note it sends.** Most of the worst block is the
  `Instrument`'s note-on work, which a live chord costs too; billing
  sequencing and note-ons apart is finer, but needs a per-event cost
  model the bench doesn't yet measure. The reserve can shrink to it later.
- **Shed to make room when ORBIT starts.** Fades held notes out under the
  player's hands; admission refuses, it doesn't cut.

## Consequences
- ORBIT playing costs one or two voices of every Sound; the factory
  Sounds keep at least four on rev V (spec § Intent's "six on rev V"
  holds with ORBIT stopped).
- Keys held before ORBIT starts can exceed the bill until released; on the
  unit, STRESS with the keys first reads 455,970 (ORBIT's notes all
  refused, its runner 40,926 of it), 1.8 % over the 70 % line and inside
  the 30 % reserve. With the keys a block after ORBIT starts, 422,976,
  five voices. Before: 534,142 and 532,820, seven voices.
- A worst block re-measured lower (or a cheaper note-on path) lowers
  `OrbitRunner::COST` and gives voices back; `cost_test` pins the counts.
- `.text` of the fat-LTO `--features orbit` build: 200 B smaller.

## Sources
- `chimera-core/tests/cost_test.rs`: `a_stolen_voice_is_billed_until_its_fade_ends`,
  `orbit_is_billed_while_it_plays`, `orbit_takes_no_voice_from_keys_already_down`,
  the factory table.
- Bench (`just flash-bench`, `--features bench,orbit`), rev V at 480 MHz,
  2026-10-04: ORBIT row, STRESS and STRESS ORBIT lines.
- ADR 0027, 0040, 0071.
