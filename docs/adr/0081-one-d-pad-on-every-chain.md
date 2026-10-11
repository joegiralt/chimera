# 0081. One D-pad on every chain: SEQ up, EDIT down with wrap, MINUS and PLUS along

- **Status:** Proposed
- **Deciders:** owner (2026-10-03, private issue #316); firmware

## Context
ADR 0044 and ADR 0066 gave the SEQ tap a different job on each chain. On
a Part's pages it went up a sub-page. On the mixer, the FX and the Sound
rung it jumped to SETTINGS › PART. In a SETTINGS list it ran the row
under the bar. In ORBIT it saved the scene. So the same key went up, went
sideways into another menu, ran something, or saved, depending on where
you were. EDIT also stopped dead on a chain's last sub-page, except on
ORBIT's RING, which cycled.

The owner ruled on 2026-10-03 that every chain moves the same way, as a
D-pad: the Part pages, the mixer, the FX, the sound browser, ORBIT and the
SETTINGS lists.

## Decision

| Key | Everywhere |
|---|---|
| **MINUS / PLUS** | Left and right along the chain: the node, the bar, the Part on the Sound rung. Unchanged. |
| **EDIT** | Down: into a sub-page, or into or running a list row. From a chain's last sub-page it wraps to the first (`nav::page_step`). |
| **SEQ tap** | Up one level. At the top of a chain it does nothing. It never jumps elsewhere and never saves. |
| **MENU tap** | Leave. Unchanged: SETTINGS opens and backs out, ORBIT returns to where it was entered. |
| **MIX + button** | Secondary and destructive actions. |
| **Holds** | Unchanged: SEQ hold enters ORBIT, MENU hold quick-saves, B1–B5 hold crosses to a ring's Part. |

Per place, as built in `ui::nav`:

- **Part pages and the FX:** EDIT goes down a sub-page and wraps from the
  last back to the first. SEQ goes up a sub-page and does nothing on the
  first.
- **The mixer (PART, SENDS):** EDIT goes to the Sound rung. SEQ does
  nothing, because the mixer is the top. It no longer jumps to
  SETTINGS › PART, which stays one EDIT from SETTINGS' top list
  (MENU, PART, EDIT).
- **The Sound rung:** SEQ goes up to the mixer it came from
  (`Recall::mix_entry`). EDIT loads the row. **MIX + EDIT saves the
  Part's Sound** over its slot, or to a new one when it has none
  (`Offer::first_save`). That is SAVE TO PROJ's own action, so it keeps
  its guards: a `PartAction` that went stale is refused (`ActionGone`,
  ADR 0043), and Parts it leaves `Stale` are asked about (`Ask::update_stale`).
  A full pool says `POOL FULL`.
- **SETTINGS:** SEQ does what a MENU tap does inside SETTINGS. It goes
  back a level, and from MANAGE's commands back to its list. At the top
  list it does nothing, where MENU closes SETTINGS. It no longer runs a
  row. EDIT runs a Screen's row (`Step::Run`: LOAD, SAVE TO, MANAGE's
  commands), as it already opened lists and ran actions.
- **ORBIT:** SEQ goes up a sub-page (CUSTOM to ORB, VOICE to RHYTHM) and
  never saves. `OrbitAct::SaveScene` and `OrbitSong::save_scene` are gone.
  Scenes are saved with SCN's SAVE (EDIT on SCN) and in SETTINGS › ORBIT,
  until the Cirklon-style edit-in-place model replaces them.

Prompts and NAMING are modal, not chains, so they keep SEQ to confirm and
MENU to cancel. The on-screen legends follow: the browser shows
`EDIT LOAD`, `MIX+EDIT SAVE` and `MIX- CLEAR`, and SETTINGS shows
`EDIT LOAD` and `EDIT RUN` in place of `SEQ LOAD` and `SEQ RUN`.

This supersedes the **SEQ tap** row and the "SEQ tap saves the scene" line
of ADR 0044, and the "SEQ runs actions and screen rows" ruling of ADR 0066.
The rest of both stands.

## Alternatives considered
- **Keep SEQ per place:** each meaning made sense alone, but a hand
  can't learn a key that means up, run, jump or save depending on the
  screen. That was the problem the owner ruled on.
- **SEQ at the top closes SETTINGS, as MENU does:** that would make SEQ a
  second leave key. The rule is that SEQ does nothing at the top of a
  chain, so leaving stays MENU's job.
- **MIX + EDIT saves to the slot under the browser's cursor:** that would
  be a second save path with its own overwrite prompt. Saving the Part's
  Sound as SAVE TO PROJ does reuses the guard the project model already
  proves.
- **Keep a SEQ-tap scene save in ORBIT:** it was the only tap that saved
  with no prompt. It goes, because SEQ is navigation everywhere now.

## Consequences
- One map to learn. The simulator's arrow keys already match it: Up is
  SEQ and Down is EDIT.
- A scene save in ORBIT now takes PLUS to SCN and then EDIT, rather than
  one tap anywhere.
- SETTINGS › PART is no longer one key away from the mixer. It is MENU,
  PLUS, EDIT.
- The browser's hints take two lines: `MIX- CLEAR` replaces the
  `A SCROLLS` hint on the info line.
- Tests: `dpad_nav_test` checks every place type against the four
  directions and the wrap, plus MIX + EDIT in the browser. The nav, ORBIT
  and SETTINGS tests that pinned the old SEQ were updated.

## Sources
- private issue #316, the owner's ruling
- ADR 0043 (part actions and their guards), ADR 0044 (the key map), ADR 0066 (SETTINGS), ADR 0077 (SCN's SAVE)
- `chimera-core/src/ui/nav.rs`, `chimera-core/src/ui/mod.rs` (`save_part`)
