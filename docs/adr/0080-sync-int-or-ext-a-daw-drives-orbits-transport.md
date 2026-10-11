# 0080. SYNC picks ORBIT's master: INT runs free, EXT lets a DAW drive the transport

- **Status:** Proposed
- **Deciders:** owner (2026-10-03: "A DAW should be able to start and stop
  the sequencer, or run freely"), firmware

## Context
ADR 0072 made ORBIT follow an incoming MIDI clock, but kept two transports:
B6's `transport` and the DAW's `ext_playing`, both needed to sound. A DAW's
Stop left ORBIT waiting on B6 *and* a Start, so after a Stop the DAW's own
Start played nothing until someone pressed B6. There was also no way to
ignore a clock: a DAW sending clock for something else moved ORBIT too.
SETTINGS › MIDI CONFIG › SYNC was a `Later` row (#264) for this.

## Decision
- **SYNC has two values, INT and EXT** (`SyncMode`), on its own leaf under
  MIDI CONFIG (`SYS_SYNC`, block `BlockRef::Midi`, `MidiSettings`).
- **INT: ORBIT's own clock, always.** `OrbitRunner::realtime` drops Clock,
  Start, Continue and Stop. Notes still play.
- **EXT: the DAW drives the transport.** Start goes to bar 0 and plays, with
  no B6, also right after a Stop. Stop stops. Continue plays on from where
  ORBIT is (`MidiClock::rebase` to the edge), which is where Stop left it
  unless B6 played ORBIT since. ORBIT follows the leading clock (ADR 0072).
  After 500 ms with no tick the internal clock takes over from the edge,
  and the transport stays where the DAW last left it.
- **One transport, last word wins.** `ext_playing` is gone. The runner's
  `transport` is set by B6, by the DAW's Start and Continue, and cleared by
  its Stop or B6. A clock alone never starts ORBIT, it only moves it.
- **A stopped DAW leads nothing.** `MidiClock::position` is `None` while its
  transport is stopped, so B6 under a stopped DAW plays ORBIT on its own
  clock from bar 0. A clock still sent while stopped keeps the leader and
  its tempo average, as before.
- **B6 is a press, not a level.** `OrbitShared.presses` is a wrapping `u8`
  bumped on every press (`OrbitUi::play_stop`), compared as `arming` and
  `scene_rev` are, and the runner toggles its transport once per press: an
  odd count since its last read flips it. With two writers, a level would
  need the UI to mirror what the DAW did, and a press in the frame before
  the mirror is published is lost. A count can't be lost or alias when
  several presses land between two runner reads, and the UI's "the runner
  stopped itself" rules go. This amends ADR 0071's edge rule: every edge is
  a press.
- **B6 toggles the transport as it was before the DAW's word.** With B6
  and a Start, Continue or Stop in one block, B6 flips the transport the
  block began with and the DAW's message only resets the position. B6 and
  a Stop together stop, never restart.
- **Switching SYNC forgets the other mode's clock.** The block that sees a
  new `OrbitShared.sync` resets `MidiClock` and drops any Start, Continue or
  Stop not yet acted on. The transport is untouched. INT takes over at the
  internal BPM from the edge. EXT takes the next tick up as a clock with no
  Start, from the edge. Nothing jumps back or stops.
- **SYNC lives in SYSTEM, not the project.** It says how the unit is wired
  into a studio, which stays the same whichever project is loaded. A
  project carried to a friend's unit, or loaded with the DAW off, must not
  bring a SYNC that fights the room. It saves as a second `Block` record
  (`Block(Midi)`, code 33, ident `MIDI`; param 0 `SYNC`, codes INT 0 and
  EXT 1), frozen in `disk_codes_v1.txt`. A SYSTEM file from before it has
  no such record and reads as the default. A SYNC change marks SYSTEM's
  defaults touched, as a theme change does (ADR 0045).
- **EXT is the default.** A DAW works out of the box, and with no clock
  arriving ORBIT still runs free on B6, so EXT costs a player with no DAW
  nothing. INT is for a rig whose clock is meant for something else.
- **Without ORBIT, SYNC stays `Later(#264)`.** There is nothing for it to
  drive. `MidiSettings` is still held and saved, so SYSTEM's records don't
  depend on the feature.

## Alternatives considered
- **SYNC in the project.** It would travel with a song, but it describes
  the studio, not the song. Loading a friend's project would silently stop
  a DAW from driving ORBIT, or the reverse. Rejected.
- **Keep `ext_playing` and AND it with B6.** That is the stuck state the
  owner hit: a DAW's Start after its Stop needed B6 too. Rejected.
- **A third mode, AUTO.** It would follow a clock when one leads and run
  free otherwise. EXT already does this through the 500 ms fallback, and
  one more value is one more thing to explain. Rejected.
- **A level `run` with the UI mirroring the runner.** The publish and the
  view each lag a frame. A B6 press in the frame the mirror flips is lost,
  either at a DAW Start or at a DAW Stop. Rejected for the press flip.
- **Song Position Pointer on Continue.** That is still out of scope (ADR
  0072). Continue resumes from ORBIT's own position.

## Consequences
- Sending Start from a DAW is enough to play ORBIT under EXT. A DAW that
  sends Start on every loop restarts ORBIT at bar 0 on every loop, which is
  what Start means.
- After a DAW Stop, B6 plays ORBIT on its own tempo. The DAW's Continue then
  takes it over from where it is.
- A SYSTEM written by this firmware carries one more record. An older file
  is only rewritten when something in it changes (`body_crc` is taken of
  what was decoded), and the v1 SYSTEM fixture still loads, as EXT.
- A project load drops a DAW Start, Continue or Stop not yet acted on.
  The load is the player's newer act, so the new project waits stopped,
  and the DAW's next Start plays it, from bar 0. A DAW still playing
  through a load leaves ORBIT stopped until that Start or B6.
- `OrbitRunner` keeps its size: `sync` and `ext_play` replace `ext_playing`
  and `ext_stop`, still under its 1 KB assert.

## Sources
- Owner's decision, 2026-10-03 (this ADR's Context).
- ADR 0045 (SYSTEM), 0071 (transport edges, amended), 0072 (MIDI clock,
  amended).
- `chimera-core/src/orbit/runner.rs` (`realtime`, `advance`),
  `orbit/clock.rs` (`SyncMode`, `MidiClock::position`),
  `ui/midi_settings.rs`, `storage/system.rs`, `ui/orbit/mod.rs`
  (`play_stop`).
- Tests: `orbit_midi_clock_test.rs` (`int_ignores_clock_and_transport`,
  `ext_start_plays_without_b6_after_a_stop`, `ext_continue_resumes`,
  `switching_sync_while_playing`, `b6_and_daw_stop_in_one_block_stop`,
  `b6_and_daw_start_in_one_block_toggle_the_transport_before`,
  `a_load_drops_a_pending_daw_start`, `int_plays_live_notes_under_a_flood`),
  `orbit_blocks_test.rs` (`presses_between_runner_reads_all_count`),
  `midi_sync_page_test.rs`,
  `system_file_test.rs` (`system_midi_fixture_round_trips`, frozen
  `fixtures/v1/system_midi.sys`).
