# 0287. A pulse machine bills its sweep head while its pulses tick, its plateau after

- **Status:** Accepted (2026-11-19)
- **Deciders:** firmware (#991)

## Context
TONE COWBELL and PLAIN IMPULSE play VA's pulse. `Pulse::sweep` renders a block as one square run when the block's pitch is steady and the width is not floored, and a sample at a time (`VarOsc::tick_pulses`) otherwise: while BUMP or SWEEP still moves the pitch, or while IMPULSE's WIDTH floors at its pitch. That per-sample path costs two to three times the run. ADR 0274 billed by machine alone, so one constant had to cover both: ADR 0275 set COWBELL at its BUMP 127 sweep head's window (1,500, bill 1,567) and left its worst block (about 2,000 over idle, `peak_pct` 32 % against the 12 % idle) above the bill, with a time-aware bill named as the fix. A COWBELL at INIT's BUMP 0 reads 600–700, so it was billed more than twice what it costs.

`Instrument::recost` re-bills every sounding voice before each block, so a hit's bill can follow its kernel.

## Decision
- **A hit bills a `KernelBill`** (`dsp::drum::types`): `Machine(m)`, its machine's `Machine::cost`, or `Ticking(Ticking)` while its pulses render a sample at a time. `Ticking` names each per-sample path a pulse machine has: `CowbellSwept`, `ImpulseSwept`, `ImpulseFloored`, `ImpulseFlooredSwept`; no other machine, and no floored COWBELL, can be billed one.
- **The kernel says how** (`KernelState::pace`, a `PulsePace`: `Runs`, `Swept`, `Floored`, `FlooredSwept`), and `KernelBill::of(machine, pace)` picks the bill, matching every machine. Swept: the sweep's fall is above `octaves`' exp2f skip (`sweeping` gives a `SweepState`, `Moving` or `Spent`, on typed `Octaves`, never below 0, and the fall as a `Unit`); it errs high for the few blocks after exp2f already rounds to 1. Floored: the pulse's width floors at the block's first, highest pitch (`Octaves::head`: `Octaves` clamps a depth under 0 to 0, so a sweep only falls), which is exactly whether any sample of the block floors. Unit tests in each kernel hold both, block by block, and that once a hit runs it stays running.
- **A strike or a waiting hit** bills from its params (`DrumHit::bill`, through `KernelParams::pace` at the Instrument's `SampleRate`: each sweep's fall starts at 1). A test holds that this equals the struck kernel's `pace` for every machine at its defaults, all knobs 0, all 127 and each knob at 1, at transposes −60, 0, +12 and +67.
- **A Part's kit bill** (`KitBill`, `PartBill::whole`) takes the costliest bill its pads' machines can run up at any knobs, pitch and rate (`KernelBill::worst`): no rate or resolve in the UI, and it is stored only when a publish changes the pads. A DRUM hit's admission bills its own; `whole` is the Part's bound.
- **Constants**, ceil(1.1 × reading) − 83 (ADR 0274):
  - TONE COWBELL 1,567 → 687: BUMP 0 and the plateau after BUMP, 600–700 on eb442a48, top of the spread (ADR 0275's rule).
  - `CowbellSwept` 2,117: BUMP 127's worst block, about 2,000.
  - `ImpulseSwept` 1,347, estimated: SWEEP 64's head window (700, of which about 37 ms is sweep over a 500 plateau) puts the sweep near 950, × 1.33 (COWBELL's worst block over its window): 1,300.
  - `ImpulseFloored` 2,117, estimated: desktop timing puts a floored IMPULSE (2 kHz, WIDTH 127, an octave up) near a 2,000 reading. It ticks for the whole hit.
  - `ImpulseFlooredSwept` 2,887, estimated: the same at SWEEP 127, near 2,700. A sweep whose head floors the pulse at its tuned pitch bills it too, until the head falls under the floor.
  - PLAIN IMPULSE steady stays 577.
- **`KernelBill::MAX_COST`** replaces `Machine::MAX_COST`: `ImpulseFlooredSwept`, 2,887, is now the costliest bill (was METAL JINGLE's 1,677). It bills a DRUM voice with no kit read.

This supersedes in part ADR 0274 (one bill per machine, a Part's bill its costliest machine's) and ADR 0275 (TONE COWBELL's 1,567 and its known limit).

## Alternatives considered
- **Bill by the knobs alone** (BUMP or SWEEP above 0 bills the head for the whole hit). Simpler, but a bumped COWBELL would hold 2,117 through a plateau of 600 for its whole decay, the cost ADR 0275 refused.
- **Make the path cheaper first.** The cost hunt's sound-identical savings (block setup for the pulse lane alone, no `end_shape`, `SawOff`) are already in (d31686d1); the 2026-11-16 readings were taken on them. What is left changes the sound (a naive square with polyBLEP) and is the owner's call.
- **Predict the swept path exactly** (run `octaves` ahead of the block). It doubles the exp2f calls for a bill that only needs to err high for a few blocks; the floor needs only the head's one.
- **One IMPULSE ticking bill** (1,347). Not a safe bound: a floored IMPULSE ticks for its whole hit at about 1.5 to 2 × that.
- **A Part's kit bill from each pad's strike** (`DrumHit::bill` per pad on publish). It resolved twelve pads' laws on every UI pass at a rate the UI does not know, for a figure DRUM admission never reads.

## Consequences
- Bills change admission, not sound: every audio golden is bit-identical (the drum nulls and goldens pass unchanged).
- INIT DRUM's COWBELL (BUMP 0) frees about 880 cycles a sample of booking; a bumped COWBELL is booked about 550 more for its first ~110 ms; a floored IMPULSE 1,540 to 2,310 more than it was. INIT DRUM's Part bill (`whole`) rises from 1,567 to its IMPULSE's worst, 2,887 (no DRUM admission reads it). Voices, by `drum_bill_test` on rev V's budget: INIT DRUM's twelve pads a block apart with `master-tape` now play the COWBELL (one refusal, the CYMBAL, where two were refused); without the tape, the roll and the twelve pads play as before.
- **The unit must re-read IMPULSE** (#1026), one hit per `stats` now that `peak_pct` restarts at each read (#995): SWEEP 64 at its tuned pitch; floored (PITCH 127, WIDTH 127, TONAL on it, an octave up); floored with SWEEP 127. Then set `ImpulseSwept`, `ImpulseFloored` and `ImpulseFlooredSwept` by the formula. COWBELL at BUMP 127 and BUMP 0 for confirmation.
- Bank 1 grows +1,856 to +1,952 B a set against origin/0.4 b8230cc6 (default +1,888, ORBIT +1,912; ORBIT's bank 1 896,256 B, 21,248 B free): `recost`'s per-hit `pace` with the floor's head (`exp2f` once a block a pulse hit), `DrumHit::bill` and `DrumHit::kernel_params` built for size once for the strike and the bill (ADR 0267; `strike` held its copy inlined), the kit's bill by `KernelBill::worst`, now stored only when a publish changes the pads. Under ADR 0251's 0.4 DRUM allowance, each line raised by its measured delta, #995's `read_peak` in the same measure; ORBIT's line also gives back 696 B it carried from before. Bank 2 unchanged.

## Sources
- private issue #991; ADR 0273, 0274, 0275.
- `chimera-engine/src/dsp/drum/types.rs` (`KernelBill`, `Ticking`, `PulsePace`), `kernel/mod.rs` (`pace`, `sweeping`, `floored`, `Octaves`, `DrumHit::bill`), `sound.rs` (`kit_bill`), `instrument.rs` (`bill_of`).
