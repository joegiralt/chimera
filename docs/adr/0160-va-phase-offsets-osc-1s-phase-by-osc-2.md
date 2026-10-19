# 0160. Run VA MODE PHASE as OSC 2 offsetting OSC 1's phase

- **Status:** Proposed
- **Deciders:** owner (the VA MODEs plan, 2026-10-19, issue #602), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) points 1 (PHASE joins the operators), 3 (PHASE's alias gate and its exceptions) and 10 (PHASE's bill); builds on [0158](0158-va-oscillators-read-a-modulated-phase-beside-a-clean-clock.md) and [0159](0159-va-fm-runs-osc-1-through-zero.md)

## Context
Issue #602 asks for phase distortion in the manner of Casio's CZ: OSC 2 warps OSC 1's phase and the result brightens like a filter sweep with no filter. Lazzarini et al. (DAFx-09) show phase distortion is phase modulation at index 2π(½ − d) for a distortion depth `d`, so PHASE needs no waveshaper of its own: ADR 0158's `MOD` render with `k = (0, 1)` moves OSC 1's read by the modulator's difference, FM's instance with a different `k`. Reserved disk code 6, assigned at merge (the branch appends it after FM).

## Decision
1. **The MODE.** PHASE is disk code 6 (reserved; assigned at merge), ident `PHASE` (`VaMode::Phase`), appended; nothing renumbered. It is shown `PHAS`: the focus font's `PHASE` is 17 px too wide beside the MODE scope (`values_stay_clear_of_the_glyphs`), and shrinking the scope would redraw every MODE's golden.
2. **The offset.** OSC 2 renders plain; `D·b/pk.b` (cycles, `b/pk.b` in −1..1) is OSC 1's phase offset. The read moves by the offset's difference each sample, `m[i] = D_i·b_i/pk.b − prev` (`k = (0, 1)`, `prev` the last sample's offset, kept across blocks in `pm_prev`). Because the offset is periodic, the read's mean rate is OSC 1's note and the pitch is centred at every depth, ±0.003 cents measured.
3. **The depth.** `D = BLEND/2` cycles peak (`phase_depth`), ramped each block (it shares FM's `fm` ramp: one MODE plays), held at the duck's swap. BLEND ½ is the CZ's full warp; the upper half goes past it, where a triangle modulator folds the wave over itself. A saw modulator's step is a whole cycle at BLEND 1 and the read's rate wraps to the nearest cycle (ADR 0158), so there it is no step at all.
4. **Free-running, phase-locked at unison.** OSC 2 is a free oscillator like MIX's: at unison and STAB 1 both start at phase 0 and stay locked (the CZ's two-phase lock). OSC 2's WAVE and SHAPE choose the warp: a TRI at SHAPE 1 is the CZ's skewed triangle, a sine a soft bend, a saw the CZ's saw-resonance family. At the MODE duck's swap the offset starts at the first sample's own (`pm_prev = NaN`), not at zero: a zero start jumps the read by up to `D` cycles in a sample with the duck already rising (measured: FM → PHASE clicked at 122 % of the click rule; none now).
5. **What is heard.** OSC 1 alone, through the sub and the chain gain as MIX. The sub flips on OSC 1's clean clock and stays in tune. BLEND's ends read `CLEAN` and `WARP`.
6. **The scope.** MODE's glyph reads OSC 1 at `p1 + phase_depth(amount)·b/pk.b` (`va_glyph_test` locks the scope at BLEND 0.5 and 1: `phas_050`, `phas_100`).
7. **The alias gate** (amends 0118 point 3). `alias_db_lattice(f_c, f_m)`, notes 72 and 84, CRSE2 0, 7 (3:2, FINE2 +1.955 cents) and 12, WAVE1 and WAVE2 over sine, triangle, saw and pulse, BLEND ¼, ½ and 1:
   - Sine and triangle carriers under a sine modulator: −45 dB where Carson's bandwidth `(2π·D + 1)·f_m + f_c` fits under Nyquist. Worst measured −49.7 dB (84 +12, triangle carrier, BLEND 1).
   - Saw and pulse carriers, and triangle or pulse modulators where it does not fit: the correction must win 10 dB over the uncorrected render; the least is 17.6 dB (72 +7, saw carrier, triangle modulator, BLEND 1).
   - **A saw modulator is printed, not gated.** Its step moves the phase by up to a cycle in one sample, which no kernel corrects (the carrier's edges only follow the read's rate): the 2-point band-limiting of OSC 2's own edge leaves fractional offsets on the step's two samples, so the read takes real, uncorrected steps, and the uncorrected render is often exact (it steps by exactly whole cycles). Measured −4.8 to −33.7 dB with the saw modulator at BLEND 1 (and BLEND ½ at −29 to −32 dB), against uncorrected −20 to −148 dB. Follow-up: a step-size correction on the carrier at the modulator's edge (to be filed with the controller).
8. **The bill** (amends 0118 point 10). By point 9's count (`bills2.py`, (private notes)): OSC 1's MOD instance 209.3, OSC 2 plain 134.4, operator and sub 29.4, block 76.4 (6.3 of it the offset loop), crossings 30.0, LP24 95: **574.5, bill 580** (`COST_PHASE`). With INIT's +98 a voice is 678: 8 on rev V, 7 beside ORBIT, 7 with the master tape, **6 on rev Y**, so ADR 0040's six voices holds in PHASE.
9. **Flash.** PHASE reuses FM's OSC 1 `MOD` instance and the plain OSC 2 instance by sharing one jitter-draw closure type across every MODE (`jitter_draw`): bank 1 free 51,724 B before, 50,324 B after (−1,400 B).

## Alternatives considered
- **A fixed CZ knee (a waveshaper).** One resonant shape per setting; OSC 2's WAVE and SHAPE give the whole family (TRI SHAPE 1 is the CZ's skewed triangle) and the same loop serves.
- **Resetting OSC 2 at OSC 1's wraps** (a true CZ's single phase). It costs the SYNC slave's `FLIP`-less monomorph (flash) for a difference only off unison; at unison the two are locked already.
- **Starting the offset at zero at the swap.** Clicks (point 4).
- **PHASE as FM with `k = (1, 0)`.** That is FM (ADR 0159): it moves the rate, not the phase; the two sound different (the read's mean rate, the pitch under a deep modulator).

## Consequences
- Rev V keeps eight voices in PHASE (580 + 98); rev Y keeps six.
- The upper harmonics (2nd–8th) rise monotonically as BLEND goes 0 → ½ with a sine carrier and a TRI SHAPE 1 modulator, the CZ's sweep.
- A locked 1:1 saw carrier under a saw modulator freezes the read between wraps at BLEND 1 (the offset cancels the clock): a pulse train, aliased as point 7 says.
- The level matches MIX within 0.4 dB at INIT's tuning and at a fifth (`phase_level_matches_mix`).
- The bench gains a `PHASE` row (BLEND 1, OSC 2 CRSE 0); its unit reading is pending.

## Sources
- The plan: `docs/superpowers/plans/2026-10-19-va-modes.md` (Tasks 5 and 6); `va_modes_test` (the alias table, printed).
- Lazzarini, Timoney, Ward & Pekonen, "Phase Distortion Synthesis" (DAFx-09): phase distortion as phase modulation. The CZ's behaviour is described from its manual; no code.
- Carson's bandwidth rule (textbook).
- ADR 0118 point 9 (the count), ADR 0158 (the read phase), ADR 0159 (FM), ADR 0040 (six voices on rev Y).
