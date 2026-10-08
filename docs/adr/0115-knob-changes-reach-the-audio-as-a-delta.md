# 0115. Knob changes reach the audio as a delta the UI diffs

- **Status:** Accepted (owner, 2026-10-08)
- **Deciders:** owner (direction 2026-10-08: "a Redux/Elm-style state diff instead of stamps and full reads"; ruling 2026-10-08: the 8-knob read budget), firmware
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
4. **Every Part's reads share one block budget** (`Knobs::READ_BUDGET`, 8 knobs a block for the whole Instrument).
   - Order each block: a playing Part's edits, then a playing Part's new Sound, then the silent Parts'. Within a Part, edits go before what lands. The Part served first turns each block.
   - What doesn't fit waits (`pending`):
     - a waiting edit plays where it was, then glides when read, so it never snaps;
     - a new Sound's waiting knobs play their stored values (a load lands anyway).
   - Bounds:
     - a knob turn is read the block it arrives;
     - a Sound loaded into a playing Part lands within 9 blocks (~12 ms), five at once within 42;
     - a project load's six Sounds within 50, while silent.
   - A knob not yet read since its Sound arrived has no old value to glide from, so an edit to it in that window lands. The window is ≤ 9 blocks after a load into a playing Part; no hand turns a knob that soon after a load.
   - A value written past the publisher is read whole at once, past the budget: it names no knob. Never on the unit.
5. **A silent Part keeps current.** It takes its deltas while silent and lands them within the budget. What it leaves waiting still lands when read, even once a voice plays it. A Part that starts playing on an unchanged Sound reads nothing.
6. **What is billed:**
   - `Knobs::COST`, 6 cycles/sample a sounding Part, on its first voice: the delta's check (~40 cycles a block) and two knobs gliding (~300), 340 / 64.
   - `Knobs::READS`, 12, reserved once beside the bus (`Instrument::reserved` and `recost`'s shed): the budget's 8 reads at ~91 cycles each (a whole Sound's ~6,000 on the unit, from the STRESS analysis, / 66), 728 / 64.
   - The owner's ruling (2026-10-08): 8 knobs a block. Why not a whole Sound (94):
     - ADR 0040's voices on rev V leave 84 cycles/sample beside eight INIT voices and the bus, and 20 beside the costliest patch's six and the master tape.
     - 6 + 12 fits both; 6 + 94 would cost a voice in either.
7. **The one input to a knob's motion that isn't a knob:** whether its Sound moves it itself (`ParamSnapshot::moves_itself`: SCRUB's POSITION, by `tape.model`). The publisher tracks it per Part and names the knob when it flips.
8. **Stamps are serials** from one `AtomicU32` and never 0 (`NONE`). A wrap takes 2³² writes. Past it, a Sound left unwritten through all of them could meet its own serial again and a change go unseen; at a write a frame, that is over two years. A u64 would cost ~200 B of AXI in the buffer slots, against the bench's 476 B, and needs 64-bit atomics the M7 lacks.

## Alternatives considered
- **Stamp per Part, full read on change (the first cut of this branch):** an edit cost one full read, not six. But a knob turn still read 66 knobs, and the audio still found the change itself.
- **A per-publish delta with no ring:** the triple buffer hands the audio only the latest publish, so a publish it skipped would lose its changes. An ack from the audio would also work, but it couples the threads.
- **Per-knob publish counters:** 264 B a Part in every buffer slot, against a 64 B ring.
- **The audio diffing a copy of the last Sound:** that is the work this ADR moves off the audio thread.
- **Billing the spike instead (raising BURST):** it refuses voices sooner for work that need not be done.

## Consequences
- **Audio, worst block:**
  - Knob reads are bounded by construction: 8 a block (~730 cycles), billed as `READS`.
  - Plus each sounding Part's check and glides, billed as `COST`.
  - Before: ~30–36k on any edit with six Parts playing, ~16k at ORBIT's start, and ~6k a playing Part's load (six at once ~36k), none of it billed.
  - A knob turn: one read (~90 cycles) and its glide. ORBIT start: none.
  - Estimated from instruction counts; re-bench STRESS with a Sound load in its timed loop.
- **UI:**
  - A publish: one stamp compare a Part.
  - An edited Part: 66 value compares and 16 route compares, ~6k cycles on the M7 (~13 µs at 480 MHz), once per publish that carries an edit. On the UI's time, under 0.1 % of a 16 ms frame.
- **RAM:**
  - `KnobPublisher` 2,304 B, UI side. On the board it sits in D2 (`hw::D2_KNOBS`, taken from `VOICE_RAM_BUDGET`, so `memory_budget_test` and `sd_probe` count it).
  - D2 as linked (shipping): 4,096 DMA + 169,552 voices and publisher, 121,264 B free; `memory_budget_test` counts 116,336 with the full 8 KB DMA reserve.
  - `KnobDelta` ~72 B a Part in each of the three `AudioShared` slots (~1.3 KB of AXI).
  - Shipping vs main: AXI −1,056 B (41,212 B free), D2 +2,448 B, flash +5,472 B.
- **Bench AXI headroom:**
  - The tightest bench build, `bench-algo,master-tape`, keeps 8,668 B against its 8,192 B floor (ADR 0087), 476 B over; `bench-orbit` and `bench-stress,orbit` keep 9,388.
  - What gives next: `DELTA_DEPTH` 4 → 2 in the bench builds (−576 B over the three slots), then the deltas out of `AudioShared` into a fourth, writer-owned copy.
- A test or bench writing `AudioShared` in place still works: a new stamp means read whole.
- **Tests:**
  - `knob_reads_test` counts knob reads per block:
    - six Parts playing: a turn reads 1 (two knobs, 2); FX, mix and ORBIT edits read 0;
    - ORBIT starting three Parts: 0;
    - a load into a playing Part, and five at once with a turn elsewhere first, stay within the budget and land;
    - publishes the audio skips (1 to 9, past the ring) lose nothing;
    - a Part back from silence mid-glide plays the stored values bit for bit;
    - the read budget is reserved once.
  - The unit tests pin the window at four, the order (edits before landing), the waiting edit's hold-then-glide, a part-read Sound reading every knob once, and the model flip.

## Sources
- (private notes) (on-unit STRESS, 2026-10-08); ADR 0112, 0107, 0021 (the triple buffer).
- `chimera-engine/src/dsp/knobs.rs` (`KnobSet`, `KnobDelta`, `KnobPublisher`, `Knobs::ease`), `chimera-engine/src/instrument.rs` (`fill`, `ease_knobs`), `chimera-core/tests/knob_reads_test.rs`.
