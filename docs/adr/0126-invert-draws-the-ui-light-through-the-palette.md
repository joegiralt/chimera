# 0126. INVERT draws the UI light through the palette (extends 0033)

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
  ACCENT_SOFT, WARN and ALERT by exact match; every other pixel by a
  bitwise NOT. A grey of level c becomes 31 - c, so INK is near black, FAINT
  a light grey, and flipping twice is the identity. NOT only suits greys, so
  `light_golden_test` asserts every colour on every screen is a theme token
  or a neutral grey; a new hard-coded colour fails it (all five coloured
  tokens are exact swaps). The renderer, the goldens and dark mode are
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
- **A hue-keeping luminance flip** for non-grey pixels: no pixel needs it
  (the guard test), and it cost about 200 B of code.
- **NOT with no exact swaps**: greys mirror, but teal turns rose and red
  turns cyan, hence the swaps.

## Consequences
Toggling INVERT reflushes the whole screen once (~25 ms), as ACCENT does. The
flush branches on INVERT once per 256-pixel chunk (`Palette::map_into`); dark
keeps its three compares a pixel, light adds two compares and a NOT, about
1.5 ms over a full-screen flush estimated (76,800 px, ~10 cycles at 480 MHz)
against ~25 ms of SPI. An antialiased blend of accent over ground would not
be an exact match and would be NOTed to a wrong hue; the guard test keeps
the renderer from drawing one.
