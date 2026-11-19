# Per-Part EQ (#342): design

Status: approved by the owner, 2026-11-18. Reference: the Intellijel Jellymix's channel EQ (tilt ±10 dB at 810 Hz, a switchable 150 Hz low cut).

## Owner's rulings
- Two controls on every Part's MIXER channel page, part of the **project's mix**, never the Sound: loading a Sound never changes them.
  - **TILT**: one knob, dark ↔ bright, ±10 dB, pivoting at 810 Hz. Centre is flat and the default. It steps in 1/128; at flat the step lands exactly on 0 dB.
  - **LO CUT**: on/off, a fixed one-pole high-pass at 150 Hz, −6 dB/oct. Default off.
- **Placement in the signal:** the very end of the Part's channel, after the Sound's chain, before the Part's level, pan and sends, so the FX hear the EQ'd Part.
- Both filters one-pole. TILT glides click-free, never zippered or steppy; LO CUT toggles click-free.
- Old projects load flat with LO CUT off. With the EQ flat and off every audio golden is bit-identical: a true bypass, costing nothing.
- **Memory and flash** (measured on 0.4 ORBIT): the EQ code in bank 2 S4–S7 behind a door (ADR 0272; ADR 0283's S4 table is the precedent), bank 1 only a measured door stub; filter state in ITCM (ADR 0266); the two settings packed into one typed byte of the Part's mix in AXI (TILT 7 bits, LO CUT 1 bit, as #966's `part::Voicing` packs its byte). If AXI can't take it, stop: ADR 0087's margin is not lowered.
- CPU: a bench row, measured; expected well under 1 %.
- SMART EQ (#396) later adds automatic cuts inside this same EQ: one EQ per Part, never a second hidden one. Not built here, not designed out.
- The DJ isolator (#570) is out of scope (a master filter, later).
- Saving: new Part block param ids through the generic codec, disk-code and NRPN fixtures updated. Whether TILT and LO CUT are CC/NRPN mappable (as LEVEL and PAN) is Claude's call, recorded in the ADR.

## Design
- **Types** (`chimera_engine::part_eq`, functional core):
  - `Tilt`: a step in −64..=63 (`Tilt::FLAT` = 0), 10/64 dB a step: −10 dB at −64, +9.84 dB at +63. 128 steps, one per 1/128 of the knob; CC/NRPN value `64 + step`, so CC 64 is exactly flat (the `ValFmt::Bi` convention).
  - `LoCut { Off, On }`.
  - `PartEq(u8)`: the stored byte, TILT's step + 64 in bits 0–6, LO CUT in bit 7. Every byte decodes; `PartEq::FLAT` (0 dB, OFF) is the default, and `is_flat()` is the bypass test.
- **The filters**, per Part, on its mono bus and, when a DRUM pad put one there, its side (the same linear filter on mid and side is the same filter on L and R):
  - LO CUT: `y = x − a · lp150(x)`, `a` LO CUT's amount eased 0 ↔ 1 (the mix's `Ease`, ~the gains' time): a one-pole high-pass at 150 Hz when on, the input when off, a crossfade between, click-free.
  - TILT: `H(s) = (k·s + ω)/(s + k·ω)`, ω = 2π·810 Hz, k = 10^(dB/20): DC 1/k, top k, exactly 0 dB at 810 Hz. As a one-pole low-pass: `y = k·x − ((k² − 1)/k) · lp_{k·ω}(x)`, bilinear with the warp fixed at the pivot, so the low-pass's TPT gain is `g = k · tan(π·810/fs)`: one constant, no `tan` per change. TILT's dB is eased a block at a time; `k`, the low-pass's coefficient and the mix weight are lerped across each block from the last block's, so a turn never steps.
  - Order: LO CUT, then TILT.
- **Bypass:** a Part whose `PartEq` is flat and whose eases have landed there is not touched: no door call, its bus bit for bit. A Part leaving bypass starts from zeroed filter state (at flat with LO CUT's amount 0 the state is not heard, and the eases start at flat).
- **Where:** `Instrument::render_streamed`, after the voices are summed into the buses and before `mix_sided` (level, pan, sends). Bank 1 holds only the stub that decides bypass and calls the door; the filters run in bank 2 (an S4 group behind the EQ's own table, as ADR 0283's). A closed door plays the Part flat.
- **State:** `PartEqs` (each Part's eases, last coefficients and filter states, mid and side): a static in ITCM on the unit (D2, where the `Instrument` lives, is full); the Instrument's own on a host, so tests stay independent. All-zero bytes are its rest state; the boot zeroes it.
- **UI:** a sub-page EQ under the MIXER's PRT page (as #966's VOICES): TILT (`ValFmt::Bi`, −64..+63) and LO CUT (OFF/ON).
- **Saving:** Part block ids 15 (TILT) and 16 (LO CUT): 13 and 14 are #966's. TILT is a stepped value, LO CUT a choice; both through the generic codec. A file without them loads `PartEq::FLAT`.
- **MIDI:** TILT is mappable on the Part block like LEVEL and PAN (a performance knob); LO CUT is a switch and stays the hand's.
- **SMART EQ later** adds its cuts as more stages of the same `part_eq` chain and the same state, behind the same bypass test.

## Open
- The docs page (annotated screenshot with numbered callouts) is not written now.
- TILT's readout is the step (−64..+63); a dB readout would cost bank-1 formatting code.
