# 0016. Visual direction: refined Elektron

- **Status:** Accepted (2026-09-24)
- **Deciders:** project owner

## Context
The on-device UI read as test equipment: one 6×10 mono font for nearly every
string, 1-pixel outlines around every element, a cyan scope trace on dark with
grid lines, and no hierarchy between the value being turned and the rest.
Three directions were mocked at native 240×320
(https://claude.ai/artifact/B1ydzBo2GpjTY1QjH5hdvS): A refined Elektron,
B colour per encoder, C dungeon crawler.

## Decision
Direction A: keep the dark ground and a single accent, and fix hierarchy and
typography.
- Two or three bitmap type sizes: large bold numerals for the value being
  turned, small uppercase labels, a mid size for values.
- The active encoder's value is shown large (with an arc gauge) while it is
  being turned, then settles.
- No outline boxes; separation by spacing. Values as thin bars/arcs.
- One accent colour for the active element; everything else in warm greys.
- Visualisations (waveform, filter curve, envelope) are drawn as the main
  element, with a soft fill; no grid lines.
- The map is a line of nodes with the current block as a filled pill.

## Alternatives considered
- **B colour per encoder** — playful and readable, but more assets and a
  louder identity than wanted.
- **C dungeon crawler** — distinctive, cheapest over SPI, but a strong
  theme to commit every page to.

## Consequences
The page anatomy (header, viz, six cells, map) is unchanged, so this is a
renderer/theme change, not a UI rewrite. Bitmap fonts cost ~6–10 KB of
flash; RAM unchanged (ADR 0013 budgets apply).

## Sources
Mockups above; `chimera-core/src/ui/theme.rs`, `ui/renderer.rs`.

## Addendum: Font licences (2026-09-24)
Recorded when `u8g2-fonts` was added (UI refresh plan); the decision above is unchanged.

| Font (u8g2 name) | Use | Author / licence |
|---|---|---|
| crate `u8g2-fonts` 0.8.0 | renderer | Finomnis; MIT OR Apache-2.0 |
| `logisoso42_tr`, `logisoso20_tr` | focus value, viz readout | Mathieu Gabiot (2009); GPL v2 with font exception per its copyright statement, OFL per openfontlibrary.org — either permits embedding in firmware |
| `helvB10_tr`, `helvR08_tr`, `helvB08_tr` | values, labels, map | Adobe / Digital Equipment Corp. X11 bitmap fonts; permission notice in the U8g2 LICENSE (use, copy, modify, distribute, sell; keep the notice) |

Sources: https://github.com/olikraus/u8g2/blob/master/LICENSE,
https://github.com/olikraus/u8g2/wiki/fntgrplogisoso
