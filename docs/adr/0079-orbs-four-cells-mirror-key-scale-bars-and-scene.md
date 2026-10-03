# 0079. ORB's four cells: KEY, SCALE, BARS and SCENE (amended: ORB the frame, CHRD the progression)

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

## Amendment (2026-10-03, owner): split by meaning
The mirror is replaced. ORB is the song's frame; CHRD is the progression.
- **ORB:** A RING, B KEY, C SCALE, D BARS, E SCENE, F BPM. KEY, SCALE and
  BARS are edited here alone; E still mirrors SCN's SCENE.
- **CHRD:** A SEGS, B DEGREE, C PLAY on the top row, so the focus starts
  on a real cell; D to F empty (`--`), reserved for chord controls such
  as inversion or voicing. Three cells are free, not two.
- **CUSTOM** is ORB's sub-page (`ORB_NODE`, sub 1). EDIT on ORB goes
  down to it whenever SCALE is CUSTOM, whichever cell was turned last;
  with SCALE named, EDIT on ORB does nothing for now. EDIT on CUSTOM
  returns to ORB, even with SCALE named under it. SCALE turned to CUSTOM
  still opens it. CHRD's EDIT does nothing.
- **Why this EDIT:** #316 makes EDIT "down into sub-pages, wrapping from
  the bottom to the top". CUSTOM as ORB's one sub-page is exactly that:
  EDIT goes down into it and wraps back up, and SEQ-as-up lands on ORB
  too. No hidden state gates it: a gate on the last-turned cell made the
  same EDIT on the same screen do two things, which surprised.
- No disk codes or `ParamId`s change; `UiState` is unchanged. `orbit_orb`, `orbit_orb_midi`,
  `orbit_chrd`, `orbit_chrd_played` and `orbit_custom` are re-recorded.
