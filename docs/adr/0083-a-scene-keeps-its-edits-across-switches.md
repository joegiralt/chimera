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
  MUTATE's drift.** `OrbitSong.live` is private: the one way to change
  it is `OrbitSong::edit`, which runs the edit on the playing slot
  (`scenes[current]`) and then on `live`. The cells and MIX + B*n*'s
  MUTE call it; a scene param's encoder calls `OrbitSong::turn`, the same
  edit replaying the nudge or snap on each, so OCTAVE moves the slot's
  own notes and HITS re-lays its own hits. Readers take `live()`; tests
  alone reach past it (`live_mut_for_test`, behind `test-support`).
  While a switch waits, edits still go to the playing scene. `absorb`
  writes the drift into `live` alone, and never while `live` holds an
  edit not yet sent.
- **The `Blocks` path is guarded at run time, not at compile time.**
  `BlockRef::OrbitChords` and `OrbitRing` stay ordinary `BlockRef`s (the
  pages, the codec and the renderer name them), so a new writer through
  `Blocks` (a per-Part MIDI MAP aimed at an ORBIT param) still compiles.
  `UiBlocks::block_mut` refuses them: `None`, and a `debug_assert` that
  fails every debug test reaching it. A separate `BlockRef` family that
  `Blocks` could not name would make that a compile error; it touches
  the pages' bindings, the codec's block codes and the renderer, and was
  judged out of proportion for this fix.
- **A capture goes into the scene it was played in**, as an Elektron
  recording goes into the pattern it was recorded in. `Captured` carries
  the scene the runner was playing when the take ended;
  `OrbitSong::capture` writes it into that slot, and into `live` while
  that scene plays. A capture into a slot that is not yet `live` keeps
  its touched marks, so they are sent when it lands: the runner may have
  adopted the slot before the capture reached it.
- **Leaving a scene keeps its edits.** `land(s)` still copies the slot
  into `live`: the scene comes back with its edits, its drift restarted
  from them.
- **SAVE locks the drift in.** SCN's SAVE, SETTINGS › ORBIT › SAVE TO
  PROJ and SAVE OVER write `live`, drift included, over a slot, as
  before. That is the deliberate act.
- **A CLEAR or RELOAD waiting on the loop stays due** when SCENE is
  turned away and back (`reland_due`): cancelling it would leave `live`
  and its slot on different bases.
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
- **A file from before this ADR is locked in once.** It can hold `live`
  edited past its slot. At the load, where `live` differs from the
  playing slot in more than drift (each ring's hits, notes and pool), it
  is written over the slot, so the edits stay. A note edit cannot be
  told from drift there, and restarts with it. A file saved after this
  ADR differs only by drift, and is left as it is.
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
- Flash, fat-LTO release text, `--features orbit`, each on a git-archive
  copy: the merged base 44f6d786 721,136 B; 55d9ca62 (first cut, a
  `keep` copy after a `Blocks` edit) 721,904 B (+768); `live` private
  with `edit` and `turn` 721,656 B (+520). A closure built round
  `part_page::apply_under` over `Scene` measured 725,916 B and was
  dropped. The base was already over the 715,428 B gate. The review
  round (capture by scene, the reland kept due, the load's lock-in, the
  HINTS-off CLEAR label) measured 725,376 B against 724,088 B at
  43d59110 (+1,288), most of it the load's comparison of `live` with its
  slot.

## Sources
- Elektron Digitakt manual, OS 1.51, p. 35: FUNC + YES / FUNC + NO, SAVE
  TO PROJ, reload pattern.
- Elektron Octatrack MKII manual, p. 53: SAVE PART, RELOAD PART.
- Sequentix Cirklon manual 1.20, "Pattern 'Saved' Status".
- Squarp Hapax manual: algorithmic effects and their lock.
- ADR 0077 (amended here), ADR 0082 (the command channel).
- `docs/superpowers/specs/2026-09-30-orbit-design.md` § 4.1, § 7.1, § 8.
