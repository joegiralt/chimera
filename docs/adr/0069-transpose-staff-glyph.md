# 0069. TRANSPOSE takes STAFF: a note on a three-line staff (amends 0067)

- **Status:** Proposed
- **Deciders:** project owner (issue #309); firmware

## Context
TRANSPOSE (−24..+24 semitones) showed ARC, which reads as an amount, not
a pitch. Issue #309 asks for a glyph that reads as a note moving by
semitones.

## Decision
- **A new kind, `FocusGlyph::Staff`**, assigned on TRANSPOSE's spec only:
  three MID lines and an ACCENT notehead, flush with the right margin
  on the arc's centre line (`theme::STAFF_*`), palette colours only.
- **Position is the diatonic step, spelled in C major** (`StaffNote::of`,
  a pure function): 0 on the middle line, no accidental; ±12 is ±7
  steps; MID ledger lines past the staff. An in-between semitone takes
  ♯ when the offset is positive and ♭ when it is negative (−1 is B,
  −2 B♭).
- **Past an octave the note wraps within it** (+19 shows +7's G); the
  number beside it carries the full value.
- **It draws the set value**, eased in `Renderer::set` like CROSSFADER:
  the notehead slides between the whole semitones either side, the
  accidental the nearest one's. No new animation.
- **Its Demo page is "Glyph: Staff"** (id 90: ORBIT has 76 to 86), slot a TRANSPOSE.

## Alternatives considered
- **A five-line staff with clefs:** too much for the focus band's 56 px; the
  middle line as C needs no clef.
- **Spelling negatives with sharps:** −1 would read as C♯ below the
  line; going down, flats are the natural spelling.

## Consequences
- Wrapping means +7 and +19 look alike on the staff; the readout tells
  them apart.

## Sources
- https://github.com/joegiralt/chimera/issues/309
- ADR 0067; `chimera-core/src/ui/glyph.rs`, `components::staff`.
