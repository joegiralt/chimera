# 0284. End each Part's channel in a TILT and LO CUT EQ, in the project's mix

- **Status:** Proposed
- **Deciders:** owner (design approved 2026-11-18, #342), firmware

## Context
The owner asked for a channel EQ on every Part, after the Intellijel Jellymix: one TILT knob (±10 dB about 810 Hz) and a switchable 150 Hz LO CUT. It belongs to the project's mix, not the Sound, and sits at the end of the Part's channel so the FX hear the EQ'd Part. SMART EQ (#396) will later add automatic cuts to the same EQ. The DJ isolator (#570) is a master filter, out of scope.

Bank 1 is frozen (ADR 0198); new 0.4 code goes to bank 2's S4 groups (ADR 0272). The door's own table is full, as ADR 0283 found; VA's solos' table heads the S4 groups at 0x08180400. D2, where the `Instrument` lives, is full. ITCM is the CPU-only RAM for state (ADR 0266). The first measurement put the setting's byte 28 B over ADR 0087's AXI margin: `PartParams` has no padding, so one byte grows it 16 → 20 B, ×6 Parts ×3 audio snapshots = 72 B. ADR 0286 then moved the snapshots to ITCM.

## Decision
1. **The setting is one typed byte in the Part's mix.** `part_eq::PartEq(u8)` (`PartParams::eq`): TILT's code in bits 0–6, LO CUT in bit 7. Every byte decodes. `PartEq::FLAT` (0 dB, OFF) is the default and what a project without them loads.
   - `Tilt` is a step, −64..=63, 1/128 of the 20 dB travel each (10/64 dB): −64 is −10 dB, 0 exactly 0 dB, +63 +9.84 dB. Its code, CC and NRPN value are `64 + step`, so CC 64 is flat.
   - `LoCut { Off, On }`.
   - Loading a Sound never touches it: the Sound has no EQ.
2. **Block ids 15 and 16 of the Part block.** TILT is stepped (`ValFmt::Bi`, stored as its step, NRPN `Index7`); LO CUT is a choice (codes OFF 0, ON 1, `Code7`). Both go through the generic codec; the disk-code and NRPN fixtures gain their lines. 13 and 14 are ADR 0281's.
3. **TILT is mappable to a CC like LEVEL and PAN; LO CUT is not.** TILT is a performance knob, as on the Jellymix. LO CUT is a setup switch, and a mapping table's rule (ADR 0217, `Mappable`) keeps switches the hand's.
4. **The filters.** One EQ per Part, after the voices are summed into its bus and before `mix_sided` (LEVEL, PAN, sends), on the bus and on a DRUM pad's side (the same linear filter on mid and side is the same on L and R).
   - LO CUT: `y = x − a · lp₁₅₀(x)`, a TPT one-pole high-pass at 150 Hz, −6 dB/oct, crossfaded by `a`, eased 0 ↔ 1 (`Ease`, 20 ms): toggling is click-free.
   - TILT: `H(s) = (k s + ω)/(s + k ω)`, ω = 2π · 810 Hz, k = 10^(dB/20): 1/k below, k above, exactly 0 dB at 810 Hz. As `y = k x − (k − 1/k) · lp(x)` with a one-pole low-pass at `k ω`, bilinear and warped at the pivot, so its `g` is `k · tan(π · 810 / fs)`: no `tan` per change. TILT's dB is eased a block at a time (20 ms); `k`, `k − 1/k`, the low-pass's gain and LO CUT's amount are lerped across each block from the last block's, so a turn never steps.
   - Order: LO CUT, then TILT.
   - A Part with nothing written this block lands its eases unheard and empties its filters, as the mix's gains land.
5. **A true bypass.** A Part whose `PartEq` is flat and whose eases have settled there is not touched: with every Part so, bank 1's stub makes no call. A Part leaving bypass starts from empty filters at flat. So with the EQ flat and off every render is bit for bit as before.
6. **Bank 2 behind its own table.** `part_eq::dsp::run` (all inlined) is an S4 group (`*7part_eq3dsp*`), reached through `part_eq::door::EqDoor` (magic 0xB2E0_0284, one entry) at `PART_EQ_DOOR_ADDR` = 0x08180410, the next 16 B after VA's solos' 8 B table, checked by the boot's door check (`door::door_ok`). `check-door-placed` pins `part_eq::dsp::` to the S4 group. A closed door plays every Part flat. Bank 1 keeps the stub (`Instrument::part_eq`), the types and `PartEqs::any_live`.
7. **State in ITCM.** `PartEqs` (each Part's eases, last coefficients, filter memory for mid and side, and whether it runs) is `part_eq::PART_EQS` on the unit, an ITCM resident (`tools/itcm-residents.txt`); all-zero bytes are its rest (`PartEqs::REST`), which `zero_itcm` leaves at boot, and each `Instrument` built resets it. `Instrument` holds a 0 B token (`EqHome`); on a host the state is the Instrument's own, so tests stay independent.
8. **UI.** MIXER › PRT's second sub-page, EQ (`block_registry::PART_EQ`, id 118, under VOICES): TILT and LO CUT in cells A and B. EDIT walks PRT → VOICES → EQ → the Sound rung under STACK; SEQ back up.
9. **SMART EQ (#396)** adds its cuts as more stages of the same `part_eq` state and block, behind the same bypass test: one EQ per Part, never a second hidden one.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, origin/0.4 a56b6fab against this change:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| default | +932 | +3,424 |
| orbit | +980 | +3,488 |
| bank2-gate | +940 | +3,424 |
| orbit,usb-config-writer | +1,004 | |
| fault-inject, filter-probes | +852 | |
| bench-algo, bench-routing | +772 | |
| bench-algo,master-tape | +876 | |
| bench-memory | +940 | |
| bench-stress | +868 | |
| bench-orbit | +676 | |
| bench-tape | +1,092 | |

- Bank 1 on ORBIT: `Instrument::part_eq` (the stub) 232 B, `PartParams`' `write`/`get`/codes +330 B, `PART_SPECS` +144 B, `PART_EQ` 96 B, the rest inlining churn. A first draft took libm's `tanf` (768 B) into bank 1 for the warps; `dsp::tan_small` (its series to x⁷, exact in f32 at these angles) keeps it out.
- Bank 2 on ORBIT: S4–S5 +3,456 B (`run`'s moving and settled loops for mid and side, `run_parts`, `EqDoor`); S0–S3 8,672 → 8,640 B free (above ADR 0251's 8 KB).
- RAM: `PART_EQS` 312 B in ITCM, and `PartParams` 16 → 20 B: the snapshots (ITCM, ADR 0286) +72 B, so ITCM 40,536 → 40,920 B used; the UI's project in D2 +24 B; the `Instrument` (D2) unchanged (`EqHome` is 0 B); AXI unchanged, 23,148 B over the margin.
- CPU: host (x86, release), six Parts EQ'd, LO CUT on: 871 ns a block steady, 885 ns with TILT moving, 0.07 % of the block. Unit, static (`tools/loop_count.py` on `part_eq::dsp::run`): 23 instructions a sample a Part plus about 6 a sample of block work, about 50 cycles a sample a Part at the script's 1.6 cycles an instruction, about 0.5 % of the CPU a Part EQ'd (a DRUM Part with a panned pad's side twice that), nothing for a Part flat and off. The unit's reading is the bench's ROUTING rows EQ (1 OP, TILT +63, LO CUT on) and EQ MOVE (TILT turned every block), each less 1 OP; unread.
- Every golden is bit for bit (`just check`): with every Part flat and off, `Instrument::part_eq` returns before the door.

## Alternatives considered
- **A new byte without ADR 0286.** 28 B over AXI's margin on ORBIT (measured); the margin is not lowered. Packing TUNING with MAPPING to free a byte was the other way and reshaped ADR 0259's fields; ADR 0286 made it unneeded.
- **An entry in `Bank2Door`.** Full (ADR 0283).
- **The state in `Instrument` or `MixState`.** D2 is full.
- **A shelving biquad pair.** The owner asked for one-pole filters; one one-pole low-pass gives the tilt exactly, with unity at the pivot.
- **Recomputing the coefficients each sample.** The ease is already per block; lerping them across the block is smooth (the click tests) at a fraction of the cost.
- **LO CUT by sweeping its corner from 0 Hz.** A crossfade of the high-pass is as smooth, cheaper and exact at both ends.
- **TILT −63..=63.** Symmetric, but CC 64 would not be flat and a step would not be 1/128 of the knob.
- **LO CUT mappable.** A switch; the mapping rule keeps those the hand's.

## Consequences
- Every Part has TILT and LO CUT in the mix, saved with the project, untouched by Sound loads; old projects load flat and off.
- With every Part flat and off the audio pays one check a block; with EQ on, see Measurements.
- TILT's readout is its step (−64..+63), not dB: a dB readout would need bank-1 formatting code.
- A docs page (annotated screenshot with numbered callouts) is still owed.

## Sources
- Owner's design, 2026-11-18; spec `docs/superpowers/specs/2026-11-18-part-eq-design.md`, plan `docs/superpowers/plans/2026-11-18-part-eq.md`.
- Intellijel Jellymix: tilt EQ ±10 dB at 810 Hz, switchable 150 Hz low cut (retailer listings; the manual's text unread).
- ADR 0010, 0021, 0087, 0198, 0217, 0251, 0266, 0272, 0281, 0283, 0286.
- Code: `chimera-engine/src/part_eq/` (`mod.rs`, `dsp.rs`, `door.rs`), `part.rs`, `instrument.rs` (`part_eq`), `chimera-core/src/ui/block_registry.rs`, `ui/nav.rs`, `chimera-stm32/build.rs`, `chimera-stm32/src/bench.rs`.
- Tests: `chimera-engine/tests/part_eq_test.rs`, `part_eq_dsp_test.rs`; `chimera-core/tests/part_eq_render_test.rs`, `part_eq_door_test.rs`, `part_eq_page_test.rs`, `project_codec_test.rs`.
