# 0284. End each Part's channel in a TILT and LO CUT EQ, in the project's mix

- **Status:** Proposed
- **Deciders:** owner (design approved 2026-11-18, #342), firmware

## Context
The owner asked for a channel EQ on every Part, after the Intellijel Jellymix: one TILT knob (±10 dB about 810 Hz) and a switchable 150 Hz LO CUT. It belongs to the project's mix, not the Sound, and sits at the end of the Part's channel so the FX hear the EQ'd Part. SMART EQ (#396) will later add automatic cuts to the same EQ. The DJ isolator (#570) is a master filter, out of scope.

Bank 1 is frozen (ADR 0198); new 0.4 code goes to bank 2's S4 groups (ADR 0272). The door's own table is full, as ADR 0283 found; VA's solos' table heads the S4 groups at 0x08180400. D2, where the `Instrument` lives, has 3,424 B left on ORBIT's link and no margin of its own (it had about 8 B on 2026-11-12, ADR 0266). ITCM is the CPU-only RAM for state, zero-wait from either bank (ADR 0266), and keeps 8 KiB free (ADR 0286). The first measurement put the setting's byte 28 B over ADR 0087's AXI margin: `PartParams` has no padding, so one byte grows it 16 → 20 B, ×6 Parts ×3 audio snapshots = 72 B. ADR 0286 then moved the snapshots to ITCM.

## Decision
1. **The setting is one typed byte in the Part's mix.** `part_eq::PartEq(u8)` (`PartParams::eq`): TILT's code in bits 0–6, LO CUT in bit 7. Every byte decodes. `PartEq::FLAT` (0 dB, OFF) is the default and what a project without them loads.
   - `Tilt` is a step, −64..=63 (`SPAN` 128, `all()`), 1/128 of the 20 dB travel each (10/64 dB): −64 is −10 dB, 0 exactly 0 dB, +63 +9.84 dB. Its code, CC and NRPN value are `step − MIN_STEP`, so CC 64 is flat; `CODE_FLAT` and the steps to full are derived from `MIN_STEP`. `PartEq::tilt` decodes through `Tilt::from_code`, the one path.
   - `LoCut { Off, On }`; `LoCut::from_f32` reads a stored value as the other choices do (`as`, so 0 and NaN are off).
   - The units are types: `Tilt::db() -> dsp::Db`, the corners `dsp::Hz` (lifted from `drum::types`), LO CUT's `amount() -> Unit`.
   - Loading a Sound never touches it: the Sound has no EQ.
2. **Block ids 15 and 16 of the Part block.** TILT is stepped (`ValFmt::Law(Law::EqTilt)`, stored as its step, NRPN `Index7`); LO CUT is a choice (codes OFF 0, ON 1, `Code7`). Both go through the generic codec; the disk-code and NRPN fixtures gain their lines. 13 and 14 are ADR 0281's.
3. **TILT is mappable to a CC like LEVEL and PAN; LO CUT is not.** TILT is a performance knob, as on the Jellymix. LO CUT is a setup switch, and a mapping table's rule (ADR 0217, `Mappable`) keeps switches the hand's.
4. **The filters.** One EQ per Part, after the voices are summed into its bus and before `mix_sided` (LEVEL, PAN, sends), on the bus and on a DRUM pad's side (the same linear filter on mid and side is the same on L and R).
   - LO CUT: `y = x − a · lp₁₅₀(x)`, a TPT one-pole high-pass at 150 Hz, −6 dB/oct, crossfaded by `a`, eased 0 ↔ 1 (`Ease`, 20 ms): toggling is click-free.
   - TILT: `H(s) = (k s + ω)/(s + k ω)`, ω = 2π · 810 Hz, k = 10^(dB/20): 1/k below, k above, exactly 0 dB at 810 Hz. As `y = k x − (k − 1/k) · lp(x)` with a one-pole low-pass at `k ω`, bilinear and warped at the pivot, so its `g` is `k · tan(π · 810 / fs)`: no `tan` per change. TILT's dB is eased a block at a time (20 ms); `k`, `k − 1/k`, the low-pass's gain and LO CUT's amount are lerped across each block from the last block's, so a turn never steps. The eases are typed (`Glide<Db>`, `Glide<Unit>`); the coefficients are `Coeffs { k: Gain, c: LpShare, g: OnePole, a: Unit }`, a block's per-sample move `CoeffStep`.
   - Order: LO CUT, then TILT.
   - A Part with nothing written this block lands its eases unheard and empties its filters, as the mix's gains land.
   - A NaN or inf on a Part's bus or side leaves its filters' memory not finite; at each Part's block end it is checked and set to rest (`Taps::ZERO`), as CLEAN's filter does (`filter/clean.rs`), so one bad block is all that is heard.
5. **A true bypass.** A Part whose `PartEq` is flat and whose eases have settled there is not touched: with every Part so, bank 1's stub makes no call. A Part leaving bypass starts from empty filters at flat. So with the EQ flat and off every render is bit for bit as before.
6. **Bank 2 behind its own table.** `part_eq::dsp::run` (all inlined) is an S4 group (`*7part_eq3dsp*`), reached through `part_eq::door::EqDoor` (magic 0xB2E0_0284, one entry) at `PART_EQ_DOOR_ADDR` = 0x08180410, one `DOOR_TABLE_STRIDE` (16 B) after VA's solos' 8 B table, checked by the boot's door check (`door::door_ok`). The head, the stride and both tables' addresses are one file, `chimera-engine/src/s4_tables.rs`, that `bank2` and chimera-stm32's build.rs both include: `bank2.x`'s placement and asserts are written from it, and `bank2` asserts each table's `size_of` fits the stride. `check-door-placed` pins `part_eq::dsp::` to the S4 group.
   - A closed door plays every Part flat. The stub then sets `PartEqs` to `REST`: if the door closes mid-note the EQ drops to flat at once (a step, as any closed door's feature does), and a reopened door starts each Part from flat with empty filters, never from the taps it left. Bank 1 keeps the stub (`Instrument::part_eq`), the types and `PartEqs::any_live`.
7. **State in ITCM.** `PartEqs` (each Part's eases, last coefficients, filter memory for mid and side, and whether it runs) is `part_eq::PART_EQS` on the unit, an ITCM resident (`tools/itcm-residents.txt`); all-zero bytes are its rest (`PartEqs::REST`), which `zero_itcm` leaves at boot, and each `Instrument` built resets it. `Instrument` holds a 0 B token (`EqHome`); on a host the state is the Instrument's own, so tests stay independent.
8. **UI.** MIXER › PRT's second sub-page, EQ (`block_registry::PART_EQ`, id 118, under VOICES): TILT and LO CUT in cells A and B. EDIT walks PRT → VOICES → EQ → the Sound rung under STACK; SEQ back up.
   - TILT reads in dB, as the EQ plays it: `0 dB` flat, else signed to a tenth (`-10.0 dB` … `+9.8 dB`). The text is the UI door's (`walk::law_text_in` → `eq_tilt_text`, `ui::fmt::fixed`), as TAPE's laws are; bank 1 adds one arm to `fmt_law`'s door match. Closed, the cell shows no value, as TAPE's; the EQ then plays flat. `Law::EqTilt` snaps and draws bipolar, as `ValFmt::Bi` did.
   - The page's `BlockDef` stays bank-1 data. Nav and the chain walk pages without a door (VOICES' page, ADR 0281, is the precedent); hiding a page behind a closed door would add bank-1 nav code larger than the page's 96 B.
9. **SMART EQ (#396)** adds its cuts as more stages of the same `part_eq` state and block, behind the same bypass test: one EQ per Part, never a second hidden one.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, origin/0.4 76de24c6 against this change (each set built in both trees). Each budget line is raised by exactly its set's measured change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| default | +988 | +4,000 |
| orbit | +868 | +3,968 |
| bank2-gate | +1,020 | +4,032 |
| orbit,usb-config-writer | +892 | |
| fault-inject | +820 | |
| bench-algo, filter-probes | +828 | |
| bench-algo,master-tape | +772 | |
| bench-routing | +796 | |
| bench-memory | +788 | |
| bench-stress | +844 | |
| bench-orbit | +876 | |
| bench-tape | +924 | |

- Merged with origin/0.4 d7611695 (#1027), each set measured again: only bench-orbit and bench-tape (bank 1, +8 B each) and default (bank 2, +32 B) were over their lines, raised by exactly that.
- Bank 1 on ORBIT:
  - The stub `Instrument::part_eq`, 252 B, with the closed door's rest.
  - `PART_SPECS` +144 B, `PART_EQ` 96 B, the row word, LO CUT's names: data, ADR 0198's closed-door data.
  - `PartParams`' TILT and LO CUT arms: `write` +140 B, `get` +44 B, `enum_ident` +38 B, `set_enum_code` +36 B, `enum_code` +10 B, about 268 B. **These are code, an exception to ADR 0198's "no functions beyond the stubs"**, as ADR 0281's get/write/code arms for MAX VOICES and RESERVE (#966) are: the Part block's codec, the project's load and save, MIDI and the editor reach a setting only through these arms, with the door open or closed. `write` was shrunk first: TILT goes straight to its 7-bit code (a saturating cast, no `roundf`, which had inlined about 60 B), LO CUT straight to its bit; it was +212 B.
  - `fmt_law`'s door arm for `Law::EqTilt`, and `ValFmt`'s bipolar snap and draw for it.
  - The rest is inlining churn.
  - A first draft took libm's `tanf` (768 B) into bank 1 for the warps; `dsp::tan_small` (its series to x⁷, exact in f32 at these angles) keeps it out.
- Bank 2 on ORBIT: S4–S5 +3,840 B (`run`'s moving and settled loops for mid and side, the NaN check, `run_parts`, `EqDoor`); S0–S3 8,768 → 8,640 B free (above ADR 0251's 8 KB), TILT's dB text in the UI door's group.
- RAM: `PART_EQS` 312 B in ITCM, and `PartParams` 16 → 20 B: the snapshots (ITCM, ADR 0286) +72 B, so ITCM +384 B (43,000 B used on ORBIT); the UI's project in D2 +24 B; the `Instrument` (D2) unchanged (`EqHome` is 0 B); AXI unchanged, 23,148 B over the margin.
- CPU: host (x86, release), six Parts EQ'd, LO CUT on: 871 ns a block steady, 885 ns with TILT moving, 0.07 % of the block. Unit, static (`tools/loop_count.py` on `part_eq::dsp::run`): 23 instructions a sample a Part plus about 6 a sample of block work, about 50 cycles a sample a Part at the script's 1.6 cycles an instruction, about 0.5 % of the CPU a Part EQ'd (a DRUM Part with a panned pad's side twice that), nothing for a Part flat and off.
- The unit's reading is the `bench-routing` build's ROUTING rows: **EQ** (1 OP, TILT +63, LO CUT on) and **EQ MOVE** (TILT swept end to end a step a block, `Tilt::SPAN`), each less the **1 OP** row. Not yet read; filed for the unit.
- Every golden is bit for bit (`just check`): with every Part flat and off, `Instrument::part_eq` returns before the door.

## Alternatives considered
- **A new byte without ADR 0286.** 28 B over AXI's margin on ORBIT (measured); the margin is not lowered. Packing TUNING with MAPPING to free a byte was the other way and reshaped ADR 0259's fields; ADR 0286 made it unneeded.
- **An entry in `Bank2Door`.** Full (ADR 0283).
- **The state in `Instrument` or `MixState`.** D2 has 3,424 B left on ORBIT's link, so 312 B fits, but with no margin of its own; ITCM is where CPU-only state now goes (ADR 0266, 0286), zero-wait from the audio loop and 8 KiB kept free.
- **The EQ page behind the UI door, as ORBIT's drawing is.** The chain and nav read a page's `BlockDef` with the door closed; a closed door can't hide a sub-page without new bank-1 nav code larger than the page's 96 B.
- **Keeping the EQ's state across a closed door.** A reopened door would resume from taps and glides a block or a whole session old; resting it costs bank 1 one store loop.
- **A shelving biquad pair.** The owner asked for one-pole filters; one one-pole low-pass gives the tilt exactly, with unity at the pivot.
- **Recomputing the coefficients each sample.** The ease is already per block; lerping them across the block is smooth (the click tests) at a fraction of the cost.
- **LO CUT by sweeping its corner from 0 Hz.** A crossfade of the high-pass is as smooth, cheaper and exact at both ends.
- **TILT −63..=63.** Symmetric, but CC 64 would not be flat and a step would not be 1/128 of the knob.
- **LO CUT mappable.** A switch; the mapping rule keeps those the hand's.

## Consequences
- Every Part has TILT and LO CUT in the mix, saved with the project, untouched by Sound loads; old projects load flat and off.
- With every Part flat and off the audio pays one check a block; with EQ on, see Measurements.
- TILT reads dB with the door open, nothing closed.
- A docs page (annotated screenshot with numbered callouts) is still owed.

## Sources
- Owner's design, 2026-11-18; spec `docs/superpowers/specs/2026-11-18-part-eq-design.md`, plan `docs/superpowers/plans/2026-11-18-part-eq.md`.
- Intellijel Jellymix: tilt EQ ±10 dB at 810 Hz, switchable 150 Hz low cut (retailer listings; the manual's text unread).
- ADR 0010, 0021, 0087, 0198, 0217, 0251, 0266, 0272, 0281, 0283, 0286.
- Code: `chimera-engine/src/part_eq/` (`mod.rs`, `dsp.rs`, `door.rs`), `part.rs`, `instrument.rs` (`part_eq`), `s4_tables.rs`, `chimera-core/src/ui/block_registry.rs`, `ui/nav.rs`, `ui/walk.rs` (`eq_tilt_text`), `chimera-stm32/build.rs`, `chimera-stm32/src/bench.rs`.
- Tests: `chimera-engine/tests/part_eq_test.rs`, `part_eq_dsp_test.rs`; `chimera-core/tests/part_eq_render_test.rs`, `part_eq_door_test.rs`, `part_eq_page_test.rs`, `project_codec_test.rs`.
