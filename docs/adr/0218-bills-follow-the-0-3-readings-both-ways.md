# 0218. Bill the 0.3 readings both ways, at the worst row, with the Part's routes off

- **Status:** Proposed
- **Deciders:** firmware (bench and bills); owner for the open items

## Context
0.3.0's efficiency pass (#793) made voices cheaper, bit for bit, but no bill moved, so admission gave no voice back. The unit read every bench row again on 2026-10-26 (rev V, 480 MHz, main 4eae5d81, DSP-identical to 40f056e8): (private notes).

- **ADR 0211's rule** set each term to its reading, rounded up to 10, pinned by a test. It only raised terms the unit read higher (delay GRAIN, the one lowered, aside).
- **The rows carried the project's routes.** `Rig::time` built Part 1 from the loaded project, so every row that sets only a Sound billed that Part's routes too. At the 0.3 run they were ENV 1 into the VCA, ENV 1 into CUTOFF (the filter's moving path) and one more destination: +134 on CLEAN, +124 on VA's 2040, read 69 or more (1 OP 558 less 1 DEST 489). The rows built with `matrix` don't have them.
- **ALGO's slope reads higher, its floor lower.** 6 OP less 1 OP reads 66 an operator (billed 60) and CHAIN less 6 OP 9.6 a link (8). 1 DEST reads 30 under its bill. A16+17 with the routes off reads about 22 over its bill.

## Decision
1. **A bill follows its reading both ways.** A term moves when the unit reads it 10 or more off. The new value is the most the worst voice of the worst bench row using it allows, rounded to 10. A row's `/VOICE` is its mean voice; VA and STRING cost more as the note climbs, so they are read at the row's top note (its last voice's increment). Per-unit slopes (an operator, a link, a mode) round to the cycle, as ADR 0026's did.
2. **Engines are read with the Part's routes off.** A row that has the project's routes is read as its cycles less 69 (59 on VA's 2040, the conservative end) and its bill less the routes' 134 (124). The bench now clears Part 1's routes before each row, so the next run reads them clean.
3. **Re-billed:**

   | Term | Was | Now | Row read (bill) |
   |---|---|---|---|
   | MODAL `COST_BOWED` | 640 | 530 | MDL BOW+ 644 (761), its worst |
   | VA AM / MAX / SOFT SYNC / PHASE | 540 / 590 / 590 / 650 | 550 / 550 / 610 / 590 | top voice 766 / 769 / 829 / 809, less the routes' 59: 707 / 710 / 770 / 750 over the voice's other 161 |
   | TAPE LOOP under 16 a pass | +300 | +180 | LOOP 64 776, REV 64 771 (floor 596) |
   | TAPE TRON under 16 | +440 | +370 | TRON 958 |
   | TAPE PING-PONG under 16 | +80 | +90 | PING 64 650 (floor 566) |
   | FX SOFT | 100 | 90 | SOFT LO 90 over 1 OP |
   | `FxBus::COST_NO_TAPE` | 1,204 | 1,190 | BUS+V 1,186, its tail gate in it |

4. **Kept, with the reason:**
   - **VA MIX, SYNC, RING, XOR, BITS, FM and MODAL `COST_STRING`.** Their means read 40–50 under, but their top voices read at ADR 0165's counts (MIX 685, FM 741 with the routes off) and STR0's at 11 over 310.
   - **TAPE's 16–59 band** (+80 / +740). LOOP 256 reads 642, but no row reads TRON or the equal-power fade in it.
   - **ALGO** (`COST_BASE`, `COST_OP`, `COST_LINK`) and the formant and NOIS terms. Their readings (OP 67, LINK 10, BASE 350, FRMT linked +30, NOIS +10, ENV BURST +10) shrink the formant budget below what existing Sounds hold (ADR 0026's floor, the owner's three-voice formant floor). `budget.rs` then fails to compile. The heavy rows' gap stays (ADR 0211 point 3).
   - **MODAL bank** (`COST_BANK` 400, `COST_MODE` 45). The unit reads 310 and 17 a mode, so RES would get six voices on rev V, up from three. BANK INIT's eight-note chord at velocity 127 then loses 7.9 dB to the limiter, past ADR 0063's 7 dB (`modal_models_match_the_loudness_reference`). That is the owner's call.
   - **BODY, ENSEMBLE, SYMPATHETIC.** STRING reads BODY 68 and the ensemble 93, but MDL SYM+ (the worst SYMP row) sits within 7 of its bill with the routes off.
   - **Filters, FB, taps, the mod pool, FUZZ, FOLD, LPG**: within 10 of their bills, or not separable from the routes in this run.
   - **TAPE ONESHOT, WAVE, SCRUB, STRETCH, GRAIN, LOOP's floor**: either they read at their bills, or the run had no source for them (`FULL`).
   - **The reverb TYPEs' extras**: HALL and BLOOM read about 100 under, but their rows may take the idle skips (ADR 0214), so they aren't the worst case.
5. **The bus is still billed whole.** Billing only the effects that are awake would mean an admission that knows which effects the new voice wakes, in bank 1's `reserved`. That is not a small change. It stays #793's.

## Alternatives considered
- **Lower ALGO's base alone (to 350).** Light patches gain about 20, but A16+17 with the routes off would read 42 over its bill.
- **Raise ALGO's slope and lower its base to keep the costliest table patch at 832.** The base would have to be 304, and 1 OP would read 40 over.
- **Take the rows as printed (the routes in).** The routes are billed about 59 over what they read, so engines anchored on those rows would be under-billed for Sounds without them.

## Consequences
- **Voices** (INIT routes; rev V, 7,500 less the bus and the control reserve; then with ADR 0211's 2,064 reserve):
  - VA (`va_bills_each_mode`): PHASE one more on rev V and beside ORBIT, MAX one more with the master tape, SOFT SYNC one fewer beside ORBIT; the rest as before.
  - MODAL BOWED: 8 / 8 (was 8 / 7).
  - TAPE STRETCH: 8 / 8 (was 8 / 7).
  - A loop on a source under its floor (a pass under 16 samples): LOOP 7 / 6 (was 6 / 5), TRON 6 / 5 (was 5 / 4).
- **Bank 1:** const values only. The bench change is bank 2's.
- **The next bench run** reads every row with the routes off. Each `/VOICE` against its `B` is then the bill's error with no correction.

## Sources
- (private notes) (`bench-algo.txt`, `bench-trs.txt`, `summary.md`), (private notes), (private notes).
- ADR 0026, 0063, 0165, 0211, 0214; #793.
