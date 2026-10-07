# 0096. Three filter TYPEs, CLEAN (Cascadia-style), 2040 (SSM2040) and DIRTY (Chamberlin), and ALGO's OP 5/6 audio-rate taps (supersedes 0037)

- **Status:** Proposed
- **Deciders:** owner (direction and rulings, 2026-10-05; the 2040's VOICE and stage clip, 2026-10-06), firmware (after adversarial review)

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
- **Supersedes ADR 0037,** which is Accepted and so is not edited (CLAUDE.md). 0037's rules that still hold are carried forward here, unchanged:
  - KIND (TYPE) is the filter page's first knob and lays out the rest from `const` data;
  - route knobs are views of matrix cells, and the matrix always applies;
  - `mode ∈ kind.modes()` holds on every write, and a KIND change keeps MODE if it can;
  - a fixed or inapplicable cell is dimmed, and its encoder ignored;
  - AMP VEL's priming exception;
  - Filter ids 3–5 are retired;
  - each kind is billed by `FilterKind::cost`.

  What changes is 0037's SVF panel row, its "`modes()[0]` is the default" and its SVF bills.
- **Types and modes.** KIND shows as TYPE. Mode labels and `default_mode()` belong to each type. Disk codes are identity; only labels change. A TYPE or MODE change warm-starts the incoming state from the outgoing outputs and crossfades over two blocks, unbilled.
  - **CLEAN** (`Svf`, code 0): LP1 LP2 **LP4** BP2 BP4 HP4 NOTCH1 NOTCH2.
  - **2040** (code 4): LP4 only, with VOICE REV2 / EXP.
  - **DIRTY** (code 6): **LP2** BP2 HP2 NOTCH1. HP2 is `Hp12`, code 9.
- **The default TYPE follows the engine.** ALGO, MODAL and TAPE default to CLEAN; the future VA engine defaults to 2040. The default is set in each engine's neutral Sound (`FilterKind::default_for(engine)` through `ParamSnapshot::for_engine`, which reaches `Sound::init` and `neutral_params`). The neutral fixture gains one row per engine, appended only. Existing patches keep their stored TYPE.
- **Names:**
  - The modes are named by what they do: NOTCH1 is one notch, NOTCH2 two. NOTCH2 is the Cascadia's PHZR; CLEAN's NOTCH1 is its NT2.
  - The SSM2040 type is called **2040** because "Prophet" is Sequential's trademark.
- **CLEAN is the Cascadia** (replacing 0037's SVF row):
  - FLT: TYPE · FREQ · Q · MODE · FM1 (ENV 1 → CUTOFF) · FM2 (NOTE → CUTOFF).
  - FLT › MOD: LEVEL · FM3 · FM3 IN · QM · QM IN.
  - FM3 IN (Filter 8, default LFO 1) and QM IN (Filter 11, default ENV 3) are typed jacks, `Jack { Mod(ModSource), Tap(TapOp) }`, with their own disk codes 0–9. `ModSource` is not extended: it is the matrix-row type, and extending it would index out of bounds.
  - Repatching moves the route, between matrix cells or between `ModState` and `TapRoutes`, converting a CUTOFF amount (10 octaves at 100 %) to a FREQ tap's (4 octaves at 100 %) so the depth holds.
  - On a non-ALGO Sound, a jack on OP 5 or OP 6 acts as its default source.
  - MIX + MINUS deletes a route from any route knob, with ADR 0086's hint.
- **CLEAN's DSP:**
  - `tan` is stmlib ACCURATE, clamped to 0.497, once a block.
  - **Divides.** A steady block uses `d = 1/(1 + kg + g²)` and `d₂` precomputed. A moving block (routed, tapped or changed) divides exactly per sample, because lerping `d` is unsafe (it errs by 95 % on a one-block octave), and that path is billed.
  - **Resonance.** `k = 2(1 − RES/0.95)`, to −0.105 at RES 1, so CLEAN self-oscillates. `1 + kg + g² > 0` for all g iff k > −2, and the in-loop C1 saturate (≤ 1.5) bounds the oscillation.
  - **Onset.** Every resonant type starts to oscillate at RES 0.95.
  - **Trims.** Per-mode output trims at onset keep every mode's self-oscillation below 0 dBFS, in one band shared with the 2040's REV2 voice.
  - **LP1** = `lp + bp`.
  - **NOTCH1** = `lp + ¼·hp`, a zero at 2 × FREQ.
  - **NOTCH2** = dry plus four TPT all-passes (the SSM2040 data sheet's Fig. 10), with notches at 0.414 and 2.414 × FREQ. Q is one-sample feedback, `fb = 0.7·RES`, with no self-oscillation, as on the Cascadia; the output is scaled by `√(1 − fb)`, which holds loudness within about 1 dB for FREQ up to about 2 kHz (about +3 dB at 10 kHz), and the dry signal is taken after the feedback node, so the notches stay exact.
- **LEVEL:**
  - It is a new param, Filter id 12. DRIVE (id 2) is retired (ADR 0009), and on load a stored DRIVE translates to `LEVEL = clamp((log₂(1 + 4·DRIVE) + 2)/4, 0, 1)`, through `storage/codes.rs` as Modal's `translate_v1` does. Routes into DRIVE are re-addressed to LEVEL, and `Sound::neutral`'s DRIVE stays frozen, so old patches keep their level up to DRIVE 0.75; above it LEVEL clamps at ×4, at most 1.94 dB quieter.
  - It attenuates below unity, as the Cascadia's does: `2^(4·LEVEL − 2)`, unity at 0.5.
  - A transparent clipper (linear to ±1, C1 knee) runs past unity with its own 20 ms faded gate.
  - A per-Part meter drives a red ring LED.
  - On the 2040, LEVEL drives the OTA stages directly and the clipper doesn't run: the stages' ±1 V clip is the clipper. LEVEL is shown once, as DRIVE on FLT.
- **2040:** four TPT one-poles.
  - Each stage is `t = g·sat((1 − G)(x − s))`, the OTA on the true error, with a divide-free cubic `sat`.
  - Each stage output is clamped to ±R, the chip's ±1 V swing (data sheet design hint 1), on both voices. `R = 2·ALPHA_REV2`, because the Prophet-5 shows 500 mV per pole at maximum saw. It costs 8 instructions, and it makes the output bound exact: `R/α_v · OUT_2040`.
  - The global loop is solved linearly in one step, then fed back through the voice's own resonance element (`ResElement`, carried by `K2040Voice`; owner's ruling, 2026-10-06). REV2's is the Prophet's SSM 2020, linear: no limit, bounded by the stage clip, and trimmed −13.3 cents at the cutoff as the Prophet is trimmed at full resonance. EXP's is the Kobol's CA3080, the limit `L·sat(·/L)` with `L = ¼`: it sets EXP's squelch, its self-oscillation level and pitch, and its full-scale bass loss.
  - **Input scale.** α ≈ 0.1 was tuned together with `L`. α = ½ would crush resonance at full scale, with self-oscillation around −24 dBFS against CLEAN's level. So LEVEL is the 2040's drive.
  - `k = 4·RES/0.95`.
  - The bass loss is authentic: DC gain `1/(1 + k)`, −14 dB at onset, uncompensated.
  - **Two references, one VOICE switch.** It covers the Prophet-5 Rev 1/2 and the RSF Kobol Expander. The switch is FLT › MOD's first cell (Filter id 13, `K2040Voice`), and its values read **REV2** and **EXP**: the panel avoids both names, out of trademark caution. It changes the voice's constants and its resonance element:
    - **Drive.** `α_EXP = 2·ALPHA_REV2`, +6.0 dB. The stage-1 gain is 10 K/200 K on the Prophet (Voice schematic SD411, R480/R476) and 10 K/100 K on the Kobol (RSF schematic p. 8, R25/R24).
    - **The output VCA, EXP only.** `sat(β·y₄)/β` with `β = 0.62/R`: the Kobol's CI3, a CA3080 "VCA linéaire" fed through 10 K/220 Ω, about 1–2 % THD at full swing. That is the circuit candidate for its reputed "natural saturation". It costs 6 instructions.
    - **Resonance.** `k` and the onset are shared; the element is not. The Kobol's resonance is a CA3080 (CI5, under T3 a 2N3906), modelled by the limit `L`: under drive its resonance ducks and blooms back. The Prophet's is an SSM 2020 in linear mode ("RES FDBK VCA", ½ U490): no limit, so it holds its resonance under drive and oscillates at the ±1 V clip. A shared `L` (revision 4) is overturned.
    - **Level.** LEVEL ½ is unity on both voices. REV2 self-oscillates at the clip, about −5.4 dBFS, in the band with CLEAN and DIRTY; EXP at its 3080's level, about −17 dBFS, in its own band.
    - A VOICE change rescales the states by `α_new/α_old` and crossfades like a MODE change.
  - **Correction.** The data sheet's Fig. 9 feeds resonance through "½ SSM 2020", not a 3080, and the Prophet-5 follows it. Only the Kobol uses a 3080. The RSF schematic (p. 11: CI3 "3080", CI4 "SSM 2040", CI5 "CA 3080") is reference only and is not in the repo.
  - **A cross-check.** René Schmitz's discrete RS2040, which Behringer's 2023 clone follows, is the check for the per-stage nonlinearity.
- **DIRTY:** the Chamberlin SVF at ×2, with `q = 2(1 − RES/0.95)`.
  - **Stability.** `f` is clamped per block to `0.95(√(q₊² + 4) − q₊)`, Jury's exact bound, using the block's largest `q`.
  - **Bounded.** Hard ±2 clamps on `lp` and `bp`, and output gain ½ clamped to ±1, bound it at every setting.
  - **The ceiling** is about 12 kHz at RES 0 and rises with RES.
- **ALGO taps:**
  - **What they are.** OP 5 and OP 6 keep their algorithm role and are also audio-rate sources. Their value is the op's output, so the op's level and envelope are the depth.
  - **Where they go.** Exactly four per-sample destinations:
    - FREQ, through FM3: ±4 octaves, `g·exp2_poly`, clamped, no per-sample `tan`; above about fs/8 it reads flat for upward moves.
    - Q, through QM.
    - The VCA, as a tap-VCA stage `1 − |a| + a·tap`, AM to ring, placed **before** the DC blocker so a self-correlated tap's DC is removed. The envelope VCA stays after the blocker.
    - FOLD: a FOLD tap makes the folder run (`stage_runs`) and is billed `FOLD_COST` when FOLD is otherwise off.
  - **Where they may not go.** Never operator pitch or level; every other cell is an X cell with a hint.
  - **Storage.** Taps live outside the matrix, in `ParamSnapshot.taps`, so `Voice::cost` sees them. They are stored as a record of 4-byte (jack, dest block, dest param, amount) entries, validated through `ValidAddr`, not as a positional blob.
  - **Rendering.** Op rows are copied per block into a stack buffer. Amounts are ramped per block. A FREQ or Q tap forces the moving, dividing path.
- **The matrix is the only patch bay, and the single source of truth.**
  - **Four permanent columns.** FREQ, Q, VCA and FOLD are permanent columns for every Sound and source. They are reserved in `ModState` (`fixed`, 36 B) outside `MAX_MOD_DESTS`'s 16 registry columns, rather than pinning 4 of the 16, so no patch loses a column. On load, routes to those four move into the fixed columns and free their registry slots, which keeps a full-16 patch whole. On disk they are ordinary route records.
  - **Ten rows on ALGO.** OP5 and OP6 are two extra rows, 10 rows of 14 px in the same span, so the grid still has no vertical scroll. Their amounts are bipolar and stored in `TapRoutes`, but shown as matrix rows.
  - **X cells.** Tap cells outside the four show a 5 × 5 px X in MID, take no amount, and on touch show `OP5 · AUDIO RATE: FREQ Q VCA FOLD ONLY` (ADR 0086).
  - **One value.** FM3 and QM on the filter page are views of the matrix cells (jack, FREQ) and (jack, Q). Editing either place edits one value.
- **Billing,** in the house manner:
  - Static probe counts (thumbv7em, cortex-m7, opt-level 2) × 1.46 × 1.1, + 12 per `vdiv`.
  - Today's LP24 is counted first. Steady paths are billed `max(0, that − old LP24)`, and each probe asserts 0 `vdiv` in its steady loop.
  - The move, LEVEL and per-tap-destination terms are billed only while they apply; an unused tap costs 0.
  - Budgets: CLEAN LP4 ≤ 56, NOTCH2 ≤ 34, 2040 REV2 ≤ 78 (65 measured without the limiter) and EXP ≤ 84 (each including the 8-instruction stage clip), DIRTY ≤ 30 instructions a sample. The 2040's moving path is +12 instructions and +45 cycles over its steady one (2 `vdiv`): it divides `G` and `D` and forms `G⁴`, `k·D` and `g·4/27` every sample; fusing the two divides into one saves about 4 cycles for +5 instructions, not worth it. The 2040 bills its measured steady cost, 15 on REV2 and 40 on EXP, and `K2040_MOVE_COST` 60 while moving.
  - Measured bills (plan § Measured, Task 15, counted at 1879169d): CLEAN moving `CLEAN_MOVE_COST` 75; the 2040 moving 60; LEVEL's clipper `FILTER_LEVEL_COST` 35; a FREQ or Q tap's shaping `TAP_SHAPE_COST` 80, once; a VCA tap 30; a FOLD tap 45 (+ `FOLD_RAMP_COST` 35, + `FOLD_COST` when FOLD is off); capture 15 a tapped op. The spec's Est. bill column is the plan's estimate, not these.
- **Order.** Implementation starts after `modal-stagger` merges. The filter's in-loop saturate stays divide-free whatever that branch lands.

## Alternatives considered
- **Lerping `d` per sample:** unsafe on large one-block moves (B1).
- **Newton for the 2040's loop:** about +100 instructions and a `vdiv` per iteration.
- **Compensating the 2040's bass loss:** the owner chose the authentic loss.
- **One 2040 voicing between the two references:** the drive, the resonance element and the output stage really differ, and the owner wants both.
- **A shared limiter `L` on both voices** (revision 4): it kept one pitch test, but made REV2's linear 2020 a 3080 and left the voices alike at high RES. The owner ruled each voice its own element.
- **A self-oscillating NOTCH2:** the Cascadia's PHZR doesn't.
- **Raising `MAX_MOD_SOURCES` to 16 for the taps:** about 7 KB of AXI for two rows with four live cells.
- **Taps to operator pitch or level:** that is double FM, which the owner ruled out.
- **DIRTY at ×4:** the owner accepted the ×2 ceiling.

## Consequences
- **Sound changes.** LP1, NOTCH1, NOTCH2 and the top octave change sound, and goldens are re-recorded. Old patches load safely, and DRIVE's translation keeps their level.
- **Bills.** The steady SVF bills less than it costs, which is headroom until a re-bench. Routed or tapped cutoff pays for its divides.
- **Memory.** About 110 B per voice and 768 B of stack in the worst block; up to about 39 B per Sound. The voice chain measures 2,104 B with all three types and the taps, past ADR 0051's 2,048: ADR 0101 raises the bound to 2,112.
- **Sparse jacks.** On the pool's sparse encoding (ADR 0087), FM3 IN and QM IN are left out only where the load's inference for pre-jack patches gives them back, so a pool round trip keeps them.
- **Codes.** All new codes are append-only: KIND 4 and 6, MODE 9, Filter params 8, 11, 12 and 13 (2 retired), `K2040Voice` 0–1, `Jack` 0–9, and the `TapRoutes` record, 0x001B (0x15 is main's `PoolSource`, 0x19 and 0x1A TAPE's zone records).

## Sources
- `docs/superpowers/specs/2026-10-05-filter-types-design.md` (revision 2) and its sources.
- Intellijel *Cascadia Manual* v1.2 § 8.
- *SSM 2040* data sheet (1977).
- RSF *Kobol Expander* schematic (1979), pp. 8, 11, 13 and 14. Reference only, not in the repo: https://yusynth.net/gear/RSF/schema-RSF-KOBOL.pdf
- Sequential Circuits, *Prophet-5 Technical Manual* (January 1980), §§ 2-7, 2-13, 4-17, 5-13 and Voice schematic SD411: https://archive.org/details/Sequential_Circuits_Prophet_5_Service_Manual
- `.superpowers/sdd/filter-types/2040-p5-vs-kobol.md`, the comparison behind VOICE.
- R. Schmitz, RS2040: https://www.schmitzbits.de/rs2040.html
- Zavalishin, *The Art of VA Filter Design* 2.1.2.
- stmlib `dsp/filter.h` (MIT).
- Chamberlin (1985); Dattorro (1997); Lazzarini and Timoney (2021).
- Issues #57, #126, #128, #165, #177.
