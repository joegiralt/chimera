# 0130. Operator pages are OSC sub-pages; page focus is kept by identity

- **Status:** Proposed
- **Deciders:** owner (2026-09-30), firmware

## Context
The formant operators need a page per operator: WAVE, COARSE, FINE, LEVEL, FDBK and VELOCITY side by side, beside the operator's wave and envelope. #37 asked for a hold gesture to reach it. The Algo chain's OSC node already has twelve group pages (CRS, FIN, DET, LVL, VEL, AR, D1R, D1L, D2R, RR, RS, FBK), each one parameter across the six operators. A page's place in a sub-row moves when the row changes, so a remembered `(node, sub)` can point at the wrong page.

## Decision
- OP1–OP6 are sub-pages of OSC, after the twelve group pages (sub 13–18). EDIT and SEQ step through them like any sub-page, and EDIT wraps from OP6 to OSC (ADR 0081). There is no new gesture: B*n* stays Part *n*.
- The twelve group pages are unchanged.
- Each OP page binds its six cells to its own operator (`ALGO_OP_PAGES`, ids 100–105; labels per the full-word rule, FDBK for FEEDBACK). The band draws the envelope with `viz::op_env` and the wave as an inset with `glyph::wave_trace`. Every colour is a theme token (ADR 0126).
- Page focus is kept by `BlockDef.id`, not by position: `ChainDef2::locate`, `refocus` and `Location::rechained` re-find the page when the sub-row or the Part's chain changes.
- #37's hold gesture is not part of this change: it stays in #37, with the playhead dot, gang edit, LEVEL SCALE and living diagram.

## Alternatives considered
- A hold or chord gesture into an operator page (#37): a new hidden gesture, and B*n* already means Part *n*.
- Merging the envelope group pages into OP pages: the owner may revisit; not done now.
- Keeping focus by `(node, sub)`: wrong once the sub-row is dynamic.

## Consequences
- OSC's sub-row is 18 long, and a lap by EDIT is 19 pages.
- A page can come and go (a kind change, a project load) without losing the user's place.
- The OP pages' cells later become `SlotBinding::OpPanel(op, k)` for the formant kinds.

## Sources
- ADR 0081 (D-pad), ADR 0126 (theme), ADR 0070 (WAVE glyph).
- The commit "Page focus is kept by page id; the Part's chain is picked per Sound" (focus by id); issue #37.
