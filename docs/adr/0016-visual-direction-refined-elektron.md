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

Measured cost (release firmware, `llvm-size -A`, UI refresh final task —
removing `cell.rs`, `CellIcon` and the dead pre-Direction-A theme constants;
the fonts and the rest of Direction A were already in place from earlier
tasks in this plan): `.text` 172 360 → 172 072 B (−288 B), `.rodata`
48 056 → 46 960 B (−1 096 B); `.text` + `.rodata` −1 384 B overall. `.data`
and `.bss` unchanged at 40 696 B / 156 844 B. Font data (all five faces)
is 10 253 B — logisoso42 4 625, logisoso20 2 226, helvB10 1 333, helvR08
1 041, helvB08 1 028 — within the 16 KB budget. AXI: 511 440 B of
524 288 B resident, 12 848 B headroom (`memory_budget_test::axi_residents_fit`),
unchanged by this task; `UiState` besides its `Performance`/`SoundPool` is
1 120 B (`ui_state_fits_the_ui_reserve`), also unchanged.

## Addendum: Whole-branch flash cost (2026-09-24)
Recorded after the whole-branch review of the UI refresh; the decision above
is unchanged.

Release firmware, `llvm-size -A`, branch base (merge-base with `main`,
f5ce050) against the reviewed branch head (2d3572c): `.text` + `.rodata` 215 316 →
219 032 B (+3 716 B: `.text` −7 824 B, `.rodata` +11 540 B). `.data`,
`.bss` and `.ram_d2` unchanged. The five font faces account for ≈10.25 KB of
the `.rodata` growth, within the 16 KB font budget.
