# 0074. Edit a ring's notes by chip; CLEAR up resets, CLEAR down rolls in key

- **Status:** Proposed
- **Deciders:** implementer (plan Task 13), for the owner's UAT

## Context
RING · NOTES (spec § 7.1) shows a ring's notes as chips and has to edit
them by hand before LEARN (Task 14) can capture them from the keys. The
plan gave the cells (MODE, WALK, TARGET, LEARN, CLEAR, NOTE) but not how
a turn of NOTE or CLEAR behaves in a pool that keeps itself sorted, or
what a STEP ring's CLEAR touches.

## Decision
- **Chips.** A POOL ring's chips are its pool (1–8); a STEP ring's are its
  STEPS steps. NOTES' selected chip is one `u8` in `OrbitUi`, shared by
  the rings and clamped to the ring's chips when read (as CHRD's segment).
- **NOTE.** One semitone a detent, C-1 to G9. MIX + NOTE picks the chip.
  The pool re-sorts after an edit and the selection follows the edited
  note to its new place.
- **CLEAR.** A turn right leaves one note at OCTAVE's C (a STEP ring:
  every one of its 32 stored notes). A turn left rolls a full pool of 8
  (a STEP ring: all 32) of notes of the key in the register, drawn as
  MUTATE draws them (`mutate::register_note`), from `Rng::for_ring(seed +
  (notes_rev << 16), ring)`; each roll bumps `notes_rev`, so every turn
  left rolls anew and the same scene rolls the same way.
- Every change to the notes bumps `notes_rev`; a NOTE turn that changes
  nothing (at an end) does not.
- **NOTES' line** is `POOL p · HITS h · COMES ROUND IN n` with
  n = `lcm(pool · (RANGE + 1), hits)` (spec § 3.4); a STEP ring reads
  `STEPS s · HITS h · COMES ROUND IN h`.
- **VOICE** draws each hit as a bar GATE long and VEL tall, an accent at
  VEL + 30 and in INK2 (the controller's ruling on accents).
- WALK's PING-PONG reads `PING-PG`: `PINGPONG` is wider than a cell.

## Alternatives considered
- The selection stays on its index after a re-sort: the next turn edits
  another note than the one just moved. Rejected.
- CLEAR behind a confirm: the encoders have no press, and a prompt for a
  turn that is quick to redo costs more than it saves. Its cost is in
  Consequences.
- Rolling only STEPS steps: raising STEPS later would reveal old notes.

## Consequences
- CLEAR discards hand-edited notes with no undo.
- The roll is reproducible from the scene's seed, so a saved scene and its
  `notes_rev` play the same.
- LEARN is inert and reads OFF until Task 14.

## Sources
- docs/superpowers/specs/2026-09-30-orbit-design.md § 3.4, § 3.5, § 7.1
- .superpowers/sdd/2026-09-30-orbit/task-13-brief.md
