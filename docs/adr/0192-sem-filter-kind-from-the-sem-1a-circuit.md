# 0192. SEM: a filter KIND from the SEM-1A circuit (supersedes in part 0168 and 0190)

- **Status:** Proposed
- **Deciders:** owner (approved the circuit lab's SEM by ear, 2026-10-20, epic #720; RES keeps the circuit's law, not spread), firmware (the port, the controls' layout, bank 2)

## Context
The owner rejected the first SEM prototype as "an existing model played with a little": its RES law was CLEAN's, so it was CLEAN up to RES ½. The second pass (#720, branch `sem-circuit-lab`, ledger (private notes)) modelled the Oberheim SEM-1A VCF as built:
- **The reference** (`chimera-utils/src/sem_circuit/reference.rs`) is the VCF sheet's parts in one nodal system, 15 unknowns, Newton with an analytic Jacobian at ×8 (384 kHz). It holds A12 (an LM301A, with finite GBW, slew and rails), the two CA3080 integrators (an Ebers–Moll pair with base currents, output mirrors that saturate, and output resistance), the 2N4302 source followers Q39/Q40, A14, the RES pot PS16 with R157, D11/D12 with R156, C15, C18, C22 and C23. The method and the CA3080 fit are Köper et al.'s (DAFx-22, CC BY 4.0). It was checked against the schematic's algebra and the datasheets (`notes.md`, § Validation): BP peak = 1/k within 0.0035, the MODE notch where PS19 puts it, and no self-oscillation (Q 59 at 100 Hz to 13 at 12 kHz, every ring decays).
- **The firmware model** (`fw.rs`) is Zavalishin's TPT SVF in the circuit's own sign and volts, with every constant derived from the reference's parts. Over the lab's 75 A/B jobs (seven stimuli, RES 0 / ½ / 0.9 / 1, LEVEL −12 / 0 / +12 dB) it nulls the circuit at −32.4 dB mean (worst −14.2, at RES 1 on bass), LSD 0.31 dB mean. The gain at fc is within 0.2 dB over every RES and level. H2 is within 1–2 dB of the reference from −12 to +18 dB of drive. The full-RES ring times to −40 dB are 402 / 278 / 55 / 8.2 / 2.8 ms against the circuit's 382 / 286 / 55 / 8.0 / 3.5 ms (100 Hz to 10 kHz). The MODE notch lands within 0.4 % of the circuit's, 55–63 dB deep. An "exact" variant (the followers folded into the solve, a divide a sample) was no better.
- **How it differs from CLEAN at matched Q** (`notes.md`): RES runs from Q 0.33 to 13–59, ending in a ring that never holds; the diodes give about 18 dB of headroom, then damp; the ring's length depends on the cutoff; MODE has a moving notch; and the followers add even harmonics.

The lab's cost probe counted 120/130 cycles steady and 155–165 moving. It found f32 `min`/`max` and `clampf` at 8 instructions a clamp, but in its own probe crate, not on the firmware build.

## Decision
- **KIND.** `FilterKind::Sem`, disk code **8**, ident "SEM", screen name SEM, last on TYPE's knob. Code 8 is past the routing spec's kept 2 (MS20), 3 (SH101) and 5 (TB303) and LPG's 7. Existing codes are unchanged. Old Sounds keep their KIND, and an unknown code still loads as CLEAN LP4. Its MODE choices are LP2's code (the MODE output) and BP2's (BANDPASS), with the MODE output as the default. A CLEAN LP2 or BP2 Sound that changes to SEM keeps its choice, and every other mode lands on the MODE output.
- **The model** is `fw.rs`, ported to `chimera-engine/src/dsp/filter/sem.rs` as no_std f32, with no allocation and no `unsafe`. Its constants are the lab's, printed by `semc-consts` (`export.rs` on `sem-circuit-lab`):
  - A12: `hp = −u − D(bp) − (k + g)·s₁ − s₂`, held at ±14 V.
  - The OTAs: `V0·tanh(v/V0)` as its series to v⁵, with V0 = 25.04 V.
  - The followers: `r(b)`, a cubic, scales each integrator's step.
  - D11/D12: a dead-zone fit, `0.154·φ(|b| − 0.1)` with a 3.32 V knee.
  - The states are held at −12.60 / +13.78 V, and BP and LP at ±13.5 V.
  - Digital 1.0 is one VCO saw at the input, ±3.6 V × R139/R155.
  - Per block, `g` is the cutoff with its droop (1.9·10⁻⁶ per Hz above the 1 kHz trim), and `k` is the pot law plus C22's coupling, the integrators' leak, and C18's lead less A12's lag at that cutoff. The pot law takes 2 divides a block.
- **RES keeps the circuit's law** (owner): PS16 is linear with 100 Ω ends (the lab's assumption). k is 3.02 at RES 0 (Q 0.33), 1.10 at ½, 0.27 at 0.9 and 0.006 at 1, so all the high Q is in the knob's last tenth.
- **The couplings are the voice's DC blocker.** C15 (4.8 Hz in) and C23 (10.6 Hz out) are not modelled. The voice's 5 Hz blocker after the filter (ADR 0060) stands in, which saves 15 cycles. The lab found the sound the same without them, only the sub-20 Hz phase moving.
- **Holds are `clampf`.** On the firmware build (`target-cpu=cortex-m7`, FPv5) `clampf` compiles to a `vmaxnm` and a `vminnm`: 2 instructions a hold. A compare and select (`filter::hold`) is 3 (vcmp, vmrs, vsel). Switching the eight per-sample holds from compare-select to `clampf` cut the steady loop from 95 to 84 instructions. The lab's 8-instruction figure was its probe crate's, not the engine's.
- **Bank 2.** Every SEM loop and the per-block ends live in `sem::kern`. With `bank2`, `bank2.x` links `*6filter3sem4kern*` into `.bank2`, and the door's `SemDoor { run }` is the entry. `Sem::process` is bank 1's side: it calls through the door, and while the door is closed the block plays dry, the LED dark, as FUZZ does (ADR 0157). TYPE then reads NEEDS UPDATE at SEM (`FilterKind::in_bank2`, `bank2::closed_text`). `check-door-placed` checks `filter::sem::kern::`, and `sem_proxy_test` covers the door open and closed.
- **Controls** (FLT's front: A KIND · B FX · C Q/RES · D MODE/VOICE · E FX PLACE · F FREQ/CUTOFF, ADR 0167):
  - **D is MODE**, a new filter param 16, ident `MORPH`, spec name "SEM MODE" (the matrix and the MIDI map), cell word MODE. It is `Uni` 0..1 with neutral 0 (LP): ½ notches at FREQ, 1 is HP, and in between the notch sits at `FREQ·√((1 − m)/m)`. It glides as a knob (`Smoothing::KNOB`), is modulatable (a matrix destination, MIDI-learnable), and is ramped across each block. SEM reads it, and the other kinds leave it inert.
  - **C is RES and F is FREQ**: the SEM's panel words, abbreviated because RESONANCE (67 px) and FREQUENCY (66 px) don't fit the cell's 62 px bar.
  - **BP is its own cell, not a MODE end.** OUTPUT sits in FLT › MOD's F, the one cell free on every kind's pages since ADR 0190, and reads MODE or BP. It is the enum MODE param (7), so it crossfades like any MODE change (a stack copy, exact, as DIRTY's). A MODE end was rejected: MODE is modulatable and smoothed, and an LFO or glide crossing into BP would switch the output mid-sweep (see Alternatives). The header reads SEM MOD on that page. BANDPASS (82 px) doesn't fit the value face, so the value reads BP, CLEAN's word (BP2) and the SEM panel's.
  - No hidden gesture is added, so no new focus-band hint is needed. OUTPUT's band shows the standard MIX + SEQ learn hint.
  - FLT › MOD's other cells are CLEAN's (FM1 · FM2 · FM3 · FDBK · QM), and FLT › FX's F is STABILITY.
- **STABILITY** (supersedes in part ADR 0168, which acted on the four-pole kinds only): SEM is a chip a voice (`FilterKind::has_chip`). The spread is the same `g × cut` and `k × res` on the ramp ends, with `k` held at the circuit's own RES-1 floor (`Spread::k_damping`), so the spread never takes SEM below its ceiling. `STAB_COST` bills as on the 2040 and LADDER.
- **FB** runs through SEM's moving loop in bank 2: `g·2^(8·FB·y)`, held to the FREQ-tap range, the FB signal the SEM's own output (ADR 0190).
- **The rest, as every kind.** Keyboard tracking (FM2 on NOTE), FM1–FM3 and QM, and the matrix and MIDI routes are the fixed columns' and the registry's. A Q tap moves `k` linearly between RES 0's and RES 1's at 1 kHz (`sem_k_tap`). LEVEL is drive: its gain feeds the circuit and no clipper runs (`level_cost` 0, as the 2040 and LADDER). The LED reads A14's BP past a third of its swing (4.5 V) and is full at the swing (13.5 V).
- **The picture.** The FLT viz draws `(1 − m) + m·s²` over `s² + ks + 1` for MODE (`Response::SemMode`, `m` in 64ths, eased with the MODE cell) and `s/(s² + ks + 1)` for BP (`Response::SemBand`).
- **Cost** (`tools/loop_count.py` on the `filter-probes` ELF, `probe::sem_probe`):

  | Path | Instructions a sample | `vdiv` a sample | Cycles |
  |---|---|---|---|
  | MODE steady, diodes conducting (quiet) | 84 (77) + 1.83 | 0.062 | 140 (130) |
  | BP steady | 79 + 1.81 | 0.062 | 135 |
  | MODE moving, with `ramps`' fill | 91 + 2.30 + 6.25 | 1.047 | 175 |
  | MODE with FB | 135 + 1.67 + 6.25 | 1.047 | 245 |

  `SEM_COST` is 45 (140 over `OLD_LP24`'s 95), `SEM_MOVE_COST` is 35, and FB's 70 over the moving path is inside `FB_COST`'s 100. The glide's three extra divides a block fit the shared `glide_cost` 5. This is a static count, not yet read on the unit. The 2040 and LADDER read 60 or more over their static counts there (#642), so the bill should be re-read on the bench (private issue #726).
- **Voices**, beside the reserve (`sem_bills_its_count_and_its_voices`), rev V (480 MHz) / rev Y (400 MHz):

  | Kind | ALGO INIT steady | moving (CUTOFF LFO) | VA INIT steady | moving |
  |---|---|---|---|---|
  | CLEAN | 7 / 5 | 6 / 5 | 8 / 6 | 7 / 6 |
  | 2040 REV2 | 6 / 5 | 6 / 4 | 7 / 6 | 7 / 5 |
  | LADDER | 6 / 4 | 5 / 4 | 6 / 5 | 6 / 4 |
  | **SEM** | **7 / 5** | **6 / 5** | **8 / 6** | **7 / 5** |

- **Memory.** The voice grows 32 B (6,544 → 6,576 B): SEM's two states, its FB signal and its last block's ends (28), plus the played Sound's MODE knob. `.ram_d2` grows 440 B and `.bss` 72 B. `MAX_SOUND_COST` goes from 2,640 to 2,656 B for the 277th param.

## Fidelity of the port
`the_firmware_is_the_labs_model` runs six of the lab's fixtures (`semc-fixtures`, `chimera-core/tests/fixtures/sem_fw_v1.bin`) through the real `Filter`: bass at RES 0.9, brass BP at RES 1 and +12 dB, the MODE sweep, the pad at −12 dB, the cutoff sweep at RES 1 and +12 dB, and the RES sweep. Each nulls the lab's `fw::Fw` at −103 to −138 dB, against a −90 dB bound. What remains is f32 against the lab's f64 coefficients and `tan_accurate` against `tan`, so the circuit-level figures above carry over unchanged.

## Alternatives considered
- **BP as MODE's end** (MODE's last detent past HP): MODE is a smoothed, modulatable knob. A route or a glide into the end would switch the output, a crossfade mid-sweep, and MODE's range would be split between a mix and a switch. Rejected for its own cell.
- **OUTPUT on FLT › FX or FLT › JACK:** FX's F is STABILITY, and JACK is patching, not sound (ADR 0190). FLT › MOD's F was the free cell.
- **Spreading RES's law** (an exponential Q over the knob): the lab's open question 1. The owner kept the circuit's.
- **The couplings as one-poles in the loop:** +15 cycles for sub-20 Hz phase, which the voice's blocker already covers.
- **The lab's "exact" variant:** a divide a sample for no measured gain.
- **Compare-and-select holds:** 3 instructions a hold here against `clampf`'s 2. The lab's advice came from a probe crate built without the M7's FPv5 `vmaxnm`.
- **SEM in bank 1:** it would add about 6 KB to the ORBIT build's bank 1 (bank 2 grew 5,856 B).
- **An output trim at high RES like CLEAN's:** not built. The owner judged the circuit's levels on RMS-matched files, so the absolute level is a question for his ear (Consequences).

## Consequences
- A fifth TYPE on every engine. Its TYPE detent moves the MIDI map's stepped bins for TYPE (`stepped_cc_hysteresis`).
- **Flash** (ORBIT build, against origin/main 9664ee54): bank 1 is 883,928 → 885,816 B (+1,888; 31,688 B under the SD-flash limit) and bank 2's `.bk2` is 178,048 → 183,904 B (+5,856). Bank 1's share is the TYPE's UI (the panel, the viz, the header), the param and `Sem::process`'s door call.
- **Levels** against the other kinds (`per_pair_levels`, a held saw at C3, FREQ 1 kHz, RMS against CLEAN LP4 at RES 0):
  - MODE (LP) is +0.1 dB at RES 0 and +1.5 at ½.
  - At RES 1 it is +5.4 dB, the hottest pair in the table (CLEAN trims its RES 1; the 2040 REV2 reads +1.6), with peaks at −4.9 dBFS.
  - BP is −12.8 dB at RES 0 (Q 0.33; CLEAN BP4 reads −15.4, DIRTY BP2 −16.8), −8.1 at ½ and +3.1 at 1.
  - Whether SEM wants a high-RES trim is for the owner's ear (private issue #727).
- **QA** (`sem_controls_qa`): every control was turned a detent a block across its range and back on a held saw, then jumped end to end. MODE, FREQ, RES, INPUT, STABILITY, FDBK, MODE at RES 0.95 and OUTPUT are each finite, under the ceiling, freed, with no click at the jump. No turn raises the high band over the brighter of its ends (no zipper).
- **Goldens.** `flt_sem`, `flt_mod_sem` and `flt_sem_bp` are new. `sound_browser_pool_full` is re-recorded because a dense Sound is 5 B larger, so the pool's last free reads 0.2K, not 0.1K. No other screen changed.
- **Supersedes in part** ADR 0168's scope (STABILITY on the four-pole kinds only) and ADR 0190's FLT › MOD row (F empty): SEM's F is OUTPUT.

## Sources
- L. Köper, M. Holters, F. Esqueda, J. D. Parker, "A Virtual Analog Model of the EDP Wasp VCF", *Proc. DAFx20in22*, Vienna 2022, pp. 17–24, licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Used for the method (one nodal system, Newton) and its measured CA3080 fit (α 0.8635, β 0.9408, Table 3), which sets V0. No text or code was copied.
- Oberheim Electronics, *SEM-1A schematic*, VCO/VCF/VCA/ENV/LFO/PSU sheets (PSU 11 Nov 1975, rev. 5-30-78), a service document: part values and topology only.
- Datasheets, facts only: Intersil CA3080/CA3080A FN475; 2N4302 (onsemi PN4302 listing); the 1N4148 SPICE card; National LM301A and 741 typicals.
- V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.0 (NI 2018), § 4.4 (the TPT SVF), § 6.9 (the OTA) and § 6.11 (the antisaturator): the firmware model's frame.
- The lab: branch `sem-circuit-lab` (`chimera-utils/src/sem_circuit/`: `reference.rs`, `validate.rs`, `fw.rs`, `ab.rs`, `export.rs`), ledger (private notes) (`notes.md`, `research.md`, `ab/table.md`). Epic #720.
- ADRs 0060, 0096, 0101, 0104, 0117, 0155, 0157, 0167, 0168, 0190; issue #642 (the unit's re-read of the filter bills).
