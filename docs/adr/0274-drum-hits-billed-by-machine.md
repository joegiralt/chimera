# 0274. Bill each DRUM hit for its machine, each DRUM Part for its kit's costliest

- **Status:** Proposed
- **Deciders:** owner, firmware

## Context
DRUM was admitted on ALGO's bill (`EngineType::Drum => AlgoEngine::cost(...)`, #951). On the unit (2026-11-14, ce1f5243, INIT DRUM) the open METAL HAT and TONE CYMBAL cost about 53 % of a block each, far above that bill. Eight overlapping hits overran the audio and twelve reset the unit (ADR 0273). The machines differ by more than 2× (TONE CLAP about 600 a sample, the metals about 1,400). One bill for all of them would either under-bill the metals or cost a light kit most of its voices. ADR 0269's "Billed by the pads" covers the pads' filters, AMP and mods only, not the kernel.

## Decision
- **A hit bills its own machine.** A Part's bill is split at its kernel (`PartBill { rest, kernel }`, from `Voice::drum_cost`). A sounding or waiting hit's voice bills the rest plus its own machine's `Machine::cost` (`PartBill::hit`). Nothing is subtracted, so no bill falls under its machine's cost. `recost` reads each Part's kit bill (`KIT_VIEW`) once per snapshot and keeps the Part's `PartBill` (`Instrument::costs`). Admission (`drum_on`) bills from that kept bill, and the per-block recost (through `hit_of`) from the same read, so both compute the same bill from one kit. A pad switched to another machine after that read, before the next snapshot, still bills its new machine over the kept rest.
- **A Part bills its kit's costliest machine.** The Part's bill (`Voice::drum_cost`, `Instrument::costs`) carries the costliest machine among its kit's pads (`KitBill::kernel`, the `machine` field read from `KIT_VIEW`). It is the bill a hit's own replaces the kernel in, and the one a Part's voice count reads. With no kit read (`KitBill::NONE`) it bills `Machine::MAX_COST`, the costliest machine of all.
- **An ended hit whose key is still held bills no kernel.** Its voice renders no kernel, so `PartBill::hit(HitKernel::Ended)` bills the rest alone.
- **The constants.** Each `Machine::cost` is `ceil(1.1 × reading) − 83` cycles a sample.
  - The reading is INIT DRUM's pad struck alone on the unit: the console's `stats` load_pct 80 ms after a vel-127 strike, whole voice over idle, 1 % = 100 cycles a sample. Rev V at 480 MHz, build 9c2cfd1d, 2026-11-14.
  - × 1.1 is the usual 10 % margin on a unit reading.
  - 83 is the part of INIT DRUM's Part bill that is not the kernel: the chain, AMP and the pads' mods, which the Part bill already holds. The reading is the whole voice, so subtracting it keeps the kernel from being billed twice.
  - `load_pct` averages its window, so a hit that ends inside it reads low. Those machines, and the ones INIT DRUM does not play, are placed from desktop release ns per block against measured machines, erring high. PLAIN IMPULSE (1567, pad 3 read 1200 in a short window) and METAL JINGLE (1677, unmeasured) are such estimates, pending unit readings. PLAIN IMPULSE is INIT DRUM's costliest machine, so its estimate sets that Part's bill.
- **The DRUM voice floor is undecided.** ADR 0202's floor lists ALGO, MODAL and VA. A kit with a metal machine gets only 2–3 voices at its Part bill, and `cost_test` exempts DRUM from the floor check. The decision is tracked in private issue #975.

## Alternatives considered
- **Keep ALGO's bill.** It under-billed the metals and reset the unit.
- **Bill every DRUM voice the costliest machine of all.** Safe, but a kit of kicks and claps would lose about half its voices to a machine it never plays.
- **Constants from desktop ns alone.** The desktop's ratios between machines do not hold on the M7 (cache, FPU), so the unit readings set the scale and the desktop only places the unmeasured.

## Consequences
- A kit's light hits keep many voices. A Part's voice count, read at its kit's costliest machine, shows 2–3 for a kit with a metal or PLAIN IMPULSE pad.
- The bills of the estimated machines err high and cost voices until they are read on the unit.
- A new machine needs a unit reading (or a high estimate) before it ships, and `KitBill`'s 4-bit `machine` field holds at most 15 machines (a const assert on `Machine::ALL` holds it).
- An under-bill that slips through is caught by ADR 0273's load net, not a reset.

## Sources
- `chimera-engine/src/dsp/drum/types.rs` (`Machine::cost`, `MAX_COST`), `chimera-engine/src/dsp/drum/sound.rs` (`KitBill::kernel`, `PartBill::hit`), `chimera-engine/src/instrument.rs` (`drum_on`, `recost`, `hit_of`).
- Tests: `chimera-core/tests/drum_bill_test.rs`, `cost_test.rs`.
- #951, #975; commits afdce176, 006bafaa; unit readings on 9c2cfd1d.
- ADR 0026 (bill by patch shape), 0202 (voice floor), 0269 (pads' bill), 0273 (load net).
