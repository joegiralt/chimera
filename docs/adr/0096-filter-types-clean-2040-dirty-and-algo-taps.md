# 0096. Three filter TYPEs, CLEAN (Cascadia-style), 2040 (SSM2040) and DIRTY (Chamberlin), and ALGO's OP 5/6 audio-rate taps (amends 0037)

- **Status:** Proposed
- **Deciders:** owner (direction and rulings, 2026-10-05), firmware (after adversarial review)

## Context
The owner wants a home filter with the Intellijel Cascadia VCF's UX, built on today's ZDF/TPT SVF ("reminds me of a 101"). Two more machines are wanted: the SSM2040, and a cheap, edgy Chamberlin SVF.

Today's SVF falls short of that in several ways:
- PHASER has no notches.
- NOTCH sits at FREQ.
- LP6 ignores resonance.
- The SVF never self-oscillates.
- It divides twice a sample.
- It reads its top octave low (#57).

The owner also wants ALGO's OP 5 and OP 6 as audio-rate modulation sources. The matrix is full (8 rows) and runs at block rate.

## Decision
- **Types and modes.** KIND shows as TYPE. Mode labels and `default_mode()` belong to each type (amends 0037's "`modes()[0]` is the default"). Disk codes are identity; only labels change. A TYPE or MODE change warm-starts the incoming state from the outgoing outputs and crossfades over two blocks, unbilled.
  - **CLEAN** (`Svf`, code 0): LP1 LP2 **LP4** BP2 BP4 HP4 NOTCH1 NOTCH2.
  - **2040** (code 4): LP4 only.
  - **DIRTY** (code 6): **LP2** BP2 HP2 NOTCH1. HP2 is `Hp12`, code 9.
- **The default TYPE follows the engine.** ALGO, MODAL and TAPE default to CLEAN; the future VA engine defaults to 2040. The default is set in each engine's neutral Sound (`FilterKind::default_for(engine)` through `ParamSnapshot::for_engine`, which reaches `Sound::init` and `neutral_params`). The neutral fixture gains one row per engine, appended only. Existing patches keep their stored TYPE.
- **Names:**
  - The modes are named by what they do: NOTCH1 is one notch, NOTCH2 two. NOTCH2 is the Cascadia's PHZR; CLEAN's NOTCH1 is its NT2.
  - The SSM2040 type is called **2040** because "Prophet" is Sequential's trademark.
- **CLEAN is the Cascadia** (amends 0037's SVF row):
  - FLT: TYPE · FREQ · Q · MODE · FM1 (ENV 1 → CUTOFF) · FM2 (NOTE → CUTOFF).
  - FLT › MOD: LEVEL · FM3 · FM3 IN · QM · QM IN.
  - FM3 IN (Filter 8, default LFO 1) and QM IN (Filter 11, default ENV 3) are jacks: repatching moves the route.
  - MIX + MINUS deletes a route from any route knob, with ADR 0086's hint.
- **CLEAN's DSP:**
  - `tan` is stmlib ACCURATE, clamped to 0.497, once a block.
  - **Divides.** A steady block uses `d = 1/(1 + kg + g²)` and `d₂` precomputed. A moving block (routed, tapped or changed) divides exactly per sample, because lerping `d` is unsafe (it errs by 95 % on a one-block octave), and that path is billed.
  - **Resonance.** `k = 2(1 − RES/0.95)`, to −0.105 at RES 1, so CLEAN self-oscillates. `1 + kg + g² > 0` for all g iff k > −2, and the in-loop C1 saturate (≤ 1.5) bounds the oscillation.
  - **Onset.** Every resonant type starts to oscillate at RES 0.95.
  - **LP1** = `lp + bp`.
  - **NOTCH1** = `lp + ¼·hp`, a zero at 2 × FREQ.
  - **NOTCH2** = dry plus four TPT all-passes (the SSM2040 data sheet's Fig. 10), with notches at 0.414 and 2.414 × FREQ. Q is one-sample feedback, `fb = 0.7·RES`, with no self-oscillation, as on the Cascadia; the output is scaled by `√(1 − fb)`, and the dry signal is taken after the feedback node, so the notches stay exact.
- **LEVEL:**
  - It attenuates below unity, as the Cascadia's does: `2^(4·LEVEL − 2)`, unity at 0.5.
  - A transparent clipper (linear to ±1, C1 knee) runs past unity with its own 20 ms faded gate.
  - A per-Part meter drives a red ring LED.
  - On the 2040, LEVEL drives the OTA stages directly and the clipper doesn't run.
- **2040:** four TPT one-poles.
  - Each stage is `t = g·sat((1 − G)(x − s))`, the OTA on the true error, with a divide-free cubic `sat`.
  - The global loop is solved linearly in one step, then limited by a 3080-style feedback limit `L·sat(·/L)` with `L = ¼`, which fixes a 330-cent flat self-oscillation.
  - `k = 4·RES/0.95`.
  - The bass loss is authentic: DC gain `1/(1 + k)`, −14 dB at onset, uncompensated.
  - **Two references.** It covers the Prophet-5 Rev 1/2 and the RSF Kobol Expander. RSF's 1979 factory schematic (p. 11 BOM: CI4 "SSM 2040"; p. 8: resonance through a CA3080, CI5, under a 2N3906, T3) confirms the 3080 resonance path. That scan is reference only and is not in the repo.
  - **The Kobol's "natural saturation".** Its reputed source is unconfirmed: the schematic shows no distinct saturating circuit, so it is not modelled separately.
  - **A cross-check.** René Schmitz's discrete RS2040, which Behringer's 2023 clone follows, is the check for the per-stage nonlinearity.
- **DIRTY:** the Chamberlin SVF at ×2, with `q = 2(1 − RES/0.95)`.
  - **Stability.** `f` is clamped per block to `0.95(√(q₊² + 4) − q₊)`, Jury's exact bound, using the block's largest `q`.
  - **Bounded.** Hard ±2 clamps on `lp` and `bp`, and output gain ½ clamped to ±1, bound it at every setting.
  - **The ceiling** is about 12 kHz at RES 0 and rises with RES.
- **ALGO taps:**
  - **What they are.** OP 5 and OP 6 keep their algorithm role and are also audio-rate sources. Their value is the op's output, so the op's level and envelope are the depth.
  - **Where they go.** Exactly four per-sample destinations: FREQ (through FM3, ±4 octaves, `g·exp2_poly`, clamped, no per-sample `tan`), Q (through QM), the VCA (`1 − |a| + a·tap`, AM to ring) and FOLD.
  - **Where they may not go.** Never operator pitch or level; every other cell greys out with a hint.
  - **Storage.** Taps live outside the matrix, in `TapRoutes` (2 × 4 amounts, 9 B per Sound). `ModSource` appends `Op5 = 8` and `Op6 = 9`.
  - **Rendering.** Op rows are copied per block into a stack buffer. Amounts are ramped per block. A FREQ or Q tap forces the moving, dividing path.
- **Billing,** in the house manner:
  - Static probe counts (thumbv7em, cortex-m7, opt-level 2) × 1.46 × 1.1, + 12 per `vdiv`.
  - Today's LP24 is counted first. Steady paths are billed `max(0, that − old LP24)`, and each probe asserts 0 `vdiv` in its steady loop.
  - The move, LEVEL and per-tap-destination terms are billed only while they apply; an unused tap costs 0.
  - Budgets: CLEAN LP4 ≤ 56, NOTCH2 ≤ 34, 2040 ≤ 70, DIRTY ≤ 30 instructions a sample.
- **Order.** Implementation starts after `modal-stagger` merges. The filter's in-loop saturate stays divide-free whatever that branch lands.

## Alternatives considered
- **Lerping `d` per sample:** unsafe on large one-block moves (B1).
- **Newton for the 2040's loop:** about +100 instructions and a `vdiv` per iteration.
- **Compensating the 2040's bass loss:** the owner chose the authentic loss.
- **A self-oscillating NOTCH2:** the Cascadia's PHZR doesn't.
- **Raising `MAX_MOD_SOURCES` to 16 for the taps:** about 7 KB of AXI for two rows with four live cells.
- **Taps to operator pitch or level:** that is double FM, which the owner ruled out.
- **DIRTY at ×4:** the owner accepted the ×2 ceiling.

## Consequences
- **Sound changes.** LP1, NOTCH1, NOTCH2, the top octave and LEVEL's law all change sound. Factory Sounds move to LEVEL 0.5, and goldens are re-recorded. Old patches load safely.
- **Bills.** The steady SVF bills less than it costs, which is headroom until a re-bench. Routed or tapped cutoff pays for its divides.
- **Memory.** About 110 B per voice and 768 B of stack in the worst block; about 11 B per Sound.
- **Codes.** All new codes are append-only: KIND 4 and 6, MODE 9, Filter params 8 and 11, ModSource 8 and 9, and `TapRoutes`.

## Sources
- `docs/superpowers/specs/2026-10-05-filter-types-design.md` (revision 2) and its sources.
- Intellijel *Cascadia Manual* v1.2 § 8.
- *SSM 2040* data sheet (1977).
- RSF *Kobol Expander* schematic (1979), pp. 8 and 11. Reference only, not in the repo: https://yusynth.net/gear/RSF/schema-RSF-KOBOL.pdf
- R. Schmitz, RS2040: https://www.schmitzbits.de/rs2040.html
- Zavalishin, *The Art of VA Filter Design* 2.1.2.
- stmlib `dsp/filter.h` (MIT).
- Chamberlin (1985); Dattorro (1997); Lazzarini and Timoney (2021).
- Issues #57, #126, #128, #165, #177.
