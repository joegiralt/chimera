# 0160. Run VA MODE PHASE as OSC 2 offsetting OSC 1's phase

- **Status:** Proposed
- **Deciders:** owner (the VA MODEs plan, 2026-10-19, issue #602; ruling R15, a modulator's step is one step of the output), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) points 1 (PHASE joins the operators), 3 (PHASE's alias gate) and 10 (PHASE's bill); builds on [0158](0158-va-oscillators-read-a-modulated-phase-beside-a-clean-clock.md), [0159](0159-va-fm-runs-osc-1-through-zero.md) and [0163](0163-the-va-mod-oscillators-live-in-bank-2.md)

## Context
Issue #602 asks for phase distortion in the manner of Casio's CZ: OSC 2 warps OSC 1's phase and the result brightens like a filter sweep with no filter. Lazzarini et al. (DAFx-09) show phase distortion is phase modulation at index 2π(½ − d) for a distortion depth `d`, so PHASE needs no waveshaper of its own: ADR 0158's `MOD` render with `k = (0, 1)` moves OSC 1's read by the modulator's difference.

A saw or pulse OSC 2 (VA's default WAVE2 is a saw) steps. A step of the modulator is a jump of the read: up to `2D` cycles at one instant. Fed to the `MOD` read as a rate, the jump is a motion one or two samples long (OSC 2's own 2-point BLEP spreads its step over two samples): the read visits phases the continuous signal never has, the carrier's edges inside the jump are each corrected at times taken from the jump's rate, and wrapping that rate to the nearest cycle can read its direction wrong. Measured, a saw modulator aliased −4.8 to −36 dB, often worse than no correction at all.

## Decision
1. **The MODE.** PHASE is disk code 8 (FM 5, AM 6 and MAX 7 merged first), ident `PHASE` (`VaMode::Phase`), appended; nothing renumbered. It is shown `PHAS`: the focus font's `PHASE` is 17 px too wide beside the MODE scope (`values_stay_clear_of_the_glyphs`); ruling R14 shows the full word once LOOPFM's `SmallBeside` layout lands.
2. **The offset.** `D·b/pk.b` (cycles, `b/pk.b` in −1..1) is OSC 1's phase offset. The read moves by the offset's difference each sample (`k = (0, 1)`, the last sample's offset kept across blocks in `pm_prev`). Because the offset is periodic, the read's mean rate is OSC 1's note and the pitch is centred at every depth, ±0.003 cents measured.
3. **A step of OSC 2 is one step of the output** (ruling R15). The continuous signal is `y(τ) = W₁(φ(τ) + D·b(τ))`; at a modulator step at `τ₀` it jumps by `Δy = W₁(read after) − W₁(read before)` and is smooth elsewhere. So:
   - **OSC 2 marks its steps instead of correcting them** (`door::osc2_steps`, a plain render with `PH`): a step crossed `t` samples before its sample `i` is summed into `[Σ s·t, Σ s]` in place of its BLEP, and at the sample's end becomes a mark in OSC 1's next sample (OSC 2 is one sample late), `mk[i + 1] = [JUMP − 1 − t̄, Σ s]` (`t̄` step-weighted); the last sample's mark lands in `mk[0]` and is carried to the next block in `pm_step`. OSC 2's output is naive at its steps; its corners keep their BLAMPs. The marks are the modulator's own crossings: no threshold, no estimate.
   - **The read's rate leaves the steps out:** `render_phase` subtracts `x_i·Σ s` (`x` the offset's scale, `D/pk.b`) from each marked sample's difference (sparse: OSC 2 lists its marks).
   - **OSC 1 takes a jump at its mark** (`door::osc1_phase`, the `MOD` render with `PH`; FM's `osc1_mod` is untouched): before the marks are cleared, `door::jump` (out of line) moves the read and the edges up to the jump at the sample's rate, crossing and correcting edges there as any sample's; takes `y₀`; jumps the read and every edge's phase by `J = x·Σ s` and re-anchors them (`frac`; the pulse's level from its edges), no edge corrected on its own; takes `y₁`; adds one BLEP of `y₁ − y₀` at `t`; then sets the read back by the motion up to the jump, so the sample's own motion and crossings play the rest after it, timed as ever.
   - **At most `PM_JUMPS` = 6 a block.** Past them OSC 2 corrects its steps as ever and OSC 1 reads them as motion, as before this ruling; the budget bounds a block's work (the bill). Six covers a pulse modulator at 2.09 kHz (C6, CRSE2 +12: 5.6 steps a block).
   - A mark lies under `JUMP` (−2): a wrap's or flip's mark (−1..1) left from the last block is never one.
4. **The depth.** `D = BLEND/2` cycles peak (`phase_depth`), ramped each block (it shares FM's `fm` ramp: one MODE plays), held at the duck's swap. BLEND ½ is the CZ's full warp; the upper half goes past it, where a triangle modulator folds the wave over itself.
5. **Free-running, phase-locked at unison.** OSC 2 is a free oscillator like MIX's: at unison and STAB 1 both start at phase 0 and stay locked (the CZ's two-phase lock). OSC 2's WAVE and SHAPE choose the warp: a TRI at SHAPE 1 is the CZ's skewed triangle, a sine a soft bend, a saw the CZ's saw-resonance family. At the MODE duck's swap the offset starts at the first sample's own (`pm_prev = NaN`, `pm_step` cleared), not at zero: a zero start jumps the read by up to `D` cycles with the duck already rising (FM → PHASE clicked at 122 % of the click rule; none now).
6. **What is heard; the scope.** OSC 1 alone, through the sub and the chain gain as MIX; the sub flips on OSC 1's clean clock. BLEND's ends read `CLEAN` and `WARP`. MODE's glyph reads OSC 1 at `p1 + phase_depth(amount)·b/pk.b` (`va_glyph_test`: `phas_050`, `phas_100`).
7. **The alias gate** (amends 0118 point 3; `phase_alias_gate`). `alias_db_lattice(f_c, f_m)`, notes 72 and 84, CRSE2 0, 7 (3:2, FINE2 +1.955 cents) and 12, WAVE1 and WAVE2 over sine, triangle, saw and pulse, BLEND ¼, ½ and 1:
   - Sine and triangle carriers under a sine modulator, where Carson's bandwidth `(2π·D + 1)·f_m + f_c` fits under Nyquist: −45 dB; worst −49.7 dB (84 +12, triangle carrier, BLEND 1). Elsewhere under a sine modulator the correction wins 10 dB; least 17.6 dB (72 +7, saw carrier, BLEND 1).
   - A triangle modulator: −45 dB or 10 dB won; it passes everywhere. The sine and triangle modulators' cells are as before this ruling, to the printed tenth.
   - **A saw or pulse modulator: −45 dB everywhere.** 144 cells; worst −49.7 dB (84 +12, triangle carrier, pulse modulator, BLEND ½). The worst cell over carriers and ratios, dB, before → after:

     | modulator | note | BLEND ¼ | BLEND ½ | BLEND 1 |
     |---|---|---|---|---|
     | saw | C5 | −36.0 → −57.9 | −32.4 → −57.9 | −4.8 → −57.9 |
     | saw | C6 | −33.1 → −55.3 | −29.4 → −55.4 | −5.1 → −55.4 |
     | pulse | C5 | −35.0 → −56.1 | −28.7 → −54.2 | −16.5 → −57.9 |
     | pulse | C6 | −32.2 → −51.9 | −24.5 → −49.7 | −12.9 → −55.4 |

   - A BLAMP of the slope's change at the jump (`(W₁′ after − W₁′ before)·rr`) was measured and left out: it moves the worst cell 1 dB (−49.7 → −50.7) and the median not at all, for two more evaluations a jump.
   - Measured and left out too: the jump taken at the sample's end after its crossings (−27.0 dB worst), and without the crossings up to it (−33.1 dB): the crossings on each side of a jump must be timed on their own side. With `PM_JUMPS` 4 the C6 +12 pulse cells fall to −15.6 dB: past the budget the old path is worse than none, so the budget covers the steps a block of every gated modulator.
8. **The bill** (amends 0118 point 10). By point 9's count on the merged tree (`bills2.py` with roles `phase_master` and `steps`; (private notes)): OSC 1's PHASE instance 223.5 (FM's is 209.3: the jump's test and its call's registers), OSC 2 marking its steps 142.9 (plain 134.4), operator and sub 29.4, block 154.5 (6.3 of it the offset loop; 33.5 six jumps and marks a block, a jump billed at its call less the crossings up to it, 316.4: each edge crosses once a sample, before the jump or after it, and its events bill that crossing every sample), crossings 31.3, LP24 95: 676.5 against MIX's 559.9 in the same count, **bill 650** (`COST_PHASE`, from 580). A voice: 6 on rev V, **5 on rev Y**, 6 beside ORBIT, 6 with the master tape (FM: 7, 5, 7, 6).
9. **Flash.** Both PHASE oscillators are the door's (ADR 0163: entries `osc1_phase` and `osc2_steps`, their runs `door::run`'s, and `door::jump`), so with `bank2` they play from bank 2 and, closed, PHASE is silent and reads `NEEDS UPDATE` (`VaMode::in_bank2`). ORBIT build: bank 1 free 48,792 B → 47,856 B (−936 B); `.bk2` 128,192 B → 145,408 B (+17,216 B). FM's instance and every plain one keep their code and bits.

## Alternatives considered
- **A fixed CZ knee (a waveshaper).** One resonant shape per setting; OSC 2's WAVE and SHAPE give the whole family and the same loop serves.
- **Resetting OSC 2 at OSC 1's wraps** (a true CZ's single phase). It costs the SYNC slave's `FLIP`-less monomorph for a difference only off unison.
- **Starting the offset at zero at the swap.** Clicks (point 5).
- **PHASE as FM with `k = (1, 0)`.** That is FM (ADR 0159): it moves the rate, not the phase.
- **A jump found by a threshold on the read's rate.** A naive modulator loses the step's time inside its sample, and a corrected one spreads it over three; OSC 2 knows it exactly at its crossing.
- **OSC 2 reporting from the plain instance** (a runtime test in `correct`). A test a crossing in every MODE's OSC 2: RING and BITS (529.3 and 529.4 of 530) would bill 540.
- **Sharing FM's `MOD` instance.** The jump's test and its call's register pressure cost FM 14 a sample (209.3 → 223.5); PHASE's own instance keeps FM's code and bill, in bank 2, where flash is not short.

## Consequences
- PHASE plays one voice fewer than FM on rev V and beside ORBIT (650 against 590).
- A stepped modulator past six steps a block (a saw over about 4.5 kHz, a pulse over about 2.25 kHz, a SHAPEd saw or a saw–pulse mix lower) is read as motion again for the rest of the block, with the alias of before this ruling. A cheaper jump would let the budget grow (follow-up issue, filed by the controller).
- The upper harmonics (2nd–8th) rise monotonically as BLEND goes 0 → ½ with a sine carrier and a TRI SHAPE 1 modulator, the CZ's sweep.
- A locked 1:1 saw carrier under a saw modulator freezes the read between wraps at BLEND 1 (the offset cancels the clock): a pulse train, now band-limited like any other step.
- The level matches MIX within 0.5 dB at INIT's tuning and at a fifth (`phase_level_matches_mix`).
- The bench's `PHASE` row (BLEND 1, OSC 2 CRSE 0) plays from bank 2 in the bench sets with `bank2`; its unit reading is pending.

## Sources
- The plan: `docs/superpowers/plans/2026-10-19-va-modes.md` (Tasks 5 and 6); rulings R14, R15; `va_modes_test` (the alias table, printed).
- Lazzarini, Timoney, Ward & Pekonen, "Phase Distortion Synthesis" (DAFx-09): phase distortion as phase modulation. The CZ's behaviour is described from its manual; no code.
- Välimäki & Huovilainen, "Antialiasing oscillators in subtractive synthesis", IEEE SPM 24(2), 2007 (the polyBLEP at a step's time).
- Carson's bandwidth rule (textbook).
- ADR 0118 point 9 (the count), ADR 0158 (the read phase), ADR 0159 (FM), ADR 0163 (the door), ADR 0040 (voices).
