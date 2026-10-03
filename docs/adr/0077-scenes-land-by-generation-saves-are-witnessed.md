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
- **A switch is a generation.** `OrbitUi::queue` sets `queued` and bumps
  `scene_rev`; every publish writes `scene_rev`, `scene` and `settings`
  (the playing scene and `live`), and `queued` with `next`, the queued
  slot. The runner acts on a rev once: it adopts `next` whole at the
  boundary (at once when stopped), and reports `playing`, the
  `scene_rev` it adopted, and `queued` while one waits. The UI lands the
  switch only on a view of this load whose `scene_rev` is the UI's own;
  a stale triple-buffer slot repeats an old rev and lands nothing.
- **Interim ruling (controller, 2026-10-03, Task 16 review I1): queuing
  copies nothing into `live`; the copy happens at landing.** Until the
  boundary `live` stays the playing scene: its edits are heard (the
  runner copies knobs from `settings` while `o.scene` is the scene it
  plays), and its drift is still absorbed (`absorb` checks only
  `view.playing == current`). Turning SCENE back to the playing scene
  before the boundary cancels the switch: a new rev with nothing queued,
  which the runner takes without adopting, so nothing is lost and the
  loop runs on without a relaunch. When a switch lands, `live` becomes a
  copy of the slot: unsaved edits and drift of the scene left are
  dropped. Whether `live` edits should persist per scene is the owner's
  question; this ruling does not decide it. Supersedes this ADR's first
  text (the copy at the turn, edits to the queued scene).
- **A queued slot always lands, the playing one too** (plan Task 17,
  SETTINGS › ORBIT): CLEAR of the playing scene and RELOAD queue the
  playing slot itself under a new rev, and the runner adopts it at the
  boundary like any switch; only a rev with nothing queued is a cancel.
  Once the runner has adopted the UI's newest rev and the UI still shows
  it queued, the runner takes no knobs from `settings`, which are the old
  `live` until the view lands it.
- **The cancel race** (Task 16 review): if the runner adopts a switch to
  B at the boundary after the UI last read its view, SCENE turned back to
  A is a cancel as the UI sees it, a rev with nothing queued. The runner,
  playing B, takes that rev as a switch back to A: B plays one extra loop,
  then A lands at the next boundary. The song never leaves A, the view
  and the song agree again after that loop, and nothing is lost: `live` is
  A's throughout. Turning SCENE on to B again in that window queues B
  under a new rev; a view still reporting B under the old rev is stale and
  lands nothing, and B lands again at the next boundary.
- **The cost:** `OrbitShared` carries `next`, a second scene, so the
  runner can hear the playing scene's knobs and still adopt the queued
  one: ≈1.2 KB AXI across the triple buffer's three slots.
- **Saves.** SEQ on any ORBIT page saves `live` over the playing slot at
  once (`save_scene`). EDIT on SCN saves to SAVE's slot (`save_to`): at
  once over the playing or a neutral slot, else it opens SAVE OVER
  SCENE n? / SAVE OVER · CANCEL. Both are refused while a switch is
  queued (`SWITCH PENDING`): what lands stays what was queued.
- **The witness is read when the prompt opens.** Every write to slot n
  moves `slot_revs[n]` (run-time only, never saved, ignored by `==`).
  The prompt holds a `SceneAsk`, the slot and its rev when it opened,
  which only the crate makes (`OrbitSong::save_to`, and SETTINGS ›
  ORBIT's CLEAR and RELOAD). `Confirmed::<SceneSlot>::said`
  takes it with the confirming pill's `Said<SceneSlot>`;
  `overwrite_scene` refuses a slot whose rev moved (`SCENE CHANGED`).

## Alternatives considered
- **Land on `playing == queued`.** A stale view with the same index
  lands a switch the runner hasn't seen; rejected.
- **`said(slot, &OrbitSong, Said)` reading the rev at the answer**, as
  R7's sketch wrote it. The witness then always holds, and a slot
  written while the prompt is open is overwritten anyway; rejected for
  the open-time `SceneAsk`.
- **Copy the slot into `live` at the turn** (this ADR's first text): one
  detent of SCENE dropped unsaved edits and drift with no way back;
  replaced by the interim ruling.
- **Publish only the queued slot while it waits** (no `next`): saves the
  1.2 KB, but edits to the playing scene would go unheard and then be
  dropped at the landing.

## Consequences
- A landed switch drops unsaved edits and drift of the scene left; SEQ
  first keeps them. Turning back before the boundary loses nothing.
- Task 18: a project save while a switch waits saves the landing, so it
  must write the queued slot as `live`, not today's `live`.
- `OrbitSong.saved` caches which slots differ from neutral, kept on each
  write; Task 18's decode must set it, and give every slot a fresh rev.
- `OrbitSong` holds nine scenes (3,552 B in the project, AXI).
- Task 18 encodes `scenes`, `live`, `current` (the queued index while
  one waits) and each scene's seed; never `slot_revs`.

## Sources
- `docs/superpowers/specs/2026-09-30-orbit-design.md` § 4.1, § 6.3, § 7.1
- `docs/superpowers/plans/2026-09-30-orbit.md` Task 16; ruling R7
- ADR 0021 (triple buffers), ADR 0044 (ORBIT's keys), ADR 0066 (prompts)
