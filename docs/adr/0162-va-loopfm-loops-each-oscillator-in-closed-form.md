# 0162. Run VA MODE LOOPFM as each oscillator's closed-form loopback FM

- **Status:** Proposed
- **Deciders:** owner (the VA MODEs plan, 2026-10-19, issue #613), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) points 1 (LOOPFM joins the operators), 3 (its alias gate) and 10 (its bill)

## Context
Issue #613 asks for one VA MODE where turning BLEND mixes OSC 1 and OSC 2 while each loops back on its own frequency (loopback FM: an oscillator whose real part modulates its own instantaneous frequency by a coefficient `B`, −1 < `B` < 1). Smyth (DAFx-26, paper 36, CC-BY 4.0) gives two forms: a sample-by-sample rotation with a unit delay in the loop (eq. 11), and a closed form, `z(n) = (b0 + e^(jω0 nT)) / (1 + b0·e^(jω0 nT))`, with `b0 = (√(1 − B²) − 1)/B` (eq. 26) and the sounding frequency `ω0 = ωc·√(1 − B²)` (eq. 12). The closed form has no delay and is a function of the sounding frequency, so the note stays in tune as `B` moves. The shape runs from a cosine at `B = 0` to a sharp, pulse-like wave as `|B|` nears 1. The open question was what WAVE and SHAPE mean when the oscillator core is a sinusoid in loopback form.

## Decision
1. **The MODE.** LOOPFM is `VaMode::LoopFm`, ident `LOOPFM` (shown `LOOPFM`, the owner's name), appended; disk code 8 (after MAX, 7). Nothing is renumbered.
2. **The closed form, not the recursion.** Each oscillator evaluates `Re z0 − b0` at its clock phase (`lbfm::lbfm`): one divide a sample, no unit delay, the pitch exact (the form is periodic in the clock). `Re z0 = (2b0 + (1 + b0²)·cos 2πφ)/(1 + b0² + 2b0·cos 2πφ)`; subtracting `b0` (its mean) leaves harmonics `(1 − b0²)·(−b0)^(k−1)`, scaled by `R·√2/√(1 − b0²)` to RMS `R`, DC free. Measured against the paper's recursion at 16× 48 kHz (A2, `B = 0.5`, phase and period aligned): 0.031 % RMS apart. In tune: 440.000 Hz (+0.001 cent) at BLEND {0, ½, 1} × WAVE {0, ⅔, 1}. Mean ≤ 5·10⁻⁶, RMS 1.0000–1.0001 R at `B` {0, 0.5, 0.95}.
3. **The clock stays in `VarOsc`.** `render_loop` runs on `VarOsc::phase` and writes it back (`set_phase`, which re-anchors the edges for whichever MODE takes over); the carry, STAB 1's reset, the jitter's draws and the sub (flipping at OSC 1's wraps) work as in MIX. The MODE change goes through the existing duck.
4. **BLEND both mixes and loops.** The operator is `a + (b − a)·BLEND/2`: OSC 1 alone at 0, an equal share at 1 (OSC 2 never louder than OSC 1). `B = 0.95·BLEND²` (`loop_b`), so the loop opens slowly and BLEND 0 is a clean sine at `R` (level against MIX at BLEND 0: 0.00 dB).
5. **WAVE scales each oscillator's own `B`** (the owner's suggestion was an offset; a scale keeps BLEND 0 clean for every WAVE): `B_k = min(0.95, 0.95·BLEND²·1.5·WAVE_k)`: WAVE 0 never loops, WAVE ⅔ (the default) is ×1, WAVE 1 is ×1.5 capped at 0.95. The two oscillators therefore loop by different amounts and beat against each other. SHAPE is unused (the core is a sinusoid in loopback form, not a morphing wave).
6. **`b0` keeps its sign** (ruling R3): `b0 = b0_of(B)` is negative for `B > 0`, the paper's + root; its magnitude is capped, never its sign, so the wave does not flip polarity under the cap.
7. **The pitch-dependent `b0` cap** (`b0_cap`). Harmonic `k` is `(1 − b0²)·|b0|^(k−1)` of the wave; with `K = 0.5/inc` harmonics under Nyquist the first alias is held to −45 dB (0.0056) by `|b0| ≤ exp(ln(0.0056)/(K − 1))`, 0 when `K ≤ 1`. It is taken at the block's faster end of the increment ramp, and `b0` is ramped per block (`lb`), the ramp's start clamped to this block's cap. It binds only above about F6 (1.4 kHz): the full `B = 0.95` has `|b0| = 0.724`, and the cap is 0.89 at C5, 0.79 at C6, 0.61 at C7.
8. **The alias gate** (amends 0118 point 3). Harmonic-lattice gate (`alias_db`) at BLEND 1, WAVE 1 (the deepest loop): −88.8 dB at C5, −87.9 dB at C6, −77.3 dB at C7 (the closed form is band-limited by its geometric spectrum, no BLEP is needed); the gate is −45 dB.
9. **The scope.** MODE's glyph evaluates `lbfm` at both phases (OSC 2 at `VA_MIX_RATIO`) with the engine's `b0` uncapped (`inc = 0`), mixed as the operator mixes; BLEND's ends read `1` and `LOOP`. The word `LOOPFM` is too wide for the large numerals beside the scope, so a MODE name that does not fit there reads in the value font with the scope kept (`Layout::SmallBeside`, VA's MODE scope only).
10. **The bill** (amends 0118 point 10). By ADR 0118 point 9's count (`count-loopfm/bills2.py`, `loopfm.out`): OSC 1's `render_loop` 84.7 (its round with the divide, the wrap at the 0.45 cap with the jitter's draw and the `t` divide, 12 a divide; the output scale ramps per block with `b0`, one root a block), OSC 2's 70.1, operator 34.5, sub 3.3, block 51.4, WAVE's bound splits 8.9, LP24 95: **347.9, bill 350** (`COST_LOOPFM`; no VarOsc run, so none of its crossings). With INIT's +98 a voice is 448: VOICES_FROM_COST. The other MODEs' bills are unchanged.

11. **Bank 2** (ruling R12, ADR 0163's recipe). `VaEngine::render_loopfm` and `lbfm::render_loop` are bank 2's: a `VaDoor::loopfm` field (`LoopFmRender`), `render`'s LOOPFM arm through `DOOR_CELL.with`, one glob in `bank2.x` (`*4lbfm11render_loop*`, `*8VaEngine13render_loopfm*`, not the whole `lbfm` module, whose `lbfm`, `b0_of`, `loop_b` the scope glyph calls from bank 1), and `VaMode::in_bank2` takes LoopFm. Closed, a LOOPFM voice is silent (the duck runs on) and MODE reads `NEEDS UPDATE`. `just check-door-placed` also checks these symbols.

## Alternatives considered
- **The recursion (eq. 11).** A unit delay in the loop detunes the note with `B`; the closed form is exact and cheaper (no state).
- **Cross-loop** (OSC 1 modulating OSC 2's frequency and back). It has no closed form and would drift out of tune; out of scope (#613).
- **WAVE as an offset to `B`.** BLEND 0 would loop at WAVE ≠ 0; a scale keeps BLEND 0 a clean sine and WAVE 0 a clean sine at any BLEND.
- **WAVE as the carrier's shape.** The closed form is defined on a sinusoid; shaping it needs a second core and its BLEPs.
- **Capping `B` instead of `b0`.** `b0` is what sets the harmonic decay, so capping it gives the −45 dB bound directly, at the cost of a tiny loss of loop at the top notes.
- **A shorter name** to fit the large numerals. The owner named it LOOPFM; only the rendering changed.

## Consequences
- LOOPFM costs 350, less than any other MODE (no VarOsc edges to correct); six voices on rev Y hold with room to spare.
- The two oscillators at one pitch and equal `B` are one shared wave at BLEND 1 (`a + (b − a)/2`); detune or a different WAVE makes them beat.
- A pitch above about F6 loses loop depth (`b0` capped), never aliasing.
- The scope word may read in the value font beside the glyph (`LOOPFM`); no other page changes.
- Flash: `just firmware-orbit`, bank 1 free 51,724 B before, 48,156 B after (−3,568 B: two `render_loop` instances, the closed form, the `b0` mapping and cap, the scope and the layout fallback). Above the owner's ~1.5 KB rule: the controller decides whether LOOPFM goes behind the bank-2 door.
- WAVE's glyph still draws the oscillator shape under LOOPFM, although WAVE is the loop depth there: follow-up #667.
- The bench gains a `LOOPFM` row (BLEND 1, WAVE default); its unit reading is pending.

## Sources
- Tamara Smyth, "Loopback Frequency Modulation Using a Time-Varying Delay Line", DAFx-26, paper 36 (CC BY 4.0, https://creativecommons.org/licenses/by/4.0/; https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_36.pdf): §2.2 (eq. 11, 12), §2.4 (the closed form, eq. 25–26); credited in `THIRD_PARTY.md` and `lbfm.rs`'s `PROVENANCE`. Our own Rust.
- The plan: `docs/superpowers/plans/2026-10-19-va-modes.md` (Tasks 13 and 14); ruling R3, R7 (private notes).
- ADR 0118 point 9 (the count), ADR 0158 (the read phase and the clock), ADR 0159 (the scope's per-point dispatch), ADR 0040 (six voices on rev Y).
