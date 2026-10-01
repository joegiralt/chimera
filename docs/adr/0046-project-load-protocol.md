# 0046. Swap projects by epoch: fade the voices, then publish

- **Status:** Proposed (accepted at the navigation plan's ship flash)
- **Deciders:** owner (projects and storage spec, 2026-09-28: § Loading
  while playing, § Boot; projects-core plan, 2026-09-30), firmware

## Context
A project load replaces every Part's Sound, every mix and the FX at once.
The audio must not hear that as a step: a held note whose Sound keeps its
engine snaps onto the new params mid-note, and a Part's mix and FX jump.
Only a voice whose engine or model changes fades (`Voice::render`'s
switch fade), and then restarts its held note on the new Sound. The UI loop is blocked for the card read (tens of ms) while the
audio keeps playing its last `AudioShared` snapshot (ADR 0021).

ADR 0021's triple buffer is latest-wins and one way (UI to audio). It
can't tell the audio to fade **before** the new snapshot exists, and it
can't tell the UI the fade is done.

A `Project` is about 34 KB, and there is no RAM for a staging copy beside
the UI's own (projects spec § RAM). ADR 0045's loader stages pass 2 and
commits it at its end.

## Decision
- **Two counters beside the triple buffer, not in it:** `LoadLink {
  epoch, ack }` in `chimera-core/src/project/swap.rs`, one static
  `LOAD_LINK` (the spec's `LOAD_EPOCH` and `LOAD_ACK`). The request must
  reach the audio while it still plays the old snapshot, and the ack goes
  the other way, so neither can ride the snapshot. Writes are Release and
  reads Acquire; only the counters ride them, and the snapshot keeps the
  triple buffer's own ordering.
- **Only `load_project` bumps the epoch** (`project/store.rs`;
  `LoadLink::bump` is `pub(in crate::project)`, and it returns a
  `#[must_use] Swap`), for a file loaded, a fallback to NEW, and `+ NEW`.
  No `Project` method writes it, so `Project` holds no statics.
- **Every snapshot carries the epoch it was published under**
  (`AudioShared.epoch`; `update_from(perf, LOAD_LINK.epoch())` in both
  shells). The gate reopens on the snapshot that carries the epoch it
  waits for.
- **The protocol:**
  1. validate (on error, RAM is untouched);
  2. parse into the UI's project, without publishing;
  3. bump the epoch;
  4. the audio kills every voice and acks once they are quiet;
  5. the UI waits for the ack (`Swap::settle`, at most
     `LOAD_ACK_TIMEOUT_MS` = 10 ms), then publishes.
- **The gate** (`LoadGate`, pure `step(requested, snapshot, quiet)`; the
  shells call `before_block` once per 64-sample block, before that
  block's drain, and drain only when it says so):

  | Phase | Input | Output, next phase |
  |---|---|---|
  | any | `requested != seen` | `seen = requested` and `kill`. Then `Open` and `drain` if `snapshot == seen` (published already), else `Fading`. |
  | `Open` | — | `drain` |
  | `Fading` | `snapshot == seen` | `drain`, `Open` |
  | `Fading` | `quiet` | `ack: Some(seen)`, `Waiting` |
  | `Fading` | else | nothing |
  | `Waiting` | `snapshot == seen` | `drain`, `Open` |
  | `Waiting` | else | nothing |

  An epoch that changes in any phase restarts the kill, so a second load
  during a fade is acked for the second epoch only, and the first one's
  ack is never owed. The gate is shut only while the snapshot is older
  than the epoch, so the synth is never left muted: the UI always
  publishes after `settle`, acked or not.
- **The kill** (`Instrument::kill_all`): every booked slot is marked
  dying (`Allocator::kill_all`), every active voice fades over
  `Voice::FADE` (128 samples, two blocks) on the snapshot it plays, so
  through the old Sounds, mix and FX, and every waiting note goes. A
  dying slot is never re-struck, retriggered by a Mono Part, shed or
  stolen as a sounding voice, and `render` frees it when its fade ends,
  held or not. A note-on that finds no free voice may still take the
  dying slot nearest the end of its fade (`Allocator::pick`, ADR 0027):
  the note waits for the fade and starts fresh.
  `Voice::kill` drops a queued note and doesn't restart a running fade.
  Nothing is counted as refused. A released Modal string that rings free
  (ADR 0062) fades with the rest, and a STEAL = GLIDE Part (ADR 0065)
  never glides from a killed note: its next note starts fresh.
- **A late ack:** the UI publishes at the timeout. The gate sees the
  epoch in the snapshot and reopens at once, so the rest of the fade
  goes through the **new** snapshot's mix and FX (LEVEL eased, PAN and
  sends eased, OUT switched at once) and its mod routing. A fading voice
  keeps the Sound it last played (`Voice::render` reads `played` while
  it fades), so the old notes keep their own Sound for up to two
  blocks. The fade still ends them; nothing is held.
- **MIDI during a load:**

  | When | What happens |
  |---|---|
  | During the card read (steps 1–2) | Notes play on the old project, and are faded at step 4. |
  | Between the epoch and the publish | The audio doesn't drain the note queues: note-ons wait and play on the new project, in queue order, so a note-off still follows its note-on. |
  | A note-off for a killed voice | Acked in time: ignored, its slot is already freed. After a timed-out publish, one drained mid-fade reaches the fading voice and changes nothing audible; the fade ends it. |
  | A key held through the swap | Not retriggered; silent until played again. |
  | Pitch bend, mod wheel and other controllers | Dropped at the note queue today (`note_queue.rs`), so there is nothing to keep. |
  | MIDI clock and thru | Not implemented. |

  A queue that fills while the gate is shut drops and counts each event
  past it, as it always does: 64 events per source, in a gap of at most
  the fade plus the 10 ms timeout.
- **Boot doesn't bump.** Nothing sounds yet, so there is nothing to
  fade. `boot_project` loads with `load_ab_in_place` (falling back to NEW
  as `load_project` does) but never bumps: the epoch stays 0, the first
  snapshot (`AudioShared::init_in_place`) carries 0, and the gate starts
  at 0, so its first step drains. A boot that bumped would need its first
  snapshot tagged with the new epoch, or the gate would hold every note
  until the UI's first publish.
- **A project load applies in place.** This supersedes in part ADR 0045's
  "Two passes, staged", **for projects only**. Pass 1 checks the CRC as
  before; pass 2 writes straight into the UI's project, because a 34 KB
  staging copy doesn't fit. `load_ab_in_place` says whether pass 2 had
  started when an error came: if pass 1 failed, RAM is as it was; if
  pass 2 failed (the card changed or was pulled between the passes), the
  project is partly overwritten, so the loader resets it to NEW, shows
  "LOAD FAILED: <file>", and still bumps, settles and publishes. The
  replace guard (ADR 0043) has already accepted losing the old state.
  Sounds and SYSTEM stay staged.
- **The project file's records** are ADR 0043's table (`0x8007` Slot,
  `0x8008` Part, `0x8009` Fx, `0x000A` Origin, their payloads and context
  rules). A new Origin shape takes a new tag; `0x000A` stays 2 bytes.

## FX at the swap

## Alternatives considered
- **The epoch inside `AudioShared`.** The audio would learn of the load
  only from the new snapshot, so the fade would run on the new Sounds.
- **Publish at once and let each voice's engine-switch fade cover it.**
  Only voices whose engine or model changes fade; the rest would step to
  their new params mid-note, and a Part's mix and FX would jump.
- **Wait for the ack without a timeout.** A stalled callback would hang
  the UI; the timeout costs at most a fade on the new snapshot.
- **Stage the project and swap it in.** Twice the 34 KB in AXI SRAM,
  which has no room for it.
- **Retrigger held keys on the new project.** The spec rules it out: a
  held key belongs to the old Sound, and a new note is the player's.

## Consequences
- A load is silent for at most a block or two between the fade and the
  first note on the new project; FX tails ring through it.
- The audio pays one atomic load, one compare and `Instrument::quiet`
  (eight flags) per block; nothing allocates or blocks.
- Every caller of `update_from` passes an epoch. A shell that forgot the
  gate would drain through a load and play notes on the old project.
- On the desktop, one cpal callback (often 10–20 ms) renders several
  blocks; the gate and the drain step per block inside it, as on the
  chip, so the first callback after a bump kills, fades and acks (its
  third block). That callback can still start more than 10 ms after
  the bump when the period is long: the UI then times out, and the
  fade's tail goes through the new mix, as above. The snapshot is read
  once per callback, so a publish reopens the gate at the next one.
- The in-place load makes a failed pass 2 cost the project (it becomes
  NEW); ADR 0045's staged loader stays for Sounds and SYSTEM.
- A project file is about 44 KB. A load reads about 130 KB (pass 1 on
  both sides, then pass 2 on one); a save reads about 88 KB (pass 1 on
  both sides, so it never writes the side a load would take) and writes
  about 44 KB.

## Sources
- Spec: `docs/superpowers/specs/2026-09-28-projects-storage-design.md`
  (§ Loading while playing, § Boot, § RAM).
- Plan: `docs/superpowers/plans/2026-09-30-projects-core.md` (§ Global
  Constraints: Loading while playing; § Decisions: a project load applies
  in place, the epoch is written by the project load functions only;
  Review Focus 1 and 5).
- Code: `chimera-core/src/project/swap.rs` (`LoadLink`, `Swap`,
  `LoadGate`), `chimera-core/src/project/store.rs` (`load_project`),
  `chimera-core/src/storage/file.rs` (`load_ab_in_place`),
  `chimera-core/src/instrument.rs` (`AudioShared.epoch`,
  `Instrument::kill_all`, `quiet`), `chimera-core/src/voice_alloc.rs`
  (`Allocator::kill_all`), `chimera-stm32/src/audio/engine.rs`,
  `chimera-desktop/src/audio.rs`.
- Tests: `chimera-core/tests/load_protocol_test.rs`,
  `chimera-core/tests/project_store_test.rs`,
  `chimera-fat/tests/power_cut_test.rs` (`project_cut_keeps_a_generation`).
- ADRs 0021 (triple buffer), 0027 (shedding), 0043 (projects, record
  table), 0045 (card format, superseded in part), 0062 (Modal rings
  free), 0065 (steal cut or glide).
