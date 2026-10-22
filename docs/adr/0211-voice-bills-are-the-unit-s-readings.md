# 0211. Bill every voice term as the unit reads it; TAPE loops by their pass, GRAIN's search once a block

- **Status:** Proposed
- **Deciders:** owner (FUZZ, ALGO, TAPE loops, #777), Claude (bench and bills)

## Context
The 0.2 stress test read every voice term on the owner's PreenFM3 (rev V, 480 MHz, 2026-10-22, DWT counter). Each bench row now prints its reading beside its own `Voice::cost`. Readings are cycles a sample a voice. Several bills were static counts that the unit reads higher:
- FUZZ reads 244 over 1 OP against 175. That breaks ADR 0156's ceiling (271 − 95).
- ALGO's base reads 55–65 over its bill.
- TAPE's cold rows read up to twice their bills: LOOP 892, TRON 1,035.
- Struck on eight windows in one block, STRETCH GRAIN with no PERIOD took 1,009,085 cycles of a 640,000-cycle block, an overrun. A window's own-period search costs about 52,000 cycles (#777).

The readings, the cost table and the voice table are in the stress-0.2 ledger.

## Decision
1. **Bills from the readings.** Every bill the unit reads higher is raised to its reading, rounded up to 10. Each is pinned by a test against its reading.
   - SEM 45 → 10, moving 35 → 50; LPG 85 → 95; FB 100 → 115; STABILITY 3 → 5.
   - SOFT 58 → 100; FOLD 45 → 80; FUZZ 175 → 245.
   - SYMPATHETIC 470 → 645; LOOPFM 350 → 480; NOISE PINK 35 → 40, RED 30 → 35.
   - Delay GRAIN 590 → 320 (it reads less).
   - TAPE: ONESHOT 230 → 345, WAVE 400 → 455, STRETCH CLASSIC 450 → 540, STRETCH GRAIN 570 → 945.
2. **FUZZ bills as it reads, over ADR 0156's ceiling.** The ceiling test goes. A FUZZ Part, meant for mono and bass, gets five voices on VA INIT. This supersedes 0156's ceiling only.
3. **ALGO's base stays 370 for 0.2.** The 55–65 gap is left to 0.3.0's efficiency pass (#793), which makes ALGO cheaper rather than billing it more.
4. **TAPE LOOP, TRON and PING-PONG bill their loop by its pass.** A pass's length is in output samples (`spw`: the loop's frames over `inc` at the fastest its note reaches, `ratio_max`). `heads::loop_extra` bills the band over the model's floor; `COST_LOOP` and `COST_TRON` are 520, `COST_PING` 490.
   - LOOP and TRON, in output samples a pass:

     | Band | Summing fade | Equal-power fade (20 ms or more, `POWER_FADE_S`) |
     |---|---|---|
     | 4,096 and up | +0 | +10 |
     | 1,024–4,095 | +0 | +20 |
     | 256–1,023 | +0 | +50 |
     | 60–255 | +20 | +200 |
     | 16–59 | +80 | +740 |
     | Under 16 | LOOP +300, TRON +440 | — |

   - The model behind the bands: a pass costs `512 + 1,216 / spw`, fitted to the unit's 816 every 4 samples and 588 every 16. An equal-power fade adds 10 a sample (`xgains`' 8 flops) and 10,500 a pass. The pass term is fitted to the 40 ms loop's 699, a pass every 62 samples. The unit read no loop longer than 62 samples a pass, so the 16–59 band's equal-power figure is that model's, not a reading.

   - PING-PONG: +80 under 16, else +0.
   - A pass under 16 samples only happens when the source is shorter than the loop's floor.
   - Admission books the band at note-on, through `AdmitDoor::loop_extra` (bank 2), with the source's frames and rate from the gate. The voice holds it in every block's re-bill.
   - A route on SIZE, START, END or XFADE bills the shortest loop, faded equal-power.
5. **One own-period search a block.** `period::of_window` runs at most one new window's search an audio block. A GRAIN note's lock waits for its first render (no grain starts before it, so one note plays as before). A note past the block's one search plays unlocked, as a source that doesn't repeat does, until a later block has one. The block's start is a GRAIN voice rendering a second time under the same block count (`period::rendering`). The PERIOD-set path and CLASSIC don't change, bit for bit.

## Alternatives considered
- **Bill the TAPE models at their worst row (820, 960).** True for a 64-frame source only. It cost the nine factory LOOP Sounds four voices each.
- **Bill GRAIN's search as a one-block reserve.** A bill can't hold a spike past the block, so eight searches still overrun.
- **Raise ALGO's base now.** It trips the formant floor, ORBIT's six voices and the project loads' budget. The owner chose to make ALGO cheaper in 0.3.0.

## Consequences
- **Voices:**
  - FUZZ Parts: five.
  - Factory LOOP Sounds: eight, a pass every 7,000 samples or more at their pitches. That matches the old static bill; the worst-row bill gave five.
  - SYMPATHETIC: one fewer. GRAIN delay Parts: about a third of a voice back.
- **Lag:** a GRAIN chord on eight windows locks over eight blocks (11 ms).
- **SIZE knob:** a knob turned to a tiny SIZE under a held note isn't re-banded. It runs in the headroom, as a UI move does (ADR 0061).
- **Bank 1:** the per-voice `looped` term, the door's entry and its wrapper, measured in `tools/bank1-budget.txt`.

## Sources
- stress-0.2 ledger: `bench-ar.txt`, `bench-trs.txt`, `cost-table.md`, `voice-table.md`.
- Issues #642, #726, #735, #746, #771, #777, #793.
- PR #792.
