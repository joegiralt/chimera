# 0291. A refusal is its Part's: said only in a diff of that Part

- **Status:** Accepted (2026-11-19)
- **Deciders:** firmware, the editor (editor-ui re-review, I-A)

## Context
ADR 0290's refusals carried no Part. `ed since` wrote them into the next answer whatever it was, a whole answer for another Part too, and the editor re-inserted them after that answer's `part` line had cleared its refusals: Part 2's PAD3 CHOKE showed Part 1's reason. A `since 0` answer carried reasons from before the editor's fresh start.

## Decision
- **Each refusal keeps its Part** (`edits::Refused { part, .. }`, the Part its set was queued for).
- **`ed since` writes only the followed Part's, and only in a diff.** A whole answer (another Part, another engine, `since 0`, a stale number) writes none. A committed answer clears them all, written or not. A `Measure` pass or an `ERR` answer still keeps them (ADR 0290).
- **The editor applies the same rules.** A `refused` line after a `part` line in the same answer is ignored, as is one whose address the current schema lacks. An engine change clears them with the values.
- Supersedes in part ADR 0290 ("an answer that writes them clears them").

## Alternatives considered
- **Write the Part on the line (`refused 1 PAD3.CHOKE …`).** A new line shape for protocol 3's editors, for reasons the editor can't show off the followed Part anyway.
- **Keep other Parts' refusals until that Part is followed again.** The toast was said long ago by then. A stale reason on a control is worse than none.

## Consequences
- **Bank 1 and bank 2:** measured with the 0.4 merge, in the budget notes under ADR 0290.
- `EdState`'s refusals grow by a `PartId` each.
- A refusal that lands just before the editor changes Part is never shown. The box's toast still says it.

## Sources
- `chimera-core/src/console/ed/{edits,since}.rs`, `editor/src/Model.elm`
- Tests: `console_ed_engines_test`'s `a_refusal_stays_with_its_part`; `ModelTest`'s `a_whole_answer_says_no_refusal_part_2_never_shows_part_1s`, `since_0_is_whole_its_refusals_stale`, `a_refusal_of_an_address_not_in_the_schema_is_ignored`
