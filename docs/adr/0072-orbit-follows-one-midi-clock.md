# 0072. ORBIT follows one MIDI clock through the note queues

- **Status:** Proposed (accepted at the ORBIT plan's ship flash)
- **Deciders:** owner (ORBIT design spec § 6, 2026-09-30), firmware

## Context
ORBIT locks to an incoming MIDI clock and its Start, Continue and Stop
(spec § 6.1–6.3). Ticks come from DIN, USB or midir, are drained once a
block, and jitter by a millisecond or more. Two sources can both send
clock. The internal clock has to take over when the ticks stop, without a
jump, and the windows have to stay contiguous whatever the ticks do
(ADR 0071).

## Decision
- **Real-time rides the note queues.** `MidiParser::feed` returns `Clock`,
  `Start`, `Continue` and `Stop` (0xF8, 0xFA, 0xFB, 0xFC) wherever they fall,
  without touching running status; 0xF9, 0xFD, 0xFE and 0xFF drop. The
  queue packs an `InEvent`: a note as before (kind 0), or a `RealTime` in
  bits 18..21, so real-time keeps its order with the notes (ADR 0019).
  `drain` hands each event its `SourceId`.
- **One source leads.** `MidiClock` follows the first source to send a
  Clock or Start, and ignores the others until the leader has been silent
  500 ms (`SILENT_MS`). A new leader, or one back from silence, starts a
  fresh tempo average.
- **A tick is a place, the time between ticks is a guess.** The n-th Clock
  after Start marks (n − 1)/96 of a bar, rounded up as `step_start` rounds,
  so a step on a tick is that tick. Between ticks the phase runs at the
  average of the last 24 intervals, at least one unit past the tick (the
  tick has come, so its step fires in its block, even before an interval
  is known), and stops one unit short of the next. Clocks count only
  between Start or Continue and Stop: a clock sent while stopped keeps the
  leader alive and the average fresh, and moves nothing.
- **A drained message came half a block before its drain.** The shells see
  real-time once a block; the runner dates each message `WINDOW` samples
  before the block that drains it, the mean of when it can have come.
  Measured on a ±1 ms jittered 90–140 BPM ramp, this centres the steps
  between ticks (mean 0.1 block late, from 0.6).
- **The runner never goes back and never skips.** Each window ends at
  `clamp(position, edge, edge + advance_cap)`, under either clock, so a
  burst of ticks is caught up a step boundary per ring per window. Under a
  leading MIDI clock the edge follows it even while ORBIT is off, so B6
  joins in phase (`restart_dice`, never a new position).
- **Taken up with no Start, a clock carries on from where ORBIT is**
  (`MidiClock::rebase`): the bar and phase don't jump to the sender's tick
  count, which ORBIT can't know without Song Position Pointer.
- **Silence falls back from the edge.** After 500 ms with no tick, the
  internal clock re-anchors at the last window's edge at the internal BPM.
  The transport and `ext_playing` don't change. While ORBIT is silent the
  internal clock stays anchored at the edge, so it never jumps when it
  starts.
- **Start and Stop act at the next block.** `realtime` only flags them;
  `block` sends the held notes' offs first. Stop drops the transport with
  no edge (ADR 0071), so only a fresh B6 starts ORBIT again; Start goes back
  to bar 0 and holds there until the first Clock.
- **F is inert under MIDI.** ORB's header reads the measured BPM and `MIDI`
  (or the internal BPM and `INT`); `SlotCtx.clock` makes BPM a dimmed,
  inert cell while MIDI leads.

## Alternatives considered
- **Sample-stamped ticks.** The shells would stamp each byte as it
  arrives. The USART interrupt could, but USB and midir arrive in packets,
  and the block's ±32-sample window already bounds a note's error; a
  half-block estimate costs nothing.
- **Holding the phase until a tempo is known.** Steps between the first
  two ticks would wait for the second; the one-unit rule keeps step 0 on
  the first Clock, which § 6.1 requires.
- **Rebasing to the sender's count.** Without Song Position Pointer (out
  of scope) the count since our boot means nothing to the sender.
- **±1 block of the ideal for every step under ±1 ms jitter.** Not
  reachable: a tick 1 ms late and drained a block later is 1.75 blocks
  late, and steps between ticks inherit the next interval's jitter. The
  test holds steps on a tick to the block that drains the tick, every step
  within ±2 blocks of its ideal time, and the mean within 0.6 of a block.

## Consequences
- After a MIDI Stop, ORBIT stays silent until B6 and the external transport
  both play again: a Start or Continue, even if the cable is gone and the
  internal clock leads. `ext_playing` survives the fallback by § 6.1.
- A tempo jump (or a burst) runs ORBIT at up to `MAX_BPM_EXT` until it has
  caught up; at `MAX_BPM_EXT` itself a burst's lag is never made up.
- `OrbitRunner` grows by `MidiClock` (616 to 728 B, under its 1 KB).

## Sources
- `docs/superpowers/specs/2026-09-30-orbit-design.md` § 6.1–6.3
- `docs/superpowers/plans/2026-09-30-orbit.md` Task 12, Decisions
- `chimera-core/src/orbit/clock.rs` (`MidiClock`), `runner.rs`
  (`realtime`, `advance`), `note_queue.rs` (`InEvent`)
- `chimera-core/tests/orbit_midi_clock_test.rs`
