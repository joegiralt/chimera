# 0083. A scene keeps its edits across switches; drift restarts

- **Status:** Proposed
- **Deciders:** owner (ruling 2026-10-04, from a bug found on the unit),
  firmware

## Context
ADR 0077's interim ruling made a landed switch copy the slot into `live`,
so the unsaved edits and the drift of the scene left were dropped, and
left the question open for the owner. On the unit: scene 1 playing, KEY
and SCALE turned, a switch to scene 2 and back, and scene 1 came back as
it was last saved, the edit gone. The owner ruled (2026-10-04) to follow
the hardware precedent: dropping edits violates least surprise.

The precedent says two things:
- **User edits persist per pattern in the working project** across
  switches, until an explicit reload. On the Digitakt a pattern edited
  and left keeps its changes in the active project; FUNC + NO reloads it
  from its saved state, SAVE TO PROJ makes it the saved state. The
  Octatrack keeps a part's edits until RELOAD PART, and SAVE PART makes
  them the reload point. The Cirklon marks a pattern unsaved and keeps
  it so until "lose changes and restore".
- **Machine-made variation is playback only.** On Elektron, probability
  and conditional trigs never rewrite the stored pattern: a pattern
  restarts from its stored data. Writing generated variation in is a
  deliberate act, like the Hapax's lock of an algorithm's output.

## Decision
- **Each slot is its scene's working copy: the user's edits, never
  MUTATE's drift.** Every edit to `live` also goes to the playing slot
  (`scenes[current]`), through `OrbitSong::edit` (cells, MIX + B*n*'s
  MUTE, a capture taken) or `OrbitSong::keep` (a param turned through
  `Blocks`, copied as `live` has it, so OCTAVE moves the slot's own notes
  and HITS re-lays its own hits). While a switch waits, edits still go
  to the playing scene. `absorb` writes the drift into `live` alone.
- **Leaving a scene keeps its edits.** `land(s)` still copies the slot
  into `live`: the scene comes back with its edits, its drift restarted
  from them.
- **SAVE locks the drift in.** SCN's SAVE, SETTINGS › ORBIT › SAVE TO
  PROJ and SAVE OVER write `live`, drift included, over a slot, as
  before. That is the deliberate act.
- **RELOAD FROM PROJ restarts the scene from its slot** (option (i)):
  `live` becomes the slot again on the loop, its edits kept and its drift
  dropped. Its prompt says so: RELOAD SCENE n? / ITS DRIFT RESTARTS, ITS
  EDITS STAY. It is dimmed with no drift (`edited()`: `live` differs from
  the slot). No card path reads one scene back from the project file,
  and none is built for this.
- **The witness stays sound.** An edit into a slot moves its rev and
  keeps `saved`, like any write, so a `Confirmed<SceneSlot>` from before
  it refuses (`SCENE CHANGED`). A slot holds no `touched` marks: they are
  `live`'s to send.
- **A project save carries every slot's edits**: slots 0–7 and `live`
  are written as before (§ 8), and the slots now hold the edits.
- **The command channel (ADR 0082) is unchanged.** The UI owns `live` and
  the slots, the runner `working`; the epoch and ack gates stand. A
  switch's `Cue` adopts `next`, the slot as the latest publish carries
  it, edits included. One window is new: landing the *playing* slot
  again (RELOAD, CLEAR), an edit after the runner adopted the slot but
  before the UI lands it is in the slot and `live` and not in `working`.
  Such a landing marks every ring touched, so the next flush sends their
  rhythm and notes once the runner is synced again.

## Alternatives considered
- **Keep 0077's drop.** The owner's bug: rejected by the ruling.
- **Keep drift in the slot too** (the slot always equals `live` when
  left). Machine variation would rewrite the stored scene, against the
  Elektron rule; the scene could never restart from what the user made.
- **Mirror at the frame's end, copying `live` into the slot but for the
  drift fields.** A ring's notes, pool and hits are both drift and edit
  targets: copying them carries the drift in, skipping them loses note
  edits. Replaying each edit on the slot keeps the two apart.
- **RELOAD from the card** (option (ii)): no single-scene read path
  exists; building card I/O for it was out of scope.

## Consequences
- A note edit (NOTE, CLEAR, a capture) on a drifting ring is replayed on
  the slot's own notes: chip *i* becomes the note in both, the rest of
  the slot's notes stay the base.
- Edits now move the playing slot's rev, and turn a neutral slot into a
  saved one (`saved`).
- `edited()` now means drift: RELOAD is dimmed after an edit with no
  drift since.
- An edit costs a second write on the UI thread; nothing on the audio
  thread, no heap.

## Sources
- Elektron Digitakt manual, OS 1.51, p. 35: FUNC + YES / FUNC + NO, SAVE
  TO PROJ, reload pattern.
- Elektron Octatrack MKII manual, p. 53: SAVE PART, RELOAD PART.
- Sequentix Cirklon manual 1.20, "Pattern 'Saved' Status".
- Squarp Hapax manual: algorithmic effects and their lock.
- ADR 0077 (amended here), ADR 0082 (the command channel).
- `docs/superpowers/specs/2026-09-30-orbit-design.md` § 4.1, § 7.1, § 8.
