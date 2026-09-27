# 0033. The theme is applied in the display shell, not the renderer

- **Status:** Proposed
- **Deciders:** project owner

## Context
System › Theme (BRIGHT · GAMMA · ACCENT / BLACK) makes the backlight, the
panel gamma, the accent colour and the ground runtime settings. The accent
and the ground were `const`s read all over the renderer, viz and tests, and
ADR 0011 locks every screen with a golden hash. On the hardware, the
PreenFM3's TN panel shows the ground as navy and the Adafruit gamma tables
(commit 7293a02) muted the teal, so both need choices, not one fixed value.

## Decision
- The settings are a `ThemeSettings` value in chimera-core
  (`ui::theme_settings`) held by `UiState`, addressed as
  `BlockRef::Theme` params 0–3 (ADR 0009) through the Theme page's slots.
  They are not in a Sound. No storage yet: every boot is 75 / PANEL / TEAL / 0.
- The renderer keeps drawing the canonical palette. `ThemeSettings::palette`
  gives three swaps (ACCENT, ACCENT_SOFT, BG), and the display shell maps each
  framebuffer pixel through them as it pushes it out (SPI on the firmware, the
  window buffer on the desktop). TEAL with BLACK 0 is the identity.
- BRIGHT is the TIM1 PWM duty; GAMMA sends E0h/E1h from the UI loop, which
  owns the display. PANEL sends the ILI9341 datasheet's reset tables (the chip
  has no restore-gamma command); PUNCH is Adafruit's; SOFT is each register
  field halfway between the two, rounded down. The desktop dims its window by
  BRIGHT and ignores GAMMA.
- The accents: TEAL `#7fd4c8`, AMBER `#e8b45c`, ROSE `#e89aae`, LIME `#b4d86a`,
  ICE `#a4c4f0`; each soft variant is 12 % over `#0a0b0d`. BLACK −2…+4 moves
  the ground's green one RGB565 level a step and red/blue half that (neutral;
  −2 is black, +4 is `#181818`); the soft accent moves with it.

## Alternatives considered
- **`theme::accent()` reading a global** — every draw call site changes, and a
  global shared by parallel host tests makes an accent test race the goldens.
- **A palette passed through every draw function** — the same churn, for a
  choice that only matters when pixels leave the framebuffer.
- **Software reset for PANEL** — restores the gamma but blanks the panel and
  reruns the whole init.

## Consequences
The goldens and the renderer are untouched by the theme; any new colour that
should follow the accent must be drawn with `theme::ACCENT`/`ACCENT_SOFT`
(an exact colour match, so no blending of them). A palette change reflushes
the whole screen (~25 ms of SPI). The pixel map costs three compares per
pixel on every flush.

## Sources
ILI9341 datasheet V1.11 (ILI Technology), § 8.1 command list pp. 86–87 and
§ 8.3.24–25 (E0h/E1h defaults), § 15.1 (reset leaves only GC0);
Adafruit_ILI9341 init tables (BSD), as in commit 7293a02; commit 56b0336.
