# 0313. Correct AM's product at its oscillators' steps, in bank 2

- **Status:** Accepted (2026-11-22)
- **Deciders:** Claude (the 0.4 bug sweep, batch D2: no known bug rolls to 0.5); the owner hears the before/after renders

## Context
VA MODE AM multiplies two band-limited waves: `K·a·(c₀ + c₁·b)` (`Operator::at`, ADR 0176). Each oscillator corrects its own steps with a 2-point polyBLEP, but the product of two corrected waves is not a corrected product. Where one wave's step meets the other's ramp, the product's slope jumps, and nothing bends it. Where both step together (a fifth's every other cycle, unison's long stretches), the two residuals multiply into a deep dip. Saw × saw at the just fifth, BLEND 85, read −43.5 dB of alias against the −45 dB gate. The gate held it as an exception (#660).

## Decision
- **Steps listed, not corrected:** in AM, both oscillators render with their steps naive and listed (`(slot, t, Σ)`), up to `PM_JUMPS` (6) a block each; past that budget, an oscillator corrects a step itself as ever. OSC 2 uses PHASE's instance (`door::osc2_steps`). OSC 1 uses a new one with its sub's flips (`door::osc1_steps`): with `FLIP` it lists its steps without marking them in `mk`, which carries the sub's flips. `VarOsc::pm_val` holds each listed step's mark beside `pm_at`.
- **The product corrected, one sample late:** `combine::AmKinks` takes each listed step and gives the product its own step (polyBLEP) and slope jump (polyBLAMP), by the product rule across a step. At OSC 1's `Δa` the product steps by `K·Δa·(c₀ + c₁·b)` and bends by `K·c₁·Δa·b'`; at OSC 2's `Δb`, by `K·c₁·a·Δb` and `K·c₁·Δb·a'`. Each wave is read at the other's step on its own last slope, past its own step if that came first (the larger `t`). A slope leaves out a step listed between its two samples. The last sample's steps carry to the next block's first. AM plays one sample later than before, the sub on the clock, as MAX's corrector does.
- **Bank 2:** the whole render is `VaEngine::render_am_kinks`, behind `VaDoor::am` and linked into bank 2 (`*8VaEngine15render_am_kinks*`). With the door closed, AM plays bank 1's uncorrected product, as before. With no `bank2` feature it is called directly.
- **The saw's solo** (ADR 0283) no longer plays under AM: its oscillators are always the listing pairs.
- **The bill:** `COST_AM` goes 550 → 590. By `tools/loop_count.py` on the release ELF, the operator's round is 55 instructions a sample against 41, a listed step's slow round adds 80 at up to six a block, and the two listing instances add 8.5 each.

## Alternatives considered
- Correcting the product from the signal alone, as MAX's kink corrector does: a step's place and size can't be read back reliably from two BLEP'd waves.
- Marking OSC 1's steps in `mk`: they would overwrite the sub's flips at a coincident sample.
- A wider kernel (4-point polyBLAMP, #451): it changes every corrector and the latency the tests assume.

## Consequences
AM is cleaner. At the fifth, saw × saw reads −58.8 dB at BLEND 85 (from −43.5), and AM now passes −45 dB everywhere MIX does. The gate's exception is gone. AM's goldens moved, and only AM's: `va_modes_test` (`am_below_the_knee_is_todays` is one sample late, Σy² within 0.5 %; `am_top_is_ring` agrees with RING's uncorrected product to 1 %), `vca_lpg_test`'s hash, VA's noise hash, AM's five `OTHER` prints, and the solo fuzz's digests where a scene plays AM. AM is a sound change for the owner's ear (private notes).

## Sources
private issue #660; ADR 0176 (AM), 0160 (PHASE's step marks), 0283 (the saw's solo), 0163 (VA's door); Välimäki & Huovilainen 2007 (polyBLEP); Esqueda, Välimäki & Bilbao, DAFx-16 (polyBLAMP at corners).
