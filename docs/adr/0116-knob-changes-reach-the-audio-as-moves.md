# 0116. Knob changes reach the audio as moves, from and to

- **Status:** Proposed
- **Deciders:** owner (direction 2026-10-08: "finish the Redux design: the delta carries values, not just dirty bits"), firmware
- **Supersedes in part:** [0115](0115-knob-changes-reach-the-audio-as-a-delta.md): its points 2, 4 and 6 (the audio reading the knobs a delta names, the per-block read budget and the owner's 8-knob ruling, and the bill). Its UI-side diff, `Tracked` Sounds and per-Part stamps stand.

## Context
On the unit (STRESS @ bbd63cd7), with ADR 0115's read plan, a knob read still cost ~540–610 cycles on the M7 (KNOBS lines). Eight reads a block came to ~4.5k, which doesn't fit ADR 0040's slack of 20 cycles a sample beside the costliest six voices and the master tape. The UI already diffs every changed knob; reading it again on the audio thread is the waste.

## Decision
1. **A delta carries moves.** `KnobPublisher` reads each changed knob with the read plan, on the UI thread, and sends a `KnobMove`: the knob, its place before (`from`) and after (`to`), exact `f32` places as the plan reads them.
   - The delta is a ring of the last `MOVES` (6) moves, counted, plus the stamp of the Sound they lead to: 80 B a Part in each `AudioShared` slot.
   - A route-amount change is a move of its own (the FREQ/Q columns are read again).
   - A knob its Sound moves itself (SCRUB's POSITION) gets `from == to`, which lands; a model flip sends the same.
2. **The audio reads no knob.** A move sets the knob's target. A knob that isn't moving starts its glide at `from`, so the audio never needs the old value; one still moving retargets.
   - A silent Part lands each move.
   - A new Sound (`loads` moved) sends no moves and lands at once: nothing moves, so the voices play the stored Sound.
   - More moves than the window between two blocks, or a publish of more than `MOVES` changes at once (only a bulk write), lands every knob too.
3. **No read budget.** The work a block is bounded by construction: at most `MOVES` moves a Part, each a store or two, plus the knobs gliding. ADR 0115's per-block read budget and the owner's 8-knob ruling no longer have anything to bound.
4. **Settled stays three compares** (`Knobs::quiet`): no move, no glide and no stamp change, nothing done.
5. **A value written past the publisher** (tests, the bench; never the shells) is still read whole with the read plan. Each knob glides from its place if known, else lands.
6. **The bill** (host rdtsc × 14.3, the unit/host ratio measured for this path at bbd63cd7; re-check on the next bench):

   | term | cycles/sample | basis |
   |---|---|---|
   | `Knobs::RESERVE` | 14 | once beside the bus: six settled Parts' checks, ~860 cycles a block (host 60) |
   | `Knobs::COST` | 6 | a sounding Part: a move and two glides, ~325 cycles a block (host 23) |

   Together that is the 20 cycles a sample ADR 0040's costliest six leave beside the master tape. A bulk write (six knobs on every Part in one block, ~7k at most) runs in the headroom, as ADR 0061's setting eases do.

## Alternatives considered
- **Keep reads, under a smaller budget (2 a block):** a load landing over 33 blocks, and still ~1.1k cycles of reads a block to bill.
- **Every Part's 66 targets in each slot:** 4.7 KB of AXI; the bench builds hold 8 KiB.
- **`u16` places:** half the bytes, but a step of 1.5e-5 of a span showed as a 0.4% excess on the slow-turn bound. Exact places at a six-move window cost 8 B more a Part.
- **The audio keeping the old value itself:** after a load it doesn't have one; `from` makes that moot.

## Consequences
- **Host** (median rdtsc a block, six Parts, Part 6 playing):

  | case | knobs' ease |
  |---|---|
  | settled | 60 |
  | a load | 60 (lands) |
  | a move on every Part every block | 196 |
  | six moves on every Part every block | 520 |

- **AXI:** the delta is 80 B (was 72): −144 B over the slots. The tightest bench build, `bench-algo,master-tape`, keeps 8,500 B against its 8,192 floor (308 B over); next to give is `MOVES` 6 → 4 (−288 B).
- **STRESS** gains a MOVE run (a knob turned on every Part every block) and a BILLED line (what the bill counted at each run's costliest block), so the unit shows a move's cost and any unbilled share directly.
- **Tests:** `knob_reads_test` counts moves:
  - a turn is 1; FX, mix and ORBIT edits 0; ORBIT's start 0;
  - a load is 0 and lands at once, five at once too;
  - skipped publishes lose nothing; a block takes at most `MOVES`, past them it lands.

  The unit tests pin the moves, the window, the model flip and the read plan against `get`.

## Sources
- STRESS on the unit @ bbd63cd7 and 6b9386cd (KNOBS lines); (private notes).
- `chimera-engine/src/dsp/knobs.rs` (`KnobMove`, `KnobDelta`, `KnobPublisher`, `Knobs::ease`), `chimera-core/tests/knob_reads_test.rs`.
