# 0059. Music theory is its own crate, built from types

- **Status:** Proposed
- **Deciders:** owner; firmware

## Context
ORBIT (#216) needs keys, scales, chords, snapping and names, and
microtuning (#217) will need pitch too. None of it is DSP or UI, and all
of it is easy to get subtly wrong with bare integers: a pitch class of 13,
a scale without its root, a chord named for tones it doesn't hold. A first
cut named every chord by its nearest `Quality` and kept only root and
quality, so C E A became C6 (C E G A) and snapped to a G nobody played.

## Decision
- **Crate boundary.** `chimera-theory` is a `no_std` crate with no
  dependencies. It holds pitch classes, intervals, notes, scales, keys,
  degrees, chords, labels and `snap`, and nothing else. It is in the
  workspace's default members and in `just check`, `just test` and
  `just clippy`.
- **Types first.** Invalid states are unrepresentable and functions are
  total. `PitchClass` is an enum, `Interval` is mod 12, `Note` is checked
  (0..=127). `Scale` is a 12-bit set that always holds its root.
  `Degree`, `Quality` and `SnapTo` are enums. Tables are `const`, with
  `const` asserts on their shape and a 512 B size cap. The public API has
  no bare `u8` for a musical quantity and no typenum-style generics.
- **Snap ties go down.** A note halfway between two allowed pitches snaps
  to the lower one, and never leaves 0..=127.
- **Exact tones, smart labels.** A `Chord` stores exactly its tones
  (`PcSet`) with the note it was built on and its bass, both among the
  tones by construction. A played chord keeps its pitch classes on the
  lowest note; a stacked degree keeps the stacked notes. Snapping uses
  exactly those tones. The name is a `ChordLabel`: every tone is tried as
  the root (the built-on note first, then the bass, then the rest rising)
  for an exact `Quality`; a root that isn't the bass reads as a slash
  chord, so C E A is Am/C. Only when nothing matches is the nearest
  quality shown, marked approximate with a trailing `?`.
- **Custom scales.** `Scale::custom` takes any 12-bit set and forces the
  root on. Degrees stack every other note of the set and wrap on sets
  smaller than seven, so a pentatonic degree stacks three of its own
  notes, and a two-note set stacks one.
- **Tuning is separate.** Microtuning (#217) adds a `Tuning` type that
  maps notes to frequency. Pitch classes, scales and chords stay 12-tone
  and don't change for it.

## Alternatives considered
- **Chord as root plus nearest quality.** Always named, but its tones
  aren't the ones played or stacked, and it snaps to notes that aren't
  there. Replaced by this ADR before it shipped.
- **Theory inside `chimera-core`.** Works, but ties theory to the DSP
  crate's build and features, and hides the boundary ORBIT and tuning
  share.
- **Bare `u8`/`u16` with checks at call sites.** Smaller to write,
  but every caller has to remember the invariants.

## Consequences
- ORBIT and tuning build on one checked vocabulary; the firmware pays
  only for what it references (the tables are under 512 B).
- Sets two qualities share (C6 and Am7, Csus2 and Gsus4, aug, dim7)
  are named from the note the chord was built on; played, that is the
  bass, so a played E G A C reads Am7/E.
- A stacked degree on a small or odd set may have no exact name; its
  label is approximate and its roman numeral reads "I?".
- Spelling is sharps-only ASCII ("C#", "m7b5", "dim"), C4 = 60.

## Sources
- ORBIT: https://github.com/joegiralt/chimera/issues/216
- Microtuning: https://github.com/joegiralt/chimera/issues/217
- `chimera-theory/src/lib.rs`, `chimera-theory/tests/theory_test.rs`
