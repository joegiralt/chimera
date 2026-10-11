# 0122. INVERT draws the UI light through the palette (extends 0033)

- **Status:** Proposed
- **Deciders:** project owner

## Context
The owner asked for a light mode on SETTINGS › PERSONALIZE › THEME. ADR 0033
keeps the renderer drawing one canonical dark palette and swaps colours in
the display shell; every screen golden (ADR 0011) hashes that palette.

## Decision
- `ThemeSettings.invert` (THEME param 5, `INVERT` OFF/ON, stored in SYSTEM
  like the others; an older SYSTEM reads OFF).
- ON, the shell maps every framebuffer pixel (`Palette::map_raw`): BG, ACCENT,
  ACCENT_SOFT, WARN and ALERT by exact match; every other colour by a
  luminance flip that keeps hue (`flip_raw`). A neutral grey of level c
  becomes 31 - c, so INK is near black, FAINT a light grey, and flipping twice
  is the identity on greys. The renderer, the goldens and dark mode are
  untouched; light goldens (`light_golden_test`) hash the mapped render.
- BLACK in light mode lifts nothing: the ground is the dark ground at the
  same BLACK, bit-flipped. -2 is white, +4 about `#e7e7e7`.
- ACCENT keeps the pick, darkened toward black in 1/128 steps only until
  its luminance holds 3:1 (WCAG, UI glyphs) against the actual ground; the
  check uses gamma 2.0 (reads dark colours lighter than sRGB) with a 3.2
  threshold, and the tests check true sRGB. WARN and ALERT follow the same
  rule. ACCENT_SOFT is 12 % of the light accent over the light ground.
- Fixed-colour art: the boot splash (the avatar's yellow, ink = BG) is drawn
  through `splash_palette` (light mode off) and stays as is. No other colour
  is hard-coded outside `ui::theme`; WARN and ALERT are theme constants.

## Alternatives considered
- **A light palette in the renderer** (consts read through a global): the
  churn ADR 0033 rejected, and it would move every dark golden.
- **Bitwise NOT of every pixel**: greys mirror, but teal turns rose and
  yellow turns blue.

## Consequences
Toggling INVERT reflushes the whole screen once (~25 ms), as ACCENT does. The
light map costs a few compares and a multiply per pixel on a flush (dark
mode keeps its three compares). Antialiased blends of accent over ground
are not exact matches and take the luminance flip, so their edge pixels
differ slightly from the accent.
