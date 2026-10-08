# 0115. Knob changes reach the audio as a delta the UI diffs

- **Status:** Proposed
- **Deciders:** owner (direction 2026-10-08: "a Redux/Elm-style state diff instead of stamps and full reads"), firmware
- **Supersedes in part:** [0112](0112-every-parameter-declares-its-motion.md) (point 3: how the Part's knobs learn what changed). Its `Motion`, its glide, its landing rules and its bit-identity at rest stand.

## Context
- ADR 0112 stamped every Part with the Project's revision and had the audio read all 66 knobs of a Sound whenever its stamp moved. That is ~6k cycles a Part on the M7.
- Two paths made it a spike (private notes, on-unit STRESS):
  - Any edit, including an FX, ORBIT or mix edit, re-read every playing Part in one block: ~30–36k against ~4.6k billed.
  - A silent Part landed every block, so its first block with a voice re-read it. ORBIT starting three Parts read ~16k in its start block, unbilled.
- The audio was doing the UI's bookkeeping: finding what changed.

## Decision
1. **The UI diffs; the audio applies.** `KnobPublisher` (`dsp/knobs.rs`) lives on the UI side, owned by the shell next to the triple buffer's writer (`shared::take_audio` on the board, `DesktopAudio` on the desktop).
   - It keeps each Part's knob values and FREQ/Q route amounts as last published.
   - At each publish it diffs a Part only if that Part's Sound was written, and sends the result in the Part's `PartAudio` as a `KnobDelta`.
   - The delta is a numbered ring of the last `DELTA_DEPTH` (4) changed sets (`KnobSet`, a bit per knob and one for the route columns), plus the stamp of the Sound it describes.
   - A new Sound (its `loads` moved) or a first publish is every knob.
   - `update_from_project` takes `&mut KnobPublisher`: no Project reaches the audio without its delta.
2. **The audio never diffs.** Each block, each Part's `Knobs` takes the union of the sets published since the last one it took, reads exactly those knobs, and glides them (or lands them while the Part is silent). It does nothing for the rest.
   - Past the ring it takes every knob. That only happens if the UI out-publishes the audio four changes to one, which the UI's rate doesn't do, and it stays exact.
3. **A Part's Sound is a `Tracked<Sound>`** (`project::Part`, `PartEdit`). Every write to it, by any path (an edit, a load, the codec), draws a fresh stamp, and a mix or FX edit leaves it alone.
   - The publisher skips an unwritten Sound at the cost of one compare.
   - The published params carry the Sound's stamp. A value written past the publisher (the tests', the bench's) has another stamp, so the audio reads it whole once. On the unit only the publisher writes.
4. **A silent Part keeps current.** It takes its deltas while silent, lands them, and reads at most one Sound's worth of knobs a block, shared by every silent Part (`pending` holds the rest). A project load's six Sounds are spread over six blocks.
   - A Part that starts playing on an unchanged Sound reads nothing.
5. **What is billed** (`Knobs::COST`, 14 cycles/sample a sounding Part, on its first voice):
   - the delta's check (~40 cycles a block);
   - two knobs gliding (~300);
   - a Sound load's 66 reads spread over a 12-block UI frame (~500).

   An edit reads one knob; FX, mix and ORBIT edits read none.

## Alternatives considered
- **Stamp per Part, full read on change (the first cut of this branch):** an edit cost one full read, not six. But a knob turn still read 66 knobs, and the audio still found the change itself.
- **A per-publish delta with no ring:** the triple buffer hands the audio only the latest publish, so a publish it skipped would lose its changes. An ack from the audio would also work, but it couples the threads.
- **Per-knob publish counters:** 264 B a Part in every buffer slot, against a 64 B ring.
- **The audio diffing a copy of the last Sound:** that is the work this ADR moves off the audio thread.
- **Billing the spike instead (raising BURST):** it refuses voices sooner for work that need not be done.

## Consequences
- **Audio, worst block:** a Sound loaded into a sounding Part (66 reads, ~6k cycles) plus silent Parts' pending reads (≤ 66 knobs, ~6k), ~12k. Before: ~30–36k on any edit with six Parts playing.
  - A knob turn: one knob read (~90 cycles) and its glide.
  - ORBIT start: none.
  - Estimated from instruction counts, not benched.
- **UI:**
  - A publish: one stamp compare a Part.
  - An edited Part: 66 value compares and 16 route compares, ~6k cycles on the M7 (~13 µs at 480 MHz), once per publish that carries an edit. On the UI's time, under 0.1 % of a 16 ms frame.
- **RAM:**
  - `KnobPublisher` 2.2 KB, UI side. On the board it sits in D2, not AXI: the bench builds hold AXI within 8 KiB of full (ADR 0087), and `bench-algo,master-tape` keeps 8,668 B.
  - `KnobDelta` ~72 B a Part in each of the three `AudioShared` slots.
  - Shipping vs main: AXI −1,056 B (41,212 B free), D2 +2,208 B, flash +3,072 B.
- A test or bench writing `AudioShared` in place still works: a new stamp means read whole.
- `knob_reads_test` counts knob reads per block:
  - six Parts playing: a turn reads 1 (two knobs, 2), and FX, mix and ORBIT edits read 0;
  - ORBIT starting three Parts: 0;
  - a Part back from silence mid-glide plays the stored values bit for bit, reading 0.

## Sources
- (private notes) (on-unit STRESS, 2026-10-08); ADR 0112, 0107, 0021 (the triple buffer).
- `chimera-engine/src/dsp/knobs.rs` (`KnobSet`, `KnobDelta`, `KnobPublisher`, `Knobs::ease`), `chimera-engine/src/instrument.rs` (`fill`, `ease_knobs`), `chimera-core/tests/knob_reads_test.rs`.
