# 0261. Keep the bus billed whole while voices bill under their worst blocks

- **Status:** Proposed
- **Deciders:** firmware (bills and bench); owner for the open items

## Context
The voice budget reserves the whole FX bus every block (`FxBus::COST`, 1,190 on rev V), even while ADR 0214's idle skips leave most of it asleep. #448, #883 and #793 asked for those cycles as voices, sound-identical, with no overrun. A first design billed each effect only while awake. It predicted an effect asleep from its own idle-skip guard (checked against `SKIPS` through the whole engine) and gave about 35 of 96 factory Sounds a voice. The unit read it on 2026-11-11 (rev V at 480 MHz, (private notes)). The bus rows were read with six A16+17 voices rendered between blocks, every effect at its worst, SIZE still for the ones asleep, and the compressor at its worst:

| Awake | none | CHO | DLY (no REV SEND) | REV | CHO+DLY | CHO+REV | DLY+REV | all (BUS+V) |
|---|---|---|---|---|---|---|---|---|
| Cycles a sample | 489 | 597 | 773 | 876 | 880 | 983 | 1,171 | 1,185 |

- **The savings don't add up per effect.** With the chorus asleep the bus still reads 1,171. Any bill must be a table of the eight combinations, each rounded up from its own row.
- **The bus's slack is what keeps the heavy scenes on time.** STRESS's six A16+17 keys read 463,605 a block against 432,128 billed (the whole bus in it). That is about 84 a voice over their bill, under cache traffic (ADR 0249 point 3: the bills bound the bench's means, not the worst block). The 700 or so cycles an asleep bus frees were covering it. Billed honestly, the freed room let a seventh voice in: STRESS WAKE NOTE read 510,031 of 480,000.
- **The design's own work costs.** The per-block rest checks added 0.3–3.8 k cycles a block to STRESS rows main already runs over (STRESS+ORBIT, MOVE, ROOM, HALL, BLOOM).

## Decision
1. **The bus stays billed whole** (`FxBus::COST`) until the voices bill at least their worst-block readings (#883). Freed bus cycles go to no voice while voices bill under what they run. No admission or billing code changes.
2. **The measurement stays.** The bench keeps the rows that read it:
   - bench-stress's BUS AWAKE screen (after REVERB): BUS ASLEEP, AWAKE CHO/DLY/REV and ASLEEP CHO/DLY/REV. These moved here from bench-routing, where their voices' note-ons pulled `handle_with` out of bank 2 into bank 1;
   - bench-stress's WAKE screen (after STRESS): STILL, WAKE NOTE and WAKE SEND, each with worst/budget, voices, refusals and billed.
3. **When #883 lands,** an awake bill is re-derived from this table, each combination rounded up plus a margin. It must pass WAKE NOTE and WAKE SEND under ORBIT before it gives any voice.

## Alternatives considered
- **The awake bill from the rows, as a table.** It is correct for the bus. But it hands the voices room that their own under-billing already spends, and STRESS overruns.
- **A fudge reserve for the voices' gap.** It would guess at what #883 must measure, Sound by Sound.

## Consequences
- Voices: unchanged from main.
- Bank 1 and RAM: unchanged. The bench rows are bank 2's (bench builds only).
- #448's bus lever waits for #883's worst-block readings. The table above is its input.

## Sources
- #448, #883, #793; ADR 0026, 0027, 0091, 0214, 0218, 0249.
- (private notes) (routing, stress, stress+orbit, and main's); (private notes).
