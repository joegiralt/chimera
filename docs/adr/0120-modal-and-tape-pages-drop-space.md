# 0120. MODAL and TAPE pages drop SPACE; the reverb send lives on SENDS only

- **Status:** Accepted (2026-10-11)
- **Deciders:** owner
- **Supersedes in part:** [0093](0093-tape-plays-12-bit-card-sources-through-six-ram-models.md) (SPACE on DECK); the MODAL resonators spec ("SPACE addresses the Part's existing reverb send", `2026-09-29-modal-2-resonators-design.md`) and the TAPE core spec (home page "then SPACE")
- **Amends in place:** [0056](0056-modal-resonators-share-four-macros.md) (Proposed)

## Context
MODAL's RES page and TAPE's DECK page each carried a SPACE cell. It was only a
second label on the Part's reverb send (`PartParams::SEND_REVERB`), the value
SENDS › REV edits. The owner heard it as a separate mini reverb inside the
engine, and it was confusing.

## Decision
RES and DECK have no SPACE cell. The slot is the page's empty cell (`EMPTY`,
shown `--`); no other cell moves. The reverb send is edited on the mixer's
SENDS page (REV) only. Tests pin that neither page addresses `SEND_REVERB`
and that SENDS › REV does.

## Alternatives considered
- Rename SPACE to REV: still a duplicate of SENDS, still reads as engine-local.
- Fill the slot with a new parameter: speculative; the owner asked only to remove it.

## Consequences
One place per control. A sound's reverb send is no longer reachable from the
engine's home page. The empty cell is free for a later parameter.

## Sources
(private notes); owner decision 2026-10-11.
