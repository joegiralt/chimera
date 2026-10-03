# 0079. ORB's four cells mirror KEY, SCALE, BARS and SCENE

- **Status:** Proposed
- **Deciders:** owner (left the choice to the implementer), for the owner's UAT

## Context
ORB has six encoder cells. A is RING and F is BPM; B to E were empty. The
owner left what fills them to us.

## Decision
- **B KEY, C SCALE, D BARS, E SCENE.** They are the CHRD page's `KEY`,
  `SCALE` and `BARS` params and SCN's `OrbitCell::Scene`, bound by the same
  `ParamSlot`s, not copies. A turn on ORB is the same turn on the home page:
  SCALE seeds CUSTOM from the scale it was; SCENE queues a switch on the
  loop, and turning back to the playing scene cancels it (ADR 0077).
- No new disk codes, `ParamId`s or `UiState` bytes.
- Why these: the key, scale, length and scene are what a player changes
  while the rings run, and ORB is where they are watching.

## Consequences
- ORB's two goldens (`orbit_orb`, `orbit_orb_midi`) are re-recorded.
- SCALE turned to CUSTOM on ORB opens the CUSTOM circle, as on CHRD.
- The owner may revise the choice at UAT; it is one `params` array in
  `ui/orbit/pages.rs`.
