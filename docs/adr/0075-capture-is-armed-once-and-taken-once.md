# 0075. A capture is armed once and taken once; a STEP ring's spans key-ups

- **Status:** Proposed
- **Deciders:** implementer (plan Task 14), for the owner's UAT

## Context
Spec § 5.3 has NOTES' LEARN and CHRD's PLAY arm a capture from the keys,
carried to the runner in `OrbitShared.capture` and reported back in
`OrbitView`. The UI publishes every frame through a triple buffer
(ADR 0021), and frames lag the audio blocks, so the runner can see the
target still armed after it has taken a capture. The spec also leaves
open how a STEP ring's capture ends when the notes are played one at a
time, what happens past eight notes, and what PLAY's turns do.

## Decision
- **Arming is a change of the published target.** The runner remembers
  the target it last saw. A different one, `Off` included, re-arms and
  drops what was under way. A finished capture disarms the runner
  itself, and the same target published again does not re-arm it. So a
  capture is taken once per arming, whatever the slots say after. The UI
  takes each new `Captured.count` once, writes it into the song (a
  ring's `notes_rev` bumped), and disarms. A load disarms too, and a
  capture reported from before a load is dropped.
- **Who decides.** The runner decides when a capture ends and whether a
  segment's notes make a chord: under two pitch classes, nothing is
  reported and it stays armed. The UI writes the result
  (`Scene::take`).
- **A capture's notes:** at most 8, each once, in the order played. A
  key held before arming is not part of it, and its key-up ends nothing.
- **A STEP ring's capture** takes one note a key-down, repeats included,
  across key-ups, and ends when `min(hits, 8)` are in: a melody is
  played one key at a time. Hits past the eighth cycle the notes. Each
  rest takes the note of the hit before it, and the steps before the
  first hit take the last hit's note. Only STEPS steps are written.
- **OCTAVE** becomes the lowest captured note's octave (held to 0..=8)
  in both modes. The notes stay as played (spec § 3.4: OCTAVE is the
  ring's lowest note's octave).
- **CHRD's PLAY** reads `-` with no played chord (the fonts are ASCII),
  `HOLD` while armed, and otherwise the played chord's name. Turned up,
  it arms (and drops any other target). Turned down, it disarms if armed
  there, and otherwise clears the played chord. That plain turn is
  destructive, unlike CLEAR (ADR 0074): it loses one chord, and the
  degree under it plays again.
- **LEARN** reads ON on the ring it armed. Up arms, down disarms.
- **A played chord's chip** shows its name as the one line, in the
  largest of the value, bold label and label fonts that fits clear of
  its dot, and else its root. **DEGREE** names the degree's own triad,
  not the played chord: that is what it edits, and what plays once PLAY
  clears.

## Alternatives considered
- Arm on every block the slot says armed: a stale or late slot re-arms
  and takes a second chord before the UI's disarm arrives.
- An arming counter in `OrbitShared`: it does the same as the latch,
  with a field the brief does not have.
- The UI decides the chord: the runner would report under-two-class
  captures, and they would cost a count and a re-arm.
- A STEP capture ending at the last key-up, like a pool's: one key at a
  time would end it after the first note.

## Consequences
- Re-arming the same target needs the UI to publish `Off` in between,
  which it does when it takes the capture. A disarm and re-arm within one
  frame would go unseen. A person cannot turn that fast.
- Arming takes effect from the next block's drain, about 1.3 ms late.
- The firmware drain gets the hook with its runner in Task 19.

## Sources
- docs/superpowers/specs/2026-09-30-orbit-design.md § 3.4, § 4.1, § 5.3
- .superpowers/sdd/2026-09-30-orbit/task-14-brief.md; the Task 9
  review, M3
