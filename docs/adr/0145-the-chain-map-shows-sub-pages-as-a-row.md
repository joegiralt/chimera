# 0145. The chain map shows a node's sub-pages as a row; STACK stays a THEME option

- **Status:** Proposed
- **Deciders:** owner (2026-10-17, private issue #579); firmware

## Context
The chain map (ADR 0016, Direction A) hung a node's sub-pages under its
pill as a vertical list. The map's band is 54 px tall, so the list showed
only the nearest two pages and scrolled; OSC has 19 and MOD 8, so most of
a node's pages were never on screen. ADR 0081 made SEQ go to the previous
sub-page and EDIT to the next, wrapping from the last to the first; with
the pages stacked, the D-pad's left and right moved up and down a list you
couldn't see. The owner approved a mockup of a sub-page row
("Sub-page row (#579)", `chainRow()` in the drive-spike mockups) and asked
for it to be built and made switchable in THEME.

## Decision
- THEME gains **PAGES** (`ThemeSettings.pages`, THEME param 7, an enum
  `Pages { Row, Stack }`, disk codes ROW 0, STACK 1), stored in SYSTEM like
  the rest. A SYSTEM from before PAGES reads ROW. ROW is the default.
- THEME's page has no free cell (BRIGHT, GAMMA, ACCENT, BLACK, HINTS,
  INVERT), so PAGES has its own leaf, SETTINGS › PERSONALIZE › CHAIN MAP,
  held in the THEME block as SCREENSAVER's SAVER is (ADR 0127).
- **ROW** (`dungeon_map::draw_row`): the nodes on a line at y 279, the
  current one an accent pill at least 34 px wide (its bold label plus 10),
  18 high; the others a 3 px ring with the label below at 296. Under a
  faint rule at y 302, the current node's pages sit on one line, baseline
  314, between x 22 and 218, 10 px apart and centred. The current page is
  bold, in the accent, underlined; the others are MID. When they don't all
  fit, `page_window` grows a window from the current page outward (a page
  right, then a page left, until neither fits); `<` at x 10 shows when
  pages are cut on the left, `>` right-aligned at 230 when cut on the
  right. A node with no sub-pages draws no rule and no row.
- **STACK** is the list as it was, bit for bit; every screen golden from
  before this ADR is recorded under STACK and unchanged, and new `row_`
  goldens lock ROW.
- **Navigation is unchanged** (ADR 0081): SEQ is the previous sub-page and
  does nothing on the first; EDIT is the next and wraps from the last;
  MINUS and PLUS move along the nodes. Only the drawing changes, so SEQ and
  EDIT now read as left and right through pages you can see.

## Alternatives considered
- **ROW only:** the owner asked for the drawing to be switchable.
- **A seventh cell on THEME:** the page has six; moving one off would
  change a page the owner just signed off.
- **The mock's widths for every page in the label font:** the current page
  is drawn bold, so its underline and spacing use the bold width.
- **A window that holds still until the current page reaches its edge:**
  it needs the last window as state; the mock grows it from the current
  page each frame, so the window is a pure function of the widths and the
  page, and the current page sits near the middle.

## Consequences
The map's band now shows six or seven pages at once and where the
rest lie. The region key carries PAGES, and ROW keys no scroll, so the
STACK list's easing doesn't redraw the row. The node line sits 7 px higher
in ROW than in STACK. Any later map change has two drawings to keep.

## Sources
- `chainRow()` in (private notes);
  rendered page "Sub-page row (#579)", an unpublished page
- ADR 0016 (visual direction), ADR 0081 (one D-pad), ADR 0126 (THEME's
  INVERT, which took the sixth cell), ADR 0127 (SCREENSAVER's own leaf,
  the precedent for CHAIN MAP's)
