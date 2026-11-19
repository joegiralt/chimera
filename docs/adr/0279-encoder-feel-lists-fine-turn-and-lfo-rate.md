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
   encoder's detents and the burst position of the first of them
   (`chimera_hal::Turn`, built by `Turn::at` from a `Detents`, never
   −128, and a `BurstPos`; `Controls::turn`). A burst starts again after
   a 150 ms rest, when SEQ goes down or up, and when the turn reverses
   (`BurstPos::next`), so the plain turn after a fine one starts slow and
   a correction never jumps rows. `encoder_delta` stays a
   parameter's ticks (`Accel::Param`, the old curve, bit for bit). The
   policy is pure and host-tested in `chimera_hal::Accel`: `Param` is 1,
   2, 4, 5, 10 then 10; `List` is 1 for four detents, 2 for four, then 3
   (`Ticks`). `List` turns every list (`list_turn`: SETTINGS, the
   browser's rows, REPLACE), the browser's tabs, and a cell that picks
   from names (`feel::accel`: a `Names` format or an `Enum` spec, SOURCE,
   OP, PAD, ROUTE). Values (levels, times, Hz, MIDI MAP's numbers) keep
   `Param`. A shell without acceleration (the desktop, the console's
   hand, tests) reports `Turn::steady`: its ticks are its detents under
   either curve.
2. **A turn during a SEQ hold cancels it; on a value it is fine.** The
   owner's ruling, unnarrowed: any turn while SEQ is down on a Part's,
   mixer's or FX page cancels SEQ's tap and ORBIT's 500 ms hold, on a
   choice as on a value; a SEQ hold with no turn opens ORBIT at 500 ms as
   before. SETTINGS (RANDOMIZE's roll among them), the browser and ORBIT
   keep SEQ's keys as they were, and so do a card row loading, the
   library at work, an open prompt and learn armed, which take the turn
   themselves. On a cell that takes it, the same turn moves one raw step
   a detent with no acceleration (several detents in one frame each
   step), and a continuous parameter a tenth of its step where pushes
   are built in (`FINE_DIV`, ADR 0140's path; without it, a whole step).
   The fine turn changes the step and nothing else: TAPE MODEL's and DIRECTION's
   refusals on a streamed source, ALGO's budget and TUNING's walk guard it
   as they guard a plain turn. MIX wins (MIX + turn still snaps). A cell
   takes it (`feel::takes`) when it is a value: not a choice (a list turn
   is a row a detent already), not a cell drawn on the staff (TRANSPOSE,
   whose notes fill the band), not one with words of its own that leave
   the hint no room (START/END's onset snap, SCRUB's KEY POSITION); never
   in SETTINGS or ORBIT. The frame's turns are one value
   (`turns::FrameTurns`: each encoder still, plain or fine with its
   ticks, and `SeqTurn`), asked of bank 2 once a frame; SEQ's state
   (`hold::SeqHold::of(SeqKey, SeqTurn)`: up, held, or held through a
   turn) goes to its gate (`HoldGates::step`).
   **Every cell that takes it says so** in its focus band, at rest:
   `SEQ+TURN FINE`, beside `MIX + SEQ LEARN` on a mappable cell and
   beside `MIX + -/+ PAD` on a pad's (`feel::hint`); with PUSH ENCODERS
   on, `PUSH DEFAULT  PUSH/SEQ FINE` (`+TURN` runs past the line). On a mixer cell the band hints the fine
   turn at rest too, and while MIX is held the PART jump (`MIX + B1-6
   PART`) in its place. This replaces the first ruling (the fine turn's
   hint only while SEQ is held): a user who held SEQ to read that hint
   landed in ORBIT at 500 ms, while MIX held alone does nothing, and the
   PART jump starts with MIX, so its hint shows on its first key.
3. **CLASSIC RATE turns a twelfth of an octave a tick**
   (`part_page::edit`, `feel::lfo_rate`, `PerOctave::SEMITONES`): 0.006 Hz a
   detent at 0.1 Hz, 0.06 at 1 Hz, 0.6 at 10 Hz, 132 detents end to end
   (the linear steps took 133). Only the turn changes: the stored Hz, its
   range, its CC/NRPN law, DRUM's SPEED (which reads RATE's range) and
   mod offsets are as before, so every saved Sound loads and sounds the
   same. Its readout shows four places under 0.1 Hz and three under 1 Hz,
   so every detent reads new (`law_text`, bank 2's; closed, two). A push
   or SEQ fine turn on RATE steps the same twelfth.
4. **Bank 2 holds the feel** (ADR 0198: bank 1 is frozen). `ui::feel`
   (the frame's turns: each cell's curve, `takes`, SEQ's cancel; the
   lists' curve; the hint and its words; RATE's semitone nudge and
   readout) links into bank 2's S4 group, reached through one `UiDoor`
   entry whose asks each carry their answer's slot (`feel::Ask`). The
   engine's door table is full with ORBIT, so RATE's tick is the UI's
   edit path's (`feel::nudge`), not `LfoParams::nudge`. Bank 1 keeps the
   board's count, `Turn`, `SeqHold` and the gate's mute, the stubs with
   their closed paths and `Accel::Param` inlined into the board. With bank 2 closed (an install, a bench build) the curve is a
   parameter's everywhere, no cell takes the fine turn, the band is as
   before, and RATE steps linearly.

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
- Bank 1 grows by the board's `Turn` and burst, SEQ's state and gate,
  the band's `Words` and the door's stubs with their closed paths:
  BANK1_LINE. Bank 2 grows by `ui::feel`: BANK2_LINE. One `UiDoor` entry
  (`feel::Ask`): the table's 128 B are 28 words with it.

## Sources
- private issue #1005; tester report joegiralt/chimera-releases#11.
- ADR 0086 (gesture hints), ADR 0140 (push encoders, `FINE_DIV`).
- Elektron Digitakt/Digitone manuals: FUNC + turn jumps a value, a
  pushed turn moves in larger increments; Elektronauts threads quoting
  them ("Key combination to set track length…", "Why does press+turn osc
  tune shift…").
