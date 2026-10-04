# 0084. EDIT on ORB always opens CUSTOM; each ring shows its Part (amends 0079)

- **Status:** Proposed
- **Deciders:** owner (on-unit QA of ORB, 2026-10-04), firmware

## Context
Two findings from the owner's on-unit QA of ORB (`docs/screens/orbit_orb.png`):

- **EDIT was a dead key.** ADR 0079's amendment let EDIT on ORB go down to
  CUSTOM, ORB's one sub-page, only while SCALE was CUSTOM; with a named
  scale it did nothing. The owner tried to reach CUSTOM and couldn't.
  ADR 0081's D-pad makes EDIT "down", and down has to go somewhere.
- **Ring n isn't Part n.** Each ring plays its TARGET Part (RING › NOTES,
  `SceneRing::TARGET`), and NEW puts rings 3 and 4 both on Part 3. ORB
  showed no Part, so the owner read ring n as Part n.

## Decision
- **EDIT on ORB always opens CUSTOM.** `OrbitCx` loses its `custom` flag;
  `OrbitAt::step` sends EDIT on ORB to `CUSTOM_PAGE` unconditionally. EDIT
  or SEQ on CUSTOM returns to ORB, as before. SCALE turned to CUSTOM still
  opens it.
- **With a named scale the circle shows that scale's notes**:
  `ChordRing::set()`, the notes SCALE plays (`scale.scale(custom)`), is
  what the circle draws and what IN SCALE reads. SCALE stays named while
  only the cursor moves; leaving without a toggle changes nothing.
- **The first toggle makes it CUSTOM.** `ChordRing::toggle(i)` flips `i`
  in `set()` and sets SCALE to CUSTOM, so a named scale's other eleven
  notes are kept, the toggle applied. IN SCALE's turn calls it through
  `OrbitSong::edit`, so `live` and its slot change together (ADR 0083's
  invariant: `live` stays private). A CUSTOM set the user made earlier is
  replaced by the named scale's notes at that toggle: the circle showed
  the named scale, and the toggle edits what was shown.
- **Each ring's Part on ORB.** The Part number, small (`FONT_LABEL`), sits
  by the ring's first step at 12 o'clock: rings 1 and 3 left of the column
  of first-step dots, rings 2 and 4 right, so each side has a label every
  other ring (18 px) rather than one every 9 px; ring 5's sits inside it,
  under its top, clear of its own last steps. A muted ring's label dims
  with its dots (`PILL_EDGE`). Just the digit: the RING cell spells it out.
- **The RING cell reads "3 › P3"**: ring 3 on Part 3. The chevron is the
  UI's `draw::CHEVRON` mark (the u8g2 faces have no `›`); a cell's value
  is now drawn with `text_tracked(.., 0)`, the same glyphs plus the marks,
  pixel-identical for every other cell (every other golden unchanged).
- **Redraws follow TARGET.** ORB's viz key hashes each ring's Part; the
  RING cell's value carries the Part a sixteenth of a ring past its ring,
  so the cells region redraws on a TARGET change too.

## Alternatives considered
- **EDIT on a named scale turns SCALE to CUSTOM at once.** Not taken: a
  look at the circle would change what plays and what is saved.
- **Part labels all on one side of the column.** Tried: at 9 px a ring the
  8 px digits touch, and ring 5's sat on its own last steps.
- **"P3" on the circle.** Tried: wider, so the column of five crowded the
  inner rings' first steps. The RING cell carries the "P".
- **Labels outside the band.** The header line leaves no room at the top.

## Consequences
- The D-pad holds on ORB: EDIT always moves.
- `orbit_orb` and `orbit_orb_midi` are re-recorded; `orbit_custom_named`
  (CUSTOM by EDIT on MINOR, nothing toggled) is new. The screen atlas
  reaches CUSTOM by EDIT alone.
- A label can still graze a dot at some step counts (ring 4 from about 12
  steps on, 16 included, touches its second step's edge by a pixel); the
  digit stays legible.
- No disk codes, `ParamId`s or `UiState` bytes change.

## Sources
- ADR 0079 (amendment 2026-10-03), ADR 0081, ADR 0083.
- `chimera-core/src/ui/nav.rs`, `orbit/chords.rs`, `ui/orbit/{cells,draw}.rs`.
- Tests: `orbit_pages_test` (`edit_opens_custom_on_a_named_scale`,
  `ring_cell_names_its_part`, `a_target_change_redraws_orb`),
  `orbit_nav_test`.
