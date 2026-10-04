# 0086. Every gesture shows a contextual hint in the focus band (amends 0074 and 0081; supersedes 0017's hint text)

- **Status:** Proposed
- **Deciders:** owner (on-unit QA of RING · NOTES, 2026-10-04), firmware

## Context
The PreenFM3 has few controls, so MIX + knob and hold gestures stand in for
the buttons it lacks. In on-unit QA the owner got stuck on RING · NOTES at
step 1 of 32: nothing on screen said that MIX + NOTE picks the step. The
chip row also squeezed 32 steps into the band, about 5 px a chip.

Hints existed, but each was made its own way:

- the browser's `HINTS` / `CLEAR_HINT`, spread across two lines;
- the mod matrix's always-on line, `PRIME MIX+PLUS  DELETE MIX+MINUS`
  (ADR 0017's text, later shortened);
- CLEAR's focus label, `MIX + UP / DOWN` (ADR 0074), which took the place
  of the cell's name.

The owner's rulings:

- A hint is contextual. It shows in the focus band, the strip that reads
  the last-touched cell, while that cell is the touched one. It goes when
  another cell is touched. It is never an always-on line.
- There is one shared hint element, with one look and one API.
- Each user can turn hints off.

## Decision
- **One component, `ui::hint`.** A `Gesture(keys, does)` is the keys in
  FONT_LABEL_BOLD INK, then what they do in FONT_LABEL MID. A `Hint` is a
  `&'static [Gesture]` set on one line. `hint::draw(d, hint, x, y)` is the
  only way a hint is drawn, and `hint::width` measures it. A test holds
  every hint within the screen less its margins (216 px).
- **A turn's direction is a mark.** `draw` gains 3×5 triangle marks: `◂▸`
  along a row, `▴▾` up and down. The fonts lack them.
- **In the focus band, under the value.** `hint::focus` draws at baseline
  `FOCUS_BOTTOM − 3` (115). The label and the value keep their places. A
  page's band shows the hint of the cell it reads:
  - ORBIT's cells give theirs through `orbit::draw::Focus::hint`
    (`OrbitCell::hint`);
  - a param gives its hint through `View::hint`.

  RING · NOTES and RING · VOICE move their band line and chips down 3 px
  to keep clear of the hint.
- **Every cell gesture has its hint:**

  | Where | Touched | Hint |
  |---|---|---|
  | RING · NOTES, STEP ring | NOTE | `MIX + NOTE ◂▸ STEP` `NOTE ▴▾ PITCH` |
  | RING · NOTES, pool | NOTE | `MIX + NOTE ◂▸ PICK` `NOTE ▴▾ PITCH` |
  | RING · NOTES | CLEAR | `MIX + CLEAR ▴ RESET  ▾ RANDOM` |
  | CHRD | DEGREE | `MIX + DEGREE ◂▸ SEGMENT` |
  | CHRD | PLAY | `MIX + PLAY ▾ DROPS THE CHORD` |
  | RING · RHYTHM | MUTE | `MIX + B1-5 MUTES THAT RING` |
  | MOD MATRIX | cursor on a route | `MIX + MINUS DELETES THE ROUTE` |
  | MOD MATRIX | cursor on no route | `MIX + PLUS ON A PARAM PRIMES IT` |
  | Sound browser | always | `EDIT LOADS` `MIX + EDIT SAVES` |
  | Sound browser | a filled slot | `MIX + MINUS CLEARS` |

  CLEAR's focus label is `CLEAR` again; its value stays `C4 / RND`. This
  amends ADR 0074.
  
  The matrix's readout band is its focus band, so its hint follows the
  cursor's route. The hint line stays at `HINT_Y`. This supersedes ADR
  0017's hint text.
  
  The browser's hints take three lines: the two keys, then CLEAR, then the
  slot count. This amends ADR 0081's two-line layout.
- **Left without a hint, deliberately.** Each of these belongs to no cell,
  or to every cell. A hint for it would be an always-on line.
  - MIX + turn snaps any param, a route knob to −127 / 0 / +127.
  - MIX + PLUS and MIX + MINUS prime and unprime the focused param. The
    outcome already reads in the band (ADR 0017), and the matrix hint says
    where columns come from.
  - The ladder's keys: MIX + B*n*, EDIT + B*n*, SEQ hold into ORBIT, MENU
    hold to quick save.
  - ORBIT's B*n* hold, which crosses to the ring's Part. ORB is a BigViz
    page with no focus band.
- **Chip row scrolls.** NOTES shows at most `CHIPS_SHOWN` (8) chips.
  `chip_window(sel, n)` is eight chips from a multiple of eight (a bar of
  a 16th ring), clamped to the last eight, so the selected chip is always
  on show. A scroll line under the row marks the window. The band reads
  `STEP n OF m`.
- **HINTS: ON / OFF**, default ON, is the fifth cell of SETTINGS ›
  PERSONALIZE › THEME (`ThemeSettings::HINTS`, param 4, a SWITCH). It is
  the page that already holds the display preferences. It is stored with
  the theme in SYSTEM, per device, not per project. A SYSTEM file from
  before HINTS has no HINTS param and reads as ON. With HINTS off, the
  `Frame::hint` gate draws no hint anywhere: the focus bands, the matrix
  and the browser. Every band keeps its normal reading, and every gesture
  still works.

## Alternatives considered
- **An always-on hint line per page.** The owner ruled it out. It also
  costs a band on every page, and it says the same thing on every cell.
- **The hint in place of the focus label, as CLEAR had it.** NOTE needs
  its label, `STEP n OF m`, so the hint can't take that place.
- **The hint beside the value.** A chord name or a SWITCH pill fills that
  space. Under the value, the line is free on every page.
- **Hints only in the focus band, the matrix and the browser keeping
  their own.** The owner asked for one element.
- **All 32 chips at once.** At about 5 px each, a chip can't hold a name
  or a selection you can see.
- **HINTS per project.** It is the user's preference, not the song's, as
  SYNC is.

## Consequences
- A new MIX or hold gesture on a cell needs a `Gesture` in its cell's
  hint. `hint_test` checks each one's text, that it fits, and that it
  shows only while its cell is touched.
- The CLEAR, NOTES, VOICE, CHRD, matrix, browser and THEME goldens are
  re-recorded, and `orbit_notes_step32` is new.
- SYSTEM gains the THEME param `B 26 4 HINTS` (`disk_codes_v1.txt`).
  Rewriting the v1 MIDI fixture is no longer byte-exact, because a write
  now carries HINTS. It still reads back the same.
- The global modifiers (snap, prime) stay undiscoverable on screen. If
  that bites, a help page under SETTINGS is the place for them, not the
  bands.

## Sources
- Owner's QA rulings, 2026-10-04 (relayed by the coordinator).
- ADR 0017 (prime status), 0074 (NOTES' chips and CLEAR), 0081 (D-pad,
  browser hints), 0080 (SYNC in SYSTEM, the model for a device setting).
- `chimera-core/src/ui/hint.rs`, `chimera-core/tests/hint_test.rs`.
