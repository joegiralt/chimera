# 0158. VA oscillators read a modulated phase beside a clean clock

- **Status:** Proposed
- **Deciders:** owner (the VA MODEs plan, 2026-10-19, issues #601-#605), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) (adds a second phase to `VarOsc`; its rules for plain renders stand)

## Context
Three new VA MODEs move where an oscillator reads its wave: FM through zero (the read runs backward while the modulator is deep), PHASE (an offset added to the read) and SOFT SYNC (the read reverses at the other oscillator's wraps, mirrored sync). `VarOsc` had one phase. It times the wraps, the sub's flips, SYNC's marks and STABILITY's jitter draws, and it holds the pitch FM centres on. Modulating that phase would carry all of these with it: a read running backward wraps backward, and the sub and SYNC's marks would follow the modulator, not the note. The edges' crossings were also timed with one rate a block (`1/r` from the block's middle increment), which is wrong when the rate moves every sample and changes sign.

## Decision
1. **Two phases.** `VarOsc` keeps its clock (`phase`) and adds a read phase (`read`, 0..=1) and its direction (`dir`, ±1). The clock runs as before in every render: wraps, the `REC` and `FLIP` marks, the jitter's draws. The edges' phases since them (`u`) and the sine follow the read.
2. **A `MOD` instance.** `render` takes a fifth const generic, `MOD`, and the modulation `m` (a sample each) with its form `k`. Sample `i`'s read rate is `rr = ie·(dir + k.0·m[i]) + k.1·m[i]` (`ie` the jittered increment): FM is `k = (1, 0)`, PM `(0, 1)`. A plain instance (`MOD = false`) ignores `m` and `k` and is the code it was, plus one store at each run's end: `read = phase`. `NEW`, `reset` and `set_phase` put the read on the clock, forward.
3. **Wrapped to the nearest cycle.** `rr` is wrapped to `rr − ⌊rr + ½⌋`, in −½..½, before it moves the read (`read = frac(read + rr)`) and the edges. A whole cycle's jump is no motion for a periodic wave, so a saw modulator's drop never asks an edge to cross twice in one sample, and `u` keeps its one-crossing-a-sample rule.
4. **The rate a crossing.** A `MOD` crossing is timed with the sample's own rate past the edge, `t = t0 + over/(rr − od)`, and a corner's BLAMP takes `|rr − od|/6`: a divide a crossing, none a sample. The block's `1/r` table is skipped in a `MOD` instance. With no modulation the per-crossing rate is within 3·10⁻⁷ of plain (ruling R1's gate is 10⁻³).
5. **The reversing turn.** With `MOD` and `SYNC`, a mark `t` samples ago is a turn, not a reset: the read and the edges move `rr·(1 − t)`, their crossings timed from `t0 = t`; at the turn the wave's slope a cycle there (`A::slope + B::slope` at the read) times the rate's jump is a BLAMP at `t`; `dir` flips; the read and the edges move the new rate's `t` (crossings from `t0 = −0`). Unmodulated the jump is `−2·dir·ie`. Nothing resets: the turn is continuous in value, so it needs a BLAMP and no BLEP.
   - *Two details chosen here:* the edges' own place motion (`od`) is split between the two legs (`(rr − od)·(1 − t)`, then `(rr' − od)·t`), so each leg's crossings are timed with the motion they made; SYNC's reset branch takes `od` whole before its reset, which its recomputed `u` absorbs, but a turn keeps `u`. And the slope jump is `d·(rr' − rr)`, the rate after the turn less the rate before, which is `−2·dir·ie·d` when `k = (0, 0)` (SOFT SYNC) and stays exact when a test adds `k = (0, 1)` (`tick_mod`).
6. **`settle` at the duck's swap.** `settle` moves the clock to the read (`phase = read`, `dir = 1`); the edges' `u` are the read's already, so they stay. `VaEngine::render` calls it on both oscillators where the MODE duck swaps `playing`, under the duck's zero gain, so a pair leaving a `MOD` MODE reads one phase again. For the plain MODEs it is a no-op (`read == phase`).
7. **Separate monomorphs.** `MOD` instances are their own code: MIX, SYNC, RING, XOR and BITS keep their loops, their bills and their bits (`golden_test` and every `va_*` test unchanged). No MODE renders `MOD` yet; the plain call sites pass `ZEROS` and `(0, 0)`.

## Alternatives considered
- **Modulate the clock itself.** One phase is cheaper, but the sub, SYNC's marks and the jitter would follow the modulator, a through-zero read would wrap backward under `REC`, and FM's carrier would lose the pitch it centres on.
- **The block's rate for `MOD` crossings.** The rate moves every sample under FM and changes sign through zero: the block's `1/r` puts `t` outside the sample, or on the wrong side.
- **A divide a sample.** Exact, but a `MOD` loop would pay ~12 cycles every sample for what only a crossing needs.
- **A step (BLEP) at a turn, or a reset.** A turn leaves the value where it is; only the slope reverses. A reset is hard sync, which SYNC already is.

## Consequences
- FM, PHASE and SOFT SYNC (#601-#605) build on one oscillator path; each picks its `k` and its `m`.
- Measured on the test bench (`chimera-core/tests/va_osc_mod_test.rs`): MOD with `m = 0` against plain, 21 WAVE x SHAPE points at C6 for 2,000 blocks, at most 2.98·10⁻⁷; a backward read at C6 (m = −2) aliases −55.4 dB at worst over GRID x SHAPE {0, ½}, equal to forward, in tune within 0.01 cent; FM through zero (index 3, f_m = f_c = C5) −49.3 dB at worst against −18.8 dB uncorrected, at least 21 dB better on every edged WAVE; a turn every 37.3 samples logs its BLAMP within 10⁻³ of `d·(rr' − rr)` and retraces the read within 10⁻⁴; a settled pair matches a fresh oscillator within 6·10⁻⁸ (unsettled, 9·10⁻²).
- Flash: bank 1 free 67,064 B → 66,928 B (+136 B, the stores, `settle` and the plain call sites' extra arguments); no `MOD` code ships until a MODE uses it.
- The clock jumps at the swap: the sub may skip or repeat one flip there, under the duck's zero gain.
- A `MOD` MODE's bill is its own count (ADR 0118 point 9) with a divide (+12) at each crossing's event rate.

## Sources
- The plan: `docs/superpowers/plans/2026-10-19-va-modes.md` (Task 1); rulings R1 (tolerances).
- Esqueda, Välimäki & Bilbao, "Rounding Corners with BLAMP", DAFx-16, 2016 (the turn's BLAMP).
- Välimäki & Huovilainen, "Antialiasing oscillators in subtractive synthesis", IEEE SPM 24(2), 2007 (polyBLEP at a crossing's time).
- Roth, Keller, Castañeda & Studer, "Alias-free oscillator synchronization via additive synthesis", DAFx-26 paper 49, CC-BY 4.0 (the mirrored-sync definition only; their additive method is not used).
- ADR 0118 (the oscillator's rules).
