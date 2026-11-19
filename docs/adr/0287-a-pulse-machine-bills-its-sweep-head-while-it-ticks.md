# 0287. A pulse machine bills its sweep head while its pulses tick, its plateau after

- **Status:** Proposed
- **Deciders:** firmware (#991)

## Context
TONE COWBELL and PLAIN IMPULSE play VA's pulse. `Pulse::sweep` renders a block as one square run when the block's pitch is steady and the width is not floored, and a sample at a time (`VarOsc::tick_pulses`) otherwise: while BUMP or SWEEP still moves the pitch, or while IMPULSE's WIDTH floors at its pitch. That per-sample path costs two to three times the run. ADR 0274 billed by machine alone, so one constant had to cover both: ADR 0275 set COWBELL at its BUMP 127 sweep head's window (1,500, bill 1,567) and left its worst block (about 2,000 over idle, `peak_pct` 32 % against the 12 % idle) above the bill, with a time-aware bill named as the fix. A COWBELL at INIT's BUMP 0 reads 600–700, so it was billed more than twice what it costs.

`Instrument::recost` re-bills every sounding voice before each block, so a hit's bill can follow its kernel.

## Decision
- **A hit bills a `KernelBill`** (`dsp::drum::types`): `Machine(m)`, its machine's `Machine::cost`, or `Ticking(PulseMachine)` while its pulses may render a sample at a time. `PulseMachine` is `Cowbell` or `Impulse`, so no other machine can be billed ticking.
- **The kernel says when** (`KernelState::ticking`): a COWBELL while BUMP's fall is above `octaves`' exp2f skip (`sweeping`), an IMPULSE while SWEEP's is or its width floors at its pitch (`Pulse::floored`). It errs high: a few blocks after exp2f already rounds to 1 still bill ticking. A unit test in each kernel holds that it never says steady before a block whose pulses tick, and that once it stops it stays stopped.
- **A strike or a waiting hit** bills from its params (`DrumHit::bill`, through `KernelParams::ticking` at the voice's rate: each sweep's fall starts at 1). A test holds that this equals the struck kernel's `ticking` for every machine at its defaults, all knobs 0, all 127 and each knob at 1, at transposes −60, 0, +12 and +67.
- **A Part's kit bill** (`KitBill`, `PartBill::whole`) takes its costliest pad's strike at its tuned pitch. A TONAL note may floor IMPULSE where the tuned pitch does not; the hit bills its own.
- **Constants**, ceil(1.1 × reading) − 83 (ADR 0274):
  - TONE COWBELL 1,567 → 687: BUMP 0 and the plateau after BUMP, 600–700 on eb442a48, top of the spread (ADR 0275's rule).
  - COWBELL ticking 2,117: BUMP 127's worst block, about 2,000.
  - IMPULSE ticking 1,347, estimated, erring high: SWEEP 64's head window (700, of which about 37 ms is sweep over a 500 plateau) puts the sweep near 950, and COWBELL's worst block runs 1.33 × its window: 1,300. PLAIN IMPULSE steady stays 577.
- **`KernelBill::MAX_COST`** replaces `Machine::MAX_COST`: COWBELL ticking, 2,117, is now the costliest bill (was METAL JINGLE's 1,677). It bills a DRUM voice with no kit read.

This supersedes in part ADR 0274 (one bill per machine) and ADR 0275 (TONE COWBELL's 1,567 and its known limit).

## Alternatives considered
- **Bill by the knobs alone** (BUMP or SWEEP above 0 bills the head for the whole hit). Simpler, but a bumped COWBELL would hold 2,117 through a plateau of 600 for its whole decay, the cost ADR 0275 refused.
- **Make the path cheaper first.** The cost hunt's sound-identical savings (block setup for the pulse lane alone, no `end_shape`, `SawOff`) are already in (d31686d1); the 2026-11-16 readings were taken on them. What is left changes the sound (a naive square with polyBLEP) and is the owner's call.
- **Predict the path exactly** (run `octaves` ahead of the block). It doubles the exp2f calls for a bill that only needs to err high for a few blocks.

## Consequences
- Bills change admission, not sound: every audio golden is bit-identical (the drum nulls and goldens pass unchanged).
- INIT DRUM's COWBELL (BUMP 0) frees about 880 cycles a sample of booking; a bumped COWBELL is booked about 550 more for its first ~110 ms. INIT DRUM's Part bill (`whole`) falls from 1,567 to the HATs' 1,457. Voices, by `drum_bill_test` on rev V's budget: INIT DRUM's twelve pads a block apart with `master-tape` now play the COWBELL (one refusal, the CYMBAL, where two were refused); without the tape, the roll and the twelve pads play as before.
- IMPULSE's ticking constant is an estimate. `peak_pct` now restarts at each `stats` read (#995), so a per-hit peak can be read; re-read IMPULSE at SWEEP 64 on the unit and set it.
- Bank 1 grows +1,384 to +1,472 B a set (default +1,432, ORBIT +1,424; ORBIT's bank 1 keeps 22,772 B free): `recost`'s per-hit `ticking` (+488), `drum_on`'s and `KitWriter::publish`'s strike bill (+236, +238), and `DrumHit::kernel_params` built for size once for the strike and the bill (+456 over the copy `strike` held inlined; ADR 0267). Under ADR 0251's 0.4 DRUM allowance, each line raised by its measured delta, #995's `read_peak` in the same measure. Bank 2 unchanged (bank2-gate +32, under its line).

## Sources
- private issue #991; ADR 0273, 0274, 0275.
- `chimera-engine/src/dsp/drum/types.rs` (`KernelBill`, `PulseMachine`), `kernel/mod.rs` (`ticking`, `sweeping`, `floored`, `DrumHit::bill`), `instrument.rs` (`bill_of`).
