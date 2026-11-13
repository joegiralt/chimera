# 0268. PAD FOLLOW decides whether a live hit moves KIT's pad

- **Status:** Proposed
- **Deciders:** owner (2026-11-13), firmware

## Context
On a DRUM Part a live hit selected its pad (ADR 0265), so playing over MIDI while shaping a pad pulled the edit away to whatever was struck. The owner wants it to follow when he plays, stay put while he edits, and to turn it off altogether.

## Decision
- SETTINGS › PERSONALIZE › PAD FOLLOW, one cell FOLLOW (PAD FOLLOW runs into the next cell): `PadFollow { Off, Idle, Always }`, disk codes 0, 1, 2 frozen, ident `FOLLOW`, in the THEME block and stored in SYSTEM with it. A SYSTEM without it loads IDLE, the default.
- OFF: hits never move the selection; MIX + MINUS / PLUS and the PAD cell do. ALWAYS: every live hit on the shown Part selects its pad. IDLE: a hit selects it only once 2 s (`FollowHold::AFTER_EDIT`, stepped by the UI's frame, `animation::FRAME`) have passed since the last encoder turn or pad step (MIX + MINUS / PLUS) on a DRUM page of that Part. A turn anywhere else (its MIXER channel, ORBIT, SETTINGS, another Part's pages) holds nothing.
- The hold is per Part (`[FollowHold; MAX_PARTS]`, reached by `PartId`): an edit on Part 1 never holds Part 2's hits. It sits beside the kits in ITCM (`Kits`), the UI's, never saved.

## Alternatives considered
- **ON / OFF only:** the first ask; IDLE came from the owner's refinement, so editing is not interrupted and playing still follows.
- **A frame count for the hold:** the frame rate is the pacer's; the hold is milliseconds on the UI's clock.

## Consequences
- 12 B more ITCM (2 B a Part; `Kits` is 990 B, `memory_budget_test`); one more SETTINGS leaf.
- Hits still light their pads in every mode.

## Sources
- Owner, 2026-11-13. ADR 0045 (SYSTEM records), 0154 (TEXT SIZE's pattern), 0265.
