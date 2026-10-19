# 0154. LARGE text scales the reading text a size up where it fits

- **Status:** Proposed
- **Deciders:** owner (private issue #566, tester feedback 2026-10-16); firmware

## Context
A tester found the text small: "may not work for older users". The
screen is 240×320 and every band is laid out to the pixel (ADR 0016):
cells 74 px wide on a 36 px row, a 28 px header with the Part dots
above its text, a 54 px map band. Most of what a player reads is in
helvR08 (8 px caps: cell labels, hints) and helvB10 (11 px caps: cell
values, the focus label, list rows). The focus value is already
Logisoso 42.

A second full set of faces would not fit, and scaling the whole frame
(a 2× render) would not fit the screen either. One size up for the
small faces costs two u8g2 fonts. Not every word fits the slot it has
at the larger size, and the header and the map have no room to grow.

## Decision
- THEME gains **TEXT** (`ThemeSettings.text`, THEME param 8, an enum
  `TextSize { Normal, Large }`, disk codes NORMAL 0, LARGE 1), stored
  in SYSTEM like the rest. A SYSTEM from before TEXT reads NORMAL, the
  default. THEME's six cells are full, so it gets its own leaf,
  SETTINGS › PERSONALIZE › TEXT SIZE (crumb TEXT), one cell, SIZE. It
  is a choice: it steps, there is nothing to lerp.
- **What LARGE scales:** the cell labels (helvR08 to helvR10, cap 8 to
  11), the cell values (helvB10 to helvB14, cap 11 to 14), the focus
  label (helvB10 to helvB14), the focus band's hint (helvR08 / helvB08
  to helvR10 / helvB10), the whole-width SETTINGS lists' rows (helvB10
  to helvB14, their notes helvR08 to helvR10) and the browser's Sound
  names (helvB10 to helvB14). `theme::font(face, size)` maps the three
  reading faces (`Face::Label`, `LabelBold`, `Value`) to each size.
- **What stays NORMAL:** the header (the Part dots sit right above it),
  the crumbs and footer, the chain map, the focus value and its unit
  (already large), BigViz readouts and plot labels, the matrix, ORBIT's
  rings and chips, dialogs, MANAGE's two columns, and the browser's
  costs, tags and hint lines. Their geometry is fixed.
- **One fit rule, per text** (`theme::fit`): a text takes LARGE's face
  only when it fits its slot whole at that size; otherwise it draws
  exactly as NORMAL does, cut where NORMAL cuts. LARGE never truncates
  more than NORMAL. A hint line, and a list row with its note, fit or
  fall back as one. `draw::text_fit` draws a text against its slot
  (`draw::Slot`).
- **Slots:** a cell's label and value are a column less a 4 px gutter
  wide (70 px). The focus label and the hint end 4 px short of the
  gauge, or at the right margin with none. A list row's label ends at
  its note. A Sound name ends before its cost or tag.
- **LARGE's cell lines** (`theme::cell_lines`): the label baseline 1 px
  lower (its 11 px cap clears the cells band's top at 186), the value at
  +17, the bar at +20, the mod line at +23, so the mod tick clears the
  next row's taller label. A whole-width list row's label sits 1 px
  lower in its bar. NORMAL's lines are the grid's, unchanged.
- A change of TEXT redraws every band and the browser, as HINTS does
  (`RegionSet::drawn_hints`).
- NORMAL draws bit for bit as before: every existing golden is
  unchanged. New `large_*` goldens lock LARGE on ALGO, LEVEL, FLT,
  MODAL AMP, VA DRV, the mixer's SENDS, SETTINGS' top and PERSONALIZE,
  the TEXT SIZE leaf and both browser tabs.
- Under the test-support feature, `draw::overflow` records every text
  drawn in a LARGE face whose inked box leaves its slot. The LARGE walks
  (`representative_pages_walk_large` on every check,
  `every_page_walk_large` ignored and slow, `every_settings_screen_large`)
  and `the_large_cases_never_overflow` fail on any record.

## Alternatives considered
- **A 2× (or 1.5×) render of the whole frame:** the frame is the screen,
  so there is nowhere for the extra pixels to go.
- **A second full font set:** every face doubled, about 13 KB, and the
  header, map and matrix would still not fit at the larger size.
- **Three sizes (NORMAL, LARGE, LARGER):** helvB18 values would not fit
  a 74 px column at all. The fit rule would send almost every cell back
  to NORMAL.
- **helvB12 for values:** a 12 px cap against helvB10's 11 is barely
  visible. helvB14 fits the row once the bar and mod line move up.
- **Truncating at LARGE instead of falling back:** a cut word reads
  worse than a whole one a size smaller.
- **Scaling the header:** helvB10's 11 px cap would reach the Part dots
  (ADR 0150). The map names the page in its pill anyway.
- **A global font switch read by every draw call:** it would scale the
  diagrams too, which have no room. Tests running in parallel at both
  sizes would also share it. The size travels in the frame (`Frame::text`,
  `Bands::text`, the browser's `draw`) to the callers that use it.

## Consequences
Bank 1 grows 5,248 B on the ORBIT build (850,440 to 855,688 B, still
61,816 B under the SD-flash limit): helvR10 (1,289 B) and helvB14
(1,697 B), plus the fit and slot code. Bank 2 is unchanged. LARGE
measures each scaled text once more at the larger face when its band
redraws, a few hundred cycles, never on the audio side.

Under LARGE a column can mix sizes: a cell whose value is too wide
(`3.54 kHz`) keeps NORMAL's face beside larger neighbours. Every new
screen that draws reading text decides whether it is reading text
(`text_fit` with a slot) or a diagram (NORMAL faces).

## Sources
- private issue #566; ADR 0016
  (Direction A), 0145 (THEME's own leaf for a new option), 0150 (the
  Part dots).
- u8g2 font headers (`u8g2-fonts` 0.8.0, `fonts/*.u8g2font`): cap heights
  helvR08 8, helvR10 11, helvB10 11, helvB14 14. Font licences as in
  ADR 0016.
