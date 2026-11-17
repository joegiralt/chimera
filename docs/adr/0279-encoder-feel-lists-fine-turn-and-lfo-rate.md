# 0279. Turn lists gentler, step fine with SEQ + turn, and turn CLASSIC RATE by the semitone

- **Status:** Proposed
- **Deciders:** owner (approved the three changes in #1005), Claude (curves and keys)

## Context
A tester (joegiralt/chimera-releases#11, filed as private issue #1005)
found menus and lists overshooting, no way to make a small change on a
unit without push encoders, and CLASSIC LFO RATE too coarse at the slow
end. The board applied one acceleration curve to every encoder (1, 2, 4,
5, 10 ticks per detent within a 150 ms burst) before the UI knew what the
encoder was turning. The fine turn (ADR 0140) needed PUSH ENCODERS and a
v1.2 control board; the owner's unit has none, and a push is only ever a
shortcut. CLASSIC RATE stepped 0.15 Hz from 0.01 Hz: 0.01, 0.16, 0.31…,
so 0.01–1 Hz was seven detents.

## Decision
1. **The board counts, the UI accelerates.** The board reports each
   encoder's raw detents and the burst position of the first of them
   (`chimera_hal::Turn { detents, burst: Option<u8> }`, `Controls::turn`);
   `encoder_delta` stays a parameter's ticks (`Accel::Param`, the old
   curve, bit for bit). The policy is pure and host-tested in
   `chimera_hal::Accel`: `Param` is 1, 2, 4, 5, 10 then 10; `List` is 1
   for four detents, 2 for four, then 3. `List` turns every list
   (`list_turn`: SETTINGS, the browser's rows, REPLACE), the browser's
   tabs, and a cell that picks from names (`View::accel`: a `Names`
   format or an `Enum` spec, SOURCE, OP, PAD, ROUTE). Values (levels,
   times, Hz, MIDI MAP's numbers) keep `Param`. A shell without
   acceleration (the desktop, the console's hand, tests) reports
   `burst: None`: its ticks are its detents under either curve.
2. **SEQ + turn is fine.** With SEQ held, a turn on a Part or FX page moves
   one raw step a detent with no acceleration, and a continuous parameter
   a tenth of its step where pushes are built in (`FINE_DIV`, ADR 0140's
   path; without it, a whole step). MIX wins (MIX + turn still snaps). A
   turn while SEQ is down mutes SEQ's gate for that hold
   (`HoldGates::step`): its release is no tap and ORBIT's 500 ms hold
   never fires; a SEQ hold with no turn opens ORBIT at 500 ms as before
   (owner's ruling). The touched value cell's focus band says
   `SEQ + TURN FINE` (on a mappable cell `SEQ+TURN FINE` then `MIX+SEQ LEARN`, unspaced to fit the line,
   `view::fine_hint`); PUSH ENCODERS' hint still replaces it when on. On a
   mixer cell, where the PART jump fills the line, the band says
   `SEQ + TURN FINE` in its place while SEQ is held (owner's ruling), so
   every hidden gesture keeps a hint. The fine turn and its hint are a
   Part's and its mixer's (`View::fine_turns`): not SETTINGS' leaves or
   ORBIT's pages, and not a cell drawn on the staff (TRANSPOSE), whose
   notes fill the hint's line; there SEQ + turn is a plain turn.
3. **CLASSIC RATE turns a twelfth of an octave a tick**
   (`LfoParams::nudge`, `RATE_TICK`): 0.006 Hz a detent at 0.1 Hz, 0.06
   at 1 Hz, 0.6 at 10 Hz, 132 detents end to end (the linear steps took
   133). Only the turn changes: the stored Hz, its range, its linear
   readout and CC/NRPN law, DRUM's SPEED (which reads RATE's range) and
   mod offsets are as before, so every saved Sound loads and sounds the
   same. RATE is its own ladder (`Block::ladder`): a push turn steps the
   same twelfth.

## Alternatives considered
- **Keys for the fine turn.** MIX + turn is the landmark snap (ADR 0086,
  as Elektron's FUNC + turn jumps a value). EDIT, PLUS, MINUS and B*n* act
  on their press, so a hold-and-turn would already have moved the page.
  MENU's hold is a quick save: a slow start to a fine turn would save the
  project. SEQ acts only on release (tap) or at 500 ms (hold): a turn
  started before the hold mutes it cleanly, as Elektron's hold-a-trig-
  and-turn does. Cost: a SEQ held still for 500 ms before any turn opens
  ORBIT, as it always has.
- **Turn slowly for fine (Elektron's own fine).** Already true for a
  slow turn (the first detent of a burst is 1 tick), but that tick is a
  whole step; the tester asked for less than one.
- **One curve chosen by the board per page.** The board would need to
  know the UI's cells; the UI picks per cell instead, and the board
  stays a counter.
- **CLASSIC RATE stored in octaves (`Scale::Octaves`).** Moves its CC
  law, its knob bar and DRUM's SPEED (which denormalizes through RATE's
  spec), and changes saved DRUM kits' sound; a turn-only taper does none
  of that.

## Consequences
- Lists of 128 rows take about 46 fast detents end to end instead of 15;
  the tester's overshoot is gone, a long list is slower to cross.
- The fine turn reaches every Part and FX value without a push; a SEQ
  tap with a turn in it no longer moves up a page.
- Bank 1 grows by the board's turn, the UI's curve choice, SEQ's fine
  step and hint, and RATE's tick: measured against 92df1b18, +944 B on
  the default build, +960 B on ORBIT, +728 to +1232 B on the bench sets;
  every set's line in `tools/bank1-budget.txt` is raised by its delta
  citing this ADR. Bank 2 is unchanged (ORBIT −32 B).

## Sources
- private issue #1005; tester report joegiralt/chimera-releases#11.
- ADR 0086 (gesture hints), ADR 0140 (push encoders, `FINE_DIV`).
- Elektron Digitakt/Digitone manuals: FUNC + turn jumps a value, a
  pushed turn moves in larger increments; Elektronauts threads quoting
  them ("Key combination to set track length…", "Why does press+turn osc
  tune shift…").
