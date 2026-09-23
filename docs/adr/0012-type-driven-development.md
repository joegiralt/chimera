# 0012. Type-driven development where it pays

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
Many bugs found in review were states the types allowed: operator index out
of range, integer params stored as floats and truncated, UI formats
disagreeing with ranges, position-based addresses.

## Decision
Encode invariants in types where cheap: newtypes created at trust boundaries
(`MidiNote`, `Velocity` in the MIDI parser), enums instead of magic numbers
(`ResonatorMode`, `Op`, `FilterMode`), private fields where construction must
be validated (`ModState`, registry), exhaustive `match` so new variants are
compile errors. Don't add types with no consumer (`Hz`, `Cost`, `Budget` wait
for the allocator and new engines). Tests cover behavior; types cover wiring.

## Consequences
Fallible conversions happen once at the boundary; the audio path takes
already-valid values.

## Sources
Project owner's stated preference; adversarial reviews of the engine
refactor spec.
