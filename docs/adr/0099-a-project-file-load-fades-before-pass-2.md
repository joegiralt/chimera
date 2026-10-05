# 0099. A project file load fades before pass 2 and reads its sources inside the fade (supersedes in part 0046)

- **Status:** Proposed
- **Deciders:** owner (sound-pool-budget spec, 2026-10-04: § Loading, § Slow card; Task 9 review rulings, 2026-10-05), firmware

## Context
ADR 0046 parses a project in place (pass 2) and only then bumps the load
epoch. Its step 6 allows no card access between the bump and the publish,
and its MIDI table bounds the time the note queues wait at the fade plus
10 ms.

ADR 0087 adds sources held in the arena and read from `/CHIMERA/TAPES`.
Pass 1 plans a file against the whole arena, which assumes the old
project's sources are gone before pass 2 stores anything. Under 0046's
order they aren't. Pass 2 retires them while their voices still play, so a
retired source that is still sounding stays pending, the slots its room
was planned for don't fit, and the load comes in lossy (Task 9 review,
H1). The spec has pass 2 run after the epoch fade and the sources read
inside it.

ADR 0046 is listed as Proposed in the README (its file says it is accepted
at the navigation ship flash). This ADR supersedes it in part all the same,
and leaves its status alone.

## Decision
- **For a project file load, this order replaces 0046's steps 1–6:**
  1. Pass 1 on both sides, while the old project plays. An error leaves
     RAM untouched.
  2. Pick a side, then list `/CHIMERA/TAPES` once (names and sizes, no
     data). A ref is present when its name matches and its size is at least
     `source_file_min_len`. Then plan the picked side: only present refs
     take room, and the note counts the rest (`LOADED · n SRC MISSING`).
  3. Bump the epoch (`ProjectDecoder::begin`).
  4. Wait at most `LOAD_DRAIN_TIMEOUT_MS` = 50 ms, on the UI's gate clock,
     for the gate's ack of that epoch (`LoadLink::acked`) and then
     `SourceTable::sounding() == 0`.
     - The ack means a block began after the kill and every voice is quiet,
       so `sounding` is no stale block's.
     - Fifty ms is two or three blocks on the chip, and one 10–20 ms
       desktop callback.
     - If either is missing, the load is refused with `Busy` (`AUDIO BUSY,
       NOT LOADED`). The project is untouched, and the UI still settles and
       publishes. It skips the page reset and the SYSTEM write
       (`project_replaced`, `remember`).
  5. Pass 2 in place. Its header retires every source, then waits within
     the same 50 ms for each hole to close and each slot to free
     (`Pool::sources_drain_timed`). A failure there, or anything after it,
     is clobbered: NEW, `LOAD FAILED`, publish. It is never lossy.
  6. The source reads, under the shut gate.
  7. `settle` (normally acked long before) and the publish.
  8. Then the SYSTEM write.
- **Card access between the bump and the publish** is allowed for project
  file loads only: pass 2 and the source reads, nothing else. NEW and
  `+ NEW` keep 0046 unchanged (bump, settle, publish, no card). Boot still
  doesn't bump: nothing sounds, and `begin` waits for nothing.
- **MIDI during a load** (replaces 0046's table for file loads, and its
  bound on the gap):

  | When | What happens |
  |---|---|
  | Pass 1 and the listing | Notes play on the old project. |
  | The bump to the publish | The note queues and ORBIT wait: no drain, no `orbit.block`. Note-ons play on the new project after the publish, in queue order. |

- **How long the queues wait, as estimated:**
  - the drain: under a block;
  - pass 2: up to about 44 KB at 1 333 KB/s (ADR 0045), about 33 ms plus
    decoding;
  - the source reads: up to about 154 KB, about 116 ms (spec § Slow card),
    plus a directory lookup per source (a linear scan: 5–12 ms each with a
    few hundred files, for up to 32 sources).
  - Typically that is 150–200 ms, and about 0.5 s with a large TAPES
    directory. A slow card stretches it, up to `SD_OP_CAP_MS` (10 s) per
    operation. Today no card holds sources, so the gap is about 35–45 ms.
- **Overflow, and the mitigation.**
  - A DIN queue (64 events) fills in 41 ms at DIN's top rate (about 1 560
    events/s with running status). At 0.5 s, about 130 events/s fills it,
    which is ordinary dense sequencing.
  - A dropped note-off is a stuck note.
  - So every `NoteQueue` producer (DIN, the desktop's MIDI and keys, the
    bench) refuses a note-on when `NOTE_OFF_RESERVE` = 8 or fewer slots
    are free. Note-offs and real-time still fit. A note sticks only if more
    than 8 note-offs are pending at once while the queue is full.
  - The note-ons refused are counted as drops, as before.

## Alternatives considered
- **Bump after pass 2, as 0046 does.** A source still sounding is retired
  pending, so the load is lossy (the H1 bug).
- **Refuse while any source sounds, without a kill.** Every load during a
  TAPE note would fail.
- **Read the sources after the publish**, so the hold falls back to about
  pass 2's length (45 ms or less). `publish` sets `held` with `Release`
  only after `source_fill`, as an add does while playing, so it would be
  safe. It amends spec § Loading, and the owner chose the reserve instead.
- **No reserve.** A long hold drops note-offs and leaves notes stuck.

## Consequences
- The late-ack case of 0046 goes away for file loads: the ack arrives
  during pass 2.
- The UI blocks for up to 100 ms of extra spinning: 50 ms at `begin`, and
  50 ms in the drain.
- A `Busy` refusal still costs what the bump did: every voice is killed,
  and ORBIT's runner sees the epoch, so it stops the transport and drops
  a capture. Only a stalled audio, or a desktop callback slower than
  50 ms, reaches it.
- A stall after `begin` loses the old project (NEW), and the error isn't
  named.
- TAPE voices must count in `Instrument::quiet` and in `sounding`
  (TAPE Task 10).
- With the queue full, the 9th held note-on is dropped where a 64th
  would have been before.

## Sources
- Spec: `docs/superpowers/specs/2026-10-04-sound-pool-budget-design.md`
  (§ Loading, § Slow card, § Failure modes).
- Review: `.superpowers/sdd/2026-10-04-sound-pool-budget/task-9-rereview.md`
  (F1–F4, § 3).
- Code: `chimera-core/src/project/codec.rs` (`ProjectDecoder::begin`, the
  header's drain), `chimera-core/src/project/pool.rs` (`quiet_timed`,
  `sources_drain_timed`, `LOAD_DRAIN_TIMEOUT_MS`),
  `chimera-core/src/project/store.rs` (`load_into`, `read_sources`),
  `chimera-core/src/note_queue.rs` (`NOTE_OFF_RESERVE`),
  `chimera-core/src/ui/mod.rs` (`load_with`).
- Tests: `source_pool_test` (`pass_one_counts_the_whole_arena`,
  `a_late_holder_fails_the_load`, `a_stalled_audio_refuses_untouched`),
  `load_protocol_test::a_long_hold_drops_note_ons_never_their_note_offs`,
  `note_queue_test::note_ons_leave_room_for_note_offs`,
  `project_boot_test::a_busy_refusal_is_no_load`, `source_codec_test`.
- ADRs 0045, 0046, 0087.
