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
- **Arming is a new arming generation.** `OrbitShared.arming` (`u8`)
  is bumped by the UI on every arming, the same target again included,
  and written on every publish with `capture`. A new generation arms the
  runner on the published target and drops what was under way. The
  target published `Off` under the same generation disarms it. A
  finished capture disarms the runner itself, and the same generation
  published again does not re-arm it. So a capture is taken once per
  arming, whatever the slots say after, and an arming in the very frame
  a capture is taken (the UI's take-time disarm and the new turn up
  together) still arms. The UI takes each new `Captured.count` once,
  writes it into the song (a ring's `notes_rev` bumped), and disarms. A
  load disarms too, the runner as well: its load publish still carries
  the old arming, so the runner drops the arming and the held keys.
  Each `Captured` carries the load epoch it was played on, and the UI
  takes only its own: a capture ended on the old project, while the card
  was read or by keys drained before the runner saw the load, is
  dropped.
- **SEGS shrunk under an armed segment** disarms it, and `Scene::take`
  writes no segment past SEGS.
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
  The runner counts the hits on its working copy (MUTATE's drift kept)
  and the UI fills the song's mask, so a drifted ring can disagree on
  the count: `played[k % len]` keeps the fill total, cycling or cutting
  the melody, never failing.
- **OCTAVE** becomes the lowest captured note's octave (held to 0..=8)
  in both modes. The notes stay as played (spec § 3.4: OCTAVE is the
  ring's lowest note's octave).
- **CHRD's PLAY** reads `-` with no played chord (the fonts are ASCII),
  `HOLD` while armed, and otherwise the played chord's name. Turned up,
  it arms (and drops any other target). Turned down, it disarms if armed
  there. **Clearing the played chord needs MIX + down** (controller
  ruling at review, as CLEAR's in ADR 0074), and the degree under it
  plays again. A plain down only disarms: detents spread over frames, so
  a two-detent flick to cancel a re-capture would otherwise disarm on
  one frame and clear the chord on the next.
- **LEARN** reads ON on the ring it armed. Up arms, down disarms.
- **A played chord's chip** shows its name as the one line, in the
  largest of the value, bold label and label fonts that fits the chip,
  else its root the same way (F# fits at SEGS 8). Its dot sits centred
  under the name, so no text, however narrow the chip, runs into it. **DEGREE** names the degree's own triad,
  not the played chord: that is what it edits, and what plays once PLAY
  clears.

## Alternatives considered
- Arm on every block the slot says armed: a stale or late slot re-arms
  and takes a second chord before the UI's disarm arrives.
- Arm on a change of the target alone (this ADR as first written): the
  UI's take-time disarm and a turn up in the same frame publish the same
  target, the runner never sees `Off`, and the UI shows HOLD while
  nothing captures. Replaced by the arming generation at review.
- A plain turn down clears the played chord (the brief's words): an
  overshoot loses it.
- The UI decides the chord: the runner would report under-two-class
  captures, and they would cost a count and a re-arm.
- A STEP capture ending at the last key-up, like a pool's: one key at a
  time would end it after the first note.

## Consequences
- `OrbitShared` and `OrbitUi` carry one more byte each.
- Arming reaches the runner with the next publish, then the next
  block's drain: up to a UI frame plus a block, about 17-22 ms on the
  desktop. Keys struck in that window are not captured.
- The firmware drain feeds the hook before its runner's block (Task 19).

## Sources
- docs/superpowers/specs/2026-09-30-orbit-design.md § 3.4, § 4.1, § 5.3
- .superpowers/sdd/2026-09-30-orbit/task-14-brief.md; the Task 9
  review, M3
