# 0132. TAPE is coming soon for alpha: unreachable, its DSP out of the firmware

- **Status:** Proposed
- **Deciders:** owner (decision), Claude (implementation)

## Context
TAPE (ADR 0093) is being reworked (private issue #476)
and won't be ready for the alpha. The owner wants it shown like DRUM: listed,
saying COMING SOON, never playable. Saved data must still load safely
(no users yet: old patches must load, not sound the same). Flash is tight:
about 26 KB free on the ORBIT build, and TAPE's DSP is about 36 KB of it.

## Decision
1. **Browser.** The INIT rows are `EngineType::PLAYABLE` (ALGO, MODAL, VA).
   Under them, one row per `browser::Soon` (TAPE, then DRUM): `Entry::Soon`
   replaces the DRUM special case. A soon row reads COMING SOON in place of
   the engine tag, and EDIT only toasts (`TAPE: COMING SOON`). Clear and save
   act on nothing there.
2. **Pool.** NEW's factory set holds an INIT per playable engine: no INIT
   TAPE (`factory.bin` re-recorded; NEW now has 11 Sounds). A TAPE Sound
   already in a project's pool stays (its slot number and data kept for
   TAPE's return), reads COMING SOON with no cost, and EDIT only toasts.
3. **Library.** LIBRARY drops TAPE Sounds from each listed page; the files
   stay on the card.
4. **Old projects.** On load, a TAPE Part becomes a clean INIT VA
   (`Origin::Init(Va)`), and the load is lossy, so a save drops the TAPE
   Sound. No Part plays TAPE, so no TAPE page or chain is reachable.
5. **Flash.** The `tape` cargo feature (chimera-engine, re-exported by
   chimera-core) holds TAPE's engine. Off in every firmware build but
   `bench-tape`; on in chimera-core's defaults (desktop, tools) and in both
   crates' tests. Off, `tape_engine::off` is a silent `TapeEngine` with the
   same surface that refuses every note. The params, sources, codecs, UI and
   console `put` stay either way.

## Alternatives considered
- **A silent TAPE Part that can't be edited.** Keeps the data, but its
  home is the TAPE chain, so every way onto a Part's pages needs a guard.
  INIT VA needs one change at load.
- **Hiding TAPE pool rows.** Rows are slot numbers; hiding one renumbers the
  list and the cursor maths. A COMING SOON row is the same path as the
  INIT rows.
- **Cutting the TAPE UI too** (pages, strip, SOURCE cell: about 7 KB). It
  would scatter `cfg` through the page registry, view and renderer.
- **Removing TAPE from `EngineType`.** Its disk codes and params must stay
  so old files decode.

## Consequences
- The ORBIT firmware frees about 45 KB of flash (about 26 KB free before, 70 KB after).
- The desktop keeps TAPE's DSP but plays by the same rules, so its QA
  matches the unit.
- TAPE's tests keep running: they load TAPE through test-only hooks
  (`UiState::load_for_test`, `list_tape_for_test`) or a pool slot.
- #476 undoes points 1 to 4 and turns `tape` on in the firmware (with a
  superseding ADR).

## Sources
ADR 0093 (TAPE), ADR 0087 (pool), ADR 0051 (one engine a slot), #476.
