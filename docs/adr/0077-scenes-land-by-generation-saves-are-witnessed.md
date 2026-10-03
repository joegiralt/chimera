# 0077. A scene switch lands by its generation; SAVE OVER is witnessed when it asks

- **Status:** Proposed
- **Deciders:** implementer (plan Task 16), for the owner's UAT

## Context
Spec § 4.1 and § 6.3 keep one working scene in the project, `live`,
beside eight saved slots. Turning SCN's SCENE queues a slot; the runner
adopts it at the playing scene's next loop boundary (at once when
stopped), and reports back through `OrbitView` (ADR 0021), frames
behind. The UI must learn that the switch landed so the song's playing
scene moves, and must not take a stale view for a landing: re-queueing
the playing slot leaves `playing` unchanged, so the index alone can't
tell. The owner asked (Q5, 2026-10-02) that SCN's SAVE over another
saved scene asks first, and plan ruling R7 that a prompt left open over
a slot written meanwhile can't overwrite it.

## Decision
- **A switch is a generation.** `OrbitUi::queue` copies the slot into
  `live`, sets `queued`, and bumps `scene_rev`; every publish writes
  `scene_rev` and `scene` (the queued slot, else the playing one). The
  runner adopts a rev once, at the boundary, and reports `playing`, the
  `scene_rev` it adopted, and `queued` while one waits. The UI lands the
  switch (`current` takes it, `queued` clears) only on a view of this
  load whose `scene_rev` is the UI's own. A stale triple-buffer slot
  repeats an old rev and lands nothing.
- **Drift is absorbed only from the playing scene with nothing queued**
  (`absorb` checks `view.playing == current` and `queued`).
- **A queued switch replaces `live` at the turn.** Edits made while it
  waits go to the queued scene and play from the boundary; unsaved edits
  and drift of the scene left behind are dropped, as the spec's one
  working scene implies. Turning SCENE back queues a copy of the slot.
- **Saves.** SEQ on any ORBIT page saves `live` over the playing slot at
  once (`save_scene`). EDIT on SCN saves to SAVE's slot (`save_to`): at
  once over the playing or a neutral slot, else it opens SAVE OVER
  SCENE n? / SAVE OVER · CANCEL. Both are refused while a switch is
  queued (`SWITCH PENDING`): `live` is then the queued scene's.
- **The witness is read when the prompt opens.** Every write to slot n
  moves `slot_revs[n]` (run-time only, never saved, ignored by `==`).
  The prompt holds a `SceneAsk`, the slot and its rev when it opened,
  which only `OrbitSong::save_to` makes. `Confirmed::<SceneSlot>::said`
  takes it with the confirming pill's `Said<SceneSlot>`;
  `overwrite_scene` refuses a slot whose rev moved (`SCENE CHANGED`).

## Alternatives considered
- **Land on `playing == queued`.** A stale view with the same index
  lands a switch the runner hasn't seen; rejected.
- **`said(slot, &OrbitSong, Said)` reading the rev at the answer**, as
  R7's sketch wrote it. The witness then always holds, and a slot
  written while the prompt is open is overwritten anyway; rejected for
  the open-time `SceneAsk`.
- **A second working scene for the queued one**, keeping the old
  scene's unsaved edits to return to. The spec's runner and project hold
  one; it would cost a scene in both and change § 4.1.

## Consequences
- Turning SCENE away and back drops unsaved edits and drift of the
  scene left; SEQ first keeps them. A UAT question.
- `OrbitSong` holds nine scenes (3,552 B in the project, AXI).
- Task 18 encodes `scenes`, `live`, `current` (the queued index while
  one waits) and each scene's seed; never `slot_revs`.

## Sources
- `docs/superpowers/specs/2026-09-30-orbit-design.md` § 4.1, § 6.3, § 7.1
- `docs/superpowers/plans/2026-09-30-orbit.md` Task 16; ruling R7
- ADR 0021 (triple buffers), ADR 0044 (ORBIT's keys), ADR 0066 (prompts)
