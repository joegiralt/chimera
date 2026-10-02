# 0069. ORBIT runs on the audio thread, block-accurate

- **Status:** Proposed (accepted at the ORBIT plan's ship flash)
- **Deciders:** owner (ORBIT design spec, 2026-09-30), firmware

## Context
ORBIT plays notes on the Parts from a clock. Its steps could come from the
UI loop (jittery: a frame is tens of ms and a card read blocks it), or from
the audio callback, where a block is 64 samples. A scene is also edited
while it plays, and MUTATE drifts it, so two writers want one scene. A
project load kills every voice (ADR 0046), and a STOP must not cut a note
the player is holding.

## Decision
- **The audio thread runs ORBIT** (`OrbitRunner::block`, spec § 4), after
  `LoadGate::before_block` has opened the gate and the live notes are
  drained, and before `Instrument::render`. Notes land block-accurate:
  contiguous windows, each centred on the block's start at -32 and +32
  samples (spec § 6.2), so a note is within 32 samples of its time and no
  step falls between two windows (ADR 0019 already accepts a block's
  quantisation). No allocation, no blocking.
- **The engine owns the drift, the UI owns the knobs, and revs say who
  wins.** The runner keeps a working copy of the scene. Each block it
  copies the knobs from `AudioShared.orbit.settings`; a ring's hits (only
  through `with_mask`, so a changed fill with no rev still stands) and its
  notes and pool only when `rhythm_rev` or `notes_rev` moved. A changed
  `scene_rev` queues the scene: the settings are ignored until the next loop
  boundary (at once if stopped), then adopted whole.
- **The transport acts on edges.** A false to true edge of `run` starts
  (fresh clock, loop start, working copy from the settings), true to false
  stops; a level with no edge does nothing. A new load epoch stops the
  runner with no edge and only a fresh false to true edge starts it again.
- **A note-off matches its origin, channel and key** (`NoteOrigin`): a ring's
  off never releases a held key or another ring's note.
- **STOP releases only ORBIT's notes**, one off each in the same block, and
  nothing else. A load forgets its pending offs instead: the voices are
  already dead.
- **The report goes back through a triple buffer** (`OrbitView`, ADR 0021),
  written after the render each block.

## Alternatives considered
- **ORBIT on the UI loop.** Jitter of a frame, and a blocked loop stalls the
  clock. Rejected.
- **Sample-accurate starts.** Threads an offset through every engine for at
  most 1.33 ms. Below what ADR 0019 accepts; rejected.
- **UI owns the whole scene, audio reads it.** MUTATE's drift would race the
  edits, or be lost at each frame's publish. Rejected.
- **A level-triggered `run`.** A project load would restart ORBIT on its own
  while `run` stays true. Rejected.

## Consequences
- The runner is 616 B in the audio thread's state, the view 288 B.
- A knob edit lands within a block, a rhythm or note edit replaces the drift
  of that ring only, and a scene switch waits for the loop.
- The UI must bump a rev when it replaces a ring's rhythm or notes.
- Held notes end on their gates or at STOP; ORBIT never damps a ring.

## Sources
- ORBIT design spec `docs/superpowers/specs/2026-09-30-orbit-design.md`:
  § 4, § 4.1, § 6.2, § 6.3.
- ADR 0019 (note queue), ADR 0021 (triple buffer), ADR 0046 (load epochs).
