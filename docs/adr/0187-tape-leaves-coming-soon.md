# 0187. TAPE leaves COMING SOON: an INIT row, a pool Sound and a LIBRARY row like any engine

- **Status:** Proposed
- **Deciders:** owner (decision), Claude (implementation)

## Context
ADR 0132 listed TAPE and DRUM as COMING SOON, never playable. TAPE's DSP now
ships in bank 2 (ADR 0137) and the owner wants to pick TAPE Sounds from the
browser. DRUM has no engine yet.

## Decision
1. **Browser.** `EngineType::PLAYABLE` gains TAPE after VA, so the INIT rows
   are ALGO, MODAL, VA, TAPE, with the engine tag and size like any. Under
   them one `browser::Soon` row remains: DRUM.
2. **Pool.** NEW's factory set holds an INIT TAPE (`factory.bin`
   re-recorded). A TAPE Sound in the pool reads its engine tag and cost, and
   EDIT loads it.
3. **Library.** LIBRARY and FACTORY list TAPE Sounds like any other.
4. **Bank 2 closed.** Unchanged (ADR 0141): the Part loads as TAPE, plays
   silent and reads NEEDS UPDATE.

This supersedes ADR 0132 points 1 to 3 for TAPE; DRUM keeps them.

## Alternatives considered
- **Keep `Soon::of` and `LibState::tape` for a future engine.** Dead code
  until one exists; DRUM has no `EngineType`.

## Consequences
- NEW holds one more INIT Sound; the pool starts one slot fuller.
- `Soon::of`, `SOON_LOAD`, `LibState::tape` and `list_tape_for_test` go.

## Sources
ADR 0132, ADR 0137, ADR 0141, #615 (0.2.0 epic).
