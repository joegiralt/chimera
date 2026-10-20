# 0181. TAPE's laws: WALK in seconds of source on a cubic, STRETCH's slow speeds exponential, AGE's corner from the source's band

- **Status:** Proposed
- **Deciders:** owner (#698), firmware

## Context
Musical findings 7(b), 10 and 14. WALK was linear in windows a second: 62 ms/s a detent on a 4 s source and 390 ms/s on a 25 s bed, about six detents of gentle drift. STRETCH SPEED was linear from −1× to +2× with 1× at +½, so the musical slow stretches (0.1–0.5×) got 8 detents. AGE's tone corner fell from an absolute 20 kHz, so on an 8 kHz TRON source it met the band edge at AGE 0.77, after the dropouts had begun.

## Decision
- **WALK:** `walk_s(m) = WALK_MAX_S · m³` seconds of source a second, `WALK_MAX_S` 4, scaled by the window's own length, so a detent feels the same on any source (MOTION's bipolar cubic elsewhere).
- **STRETCH SPEED:** freeze at 0, 1× at ½ and 2× at 1 as before; under 1× either way `(2^(4u) − 1)/15`, so 0.1× to 0.5× takes 14 detents. `HALF_SPEED` (MOTION 0.386) is 0.5×.
- **AGE's corner:** `min(rate/2, 20 kHz) · 8^(−AGE)`, set from the note's source at note-on (`Age::set_band`): one multiply, so AGE darkens over its whole travel on every source.

## Alternatives considered
- **WALK exponential in windows.** It still feels different on a 2 s and a 25 s source.
- **STRETCH SPEED fully exponential.** It would lose freeze at the centre, the S950's TIMESTRETCH stop.

## Consequences
- Saved Sounds with WALK, STRETCH SPEED below 1× or AGE above 0 move differently or sound darker; they load safely (no-legacy-patch-compat rule).
- The STRETCH listening script uses `HALF_SPEED` for its 0.5× takes.

## Sources
- Musical findings 7, 10, 14; #698. Akai S950 Operator's Manual p. 32 (TIMESTRETCH).
- `chimera-engine/src/dsp/tape_engine/{mod,stretch,age}.rs`.
