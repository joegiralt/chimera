# 0129. Draw VA's oscillators and operator on the focus band; name BLEND's ends by MODE

- **Status:** Proposed
- **Deciders:** owner (UX approval), Claude (implementation)

## Context
VA's home page (WAVE1, WAVE2, MODE, AMNT, SUB, STAB) showed an arc for
WAVE, no gauge for MODE and a bare crossfader for AMOUNT. None of them
showed what the knob does to the sound, which the product vision asks of
every gauge. AMOUNT also means different things per MODE: a fade from
OSC 1 to OSC 2 under MIX and SYNC, from the plain mix to the operator
under RING and XOR, and the crusher's depth under BITS (`combine::bits`,
16 bits at 0 down to 2 at 1; AMOUNT does not fade between the
oscillators there).

## Decision
1. WAVE1 and WAVE2 (`FocusGlyph::Va(VaGlyph::Wave(_))`) draw a scope of
   one period of that oscillator's naive wave at its set WAVE and SHAPE,
   from `osc::wave_at` (the oscillator's own shape code, no second copy),
   scaled to its peak. WAVE's in-between values draw the mixed shape, so
   the scope slides as WAVE turns.
2. MODE (`VaGlyph::Mode`) draws `combine::combine` over two of OSC 1's
   periods: both naive waves at their WAVE and SHAPE, the set AMOUNT, the
   engine's peak bounds, RING gain and crusher, OSC 2 at a fixed 2.5
   times OSC 1 (`VA_MIX_RATIO`) and, under SYNC, reset at OSC 1's wrap.
   A fixed ratio, not CRSE: INIT's unison would hide SYNC. Its frame is
   96 px wide so two periods read; every MODE word ends before it.
3. AMOUNT reads BLEND on the page. It keeps the crossfader, on a 96 px
   track, with MID end words under its ends from `blend_ends`: MIX and
   SYNC `1` / `2`; RING `MIX` / `RING`; XOR `MIX` / `XOR`; BITS `CLEAN` /
   `CRUSH`.
4. STAB reads STABILITY on the page.
5. Labels change on the page slot only (`with_label`): the idents and
   disk names AMOUNT and STAB stay, so saved Sounds load unchanged.
6. The glyphs are pure functions of set values (`va_wave_trace`,
   `va_mix_trace`, `blend_ends`), drawn only when their inputs change:
   the focus band's key carries `Gauge::outside_key`, the params they
   read beyond the focused one. Nothing on the audio path changes.

## Alternatives considered
- The real CRSE ratio on MODE's glyph: at INIT's unison SYNC and RING
  look like nothing at this size.
- BITS ends `16 BIT` / `2 BIT`: exact, but the band names what the sound
  does; CLEAN and CRUSH are honest, 16 bits being clean at this level.
- The 64 px crossfader: CLEAN and CRUSH together are 84 px in the label
  font.
- A BLEP-corrected scope: the alias correction is invisible at 55 points.

## Consequences
The scopes cost a few dozen `wave_at` calls a redraw, on the UI side only.
MODE and BLEND's glyphs are wider than the other gauges, so their value
texts are pinned to end before them (`va_glyph_test`).

## Sources
- `chimera-engine/src/dsp/va/osc.rs` (`wave_at`, `peak_bound`),
  `chimera-engine/src/dsp/va/combine.rs` (`combine`, `Crusher`, `bits`).
- `chimera-core/tests/va_glyph_test.rs`.
- Owner approval of the four changes, 2026-10-11.
