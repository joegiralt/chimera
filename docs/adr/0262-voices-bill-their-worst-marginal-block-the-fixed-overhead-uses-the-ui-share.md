# 0262. Bill each voice at its worst marginal block; let the fixed overhead use the UI's share

- **Status:** Accepted (2026-11-11: the owner chose option B and approved the voice losses)
- **Deciders:** owner (option B, the five voice losses, the formant gate's freeze), firmware (bench and bills)

## Context
ADR 0249 billed every term at the bench's 64-block means. ADR 0261 found STRESS's six A16+17 voices reading 463,605 cycles a block against 432,128 billed. #883 asked for every Sound's bill to cover its worst block.

The bench now times every voice row under the bus at its worst (every effect at its worst, sends at ½). Each row is read at 0 to 8 voices, keeping each count's costliest block. Rev V at 480 MHz, 2026-11-11, bench 280c8c2e: (private notes).

- **A voice's marginal cost at its worst block** (the slope of the costliest blocks over 1 to 8 voices) is at or under its bill on nearly every row. The exceptions are below.
- **The overrun is fixed overhead, not voices.** A block with no voice reads about 160 cycles a sample over the bus. A Part's first voice adds about 230 more. Admission bills neither: only `FxBus::COST` and the control reserve (24).
  - STRESS's 7,244 a sample less its voices' 5,538 and the bus's 1,190 leaves 516, knobs and DAC interleave in it.
- **The deadline is never at risk.** The worst block read is 518,800 (STRESS HALL with ORBIT) of the hard 640,000, which is 81 %. ADR 0124's 480,000 is the audio's 75 % share, not the deadline.
- **Billing the overhead as a reserve** (about 510 a sample) would cost 65 of 96 factory Sounds a voice.

## Decision
1. **Bills cover each voice's worst marginal cost.** Each term is at or over its worst-block slope from the row that isolates it, rounded up. Only `Cost` constants and one VA term change; every render is bit for bit.
   - VA: a route on WAVE1 or WAVE2 bills `COST_WAVE_MOVE` 50 (VA WAVE − VA reads 92, the MODE's bill holds the rest).
   - `TAP_SHAPE_COST` 80 → 115 (TAP FREQ − 1 OP reads 201 against 170).
   - `ModRouting::ENV_B` 98 → 120, `SLIDE` 33 → 37, `DEST_FIRST` 6 → 8, `DEST` 1 → 5 ((VEL 4R − VEL 1R) / 3 = 4.3).
   - ALGO's FRMT terms 210/220/380/390 → 220/230/400/410 (FRMT PM: charge × 1.03).
   - NOIS 80/100 → 95/125. One rule scales both FRMT and NOIS: the costliest added voice. NOIS's reads charge × 1.85, so 97/134 would be its reading. It stays 2 (unlinked) and 9 (linked) short a NOIS operator, known and pinned (`nois_term_shortfall_is_known`). At the reading CRACK SNARE would drop 6 → 5 (owner, 2026-11-11). No factory Sound bills under its model for it; the shortfall runs in option B's overhead allowance.
   - ALGO's table terms stay (`COST_OP` 60, `COST_LINK` 8, `COST_FEEDBACK` 1). The rows read about 1.15 × their share over 1 OP, but 1 OP's bill holds that.
2. **The fixed overhead is not billed.** It may use the UI's 25 %, never past the deadline. This supersedes ADR 0124's 75 % share for the worst block only; admission still books 7,500 a sample.
   - The overhead is about 160 a sample a block, plus about 230 for each sounding Part's first voice.
   - Worst case: 480,000 + 160 × 64 + 230 × 64 a sounding Part. One Part reads 518,800, 81 % of the 640,000 deadline. With 4 Parts sounding it is about 549,000, 86 %. With 6 Parts it is about 569,700, 89 %. Multi-Part loads can pass 85 %.
3. **The formant gate is frozen at 0.3's bills.** It keeps `GATE_HEAVY` 472, `GATE_TABLE` 832 and the 0.3 charges (FRMT 390/200, NOIS 40/20), so every patch that loaded still loads and sounds the same.
   - This supersedes ADR 0218's 4/3-voice floor guarantee for the synthetic worst formant patch under heavy routing: that patch keeps one voice fewer (`cost_test`). Every factory and built-in Sound keeps its floor.
4. **The knob-move rows bill nothing.** SWITCH, SEM SWEEP and VA WAVE step a parameter every block with no route. They run in ADR 0061's UI-move headroom. So does the cache-off row (1 OP NO I$), which is a diagnostic.
5. **The guard models every factory Sound from the 883b rows.** `sound_bill_test` takes each term's worst slope, VA and ALGO at their costliest added voice. Three Sounds stay pinned in `KNOWN_UNDER`: VOWEL BUZZ and THROAT SING (−2) and MORPH BASS (−1).

## Alternatives considered
- **A fixed reserve for the overhead** (about 510 a sample): 65 Sounds lose a voice.
- **ALGO's table terms × 1.15:** 20 Sounds lose a voice, for gaps the base already covers.
- **The gate kept derived from the bills:** factory Sounds stop loading (a third FRMT refused) and FRMT turns to NOIS in a factory patch.
- **`DEST` at 3:** under its reading.

## Consequences
- **Voices** (rev V, QUAD, no ORBIT): five Sounds lose one each. All other factory Sounds keep their count.
  - DEAD RADIO 8 → 7
  - SERVO 6 → 5
  - GROWL BASS 6 → 5
  - YELLING LEAD 5 → 4
  - BIT SHRED 7 → 6
- **Under the worst load,** audio may take about 81 % of a block with one Part sounding and about 86 % with four. The UI loop gets less, and its responsiveness is checked on the unit at STRESS (`loop_peak_us`, a key's round trip).
- **The bus lever** (ADR 0261's awake bill) still waits: freed bus cycles would go to voices whose fixed overhead is not billed.
- **Bank 1:** const values and VA's WAVE check, measured by `just check`.

## Sources
- (private notes) (ROUTING, ALGO), (private notes) (STRESS), (private notes).
- #883, #448, #704; ADR 0026, 0061, 0124, 0211, 0218, 0249, 0261.
