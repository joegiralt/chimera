# 0249. Bill each Sound term by term, never under the unit's reading of it

- **Status:** Proposed
- **Deciders:** firmware (bills and bench); owner for the open items

## Context
#448 asked that a Sound be billed only for what it routes or enables, so light Sounds get more voices: 8 typical, 10 or more for light ones. The owner accepted about 9–10 six-operator voices as 0.3's goal (#793, #329).

By 0.3 the bill already works that way. `Voice::cost` adds a term only when the Sound turns it on:
- ALGO's operators, by LEVEL or route (ADR 0026);
- the filter's moving path, by a CUTOFF or RES route, a tap or FB (ADR 0190);
- LEVEL's clipper, when it can open;
- the FX slot, while it runs, and its ramp when routed;
- VA's NOISE, when it plays (ADR 0197);
- the pool's VCA, destination, slide and FUNC terms, per route;
- TAPE's loops, by length (ADR 0211).

ADR 0218 set the engine terms to the 0.3 unit readings, routes off.

To find what is left, a model was built from the bench-0.3 readings alone (private notes, rev V at 480 MHz, 2026-10-26, 4eae5d81). It takes each term from the row that isolates it and models every public factory Sound from those terms (`chimera-desktop/tests/sound_bill_test.rs`).

The rows are warm 64-block averages. Most are read at a voice's mean. Where a voice climbs with its note (VA, ALGO's FRMT rows) they are read at the top voice, and ALGO takes the higher of the two. No row reads a worst block.

Against that model:
- **The engine terms bill at their readings.** VA's MODE terms sit within 1–6 of the top-voice readings. A typical VA Sound models 782–815 and bills 815–837. Eight voices need 785 or less, so even a bill at the model would give it no voice.
- **The filter's moving paths are the only terms billed over a reading.** CLEAN bills 75 against 42, its worst row (CUTOFF stepped every block, CLEAN MOVE − SVF). An LFO into CUTOFF reads 0 (1 CUTOFF − 1 DEST). The 2040, LADDER and SEM have no row without a CUTOFF route, so their steady and moving costs can't be told apart.
- **Some Sounds bill under the model (30 of 89):**
  - the formant and six-operator ALGO Sounds, by up to 61 (ALGO's terms are held for the formant floor, ADR 0211 point 3 and ADR 0218 point 4);
  - CLEAN driven over ½, by 32 (LP24 HOT reads 97 over SVF, billed 35);
  - the rest within 12, the bench's spread.

## Decision
1. **A Sound's bill is the sum of the terms it turns on.** Each term is at least its worst unit reading with the routes off, from the row that isolates it, rounded up to 10. A term with no isolating row is never cut: it keeps its counted bill until a unit run reads one.
2. **The guard.** `sound_bill_test` models every public factory Sound from the readings and fails when a bill is under its model. The Sounds under it now are listed in `KNOWN_UNDER`, each at its present margin:
   - a bill cut that widens a margin fails;
   - a gap that closes must come off the list (`every_known_gap_is_still_open`).
3. **The guard bounds the bench's averages, not the worst block.** A block over its average comes out of the 25 % the UI keeps (ADR 0124) until worst-block rows are read on the unit. Until then the guard catches a cut below the readings; it does not prove a deadline.
4. **Raised from bench-0.3.** Only `Cost` constants change, so every render stays bit for bit.
   - `SEM_MOVE_COST` 50 → 70: SEM MOVE reads 38 over a moving CLEAN, and a stepped CUTOFF adds 42, over the steady 10.
   - `FB_COST` 115 → 130: the 2040's 874 − 682 − 65.
5. **Kept until their rows are read:**
   - CLEAN's, the 2040's and LADDER's moving bills (`LADDER SWEEP` isolates LADDER's);
   - FILTER_LEVEL's 35;
   - ALGO's terms;
   - the MODAL bank (ADR 0218 point 4).
6. **The next unit run separates them** with the ROUTING rows added here (`2040 SWEEP`, `LADDER SWEEP`, `SEM SWEEP`, `VA SWEEP`, `VA WAVE`, `HOT RES`, `DIRTY HOT`). It also reads the existing rows again, now with the routes cleared (ADR 0218 point 2).

## Alternatives considered
- **LADDER's moving bill to 50 now.** ACID CLASSIC and ACID TRI would go to 7 voices, TORN BASS to 6. Not yet: no row isolates LADDER's moving path (point 1), so it waits for `LADDER SWEEP`.
- **CLEAN's moving bill to 50.** That needs CLEAN driven billed at 97 too, or SOLID BASS and FUNK CLAV fall a further 25 under. Billing it lifts ADR 0026's reference heavy routing by 40, and `budget.rs` then fails to compile ("over the formant budget"). That is the owner's floor (ADR 0218 point 4).
- **The 2040's moving bill to 42.** Every 2040 VA Sound gets about 23 back, but no voice: 795 is still 7.
- **A tolerance in the guard.** It would hide the gaps the guard exists to show.

## Consequences
- **Voices** (rev V, QUAD reverb, no ORBIT; room 7,500 − 1,190 − 24 = 6,286): unchanged for all 89 public factory Sounds.

  | Voices | Sounds |
  |---|---|
  | 8 | 15 |
  | 7 | 48 |
  | 6 | 17 |
  | 5 | 4 |
  | 4 | 9 |
  | 3 | 3 |

  VELVET FOG bills 753 (was 733) and keeps 8. VA INIT on SEM with a CUTOFF LFO drops from 8 to 7. The full table is in the spec.
- **#448's targets are out of reach by billing alone.** What remains:
  - the 8-voice pool (`MAX_VOICES`), which caps light Sounds at 8;
  - the bus, billed whole at 1,190. About 400 of that is effects that sleep at send 0 (ADR 0214, 0218 point 5). Billed awake-only, VA's 818 fits 8;
  - an 80 % audio share (ADR 0124), at which VA's 818 fits 8;
  - engine cuts (#444, #446).
- **Bank 1:** const values only. The bench rows are bank 2's.

## Sources
- (private notes) (`bench-algo.txt`, `bench-trs.txt`, `summary.md`); (private notes).
- `docs/superpowers/specs/2026-11-08-sound-billing-design.md`.
- ADR 0026, 0124, 0190, 0197, 0211, 0214, 0218; #448, #793, #329.
