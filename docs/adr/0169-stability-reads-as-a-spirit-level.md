# 0169. Draw STABILITY as a spirit level, VA's and the filter's alike

- **Status:** Proposed
- **Deciders:** owner (2026-10-17 picked the spirit level from the mockups, #593; 2026-10-20 asked for it with the filter's STABILITY)

## Context

VA's oscillator STABILITY (ADR 0118) and the 2040's and LADDER's STABILITY (ADR 0168) are one idea, "how far the voices wander", and both showed the arc. #593 asks for a spirit level: a vial with two centre marks, the bubble between them at full stability and wandering further as stability drops. Glyphs here key on quantised set values with no frame term, so an idle page never redraws (ADR 0177's rule).

## Decision

- **One glyph, `FocusGlyph::Stability`**, on VA's `STAB` and the filter's `STAB`. `Gauge::Stability { offset, reach }` is still (`animates` false, no outside key), drawn from the set value as the crossfader is, never the modulated one.
- **The law.** VA's depth, `(1 − STAB)²`, so the picture moves as the sound does. `reach` is that share of the bubble's travel (`BUBBLE_TRAVEL`, 22 px either side) in whole pixels. The bubble rests `reach / 2` right of the marks, and a FAINT ghost spans ±`reach`: where it can wander. At 127 the bubble sits between the marks with no ghost. At the Prophet setting (64) it is 3 px off with a 6 px reach. At 0 the ghost reaches the vial's ends.
- **The drawing.** A 64 × 20 FAINT vial outline on the arc's centre line, flush right. Two MID marks just clear of the bubble at rest, through the vial's top. A 16 × 12 ACCENT bubble. Every colour is a theme token. It is vector drawing, a few hundred bytes in bank 1, with no raster and nothing behind the bank-2 door.
- **No live wobble.** The bubble does not follow the voices' actual drift: that would need an audio-side view and a redraw while sounding. The ghost says how far it wanders instead. The cells keep their value and bar: they already have a glyph.

## Alternatives considered

- A spinning top: rejected by the owner (it read as a flat kite).
- A wobbling bubble driven by a frame term: redraws an idle page every frame. Driven by the voices' drift (as the LFO dot): costs an audio view for a picture that a ghost already tells.
- A mini vial in the cells: the cells' value and bar already say the setting; the band shows the picture.

## Consequences

VA's and the filter's STABILITY focus bands draw the vial. Bank 1 grows 232 B (ORBIT). The glyph can serve other "how much it wanders" controls later (TAPE's AGE or WOW).

## Sources

#593; the mockups' STABILITY glyphs page; ADR 0118, 0168, 0177; owner request 2026-10-20.
